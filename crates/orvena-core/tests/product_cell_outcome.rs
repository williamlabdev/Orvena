use std::path::{Path, PathBuf};

use orvena_core::config::agent::{AgentConfig, ProviderSelection, Tier};
use orvena_core::config::commands::Commands;
use orvena_core::config::context_budget::ContextBudgets;
use orvena_core::config::gates::{Gate, Gatekeeper, Gates};
use orvena_core::config::roles::{Role, Roles};
use orvena_core::config::Config;
use orvena_core::metrics::evidence;
use orvena_core::metrics::{RollbackEvidence, ValueSignal, ValueSignalResult};
use orvena_core::{Agent, Task};

fn temp_dir() -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("orvena-product-cell-outcome-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn config() -> Config {
    Config {
        agent: AgentConfig {
            provider: ProviderSelection {
                kind: "offline".into(),
                model: "native-test-model".into(),
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
                condition: "result.txt exists".into(),
                verify: Some("test -f result.txt".into()),
                gatekeeper: Gatekeeper::Automated,
                timeout_secs: None,
            }],
        },
        budgets: ContextBudgets::default(),
        commands: Commands::default(),
    }
}

#[tokio::test]
async fn native_run_writes_and_reads_product_cell_outcome_contract() {
    let root = temp_dir();
    let selection = config().agent.provider.clone();
    let agent = Agent::with_provider(
        config(),
        &root,
        Box::new(orvena_core::provider::offline::Offline::new(&selection)),
    );
    let report = agent
        .run(Task::new("Create the native ProductCell result", vec!["result.txt".into()]))
        .await
        .unwrap();
    assert!(report.completed, "native run should pass: {:?}", report.blockers);

    let report = report
        .with_product_cell_outcome(
            ValueSignal {
                result: ValueSignalResult::Pass,
                source_evidence_refs: vec!["orvena://runs/native-product-cell-acceptance".into()],
            },
            RollbackEvidence {
                rehearsed: true,
                evidence_ref: "package://aine/product-cell/native-rollback-acceptance".into(),
            },
            vec!["orvena://runs/native-product-cell-acceptance".into()],
        )
        .unwrap();
    let path = evidence::bundle_path(&root, "native-acceptance");
    evidence::write_bundle(&report, &path).unwrap();
    assert!(evidence::validate_bundle(&path).unwrap().is_empty());

    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let contract = value.get("outcome_contract").unwrap();
    let schema_text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../schemas/product-cell-outcome.v1.json"
    ))
    .unwrap();
    let schema: serde_json::Value = serde_json::from_str(&schema_text).unwrap();
    let validator = jsonschema::validator_for(&schema).unwrap();
    assert!(validator.is_valid(contract), "native outcome contract schema mismatch: {contract}");
    assert_eq!(contract["execution_provenance"]["provider"], "offline");
    assert_eq!(contract["execution_provenance"]["model"], "native-test-model");
    assert_eq!(contract["execution_provenance"]["run_count"], 1);
    assert_eq!(contract["value_signal"]["result"], "PASS");
    assert_eq!(contract["rollback"]["rehearsed"], true);

    if let Ok(retain_dir) = std::env::var("ORVENA_RETAIN_PRODUCT_CELL_EVIDENCE") {
        let retain_dir = Path::new(&retain_dir);
        std::fs::create_dir_all(retain_dir).unwrap();
        std::fs::copy(&path, retain_dir.join("evidence.json")).unwrap();
        std::fs::write(
            retain_dir.join("source.txt"),
            "native acceptance test: bounded offline Agent::run with outcome contract",
        )
        .unwrap();
    } else {
        let _ = std::fs::remove_dir_all(Path::new(&root));
    }
}
