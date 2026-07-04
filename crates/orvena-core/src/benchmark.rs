//! Minimal benchmark harness (MVP+1). Run a set of hand-picked, auto-verifiable
//! coding tasks through the bounded loop and report a **completion rate** — the
//! fraction that reached a passing `verify` (`completed: true`).
//!
//! This adds no execution engine: it orchestrates the existing loop over a task
//! set and aggregates. Each task carries its **own** `verify` command (its
//! success criterion) — a shared always-pass gate would make the number
//! meaningless. Every task runs in an isolated workdir and leaves an evidence
//! bundle, so a published number is auditable per task.
//!
//! One run per task; real-provider numbers vary run-to-run (see
//! `docs/benchmark.md`). The `offline` provider makes the harness itself
//! deterministically testable, but is only a smoke — not a real number.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::agent::{Agent, Task};
use crate::config::agent::{AgentConfig, ProviderSelection, Tier};
use crate::config::commands::Commands;
use crate::config::context_budget::ContextBudgets;
use crate::config::gates::{Gate, Gatekeeper, Gates};
use crate::config::roles::{Role, Roles};
use crate::config::Config;
use crate::metrics::evidence;
use crate::Result;

/// Bounded re-attempts per task. Enough for an observe-failing-check → fix →
/// re-verify loop, still capped.
const MAX_STEPS: u32 = 4;

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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchTaskSet {
    pub tasks: Vec<BenchTask>,
}

/// The outcome of one task in the set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub id: String,
    pub completed: bool,
    /// True when the task was not run because a required toolchain was absent.
    /// A skipped task is excluded from the completion-rate denominator.
    pub skipped: bool,
    pub skip_reason: Option<String>,
    pub steps: u32,
    pub tool_calls: u32,
    pub input_tokens: u32,
    pub output_tokens: u32,
    /// Path to the task's evidence bundle (`None` if skipped, or if the run
    /// errored before one could be written).
    pub evidence_path: Option<PathBuf>,
    pub blockers: Vec<String>,
}

/// The aggregate benchmark result. `completion_rate = passed / ran`, where
/// `ran = task_count - skipped` (skipped tasks are not counted against the rate).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchReport {
    pub provider: String,
    pub model: String,
    pub run_id: String,
    pub task_count: u32,
    pub passed: u32,
    pub skipped: u32,
    pub completion_rate: f32,
    pub results: Vec<TaskResult>,
}

/// Run every task in `set` against `provider`, each in its own workdir under
/// `base_dir/<run_id>/<task.id>/`, and aggregate the completion rate.
///
/// A task whose run errors is recorded as a non-completion (with the error as a
/// blocker) rather than aborting the whole benchmark — an honest number counts
/// failures, it does not hide them.
pub async fn run_benchmark(
    set: &BenchTaskSet,
    provider: &ProviderSelection,
    base_dir: &Path,
    run_id: &str,
) -> Result<BenchReport> {
    let mut results = Vec::with_capacity(set.tasks.len());

    for task in &set.tasks {
        // Skip (do not fail) a task whose required toolchain is absent — a
        // missing `cargo`/`pytest` must not read as a completion failure.
        if let Some(missing) = task.requires.iter().find(|c| !command_exists(c)) {
            results.push(TaskResult {
                id: task.id.clone(),
                completed: false,
                skipped: true,
                skip_reason: Some(format!("requires `{missing}` (not found on PATH)")),
                steps: 0,
                tool_calls: 0,
                input_tokens: 0,
                output_tokens: 0,
                evidence_path: None,
                blockers: Vec::new(),
            });
            continue;
        }

        let workdir = base_dir.join(run_id).join(&task.id);
        std::fs::create_dir_all(&workdir)?;
        for f in &task.seed {
            let p = workdir.join(&f.path);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&p, &f.contents)?;
        }

        let config = bench_config(provider, task);
        // Building the provider can fail (e.g. a missing key); that is systemic,
        // so surface it rather than scoring every task as a failure.
        let agent = Agent::new(config, &workdir)?;
        let run = agent.run(Task::new(task.instruction.clone(), task.writes.clone())).await;

        results.push(match run {
            Ok(report) => {
                let evidence_path = workdir.join(evidence::BUNDLE_FILE);
                evidence::write_bundle(&report, &evidence_path)?;
                TaskResult {
                    id: task.id.clone(),
                    completed: report.completed,
                    skipped: false,
                    skip_reason: None,
                    steps: report.steps,
                    tool_calls: report.tool_calls,
                    input_tokens: report.input_tokens,
                    output_tokens: report.output_tokens,
                    evidence_path: Some(evidence_path),
                    blockers: report.blockers,
                }
            }
            Err(e) => TaskResult {
                id: task.id.clone(),
                completed: false,
                skipped: false,
                skip_reason: None,
                steps: 0,
                tool_calls: 0,
                input_tokens: 0,
                output_tokens: 0,
                evidence_path: None,
                blockers: vec![e.to_string()],
            },
        });
    }

    let task_count = results.len() as u32;
    let skipped = results.iter().filter(|r| r.skipped).count() as u32;
    let passed = results.iter().filter(|r| r.completed).count() as u32;
    // Rate is over tasks that actually ran, so skips neither help nor hurt it.
    let ran = task_count - skipped;
    let completion_rate = if ran == 0 { 0.0 } else { passed as f32 / ran as f32 };

    Ok(BenchReport {
        provider: provider.kind.clone(),
        model: provider.model.clone(),
        run_id: run_id.to_string(),
        task_count,
        passed,
        skipped,
        completion_rate,
        results,
    })
}

/// Does `cmd` resolve to an executable? A value containing `/` is treated as a
/// direct path; otherwise `PATH` is scanned. Dep-free (no `which` crate).
fn command_exists(cmd: &str) -> bool {
    if cmd.contains('/') {
        return Path::new(cmd).is_file();
    }
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| dir.join(cmd).is_file())
}

/// Serialize a [`BenchReport`] as pretty JSON to `path`, creating parents.
pub fn write_report(report: &BenchReport, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(report)?)?;
    Ok(())
}

/// The fixed harness config for one task: the caller's provider, a `developer`
/// role that can read/write/search, and a single automated gate whose `verify`
/// is the task's success criterion. Only the *provider* is borrowed from the
/// user; "solved" is defined by the task, not by the user's gates.
fn bench_config(provider: &ProviderSelection, task: &BenchTask) -> Config {
    Config {
        agent: AgentConfig {
            provider: provider.clone(),
            tier: Tier::Light,
            default_role: "developer".into(),
            max_steps: MAX_STEPS,
        },
        roles: Roles {
            roles: vec![Role {
                name: "developer".into(),
                allowed_tools: vec!["fs.read".into(), "fs.write".into(), "grep.search".into()],
                forbidden_tools: vec![],
                knowledge_scope: vec![],
            }],
        },
        gates: Gates {
            gates: vec![Gate {
                name: "solved".into(),
                condition: task.id.clone(),
                verify: Some(task.verify.clone()),
                gatekeeper: Gatekeeper::Automated,
                timeout_secs: task.timeout_secs.or(Some(30)),
            }],
        },
        budgets: ContextBudgets::default(),
        commands: Commands::default(),
    }
}
