---
title: Context Window Management
status: done
priority: high
---

# Context Window Management

## Problem

mlrs has no token counting, no history summarization, and no truncation policy. At
`max_iterations=20` with large contexts, the accumulated notebook history will silently
exceed the provider's context window, producing a provider error with no recovery path.

## Deliverables

- Token estimation utility (`approx_token_count(text: &str) -> usize`) — character-based
  heuristic (~4 chars/token) sufficient for triggering compaction; no hard dependency on a
  tokenizer crate.
- `Notebook::token_estimate(&self) -> usize` — sum over all cells.
- Compaction trigger in `Rlm::run`: when estimated tokens exceed a configurable threshold
  (`compaction_threshold`, default 80% of a model's known context window), run a
  summarization pass before the next iteration.
- Summarization pass: send the current notebook to the provider with a fixed
  `COMPACT_PROMPT` asking for a concise summary, replace notebook cells with a single
  synthetic cell whose output is the summary.
- `Rlm::with_compaction_threshold(usize)` builder method.
- Per-cell output truncation: if a single cell output exceeds `max_cell_output_bytes`
  (default 8 KB), truncate with a `[truncated — N bytes omitted]` suffix before storing.

## Design notes

- Keep compaction in `mlrs-core` — it is engine logic, not provider logic.
- Summarization reuses the existing `LlmProvider::complete` — no new trait method needed.
- Compact prompt lives in `crates/mlrs-core/src/templates/compact_prompt.md`.
- The summarization call does NOT count against `max_iterations`.
- Expose `compaction_triggered: bool` in a future step-event type (plan 04).

## Delivered (2026-08-17)

- `approx_token_count` + `Notebook::token_estimate` in mlrs-core.
- Compaction pass in `Rlm::run` via `COMPACT_PROMPT` (templates/compact_prompt.md);
  does not count against max_iterations; respects cancellation; requires >= 2 cells
  so a compacted single-cell notebook never re-compacts.
- `with_compaction_threshold`; default is the fixed constant
  `DEFAULT_COMPACTION_THRESHOLD` (102,400 = 80% of 128k) rather than a per-model
  window registry — no such registry exists yet.
- Per-cell truncation was delivered separately by plan 07.
