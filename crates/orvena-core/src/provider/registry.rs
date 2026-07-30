//! Provider registry: the known providers, their env-key mapping, and a
//! readiness check (used by `orvena doctor`). No provider is poked silently —
//! readiness only *reports*; it never picks a default.

use crate::config::agent::ProviderSelection;

/// Static facts about a known provider.
pub struct ProviderInfo {
    pub kind: &'static str,
    /// Env var holding the API key, if any (`None` for local/offline, or when
    /// the kind's key env var name is chosen at config time via
    /// `ProviderSelection.api_key_env`, e.g. `openai_compat`).
    pub env_key: Option<&'static str>,
    pub description: &'static str,
    /// Whether a base_url is typically needed (e.g. local Ollama).
    pub local: bool,
    /// Whether `orvena init` must prompt for (and `orvena.yaml` must set) an
    /// explicit `base_url` — there is no sane default endpoint to fall back to.
    pub requires_base_url: bool,
}

/// The providers orvena knows how to build.
pub fn known() -> Vec<ProviderInfo> {
    vec![
        ProviderInfo {
            kind: "anthropic",
            env_key: Some("ANTHROPIC_API_KEY"),
            description: "Anthropic Claude (hosted). Recommended first run.",
            local: false,
            requires_base_url: false,
        },
        ProviderInfo {
            kind: "openai",
            env_key: Some("OPENAI_API_KEY"),
            description: "OpenAI (hosted).",
            local: false,
            requires_base_url: false,
        },
        ProviderInfo {
            kind: "openrouter",
            env_key: Some("OPENROUTER_API_KEY"),
            description: "OpenRouter (hosted) — one key, many models.",
            local: false,
            requires_base_url: false,
        },
        ProviderInfo {
            kind: "ollama",
            env_key: None,
            description: "Local via Ollama (offline/private). You run Ollama yourself.",
            local: true,
            requires_base_url: false,
        },
        ProviderInfo {
            kind: "openai_compat",
            env_key: None,
            description: "Generic OpenAI-compatible endpoint — self-hosted OSS servers \
                (vLLM, llama.cpp server, LM Studio, TGI, SGLang) or hosted open-weight \
                aggregators (Groq, Together, Fireworks). Needs base_url; API key optional.",
            local: true,
            requires_base_url: true,
        },
        ProviderInfo {
            kind: "offline",
            env_key: None,
            description: "Deterministic stub for tests and L1 baselines (no network).",
            local: true,
            requires_base_url: false,
        },
    ]
}

pub fn info(kind: &str) -> Option<ProviderInfo> {
    known().into_iter().find(|p| p.kind == kind)
}

/// Result of a readiness probe for a provider kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Readiness {
    /// Ready to use (key present, or no key needed).
    Ready,
    /// Hosted provider whose API key env var is missing.
    MissingKey(String),
    /// Provider kind is not known to orvena.
    Unknown,
}

/// Check readiness without making a network call. (Connectivity probes are a
/// separate, explicit step in `doctor`.) `sel.api_key_env` — a per-config env
/// var name — takes precedence over the kind's static default, since kinds
/// like `openai_compat` have no fixed key var (or none at all).
pub fn readiness(sel: &ProviderSelection) -> Readiness {
    match info(&sel.kind) {
        None => Readiness::Unknown,
        Some(p) => match sel.api_key_env.as_deref().or(p.env_key) {
            None => Readiness::Ready,
            Some(key) => match std::env::var(key) {
                Ok(v) if !v.trim().is_empty() => Readiness::Ready,
                _ => Readiness::MissingKey(key.to_string()),
            },
        },
    }
}
