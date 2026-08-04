//! The **Continue** profile (`cn`) — the third wrapped agent, and the one that
//! forced [`AdapterSpec::config_files`] to exist.
//!
//! Continue selects its model through a `config.yaml` and offers no flag and no
//! environment variable for it. An adapter that could only pass arguments would
//! have had to accept whatever model the operator happened to have configured,
//! and a differential whose two legs quietly drove different models measures
//! nothing. So the profile writes its own config into the agent scratch
//! directory and points `--config` at it: the model is pinned by the same
//! `ProviderSelection` that drives every other leg, and the file lands somewhere
//! already writable under the strict policy and already excluded by the
//! violation oracle.
//!
//! The module is `continue_cli` rather than `continue` because the latter is a
//! Rust keyword; the agent's name on the CLI is still `--agent continue`.
//!
//! ## Flags
//!
//! Verified against `cn --help` on 1.5.47:
//!
//! - `-p` / `--print` — headless: run the prompt, print the response, exit.
//! - `--auto` — "auto mode (all tools allowed)". The blanket approval. Without
//!   it a headless run stalls on the first tool it wants to use, and a stalled
//!   agent would score as governance costing the task.
//! - `--config <path>` — the generated config above.
//! - The prompt is positional, which is why `{instruction}` is a bare argument.
//!
//! Deliberately **not** used: `--silent`, which strips `<think>` blocks and
//! excess whitespace. It would tidy the transcript of a reasoning model, and the
//! transcript is exactly what `refusal_lines` reads to find out what the sandbox
//! refused. A quieter log is not worth a refusal that goes unrecorded.

use super::{AdapterSpec, AGENT_SCRATCH_DIR};
use crate::config::agent::ProviderSelection;
use crate::{Error, Result};

pub const NAME: &str = "continue";

/// Filename of the generated config, relative to the agent scratch directory.
const CONFIG_FILE: &str = "continue-config.yaml";

/// Default endpoint for a local Ollama, matching `provider::ollama`'s own
/// default — the adapter must drive the *same* model the native loop would.
const OLLAMA_DEFAULT_BASE: &str = "http://127.0.0.1:11434";

/// Build the Continue invocation profile for `provider`.
pub fn spec(provider: &ProviderSelection) -> Result<AdapterSpec> {
    let config = config_yaml(provider)?;
    Ok(AdapterSpec {
        name: NAME.into(),
        program: "cn".into(),
        args: vec![
            "-p".into(),
            "--auto".into(),
            "--config".into(),
            format!("{AGENT_SCRATCH_DIR}/{CONFIG_FILE}"),
            // Positional prompt. No `{files}`: Continue chooses its own file
            // set, and the scope contract reaches it through the composed
            // message the same way it reaches the native loop.
            "{instruction}".into(),
        ],
        env: vec![],
        version_args: vec!["--version".into()],
        config_files: vec![(CONFIG_FILE.to_string(), config)],
    })
}

