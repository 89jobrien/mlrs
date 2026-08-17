use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Truncation policy
// ---------------------------------------------------------------------------

/// How to shorten a cell output that exceeds `max_bytes`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TruncationStrategy {
    /// Keep the first `max_bytes`, append a marker.
    TailDrop,
    /// Keep the last `max_bytes`, prepend a marker.
    HeadDrop,
    /// Keep the first and last halves, replace the middle with a marker.
    Middle,
}

/// Controls whether and how cell outputs are shortened before being stored.
#[derive(Debug, Clone)]
pub struct TruncationPolicy {
    /// Maximum byte length of a stored cell output. `0` means unlimited.
    pub max_bytes: usize,
    pub strategy: TruncationStrategy,
}

impl TruncationPolicy {
    /// Default: 8 KiB cap, TailDrop.
    pub fn default_policy() -> Self {
        Self {
            max_bytes: 8192,
            strategy: TruncationStrategy::TailDrop,
        }
    }

    /// No truncation — stores output verbatim.
    pub fn none() -> Self {
        Self {
            max_bytes: 0,
            strategy: TruncationStrategy::TailDrop,
        }
    }
}

impl Default for TruncationPolicy {
    fn default() -> Self {
        Self::default_policy()
    }
}

/// Truncate `text` according to `policy`. Returns the original string
/// unchanged if it fits within the limit or if `max_bytes == 0`.
pub fn truncate_output(text: &str, policy: &TruncationPolicy) -> String {
    let limit = policy.max_bytes;
    if limit == 0 || text.len() <= limit {
        return text.to_string();
    }
    let omitted = text.len() - limit;
    match policy.strategy {
        TruncationStrategy::TailDrop => {
            // Find a valid UTF-8 boundary at or before `limit`.
            let cut = floor_char_boundary(text, limit);
            format!("{}\n[{omitted} bytes truncated]", &text[..cut])
        }
        TruncationStrategy::HeadDrop => {
            let start = text.len() - limit;
            let cut = ceil_char_boundary(text, start);
            format!("[{omitted} bytes truncated]\n{}", &text[cut..])
        }
        TruncationStrategy::Middle => {
            let half = limit / 2;
            let head_cut = floor_char_boundary(text, half);
            let tail_start = ceil_char_boundary(text, text.len() - half);
            let mid_bytes = tail_start - head_cut;
            format!(
                "{}\n[{mid_bytes} bytes omitted]\n{}",
                &text[..head_cut],
                &text[tail_start..]
            )
        }
    }
}

/// Largest index ≤ `index` that is a valid UTF-8 char boundary.
fn floor_char_boundary(s: &str, index: usize) -> usize {
    let index = index.min(s.len());
    (0..=index)
        .rev()
        .find(|&i| s.is_char_boundary(i))
        .unwrap_or(0)
}

/// Smallest index ≥ `index` that is a valid UTF-8 char boundary.
fn ceil_char_boundary(s: &str, index: usize) -> usize {
    let index = index.min(s.len());
    (index..=s.len())
        .find(|&i| s.is_char_boundary(i))
        .unwrap_or(s.len())
}

// ---------------------------------------------------------------------------
// Token estimation
// ---------------------------------------------------------------------------

/// Character-based token estimate (~4 bytes per token, rounded up).
///
/// Deliberately crude: it only needs to be accurate enough to trigger
/// notebook compaction well before the provider's context window overflows,
/// without pulling in a tokenizer dependency.
pub fn approx_token_count(text: &str) -> usize {
    text.len().div_ceil(4)
}

/// The result of executing one Rhai cell in the RLM loop.
#[derive(Debug, Clone)]
pub enum StepResult {
    /// Script ran, produced output — loop continues with this output appended to the notebook.
    Continue(String),
    /// Model called `final(answer)` — loop exits, answer is returned to caller.
    Final(String),
}

/// A single executed cell: the script the model wrote and its output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cell {
    pub script: String,
    pub output: String,
}

/// Accumulated history of all cells executed in one RLM run.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Notebook {
    pub cells: Vec<Cell>,
}

impl Notebook {
    pub fn push(&mut self, script: String, output: String) {
        self.cells.push(Cell { script, output });
    }

