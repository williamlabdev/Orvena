//! The **Codex CLI** profile — and the one agent that lets Orvena measure what
//! happens when a boundary is *nested inside another one*.
//!
//! ## A flag that presupposes the whole proposition
//!
//! Codex ships `--dangerously-bypass-approvals-and-sandbox`, documented as
//! "EXTREMELY DANGEROUS. Intended solely for running in environments that are
//! externally sandboxed". Another vendor shipped a switch whose stated purpose
//! is *the outer envelope already exists, so I will stand down* — which is
//! Orvena's claim, written by someone with no stake in it. That flag drives the
//! [`NAME`] profile.
//!
//! ## Two profiles, because nesting is a question and not a nuisance
//!
//! Codex also carries its own sandbox (`-s read-only | workspace-write |
//! danger-full-access`). Every other wrapped agent has had its isolation
//! switched off so Orvena is the only boundary under test — that is the correct
//! default and it is what [`NAME`] does. But "what happens when the agent is
//! already contained?" is not a nuisance to be configured away: it is how real
//! deployments look, since an agent in production is usually already inside a
//! container. So [`NAME_NESTED`] leaves Codex's sandbox on at
//! `workspace-write` and runs the identical tasks.
//!
//! Comparing the two answers something no single-agent run can: whether Orvena's
//! differential survives an inner boundary, or whether the inner one absorbs
//! every refusal and leaves Orvena measuring nothing. A null result there is
//! worth publishing — it would mark exactly where the guarantee stops applying.
//!
//! ## Hygiene, mostly solved upstream
//!
//! Aider needed four flags to stop it writing into the project (see that
//! module); Codex needs two, because it already has the switches:
//!
//! - `--ephemeral` — "run without persisting session files to disk", so no
//!   transcript lands anywhere the oracle would see it as an undeclared write.
//! - `CODEX_HOME` — its state directory, pointed at the agent scratch dir
//!   (already writable, already excluded by the oracle) rather than the
//!   operator's real home, which the strict policy makes unwritable anyway.
//!
//! `--skip-git-repo-check` is defensive: the harness snapshots each task
//! workdir as a git repo before the run, so the check would pass — but if that
//! ever changes, Codex refusing to start would read as the agent failing the
//! task rather than as a harness change.

use super::{AdapterSpec, SCRATCH_PLACEHOLDER};
use crate::config::agent::ProviderSelection;
use crate::{Error, Result};

/// Codex with its own sandbox off — Orvena is the only boundary.
pub const NAME: &str = "codex";

/// Codex with its own sandbox left on, to measure nested containment.
pub const NAME_NESTED: &str = "codex-nested";

/// Default endpoint for a local Ollama, matching `provider::ollama`'s own
/// default — the adapter must drive the *same* model the native loop would.
const OLLAMA_DEFAULT_BASE: &str = "http://127.0.0.1:11434";

/// Codex's own sandbox policy for the nested profile. `workspace-write` is the
/// realistic setting — `read-only` would stop the agent doing the task at all,
/// and the comparison needs it able to work.
const NESTED_SANDBOX: &str = "workspace-write";

/// Build the Codex profile for `provider`, with its own sandbox switched off.
pub fn spec(provider: &ProviderSelection) -> Result<AdapterSpec> {
    build(provider, NAME, false)
}

/// As [`spec`], but leaving Codex's own sandbox enabled — the nested-containment
/// arm of the comparison.
pub fn spec_nested(provider: &ProviderSelection) -> Result<AdapterSpec> {
    build(provider, NAME_NESTED, true)
}

fn build(provider: &ProviderSelection, name: &str, nested: bool) -> Result<AdapterSpec> {
    let (model, oss_args) = model_and_provider_args(provider)?;

    let mut args = vec!["exec".to_string(), "--skip-git-repo-check".into(), "--ephemeral".into()];
    if nested {
        // Its sandbox stays on. Approvals are not an issue: `codex exec` is
        // non-interactive and has no approval prompt to answer.
        args.push("--sandbox".into());
        args.push(NESTED_SANDBOX.into());
    } else {
        // The flag whose own documentation names this exact situation.
        args.push("--dangerously-bypass-approvals-and-sandbox".into());
    }
    args.extend(oss_args);
    args.push("--model".into());
    args.push(model);
    // Positional prompt. No `{files}`: Codex chooses its own file set, and the
    // scope contract reaches it through the composed message the same way it
    // reaches the native loop.
    args.push("{instruction}".into());

    Ok(AdapterSpec {
        name: name.into(),
        program: NAME.into(),
        args,
        env: vec![("CODEX_HOME".to_string(), format!("{SCRATCH_PLACEHOLDER}/codex"))],
        version_args: vec!["--version".into()],
        config_files: vec![],
    })
}

