---
title: Final Answer Validation
status: open
priority: medium
---

# Final Answer Validation

## Problem

`done(answer)` in a Rhai script exits the loop immediately with whatever string is
passed. There is no way to enforce that the answer meets a format contract (JSON,
markdown, non-empty, etc.) before returning it to the caller.

## Deliverables

- `AnswerValidator` trait in `mlrs-core`:

  ```rust
  pub trait AnswerValidator: Send + Sync {
      fn validate(&self, answer: &str) -> Result<(), String>;
  }
  ```

- Built-in validators:
  - `NonEmptyValidator` — rejects blank answers.
  - `JsonValidator` — rejects non-parseable JSON.
  - `RegexValidator(pattern)` — rejects answers not matching the pattern.

- `Rlm::with_validators(Vec<Box<dyn AnswerValidator>>)` builder method.

- In `Rlm::run`, after `StepResult::Final(answer)` is received:
  1. Run all validators in order.
  2. On failure: feed the validation error back as a user message ("Your answer failed
     validation: <error>. Rewrite it and call done() again.") and continue the loop.
  3. Count validation retries separately; cap at `max_validation_retries` (default 3).
  4. If all retries exhausted, return `Err(RlmError::ValidationFailed(last_error))`.

- `RlmError` gains `ValidationFailed(String)`.

## Design notes

- Validators are pure functions — no async, no provider calls.
- The feedback message on failure is a `user` role message injected into `build_messages`
  as a `ValidationFeedback` cell type — distinguish from normal cell outputs so future
  tooling can filter them.
- CLI: `--expect-json` flag installs `JsonValidator` automatically.
