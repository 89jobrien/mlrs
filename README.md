# mlrs

`mlrs` is an experimental Recursive Language Model (RLM) implementation in Rust. Rather than
sending an entire context to a model, it asks the model to write one Rhai script at a time. Each
script can inspect the context, its output becomes a notebook cell, and the loop continues until
the script calls `done(answer)` or a safety limit is reached.

The workspace currently provides a reusable engine, OpenAI and Anthropic adapters, and an `mlrs`
CLI with one-shot and interactive modes. Cancellation, per-cell output truncation, and notebook
compaction are implemented. Streaming, persistence, structured events, retry/backoff, and final
answer validation remain planned work.

> **Implementation note:** the engine prompt advertises `rlm_call(query, ctx_fragment)`, but the
> current Rhai environment does not register that function. Runs can inspect a context iteratively,
> but model-requested recursive subcalls are not yet available.

## How it works

1. `Rlm` sends the query and notebook history to an `LlmProvider`.
2. The provider returns a Rhai script rather than a prose answer.
3. `mlrs-core` executes the script with a small context API.
4. Script output is truncated if necessary and appended to the notebook.
5. The next provider request includes the notebook; `done(...)` ends the run.
6. After a notebook's token estimate exceeds the configured threshold, it is summarized before
   the next provider request.

The Rhai API available to generated scripts is:

| Function                | Purpose                                             |
| ----------------------- | --------------------------------------------------- |
| `ctx_len()`             | Return the context length in bytes.                 |
| `ctx_slice(start, end)` | Return a byte range from the context.               |
| `ctx_grep(pattern)`     | Return context lines matching a regular expression. |
| `print_cell(message)`   | Add text to the current notebook cell output.       |
| `done(answer)`          | Return the final answer and stop the loop.          |

`ctx_slice` uses byte indexes and the current implementation indexes the Rust string directly.
Callers must therefore use UTF-8 boundaries. Rhai compile/runtime errors are returned to the model
for self-correction, up to the retry limit.

> **Privacy:** context excerpts returned by `ctx_slice` or `ctx_grep` become notebook cells and are
> sent to the configured external provider on later requests. Do not supply sensitive context that
> the provider is not permitted to receive.

## Workspace layout

| Path                    | Role                                                                        |
| ----------------------- | --------------------------------------------------------------------------- |
| `crates/mlrs-core`      | RLM loop, Rhai environment, notebook protocol, limits, and cancellation.    |
| `crates/mlrs-providers` | OpenAI and Anthropic implementations of `LlmProvider`.                      |
| `crates/mlrs-cli`       | `mlrs` binary and slash-command interactive REPL.                           |
| `docs/plans`            | Design plans; some status labels lag the implementations in current source. |

The central public API is exported by `mlrs-core`: `Rlm`, `LlmProvider`, `CancellationToken`,
notebook types, truncation policy types, and the low-level Rhai engine helpers.

## Build and install

The workspace uses Rust 2021 and standard Cargo commands:

```console
cargo build --workspace
cargo install --path crates/mlrs-cli
```

The repository currently declares version `0.1.0`. Installing from the workspace path is the
source-grounded installation route; this README does not assume the crates are published.

## Configuration

| Setting             | Meaning                                              | Default           |
| ------------------- | ---------------------------------------------------- | ----------------- |
| `RSLM_PROVIDER`     | `openai` or `anthropic`; overridden by `--provider`. | `openai`          |
| `RSLM_MODEL`        | Provider model ID; overridden by `--model`.          | Provider-specific |
| `OPENAI_API_KEY`    | Credential used by the OpenAI client.                | None              |
| `ANTHROPIC_API_KEY` | Credential required by the Anthropic adapter.        | None              |
| `RUST_LOG`          | `tracing-subscriber` filter.                         | Adds `mlrs=info`  |

The `RSLM_*` prefix is retained for compatibility with the project's legacy name. The CLI currently
recognizes `RSLM_PROVIDER` and `RSLM_MODEL`; it does not provide `MLRS_*` aliases.

The default model is `gpt-4o` for OpenAI and `claude-sonnet-4-6` for Anthropic. Unknown provider
names currently fall back to the OpenAI branch rather than producing a validation error.

## CLI usage

Run a question with inline context:

```console
cargo run -p mlrs-cli -- query \
  "Which service owns authentication?" \
  --context "The gateway validates tokens. The account service stores profiles."
```

Use a file as the context and show each generated cell on standard error:

```console
cargo run -p mlrs-cli -- \
  --provider anthropic \
  --model claude-sonnet-4-6 \
  --max-depth 5 \
  --verbose \
  query "Summarize the error handling" --context-file ./src/lib.rs
```

Start the interactive REPL:

```console
cargo run -p mlrs-cli -- interactive
```

The REPL supports `/query(<text>)`, `/context(<text>)`, `/context-file(<path>)`, `/run`,
`/status`, `/clear`, `/help`, and `/quit`. `/run` also accepts the slash arguments `depth` and
`verbose`. Pressing Ctrl-C cancels a run between or during provider requests.

Generate Nushell completions with the source-supported, pre-parser command:

```console
mlrs completions > mlrs-completions.nu
```

Because completion generation is handled before Clap parses the command line, `completions` does
not appear in `mlrs --help`.

## Library usage

Implement `LlmProvider` to use another inference backend, then construct an `Rlm` around it:

```rust
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use mlrs_core::{CancellationToken, LlmProvider, Message, Rlm};

struct Provider;

#[async_trait]
impl LlmProvider for Provider {
    async fn complete(&self, _messages: Vec<Message>) -> Result<String> {
        Ok(r#"done("answer")"#.to_owned())
    }

    fn model_id(&self) -> &str {
        "example"
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let rlm = Rlm::new(Arc::new(Provider))
        .with_max_depth(5)
        .with_compaction_threshold(102_400);
    let answer = rlm
        .run("question", "context", CancellationToken::new())
        .await?;
    assert_eq!(answer, "answer");
    Ok(())
}
```

Defaults include 20 iterations, three retries after script errors, an 8 KiB tail-drop limit per
cell, and notebook compaction after its estimate exceeds 102,400 tokens. These fields and the
truncation policy are configurable through `Rlm`'s public API.

## Development

```console
cargo fmt --check
cargo check --workspace
cargo clippy --workspace -- -D warnings
cargo test --workspace
```

The provider manifest declares a `live-tests` feature, but the current provider source has no live
tests. The feature therefore enables no tests today; the command reserved for future live tests is:

```console
cargo test -p mlrs-providers --features live-tests
```

Normal core tests use local mock providers and do not require API keys. The implementation is
cross-platform Rust, but future live tests will depend on network access and the selected provider
API.

## License

Licensed under either Apache-2.0 or MIT, at your option.
