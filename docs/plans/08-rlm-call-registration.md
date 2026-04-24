---
title: Register rlm_call as a Real Rhai Function
status: open
priority: medium
---

# Register rlm_call as a Real Rhai Function

## Problem

The system prompt documents `rlm_call(query, ctx_fragment)` as an available function,
but it is never registered in `env.rs`. Any model that attempts to use it gets a Rhai
runtime error. This is a documentation lie that silently degrades model performance on
tasks where recursive decomposition would help.

## Deliverables

- Register `rlm_call(query: String, ctx: String) -> String` in `build_engine`.
- Implementation: spawns a child `Rlm` with `depth = parent.depth + 1`, runs
  `child.run(query, ctx)` synchronously via `Handle::current().block_on(...)`.
- Returns the answer string on success, or an error string prefixed with
  `"rlm_call error: "` on failure (so the model can self-correct).
- Depth limit is enforced — if `depth >= max_depth`, returns
  `"rlm_call error: max depth exceeded"` without spawning.
- The child `Rlm` inherits `max_iterations`, `max_retries_per_cell`, and the provider
  from the parent via thread-local or closure capture.

## Design notes

- Provider is `Arc<dyn LlmProvider>` — cheap to clone into the closure.
- `block_on` inside a Rhai function called from within an async context requires care:
  use `tokio::task::block_in_place` + `Handle::current().block_on(...)` to avoid
  blocking the async executor thread directly.
- Child notebook is independent — not merged into parent notebook.
- Remove the `rlm_call` mention from the system prompt's "note:" section and move it
  to the main function table once it actually works.
- Add a unit test: parent script calls `rlm_call("what is 2+2", "")`, child returns
  "4", parent calls `done("answer: 4")`.
