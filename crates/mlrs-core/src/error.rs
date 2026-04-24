use thiserror::Error;

#[derive(Debug, Error)]
pub enum RlmError {
    #[error("max depth {0} exceeded")]
    MaxDepthExceeded(usize),

    #[error("max iterations {0} exceeded without calling done()")]
    MaxIterationsExceeded(usize),

    #[error("script error: {0}")]
    ScriptError(String),

    #[error("max script retries ({0}) exceeded for one cell")]
    MaxRetriesExceeded(usize),

    #[error("provider error: {0}")]
    ProviderError(#[source] anyhow::Error),

    #[error("run cancelled")]
    Cancelled,
}
