---
title: Parallel Tool Execution
status: open
priority: low
---

# Parallel Tool Execution

## Problem

mlrs executes one Rhai script per iteration. If a model wants to probe multiple
independent context regions in one step (e.g. `ctx_grep("foo")` AND `ctx_grep("bar")`),
it must do so sequentially across two iterations. AutoGen and Codex execute multiple
tool calls concurrently when the model issues them in one response.

This plan applies once mlrs supports a multi-call response format — currently the model
emits one script per turn. The prerequisite is defining a structured response format
that allows the model to declare multiple independent scripts/queries in one response.

## Deliverables

- `MultiScript` response type: the model can optionally return a JSON-wrapped list of
  scripts instead of a single Rhai script. Detection heuristic: if the response begins
  with `[` and parses as a JSON array of strings, treat each element as an independent
  script.

- `execute_scripts_parallel(scripts: Vec<&str>, engine, scope) -> Vec<StepResult>`:
  runs each script in a separate `rayon` thread (Rhai `Engine` is `!Send`, so we use
  `rayon::scope` with per-thread engine instances cloned from a template).

- Outputs are merged: all `Continue` outputs concatenated with `---` separators; if any
  script calls `done()`, that `Final` result wins and parallel execution is cancelled.

- `Rlm::with_parallel_scripts(bool)` builder (default `false`, opt-in).

## Design notes

- This is architecturally complex because Rhai's `Engine` is not `Send`. Each parallel
  script needs its own engine instance built from the same context. `build_engine` must
  become cheap enough to call N times per iteration.
- The multi-call response format requires prompt engineering changes in `SYSTEM_PROMPT`
  to teach the model the JSON array syntax.
- Prerequisite: structured step events (plan 04) so the REPL can display parallel
  execution progress.
- Do not implement until plans 01–08 are complete.
