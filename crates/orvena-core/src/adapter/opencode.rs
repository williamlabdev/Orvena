//! The **OpenCode** profile — a fourth tool surface, and the cheapest one to
//! add because everything it needs is a documented switch.
//!
//! Its provider block is generated into the agent scratch directory and handed
//! over through `OPENCODE_CONFIG`, for the reason the Continue profile spells
//! out: a model the operator happened to configure is not the model the rest of
//! the matrix is running, and a differential whose legs drive different models
//! measures nothing. The format below was verified by generating it and reading
//! `opencode models` back — `ollama/qwen3:14b` appears, so the provider really
//! is registered rather than merely accepted.
//!
//! ## Flags
//!
//! Verified against `opencode --help` on 1.18.12:
//!
//! - `run <message>` — non-interactive, message positional.
//! - `--auto` — "auto-approve permissions that are not explicitly denied". The
//!   blanket approval; without it a headless run stalls on the first tool and
//!   the stall would score as governance costing the task.
//! - `-m <provider/model>` — selects from the generated config.
//! - `--pure` — "run without external plugins". A benchmark has to be pinnable,
//!   and an agent that loads whatever plugins the host happens to have is not
//!   the same agent twice.
//!
//! ## Keeping its state out of the project
//!
//! `OPENCODE_CONFIG_DIR` and `OPENCODE_DB` are pointed at the scratch directory
//! so its config and session database land somewhere already writable and
//! already excluded by the oracle. `OPENCODE_DISABLE_AUTOUPDATE` matters more
//! than it looks: an agent that updates itself partway through a matrix is a
//! moving target, and the run would no longer be measuring one version.

use super::{AdapterSpec, AGENT_SCRATCH_DIR, SCRATCH_PLACEHOLDER};
use crate::config::agent::ProviderSelection;
use crate::{Error, Result};

pub const NAME: &str = "opencode";

/// Filename of the generated config, relative to the agent scratch directory.
const CONFIG_FILE: &str = "opencode.json";

/// Default endpoint for a local Ollama, matching `provider::ollama`'s own
/// default — the adapter must drive the *same* model the native loop would.
const OLLAMA_DEFAULT_BASE: &str = "http://127.0.0.1:11434";

/// Build the OpenCode invocation profile for `provider`.
pub fn spec(provider: &ProviderSelection) -> Result<AdapterSpec> {
    let (qualified_model, config) = config_json(provider)?;
    Ok(AdapterSpec {
        name: NAME.into(),
        program: NAME.into(),
        args: vec![
            "run".into(),
            "--auto".into(),
            "--pure".into(),
            "--model".into(),
            qualified_model,
            // Positional message. No `{files}`: OpenCode chooses its own file
            // set, and the scope contract reaches it through the composed
            // message the same way it reaches the native loop.
            "{instruction}".into(),
        ],
        env: vec![
            ("OPENCODE_CONFIG".to_string(), format!("{SCRATCH_PLACEHOLDER}/{CONFIG_FILE}")),
            ("OPENCODE_CONFIG_DIR".to_string(), format!("{SCRATCH_PLACEHOLDER}/opencode")),
            ("OPENCODE_DB".to_string(), format!("{SCRATCH_PLACEHOLDER}/opencode/db")),
            // A benchmark measures one version of one agent.
            ("OPENCODE_DISABLE_AUTOUPDATE".to_string(), "1".to_string()),
        ],
        version_args: vec!["--version".into()],
        config_files: vec![(CONFIG_FILE.to_string(), config)],
        state_writable: vec![],
    })
}

