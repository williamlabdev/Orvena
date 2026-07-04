//! CLI commands. Each is a thin wrapper that calls into `orvena-core`.

pub mod bench;
pub mod doctor;
pub mod init;
pub mod run;
pub mod status;

use anyhow::{bail, Result};
use orvena_core::provider::registry::{self, Readiness};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Project-local config directory deployed by `orvena init`.
pub const CONFIG_DIR: &str = ".orvena";

pub fn config_dir() -> PathBuf {
    PathBuf::from(CONFIG_DIR)
}

pub fn project_root() -> PathBuf {
    PathBuf::from(".")
}

/// Embedded default scaffold (deployed verbatim by `orvena init`). Kept neutral —
/// no lab-private or methodology-evidence references.
pub struct ScaffoldFile {
    pub rel: &'static str,
    pub contents: &'static str,
}

pub const SCAFFOLD: &[ScaffoldFile] = &[
    ScaffoldFile { rel: "orvena.yaml", contents: include_str!("../scaffold/orvena.yaml") },
    ScaffoldFile { rel: "roles.yaml", contents: include_str!("../scaffold/roles.yaml") },
    ScaffoldFile { rel: "gates.yaml", contents: include_str!("../scaffold/gates.yaml") },
    ScaffoldFile { rel: "commands.yaml", contents: include_str!("../scaffold/commands.yaml") },
    ScaffoldFile {
        rel: "context-budgets.yaml",
        contents: include_str!("../scaffold/context-budgets.yaml"),
    },
    ScaffoldFile {
        rel: "skills/summarize-changes/SKILL.md",
        contents: include_str!("../scaffold/skills/summarize-changes/SKILL.md"),
    },
];

pub const ENV_EXAMPLE: &str = include_str!("../scaffold/env.example");

/// Does the project already have a deployed config?
pub fn is_initialized(dir: &Path) -> bool {
    dir.join("orvena.yaml").is_file()
}

/// Check the provider is usable *before* building it, and turn a not-ready state
/// into the same actionable guidance `orvena doctor` gives. Readiness is a local,
/// network-free check (a missing key, or an unknown kind) — reused from the
/// registry so `run`, `bench`, and `doctor` never drift. Shared by `run` and
/// `bench`.
pub fn preflight_provider(kind: &str) -> Result<()> {
    match registry::readiness(kind) {
        Readiness::Ready => Ok(()),
        Readiness::MissingKey(key) => bail!(
            "provider '{kind}' is not ready — {key} is not set.\n  \
             • add it to .env (see .env.example), then `orvena doctor` to verify; or\n  \
             • see the loop run right now with no key: `orvena run --provider offline \"<task>\"`"
        ),
        Readiness::Unknown => bail!(
            "provider '{kind}' is unknown — choose anthropic | openai | openrouter | ollama | offline\n  \
             (edit .orvena/orvena.yaml, or pass `--provider <kind>`)."
        ),
    }
}

/// A filesystem-safe run identifier: milliseconds since the Unix epoch. Millis
/// (not seconds) to avoid collisions between back-to-back runs; a raw epoch
/// number (not ISO-8601) keeps this dependency-free of a date library in v0.1 —
/// see ADR-002. Sortable lexicographically for equal-width values. Shared by
/// `run` and `bench`.
pub fn run_timestamp() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
        .to_string()
}

/// Minimal `.env` loader (no extra dependency). Reads `KEY=VALUE` lines from
/// `./.env` and sets any that are not already present in the environment, so API
/// keys live only in `.env`. Quiet if the file is absent.
pub fn load_dotenv() {
    let Ok(text) = std::fs::read_to_string(".env") else {
        return;
    };
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let key = key.trim();
            let value = value.trim().trim_matches('"');
            if std::env::var_os(key).is_none() && !key.is_empty() {
                std::env::set_var(key, value);
            }
        }
    }
}