    /// Estimated token footprint of the whole notebook (scripts + outputs).
    pub fn token_estimate(&self) -> usize {
        self.cells
            .iter()
            .map(|c| approx_token_count(&c.script) + approx_token_count(&c.output))
            .sum()
    }

    /// Render cells as a message history string for the LLM.
    pub fn as_history(&self) -> String {
        self.cells
            .iter()
            .enumerate()
            .map(|(i, c)| {
                format!(
                    "# Cell {}\n```\n{}\n```\nOutput:\n{}",
                    i, c.script, c.output
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- truncation ---

    #[test]
    fn truncate_noop_when_under_limit() {
        let p = TruncationPolicy {
            max_bytes: 100,
            strategy: TruncationStrategy::TailDrop,
        };
        assert_eq!(truncate_output("hello", &p), "hello");
    }

    #[test]
    fn truncate_noop_when_limit_zero() {
        let p = TruncationPolicy::none();
        let big = "x".repeat(100_000);
        assert_eq!(truncate_output(&big, &p), big);
    }

    #[test]
    fn truncate_tail_drop() {
        let p = TruncationPolicy {
            max_bytes: 5,
            strategy: TruncationStrategy::TailDrop,
        };
        let out = truncate_output("hello world", &p);
        assert!(out.starts_with("hello"), "got: {out}");
        assert!(out.contains("truncated"), "got: {out}");
    }

    #[test]
    fn truncate_head_drop() {
        let p = TruncationPolicy {
            max_bytes: 5,
            strategy: TruncationStrategy::HeadDrop,
        };
        let out = truncate_output("hello world", &p);
        assert!(out.ends_with("world"), "got: {out}");
        assert!(out.contains("truncated"), "got: {out}");
    }

    #[test]
    fn truncate_middle() {
        let p = TruncationPolicy {
            max_bytes: 6,
            strategy: TruncationStrategy::Middle,
        };
        let out = truncate_output("abcdefghijkl", &p);
        assert!(out.starts_with("abc"), "got: {out}");
        assert!(out.ends_with("jkl"), "got: {out}");
        assert!(out.contains("omitted"), "got: {out}");
    }

    #[test]
    fn truncate_respects_utf8_boundary() {
        // "é" is 2 bytes — truncating at 3 bytes must not split a multi-byte char.
        let s = "aéb"; // 4 bytes: a(1) + é(2) + b(1)
        let p = TruncationPolicy {
            max_bytes: 2,
            strategy: TruncationStrategy::TailDrop,
        };
        let out = truncate_output(s, &p);
        // floor_char_boundary(4, 2) = 1 (only 'a' fits cleanly)
        assert!(out.starts_with('a'), "got: {out}");
        assert!(!out.contains('\u{FFFD}'), "got replacement char: {out}");
    }

    // --- token estimation ---

    #[test]
    fn approx_token_count_rounds_up() {
        assert_eq!(approx_token_count(""), 0);
        assert_eq!(approx_token_count("abc"), 1);
        assert_eq!(approx_token_count("abcd"), 1);
        assert_eq!(approx_token_count("abcde"), 2);
    }

    #[test]
    fn notebook_token_estimate_sums_scripts_and_outputs() {
        let mut nb = Notebook::default();
        assert_eq!(nb.token_estimate(), 0);
        nb.push("abcd".into(), "efgh".into()); // 1 + 1
        nb.push("x".into(), "y".into()); // 1 + 1
        assert_eq!(nb.token_estimate(), 4);
    }

    // --- notebook ---

    #[test]
    fn notebook_push_and_history() {
        let mut nb = Notebook::default();
        assert!(nb.as_history().is_empty());
        nb.push("ctx_len()".into(), "42".into());
        let h = nb.as_history();
        assert!(h.contains("Cell 0"));
        assert!(h.contains("ctx_len()"));
        assert!(h.contains("42"));
    }

    #[test]
    fn step_result_variants() {
        let c = StepResult::Continue("out".into());
        let f = StepResult::Final("answer".into());
        assert!(matches!(c, StepResult::Continue(_)));
        assert!(matches!(f, StepResult::Final(_)));
    }
}
