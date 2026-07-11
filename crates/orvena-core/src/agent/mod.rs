//! The agent: one bounded coding loop wired from config + provider + governance.
//!
//! The loop is `prepare context → call model → apply → gate check`, with a
//! bounded re-attempt when an automated gate fails (observe → re-attempt, capped
//! by `max_steps`). This is deliberately single-role and bounded — not a planner
//! or multi-agent system (those are out of v0.1 scope).

pub mod context;
pub mod driver;
pub mod step;

use crate::config::Config;
use crate::error::Result;
use crate::metrics::RunReport;
use crate::provider::{build_chat_provider, Provider};
use std::path::PathBuf;

/// A unit of bounded work.
#[derive(Debug, Clone)]
pub struct Task {
    pub instruction: String,
    /// Relative paths the task is allowed to modify. Everything else is
    /// read-only by default.
    pub allowed_modifications: Vec<String>,
}

impl Task {
    pub fn new(instruction: impl Into<String>, allowed_modifications: Vec<String>) -> Self {
        Self { instruction: instruction.into(), allowed_modifications }
    }
}

pub struct Agent {
    config: Config,
    provider: Box<dyn Provider>,
    root: PathBuf,
}

impl Agent {
    /// Build an agent, resolving the provider from config (and the environment
    /// for API keys). Fails loudly if the provider is unknown/unconfigured.
    pub fn new(config: Config, root: impl Into<PathBuf>) -> Result<Self> {
        let provider = build_chat_provider(&config.agent.provider)?;
        Ok(Self { config, provider, root: root.into() })
    }

    /// Build an agent with an injected provider (e.g. the offline stub in tests
    /// and L1 baselines), bypassing env-based construction.
    pub fn with_provider(
        config: Config,
        root: impl Into<PathBuf>,
        provider: Box<dyn Provider>,
    ) -> Self {
        Self { config, provider, root: root.into() }
    }

    pub async fn run(&self, task: Task) -> Result<RunReport> {
        driver::run_loop(self, task).await
    }

    /// Bench-only ungoverned baseline (D2): identical prompt, no enforcement,
    /// completion = the model's own unverified claim. Crate-private on purpose —
    /// the only caller is the benchmark harness; no product path reaches this.
    pub(crate) async fn run_ungoverned_baseline(&self, task: Task) -> Result<RunReport> {
        driver::run_loop_with(self, task, driver::LoopOptions { ungoverned: true }).await
    }

    pub(crate) fn config(&self) -> &Config {
        &self.config
    }
    pub(crate) fn provider(&self) -> &dyn Provider {
        self.provider.as_ref()
    }
    pub(crate) fn root(&self) -> &std::path::Path {
        &self.root
    }
}
