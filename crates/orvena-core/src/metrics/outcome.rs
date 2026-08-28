//! Native ProductCell outcome contract.
//!
//! The contract is an additive evidence payload.  A native run can attach an
//! explicit value signal and rollback rehearsal after execution; provenance is
//! copied from the run itself.  Missing or unverified value evidence is never
//! upgraded to `PASS` by this module.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::RunReport;
use crate::{Error, Result};

pub const PRODUCT_CELL_OUTCOME_SCHEMA_V1: &str = "orvena-product-cell-outcome-v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ValueSignalResult {
    Pass,
    Fail,
    Inconclusive,
    Blocked,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValueSignal {
    pub result: ValueSignalResult,
    pub source_evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionProvenance {
    pub provider: String,
    pub model: String,
    pub run_count: u32,
    pub source_evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RollbackEvidence {
    pub rehearsed: bool,
    pub evidence_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductCellOutcomeContract {
    pub schema: String,
    pub value_signal: ValueSignal,
    pub execution_provenance: ExecutionProvenance,
    pub rollback: RollbackEvidence,
}

impl ProductCellOutcomeContract {
    /// Build a contract for one native run without inventing a value result.
    pub fn from_native_run(
        report: &RunReport,
        value_signal: ValueSignal,
        rollback: RollbackEvidence,
        source_evidence_refs: Vec<String>,
    ) -> Result<Self> {
        Self::from_native_runs(report, value_signal, rollback, source_evidence_refs, 1)
    }

    /// Build a contract that honestly aggregates `run_count` bounded native runs.
    ///
    /// `report` is the final run of the series; provider/model provenance is
    /// copied from it, so every aggregated run must share that provenance. The
    /// provenance refs must contain at least `run_count` distinct portable
    /// refs — one per aggregated run — so a count is never claimed without a
    /// matching evidence reference.
    pub fn from_native_runs(
        report: &RunReport,
        value_signal: ValueSignal,
        rollback: RollbackEvidence,
        source_evidence_refs: Vec<String>,
        run_count: u32,
    ) -> Result<Self> {
        if run_count == 0 {
            return Err(invalid("execution_provenance.run_count must be positive"));
        }
        if matches!(value_signal.result, ValueSignalResult::Pass) && !report.completed {
            return Err(invalid("PASS value signal requires a completed native run"));
        }
        validate_refs(&value_signal.source_evidence_refs, "value_signal.source_evidence_refs")?;
        validate_refs(&source_evidence_refs, "execution_provenance.source_evidence_refs")?;
        let distinct: std::collections::BTreeSet<&str> =
            source_evidence_refs.iter().map(|v| v.trim()).collect();
        if (distinct.len() as u32) < run_count {
            return Err(invalid(&format!(
                "execution_provenance.run_count is {run_count} but only {} distinct \
                 evidence refs were supplied; one ref per aggregated run is required",
                distinct.len()
            )));
        }
        validate_rollback(&rollback)?;
        let provider = report
            .provider
            .clone()
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| invalid("native run provider provenance is missing"))?;
        let model = report
            .model
            .clone()
            .filter(|v| !v.trim().is_empty())
            .ok_or_else(|| invalid("native run model provenance is missing"))?;
        Ok(Self {
            schema: PRODUCT_CELL_OUTCOME_SCHEMA_V1.into(),
            value_signal,
            execution_provenance: ExecutionProvenance {
                provider,
                model,
                run_count,
                source_evidence_refs,
            },
            rollback,
        })
    }

    /// Honest default for a run whose product value has not been established.
    pub fn inconclusive_for_native_run(
        report: &RunReport,
        source_evidence_ref: String,
    ) -> Result<Self> {
        Self::from_native_run(
            report,
            ValueSignal {
                result: ValueSignalResult::Inconclusive,
                source_evidence_refs: vec![source_evidence_ref.clone()],
            },
            RollbackEvidence {
                rehearsed: false,
                evidence_ref: "orvena://rollback/not-rehearsed".into(),
            },
            vec![source_evidence_ref],
        )
    }

    pub fn validate(&self) -> Result<()> {
        let value = serde_json::to_value(self)?;
        let problems = validate_outcome_contract_value(&value);
        if problems.is_empty() {
            Ok(())
        } else {
            Err(invalid(&problems.join("; ")))
        }
    }
}

/// Validate the JSON shape without deserializing defaults over missing fields.
pub fn validate_outcome_contract_value(value: &serde_json::Value) -> Vec<String> {
    use serde_json::Value;
    let Some(obj) = value.as_object() else {
        return vec!["outcome_contract is not an object".into()];
    };
    let mut problems = Vec::new();
    if obj.get("schema").and_then(Value::as_str) != Some(PRODUCT_CELL_OUTCOME_SCHEMA_V1) {
        problems.push("outcome_contract.schema is not the native v1 identifier".into());
    }
    let Some(signal) = obj.get("value_signal").and_then(Value::as_object) else {
        problems.push("outcome_contract.value_signal is not an object".into());
        return problems;
    };
    if !matches!(
        signal.get("result").and_then(Value::as_str),
        Some("PASS" | "FAIL" | "INCONCLUSIVE" | "BLOCKED")
    ) {
        problems.push("outcome_contract.value_signal.result is invalid".into());
    }
    require_refs(signal, "value_signal", &mut problems);
    let Some(provenance) = obj.get("execution_provenance").and_then(Value::as_object) else {
        problems.push("outcome_contract.execution_provenance is not an object".into());
        return problems;
    };
    for field in ["provider", "model"] {
        if provenance.get(field).and_then(Value::as_str).is_none() {
            problems.push(format!("outcome_contract.execution_provenance.{field} is missing"));
        }
    }
    if provenance.get("run_count").and_then(Value::as_u64).unwrap_or(0) == 0 {
        problems.push("outcome_contract.execution_provenance.run_count must be positive".into());
    }
    require_refs(provenance, "execution_provenance", &mut problems);
    let Some(rollback) = obj.get("rollback").and_then(Value::as_object) else {
        problems.push("outcome_contract.rollback is not an object".into());
        return problems;
    };
    if rollback.get("rehearsed").and_then(Value::as_bool).is_none() {
        problems.push("outcome_contract.rollback.rehearsed is missing".into());
    }
    if rollback.get("evidence_ref").and_then(Value::as_str).is_none_or(|v| v.trim().is_empty()) {
        problems.push("outcome_contract.rollback.evidence_ref is missing".into());
    }
    problems
}

fn require_refs(
    object: &serde_json::Map<String, serde_json::Value>,
    label: &str,
    problems: &mut Vec<String>,
) {
    let Some(refs) = object.get("source_evidence_refs").and_then(|v| v.as_array()) else {
        problems.push(format!("outcome_contract.{label}.source_evidence_refs is not an array"));
        return;
    };
    if refs.is_empty()
        || refs.iter().any(|v| {
            v.as_str().is_none_or(|s| {
                s.trim().is_empty() || s.starts_with('/') || s.starts_with("file://")
            })
        })
    {
        problems.push(format!(
            "outcome_contract.{label}.source_evidence_refs must be portable and non-empty"
        ));
    }
}

fn validate_refs(refs: &[String], label: &str) -> Result<()> {
    if refs.is_empty()
        || refs.iter().any(|v| {
            let s = v.trim();
            s.is_empty() || s.starts_with('/') || s.starts_with("file://")
        })
    {
        return Err(invalid(&format!("{label} must contain portable, non-empty refs")));
    }
    Ok(())
}

fn validate_rollback(value: &RollbackEvidence) -> Result<()> {
    let s = value.evidence_ref.trim();
    if s.is_empty() || s.starts_with('/') || s.starts_with("file://") {
        return Err(invalid("rollback.evidence_ref must be portable"));
    }
    Ok(())
}

fn invalid(message: &str) -> Error {
    Error::Config(format!("product-cell outcome contract: {message}"))
}

#[derive(Debug, Clone)]
struct Snapshot {
    path: PathBuf,
    contents: Option<Vec<u8>>,
}

/// A bounded, declared-path rollback rehearsal for a native workspace.
pub struct RollbackJournal {
    root: PathBuf,
    snapshots: Vec<Snapshot>,
}

impl RollbackJournal {
    pub fn capture(root: &Path, paths: &[PathBuf]) -> Result<Self> {
        if root.is_symlink() {
            return Err(invalid("rollback root must not be a symlink"));
        }
        let root = root.canonicalize()?;
        let mut snapshots = Vec::with_capacity(paths.len());
        for relative in paths {
            let target = safe_target(&root, relative)?;
            let contents = if target.is_symlink() {
                return Err(invalid("rollback target must not be a symlink"));
            } else if target.exists() {
                Some(std::fs::read(&target)?)
            } else {
                None
            };
            snapshots.push(Snapshot { path: target, contents });
        }
        Ok(Self { root, snapshots })
    }

    pub fn restore(&self) -> Result<()> {
        for snapshot in &self.snapshots {
            let relative = snapshot
                .path
                .strip_prefix(&self.root)
                .map_err(|_| invalid("rollback path escaped root"))?;
            let target = safe_target(&self.root, relative)?;
            match &snapshot.contents {
                Some(contents) => {
                    if let Some(parent) = target.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&target, contents)?;
                }
                None if target.exists() => std::fs::remove_file(&target)?,
                None => {}
            }
        }
        Ok(())
    }

    pub fn rehearse<F>(self, evidence_ref: String, operation: F) -> Result<RollbackEvidence>
    where
        F: FnOnce() -> Result<()>,
    {
        validate_rollback(&RollbackEvidence {
            rehearsed: true,
            evidence_ref: evidence_ref.clone(),
        })?;
        let operation_result = operation();
        let restore_result = self.restore();
        operation_result?;
        restore_result?;
        Ok(RollbackEvidence { rehearsed: true, evidence_ref })
    }
}

fn safe_target(root: &Path, relative: &Path) -> Result<PathBuf> {
    if relative.as_os_str().is_empty()
        || relative.is_absolute()
        || relative.components().any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(invalid("rollback paths must be non-empty relative paths"));
    }
    let target = root.join(relative);
    let mut cursor = root.to_path_buf();
    for component in relative.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(part) => {
                cursor.push(part);
                if cursor.is_symlink() {
                    return Err(invalid("rollback path must not contain symlink components"));
                }
            }
            _ => return Err(invalid("rollback paths must be normal relative paths")),
        }
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::agent::ProviderSelection;

    fn report(completed: bool) -> RunReport {
        RunReport::new("native ProductCell task")
            .with_provenance(&ProviderSelection {
                kind: "offline".into(),
                model: "stub".into(),
                base_url: None,
                api_key_env: None,
                sampling: None,
            })
            .finished(completed)
    }

    #[test]
    fn native_contract_carries_value_provenance_and_rollback() {
        let contract = ProductCellOutcomeContract::from_native_run(
            &report(true),
            ValueSignal {
                result: ValueSignalResult::Pass,
                source_evidence_refs: vec!["orvena://runs/native-1".into()],
            },
            RollbackEvidence {
                rehearsed: true,
                evidence_ref: "package://rollback/native-1".into(),
            },
            vec!["orvena://runs/native-1".into()],
        )
        .unwrap();
        assert_eq!(contract.execution_provenance.run_count, 1);
        assert!(contract.validate().is_ok());
    }

    #[test]
    fn aggregate_contract_carries_run_count_with_one_ref_per_run() {
        let contract = ProductCellOutcomeContract::from_native_runs(
            &report(true),
            ValueSignal {
                result: ValueSignalResult::Pass,
                source_evidence_refs: vec!["orvena://runs/native-3".into()],
            },
            RollbackEvidence {
                rehearsed: true,
                evidence_ref: "package://rollback/native-3".into(),
            },
            vec![
                "orvena://runs/native-1".into(),
                "orvena://runs/native-2".into(),
                "orvena://runs/native-3".into(),
            ],
            3,
        )
        .unwrap();
        assert_eq!(contract.execution_provenance.run_count, 3);
        assert_eq!(contract.execution_provenance.source_evidence_refs.len(), 3);
        assert!(contract.validate().is_ok());
    }

    #[test]
    fn aggregate_contract_rejects_count_without_matching_refs() {
        let error = ProductCellOutcomeContract::from_native_runs(
            &report(true),
            ValueSignal {
                result: ValueSignalResult::Pass,
                source_evidence_refs: vec!["orvena://runs/native-2".into()],
            },
            RollbackEvidence {
                rehearsed: true,
                evidence_ref: "package://rollback/native-2".into(),
            },
            vec!["orvena://runs/native-1".into(), "orvena://runs/native-1".into()],
            3,
        )
        .unwrap_err();
        assert!(error.to_string().contains("one ref per aggregated run"));
    }

    #[test]
    fn aggregate_contract_rejects_zero_run_count() {
        let error = ProductCellOutcomeContract::from_native_runs(
            &report(true),
            ValueSignal {
                result: ValueSignalResult::Inconclusive,
                source_evidence_refs: vec!["orvena://runs/native-1".into()],
            },
            RollbackEvidence {
                rehearsed: false,
                evidence_ref: "orvena://rollback/not-rehearsed".into(),
            },
            vec!["orvena://runs/native-1".into()],
            0,
        )
        .unwrap_err();
        assert!(error.to_string().contains("run_count must be positive"));
    }

    #[test]
    fn pass_signal_requires_completed_run() {
        let error = ProductCellOutcomeContract::from_native_run(
            &report(false),
            ValueSignal {
                result: ValueSignalResult::Pass,
                source_evidence_refs: vec!["orvena://runs/native-1".into()],
            },
            RollbackEvidence {
                rehearsed: false,
                evidence_ref: "orvena://rollback/not-rehearsed".into(),
            },
            vec!["orvena://runs/native-1".into()],
        )
        .unwrap_err();
        assert!(error.to_string().contains("completed native run"));
    }

    #[test]
    fn rollback_rehearsal_restores_declared_file() {
        let root = std::env::temp_dir()
            .join(format!("orvena-product-cell-rollback-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = PathBuf::from("state.txt");
        std::fs::write(root.join(&path), "before").unwrap();
        let journal = RollbackJournal::capture(&root, std::slice::from_ref(&path)).unwrap();
        let evidence = journal
            .rehearse("package://rollback/native-test".into(), || {
                std::fs::write(root.join(&path), "after").map_err(Error::from)
            })
            .unwrap();
        assert!(evidence.rehearsed);
        assert_eq!(std::fs::read_to_string(root.join(path)).unwrap(), "before");
        let _ = std::fs::remove_dir_all(root);
    }
}
