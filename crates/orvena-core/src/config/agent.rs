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

/// `deny_unknown_fields`: a typo here is a security posture change, not a
/// cosmetic one — `api_key_evn` under `openai_compat` used to be silently
/// ignored by serde, downgrading the request to **no auth** while `doctor`
/// reported ready. An unknown key in the `provider:` block is now a hard parse
/// error naming the field.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderSelection {
    /// `anthropic` | `openai` | `openrouter` | `ollama` | `openai_compat` | `offline`.
    pub kind: String,
    pub model: String,
    /// Optional endpoint override (required-ish for `ollama`, defaulted there;
    /// required with no default for `openai_compat`).
    #[serde(default)]
    pub base_url: Option<String>,
    /// Env var to read the API key from, overriding the kind's default.
    ///
    /// Honored by the OpenAI-compatible kinds only (`openai`, `openrouter`,
    /// `openai_compat`) — those are the ones whose builder reads it. Setting it
    /// on `anthropic`, `ollama`, or `offline` has no effect: their builders
    /// hardcode their own key var or need none, so readiness deliberately
    /// ignores it there rather than reporting a state the build won't honor.
    ///
    /// Unset on `openai_compat` means the endpoint is treated as
    /// **unauthenticated** — no `Authorization` header is sent at all, matching
    /// self-hosted OSS servers (vLLM, llama.cpp server, LM Studio). Note this
    /// is "no auth", not "auth optional": a *misspelled* key (`api_key_evn`)
    /// would silently downgrade the request to no-auth, which is why the struct
    /// rejects unknown fields at parse time.
    #[serde(default)]
    pub api_key_env: Option<String>,
    /// Sampling parameters, when this repo takes control of them.
    ///
    /// `None` means **inherited**, not "the defaults": nothing is sent and the
    /// backend decides. For Ollama that is the model's Modelfile — a file this
    /// repo neither versions nor can see from a published report. Measured
    /// 0806: `qwen3:14b` ships `temperature 0.6` while `qwen3.6:27b` and
    /// `qwen3.6:35b` ship `temperature 1`, so the floor cell and the ceiling
    /// cell of the capability ladder were never sampled under equal conditions
    /// — and nothing in the report said so. Provenance now distinguishes the
    /// two states rather than printing a number for both (slice-029).
    #[serde(default)]
    pub sampling: Option<Sampling>,
}

/// Sampling parameters the repo sends explicitly.
///
/// Whatever is here becomes part of a reading's identity: two reports with
/// different sampling are not comparable, and a report with `None` is not
/// comparable to anything — including a later run of itself.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sampling {
    /// `f64`, not `f32`, because these values are **transmitted and published**
    /// rather than computed: `0.6f32` serializes as `0.6000000238418579`, which
    /// would reach the backend and land in every report as a number nobody
    /// configured. Rates elsewhere in this crate stay `f32` — they are derived,
    /// and their last bits carry no intent.
    pub temperature: f64,
    pub top_p: f64,
    pub top_k: u32,
    /// Fixing the seed makes one path reproducible — and makes `--repeat`
    /// measure nothing, because every repeat returns the same sample. Left
    /// `None` on purpose: how stable a model is under resampling is itself the
    /// reading slice-028 was built to take. A seeded regression run belongs
    /// beside `repeat`, never in place of it.
    #[serde(default)]
    pub seed: Option<u64>,
}

