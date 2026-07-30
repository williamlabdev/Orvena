//! Integration tests for the shell RUN tool wired through the bounded loop, using
//! a scripted offline provider (mirrors `search_roundtrip.rs`). Covers the four
//! slice-002 acceptance criteria:
//!
//!   AC1 — an undeclared command name is denied (Error::Scope) and, in the
//!         engineering tier, hard-stops the loop with a blocker.
//!   AC2 — a `mutating` command is denied even when declared.
//!   AC3 — round-trip: RUN fails (exit != 0) -> the model fixes the file from the
//!         fed-back evidence -> RUN passes; the engineering tier never hard-stops
//!         on the failing test, and the run finishes completed.
//!   AC4 — a gate whose `verify` outruns its timeout fails verify (covered as a
//!         unit test in `governance/gate.rs`; re-asserted end-to-end here).

use async_trait::async_trait;
use orvena_core::config::agent::{AgentConfig, ProviderSelection, Tier};
use orvena_core::config::commands::{Command, Commands, Intent};
use orvena_core::config::context_budget::ContextBudgets;
use orvena_core::config::gates::{Gate, Gatekeeper, Gates};
use orvena_core::config::roles::{Role, Roles};
use orvena_core::config::Config;
use orvena_core::provider::{ChatRequest, ChatResponse, Provider};
use orvena_core::{Agent, Result, Task};
use std::sync::atomic::{AtomicUsize, Ordering};

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("orvena-runtest-{tag}-{pid}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn cmd(name: &str, argv: &[&str], intent: Intent) -> Command {
    Command {
        name: name.into(),
        argv: argv.iter().map(|s| s.to_string()).collect(),
        intent,
        timeout_secs: Some(30),
    }
}

/// Base config: engineering tier, developer role with `shell.run`. Callers tweak
/// the commands and gates for each scenario.
fn config(commands: Commands, gates: Gates) -> Config {
    Config {
        agent: AgentConfig {
            provider: ProviderSelection {
                kind: "offline".into(),
                model: "scripted".into(),
                base_url: None,
                api_key_env: None,
            },
            tier: Tier::Engineering,
            default_role: "developer".into(),
            max_steps: 4,
            sandbox: Default::default(),
        },
        roles: Roles {
            roles: vec![Role {
                name: "developer".into(),
                allowed_tools: vec![
                    "fs.read".into(),
                    "fs.write".into(),
                    "grep.search".into(),
                    "shell.run".into(),
                ],
                forbidden_tools: vec![],
                knowledge_scope: vec![],
            }],
        },
        gates,
        budgets: ContextBudgets::default(),
        commands,
    }
}

