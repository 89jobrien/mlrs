---
title: Structured Step Events
status: open
priority: medium
---

# Structured Step Events

## Problem

mlrs has no way for callers to observe what is happening inside a run. The only
observability is `verbose: bool`, which writes to stderr. The REPL cannot show a spinner,
per-cell status, token counts, or compaction notices without invasive hacking.

## Deliverables

- `StepEvent` enum in `mlrs-core::protocol`:

  ```rust
  pub enum StepEvent {
      ScriptGenerated { iteration: usize, script: String },
      ScriptExecuted  { iteration: usize, output: String },
      StreamToken     { token: String },
      CompactionTriggered { cells_before: usize, summary_len: usize },
      Cancelled,
      Final           { answer: String },
      Error           { error: String },
  }
  ```

- `Rlm::run_with_events` — same signature as `run` plus a `tx: mpsc::Sender<StepEvent>`.
  Sends events at each step. `run` becomes a thin wrapper that creates a channel,
  spawns `run_with_events`, and discards events.
- REPL `/run` command switches to `run_with_events`, renders a simple progress line:
  `[iter N] executing cell…` updated in-place via `\r`.
- CLI `query --verbose` flag prints events as they arrive.

## Design notes

- `mpsc::Sender<StepEvent>` is non-blocking (bounded channel, capacity = 32). If the
  receiver is slow/dropped, `send` errors are silently ignored — events are best-effort.
- `StreamToken` events flow through here so streaming (plan 02) and event observability
  share one channel.
- Keep `StepEvent` in `mlrs-core` — it is protocol, not infrastructure.
