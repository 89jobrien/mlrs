---
title: Provider Retry with Backoff
status: open
priority: low
---

# Provider Retry with Backoff

## Problem

All provider errors surface as `RlmError::ProviderError` with no distinction between
transient failures (rate limits, 429, 503, network blip) and permanent ones (bad API
key, invalid model, 400). Transient errors abort the entire run unnecessarily.

## Deliverables

- `ProviderError` enum replacing the current `anyhow::Error` in the trait return type:

  ```rust
  pub enum ProviderError {
      RateLimit { retry_after_secs: Option<u64> },
      ServerError(String),
      Network(String),
      InvalidRequest(String),   // non-retryable
      Unauthorized,             // non-retryable
      Other(String),
  }

  impl ProviderError {
      pub fn is_retryable(&self) -> bool { ... }
  }
  ```

- `LlmProvider::complete` return type changes to `Result<String, ProviderError>`.

- `Rlm::run` retries on `is_retryable()` errors with exponential backoff:
  - Initial delay: 1s
  - Max delay: 30s
  - Max retries: 5 (configurable via `Rlm::with_max_provider_retries(usize)`)
  - Respects `retry_after_secs` from rate-limit responses when present.

- `RlmError::ProviderError(ProviderError)` replaces `RlmError::ProviderError(anyhow::Error)`.

- Both `OpenAiProvider` and `AnthropicProvider` map their HTTP status codes to the
  appropriate `ProviderError` variant.

## Design notes

- Use `tokio::time::sleep` for backoff — already in scope.
- Backoff does not count against `max_iterations`.
- `ProviderError` moves to `mlrs-core` (provider errors are part of the protocol
  contract); providers in `mlrs-providers` implement the mapping.
- The `anyhow` dependency in `mlrs-core` can be removed once `ProviderError` replaces
  it in the trait boundary.