/// A provider that always emits the same block, for the single-step denial tests.
struct Fixed(&'static str);
#[async_trait]
impl Provider for Fixed {
    fn id(&self) -> &str {
        "scripted"
    }
    async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse> {
        Ok(ChatResponse { content: self.0.into(), input_tokens: 0, output_tokens: 0 })
    }
}

// AC1 — an undeclared name hard-stops the engineering-tier loop with a blocker.
#[tokio::test]
async fn undeclared_command_is_denied_and_stops_engineering_loop() {
    let root = temp_dir("undeclared");
    let commands = Commands { commands: vec![cmd("test", &["true"], Intent::ReadOnly)] };
    let gates = Gates {
        gates: vec![Gate {
            name: "never".into(),
            condition: "unsatisfiable".into(),
            verify: Some("false".into()),
            gatekeeper: Gatekeeper::Automated,
            timeout_secs: None,
        }],
    };
    let agent =
        Agent::with_provider(config(commands, gates), &root, Box::new(Fixed("<<<RUN deploy\n>>>")));
    let report = agent.run(Task::new("run an undeclared command", vec![])).await.unwrap();

    assert!(!report.completed, "an unauthorized RUN must not complete");
    assert_eq!(report.steps, 1, "engineering tier hard-stops on the first denied RUN");
    assert!(
        report.blockers.iter().any(|b| b.contains("not declared")),
        "the denial is recorded as a blocker: {:?}",
        report.blockers
    );
    let _ = std::fs::remove_dir_all(&root);
}

// AC2 — a declared but `mutating` command is denied when the model triggers it.
#[tokio::test]
async fn mutating_command_is_denied_even_when_declared() {
    let root = temp_dir("mutating");
    let commands = Commands { commands: vec![cmd("fmt-fix", &["true"], Intent::Mutating)] };
    let gates = Gates {
        gates: vec![Gate {
            name: "never".into(),
            condition: "unsatisfiable".into(),
            verify: Some("false".into()),
            gatekeeper: Gatekeeper::Automated,
            timeout_secs: None,
        }],
    };
    let agent = Agent::with_provider(
        config(commands, gates),
        &root,
        Box::new(Fixed("<<<RUN fmt-fix\n>>>")),
    );
    let report = agent.run(Task::new("trigger a mutating command", vec![])).await.unwrap();

    assert!(!report.completed);
    assert_eq!(report.steps, 1, "a mutating RUN is an authorization denial → hard stop");
    assert!(
        report.blockers.iter().any(|b| b.contains("mutating")),
        "the mutating denial is recorded: {:?}",
        report.blockers
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// The round-trip script: step 1 runs `test` (which fails until `marker.txt`
/// contains DONE); step 2, having seen the failure evidence fed back, writes the
/// fix AND re-runs `test`. Completion is driven by the gate, whose verify matches
/// the command — so `completed` implies the second RUN would pass.
struct RoundTrip {
    calls: AtomicUsize,
}
#[async_trait]
impl Provider for RoundTrip {
    fn id(&self) -> &str {
        "scripted"
    }
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse> {
        let prompt: String =
            req.messages.iter().map(|m| m.content.as_str()).collect::<Vec<_>>().join("\n");
        let content = match self.calls.fetch_add(1, Ordering::SeqCst) {
            0 => "<<<RUN test\n>>>".to_string(),
            _ => {
                // Only write the correct fix if the failing-RUN evidence was fed
                // back — proving the loop actually returned the tool output.
                let saw_failure = prompt.contains("RUN 'test'") && prompt.contains("exit 1");
                let payload = if saw_failure { "DONE" } else { "EVIDENCE-NOT-FED-BACK" };
                format!("<<<WRITE marker.txt\n{payload}\n>>>\n<<<RUN test\n>>>")
            }
        };
        Ok(ChatResponse { content, input_tokens: 0, output_tokens: 0 })
    }
}

// AC3 — RUN fails → fix from evidence → RUN passes; engineering never hard-stops
// on the failing test, and the run finishes completed.
#[tokio::test]
async fn failing_run_then_fix_then_passing_run_completes() {
    let root = temp_dir("roundtrip");
    // Pre-create the marker with non-matching content so the first RUN exits 1
    // (grep on a *missing* file would exit 2 — a different failure mode).
    std::fs::write(root.join("marker.txt"), "TODO\n").unwrap();
    // `test` and the gate share the same condition: marker.txt must contain DONE.
    let check = "grep -q DONE marker.txt";
    let commands = Commands { commands: vec![cmd("test", &["sh", "-c", check], Intent::ReadOnly)] };
    let gates = Gates {
        gates: vec![Gate {
            name: "marker-done".into(),
            condition: "marker.txt contains DONE".into(),
            verify: Some(check.into()),
            gatekeeper: Gatekeeper::Automated,
            timeout_secs: None,
        }],
    };
    let agent = Agent::with_provider(
        config(commands, gates),
        &root,
        Box::new(RoundTrip { calls: AtomicUsize::new(0) }),
    );
    let report =
        agent.run(Task::new("make the tests pass", vec!["marker.txt".into()])).await.unwrap();

    assert!(
        report.completed,
        "the loop must finish once the fix makes the test/gate pass; blockers: {:?}",
        report.blockers
    );
    assert_eq!(report.steps, 2, "step 1 runs a failing test, step 2 fixes and re-runs");
    assert!(
        report.blockers.is_empty(),
        "a failing read_only RUN is evidence-only — it must NOT record a blocker: {:?}",
        report.blockers
    );
    let written = std::fs::read_to_string(root.join("marker.txt")).unwrap();
    assert_eq!(written.trim(), "DONE", "the fix must come from the fed-back RUN evidence");
    let _ = std::fs::remove_dir_all(&root);
}

// AC4 — a gate whose verify outruns its timeout fails verify (does not hang).
#[tokio::test]
async fn gate_timeout_is_a_verify_failure() {
    let root = temp_dir("gate-timeout");
    let commands = Commands::default();
    let gates = Gates {
        gates: vec![Gate {
            name: "slow-verify".into(),
            condition: "a verify that never returns".into(),
            verify: Some("sleep 30".into()),
            gatekeeper: Gatekeeper::Automated,
            timeout_secs: Some(1),
        }],
    };
    // The model writes nothing useful; the point is the gate times out → fails →
    // the loop re-attempts and stops at max_steps (rather than hanging forever).
    // Keep max_steps small: each step waits out the 1s gate timeout.
    let mut cfg = config(commands, gates);
    cfg.agent.max_steps = 2;
    let agent = Agent::with_provider(cfg, &root, Box::new(Fixed("no action here")));
    let report = agent.run(Task::new("hit a slow gate", vec![])).await.unwrap();

    assert!(!report.completed, "a timed-out gate never passes");
    assert!(
        report.gate_outcomes.iter().any(|g| g.gate == "slow-verify" && !g.passed),
        "the slow gate is recorded as failed: {:?}",
        report.gate_outcomes
    );
    let _ = std::fs::remove_dir_all(&root);
}
