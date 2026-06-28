# mlrs — Agent Operating Guide

This file instructs AI coding agents (Codex, Claude, Copilot, or any
shell-capable LLM agent) how to work effectively in the mlrs codebase.

## Identity & Purpose

You are working on **mlrs**, a Recursive Language Model (RLM) system. Instead of
receiving the full context directly, the LLM writes Rhai scripts each turn that
query the context through registered functions. Scripts execute, produce output,
and the output is fed back to the model as a "notebook cell". The loop continues
until the model calls `done(answer)` or limits are hit.

## Primary Toolkit

All development workflows use standard Rust tooling. No xtask or make required.

| Command                                              | Purpose                                 |
| ---------------------------------------------------- | --------------------------------------- |
| `cargo build --workspace`                            | Build all crates                        |
| `cargo check --workspace`                            | Fast check (no codegen)                 |
| `cargo clippy --workspace`                           | Lint all crates                         |
| `cargo test --workspace`                             | Run all tests                           |
| `cargo nextest run -E 'test(pattern)'`               | Run filtered test by pattern            |
| `cargo run -p mlrs-cli -- query "q"`                 | Run query command                       |
| `cargo run -p mlrs-cli -- interactive`               | Run interactive REPL                    |
| `cargo test -p mlrs-providers --features live-tests` | Live provider tests (requires API keys) |

## Workspace Layout

```
mlrs/
├── crates/
│   ├── mlrs-core/      # RLM engine, Rhai scripting, protocol types
│   ├── mlrs-providers/ # LlmProvider adapters (OpenAI, Anthropic)
│   └── mlrs-cli/       # CLI binary and interactive REPL
├── Cargo.toml          # Workspace config
└── CLAUDE.md           # Project documentation
```

## Code Conventions

### Rust (Edition 2021)

- **Linting**: `cargo clippy --workspace -- -D warnings`
- **Formatting**: `cargo fmt --check`
- **Line width**: 100 characters (follow existing code)
- **Error handling**: `anyhow::Result<T>`, propagate with `?`, no unwrap in
  production code
- **Naming**: PascalCase structs/enums, snake_case functions, SCREAMING_SNAKE_CASE
  constants
- **Tests**: Unit tests in `mod tests {}`, integration tests in `tests/`
- **Async**: Use `tokio::runtime::Handle::current().block_on()` for sync-to-async
  bridging (REPL context)

## Architecture Overview

### Key Types (mlrs-core)

| Type          | Module        | Purpose                                     |
| ------------- | ------------- | ------------------------------------------- |
| `Rlm`         | `rlm.rs`      | Orchestrates inference loop, holds provider |
| `LlmProvider` | `rlm.rs`      | Async trait: `complete()`, `model_id()`     |
| `Notebook`    | `protocol.rs` | Accumulates (script, output) cell pairs     |
| `StepResult`  | `protocol.rs` | `Continue(output)` or `Final(answer)` enum  |
| `Engine`      | `env.rs`      | Rhai execution context with registered fns  |

### Rhai Context API

Functions available inside model-written scripts:

| Function     | Signature                          | Purpose                       |
| ------------ | ---------------------------------- | ----------------------------- |
| `ctx_len`    | `() -> int`                        | Byte length of full context   |
| `ctx_slice`  | `(start: int, end: int) -> String` | Byte-range slice              |
| `ctx_grep`   | `(pattern: String) -> String`      | Regex filter — matching lines |
| `print_cell` | `(msg: String)`                    | Append to cell output         |
| `done`       | `(answer: String)`                 | Exit loop with final answer   |

**Note**: `final` is reserved in Rhai — the exit function is named `done`.

## Environment Variables

| Variable            | Purpose                                    | Default               |
| ------------------- | ------------------------------------------ | --------------------- |
| `RSLM_PROVIDER`     | Default provider (`openai` or `anthropic`) | `openai`              |
| `RSLM_MODEL`        | Model ID override                          | None (provider picks) |
| `OPENAI_API_KEY`    | Required for OpenAI provider               | —                     |
| `ANTHROPIC_API_KEY` | Required for Anthropic provider            | —                     |

## Workflow: Add a New Provider

1. Implement `LlmProvider` trait in `crates/mlrs-providers/src/<name>.rs`:

   ```rust
   #[async_trait]
   impl LlmProvider for YourProvider {
       async fn complete(&self, messages: Vec<Message>) -> Result<String> {
           // Implementation
       }
       fn model_id(&self) -> &str {
           "your-model-id"
       }
   }
   ```

2. Re-export from `crates/mlrs-providers/src/lib.rs`
3. Wire provider name in `crates/mlrs-cli/src/main.rs` match arm

## Workflow: Debug a Test Failure

1. Check which test failed: `cargo test --workspace -- --nocapture`
2. Run single test: `cargo nextest run -E 'test(your_test_name)'`
3. Check provider status: `cargo test -p mlrs-providers --features live-tests`
4. Verify environment variables are set (API keys for live tests)

## Pre-Commit Quality

Before committing, run:

```bash
cargo fmt --check       # Check formatting
cargo clippy --workspace -- -D warnings  # Lint all
cargo test --workspace  # Test all
```

Or commit and let the pre-commit hook validate.

## Quick Fixes

| Issue                     | Fix                                |
| ------------------------- | ---------------------------------- |
| Format error on commit    | `cargo fmt && git add -A`          |
| Clippy warnings           | `cargo clippy --fix --allow-dirty` |
| Test failing in one crate | `cargo test -p <crate-name>`       |

## Key Dependencies

- **LLM inference**: `async-trait`, `tokio`
- **Scripting**: `rhai` (1.x, sync feature)
- **CLI**: `clap` (derive), `reedline`, `nu-ansi-term`
- **Parsing**: `regex`, `slash-core`, `slash-lang`
- **Error handling**: `anyhow`, `thiserror`
- **Logging**: `tracing`, `tracing-subscriber`
