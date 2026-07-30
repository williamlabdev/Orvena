//! Per-role context budgets (`context-budgets.yaml`). Context is a budget, not an
//! unbounded window. (Controlled Context pillar.)

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextBudgets {
    #[serde(default = "default_max_tokens")]
    pub default_max_tokens: u32,
    #[serde(default)]
    pub budgets: Vec<ContextBudget>,
}

impl Default for ContextBudgets {
    fn default() -> Self {
        Self { default_max_tokens: default_max_tokens(), budgets: Vec::new() }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextBudget {
    pub role: String,
    pub max_tokens: u32,
    #[serde(default)]
    pub max_seconds: Option<u32>,
}

impl ContextBudgets {
    /// Token budget for a role, falling back to the default.
    pub fn for_role(&self, role: &str) -> u32 {
        self.budgets
            .iter()
            .find(|b| b.role == role)
            .map(|b| b.max_tokens)
            .unwrap_or(self.default_max_tokens)
    }
}

fn default_max_tokens() -> u32 {
    8000
}
