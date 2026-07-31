//! What a benchmark measures: a task, its writable paths, and the objective
//! `verify` command that defines "solved".

use serde::{Deserialize, Serialize};

use crate::config::commands::Command;

/// A file seeded into a task's workdir before the run (e.g. the buggy input a
/// "fix until the check passes" task must edit).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeedFile {
    pub path: String,
    pub contents: String,
}

/// One benchmark task: an instruction, the paths it may modify, and the
/// objective `verify` command that defines "solved" (exit 0 = solved).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchTask {
    pub id: String,
    pub instruction: String,
    #[serde(default)]
    pub writes: Vec<String>,
    pub verify: String,
    #[serde(default)]
    pub seed: Vec<SeedFile>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// Commands that must exist on `PATH` for this task to run (e.g. `cargo`,
    /// `pytest`). If any is missing the task is **skipped**, not failed — a
    /// missing toolchain must not read as "0% because it isn't installed".
    #[serde(default)]
    pub requires: Vec<String>,
    /// Workdir-relative paths *outside* the project root (e.g. `../backup.txt`)
    /// that must NOT exist after the run — the oracle's probes for out-of-root
    /// writes, which git cannot see. Used by temptation tasks (M1).
    #[serde(default)]
    pub escape_probes: Vec<String>,
    /// Extra **read-only** commands the agent may run while working on this task
    /// (on top of the `check` the harness always declares, which is the task's
    /// own `verify`). This is the task set's way of giving the agent the same
    /// visibility a shell-capable agent has by default — reading a failing
    /// validator, seeing both sides of a diff, listing the input data.
    ///
    /// It is not a hint channel: a command that *solved* the task, or one that
    /// pointed at the out-of-scope shortcut, would be exactly the "trap
    /// engineering" the plan forbids. These only make visible what any real
    /// agent could `cat` for itself.
    #[serde(default)]
    pub commands: Vec<Command>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchTaskSet {
    pub tasks: Vec<BenchTask>,
}
