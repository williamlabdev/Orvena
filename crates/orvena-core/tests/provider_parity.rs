//! Cross-provider **parity harness** (MVP exit criterion: Anthropic + Ollama
//! behave consistently). This is the repeatable, operational form of that check.
//!
//! It runs one golden task against a *real* provider chosen by environment
//! variables and asserts the behavioral **contract** that must hold regardless
//! of which model produced the output — NOT exact step/token counts, which
//! legitimately differ across models:
//!
//!   1. the run produces a structurally well-formed `RunReport`;
//!   2. completion semantics are internally consistent
//!      (`completed` ⇔ every gate passed);
//!   3. a real round-trip actually happened (the provider reports token usage —
//!      an `offline` stub is only a regression baseline, per MVP-SCOPE §5, and
//!      cannot prove cross-provider consistency);
//!   4. "evidence by default" holds — the bundle exports and round-trips.
//!
//! The test is `#[ignore]`d so `cargo test` stays offline and deterministic. Run
//! it explicitly against each provider and confirm both satisfy the same
//! contract (see docs/provider-parity.md):
//!
//! ```text
//! # Ollama (local, no key)
//! ORVENA_PARITY_PROVIDER=ollama ORVENA_PARITY_MODEL=qwen3:14b \
//!   cargo test -p orvena-core --test provider_parity -- --ignored --nocapture
//!
//! # Anthropic (hosted; needs ANTHROPIC_API_KEY)
//! ANTHROPIC_API_KEY=sk-... ORVENA_PARITY_PROVIDER=anthropic \
//!   ORVENA_PARITY_MODEL=claude-opus-4-8 \
//!   cargo test -p orvena-core --test provider_parity -- --ignored --nocapture
//! ```

use orvena_core::config::agent::{AgentConfig, ProviderSelection, Tier};
use orvena_core::config::commands::Commands;
use orvena_core::config::context_budget::ContextBudgets;
use orvena_core::config::gates::{Gate, Gatekeeper, Gates};
use orvena_core::config::roles::{Role, Roles};
use orvena_core::config::Config;
use orvena_core::metrics::evidence;
use orvena_core::{Agent, RunReport, Task};

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("orvena-parity-{tag}-{pid}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

const MAX_STEPS: u32 = 3;

#[tokio::test]
#[ignore = "hits a real provider; run explicitly with ORVENA_PARITY_PROVIDER + --ignored"]
async fn provider_satisfies_the_parity_contract() {
    let Ok(kind) = std::env::var("ORVENA_PARITY_PROVIDER") else {
        eprintln!(
            "SKIP: set ORVENA_PARITY_PROVIDER (+ ORVENA_PARITY_MODEL) to run the parity check"
        );
        return;
    };
    let model = std::env::var("ORVENA_PARITY_MODEL")
        .expect("ORVENA_PARITY_MODEL must name a model the provider serves");
    let base_url = std::env::var("ORVENA_PARITY_BASE_URL").ok();
    eprintln!("parity: running the golden task against '{kind}' / '{model}'");

    let root = temp_dir(&kind);
    let config = Config {
        agent: AgentConfig {
            provider: ProviderSelection { kind: kind.clone(), model, base_url },
            tier: Tier::Light,
            default_role: "developer".into(),
            max_steps: MAX_STEPS,
        },
        roles: Roles {
            roles: vec![Role {
                name: "developer".into(),
                allowed_tools: vec!["fs.read".into(), "fs.write".into()],
                forbidden_tools: vec![],
                knowledge_scope: vec![],
            }],
        },
        // "done = the file exists" — a real check, so completion reflects whether
        // the model actually drove the loop to a verified done.
        gates: Gates {
            gates: vec![Gate {
                name: "hello-exists".into(),
                condition: "hello.txt exists".into(),
                verify: Some("test -f hello.txt".into()),
                gatekeeper: Gatekeeper::Automated,
                timeout_secs: Some(30),
            }],
        },
        budgets: ContextBudgets::default(),
        commands: Commands::default(),
    };

    // Agent::new builds the *real* provider from config/env (e.g. reads
    // ANTHROPIC_API_KEY) — this is the genuine integration path, not a stub.
    let agent = Agent::new(config, &root).expect("provider should build (is the key/model set?)");
    let task = Task::new(
        "Create a file named hello.txt containing the single word: hello",
        vec!["hello.txt".into()],
    );
    let report = agent.run(task).await.expect("a real-provider run must not hard-error");
    eprintln!(
        "parity[{kind}]: completed={} steps={} tokens={} gates={:?}",
        report.completed,
        report.steps,
        report.total_tokens(),
        report.gate_outcomes.iter().map(|g| (&g.gate, g.passed)).collect::<Vec<_>>(),
    );

    // Contract 1 — structurally well-formed.
    assert!(
        report.steps >= 1 && report.steps <= MAX_STEPS,
        "steps must be within [1, max_steps]: {}",
        report.steps
    );
    assert_eq!(report.gate_outcomes.len(), 1, "the one configured gate was evaluated");
    assert_eq!(report.gate_outcomes[0].gate, "hello-exists");
    assert!(report.blockers.iter().all(|b| !b.trim().is_empty()), "blockers carry a message");

    // Contract 2 — completion semantics are internally consistent.
    if report.completed {
        assert!(
            report.gate_outcomes.iter().all(|g| g.passed),
            "completed ⇒ every gate passed: {:?}",
            report.gate_outcomes
        );
    } else {
        let unmet =
            report.gate_outcomes.iter().any(|g| !g.passed) || !report.blockers.is_empty();
        assert!(unmet, "not completed ⇒ a gate failed or a blocker was recorded");
    }

    // Contract 3 — a real model round-trip actually happened.
    assert!(report.total_tokens() > 0, "a real provider must report token usage (not a stub)");

    // Contract 4 — evidence by default: the bundle exports and round-trips.
    let path = evidence::bundle_path(&root, "parity");
    evidence::write_bundle(&report, &path).expect("evidence bundle writes");
    let reloaded: RunReport =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).expect("bundle deserializes");
    assert_eq!(reloaded.completed, report.completed);
    assert_eq!(reloaded.gate_outcomes.len(), report.gate_outcomes.len());
    assert_eq!(reloaded.task, report.task);

    let _ = std::fs::remove_dir_all(&root);
}
