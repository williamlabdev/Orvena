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
    std::fs::write(path, serde_json::to_string_pretty(report)?)?;
    Ok(())
}
