//! Regression tests for the verify-gate "done" semantics, driven through the
//! bounded loop with deterministic providers (no network). These pin the
//! reliability of "done = your verify command exits 0" on real projects:
//!
//!   T1 — done requires *all* gates green, not just one.
//!   T2 — a silently-failing verify still feeds back actionable evidence, so the
//!        fail → fix → pass loop converges (regression for the empty-feedback
//!        bug: an empty evidence string used to leave the next attempt with an
//!        unchanged context and spin out to max_steps).
//!   T3 — a human gate escalates immediately with a blocker (no max_steps burn).
//!   T4 — a permanently-failing gate stops at max_steps with a blocker.

use async_trait::async_trait;
use orvena_core::config::agent::{AgentConfig, ProviderSelection, Tier};
use orvena_core::config::commands::Commands;
use orvena_core::config::context_budget::ContextBudgets;
use orvena_core::config::gates::{Gate, Gatekeeper, Gates};
use orvena_core::config::roles::{Role, Roles};
use orvena_core::config::Config;
use orvena_core::provider::offline::Offline;
use orvena_core::provider::{ChatRequest, ChatResponse, Provider};
use orvena_core::{Agent, Result, Task};
use std::sync::atomic::{AtomicUsize, Ordering};

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("orvena-gatetest-{tag}-{pid}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn automated(name: &str, condition: &str, verify: &str) -> Gate {
    Gate {
        name: name.into(),
        condition: condition.into(),
        verify: Some(verify.into()),
        gatekeeper: Gatekeeper::Automated,
        timeout_secs: None,
    }
}

/// Engineering tier, developer role able to read/write. Callers supply the gates
/// and the step budget for each scenario.
fn config(gates: Gates, max_steps: u32) -> Config {
    Config {
        agent: AgentConfig {
            provider: ProviderSelection { kind: "offline".into(), model: "stub".into(), base_url: None },
            tier: Tier::Engineering,
            default_role: "developer".into(),
            max_steps,
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
        gates,
        budgets: ContextBudgets::default(),
        commands: Commands::default(),
    }
}

fn offline_agent(root: &std::path::Path, config: Config) -> Agent {
    let sel = ProviderSelection { kind: "offline".into(), model: "stub".into(), base_url: None };
    Agent::with_provider(config, root, Box::new(Offline::new(&sel)))
}

// T1 — a run is "done" only when every gate passes.
#[tokio::test]
async fn all_gates_must_pass_for_done() {
    let root = temp_dir("conjunction");
    // gate-a is satisfiable by writing a.txt; gate-b requires b.txt, which the
    // offline provider (writes only the first writable target) never produces.
    let gates = Gates {
        gates: vec![
            automated("a-exists", "a.txt exists", "test -f a.txt"),
            automated("b-exists", "b.txt exists", "test -f b.txt"),
        ],
    };
    let agent = offline_agent(&root, config(gates, 2));
    let report = agent.run(Task::new("create a.txt", vec!["a.txt".into()])).await.unwrap();

    assert!(!report.completed, "one failing gate must block done");
    let a = report.gate_outcomes.iter().find(|g| g.gate == "a-exists").unwrap();
    let b = report.gate_outcomes.iter().find(|g| g.gate == "b-exists").unwrap();
    assert!(a.passed, "the satisfiable gate should pass");
    assert!(!b.passed, "the unsatisfiable gate should fail");
    assert!(root.join("a.txt").exists());
    let _ = std::fs::remove_dir_all(&root);
}

/// Two-step script: step 1 does nothing useful; step 2 writes the fix **only if**
/// the fed-back prompt carries the failed gate's condition — proving a silent
/// verify still produced actionable feedback.
struct FixOnFeedback {
    calls: AtomicUsize,
}
#[async_trait]
impl Provider for FixOnFeedback {
    fn id(&self) -> &str {
        "scripted"
    }
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse> {
        let prompt: String =
            req.messages.iter().map(|m| m.content.as_str()).collect::<Vec<_>>().join("\n");
        let content = match self.calls.fetch_add(1, Ordering::SeqCst) {
            0 => "thinking — no action yet".to_string(),
            _ => {
                // The gate `test -f done.txt` is silent on failure; only its
                // condition, fed back by the fixed driver, tells us what to do.
                if prompt.contains("done.txt is present") {
                    "<<<WRITE done.txt\nfixed\n>>>".to_string()
                } else {
                    "<<<WRITE wrong.txt\nfeedback-was-empty\n>>>".to_string()
                }
            }
        };
        Ok(ChatResponse { content, input_tokens: 0, output_tokens: 0 })
    }
}

// T2 — regression: a silent verify failure still drives convergence.
#[tokio::test]
async fn silent_verify_failure_still_drives_convergence() {
    let root = temp_dir("silent-converge");
    // `test -f done.txt` prints nothing when the file is missing — the exact
    // silent-failure shape that used to feed back an empty (useless) evidence.
    let gates =
        Gates { gates: vec![automated("done", "done.txt is present", "test -f done.txt")] };
    let agent = Agent::with_provider(
        config(gates, 4),
        &root,
        Box::new(FixOnFeedback { calls: AtomicUsize::new(0) }),
    );
    let report = agent
        .run(Task::new("make the check pass", vec!["done.txt".into(), "wrong.txt".into()]))
        .await
        .unwrap();

    assert!(
        report.completed,
        "the loop must converge once the silent failure's feedback names the target; blockers: {:?}",
        report.blockers
    );
    assert_eq!(report.steps, 2, "step 1 fails the silent gate, step 2 fixes it from the feedback");
    assert!(report.blockers.is_empty(), "a converging run records no blocker: {:?}", report.blockers);
    assert!(root.join("done.txt").exists(), "the fix must come from the fed-back condition");
    assert!(!root.join("wrong.txt").exists(), "the model should not have taken the no-feedback path");
    let _ = std::fs::remove_dir_all(&root);
}

// T3 — a human gate escalates immediately with a blocker.
#[tokio::test]
async fn human_gate_stops_with_blocker() {
    let root = temp_dir("human");
    let gates = Gates {
        gates: vec![Gate {
            name: "review".into(),
            condition: "a maintainer approved the change".into(),
            verify: None,
            gatekeeper: Gatekeeper::Human,
            timeout_secs: None,
        }],
    };
    // No writable target: the point is the human gate, not any write.
    let agent = offline_agent(&root, config(gates, 3));
    let report = agent.run(Task::new("do something needing review", vec![])).await.unwrap();

    assert!(!report.completed, "a human gate cannot be auto-confirmed");
    assert_eq!(report.steps, 1, "a human gate escalates on the first check, not at max_steps");
    assert!(
        report.blockers.iter().any(|b| b.contains("human")),
        "the escalation is recorded as a blocker: {:?}",
        report.blockers
    );
    let _ = std::fs::remove_dir_all(&root);
}

// T4 — a gate that can never pass stops the loop at max_steps with a blocker.
#[tokio::test]
async fn permanently_failing_gate_exhausts_max_steps() {
    let root = temp_dir("exhaust");
    let gates = Gates { gates: vec![automated("never", "an impossible condition", "false")] };
    let agent = offline_agent(&root, config(gates, 2));
    let report = agent.run(Task::new("attempt the impossible", vec!["a.txt".into()])).await.unwrap();

    assert!(!report.completed);
    assert_eq!(report.steps, 2, "the loop should use its full step budget");
    assert!(
        report.blockers.iter().any(|b| b.contains("reached max_steps")),
        "exhausting max_steps is recorded as a blocker: {:?}",
        report.blockers
    );
    let _ = std::fs::remove_dir_all(&root);
}
