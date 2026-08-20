//! The **Claude Code CLI** profile — headless (`claude -p`), one shot per step.
//!
//! ## The same flag, from the other vendor
//!
//! Claude Code ships `--dangerously-skip-permissions`, documented for exactly
//! one situation: an environment that is already externally sandboxed. Like
//! Codex's `--dangerously-bypass-approvals-and-sandbox`, it is a vendor switch
//! whose stated purpose is *the outer envelope exists, so my own gate stands
//! down* — which is the proposition Orvena exists to provide. That flag drives
//! this profile: Orvena's OS sandbox is the only boundary, and an out-of-scope
//! write fails at the syscall no matter what the agent believes.
//!
//! ## Auth is the agent's own, never re-plumbed
//!
//! The CLI authenticates through its own subscription login (macOS keeps the
//! OAuth credential in the Keychain, not in the config directory). No API key
//! is exported for it here — the same rule every other profile follows — and
//! `bench` skips the provider-key preflight for wrapped agents for the same
//! reason. A stray `ANTHROPIC_API_KEY` in the operator's environment would
//! silently switch the CLI from subscription to metered API billing; this
//! profile inherits the environment as-is, so keep that variable unset when
//! driving this cell.
//!
//! ## The state directory cannot be redirected — measured, not assumed
//!
//! The obvious hygiene move — `CLAUDE_CONFIG_DIR` pointed at the agent scratch
//! dir, the way the Codex profile redirects `CODEX_HOME` for local cells —
//! produces `Not logged in` (verified 2026-08-20, CLI 2.1.237, even with the
//! operator's `.claude.json` seeded into the moved dir): the subscription
//! credential is bound to the default state location. So this profile grants
//! the real `~/.claude` and `~/.claude.json` as
//! [`AdapterSpec::state_writable`] instead — a spoken widening in the run
//! evidence. Two honest consequences:
//!
//! - Session transcripts from benchmark runs persist under the operator's
//!   `~/.claude/projects`, and the CLI's own config file may be rewritten.
//!   Neither is inside any task workdir, so the oracle's verdict is untouched.
//! - Writes the CLI attempts *outside* those two paths (caches under
//!   `~/Library`, say) still fail at the syscall; whether the CLI tolerates
//!   that is empirical per version.

use super::{home_dir, AdapterSpec};
use crate::config::agent::ProviderSelection;
use crate::{Error, Result};

/// Claude Code headless, its own permission gate stood down — Orvena is the
/// only boundary.
pub const NAME: &str = "claude";

/// Build the Claude Code profile for `provider`.
pub fn spec(provider: &ProviderSelection) -> Result<AdapterSpec> {
    let model = model_for(provider)?;
    let home = home_dir()?;
    Ok(AdapterSpec {
        name: NAME.to_string(),
        program: "claude".to_string(),
        args: vec![
            "-p".to_string(),
            "--dangerously-skip-permissions".into(),
            "--model".into(),
            model,
            "{instruction}".into(),
        ],
        env: vec![],
        version_args: vec!["--version".into()],
        config_files: vec![],
        state_writable: vec![home.join(".claude"), home.join(".claude.json")],
    })
}

fn model_for(provider: &ProviderSelection) -> Result<String> {
    match provider.kind.as_str() {
        // The CLI drives Anthropic's own models and nothing else; the model
        // string passes through untouched (full ids and CLI aliases both work).
        "anthropic" => Ok(provider.model.clone()),
        "offline" => Err(Error::Config(
            "the `offline` provider is a deterministic stub for the native loop — an external \
             agent brings its own model client and cannot be pointed at it. Use `--provider \
             anthropic` for a Claude Code cell"
                .into(),
        )),
        other => Err(Error::Config(format!(
            "provider '{other}' has no Claude Code model mapping (known: anthropic). The CLI \
             only drives Anthropic's own models; drive {other} through --agent aider or \
             --agent openhands instead"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel(kind: &str, model: &str) -> ProviderSelection {
        ProviderSelection {
            kind: kind.into(),
            model: model.into(),
            base_url: None,
            api_key_env: None,
            sampling: None,
        }
    }

    #[test]
    fn the_profile_stands_its_own_permission_gate_down() {
        // Orvena must be the only boundary, or the containment number belongs
        // to whichever layer refused first.
        let s = spec(&sel("anthropic", "claude-opus-4-8")).unwrap();
        assert!(s.args.iter().any(|a| a == "--dangerously-skip-permissions"));
        assert!(s.args.iter().any(|a| a == "-p"), "headless, one shot per step");
    }

    #[test]
    fn anthropic_model_passes_through_untouched() {
        let s = spec(&sel("anthropic", "claude-opus-4-8")).unwrap();
        let m = s.args.iter().position(|a| a == "--model").map(|i| &s.args[i + 1]).unwrap();
        assert_eq!(m, "claude-opus-4-8");
    }

    #[test]
    fn the_real_state_location_is_a_spoken_widening() {
        // A redirected CLAUDE_CONFIG_DIR measures a login screen (module
        // docs), so the profile must grant the real state paths — and must
        // not touch CLAUDE_CONFIG_DIR at all.
        let s = spec(&sel("anthropic", "claude-opus-4-8")).unwrap();
        assert!(s.env.iter().all(|(k, _)| k != "CLAUDE_CONFIG_DIR"));
        assert_eq!(s.state_writable.len(), 2);
        assert!(s.state_writable[0].ends_with(".claude"));
        assert!(s.state_writable[1].ends_with(".claude.json"));
    }

    #[test]
    fn other_providers_are_refused_loudly() {
        for kind in ["ollama", "openai", "openrouter", "offline"] {
            assert!(spec(&sel(kind, "m")).is_err(), "{kind} must not silently map");
        }
    }
}
