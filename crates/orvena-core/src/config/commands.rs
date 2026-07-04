//! Declared shell commands (`commands.yaml`). Per ADR-001, the model never
//! supplies a command *string* — it references a human-declared command by
//! `name`, and the runtime spawns that command's fixed `argv` directly (no
//! shell). Each command carries an `intent` the human vouches for; the runtime
//! trusts that declaration (it does not try to prove a command is read-only).

use crate::error::{Error, Result};
use crate::exec::DEFAULT_TIMEOUT_SECS;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Commands {
    #[serde(default)]
    pub commands: Vec<Command>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Command {
    pub name: String,
    /// Fixed argument vector spawned directly — `argv[0]` is the program.
    pub argv: Vec<String>,
    pub intent: Intent,
    /// Wall-clock ceiling in seconds. Optional; defaults to
    /// [`DEFAULT_TIMEOUT_SECS`].
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

/// The human's trust declaration for a command. Only `read_only` commands may be
/// triggered by the model; `mutating` ones are declared for humans/gates only.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Intent {
    ReadOnly,
    Mutating,
}

impl Command {
    /// The effective timeout, applying the default when none is declared.
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS))
    }
}

impl Commands {
    pub fn get(&self, name: &str) -> Option<&Command> {
        self.commands.iter().find(|c| c.name == name)
    }

    /// Cheap structural checks surfaced at config-load time (never at runtime):
    /// a duplicate `name` or an empty `argv` is an `Error::Config`.
    pub fn validate(&self) -> Result<()> {
        let mut seen = HashSet::new();
        for c in &self.commands {
            if c.argv.is_empty() {
                return Err(Error::Config(format!(
                    "command '{}' has an empty argv — declare the program to run",
                    c.name
                )));
            }
            if !seen.insert(c.name.as_str()) {
                return Err(Error::Config(format!("duplicate command name '{}'", c.name)));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(name: &str, argv: &[&str], intent: Intent) -> Command {
        Command {
            name: name.into(),
            argv: argv.iter().map(|s| s.to_string()).collect(),
            intent,
            timeout_secs: None,
        }
    }

    #[test]
    fn timeout_defaults_when_absent() {
        let c = cmd("test", &["cargo", "test"], Intent::ReadOnly);
        assert_eq!(c.timeout(), Duration::from_secs(DEFAULT_TIMEOUT_SECS));
    }

    #[test]
    fn timeout_uses_declared_value() {
        let mut c = cmd("test", &["cargo", "test"], Intent::ReadOnly);
        c.timeout_secs = Some(30);
        assert_eq!(c.timeout(), Duration::from_secs(30));
    }

    #[test]
    fn duplicate_name_is_a_config_error() {
        let cmds = Commands {
            commands: vec![
                cmd("test", &["cargo", "test"], Intent::ReadOnly),
                cmd("test", &["cargo", "nextest"], Intent::ReadOnly),
            ],
        };
        let err = cmds.validate().unwrap_err();
        assert!(matches!(err, Error::Config(_)));
        assert!(err.to_string().contains("duplicate command name 'test'"));
    }

    #[test]
    fn empty_argv_is_a_config_error() {
        let cmds = Commands { commands: vec![cmd("broken", &[], Intent::ReadOnly)] };
        let err = cmds.validate().unwrap_err();
        assert!(matches!(err, Error::Config(_)));
        assert!(err.to_string().contains("empty argv"));
    }

    #[test]
    fn intent_deserializes_snake_case() {
        let yaml = "commands:\n  - name: t\n    argv: [cargo, test]\n    intent: read_only\n  - name: f\n    argv: [cargo, fmt]\n    intent: mutating\n";
        let cmds: Commands = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(cmds.get("t").unwrap().intent, Intent::ReadOnly);
        assert_eq!(cmds.get("f").unwrap().intent, Intent::Mutating);
        cmds.validate().unwrap();
    }
}
