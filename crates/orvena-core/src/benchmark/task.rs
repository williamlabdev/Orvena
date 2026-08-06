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
    /// The frozen selection: task ids that constitute the set's official
    /// reading. When non-empty, `bench` runs only these (in `tasks` order) —
    /// the remaining entries are on-file alternates that never enter a
    /// default run. Empty (or absent in the YAML) = run every task.
    /// Swapping an id in or out of this list is selection, not a task edit.
    #[serde(default)]
    pub frozen: Vec<String>,
}

impl BenchTaskSet {
    /// Apply the frozen selection: keep only tasks named in `frozen`,
    /// preserving `tasks` order. Errors on ids that name no task — a typo
    /// silently shrinking the official set must fail loudly, not read as a
    /// smaller ruler.
    pub fn apply_frozen_selection(&mut self) -> Result<(), String> {
        if self.frozen.is_empty() {
            return Ok(());
        }
        let missing: Vec<&String> = self
            .frozen
            .iter()
            .filter(|id| !self.tasks.iter().any(|t| &t.id == *id))
            .collect();
        if !missing.is_empty() {
            return Err(format!(
                "frozen selection names unknown task id(s): {}",
                missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
            ));
        }
        self.tasks.retain(|t| self.frozen.contains(&t.id));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(id: &str) -> BenchTask {
        BenchTask {
            id: id.into(),
            instruction: "x".into(),
            writes: vec![],
            verify: "true".into(),
            seed: vec![],
            timeout_secs: None,
            requires: vec![],
            escape_probes: vec![],
            commands: vec![],
        }
    }

    #[test]
    fn empty_frozen_is_a_no_op() {
        let mut set = BenchTaskSet { tasks: vec![task("a"), task("b")], frozen: vec![] };
        set.apply_frozen_selection().unwrap();
        assert_eq!(set.tasks.len(), 2);
    }

    #[test]
    fn frozen_filters_and_keeps_file_order() {
        // frozen listed out of file order — file order must win.
        let mut set = BenchTaskSet {
            tasks: vec![task("a"), task("b"), task("c")],
            frozen: vec!["c".into(), "a".into()],
        };
        set.apply_frozen_selection().unwrap();
        let ids: Vec<&str> = set.tasks.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["a", "c"]);
    }

    #[test]
    fn unknown_frozen_id_fails_loudly() {
        // A typo must not silently shrink the official set.
        let mut set =
            BenchTaskSet { tasks: vec![task("a")], frozen: vec!["a".into(), "typo".into()] };
        let err = set.apply_frozen_selection().unwrap_err();
        assert!(err.contains("typo"), "error should name the missing id: {err}");
    }
}