impl ProviderSelection {
    /// Origin of the configured endpoint (`scheme://host[:port]`), for
    /// provenance in published reports — or `None` when no `base_url` is set
    /// and the kind therefore uses its own fixed endpoint.
    ///
    /// `kind` alone used to identify the endpoint, but `openai_compat` is
    /// deliberately endpoint-agnostic: a local llama.cpp and Groq serving the
    /// same open-weight model would otherwise produce byte-identical
    /// provenance. Origin restores the distinction.
    ///
    /// Deliberately **not** the full URL. `base_url` is user-supplied and may
    /// carry credentials in userinfo (`https://user:token@host/v1`) or a query
    /// string, and benchmark reports are committed and published — so userinfo,
    /// path, query, and fragment are all dropped rather than trusted.
    pub fn endpoint_origin(&self) -> Option<String> {
        let raw = self.base_url.as_deref()?.trim();
        let (scheme, rest) = raw.split_once("://")?;
        let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
        // Anything before '@' is userinfo — credentials. Drop it.
        let host = authority.rsplit_once('@').map_or(authority, |(_, host)| host);
        if scheme.is_empty() || host.is_empty() {
            return None;
        }
        Some(format!("{}://{}", scheme.to_ascii_lowercase(), host.to_ascii_lowercase()))
    }
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
    // Sized for the READ/EDIT loop (slice-021): locate → edit → verify → read
    // the evidence → re-edit → verify is 4-5 steps for one honest attempt; 8
    // buys a full retry. The v0.1 default of 3 was sized for the blind
    // full-file-WRITE loop.
    8
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel(base_url: Option<&str>) -> ProviderSelection {
        ProviderSelection {
            kind: "openai_compat".into(),
            model: "m".into(),
            base_url: base_url.map(Into::into),
            api_key_env: None,
            sampling: None,
        }
    }

    #[test]
    fn the_default_step_budget_is_sized_for_the_read_edit_loop() {
        // slice-021: 4-5 steps is one honest locate → edit → verify → re-edit
        // attempt; 8 buys a full retry. Changing this changes the measurement
        // envelope — update SLICE-021-step-budget.md alongside.
        assert_eq!(default_max_steps(), 8);
    }

    #[test]
    fn endpoint_origin_distinguishes_backends() {
        assert_eq!(
            sel(Some("http://localhost:11434/v1")).endpoint_origin().as_deref(),
            Some("http://localhost:11434")
        );
        assert_eq!(
            sel(Some("https://api.groq.com/openai/v1")).endpoint_origin().as_deref(),
            Some("https://api.groq.com")
        );
        assert_eq!(sel(None).endpoint_origin(), None);
    }

    // Reports are committed and published — a token in the URL must never
    // ride along into one.
    #[test]
    fn endpoint_origin_strips_credentials_and_query() {
        assert_eq!(
            sel(Some("https://user:s3cret@api.example.com/v1")).endpoint_origin().as_deref(),
            Some("https://api.example.com")
        );
        assert_eq!(
            sel(Some("https://host/v1?api-key=s3cret")).endpoint_origin().as_deref(),
            Some("https://host")
        );
        for raw in ["https://user:s3cret@api.example.com/v1", "https://host/v1?api-key=s3cret"] {
            assert!(
                !sel(Some(raw)).endpoint_origin().unwrap().contains("s3cret"),
                "secret leaked from {raw}"
            );
        }
    }

    #[test]
    fn endpoint_origin_handles_ipv6_and_rejects_schemeless() {
        assert_eq!(
            sel(Some("http://[::1]:8000/v1")).endpoint_origin().as_deref(),
            Some("http://[::1]:8000")
        );
        assert_eq!(sel(Some("localhost:11434")).endpoint_origin(), None);
        assert_eq!(sel(Some("   ")).endpoint_origin(), None);
    }

    // The typo that motivated deny_unknown_fields: `api_key_evn` under
    // `openai_compat` used to be silently ignored, downgrading the request to
    // no-auth while `doctor` reported ready. A typo in a security-relevant
    // field must be a parse error, not a posture change.
    #[test]
    fn a_misspelled_provider_field_is_a_parse_error_not_a_silent_downgrade() {
        let err = serde_yaml::from_str::<ProviderSelection>(
            "kind: openai_compat\nmodel: m\nbase_url: http://localhost:8000/v1\napi_key_evn: MY_KEY\n",
        )
        .expect_err("an unknown field must not parse");
        let msg = err.to_string();
        assert!(msg.contains("api_key_evn"), "the error names the offending field: {msg}");
    }

    #[test]
    fn every_declared_provider_field_still_parses() {
        let s = serde_yaml::from_str::<ProviderSelection>(
            "kind: openai_compat\nmodel: m\nbase_url: http://localhost:8000/v1\napi_key_env: MY_KEY\n",
        )
        .expect("the full legitimate surface parses");
        assert_eq!(s.api_key_env.as_deref(), Some("MY_KEY"));
    }
}
