---
title: Reflection Step
status: open
priority: low
---

# Reflection Step

## Problem

After executing a Rhai script, mlrs feeds the raw output directly back as a user
message. For complex reasoning tasks, the model gets no opportunity to reason about
whether the output actually answers the query or whether the approach should change.
AutoGen's `reflect_on_tool_use` addresses this — after tool results arrive, the model
makes an additional inference to reason about them before producing the next action.

## Deliverables

- `Rlm::with_reflection(bool)` builder method (default `false`).

- When reflection is enabled, after each `StepResult::Continue(output)`:
  1. Add the cell to the notebook as normal.
  2. Make an additional provider call with a `REFLECTION_PROMPT` appended to the
     messages:
     ```
     "Review the output above. Does it make progress toward the query?
      If yes, write 'CONTINUE'. If you have enough to answer, write 'ANSWER: <text>'.
      If the approach is wrong, write 'REPLAN: <new approach>'."
     ```
  3. Parse the response:
     - `CONTINUE` → proceed to next iteration normally.
     - `ANSWER: ...` → treat as a `done()` call, return the answer.
     - `REPLAN: ...` → inject the replan text as a user message before the next
       iteration.

- Reflection calls do not count against `max_iterations` but do count against a
  separate `max_reflection_calls` limit (default = `max_iterations`).

- `StepEvent::ReflectionResult { verdict: String }` emitted for observability (plan 04).

## Design notes

- Reflection is expensive — doubles provider call count per iteration. Default off.
- `REFLECTION_PROMPT` is a `const &str` in `rlm.rs`, not a separate template file.
- The reflection response is not stored in the notebook — it is ephemeral reasoning.
- This is intentionally a simple heuristic; a more sophisticated version could use
  structured output (plan 05 validators) to parse the verdict reliably.
