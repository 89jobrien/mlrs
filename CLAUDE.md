# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands

```bash
# Build
cargo build --workspace

# Check (fast, no codegen)
cargo check --workspace

# Lint
cargo clippy --workspace

# Test (all)
cargo test --workspace

# Test (single test by name)
cargo nextest run -E 'test(notebook_push_and_history)'

# Run the CLI
cargo run -p mlrs-cli -- query "your question" --context "some text"
cargo run -p mlrs-cli -- query "your question" --context-file ./some-file.txt
cargo run -p mlrs-cli -- interactive

# Live provider tests (require real API keys)
cargo test -p mlrs-providers --features live-tests
```

## Environment Variables

| Variable            | Purpose                                                |
| ------------------- | ------------------------------------------------------ |
| `RSLM_PROVIDER`     | Default provider: `openai` (default) or `anthropic`    |
| `RSLM_MODEL`        | Model ID override (e.g. `gpt-4o`, `claude-sonnet-4-6`) |
| `OPENAI_API_KEY`    | Required when using the OpenAI provider                |
| `ANTHROPIC_API_KEY` | Required when using the Anthropic provider             |

## Architecture

**mlrs** is a Recursive Language Model (RLM) system. Instead of receiving the full context
directly, the LLM writes [Rhai](https://rhai.rs) scripts each turn that query the context
through registered functions. Scripts execute, produce output, and the output is fed back to
the model as a "notebook cell". The loop continues until the model calls `done(answer)` or
limits are hit.

### Crates

| Crate            | Role                                                    |
| ---------------- | ------------------------------------------------------- |
| `mlrs-core`      | RLM engine, Rhai scripting environment, protocol types  |
| `mlrs-providers` | `LlmProvider` adapters for OpenAI and Anthropic         |
| `mlrs-cli`       | `mlrs` binary — `query` subcommand and interactive REPL |

### Key types (`mlrs-core`)

- **`Rlm`** (`rlm.rs`) — orchestrates the inference loop. Holds a `Arc<dyn LlmProvider>`,
  depth/iteration limits, and runs `build_engine` + `execute_script` each turn.
- **`LlmProvider`** (`rlm.rs`) — async trait: `complete(messages) -> Result<String>` +
  `model_id()`. Implemented in `mlrs-providers`.
- **`Notebook` / `Cell`** (`protocol.rs`) — accumulates (script, output) pairs and renders
  them as message history for the next LLM call.
- **`StepResult`** (`protocol.rs`) — `Continue(output)` keeps looping; `Final(answer)` exits.
- **`build_engine` / `execute_script`** (`env.rs`) — constructs the Rhai `Engine` with
  context functions registered (`ctx_len`, `ctx_slice`, `ctx_grep`, `print_cell`, `done`),
  then executes a script and returns a `StepResult`. Uses thread-locals to share print/done
  buffers between registered Rhai closures and the executor.

### REPL (`mlrs-cli/src/repl.rs`)

The interactive REPL is built on the `slash` crate (slash-core + slash-lang). Each `/command`
is a `SlashCommand` impl registered in a `CommandRegistry`. `/run` holds an `Arc<Mutex<Rlm>>`
and calls `Rlm::run` via `tokio::runtime::Handle::current().block_on(...)`.

Available REPL commands: `/query`, `/context`, `/context-file`, `/run`, `/status`, `/clear`,
`/help`, `/quit`.

### Adding a provider

1. Implement `LlmProvider` for your struct in `crates/mlrs-providers/src/<name>.rs`.
2. Re-export from `crates/mlrs-providers/src/lib.rs`.
3. Wire the provider name string in `crates/mlrs-cli/src/main.rs` match arm.

### Rhai context API (available inside model-written scripts)

| Function     | Signature                          | Description                           |
| ------------ | ---------------------------------- | ------------------------------------- |
| `ctx_len`    | `() -> int`                        | Byte length of the full context       |
| `ctx_slice`  | `(start: int, end: int) -> String` | Byte-range slice                      |
| `ctx_grep`   | `(pattern: String) -> String`      | Regex filter — returns matching lines |
| `print_cell` | `(msg: String)`                    | Append to current cell output         |
| `done`       | `(answer: String)`                 | Signal final answer and exit the loop |

Note: `final` is a reserved Rhai keyword — the exit function is named `done`.
