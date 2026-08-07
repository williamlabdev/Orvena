//! "Evidence by default" must hold on the *error* path, not only when the loop
//! returns Ok: a run whose provider fails still finishes with a report (a
//! recorded blocker), so the caller can write an auditable bundle. See
//! tkt-evidence-all-exit-paths.

use async_trait::async_trait;
use orvena_core::config::agent::{AgentConfig, ProviderSelection, Tier};
use orvena_core::config::commands::Commands;
use orvena_core::config::context_budget::ContextBudgets;
use orvena_core::config::gates::{Gate, Gatekeeper, Gates};
use orvena_core::config::roles::{Role, Roles};
use orvena_core::config::Config;
use orvena_core::metrics::evidence;
use orvena_core::{Agent, ChatRequest, ChatResponse, Error, Provider, Result, RunReport, Task};

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("orvena-everr-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn dev_config() -> Config {
    Config {
        agent: AgentConfig {
            provider: ProviderSelection {
                kind: "offline".into(),
                model: "stub".into(),
                base_url: None,
                api_key_env: None,
                sampling: None,
            },
            tier: Tier::Engineering,
            default_role: "developer".into(),
            max_steps: 3,
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
        gates: Gates {
            gates: vec![Gate {
                name: "file-exists".into(),
                condition: "hello.txt was created".into(),
                verify: Some("test -f hello.txt".into()),
                gatekeeper: Gatekeeper::Automated,
                timeout_secs: None,
            }],
        },
        budgets: ContextBudgets::default(),
        commands: Commands::default(),
    }
}

/// A provider that always fails — stands in for an outage / bad key / network error.
struct FailingProvider;

#[async_trait]
impl Provider for FailingProvider {
    fn id(&self) -> &str {
        "failing"
    }
    async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse> {
        Err(Error::Other(anyhow::anyhow!("simulated provider outage")))
    }
}

#[tokio::test]
async fn provider_error_still_yields_a_report_and_bundle() {
    let root = temp_dir("provider-error");
    let agent = Agent::with_provider(dev_config(), &root, Box::new(FailingProvider));

    // The run must NOT bubble the provider error out as Err — it should finish
    // with a report so evidence can be written.
    let report = agent
        .run(Task::new("Create a greeting file", vec!["hello.txt".into()]))
        .await
        .expect("a provider error must be captured into the report, not propagated");

    assert!(!report.completed, "a run whose provider failed cannot be complete");
    assert!(
        report.blockers.iter().any(|b| b.contains("provider error")),
        "the provider failure must be recorded as a blocker: {:?}",
        report.blockers,
    );
    // Structured, not just prose: the benchmark excludes provider-killed runs
    // from its denominators, and it must not have to pattern-match a message to
    // know which runs those were.
    assert_eq!(
        report.provider_error.as_deref(),
        Some("simulated provider outage"),
        "a provider failure must set the structured flag, not only a blocker string",
    );

    // And that report writes to an auditable, round-trippable bundle on disk.
    let path = evidence::bundle_path(&root, "err-run-id");
    evidence::write_bundle(&report, &path).expect("bundle should write on the error path");
    assert!(path.exists(), "evidence bundle must land even when the provider failed");
    let reloaded: RunReport = serde_json::from_str(&std::fs::read_to_string(&path).unwrap())
        .expect("bundle deserializes");
    assert!(!reloaded.completed);
    assert!(reloaded.blockers.iter().any(|b| b.contains("provider error")));
    assert_eq!(
        reloaded.provider_error.as_deref(),
        Some("simulated provider outage"),
        "the structured flag must survive the round-trip through the bundle",
    );

    let _ = std::fs::remove_dir_all(&root);
}
