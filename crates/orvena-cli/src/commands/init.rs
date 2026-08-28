//! `orvena init` — deploy the scaffold and walk the user through choosing a
//! provider. No provider is assumed silently; when we cannot prompt we deploy
//! the scaffold and print the next steps instead.
//!
//! "Cannot prompt" is deliberately stricter than "stdin is not a terminal" —
//! see [`can_prompt`]. Scripts should not depend on that detection at all:
//! `--provider` (with `--model` / `--base-url` / `--api-key-env`) sets the
//! provider outright and never prompts.

use super::{config_dir, is_initialized, ENV_EXAMPLE, SCAFFOLD};
use anyhow::{bail, Context, Result};
use orvena_core::provider::registry;
use std::io::{IsTerminal, Write};
use std::path::Path;

/// Provider settings supplied on the command line, for scripted setup.
#[derive(Default)]
pub struct ProviderArgs {
    pub kind: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub api_key_env: Option<String>,
}

pub fn run(args: ProviderArgs, non_interactive: bool) -> Result<()> {
    let dir = config_dir();
    if is_initialized(&dir) {
        println!("Already initialized at {}/ — leaving it untouched.", dir.display());
    } else {
        deploy_scaffold(&dir)?;
        deploy_env_example()?;
        println!("Scaffolded config into {}/", dir.display());
    }

    // An explicit provider is an instruction, not a preference: honor it whether
    // or not there is a terminal, and never prompt on top of it.
    if args.kind.is_some() {
        return apply_provider_args(&dir, &args);
    }
    if !non_interactive && can_prompt() {
        choose_provider(&dir)?;
    } else {
        print_manual_next_steps();
    }
    Ok(())
}

