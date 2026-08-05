//! Evidence-bundle exporter — write a finished [`RunReport`] to a single JSON
//! file on disk. This is the minimal, provable form of Orvena's "evidence by
//! default" promise: every run — completed or stopped by a gate — leaves an
//! auditable artifact carrying the frozen report fields (completion status,
//! gate outcomes, blockers, steps / tool-calls / tokens).
//!
//! This is deliberately *not* a new subsystem: [`RunReport`] already derives
//! `serde::Serialize`, so exporting it is just "write the report we already
//! hold to a file". A persistent event log is a separate, explicitly deferred
//! concern and is not part of this bundle.
//!
//! The runtime owns the *serialization*; the caller owns the *path* (where the
//! bundle lands and how the timestamp is formed) — see ADR-002.

use std::path::{Path, PathBuf};

use super::RunReport;
use crate::Result;

/// File name written inside a run's evidence directory.
pub const BUNDLE_FILE: &str = "evidence.json";

/// Build the on-disk path for a run's evidence bundle under `base_dir`:
/// `<base_dir>/runs/<timestamp>/evidence.json`.
///
/// A per-run subdirectory (rather than a flat `evidence-<timestamp>.json`)
/// leaves room for future per-run artifacts without changing the bundle format
/// — see ADR-002. `timestamp` is supplied by the caller so this stays a pure,
/// clock-free function (the CLI reads the clock; the core does not).
pub fn bundle_path(base_dir: &Path, timestamp: &str) -> PathBuf {
    base_dir.join("runs").join(timestamp).join(BUNDLE_FILE)
}

/// Validate a bundle file against the **v1 schema contract**
/// (`schemas/evidence.v1.json`): required fields present with the right types,
/// gate outcomes well-formed, and the `schema` identifier correct.
///
/// Deliberately hand-rolled over `serde_json::Value` rather than
/// deserializing into [`RunReport`] — the derive's `#[serde(default)]`s would
/// silently *repair* a bundle that is missing required fields, which is
/// exactly what a validator must catch. Kept in lockstep with the schema file
/// by a test that cross-checks both against the same samples with a real
/// JSON-Schema engine.
///
/// Returns the list of problems (empty = valid).
pub fn validate_bundle(path: &Path) -> Result<Vec<String>> {
    let text = std::fs::read_to_string(path)?;
    let value: serde_json::Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => return Ok(vec![format!("not valid JSON: {e}")]),
    };
    Ok(validate_bundle_value(&value))
}

/// The v1 contract check on an already-parsed value. See [`validate_bundle`].
pub fn validate_bundle_value(value: &serde_json::Value) -> Vec<String> {
    use serde_json::Value;
    let mut problems = Vec::new();
    let Some(obj) = value.as_object() else {
        return vec!["bundle is not a JSON object".into()];
    };

    let mut require = |field: &str, check: fn(&Value) -> bool, want: &str| match obj.get(field) {
        None => problems.push(format!("missing required field `{field}`")),
        Some(v) if !check(v) => problems.push(format!("field `{field}` is not {want}")),
        Some(_) => {}
    };

    fn is_uint(v: &Value) -> bool {
        v.as_u64().is_some()
    }
    fn is_string_array(v: &Value) -> bool {
        v.as_array().is_some_and(|a| a.iter().all(Value::is_string))
    }

    require("schema", Value::is_string, "a string");
    require("task", Value::is_string, "a string");
    require("completed", Value::is_boolean, "a boolean");
    require("steps", is_uint, "a non-negative integer");
    require("input_tokens", is_uint, "a non-negative integer");
    require("output_tokens", is_uint, "a non-negative integer");
    require("tool_calls", is_uint, "a non-negative integer");
    require("blockers", is_string_array, "an array of strings");
    require("scope_refusals", is_string_array, "an array of strings");
    require("gate_outcomes", Value::is_array, "an array");

    // Optional (a bundle may be from a wrapped agent or predate the field), but
    // if present it must be usable without guessing: every counter an unsigned
    // integer, or a consumer computing a search-use rate silently gets nonsense.
    if let Some(v) = obj.get("action_counts") {
        if !v.is_null() {
            match v.as_object() {
                None => problems.push("field `action_counts` is not an object".into()),
                Some(counts) => {
                    for (k, val) in counts {
                        if !is_uint(val) {
                            problems
                                .push(format!("action_counts.{k} is not a non-negative integer"));
                        }
                    }
                }
            }
        }
    }

    // Same contract as `action_counts`, one level down: present or absent, but
    // never ambiguous. `null` inside the array is meaningful (a search that
    // errored) — anything else in there would make a yield rate nonsense.
    if let Some(v) = obj.get("search_hits") {
        if !v.is_null() {
            match v.as_array() {
                None => problems.push("field `search_hits` is not an array".into()),
                Some(items) => {
                    for (i, item) in items.iter().enumerate() {
                        if !item.is_null() && !is_uint(item) {
                            problems.push(format!(
                                "search_hits[{i}] is neither null nor a non-negative integer"
                            ));
                        }
                    }
                }
            }
        }
    }

    if let Some(s) = obj.get("schema").and_then(Value::as_str) {
        if s != super::EVIDENCE_SCHEMA_V1 {
            problems.push(format!(
                "unknown schema identifier `{s}` (expected `{}`)",
                super::EVIDENCE_SCHEMA_V1
            ));
        }
    }
    if let Some(gates) = obj.get("gate_outcomes").and_then(Value::as_array) {
        for (i, g) in gates.iter().enumerate() {
            let Some(g) = g.as_object() else {
                problems.push(format!("gate_outcomes[{i}] is not an object"));
                continue;
            };
            for (field, ok) in [
                ("step", g.get("step").is_some_and(is_uint)),
                ("gate", g.get("gate").is_some_and(|v| v.is_string())),
                ("passed", g.get("passed").is_some_and(Value::is_boolean)),
                ("needs_human", g.get("needs_human").is_some_and(Value::is_boolean)),
            ] {
                if !ok {
                    problems.push(format!("gate_outcomes[{i}].{field} missing or mistyped"));
                }
            }
        }
    }

    problems
}

/// Serialize `report` as pretty JSON and write it to `path`, creating any
/// missing parent directories. Round-trips: the written file deserializes back
/// into an equal [`RunReport`].
pub fn write_bundle(report: &RunReport, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(report)?;
    // Atomic write: serialize to a sibling temp file, then rename into place. A
    // crash or `kill -9` mid-write can only leave a stray `.tmp`; the bundle at
    // `path` is either the previous complete file or the new complete one, never
    // a truncated / invalid-JSON artifact.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::metrics::RunReport;

    #[test]
    fn write_is_atomic_and_leaves_no_temp() {
        let dir = std::env::temp_dir().join(format!("orvena-ev-atomic-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = bundle_path(&dir, "run-1");

        let report = RunReport::new("demo").finished(false);
        write_bundle(&report, &path).unwrap();

        // Final file exists, is valid JSON, and no temp file is left behind.
        assert!(path.exists(), "bundle must exist at the final path");
        let reloaded: RunReport =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(reloaded.task, "demo");
        assert!(!path.with_extension("json.tmp").exists(), "temp file must be renamed away");

        // Overwriting an existing bundle also lands atomically.
        let report2 = RunReport::new("demo-2").finished(true);
        write_bundle(&report2, &path).unwrap();
        let reloaded2: RunReport =
            serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(reloaded2.task, "demo-2");
        assert!(reloaded2.completed);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