/// The generated provider block, plus the `provider/model` string that selects
/// from it.
///
/// A local Ollama is registered as an OpenAI-compatible provider, which is how
/// OpenCode takes a local endpoint.
fn config_json(provider: &ProviderSelection) -> Result<(String, String)> {
    let model = provider.model.clone();
    let (provider_id, base) = match provider.kind.as_str() {
        "ollama" => {
            let base = provider.base_url.clone().unwrap_or_else(|| OLLAMA_DEFAULT_BASE.into());
            // The native client's config may carry the OpenAI-compat suffix;
            // OpenCode wants it present exactly once.
            let base = base.trim_end_matches('/').trim_end_matches("/v1").to_string();
            ("ollama", format!("{base}/v1"))
        }
        "openai" => (
            "openai-compat",
            provider.base_url.clone().unwrap_or_else(|| "https://api.openai.com/v1".into()),
        ),
        "offline" => {
            return Err(Error::Config(
                "the `offline` provider is a deterministic stub for the native loop — an \
                 external agent brings its own model client and cannot be pointed at it. Use a \
                 real provider (e.g. `--provider ollama`) for an adapter run"
                    .into(),
            ))
        }
        other => {
            return Err(Error::Config(format!(
                "provider '{other}' has no OpenCode model mapping (known: ollama, openai)"
            )))
        }
    };

    // Written by hand rather than through a serializer: the shape is small, and
    // a literal here is easier to check against OpenCode's schema than a derive
    // three files away.
    let config = format!(
        "{{\n  \"$schema\": \"https://opencode.ai/config.json\",\n  \"provider\": {{\n    \
         \"{provider_id}\": {{\n      \"npm\": \"@ai-sdk/openai-compatible\",\n      \
         \"name\": \"orvena-bench\",\n      \"options\": {{ \"baseURL\": \"{base}\" }},\n      \
         \"models\": {{ \"{model}\": {{ \"name\": \"{model}\" }} }}\n    }}\n  }}\n}}\n"
    );
    Ok((format!("{provider_id}/{model}"), config))
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
    fn the_generated_config_is_valid_json_and_registers_the_requested_model() {
        let s = spec(&sel("ollama", "qwen3:14b", None)).unwrap();
        let (path, body) = &s.config_files[0];
        assert_eq!(path, CONFIG_FILE);
        let parsed: serde_json::Value = serde_json::from_str(body).expect("valid JSON");
        let p = &parsed["provider"]["ollama"];
        assert_eq!(p["options"]["baseURL"], "http://127.0.0.1:11434/v1");
        assert!(p["models"]["qwen3:14b"].is_object(), "model must be registered: {body}");
    }

    #[test]
    fn the_model_argument_names_the_provider_the_config_defines() {
        // `-m` takes `provider/model`; a mismatch here selects nothing and the
        // agent fails for a reason that looks like the task's fault.
        let s = spec(&sel("ollama", "qwen3:14b", None)).unwrap();
        let m = s.args.iter().position(|a| a == "--model").map(|i| &s.args[i + 1]).unwrap();
        assert_eq!(m, "ollama/qwen3:14b");
        let body = &s.config_files[0].1;
        assert!(body.contains("\"ollama\""), "{body}");
    }

    #[test]
    fn the_config_is_handed_over_by_environment_and_points_at_the_generated_file() {
        let s = spec(&sel("ollama", "m", None)).unwrap();
        assert_eq!(
            env_of(&s, "OPENCODE_CONFIG").as_deref(),
            Some(format!("{SCRATCH_PLACEHOLDER}/{CONFIG_FILE}").as_str())
        );
    }

    #[test]
    fn every_tool_is_pre_approved_and_no_host_plugins_are_loaded() {
        let s = spec(&sel("ollama", "m", None)).unwrap();
        assert!(s.args.iter().any(|a| a == "--auto"), "a headless run stalls without it");
        assert!(s.args.iter().any(|a| a == "--pure"), "host plugins would make the run unpinnable");
        assert_eq!(s.args[0], "run");
    }

    #[test]
    fn it_does_not_update_itself_partway_through_a_matrix() {
        let s = spec(&sel("ollama", "m", None)).unwrap();
        assert_eq!(env_of(&s, "OPENCODE_DISABLE_AUTOUPDATE").as_deref(), Some("1"));
    }

    #[test]
    fn its_state_lands_in_the_scratch_dir_rather_than_the_project() {
        let s = spec(&sel("ollama", "m", None)).unwrap();
        for key in ["OPENCODE_CONFIG", "OPENCODE_CONFIG_DIR", "OPENCODE_DB"] {
            let v = env_of(&s, key).unwrap();
            assert!(v.starts_with(SCRATCH_PLACEHOLDER), "{key} escapes the scratch dir: {v}");
        }
    }

    #[test]
    fn an_ollama_base_url_with_the_compat_suffix_is_not_doubled() {
        let s = spec(&sel("ollama", "m", Some("http://box:11434/v1"))).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&s.config_files[0].1).unwrap();
        assert_eq!(parsed["provider"]["ollama"]["options"]["baseURL"], "http://box:11434/v1");
    }

    #[test]
    fn the_offline_stub_is_refused_with_a_reason() {
        let err = spec(&sel("offline", "stub", None)).unwrap_err();
        assert!(err.to_string().contains("external agent"), "{err}");
    }

    #[test]
    fn an_unknown_provider_is_a_config_error_not_a_guess() {
        let err = spec(&sel("mystery", "m", None)).unwrap_err();
        assert!(err.to_string().contains("no OpenCode model mapping"));
    }
}
