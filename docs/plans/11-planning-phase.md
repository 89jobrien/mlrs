---
title: Planning Phase
status: open
priority: low
---

# Planning Phase

## Problem

For long-horizon tasks, the model drifts without an explicit plan. smolagents addresses
this with `planning_interval` — every N steps the model generates a structured plan
before acting. This improves coherence on tasks requiring many iterations.

## Deliverables

- `PlanningConfig` in `mlrs-core`:

  ```rust
  pub struct PlanningConfig {
      pub interval: usize,        // re-plan every N iterations (0 = never)
      pub initial_plan: bool,     // generate a plan before iteration 1
  }
  ```

- `Rlm::with_planning(PlanningConfig)` builder method.

- Planning call: before iteration 1 (if `initial_plan`) and every `interval` iterations,
  inject a planning prompt:

  ```text
  "Before writing your next script, produce a brief numbered plan (3–7 steps) for how
   you will answer the query given what you know so far. Output only the plan, no code."
  ```

  The model's response is stored as a `PlanCell` in a separate `plan_history: Vec<String>`
  field on the run state (not in the main notebook).

- The current plan is injected into `build_messages` as a system-level context item
  after the system prompt:

  ```rust
  Message { role: Role::User, content: format!("Current plan:\n{plan}") }
  Message { role: Role::Assistant, content: "Understood.".into() }
  ```

- `StepEvent::PlanGenerated { iteration: usize, plan: String }` for observability.

## Design notes

- Planning calls do not count against `max_iterations`.
- `plan_history` is included in `RunRecord` (plan 06) for post-hoc inspection.
- `initial_plan = true` with `interval = 0` means plan once at the start only.
- The plan is intentionally free-text — no structured format enforced at this stage.
  Structured plan parsing is a future extension.
