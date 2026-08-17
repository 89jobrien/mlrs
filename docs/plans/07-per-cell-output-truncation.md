---
title: Per-Cell Output Truncation
status: done
priority: medium
---

# Per-Cell Output Truncation

## Problem

`ctx_grep` or `print_cell` calls that produce large outputs are stored verbatim in the
notebook and fed back into the context window on the next iteration. A grep returning
10k lines will silently consume most of the model's context budget.

## Deliverables

- `TruncationPolicy` in `mlrs-core`:

  ```rust
  pub struct TruncationPolicy {
      pub max_bytes: usize,
      pub strategy: TruncationStrategy,
  }

  pub enum TruncationStrategy {
      TailDrop,   // keep first max_bytes, append "[N bytes truncated]"
      HeadDrop,   // keep last max_bytes, prepend "[N bytes truncated]"
      Middle,     // keep first half + last half, replace middle with "[N bytes omitted]"
  }
  ```

  Default: `max_bytes = 8192`, `strategy = TailDrop`.

- `truncate_output(text: &str, policy: &TruncationPolicy) -> String` — pure function.

- In `env.rs`, the `print_cell` buffer and the script execution output are both truncated
  via `truncate_output` before being returned as `StepResult::Continue(output)`.

- `Rlm::with_truncation_policy(TruncationPolicy)` builder method.

- `TruncationPolicy::none()` disables truncation (for tests and when caller manages
  context themselves).

## Design notes

- Truncation happens at the engine layer (`env.rs` / `rlm.rs`), not in the Rhai
  functions themselves — callers get the raw value from `ctx_grep`, but what gets stored
  in the notebook and fed back is truncated.
- Byte-based truncation (not token-based) — simple and dependency-free.
- The truncation marker is included verbatim in the cell output so the model can see
  that data was cut.
