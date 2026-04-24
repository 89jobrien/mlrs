# gha-gen Design Spec

**Date:** 2026-04-24
**Status:** Approved

## Summary

`gha-gen` is a Rust library (no binary) that models and emits GitHub Actions workflow YAML.
It is API-first: callers construct typed values and call `.to_yaml()`. Designed for eventual
crux integration. Targets reusable workflows and composite actions for Rust projects, starting
with a minimal core.

## Architecture

Single crate, three modules:

- `model` — typed structs for every GHA concept in scope
- `emit` — serde/serde_yaml serialization, custom handling for GHA YAML quirks
- `lib.rs` — public re-exports

No file I/O. No config parsing. No binary. Callers own values and write output to disk.

## Core Types

```
Workflow
  ├── name: String
  ├── on: Trigger
  │     ├── push / pull_request (branch filters)
  │     └── workflow_call (inputs map)
  ├── env: IndexMap<String, String>
  └── jobs: IndexMap<String, Job>

Job
  ├── runs_on: String
  ├── needs: Vec<String>
  ├── if_condition: Option<String>
  ├── permissions: Option<Permissions>
  ├── strategy: Option<Matrix>
  ├── steps: Vec<Step>
  └── outputs: IndexMap<String, String>

Step (uses/run are exclusive — modelled as enum StepBody)
  ├── id: Option<String>
  ├── name: Option<String>
  ├── if_condition: Option<String>
  ├── body: StepBody { Uses { uses, with } | Run { run } }
  └── env: IndexMap<String, String>

Input  (workflow_call + composite actions)
  ├── description: String
  ├── required: bool
  ├── default: Option<String>
  └── type: InputType { String | Boolean | Number | Choice(Vec<String>) }

CompositeAction
  ├── name / description / author
  ├── inputs: IndexMap<String, Input>
  ├── outputs: IndexMap<String, ActionOutput>
  └── runs: CompositeRuns { steps: Vec<Step> }
```

Builders use `typed_builder`. All collection fields default to empty.

`ActionOutput` is a simple struct: `{ value: String, description: String }` where `value` is
a GHA expression string (e.g. `"${{ steps.foo.outputs.bar }}"`).

`Permissions` is an `IndexMap<String, PermissionLevel>` where `PermissionLevel` is
`read | write | none`.

`Matrix` is deferred — represented as `IndexMap<String, Vec<serde_yaml::Value>>` for now.

## Serialization

GHA YAML quirks handled explicitly:

| Quirk                                               | Handling                                       |
| --------------------------------------------------- | ---------------------------------------------- |
| `on:` is reserved in YAML                           | `#[serde(rename = "on")]` on the trigger field |
| `if:` is a Rust keyword                             | field named `if_condition`, serialized as `if` |
| kebab-case keys (`runs-on`, `timeout-minutes`)      | `#[serde(rename_all = "kebab-case")]`          |
| Boolean input type serializes as `"boolean"` string | custom `Serialize` impl on `InputType`         |
| Empty maps/vecs omitted                             | `#[serde(skip_serializing_if = "...")]`        |

Public emit surface:

```rust
impl Workflow {
    pub fn to_yaml(&self) -> Result<String, EmitError>
}

impl CompositeAction {
    pub fn to_yaml(&self) -> Result<String, EmitError>
}
```

`EmitError` is a newtype over `serde_yaml::Error`.

## Testing Strategy

- **Unit tests** — builder correctness, field serialization (especially `on:`, `if:`, kebab-case)
- **Snapshot tests** (`insta`) — `.to_yaml()` output for representative fixtures:
  - Push-triggered workflow
  - `workflow_call` workflow with typed inputs
  - Composite action with inputs and outputs
- **Round-trip test** — emit YAML, parse back with `serde_yaml`, assert key fields match

No live GHA execution tests in the library.

## Crux Integration Notes

The library is designed so crux can construct `Workflow` and `CompositeAction` values directly
via the builder API and emit them as part of an agentic pipeline. No config layer needed for
this use case — crux drives the model programmatically.

## Out of Scope (v1)

- Config/TOML schema layer (can be added later as Option C)
- Binary / CLI
- Full GHA surface (matrix, services, environments, concurrency, reusable workflow caller
  syntax) — add incrementally as needed
- File I/O helpers
