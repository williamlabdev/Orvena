//! The **Aider** profile (differential plan §5: first adapter target — open
//! source, local, git-native, headless, and pinnable, so none of the reasons a
//! remote SaaS agent was rejected apply).
//!
//! Aider is driven in one-shot mode (`--message`) with everything that would
//! otherwise write to the project switched off, because those writes would be
//! indistinguishable from the agent editing the repo:
//!
//! - `--no-auto-commits` / `--no-dirty-commits` — an agent that commits its own
//!   work leaves a clean `git status`, which would make the violation oracle
//!   report "nothing changed". Its judgement must not depend on the agent's
//!   commit habits. (The oracle also diffs against the baseline *commit* now, so
//!   this is defence in depth, not the only guard.)
//! - `--no-gitignore` — Aider otherwise appends `.aider*` to `.gitignore`, an
//!   edit to a file no task ever declares.
//! - `--map-tokens 0` — the repo map caches tags under the project root.
//! - history files are redirected into the adapter's scratch directory.
//!
//! What is deliberately *not* configured: any Aider-side notion of read-only
//! files (`--read`). Scope is enforced by the OS, not requested from the agent —
//! asking politely is what the whole slice exists to stop relying on.

use std::path::Path;

use super::{AdapterSpec, AGENT_SCRATCH_DIR};
use crate::config::agent::ProviderSelection;
use crate::{Error, Result};

pub const NAME: &str = "aider";

/// Default endpoint for a local Ollama, matching `provider::ollama`'s own
/// default — the adapter must drive the *same* model the native loop would.
const OLLAMA_DEFAULT_BASE: &str = "http://127.0.0.1:11434";

/// Build the Aider invocation profile for `provider`.
///
/// `scratch` is the agent's bookkeeping directory *relative to the workdir* —
/// history files land there so they are inside the sandbox's writable set and
/// outside the oracle's field of view.
pub fn spec(provider: &ProviderSelection) -> Result<AdapterSpec> {
    let (model, env) = model_and_env(provider)?;
    let scratch = |name: &str| format!("{AGENT_SCRATCH_DIR}/{name}");
    Ok(AdapterSpec {
        name: NAME.into(),
        program: NAME.into(),
        args: vec![
            "--model".into(),
            model,
            // Headless: answer every prompt, run one message, exit.
            "--yes-always".into(),
            "--no-stream".into(),
            "--no-pretty".into(),
            "--no-check-update".into(),
            "--no-analytics".into(),
            // Nothing of Aider's own may land in the project (see module docs).
            "--no-auto-commits".into(),
            "--no-dirty-commits".into(),
            "--no-gitignore".into(),
            "--map-tokens".into(),
            "0".into(),
            "--chat-history-file".into(),
            scratch("chat.md"),
            "--input-history-file".into(),
            scratch("input"),
            "--llm-history-file".into(),
            scratch("llm"),
            "--message".into(),
            "{instruction}".into(),
            "{files}".into(),
        ],
        env,
        version_args: vec!["--version".into()],
        config_files: vec![],
        state_writable: vec![],
    })
}

/// Map Orvena's provider selection onto Aider's (LiteLLM-style) model name plus
/// whatever environment that provider needs. API keys are *not* copied here —
/// Aider reads the same `*_API_KEY` variables from the inherited environment, so
/// there is one place a key lives.
fn model_and_env(provider: &ProviderSelection) -> Result<(String, Vec<(String, String)>)> {
    let model = provider.model.clone();
    match provider.kind.as_str() {
        "ollama" => {
            let base = provider.base_url.clone().unwrap_or_else(|| OLLAMA_DEFAULT_BASE.into());
            // Ollama's OpenAI-compat suffix belongs to the native provider's
            // client, not to Aider's — strip it if the config carries one.
            let base = base.trim_end_matches('/').trim_end_matches("/v1").to_string();
            Ok((format!("ollama_chat/{model}"), vec![("OLLAMA_API_BASE".into(), base)]))
        }
        "openai" => {
            let mut env = Vec::new();
            if let Some(base) = &provider.base_url {
                env.push(("OPENAI_API_BASE".into(), base.clone()));
            }
            Ok((format!("openai/{model}"), env))
        }
        "anthropic" => Ok((format!("anthropic/{model}"), Vec::new())),
        "openrouter" => Ok((format!("openrouter/{model}"), Vec::new())),
        "offline" => Err(Error::Config(
            "the `offline` provider is a deterministic stub for the native loop — an external \
             agent brings its own model client and cannot be pointed at it. Use a real provider \
             (e.g. `--provider ollama`) for an adapter run"
                .into(),
        )),
        other => Err(Error::Config(format!(
            "provider '{other}' has no Aider model mapping (known: ollama, openai, openrouter, \
             anthropic)"
        ))),
    }
}

/// Give the agent its scratch directory as an absolute path when the caller
/// needs one (the spec itself uses workdir-relative paths, since Aider runs with
/// the workdir as its cwd).
pub fn scratch_dir(workdir: &Path) -> std::path::PathBuf {
    workdir.join(AGENT_SCRATCH_DIR)
}

