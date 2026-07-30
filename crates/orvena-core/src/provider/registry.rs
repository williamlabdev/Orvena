//! Provider registry: the known providers, their env-key mapping, and a
//! readiness check (used by `orvena doctor`). No provider is poked silently —
//! readiness only *reports*; it never picks a default.

use crate::config::agent::ProviderSelection;

/// Static facts about a known provider.
pub struct ProviderInfo {
    pub kind: &'static str,
    /// Env var this kind's builder reads the API key from (`None` for
    /// local/offline kinds that need no key, and for `openai_compat` whose key
    /// var is named at config time via `ProviderSelection.api_key_env`).
    pub env_key: Option<&'static str>,
    pub description: &'static str,
    /// Whether a base_url is typically needed (e.g. local Ollama).
    pub local: bool,
    /// Whether `orvena init` must prompt for (and `orvena.yaml` must set) an
    /// explicit `base_url` — there is no sane default endpoint to fall back to.
    pub requires_base_url: bool,
    /// Whether this kind's **builder** actually reads `api_key_env`. Only the
    /// kinds routed to `OpenAiCompat` do. Readiness must not honor the field
    /// for the others: `Anthropic::from_env` hardcodes `ANTHROPIC_API_KEY` and
    /// `Ollama`/`Offline` read no key at all, so treating `api_key_env` as
    /// authoritative there makes this check disagree with `build_chat_provider`
    /// — `doctor` reporting ready on a config that cannot build, or blocking
    /// one that can.
    pub honors_api_key_env: bool,
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
            honors_api_key_env: false,
        },
        ProviderInfo {
            kind: "openai",
            env_key: Some("OPENAI_API_KEY"),
            description: "OpenAI (hosted).",
            local: false,
            requires_base_url: false,
            honors_api_key_env: true,
        },
        ProviderInfo {
            kind: "openrouter",
            env_key: Some("OPENROUTER_API_KEY"),
            description: "OpenRouter (hosted) — one key, many models.",
            local: false,
            requires_base_url: false,
            honors_api_key_env: true,
        },
        ProviderInfo {
            kind: "ollama",
            env_key: None,
            description: "Local via Ollama (offline/private). You run Ollama yourself.",
            local: true,
            requires_base_url: false,
            honors_api_key_env: false,
        },
        ProviderInfo {
            kind: "openai_compat",
            env_key: None,
            description: "Generic OpenAI-compatible endpoint — self-hosted OSS servers \
                (vLLM, llama.cpp server, LM Studio, TGI, SGLang) or hosted open-weight \
                aggregators (Groq, Together, Fireworks). Needs base_url; API key optional.",
            local: true,
            requires_base_url: true,
            honors_api_key_env: true,
        },
        ProviderInfo {
            kind: "offline",
            env_key: None,
            description: "Deterministic stub for tests and L1 baselines (no network).",
            local: true,
            requires_base_url: false,
            honors_api_key_env: false,
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
    /// Kind requires an explicit `base_url` and the config has none. Checked
    /// here so `doctor`/preflight catch it, rather than the run dead-ending in
    /// `build_chat_provider` after setup looked green.
    MissingBaseUrl,
    /// Provider kind is not known to orvena.
    Unknown,
}

/// Which env var, if any, this selection's key must come from — the single
/// place that decides it, so readiness cannot disagree with the builder.
///
/// `api_key_env` is honored **only** for kinds whose builder reads it
/// (`ProviderInfo::honors_api_key_env`). Applying it to `anthropic` would let
/// readiness pass on a var that `Anthropic::from_env` never looks at; applying
/// it to `ollama`/`offline` would demand a key for providers that use none.
fn key_var<'a>(p: &'a ProviderInfo, sel: &'a ProviderSelection) -> Option<&'a str> {
    if p.honors_api_key_env {
        sel.api_key_env.as_deref().or(p.env_key)
    } else {
        p.env_key
    }
}

/// Check readiness without making a network call. (Connectivity probes are a
/// separate, explicit step in `doctor`.)
pub fn readiness(sel: &ProviderSelection) -> Readiness {
    let Some(p) = info(&sel.kind) else {
        return Readiness::Unknown;
    };
    if p.requires_base_url && sel.base_url.as_deref().is_none_or(|u| u.trim().is_empty()) {
        return Readiness::MissingBaseUrl;
    }
    match key_var(&p, sel) {
        None => Readiness::Ready,
        Some(key) => match std::env::var(key) {
            Ok(v) if !v.trim().is_empty() => Readiness::Ready,
            _ => Readiness::MissingKey(key.to_string()),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel(kind: &str, api_key_env: Option<&str>, base_url: Option<&str>) -> ProviderSelection {
        ProviderSelection {
            kind: kind.into(),
            model: "m".into(),
            base_url: base_url.map(Into::into),
            api_key_env: api_key_env.map(Into::into),
        }
    }

    // Regression: `api_key_env` on a kind whose builder ignores it must not
    // steer readiness — `Anthropic::from_env` hardcodes ANTHROPIC_API_KEY, so
    // reporting ready on some other var made `doctor` green on a config that
    // then died with "ANTHROPIC_API_KEY is not set".
    #[test]
    fn api_key_env_does_not_steer_kinds_whose_builder_ignores_it() {
        let anthropic = info("anthropic").unwrap();
        assert_eq!(
            key_var(&anthropic, &sel("anthropic", Some("MY_KEY"), None)),
            Some("ANTHROPIC_API_KEY")
        );
        for kind in ["ollama", "offline"] {
            let p = info(kind).unwrap();
            assert_eq!(key_var(&p, &sel(kind, Some("MY_KEY"), None)), None, "{kind} needs no key");
        }
    }

    // Regression: a stale `api_key_env` used to make the keyless `offline`
    // stub unreachable — and the resulting error advised `--provider offline`,
    // the very thing it was blocking.
    #[test]
    fn offline_stays_ready_despite_a_stale_api_key_env() {
        assert_eq!(
            readiness(&sel("offline", Some("ORVENA_TEST_UNSET_VAR"), None)),
            Readiness::Ready
        );
        assert_eq!(
            readiness(&sel("ollama", Some("ORVENA_TEST_UNSET_VAR"), None)),
            Readiness::Ready
        );
    }

    // The compat family is exactly where the field is meant to work.
    #[test]
    fn openai_compat_family_honors_api_key_env() {
        for kind in ["openai", "openrouter", "openai_compat"] {
            let p = info(kind).unwrap();
            assert_eq!(
                key_var(&p, &sel(kind, Some("MY_KEY"), Some("http://x/v1"))),
                Some("MY_KEY"),
                "{kind} routes to OpenAiCompat, which reads api_key_env"
            );
        }
    }

    // Regression: doctor used to print "All checks passed" for an
    // openai_compat config with no base_url, which cannot build.
    #[test]
    fn missing_base_url_is_caught_by_preflight_not_at_build_time() {
        assert_eq!(readiness(&sel("openai_compat", None, None)), Readiness::MissingBaseUrl);
        assert_eq!(readiness(&sel("openai_compat", None, Some("   "))), Readiness::MissingBaseUrl);
        // Kinds with a sane default endpoint are unaffected.
        assert_ne!(readiness(&sel("ollama", None, None)), Readiness::MissingBaseUrl);
    }

    #[test]
    fn unknown_kind_still_reports_unknown() {
        assert_eq!(readiness(&sel("nope", None, None)), Readiness::Unknown);
    }
}
