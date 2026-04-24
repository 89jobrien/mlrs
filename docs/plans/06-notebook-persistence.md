---
title: Notebook Persistence and Session Resumption
status: open
priority: medium
---

# Notebook Persistence and Session Resumption

## Problem

Notebooks are in-memory only. A crash, timeout, or REPL exit loses all accumulated cell
history. There is no way to inspect a past run, resume a partial run, or share a run
trace for debugging.

## Deliverables

- `Notebook` already derives `Serialize`/`Deserialize`. Add:
  - `Notebook::save(&self, path: &Path) -> Result<()>` — writes JSON to path.
  - `Notebook::load(path: &Path) -> Result<Self>` — reads JSON from path.

- `RunRecord` type wrapping a completed (or interrupted) run:

  ```rust
  pub struct RunRecord {
      pub query: String,
      pub context_hash: String,   // sha256 of context, not the full text
      pub notebook: Notebook,
      pub answer: Option<String>, // None if interrupted
      pub started_at: u64,        // unix seconds
      pub finished_at: Option<u64>,
  }
  ```

- `Rlm::run_recorded(query, context, path)` — wraps `run`, writes a `RunRecord` to
  `path` on completion or interruption.

- REPL:
  - `/status` already exists — extend it to show the path of the last saved record if
    any.
  - `/load <path>` command — loads a `RunRecord`, restores the notebook into the current
    `Rlm` instance, and prints a summary of cells loaded.
  - `/save <path>` command — saves the current in-progress notebook.

- CLI: `--record <path>` flag on `query` subcommand.

## Design notes

- `context_hash` rather than full context avoids doubling storage for large inputs.
- `RunRecord` is in `mlrs-core::protocol` — it is a protocol type.
- File format is newline-delimited JSON (one record per file) when multiple runs are
  appended, or a single JSON object for single-run files. Use single-object for now.
