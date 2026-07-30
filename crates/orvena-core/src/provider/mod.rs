//! Provider abstraction. A single [`Provider`] trait fronts multiple backends;
//! [`build_chat_provider`] is the explicit factory — it **never silently
//! defaults**. An unknown or unconfigured provider returns an error telling the
//! user to choose one (via `orvena init`).
//!
//! Design reference: the lab's `provider_setup.py` (rewritten, not copied).

pub mod anthropic;
pub mod offline;
pub mod ollama;
pub mod openai_compat;
pub mod registry;

pub use registry::{readiness, ProviderInfo, Readiness};

use crate::config::agent::ProviderSelection;
use crate::error::{Error, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// `system` | `user` | `assistant`.
    pub role: String,
    pub content: String,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: "system".into(), content: content.into() }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: "user".into(), content: content.into() }
    }
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub messages: Vec<Message>,
    pub max_tokens: u32,
}

#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub content: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// A chat-completion backend. Implementors are `Send + Sync` so the core can be
/// embedded in a multi-threaded runtime.
#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &str;
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse>;
}

/// Build a provider from an **explicit** selection. No silent fallback.
pub fn build_chat_provider(sel: &ProviderSelection) -> Result<Box<dyn Provider>> {
    match sel.kind.as_str() {
        "anthropic" => Ok(Box::new(anthropic::Anthropic::from_env(sel)?)),
        "openai" | "openrouter" | "openai_compat" => {
            Ok(Box::new(openai_compat::OpenAiCompat::from_env(sel)?))
        }
        "ollama" => Ok(Box::new(ollama::Ollama::new(sel))),
        "offline" => Ok(Box::new(offline::Offline::new(sel))),
        other => Err(Error::Provider(format!(
            "unknown provider '{other}'. Choose one explicitly (run `orvena init`): \
             anthropic | openai | openrouter | ollama | openai_compat | offline. \
             No default is assumed."
        ))),
    }
}
