//! The frozen evidence-bundle contract (schema v1, D4) exercised across every
//! exit path the driver can produce (M3): success, gate-fail to max_steps,
//! human-gate stop, provider error, and the bench-only ungoverned baseline.
//! Each bundle is checked twice — by the shipped hand-rolled validator AND by
//! a real JSON-Schema engine against `schemas/evidence.v1.json` — so the
//! schema file and the validator cannot drift apart silently.

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
    let dir = std::env::temp_dir().join(format!("orvena-evsch-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn config(gates: Gates, tier: Tier) -> Config {
    Config {
        agent: AgentConfig {
            provider: ProviderSelection {
                kind: "offline".into(),
                model: "stub".into(),
                base_url: None,
                api_key_env: None,
            },
            tier,
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
        gates,
        budgets: ContextBudgets::default(),
        commands: Commands::default(),
    }
}

fn gate(verify: &str) -> Gates {
    Gates {
        gates: vec![Gate {
            name: "check".into(),
            condition: "the check passes".into(),
            verify: Some(verify.into()),
            gatekeeper: Gatekeeper::Automated,
            timeout_secs: None,
        }],
    }
}

struct FailingProvider;

#[async_trait]
impl Provider for FailingProvider {
    fn id(&self) -> &str {
        "failing"
    }
    async fn chat(&self, _req: ChatRequest) -> Result<ChatResponse> {
        Err(Error::Other(anyhow::anyhow!("simulated outage")))
    }
}

/// Both judges on one bundle: the shipped validator and the schema engine.
fn assert_valid_both_ways(path: &std::path::Path) {
    let problems = evidence::validate_bundle(path).unwrap();
    assert!(problems.is_empty(), "shipped validator rejected the bundle: {problems:?}");

    let schema_text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../schemas/evidence.v1.json"
    ))
    .expect("schema file exists");
    let schema: serde_json::Value = serde_json::from_str(&schema_text).unwrap();
    let validator = jsonschema::validator_for(&schema).expect("schema file compiles");
    let instance: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let errors: Vec<String> = validator.iter_errors(&instance).map(|e| e.to_string()).collect();
    assert!(errors.is_empty(), "schema engine rejected the bundle: {errors:?}");
}

async fn offline_run(
    gates: Gates,
    tier: Tier,
    writes: Vec<String>,
    tag: &str,
) -> std::path::PathBuf {
    let root = temp_dir(tag);
    let sel = ProviderSelection {
        kind: "offline".into(),
        model: "stub".into(),
        base_url: None,
        api_key_env: None,
    };
    let cfg = config(gates, tier);
    let provider = orvena_core::build_chat_provider(&sel).unwrap();
    let agent = Agent::with_provider(cfg, &root, provider);
    let report = agent.run(Task::new("do the task", writes)).await.unwrap();
    let path = evidence::bundle_path(&root, "run");
    evidence::write_bundle(&report, &path).unwrap();
    path
}

#[tokio::test]
async fn every_driver_exit_path_leaves_a_schema_valid_bundle() {
    // Success: the write satisfies the gate.
    let p = offline_run(gate("test -f a.txt"), Tier::Engineering, vec!["a.txt".into()], "ok").await;
    assert_valid_both_ways(&p);

    // Gate-fail to max_steps: the gate can never pass.
    let p = offline_run(gate("false"), Tier::Engineering, vec!["a.txt".into()], "maxsteps").await;
    assert_valid_both_ways(&p);

    // Human gate: stops on the first check with a blocker.
    let human = Gates {
        gates: vec![Gate {
            name: "review".into(),
            condition: "a maintainer approved".into(),
            verify: None,
            gatekeeper: Gatekeeper::Human,
            timeout_secs: None,
        }],
    };
    let p = offline_run(human, Tier::Engineering, vec![], "human").await;
    assert_valid_both_ways(&p);

    // Provider error: captured as a blocker, still a full report.
    let root = temp_dir("perr");
    let agent = Agent::with_provider(
        config(gate("test -f x"), Tier::Engineering),
        &root,
        Box::new(FailingProvider),
    );
    let report = agent.run(Task::new("t", vec!["x.txt".into()])).await.unwrap();
    let path = evidence::bundle_path(&root, "run");
    evidence::write_bundle(&report, &path).unwrap();
    assert_valid_both_ways(&path);
}

