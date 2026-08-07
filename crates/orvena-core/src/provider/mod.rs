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

/// Backend identity as it can be **checked after the fact**, not as configured.
///
/// `model` in a report is a tag, and a tag is a mutable pointer: re-pulling
/// `qwen3:14b` can swap the weights, the quantization, and the sampling
/// defaults under a name that never changes. Three invocations of one probe on
/// 0805–06 agreed on every recorded field and still differed by 100 points on a
/// per-task pass rate; nothing in the report could say whether they had even
/// measured the same thing. These fields are what makes that question
/// answerable (slice-029).
///
/// Every field is `Option` and every one is best-effort: provenance is
/// bookkeeping, so failing to read it degrades the record but must never fail a
/// benchmark. `None` means **not recorded** — never "same as default".
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProviderProvenance {
    /// Backend server version (`ollama 0.32.5`), where the backend exposes one.
    pub server_version: Option<String>,
    /// Immutable content hash of the weights behind the mutable tag.
    pub model_digest: Option<String>,
    pub quantization: Option<String>,
    /// The context length the *model* declares (its ceiling).
    pub context_length_declared: Option<u32>,
    /// The context length the *runtime* actually gave this model, which on a
    /// local backend depends on memory pressure and on what else was resident.
    /// Differing from `declared` is the finding, not an error — record both.
    pub context_length_effective: Option<u32>,
}

/// A chat-completion backend. Implementors are `Send + Sync` so the core can be
/// embedded in a multi-threaded runtime.
#[async_trait]
pub trait Provider: Send + Sync {
    fn id(&self) -> &str;
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse>;
    /// Best-effort backend identity, for the report header. The default is
    /// `None` — "this backend records nothing" is a truthful answer, and a
    /// backend that cannot be identified must say so rather than let the
    /// configured tag stand in for the thing that actually ran.
    async fn provenance(&self) -> Option<ProviderProvenance> {
        None
    }
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
