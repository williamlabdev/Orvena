//! Gates (`gates.yaml`). A change is "done" only when it passes these gates.
//! Each gate has a human-readable condition, an optional `verify` command that
//! produces **observable evidence** (exit 0 = pass — the local analogue of
//! "re-run CI until green"), and a gatekeeper that is either `automated`
//! (evidence decides) or `human` (escalates and stops the loop).

use crate::exec::DEFAULT_TIMEOUT_SECS;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Gates {
    #[serde(default)]
    pub gates: Vec<Gate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gate {
    pub name: String,
    pub condition: String,
    /// Shell command run to produce evidence. Exit 0 = pass.
    #[serde(default)]
    pub verify: Option<String>,
    #[serde(default)]
    pub gatekeeper: Gatekeeper,
    /// Wall-clock ceiling for `verify` in seconds. Optional; defaults to
    /// [`DEFAULT_TIMEOUT_SECS`]. A verify that outruns it counts as a failure.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

impl Gate {
    /// The effective `verify` timeout, applying the default when none is declared.
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_secs.unwrap_or(DEFAULT_TIMEOUT_SECS))
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Gatekeeper {
    /// Mechanical: the `verify` command's evidence decides pass/fail.
    #[default]
    Automated,
    /// Judgment: requires a human; the loop stops and reports a blocker.
    Human,
}
