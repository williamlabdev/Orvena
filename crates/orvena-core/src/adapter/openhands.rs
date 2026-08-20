//! The **OpenHands** profile — the second wrapped agent, chosen because its tool
//! surface is as unlike Aider's as an available agent gets.
//!
//! Aider edits the files handed to its chat and little else. Through the whole
//! 2026-08-03 matrix it never attempted a redirect, a copy, a rename or a
//! symlink, so the containment number that run produced recorded that nothing
//! attacked the sandbox rather than that the sandbox held. OpenHands drives a
//! shell and decides its own file set, which is the point: the guarantee is only
//! worth what has been thrown at it.
//!
//! ## The sandbox that must not be there
//!
//! OpenHands ships its own isolation and defaults to a Docker sandbox. Left on,
//! the agent would be contained by *its own* container and Orvena would be
//! measuring OpenHands' boundary while reporting its own — the one result worse
//! than no result. `RUNTIME=process` runs it as a plain local process, which
//! their docs describe as providing "no sandbox isolation": exactly right here,
//! because Orvena is the isolation under test and there must be only one.
//!
//! ## Keeping its state out of the project and out of the real home
//!
//! Two places OpenHands writes that neither the task nor the operator asked for:
//!
//! - `Path.home()/.openhands/cache` — hardcoded in `sdk/skills/utils.py`, with
//!   no environment variable to move it. Under the strict policy the real home
//!   is not writable, so this is not merely untidy, it is a hang: the startup
//!   path clones a public skills repository into that cache and blocks. Handing
//!   the agent a different `HOME` (see [`SCRATCH_PLACEHOLDER`]) puts it inside
//!   the writable set, which the violation oracle already excludes.
//! - `.openhands/` and `.agents/` under the *project* root, which it probes for
//!   skills. Reads are harmless; a write there would be an edit to a file no
//!   task ever declared, and is left to the sandbox and the oracle rather than
//!   asked for politely — the same posture as Aider's `--no-gitignore`.
//!
//! ## What is verified and what is not
//!
//! Flags below are read off `openhands --help` on SDK v1.21.0, and the
//! `--override-with-envs` requirement is real: without it "environment variables
//! are ignored" and the run would silently use whatever model was configured
//! last, which would make the two legs incomparable.
//!
//! Not yet verified against a live model: the startup skills clone still costs a
//! network round trip on every run, and no setting to disable it was found. If
//! that proves too slow, the fix is a pre-warmed `HOME` shared across runs
//! rather than reaching for the project root.

use super::{AdapterSpec, AGENT_SCRATCH_DIR, SCRATCH_PLACEHOLDER};
use crate::config::agent::ProviderSelection;
use crate::{Error, Result};

pub const NAME: &str = "openhands";

/// Default endpoint for a local Ollama, matching `provider::ollama`'s own
/// default — the adapter must drive the *same* model the native loop would.
const OLLAMA_DEFAULT_BASE: &str = "http://127.0.0.1:11434";

/// Build the OpenHands invocation profile for `provider`.
pub fn spec(provider: &ProviderSelection) -> Result<AdapterSpec> {
    let (model, base_url, api_key) = model_and_endpoint(provider)?;

    let mut env = vec![
        // Its own isolation must be off; Orvena is the boundary under test.
        ("RUNTIME".to_string(), "process".to_string()),
        // Otherwise the LLM_* variables below are ignored and the run uses
        // whatever was configured last — a silently different model on one leg.
        ("LLM_MODEL".to_string(), model),
        ("LLM_API_KEY".to_string(), api_key),
        // Cache and skills state land in the scratch dir, not the real home.
        ("HOME".to_string(), SCRATCH_PLACEHOLDER.to_string()),
        // A banner in the transcript is a banner in the refusal parser's input.
        ("OPENHANDS_SUPPRESS_BANNER".to_string(), "1".to_string()),
    ];
    if let Some(base) = base_url {
        env.push(("LLM_BASE_URL".to_string(), base));
    }

    Ok(AdapterSpec {
        name: NAME.into(),
        program: NAME.into(),
        args: vec![
            "--headless".into(),
            // Without this the LLM_* environment is ignored (see module docs).
            "--override-with-envs".into(),
            "--exit-without-confirmation".into(),
            "-t".into(),
            "{instruction}".into(),
            // Deliberately no `{files}`: OpenHands chooses its own file set, and
            // the scope contract reaches it through the composed message the
            // same way it reaches the native loop.
        ],
        env,
        version_args: vec!["--version".into()],
        config_files: vec![],
        state_writable: vec![],
    })
}

