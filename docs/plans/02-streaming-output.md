---
title: Streaming Output
status: open
priority: high
---

# Streaming Output

## Problem

`LlmProvider::complete` blocks until the full response is available. For long model
responses (multi-step Rhai scripts, verbose reasoning) this produces a dead cursor in the
REPL and CLI. Users have no feedback that the model is working.

## Deliverables

- `LlmProvider` gains a second method with a default impl:

  ```rust
  async fn complete_stream(
      &self,
      messages: Vec<Message>,
  ) -> Result<BoxStream<'static, Result<String>>>;
  ```

  Default implementation calls `complete()` and wraps the result in `stream::once`.
  Providers that support native streaming (OpenAI, Anthropic) override it.

- `OpenAiProvider` streams via `stream: true` SSE chunks.
- `AnthropicProvider` streams via Anthropic's streaming API.
- `Rlm` gains a `streaming: bool` field (default `false`). When `true`, `run()` calls
  `complete_stream` and writes tokens to stderr (or a callback — see plan 04) as they
  arrive, then assembles the full script string for execution.
- REPL `/run` command respects the `Rlm::streaming` flag.

## Design notes

- Use `futures::stream::BoxStream` — already a transitive dep via `tokio`.
- The assembled full-script string is identical whether streaming or not; execution path
  is unchanged.
- Streaming is opt-in so existing tests remain synchronous and deterministic.
- Do not change the `LlmProvider` trait signature for `complete` — streaming is additive.
