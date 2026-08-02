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

/// What the *provider* actually receives on each posture's path.
///
/// `context::build` is unit-tested against a hand-made `Scope`, which proves the
/// string is assembled correctly but not that the ungoverned entry point ever
/// reaches it with an unrestricted scope. That link — `run_ungoverned_baseline`
/// → `LoopOptions { ungoverned }` → `Scope::unrestricted_baseline` → `build` —
/// is four lines of plumbing, and it is exactly the kind of link that drifts
/// silently: nothing downstream would fail if the baseline quietly went back to
/// receiving the governed prompt, and the benchmark would report the resulting
/// null as a finding about the model. So assert it at the boundary the model
/// sees (`tkt-m1-null-is-structural`).
#[cfg(test)]
mod prompt_reaches_provider {
    use super::*;
    use crate::config::agent::{AgentConfig, ProviderSelection, Tier};
    use crate::config::commands::Commands;
    use crate::config::context_budget::ContextBudgets;
    use crate::config::gates::Gates;
    use crate::config::roles::{Role, Roles};
    use crate::provider::{ChatRequest, ChatResponse};
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};

    fn config() -> Config {
        Config {
            agent: AgentConfig {
                provider: ProviderSelection {
                    kind: "offline".into(),
                    model: "stub".into(),
                    base_url: None,
                    api_key_env: None,
                },
                tier: Tier::Engineering,
                default_role: "developer".into(),
                max_steps: 1,
                sandbox: Default::default(),
            },
            roles: Roles {
                roles: vec![Role {
                    name: "developer".into(),
                    allowed_tools: vec!["fs.read".into(), "fs.write".into()],
                    forbidden_tools: vec![],
                    knowledge_scope: vec![],
                }],
            },
            // No gates: the point is the outbound prompt, not the loop's verdict.
            gates: Gates { gates: vec![] },
            budgets: ContextBudgets::default(),
            commands: Commands::default(),
        }
    }

    /// Records the system message, then returns no actions so the loop ends on
    /// the first step (the baseline's "done" is emitting nothing).
    struct Capture(Arc<Mutex<Vec<String>>>);

    #[async_trait]
    impl Provider for Capture {
        fn id(&self) -> &str {
            "capture"
        }
        async fn chat(&self, req: ChatRequest) -> Result<ChatResponse> {
            let system = req
                .messages
                .iter()
                .filter(|m| m.role == "system")
                .map(|m| m.content.clone())
                .collect::<Vec<_>>()
                .join("\n");
            self.0.lock().unwrap().push(system);
            Ok(ChatResponse { content: String::new(), input_tokens: 0, output_tokens: 0 })
        }
    }

    fn seen(ungoverned: bool) -> String {
        let root = std::env::temp_dir().join(format!(
            "orvena-prompt-{}-{}",
            std::process::id(),
            ungoverned
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.txt"), "x\n").unwrap();

        let log = Arc::new(Mutex::new(Vec::new()));
        let agent = Agent::with_provider(config(), &root, Box::new(Capture(Arc::clone(&log))));
        let task = Task::new("do the thing", vec!["a.txt".into()]);
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            if ungoverned {
                agent.run_ungoverned_baseline(task).await.unwrap()
            } else {
                agent.run(task).await.unwrap()
            }
        });
        let _ = std::fs::remove_dir_all(&root);
        let out = log.lock().unwrap().join("\n");
        assert!(!out.is_empty(), "the provider was never called");
        out
    }

    #[test]
    fn the_governed_path_sends_the_obligation() {
        let p = seen(false);
        assert!(p.contains("never expand scope"));
        assert!(p.contains("modify ONLY files listed under WRITABLE"));
    }

    #[test]
    fn the_ungoverned_path_does_not_send_the_obligation() {
        let p = seen(true);
        assert!(!p.contains("never expand scope"), "the baseline was told to obey: {p}");
        assert!(!p.contains("modify ONLY files listed under WRITABLE"));
        assert!(p.contains("the ones this task is about"), "informational line missing");
    }
}
