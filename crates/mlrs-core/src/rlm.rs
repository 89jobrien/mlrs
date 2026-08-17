use anyhow::Result;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{debug, warn};

use crate::{
    env::{build_engine, execute_script},
    error::RlmError,
    protocol::{truncate_output, Notebook, StepResult, TruncationPolicy},
};

/// The LLM provider abstraction — implemented in `mlrs-providers`.
#[async_trait::async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, messages: Vec<Message>) -> Result<String>;
    fn model_id(&self) -> &str;
}

#[derive(Debug, Clone)]
pub struct Message {
    pub role: Role,
    pub content: String,
}

#[derive(Debug, Clone)]
pub enum Role {
    System,
    User,
    Assistant,
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Role::System => write!(f, "system"),
            Role::User => write!(f, "user"),
            Role::Assistant => write!(f, "assistant"),
        }
    }
}

const SYSTEM_PROMPT: &str = r#"You are an RLM (Recursive Language Model) agent. You never see the
full context directly. Instead, you write Rhai scripts that interact with the context via
registered functions, and the results are returned to you as cell outputs.

Available Rhai functions:
- ctx_len() -> int           — byte length of the full context
- ctx_slice(start, end) -> String  — byte-range slice of context
- ctx_grep(pattern) -> String      — regex search; returns matching lines
- print_cell(msg)            — append a message to this cell's output log
- done(answer)               — signal that you have the final answer; exits the loop

Rules:
- Write ONE Rhai script per response. No prose before or after the script.
- To exit, call done("your answer here") in the script.
- If a script errors, you will see the error as cell output — self-correct and try again.
- You may call rlm_call(query, ctx_fragment) to recursively process a sub-context.
"#;

/// Prompt for the notebook-compaction summarization pass.
const COMPACT_PROMPT: &str = include_str!("templates/compact_prompt.md");

/// Default compaction trigger: 80% of a 128k-token context window, the
/// smallest window among the providers mlrs targets.
pub const DEFAULT_COMPACTION_THRESHOLD: usize = 102_400;

/// Core RLM engine.
pub struct Rlm {
    provider: Arc<dyn LlmProvider>,
    pub depth: usize,
    pub max_depth: usize,
    pub max_iterations: usize,
    pub max_retries_per_cell: usize,
    pub verbose: bool,
    pub truncation: TruncationPolicy,
    /// Estimated-token threshold above which the notebook is summarized
    /// into a single synthetic cell before the next iteration.
    pub compaction_threshold: usize,
}

