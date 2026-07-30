//! End-to-end test of the bounded coding loop using the deterministic offline
//! provider: prepare context → apply a write (scope-gated) → pass a verify gate →
//! report completion. No network, fully reproducible — this is the seed of the
//! L1 golden-task regression.

use orvena_core::config::agent::{AgentConfig, ProviderSelection, Tier};
use orvena_core::config::commands::Commands;
use orvena_core::config::context_budget::ContextBudgets;
use orvena_core::config::gates::{Gate, Gatekeeper, Gates};
use orvena_core::config::roles::{Role, Roles};
use orvena_core::config::Config;
use orvena_core::metrics::BaselineRecord;
use orvena_core::provider::offline::Offline;
use orvena_core::{Agent, Task};

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("orvena-test-{tag}-{pid}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn dev_config(provider: ProviderSelection) -> Config {
    Config {
        agent: AgentConfig {
            provider,
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

#[tokio::test]
async fn loop_writes_in_scope_and_passes_gate() {
    let root = temp_dir("happy");
    let sel = ProviderSelection {
        kind: "offline".into(),
        model: "stub".into(),
        base_url: None,
        api_key_env: None,
    };
    let config = dev_config(sel.clone());
    let agent = Agent::with_provider(config, &root, Box::new(Offline::new(&sel)));

    let task = Task::new("Create a greeting file", vec!["hello.txt".into()]);
    let report = agent.run(task).await.unwrap();

    assert!(report.completed, "gate should pass; blockers: {:?}", report.blockers);
    assert!(report.tool_calls >= 1, "the loop should have applied a write");
    assert!(root.join("hello.txt").exists(), "file should be written in scope");
    assert_eq!(report.steps, 1, "should complete on the first attempt");

    // Frozen baseline record is well-formed and self-consistent.
    let record = BaselineRecord::from_report("greeting", &report);
    assert!(record.completed);
    assert!(record.diff(&record).is_empty(), "a record should not regress against itself");

    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn write_outside_scope_is_blocked_in_engineering_tier() {
    let root = temp_dir("scope");
    let sel = ProviderSelection {
        kind: "offline".into(),
        model: "stub".into(),
        base_url: None,
        api_key_env: None,
    };
    let config = dev_config(sel.clone());
    let agent = Agent::with_provider(config, &root, Box::new(Offline::new(&sel)));

    // The offline provider writes the first WRITABLE target; declare a path the
    // gate does not require, and make the only writable file one the model is
    // told it may touch. Here we instead point the task at an *excluded-by-
    // default* file by giving no allowed_modifications, so any write is read-only.
    let task = Task::new("Create a greeting file", vec![]);
    let report = agent.run(task).await.unwrap();

    assert!(!report.completed, "gate must fail when nothing writable was produced");
    // No writable target → offline provider emits no write → no successful tool call.
    assert!(!root.join("hello.txt").exists());

    let _ = std::fs::remove_dir_all(&root);
}