/// The generated `config.yaml`, pinning the model the rest of the matrix runs.
///
/// `roles` has to list `chat`, `edit` and `apply`: a model registered for chat
/// alone is not offered for the edits this benchmark is entirely about, and the
/// agent would read as unable to do the task rather than as contained.
fn config_yaml(provider: &ProviderSelection) -> Result<String> {
    let (kind, model, api_base) = match provider.kind.as_str() {
        "ollama" => {
            let base = provider.base_url.clone().unwrap_or_else(|| OLLAMA_DEFAULT_BASE.into());
            // Continue's ollama provider wants the bare host; the native
            // client's config may carry the OpenAI-compat suffix.
            let base = base.trim_end_matches('/').trim_end_matches("/v1").to_string();
            ("ollama", provider.model.clone(), Some(base))
        }
        "openai" => ("openai", provider.model.clone(), provider.base_url.clone()),
        "anthropic" => ("anthropic", provider.model.clone(), None),
        "openrouter" => ("openrouter", provider.model.clone(), None),
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
                "provider '{other}' has no Continue model mapping (known: ollama, openai, \
                 openrouter, anthropic)"
            )))
        }
    };

    let mut yaml =
        String::from("name: orvena-bench\nversion: 0.0.1\nschema: v1\nmodels:\n  - name: bench\n");
    yaml.push_str(&format!("    provider: {kind}\n"));
    yaml.push_str(&format!("    model: {model}\n"));
    if let Some(base) = api_base {
        yaml.push_str(&format!("    apiBase: {base}\n"));
    }
    // The key is read from the inherited environment rather than baked into a
    // file on disk, so there stays one place a key lives.
    if let Some(var) = &provider.api_key_env {
        yaml.push_str(&format!("    apiKey: ${{{{ env.{var} }}}}\n"));
    }
    yaml.push_str("    roles:\n      - chat\n      - edit\n      - apply\n");
    Ok(yaml)
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
        }
    }

    #[test]
    fn the_model_is_pinned_by_a_generated_config_not_inherited_from_the_operator() {
        // The reason this profile needs `config_files` at all: without it the
        // run would use whatever `~/.continue/config.yaml` happened to say, and
        // the two legs could be driving different models.
        let s = spec(&sel("ollama", "qwen3:14b", None)).unwrap();
        let (path, body) = &s.config_files[0];
        assert_eq!(path, CONFIG_FILE);
        assert!(body.contains("provider: ollama"), "{body}");
        assert!(body.contains("model: qwen3:14b"), "{body}");
        assert!(body.contains("apiBase: http://127.0.0.1:11434"), "{body}");
        assert!(
            s.args.iter().any(|a| a == &format!("{AGENT_SCRATCH_DIR}/{CONFIG_FILE}")),
            "the generated config must be the one actually passed to --config"
        );
    }

    #[test]
    fn the_model_is_registered_for_editing_and_not_only_for_chat() {
        // Registered for chat alone, it is never offered for the edits this
        // benchmark exists to measure, and reads as incapable rather than
        // contained.
        let s = spec(&sel("ollama", "m", None)).unwrap();
        let body = &s.config_files[0].1;
        for role in ["chat", "edit", "apply"] {
            assert!(body.contains(&format!("- {role}")), "missing role {role} in:\n{body}");
        }
    }

    #[test]
    fn every_tool_is_pre_approved_or_a_headless_run_stalls_on_the_first_one() {
        let s = spec(&sel("ollama", "m", None)).unwrap();
        assert!(s.args.iter().any(|a| a == "--auto"));
        assert!(s.args.iter().any(|a| a == "-p"));
    }

    #[test]
    fn the_transcript_is_left_noisy_so_refusals_survive_to_be_parsed() {
        let s = spec(&sel("ollama", "m", None)).unwrap();
        assert!(
            !s.args.iter().any(|a| a == "--silent"),
            "--silent strips model output, and the transcript is what refusal parsing reads"
        );
    }

    #[test]
    fn an_ollama_base_url_with_the_openai_compat_suffix_is_normalized() {
        let s = spec(&sel("ollama", "m", Some("http://box:11434/v1"))).unwrap();
        assert!(
            s.config_files[0].1.contains("apiBase: http://box:11434\n"),
            "{}",
            s.config_files[0].1
        );
    }

    #[test]
    fn a_key_is_referenced_by_variable_rather_than_written_into_the_file() {
        let mut p = sel("openai", "gpt-x", None);
        p.api_key_env = Some("MY_KEY".into());
        let s = spec(&p).unwrap();
        let body = &s.config_files[0].1;
        assert!(body.contains("env.MY_KEY"), "{body}");
        assert!(!body.contains("sk-"), "no key material may be written to disk");
    }

    #[test]
    fn the_offline_stub_is_refused_with_a_reason() {
        let err = spec(&sel("offline", "stub", None)).unwrap_err();
        assert!(err.to_string().contains("external agent"), "{err}");
    }

    #[test]
    fn an_unknown_provider_is_a_config_error_not_a_guess() {
        let err = spec(&sel("mystery", "m", None)).unwrap_err();
        assert!(err.to_string().contains("no Continue model mapping"));
    }
}
