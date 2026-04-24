use anyhow::Result;
use std::sync::Arc;
use tracing::{debug, warn};

use crate::{
    env::{build_engine, execute_script},
    error::RlmError,
    protocol::{Notebook, StepResult},
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

/// Core RLM engine.
pub struct Rlm {
    provider: Arc<dyn LlmProvider>,
    pub depth: usize,
    pub max_depth: usize,
    pub max_iterations: usize,
    pub max_retries_per_cell: usize,
    pub verbose: bool,
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

    /// Run the RLM loop: execute until `done()` is called or limits are hit.
    pub async fn run(&self, query: &str, context: &str) -> Result<String, RlmError> {
        if self.depth >= self.max_depth {
            return Err(RlmError::MaxDepthExceeded(self.depth));
        }

        let (engine, mut scope) = build_engine(context.to_string());
        let mut notebook = Notebook::default();
        let mut iterations = 0;

        loop {
            if iterations >= self.max_iterations {
                return Err(RlmError::MaxIterationsExceeded(self.max_iterations));
            }
            iterations += 1;

            let messages = self.build_messages(query, &notebook);
            let script = self
                .provider
                .complete(messages)
                .await
                .map_err(RlmError::ProviderError)?;

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
                    warn!(depth = self.depth, iteration = iterations, error = %output, "rlm: script error, model will self-correct");
                    notebook.push(script.to_string(), output.clone());
                }
                StepResult::Continue(output) => {
                    notebook.push(script.to_string(), output);
                }
            }
        }
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
