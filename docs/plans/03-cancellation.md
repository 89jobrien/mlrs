---
title: Cancellation
status: done
priority: high
---

# Cancellation

## Problem

`Rlm::run` is an async loop with no cancellation path. The REPL has no way to interrupt
a running query (Ctrl-C during `/run` does nothing). Long-running or stuck loops must
wait for `max_iterations` to exhaust.

## Deliverables

- `Rlm::run` signature changes to accept a `CancellationToken`:

  ```rust
  pub async fn run(
      &self,
      query: &str,
      context: &str,
      cancel: CancellationToken,
  ) -> Result<String, RlmError>
  ```

- Each loop iteration selects on `cancel.cancelled()` vs. the provider future. On
  cancellation, return `Err(RlmError::Cancelled)`.
- `RlmError` gains a `Cancelled` variant.
- REPL `/run` command creates a `CancellationToken`, spawns `Rlm::run` on the current
  tokio runtime, and installs a Ctrl-C handler that calls `token.cancel()`. Prints
  `[cancelled]` on interrupt.
- CLI `query` subcommand does the same.
- Callers that don't need cancellation pass `CancellationToken::new()` (never cancelled).

## Design notes

- Use `tokio_util::sync::CancellationToken` — already used in `slash` and available in
  the ecosystem. Add as a dep to `mlrs-core`.
- Do not change the REPL's `Arc<Mutex<Rlm>>` — cancellation is per-invocation, not
  per-instance.
- The provider future itself is not cancelled mid-flight (HTTP request continues); we
  cancel between iterations, not inside `complete()`. Deeper cancellation is a later
  concern.
