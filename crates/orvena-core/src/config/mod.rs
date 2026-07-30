//! Config-first surface. Every behavioral knob is YAML the user can edit without
//! forking code: provider selection, roles & tool boundaries, context budgets,
//! and gates. v0.1's minimal set is `orvena.yaml` / `roles.yaml` / `gates.yaml`
//! / `context-budgets.yaml`.

pub mod agent;
pub mod commands;
pub mod context_budget;
pub mod gates;
pub mod roles;
pub mod sandbox;

pub use agent::{AgentConfig, ProviderSelection, Tier};
pub use commands::{Command, Commands, Intent};
pub use context_budget::{ContextBudget, ContextBudgets};
pub use gates::{Gate, Gatekeeper, Gates};
pub use roles::{Role, Roles};
pub use sandbox::SandboxConfig;

use crate::error::{Error, Result};
use serde::de::DeserializeOwned;
use std::path::Path;

/// The fully-loaded config-first surface for a project.
#[derive(Debug, Clone)]
pub struct Config {
    pub agent: AgentConfig,
    pub roles: Roles,
    pub gates: Gates,
    pub budgets: ContextBudgets,
    /// Named shell commands the model may run by reference (ADR-001). Optional:
    /// a project without `commands.yaml` simply has no runnable commands.
    pub commands: Commands,
}

impl Config {
    /// Load the config files from a directory (typically `.orvena/`).
    pub fn load_dir(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        let cfg = Self {
            agent: read_yaml(dir.join("orvena.yaml"))?,
            roles: read_yaml(dir.join("roles.yaml"))?,
            gates: read_yaml(dir.join("gates.yaml"))?,
            budgets: read_yaml(dir.join("context-budgets.yaml"))?,
            // Backward-compatible: pre-slice-002 projects have no commands.yaml.
            commands: read_yaml_optional(dir.join("commands.yaml"))?,
        };
        cfg.validate()?;
        Ok(cfg)
    }

    /// Cheap structural checks surfaced as human-readable errors (used by `doctor`).
    pub fn validate(&self) -> Result<()> {
        if self.roles.get(&self.agent.default_role).is_none() {
            return Err(Error::Config(format!(
                "default_role '{}' is not defined in roles.yaml",
                self.agent.default_role
            )));
        }
        if self.agent.max_steps == 0 {
            return Err(Error::Config("max_steps must be >= 1".into()));
        }
        self.commands.validate()?;
        self.agent.sandbox.validate()?;
        Ok(())
    }
}

/// Read + deserialize a YAML file, mapping the path into the error for clarity.
pub(crate) fn read_yaml<T: DeserializeOwned>(path: impl AsRef<Path>) -> Result<T> {
    let path = path.as_ref();
    let text = std::fs::read_to_string(path)
        .map_err(|e| Error::Config(format!("cannot read {}: {e}", path.display())))?;
    serde_yaml::from_str(&text)
        .map_err(|e| Error::Config(format!("invalid YAML in {}: {e}", path.display())))
}

/// Like [`read_yaml`], but a missing file yields `T::default()` instead of an
/// error — for config files that are optional (a project may not have declared
/// any). A file that exists but is malformed is still a loud error.
pub(crate) fn read_yaml_optional<T: DeserializeOwned + Default>(
    path: impl AsRef<Path>,
) -> Result<T> {
    let path = path.as_ref();
    if !path.exists() {
        return Ok(T::default());
    }
    read_yaml(path)
}