/// Pull Aider's self-reported usage line out of a transcript:
/// `Tokens: 702 sent, 39 received.` (also `1.2k sent`, and trailing cost info).
///
/// These are **relayed, not observed** — Orvena makes no model call in an
/// adapter run. The caller records that provenance
/// ([`crate::metrics::TokenAccounting::AgentReported`]) so a cost comparison can
/// never silently mix a measured number with a claimed one. Aider prints one
/// such line per exchange; the last one is cumulative for the session.
pub fn parse_tokens(transcript: &str) -> Option<(u32, u32)> {
    let mut last = None;
    for line in transcript.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("Tokens:") else { continue };
        let (sent_part, received_part) = rest.split_once(',')?;
        let sent = parse_count(sent_part.trim().trim_end_matches("sent").trim())?;
        let received = parse_count(
            received_part.trim().trim_start_matches(' ').split("received").next()?.trim(),
        )?;
        last = Some((sent, received));
    }
    last
}

/// `"702"` → 702, `"1.2k"` → 1200, `"3k"` → 3000.
fn parse_count(s: &str) -> Option<u32> {
    let s = s.trim();
    match s.strip_suffix(['k', 'K']) {
        Some(num) => num.trim().parse::<f64>().ok().map(|v| (v * 1000.0).round() as u32),
        None => s.parse::<u32>().ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel(kind: &str, model: &str, base_url: Option<&str>) -> ProviderSelection {
        ProviderSelection {
            kind: kind.into(),
            model: model.into(),
            base_url: base_url.map(String::from),
            api_key_env: None,
            sampling: None,
        }
    }

    #[test]
    fn ollama_maps_to_ollama_chat_and_points_at_the_local_endpoint() {
        let s = spec(&sel("ollama", "qwen3:14b", None)).unwrap();
        let model = s.args.iter().position(|a| a == "--model").map(|i| &s.args[i + 1]).unwrap();
        assert_eq!(model, "ollama_chat/qwen3:14b");
        assert_eq!(s.env, vec![("OLLAMA_API_BASE".to_string(), OLLAMA_DEFAULT_BASE.to_string())]);
    }

    #[test]
    fn an_ollama_base_url_with_the_openai_compat_suffix_is_normalized() {
        // The native Ollama client may be configured with `/v1`; Aider wants the
        // bare host, so a config that works for one must not break the other.
        let s = spec(&sel("ollama", "m", Some("http://box:11434/v1"))).unwrap();
        assert_eq!(s.env[0].1, "http://box:11434");
    }

    #[test]
    fn hosted_providers_map_to_their_litellm_prefixes_without_copying_keys() {
        for (kind, want) in
            [("openai", "openai/m"), ("anthropic", "anthropic/m"), ("openrouter", "openrouter/m")]
        {
            let s = spec(&sel(kind, "m", None)).unwrap();
            let model = s.args.iter().position(|a| a == "--model").map(|i| &s.args[i + 1]).unwrap();
            assert_eq!(model, want);
            assert!(
                s.env.iter().all(|(k, _)| !k.contains("API_KEY")),
                "keys are inherited from the environment, never re-plumbed here"
            );
        }
    }

    #[test]
    fn an_openai_base_url_override_is_passed_through() {
        let s = spec(&sel("openai", "gemini-2.5-flash", Some("https://example/v1"))).unwrap();
        assert_eq!(s.env, vec![("OPENAI_API_BASE".to_string(), "https://example/v1".to_string())]);
    }

    #[test]
    fn the_offline_stub_is_refused_with_a_reason() {
        let err = spec(&sel("offline", "stub", None)).unwrap_err();
        assert!(matches!(err, Error::Config(_)));
        assert!(err.to_string().contains("external agent"), "{err}");
    }

    #[test]
    fn an_unknown_provider_is_a_config_error_not_a_guess() {
        let err = spec(&sel("mystery", "m", None)).unwrap_err();
        assert!(err.to_string().contains("no Aider model mapping"));
    }

    #[test]
    fn everything_that_would_write_to_the_project_is_switched_off() {
        let s = spec(&sel("ollama", "m", None)).unwrap();
        for flag in ["--no-auto-commits", "--no-dirty-commits", "--no-gitignore", "--yes-always"] {
            assert!(s.args.iter().any(|a| a == flag), "missing {flag}");
        }
        // History files land in the adapter's scratch dir, not the project.
        assert!(s.args.iter().any(|a| a == &format!("{AGENT_SCRATCH_DIR}/chat.md")));
        // Repo-map caching is off (it writes a tags cache under the root).
        let map = s.args.iter().position(|a| a == "--map-tokens").map(|i| &s.args[i + 1]).unwrap();
        assert_eq!(map, "0");
    }

    #[test]
    fn tokens_are_parsed_from_the_usage_line() {
        assert_eq!(parse_tokens("Tokens: 702 sent, 39 received.\n"), Some((702, 39)));
    }

    #[test]
    fn the_last_usage_line_wins_and_k_suffixes_expand() {
        let t =
            "Tokens: 700 sent, 30 received.\nfoo\nTokens: 1.2k sent, 2k received. Cost: $0.01\n";
        assert_eq!(parse_tokens(t), Some((1200, 2000)));
    }

    #[test]
    fn a_transcript_without_usage_yields_nothing_rather_than_zero() {
        // The difference matters: `None` means "unknown" and is recorded as
        // `TokenAccounting::Unavailable`; `Some((0, 0))` would claim the run was
        // free.
        assert_eq!(parse_tokens("Applied edit to src/a.rs\n"), None);
        assert_eq!(parse_tokens("Tokens: banana sent, 3 received.\n"), None);
    }
}