#[test]
fn the_validator_and_the_schema_engine_agree_on_broken_bundles() {
    let schema_text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../schemas/evidence.v1.json"
    ))
    .unwrap();
    let schema: serde_json::Value = serde_json::from_str(&schema_text).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();

    let good = serde_json::to_value(RunReport::new("t").finished(true)).unwrap();
    assert!(evidence::validate_bundle_value(&good).is_empty());
    assert!(validator.is_valid(&good));

    let breakages: Vec<(&str, serde_json::Value)> = vec![
        ("missing completed", {
            let mut v = good.clone();
            v.as_object_mut().unwrap().remove("completed");
            v
        }),
        ("steps as string", {
            let mut v = good.clone();
            v["steps"] = serde_json::json!("three");
            v
        }),
        ("wrong schema id", {
            let mut v = good.clone();
            v["schema"] = serde_json::json!("orvena-evidence-v99");
            v
        }),
        ("blockers not strings", {
            let mut v = good.clone();
            v["blockers"] = serde_json::json!([1, 2]);
            v
        }),
        ("malformed gate outcome", {
            let mut v = good.clone();
            v["gate_outcomes"] = serde_json::json!([{ "gate": "g" }]);
            v
        }),
    ];
    for (what, broken) in breakages {
        assert!(
            !evidence::validate_bundle_value(&broken).is_empty(),
            "shipped validator must reject: {what}"
        );
        assert!(!validator.is_valid(&broken), "schema engine must reject: {what}");
    }
}

#[test]
fn a_truncated_file_reads_as_invalid_not_a_crash() {
    let dir = temp_dir("truncated");
    let path = dir.join("evidence.json");
    std::fs::write(&path, "{\"schema\": \"orvena-evidence-v1\", \"task\":").unwrap();
    let problems = evidence::validate_bundle(&path).unwrap();
    assert!(problems.iter().any(|p| p.contains("not valid JSON")), "{problems:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_pre_v1_bundle_without_the_schema_field_still_deserializes_as_v1() {
    // Bundles written before the `schema` field existed are v1 by definition —
    // reading them back must not break (serde default fills the identifier).
    let old = serde_json::json!({
        "task": "legacy",
        "completed": true,
        "steps": 1,
        "input_tokens": 10,
        "output_tokens": 5,
        "tool_calls": 1,
        "gate_outcomes": [],
        "blockers": []
    });
    let report: RunReport = serde_json::from_value(old).unwrap();
    assert_eq!(report.schema, orvena_core::metrics::EVIDENCE_SCHEMA_V1);
    // Additive fields read back as "unrecorded", never as fabricated data: a
    // legacy bundle has no step budget and no typed exit reason.
    assert_eq!(report.max_steps, 0, "legacy budget is unrecorded (0), not invented");
    assert_eq!(report.exit, orvena_core::metrics::ExitReason::Unrecorded);
    // slice-026: a legacy bundle recorded no action kinds. That must read as
    // "not attributable", never as an all-zero breakdown — a consumer would
    // otherwise conclude the loop never searched, from a file that never said.
    assert!(report.action_counts.is_none(), "unrecorded attribution is None, not zeros");
    // But as a FILE on disk it predates the frozen contract and the validator
    // reports the missing fields — honesty over leniency.
}

// The committed parity artifact is only worth committing if it is a valid
// bundle AND identifies its own backend — a bundle that cannot say which
// provider produced it cannot back a parity claim.
#[test]
fn the_committed_parity_artifact_is_valid_and_self_describing() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/parity-results");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return; // no artifacts committed yet — nothing to check
    };
    let mut checked = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let problems = orvena_core::metrics::evidence::validate_bundle(&path)
            .unwrap_or_else(|e| panic!("{} is not readable as a bundle: {e}", path.display()));
        assert!(problems.is_empty(), "{} fails schema v1: {problems:?}", path.display());

        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        for field in ["provider", "model"] {
            assert!(
                v[field].as_str().is_some_and(|s| !s.is_empty()),
                "{} has no '{field}' — it cannot identify what produced it",
                path.display()
            );
        }
        checked += 1;
    }
    assert!(checked > 0, "docs/parity-results exists but holds no bundles");
}