/// Whether this process may actually prompt on stdin.
///
/// `stdin().is_terminal()` answers whether stdin *is* a terminal — not whether
/// we are allowed to read it. A backgrounded process (`&`, `nohup`, a launchd
/// job that inherited a tty) keeps stdin attached to the terminal, so the naive
/// check says "interactive" and the first read earns **SIGTTIN**, which stops
/// the process with no error, no exit, and no output. `orvena init` is the
/// first command anyone runs and is exactly the kind of thing scripts run
/// unattended, so it must fall back to printing next steps instead of hanging.
///
/// The question that actually matters is whether we are the terminal's
/// foreground process group.
fn can_prompt() -> bool {
    if !std::io::stdin().is_terminal() {
        return false;
    }
    #[cfg(unix)]
    {
        // SAFETY: both are argument-free getters with no side effects;
        // `tcgetpgrp` only reads terminal state for the given fd.
        let (foreground, ours) = unsafe { (libc::tcgetpgrp(libc::STDIN_FILENO), libc::getpgrp()) };
        // A -1 means stdin is not our controlling terminal; treat that as
        // "cannot prompt" rather than guessing.
        foreground != -1 && foreground == ours
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// Apply a provider chosen on the command line. Unknown kinds and a missing
/// `base_url` are errors here rather than silent downgrades — the same standard
/// the config parser holds.
fn apply_provider_args(dir: &Path, args: &ProviderArgs) -> Result<()> {
    let kind = args.kind.as_deref().expect("caller checked");
    let Some(p) = registry::info(kind) else {
        let known: Vec<&str> = registry::known().iter().map(|p| p.kind).collect();
        bail!("unknown provider '{kind}' — known kinds: {}", known.join(", "));
    };
    let base_url = args.base_url.as_deref().map(str::trim).filter(|s| !s.is_empty());
    if p.requires_base_url && base_url.is_none() {
        bail!(
            "provider '{kind}' needs --base-url (e.g. --base-url http://localhost:8000/v1); \
             there is no default to fall back to"
        );
    }
    let model = args.model.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let key_env = args.api_key_env.as_deref().map(str::trim).filter(|s| !s.is_empty());
    write_provider(dir, p.kind, model, base_url, key_env)?;
    println!("Set provider to '{}'.", p.kind);
    match key_env.or(p.env_key) {
        Some(key) => {
            println!("Next: put your key in .env as  {key}=<your-key>  (never commit .env).")
        }
        None => println!("This provider needs no API key."),
    }
    Ok(())
}

fn deploy_scaffold(dir: &Path) -> Result<()> {
    for f in SCAFFOLD {
        let path = dir.join(f.rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, f.contents).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}

fn deploy_env_example() -> Result<()> {
    let path = Path::new(".env.example");
    if !path.exists() {
        std::fs::write(path, ENV_EXAMPLE)?;
    }
    Ok(())
}

/// Interactive provider picker. Writes the chosen provider into orvena.yaml.
fn choose_provider(dir: &Path) -> Result<()> {
    let providers = registry::known();
    println!("\nChoose a model provider (nothing is assumed silently):");
    for (i, p) in providers.iter().enumerate() {
        println!("  {}) {:<11} — {}", i + 1, p.kind, p.description);
    }
    print!("Selection [1-{}]: ", providers.len());
    std::io::stdout().flush()?;

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;
    let choice: usize = input.trim().parse().unwrap_or(0);
    let Some(p) = choice.checked_sub(1).and_then(|i| providers.get(i)) else {
        println!("No valid selection — leaving provider as the scaffold default.");
        return Ok(());
    };

    print!("Model id for {} [press enter to keep scaffold default]: ", p.kind);
    std::io::stdout().flush()?;
    let mut model = String::new();
    std::io::stdin().read_line(&mut model)?;
    let model = model.trim().to_string();

    let mut base_url = String::new();
    if p.requires_base_url {
        loop {
            print!("base_url for {} (required, e.g. http://localhost:8000/v1): ", p.kind);
            std::io::stdout().flush()?;
            base_url.clear();
            std::io::stdin().read_line(&mut base_url)?;
            if !base_url.trim().is_empty() {
                break;
            }
            println!(
                "base_url cannot be empty for '{}' — there is no default to fall back to.",
                p.kind
            );
        }
    }

    let mut api_key_env = String::new();
    if p.requires_base_url {
        print!("Env var to read the API key from [press enter if {} needs no key]: ", p.kind);
        std::io::stdout().flush()?;
        std::io::stdin().read_line(&mut api_key_env)?;
    }

    let base_url = base_url.trim();
    let key_env = api_key_env.trim();
    write_provider(
        dir,
        p.kind,
        model.as_deref_or_none(),
        (!base_url.is_empty()).then_some(base_url),
        (!key_env.is_empty()).then_some(key_env),
    )?;
    println!("Set provider to '{}'.", p.kind);

    if !key_env.is_empty() {
        println!("Next: put your key in .env as  {key_env}=<your-key>  (never commit .env).");
    } else if let Some(key) = p.env_key {
        println!("Next: put your key in .env as  {key}=<your-key>  (never commit .env).");
    } else {
        println!("This provider needs no API key.");
    }
    Ok(())
}

/// Minimal targeted rewrite of the provider block in orvena.yaml.
fn write_provider(
    dir: &Path,
    kind: &str,
    model: Option<&str>,
    base_url: Option<&str>,
    api_key_env: Option<&str>,
) -> Result<()> {
    let path = dir.join("orvena.yaml");
    let mut value: serde_yaml::Value = serde_yaml::from_str(&std::fs::read_to_string(&path)?)?;
    if let Some(provider) = value.get_mut("provider").and_then(|p| p.as_mapping_mut()) {
        provider.insert("kind".into(), kind.into());
        if let Some(m) = model {
            provider.insert("model".into(), m.into());
        }
        if let Some(b) = base_url {
            provider.insert("base_url".into(), b.into());
        }
        if let Some(e) = api_key_env {
            provider.insert("api_key_env".into(), e.into());
        }
    }
    std::fs::write(&path, serde_yaml::to_string(&value)?)?;
    Ok(())
}

fn print_manual_next_steps() {
    println!(
        "\nNot prompting (no terminal to read, or this process is not in its \
         foreground) — next steps:\n  \
         1. Set the provider without prompting, e.g.\n     \
              orvena init --provider openai --model gpt-5.6-luna\n     \
            (or edit {}/orvena.yaml and set provider.kind + provider.model).\n  \
         2. Put the matching key in .env (see .env.example).\n  \
         3. Run `orvena doctor` to verify, then `orvena run \"<task>\"`.",
        CONFIG_DIR
    );
}

use super::CONFIG_DIR;

/// Tiny helper: treat an empty string as `None`.
trait AsDerefOrNone {
    fn as_deref_or_none(&self) -> Option<&str>;
}
impl AsDerefOrNone for String {
    fn as_deref_or_none(&self) -> Option<&str> {
        if self.is_empty() {
            None
        } else {
            Some(self.as_str())
        }
    }
}
