//! `orvena init` — deploy the scaffold and walk the user through choosing a
//! provider. No provider is assumed silently; in a non-interactive shell we
//! deploy the scaffold and print the next steps instead of prompting.

use super::{config_dir, is_initialized, ENV_EXAMPLE, SCAFFOLD};
use anyhow::{Context, Result};
use orvena_core::provider::registry;
use std::io::{IsTerminal, Write};
use std::path::Path;

pub fn run() -> Result<()> {
    let dir = config_dir();
    if is_initialized(&dir) {
        println!("Already initialized at {}/ — leaving it untouched.", dir.display());
    } else {
        deploy_scaffold(&dir)?;
        deploy_env_example()?;
        println!("Scaffolded config into {}/", dir.display());
    }

    if std::io::stdin().is_terminal() {
        choose_provider(&dir)?;
    } else {
        print_manual_next_steps();
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
        "\nNon-interactive shell — next steps:\n  \
         1. Edit {}/orvena.yaml and set provider.kind + provider.model.\n  \
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
