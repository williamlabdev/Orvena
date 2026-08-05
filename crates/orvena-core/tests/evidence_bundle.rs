//! Evidence-bundle round-trip, using the deterministic offline provider: run one
//! bounded loop → export the report to disk → read the file back → confirm it
//! deserializes into an equal [`RunReport`] with the key evidence fields
//! (`completed`, `gate_outcomes`, `blockers`) intact.
//!
//! Both paths are covered, because the bundle's whole value is that it lands
//! either way:
//!   - a completed run leaves a passing-gate bundle;
//!   - an incomplete run (stopped by a failing gate) leaves one too.

use orvena_core::config::agent::{AgentConfig, ProviderSelection, Tier};
use orvena_core::config::commands::Commands;
use orvena_core::config::context_budget::ContextBudgets;
use orvena_core::config::gates::{Gate, Gatekeeper, Gates};
use orvena_core::config::roles::{Role, Roles};
use orvena_core::config::Config;
use orvena_core::metrics::evidence;
use orvena_core::provider::offline::Offline;
use orvena_core::{Agent, RunReport, Task};

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("orvena-evidence-{tag}-{pid}"));
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

fn offline_agent(root: &std::path::Path, config: Config) -> Agent {
    let sel = ProviderSelection {
        kind: "offline".into(),
        model: "stub".into(),
        base_url: None,
        api_key_env: None,
        sampling: None,
    };
    Agent::with_provider(config, root, Box::new(Offline::new(&sel)))
}

/// Export `report` under `root` and read it straight back off disk.
fn export_and_reload(
    report: &RunReport,
    root: &std::path::Path,
) -> (std::path::PathBuf, RunReport) {
    // A fixed timestamp keeps the test deterministic; the CLI supplies a real
    // clock value at runtime.
    let path = evidence::bundle_path(root, "fixed-run-id");
    evidence::write_bundle(report, &path).expect("bundle should write");
    assert!(path.exists(), "evidence bundle must be created on disk");
    let json = std::fs::read_to_string(&path).expect("bundle should be readable");
    let reloaded: RunReport = serde_json::from_str(&json).expect("bundle must deserialize back");
    (path, reloaded)
}

#[tokio::test]
async fn completed_run_exports_a_roundtrippable_bundle() {
    let root = temp_dir("completed");
    let agent = offline_agent(&root, dev_config(sel()));

    let task = Task::new("Create a greeting file", vec!["hello.txt".into()]);
    let report = agent.run(task).await.unwrap();
    assert!(
        report.completed,
        "precondition: this run should complete; blockers: {:?}",
        report.blockers
    );

    let (path, reloaded) = export_and_reload(&report, &root);

    // Written under runs/<timestamp>/evidence.json.
    assert!(
        path.ends_with("runs/fixed-run-id/evidence.json"),
        "unexpected layout: {}",
        path.display()
    );
    // Key evidence fields survive the round-trip.
    assert!(reloaded.completed, "a completed run must serialize as completed");
    assert_eq!(reloaded.task, report.task);
    assert_eq!(reloaded.gate_outcomes.len(), 1, "the single gate must be recorded");
    assert!(reloaded.gate_outcomes.iter().all(|g| g.gate == "file-exists" && g.passed));
    assert!(
        reloaded.blockers.is_empty(),
        "a completed run has no blockers: {:?}",
        reloaded.blockers
    );
    // Counters carry over too.
    assert_eq!(reloaded.steps, report.steps);
    assert_eq!(reloaded.tool_calls, report.tool_calls);

    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn incomplete_run_still_exports_a_bundle() {
    let root = temp_dir("incomplete");
    let agent = offline_agent(&root, dev_config(sel()));

    // No writable target → the offline provider emits no write → the gate never
    // passes → the run does not complete. The evidence must still land.
    let report = agent.run(Task::new("Create a greeting file", vec![])).await.unwrap();
    assert!(!report.completed, "precondition: this run should not complete");

    let (_path, reloaded) = export_and_reload(&report, &root);

    assert!(!reloaded.completed, "a failed run must serialize as not completed");
    assert!(
        reloaded.gate_outcomes.iter().any(|g| g.gate == "file-exists" && !g.passed),
        "the failing gate must be captured in the bundle: {:?}",
        reloaded.gate_outcomes,
    );

    let _ = std::fs::remove_dir_all(&root);
}

fn sel() -> ProviderSelection {
    ProviderSelection {
        kind: "offline".into(),
        model: "stub".into(),
        base_url: None,
        api_key_env: None,
        sampling: None,
    }
}
