//! Provider adapters for the `mlrs-core` LLM abstraction.

pub mod anthropic;
pub mod openai;

pub use anthropic::AnthropicProvider;
pub use openai::OpenAiProvider;