impl Rlm {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            provider,
            depth: 0,
            max_depth: 5,
            max_iterations: 20,
            max_retries_per_cell: 3,
            verbose: false,
            truncation: TruncationPolicy::default(),
            compaction_threshold: DEFAULT_COMPACTION_THRESHOLD,
        }
    }

    pub fn with_depth(mut self, depth: usize) -> Self {
        self.depth = depth;
        self
    }

    pub fn with_max_depth(mut self, max_depth: usize) -> Self {
        self.max_depth = max_depth;
        self
    }

    pub fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    pub fn with_truncation_policy(mut self, policy: TruncationPolicy) -> Self {
        self.truncation = policy;
        self
    }

    /// Set the estimated-token threshold that triggers notebook compaction.
    pub fn with_compaction_threshold(mut self, threshold: usize) -> Self {
        self.compaction_threshold = threshold;
        self
    }

    /// Run the RLM loop: execute until `done()` is called or limits are hit.
    ///
    /// Pass `CancellationToken::new()` if you don't need cancellation.
    /// The token is checked between iterations; an in-flight provider call is not
    /// interrupted mid-request.
    pub async fn run(
        &self,
        query: &str,
        context: &str,
        cancel: CancellationToken,
    ) -> Result<String, RlmError> {
        if self.depth >= self.max_depth {
            return Err(RlmError::MaxDepthExceeded(self.depth));
        }

        let (engine, mut scope) = build_engine(context.to_string());
        let mut notebook = Notebook::default();
        let mut iterations = 0;
        let mut consecutive_errors: usize = 0;

        loop {
            if cancel.is_cancelled() {
                return Err(RlmError::Cancelled);
            }

            // Compact the notebook when its estimated token footprint would
            // crowd out the context window. Requires ≥2 cells so an
            // already-compacted (single-cell) notebook is never re-compacted
            // in a loop. Does not count against max_iterations.
            if notebook.cells.len() >= 2 && notebook.token_estimate() > self.compaction_threshold {
                self.compact(&mut notebook, &cancel).await?;
            }

            if iterations >= self.max_iterations {
                return Err(RlmError::MaxIterationsExceeded(self.max_iterations));
            }
            iterations += 1;

            let messages = self.build_messages(query, &notebook);
            let script = tokio::select! {
                result = self.provider.complete(messages) => {
                    result.map_err(RlmError::ProviderError)?
                }
                _ = cancel.cancelled() => {
                    return Err(RlmError::Cancelled);
                }
            };

            let script = extract_script(&script);

            if self.verbose {
                eprintln!("[depth={}] >> {}", self.depth, script);
            }

            debug!(
                depth = self.depth,
                iteration = iterations,
                "rlm: executing cell"
            );

            let step = execute_script(&engine, &mut scope, script)
                .map_err(|e| RlmError::ScriptError(e.to_string()))?;

            if self.verbose {
                eprintln!(
                    "[depth={}] << {}",
                    self.depth,
                    match &step {
                        StepResult::Continue(o) => o.as_str(),
                        StepResult::Final(a) => a.as_str(),
                    }
                );
            }

            match step {
                StepResult::Final(answer) => {
                    debug!(depth = self.depth, "rlm: final answer received");
                    return Ok(answer);
                }
                StepResult::Continue(ref output) if output.starts_with("script error:") => {
                    consecutive_errors += 1;
                    warn!(
                        depth = self.depth,
                        iteration = iterations,
                        consecutive_errors,
                        max = self.max_retries_per_cell,
                        error = %output,
                        "rlm: script error"
                    );
                    if consecutive_errors > self.max_retries_per_cell {
                        return Err(RlmError::MaxRetriesExceeded(self.max_retries_per_cell));
                    }
                    notebook.push(
                        script.to_string(),
                        truncate_output(output, &self.truncation),
                    );
                }
                StepResult::Continue(output) => {
                    consecutive_errors = 0;
                    notebook.push(
                        script.to_string(),
                        truncate_output(&output, &self.truncation),
                    );
                }
            }
        }
    }

    /// Summarize the notebook into a single synthetic cell via the provider.
    ///
    /// Reuses `LlmProvider::complete` — the summarization request is a
    /// normal completion with `COMPACT_PROMPT` as the system message and the
    /// rendered notebook history as the user message. Respects cancellation
    /// the same way the main loop does.
    async fn compact(
        &self,
        notebook: &mut Notebook,
        cancel: &CancellationToken,
    ) -> Result<(), RlmError> {
        let estimate = notebook.token_estimate();
        let messages = vec![
            Message {
                role: Role::System,
                content: COMPACT_PROMPT.to_string(),
            },
            Message {
                role: Role::User,
                content: notebook.as_history(),
            },
        ];

        let summary = tokio::select! {
            result = self.provider.complete(messages) => {
                result.map_err(RlmError::ProviderError)?
            }
            _ = cancel.cancelled() => {
                return Err(RlmError::Cancelled);
            }
        };

        let dropped = notebook.cells.len();
        debug!(
            depth = self.depth,
            cells = dropped,
            token_estimate = estimate,
            threshold = self.compaction_threshold,
            "rlm: compacted notebook"
        );
        if self.verbose {
            eprintln!(
                "[depth={}] ~~ compacted {dropped} cells (~{estimate} tokens)",
                self.depth
            );
        }

        notebook.cells.clear();
        notebook.push(
            "// [notebook compacted]".to_string(),
            format!("[Summary of {dropped} earlier cells]\n{summary}"),
        );
        Ok(())
    }

    fn build_messages(&self, query: &str, notebook: &Notebook) -> Vec<Message> {
        let mut messages = vec![
            Message {
                role: Role::System,
                content: SYSTEM_PROMPT.to_string(),
            },
            Message {
                role: Role::User,
                content: format!("Query: {query}"),
            },
        ];

        for cell in &notebook.cells {
            messages.push(Message {
                role: Role::Assistant,
                content: cell.script.clone(),
            });
            messages.push(Message {
                role: Role::User,
                content: cell.output.clone(),
            });
        }

        if !notebook.cells.is_empty() {
            messages.push(Message {
                role: Role::User,
                content:
                    "Continue. Write the next Rhai script or call done() if you have the answer."
                        .to_string(),
            });
        }

        messages
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// A provider that never resolves — useful for testing cancellation.
    struct HangingProvider;

    #[async_trait::async_trait]
    impl LlmProvider for HangingProvider {
        async fn complete(&self, _messages: Vec<Message>) -> anyhow::Result<String> {
            // Hang forever.
            futures::future::pending::<()>().await;
            unreachable!()
        }
        fn model_id(&self) -> &str {
            "hanging"
        }
    }

    /// A provider that immediately returns a `done()` script.
    struct DoneProvider;

    #[async_trait::async_trait]
    impl LlmProvider for DoneProvider {
        async fn complete(&self, _messages: Vec<Message>) -> anyhow::Result<String> {
            Ok(r#"done("the answer")"#.to_string())
        }
        fn model_id(&self) -> &str {
            "done"
        }
    }

    #[tokio::test]
    async fn cancellation_before_first_iteration() {
        let rlm = Rlm::new(Arc::new(HangingProvider));
        let cancel = CancellationToken::new();
        cancel.cancel();
        let err = rlm.run("q", "", cancel).await.unwrap_err();
        assert!(
            matches!(err, RlmError::Cancelled),
            "expected Cancelled, got {err:?}"
        );
    }

    #[tokio::test]
    async fn cancellation_during_provider_call() {
        let rlm = Rlm::new(Arc::new(HangingProvider));
        let cancel = CancellationToken::new();
        let cancel2 = cancel.clone();
        // Cancel after a short delay so the select! fires.
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            cancel2.cancel();
        });
        let err = rlm.run("q", "", cancel).await.unwrap_err();
        assert!(
            matches!(err, RlmError::Cancelled),
            "expected Cancelled, got {err:?}"
        );
    }

    #[tokio::test]
    async fn no_cancellation_completes_normally() {
        let rlm = Rlm::new(Arc::new(DoneProvider));
        let answer = rlm.run("q", "", CancellationToken::new()).await.unwrap();
        assert_eq!(answer, "the answer");
    }

    /// Replays a fixed list of responses and records every request's
    /// system-message head, so tests can see whether a compaction request
    /// (COMPACT_PROMPT) was issued and in what order.
    struct ScriptedProvider {
        responses: Vec<&'static str>,
        calls: std::sync::Mutex<Vec<String>>,
    }

    impl ScriptedProvider {
        fn new(responses: Vec<&'static str>) -> Self {
            Self {
                responses,
                calls: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl LlmProvider for ScriptedProvider {
        async fn complete(&self, messages: Vec<Message>) -> anyhow::Result<String> {
            let mut calls = self.calls.lock().unwrap();
            let idx = calls.len();
            calls.push(messages[0].content.clone());
            Ok(self
                .responses
                .get(idx)
                .unwrap_or(&r#"done("fallback")"#)
                .to_string())
        }
        fn model_id(&self) -> &str {
            "scripted"
        }
    }

    #[tokio::test]
    async fn compaction_triggers_above_threshold_and_replaces_history() {
        let provider = Arc::new(ScriptedProvider::new(vec![
            r#""cell-one-output""#, // cell 1
            r#""cell-two-output""#, // cell 2
            "COMPACT-SUMMARY",      // compaction pass (not an iteration)
            r#"done("fin")"#,       // final
        ]));
        let rlm =
            Rlm::new(Arc::clone(&provider) as Arc<dyn LlmProvider>).with_compaction_threshold(1); // force compaction once ≥2 cells exist
        let answer = rlm.run("q", "", CancellationToken::new()).await.unwrap();
        assert_eq!(answer, "fin");

        let calls = provider.calls.lock().unwrap();
        assert_eq!(calls.len(), 4, "expected 4 provider calls, got {calls:?}");
        // Calls 1, 2, 4 are normal loop turns; call 3 is the compaction pass.
        assert!(
            calls[2].starts_with(COMPACT_PROMPT.lines().next().unwrap()),
            "third call should be the compaction request, got: {}",
            &calls[2][..calls[2].len().min(80)],
        );
        assert_eq!(calls[3], SYSTEM_PROMPT, "loop resumes with normal prompt");
    }

    #[tokio::test]
    async fn no_compaction_below_threshold() {
        let provider = Arc::new(ScriptedProvider::new(vec![r#""small""#, r#"done("fin")"#]));
        let rlm = Rlm::new(Arc::clone(&provider) as Arc<dyn LlmProvider>);
        let answer = rlm.run("q", "", CancellationToken::new()).await.unwrap();
        assert_eq!(answer, "fin");
        let calls = provider.calls.lock().unwrap();
        assert!(
            calls.iter().all(|c| c == SYSTEM_PROMPT),
            "no compaction request expected, got {calls:?}",
        );
    }

    #[tokio::test]
    async fn single_cell_notebook_is_never_compacted() {
        // Threshold of 0 would trip on any content, but a 1-cell notebook
        // must not be re-compacted — that would loop on its own summary.
        let provider = Arc::new(ScriptedProvider::new(vec![
            r#""only-cell""#,
            r#"done("fin")"#,
        ]));
        let rlm =
            Rlm::new(Arc::clone(&provider) as Arc<dyn LlmProvider>).with_compaction_threshold(0);
        let answer = rlm.run("q", "", CancellationToken::new()).await.unwrap();
        assert_eq!(answer, "fin");
        let calls = provider.calls.lock().unwrap();
        assert_eq!(calls.len(), 2, "no compaction call expected, got {calls:?}");
    }
}

/// Strip markdown code fences if the model wraps the script.
fn extract_script(raw: &str) -> &str {
    let trimmed = raw.trim();
    if let Some(inner) = trimmed.strip_prefix("```rhai") {
        inner.trim_end_matches("```").trim()
    } else if let Some(inner) = trimmed.strip_prefix("```") {
        inner.trim_end_matches("```").trim()
    } else {
        trimmed
    }
}
