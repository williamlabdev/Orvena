//! `orvena doctor` — preflight checks with human-readable messages (not
//! tracebacks): config presence/validity and provider readiness.

use super::{config_dir, is_initialized, load_dotenv};
use anyhow::Result;
use orvena_core::config::Config;
use orvena_core::provider::registry::{self, Readiness};

pub fn run() -> Result<()> {
    load_dotenv();
    let mut ok = true;

    let dir = config_dir();
    if !is_initialized(&dir) {
        println!("✗ config: not initialized — run `orvena init`");
        return Ok(());
    }
    println!("✓ config: found at {}/", dir.display());

    let config = match Config::load_dir(&dir) {
        Ok(c) => {
            println!("✓ config: valid");
            c
        }
        Err(e) => {
            println!("✗ config: {e}");
            return Ok(());
        }
    };

    let kind = &config.agent.provider.kind;
    match registry::readiness(kind) {
        Readiness::Ready => println!("✓ provider '{kind}': ready"),
        Readiness::MissingKey(key) => {
            ok = false;
            println!("✗ provider '{kind}': {key} not set — add it to .env");
        }
        Readiness::Unknown => {
            ok = false;
            println!(
                "✗ provider '{kind}': unknown — choose anthropic | openai | openrouter | ollama | offline"
            );
        }
    }

    if ok {
        println!("\nAll checks passed.");
        println!(
            "A run exports an evidence bundle to {}/runs/<timestamp>/evidence.json (success or not).",
            dir.display()
        );
    } else {
        println!("\nSome checks failed — see above.");
    }
    Ok(())
}