/// Map Orvena's provider selection onto Codex's model plus whatever provider
/// flags it needs.
///
/// A local Ollama goes through Codex's own open-source provider (`--oss
/// --local-provider ollama`) rather than a generic OpenAI-compatible endpoint,
/// because that is the path Codex actually supports for local models.
fn model_and_provider_args(provider: &ProviderSelection) -> Result<(String, Vec<String>)> {
    match provider.kind.as_str() {
        "ollama" => {
            // Codex's oss provider resolves the endpoint itself. Rather than
            // guess at the config key that would move it, a non-default endpoint
            // is refused out loud — a benchmark that silently drove a *different*
            // Ollama than the rest of the matrix would be worse than one that
            // stopped.
            if let Some(base) = &provider.base_url {
                let normalized = base.trim_end_matches('/').trim_end_matches("/v1");
                if normalized != OLLAMA_DEFAULT_BASE {
                    return Err(Error::Config(format!(
                        "codex drives a local Ollama through its own `--oss` provider, whose \
                         endpoint this profile does not yet know how to override — '{base}' \
                         would be ignored and the run would quietly use {OLLAMA_DEFAULT_BASE}. \
                         Point Ollama at the default endpoint, or use --agent aider/openhands \
                         for this cell"
                    )));
                }
            }
            Ok((
                provider.model.clone(),
                vec!["--oss".into(), "--local-provider".into(), "ollama".into()],
            ))
        }
        // Codex authenticates to OpenAI itself (`codex login`), so no key is
        // re-plumbed here — the same rule every other profile follows.
        "openai" => Ok((provider.model.clone(), Vec::new())),
        "offline" => Err(Error::Config(
            "the `offline` provider is a deterministic stub for the native loop — an external \
             agent brings its own model client and cannot be pointed at it. Use a real provider \
             (e.g. `--provider ollama`) for an adapter run"
                .into(),
        )),
        other => Err(Error::Config(format!(
            "provider '{other}' has no Codex model mapping (known: ollama, openai). Codex is \
             OpenAI-centric and does not carry the LiteLLM-style prefixes the other adapters \
             use; drive {other} through --agent aider or --agent openhands instead"
        ))),
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
    fn the_default_profile_stands_its_own_sandbox_down() {
        // Orvena must be the only boundary, or the containment number belongs to
        // whichever layer refused first.
        let s = spec(&sel("ollama", "qwen3:14b", None)).unwrap();
        assert!(s.args.iter().any(|a| a == "--dangerously-bypass-approvals-and-sandbox"));
        assert!(!s.args.iter().any(|a| a == "--sandbox"));
    }

    #[test]
    fn the_nested_profile_keeps_its_sandbox_on_and_is_a_distinct_agent_in_the_report() {
        let s = spec_nested(&sel("ollama", "qwen3:14b", None)).unwrap();
        let mode = s.args.iter().position(|a| a == "--sandbox").map(|i| &s.args[i + 1]).unwrap();
        assert_eq!(mode, NESTED_SANDBOX);
        assert!(!s.args.iter().any(|a| a == "--dangerously-bypass-approvals-and-sandbox"));
        // The two arms must be distinguishable in the evidence, or the
        // comparison collapses into one unlabelled number.
        assert_ne!(s.name, spec(&sel("ollama", "qwen3:14b", None)).unwrap().name);
        assert_eq!(s.program, NAME, "both arms are the same binary");
    }

    #[test]
    fn nothing_of_codexs_own_is_left_in_the_project_or_the_real_home() {
        let s = spec(&sel("ollama", "m", None)).unwrap();
        assert!(s.args.iter().any(|a| a == "--ephemeral"), "session files must not be persisted");
        let home = s.env.iter().find(|(k, _)| k == "CODEX_HOME").map(|(_, v)| v).unwrap();
        assert!(home.starts_with(SCRATCH_PLACEHOLDER), "state belongs in the scratch dir: {home}");
    }

    #[test]
    fn ollama_goes_through_the_oss_provider() {
        let s = spec(&sel("ollama", "qwen3:14b", None)).unwrap();
        assert!(s.args.iter().any(|a| a == "--oss"));
        let lp = s.args.iter().position(|a| a == "--local-provider").map(|i| &s.args[i + 1]);
        assert_eq!(lp.map(String::as_str), Some("ollama"));
        let m = s.args.iter().position(|a| a == "--model").map(|i| &s.args[i + 1]).unwrap();
        assert_eq!(m, "qwen3:14b");
    }

    #[test]
    fn the_default_ollama_endpoint_is_accepted_in_either_spelling() {
        // The native client's config may carry the OpenAI-compat suffix; that is
        // the same endpoint and must not be refused.
        assert!(spec(&sel("ollama", "m", Some("http://127.0.0.1:11434"))).is_ok());
        assert!(spec(&sel("ollama", "m", Some("http://127.0.0.1:11434/v1"))).is_ok());
    }

    #[test]
    fn a_non_default_ollama_endpoint_is_refused_rather_than_silently_ignored() {
        // Accepting it would run a different Ollama than the rest of the matrix
        // while reporting the same cell.
        let err = spec(&sel("ollama", "m", Some("http://box:11434"))).unwrap_err();
        assert!(err.to_string().contains("would be ignored"), "{err}");
    }

    #[test]
    fn the_offline_stub_is_refused_with_a_reason() {
        let err = spec(&sel("offline", "stub", None)).unwrap_err();
        assert!(err.to_string().contains("external agent"), "{err}");
    }

    #[test]
    fn an_unmappable_provider_says_which_agent_to_use_instead() {
        let err = spec(&sel("anthropic", "m", None)).unwrap_err();
        assert!(err.to_string().contains("no Codex model mapping"), "{err}");
        assert!(err.to_string().contains("aider"), "an error that names the way out: {err}");
    }
}