/// Map Orvena's provider selection onto the `LLM_MODEL` / `LLM_BASE_URL` pair.
///
/// OpenHands routes through LiteLLM, so a local Ollama is addressed as an
/// OpenAI-compatible endpoint (`openai/<model>` against `…/v1`) rather than
/// through a native Ollama provider — their own local-model guide says so, and
/// the `ollama/` prefix drives a different code path.
fn model_and_endpoint(provider: &ProviderSelection) -> Result<(String, Option<String>, String)> {
    let model = provider.model.clone();
    match provider.kind.as_str() {
        "ollama" => {
            let base = provider.base_url.clone().unwrap_or_else(|| OLLAMA_DEFAULT_BASE.into());
            let base = base.trim_end_matches('/').trim_end_matches("/v1").to_string();
            Ok((
                format!("openai/{model}"),
                Some(format!("{base}/v1")),
                // LiteLLM requires *a* key against an OpenAI-compatible endpoint;
                // a local Ollama ignores it. Naming it plainly beats a value that
                // looks like it might have been a real key.
                "local-no-key-required".into(),
            ))
        }
        "openai" => Ok((format!("openai/{model}"), provider.base_url.clone(), inherited_key())),
        "anthropic" => Ok((format!("anthropic/{model}"), None, inherited_key())),
        "openrouter" => Ok((format!("openrouter/{model}"), None, inherited_key())),
        "offline" => Err(Error::Config(
            "the `offline` provider is a deterministic stub for the native loop — an external \
             agent brings its own model client and cannot be pointed at it. Use a real provider \
             (e.g. `--provider ollama`) for an adapter run"
                .into(),
        )),
        other => Err(Error::Config(format!(
            "provider '{other}' has no OpenHands model mapping (known: ollama, openai, \
             openrouter, anthropic)"
        ))),
    }
}

/// The key comes from the inherited environment, so there is one place a key
/// lives — the same rule the Aider profile follows. An empty value here means
/// "not set in this environment", and OpenHands will say so louder than a
/// fabricated placeholder would.
fn inherited_key() -> String {
    std::env::var("LLM_API_KEY").unwrap_or_default()
}

/// The agent's scratch directory as an absolute path, for callers that need one.
pub fn scratch_dir(workdir: &std::path::Path) -> std::path::PathBuf {
    workdir.join(AGENT_SCRATCH_DIR)
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

    fn env_of(s: &AdapterSpec, key: &str) -> Option<String> {
        s.env.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    #[test]
    fn its_own_sandbox_is_switched_off_so_orvena_is_the_only_boundary() {
        // The whole measurement depends on this: contained by its own container,
        // OpenHands would pass every containment assertion Orvena makes without
        // Orvena having done anything.
        let s = spec(&sel("ollama", "qwen3:14b", None)).unwrap();
        assert_eq!(env_of(&s, "RUNTIME").as_deref(), Some("process"));
    }

    #[test]
    fn the_environment_override_flag_is_present_or_the_model_is_whatever_was_last_configured() {
        let s = spec(&sel("ollama", "m", None)).unwrap();
        assert!(
            s.args.iter().any(|a| a == "--override-with-envs"),
            "without this the LLM_* variables are ignored and the two legs run different models"
        );
        assert!(s.args.iter().any(|a| a == "--headless"));
    }

    #[test]
    fn home_is_redirected_into_the_scratch_dir_rather_than_the_operators_own() {
        // `Path.home()/.openhands/cache` is hardcoded upstream, and under the
        // strict policy the real home is not writable — so this is a hang, not
        // just untidiness.
        assert_eq!(
            env_of(&spec(&sel("ollama", "m", None)).unwrap(), "HOME").as_deref(),
            Some(SCRATCH_PLACEHOLDER)
        );
    }

    #[test]
    fn ollama_is_addressed_as_an_openai_compatible_endpoint() {
        let s = spec(&sel("ollama", "qwen3:14b", None)).unwrap();
        assert_eq!(env_of(&s, "LLM_MODEL").as_deref(), Some("openai/qwen3:14b"));
        assert_eq!(env_of(&s, "LLM_BASE_URL").as_deref(), Some("http://127.0.0.1:11434/v1"));
    }

    #[test]
    fn an_ollama_base_url_carrying_the_compat_suffix_is_not_doubled() {
        let s = spec(&sel("ollama", "m", Some("http://box:11434/v1"))).unwrap();
        assert_eq!(env_of(&s, "LLM_BASE_URL").as_deref(), Some("http://box:11434/v1"));
    }

    #[test]
    fn the_file_list_is_not_passed_because_openhands_picks_its_own() {
        let s = spec(&sel("ollama", "m", None)).unwrap();
        assert!(
            !s.args.iter().any(|a| a == "{files}"),
            "the scope contract reaches it through the composed message instead"
        );
        assert!(s.args.iter().any(|a| a == "{instruction}"));
    }

    #[test]
    fn the_offline_stub_is_refused_with_a_reason() {
        let err = spec(&sel("offline", "stub", None)).unwrap_err();
        assert!(err.to_string().contains("external agent"), "{err}");
    }

    #[test]
    fn an_unknown_provider_is_a_config_error_not_a_guess() {
        let err = spec(&sel("mystery", "m", None)).unwrap_err();
        assert!(err.to_string().contains("no OpenHands model mapping"));
    }
}
