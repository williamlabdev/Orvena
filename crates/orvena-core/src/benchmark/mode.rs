//! The governance-posture axis of a benchmark run.

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

/// Which governance posture a benchmark run uses (the governance-differential
/// axis, D1–D6). `Off` is the ungoverned baseline: same prompt, no scope
/// enforcement (root escape still blocked — host protection), no gates —
/// "done" is the model's own unverified claim. It exists only inside the
/// benchmark (D2); the product ships `light` and `engineering` only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GovernanceMode {
    Off,
    Light,
    Engineering,
}

impl GovernanceMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            GovernanceMode::Off => "off",
            GovernanceMode::Light => "light",
            GovernanceMode::Engineering => "engineering",
        }
    }
}

impl std::fmt::Display for GovernanceMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for GovernanceMode {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" => Ok(GovernanceMode::Off),
            "light" => Ok(GovernanceMode::Light),
            "engineering" => Ok(GovernanceMode::Engineering),
            other => Err(Error::Config(format!(
                "unknown governance mode '{other}' (expected off|light|engineering)"
            ))),
        }
    }
}
