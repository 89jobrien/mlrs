use serde::{Deserialize, Serialize};

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
