//! Top-level agent config (`orvena.yaml`): which provider, which governance tier,
//! default role, the bounded-loop step ceiling, and the OS sandbox posture.

use super::sandbox::SandboxConfig;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Explicitly selected provider. There is **no silent default** — the
    /// selection is materialized by `orvena init`, and an unknown/unconfigured
    /// provider fails loudly at build time.
    pub provider: ProviderSelection,

    /// Governance tier. v0.1 ships two; higher tiers are deferred.
    #[serde(default)]
    pub tier: Tier,

    #[serde(default = "default_role")]
    pub default_role: String,

    /// Upper bound on loop iterations (bounded autonomy — not Devin-style
    /// unbounded re-planning). The gate-fail → re-attempt loop stops here.
    #[serde(default = "default_max_steps")]
    pub max_steps: u32,

    /// OS-level sandbox posture for spawned children (ADR-003). Optional: an
    /// `orvena.yaml` without a `sandbox:` block gets `SandboxConfig::default()`
    /// (disabled), so pre-slice-015 projects keep the previous behavior.
    #[serde(default)]
    pub sandbox: SandboxConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSelection {
    /// `anthropic` | `openai` | `openrouter` | `ollama` | `openai_compat` | `offline`.
    pub kind: String,
    pub model: String,
    /// Optional endpoint override (required-ish for `ollama`, defaulted there;
    /// required with no default for `openai_compat`).
    #[serde(default)]
    pub base_url: Option<String>,
    /// Env var to read the API key from, overriding the kind's default (if
    /// any). Unset for `openai_compat` means the endpoint is unauthenticated —
    /// no `Authorization` header is sent, matching most self-hosted OSS
    /// inference servers (vLLM, llama.cpp server, LM Studio).
    #[serde(default)]
    pub api_key_env: Option<String>,
}

/// Discipline scales with risk (Tiered Governance). v0.1 minimal set.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Tier {
    /// Assistant-like: an in-root scope-lock or gate violation is advisory —
    /// recorded as a blocker, but the loop continues rather than halting.
    #[default]
    Light,
    /// Engineering: an in-root scope-lock or gate violation halts the run.
    Engineering,
}

impl Tier {
    /// Whether an in-root scope/gate violation halts the run rather than just
    /// being recorded. Note: this governs *loop-halt* behavior only — a write
    /// that escapes the project root is always refused by the fs tool regardless
    /// of tier (see `FsTool::resolve_in_root`), so `Light` is never a path to
    /// writing outside the root.
    pub fn enforces(&self) -> bool {
        matches!(self, Tier::Engineering)
    }
}

fn default_role() -> String {
    "developer".to_string()
}

fn default_max_steps() -> u32 {
    3
}
