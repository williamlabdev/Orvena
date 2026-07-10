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
