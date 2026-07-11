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
use crate::governance::gate::GateRunner;
use crate::metrics::evidence;
use crate::{Error, Result};

/// Which governance posture a benchmark run uses (the governance-differential
/// axis, D1–D6). `Off` is the ungoverned baseline: same prompt, no scope
/// enforcement (root escape still blocked — host protection), no gates —
/// "done" is the model's own unverified claim. It exists only inside the
/// benchmark (D2); the product ships `light` and `engineering` only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GovernanceMode {
    Off,
    Light,
    Engineering,
}

impl GovernanceMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            GovernanceMode::Off => "off",
            GovernanceMode::Light => "light",
            GovernanceMode::Engineering => "engineering",
        }
    }
}

impl std::fmt::Display for GovernanceMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for GovernanceMode {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" => Ok(GovernanceMode::Off),
            "light" => Ok(GovernanceMode::Light),
            "engineering" => Ok(GovernanceMode::Engineering),
            other => Err(Error::Config(format!(
                "unknown governance mode '{other}' (expected off|light|engineering)"
            ))),
        }
    }
}

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
    /// The in-loop outcome. Governed: all gates passed. Ungoverned (`off`): the
    /// model's own claim of done — which is exactly what M2 interrogates.
    pub completed: bool,
    /// Ground truth: the task's `verify` command, run by the harness *after*
    /// the loop finished, independent of any in-loop gate. `completed` without
    /// `verified` is a false done.
    #[serde(default)]
    pub verified: bool,
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
    /// Governance posture this report was measured under.
    #[serde(default = "default_governance")]
    pub governance: String,
    pub task_count: u32,
    pub passed: u32,
    pub skipped: u32,
    pub completion_rate: f32,
    /// Tasks whose external `verify` (ground truth) passed, and the rate over ran.
    #[serde(default)]
    pub verified: u32,
    #[serde(default)]
    pub verified_rate: f32,
    /// Claimed done but ground truth failed (M2). Rate is over *claims*
    /// (`passed`), not over ran — "of the runs that said done, how many lied".
    #[serde(default)]
    pub false_done: u32,
    #[serde(default)]
    pub false_done_rate: f32,
    pub results: Vec<TaskResult>,
}

fn default_governance() -> String {
    "light".into()
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
    mode: GovernanceMode,
) -> Result<BenchReport> {
    let mut results = Vec::with_capacity(set.tasks.len());

    for task in &set.tasks {
        // Skip (do not fail) a task whose required toolchain is absent — a
        // missing `cargo`/`pytest` must not read as a completion failure.
        if let Some(missing) = task.requires.iter().find(|c| !command_exists(c)) {
            results.push(TaskResult {
                id: task.id.clone(),
                completed: false,
                verified: false,
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

        let config = bench_config(provider, task, mode);
        // Building the provider can fail (e.g. a missing key); that is systemic,
        // so surface it rather than scoring every task as a failure.
        let agent = Agent::new(config, &workdir)?;
        let bench_task = Task::new(task.instruction.clone(), task.writes.clone());
        let run = match mode {
            GovernanceMode::Off => agent.run_ungoverned_baseline(bench_task).await,
            _ => agent.run(bench_task).await,
        };

        // Ground truth, in every mode: the harness runs the task's `verify`
        // itself after the loop — independent of any in-loop gate. This is what
        // makes a "claimed done" comparable to an actual done (M2), and it
        // cross-checks the gate in governed modes (a gate-passed run whose
        // external verify fails would be a harness/gate bug).
        let verified = GateRunner::run(&verify_gate(task), &workdir).passed;

        results.push(match run {
            Ok(report) => {
                let evidence_path = workdir.join(evidence::BUNDLE_FILE);
                evidence::write_bundle(&report, &evidence_path)?;
                TaskResult {
                    id: task.id.clone(),
                    completed: report.completed,
                    verified,
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
                verified,
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
    let verified = results.iter().filter(|r| !r.skipped && r.verified).count() as u32;
    let false_done = results.iter().filter(|r| r.completed && !r.verified).count() as u32;
    // Rate is over tasks that actually ran, so skips neither help nor hurt it.
    let ran = task_count - skipped;
    let completion_rate = if ran == 0 { 0.0 } else { passed as f32 / ran as f32 };
    let verified_rate = if ran == 0 { 0.0 } else { verified as f32 / ran as f32 };
    // Over claims: "of the runs that said done, how many lied".
    let false_done_rate = if passed == 0 { 0.0 } else { false_done as f32 / passed as f32 };

    Ok(BenchReport {
        provider: provider.kind.clone(),
        model: provider.model.clone(),
        run_id: run_id.to_string(),
        governance: mode.to_string(),
        task_count,
        passed,
        skipped,
        completion_rate,
        verified,
        verified_rate,
        false_done,
        false_done_rate,
        results,
    })
}

/// The task's success criterion as a gate, shared by the in-loop config and the
/// harness's external ground-truth check so "solved" is one definition.
fn verify_gate(task: &BenchTask) -> Gate {
    Gate {
        name: "solved".into(),
        condition: task.id.clone(),
        verify: Some(task.verify.clone()),
        gatekeeper: Gatekeeper::Automated,
        timeout_secs: task.timeout_secs.or(Some(30)),
    }
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

/// Per-task pass rate across repeated runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskPassRate {
    pub id: String,
    pub runs: u32,
    pub solved: u32,
    pub skipped: bool,
    pub pass_rate: f32,
}

/// Aggregate of `repeat` benchmark runs — a de-noised completion rate that
/// tolerates a stochastic model, unlike a single pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepeatedReport {
    pub provider: String,
    pub model: String,
    pub run_id: String,
    /// Governance posture this report was measured under.
    #[serde(default = "default_governance")]
    pub governance: String,
    pub repeat: u32,
    pub task_count: u32,
    pub ran: u32,
    pub skipped: u32,
    /// Mean of per-task pass rates over ran tasks — the expected single-pass
    /// completion rate, averaged over `repeat` attempts to cut model noise.
    pub mean_pass_rate: f32,
    /// Tasks solved in at least one run (an optimistic pass@k upper bound).
    pub solved_any: u32,
    /// Ground truth across all task-runs: externally-verified rate, and the
    /// false-done rate over claims (M2).
    #[serde(default)]
    pub verified_rate: f32,
    #[serde(default)]
    pub false_done_rate: f32,
    /// Cost per ran task-run (M4): mean steps and mean total tokens.
    #[serde(default)]
    pub mean_steps: f32,
    #[serde(default)]
    pub mean_total_tokens: f32,
    pub tasks: Vec<TaskPassRate>,
    /// The underlying per-repeat reports, for full auditability.
    pub runs: Vec<BenchReport>,
}

/// Run the whole set `repeat` times and aggregate per-task pass rates. Each
/// repeat gets its own workdir namespace (`<run_id>-rep<i>`) so runs never
/// collide. `repeat` is clamped to at least 1.
pub async fn run_benchmark_repeated(
    set: &BenchTaskSet,
    provider: &ProviderSelection,
    base_dir: &Path,
    run_id: &str,
    repeat: u32,
    mode: GovernanceMode,
) -> Result<RepeatedReport> {
    let repeat = repeat.max(1);
    let mut runs = Vec::with_capacity(repeat as usize);
    for i in 0..repeat {
        runs.push(
            run_benchmark(set, provider, base_dir, &format!("{run_id}-rep{i}"), mode).await?,
        );
    }

    // Aggregate per task, in the set's declared order. A skip is deterministic
    // (it depends only on PATH), so a task skipped in the first run is skipped
    // in all of them and counts as not-run.
    let mut tasks = Vec::with_capacity(set.tasks.len());
    for t in &set.tasks {
        let skipped = runs[0].results.iter().any(|r| r.id == t.id && r.skipped);
        if skipped {
            tasks.push(TaskPassRate {
                id: t.id.clone(),
                runs: 0,
                solved: 0,
                skipped: true,
                pass_rate: 0.0,
            });
            continue;
        }
        let solved = runs
            .iter()
            .filter(|rep| rep.results.iter().any(|r| r.id == t.id && r.completed))
            .count() as u32;
        tasks.push(TaskPassRate {
            id: t.id.clone(),
            runs: repeat,
            solved,
            skipped: false,
            pass_rate: solved as f32 / repeat as f32,
        });
    }

    let task_count = tasks.len() as u32;
    let skipped = tasks.iter().filter(|t| t.skipped).count() as u32;
    let ran = task_count - skipped;
    let mean_pass_rate = if ran == 0 {
        0.0
    } else {
        tasks.iter().filter(|t| !t.skipped).map(|t| t.pass_rate).sum::<f32>() / ran as f32
    };
    let solved_any = tasks.iter().filter(|t| !t.skipped && t.solved > 0).count() as u32;

    // Ground-truth and cost aggregates over all ran task-runs (M2/M4).
    let ran_results: Vec<&TaskResult> =
        runs.iter().flat_map(|r| r.results.iter()).filter(|t| !t.skipped).collect();
    let n_ran = ran_results.len() as f32;
    let claims = ran_results.iter().filter(|t| t.completed).count() as f32;
    let false_dones = ran_results.iter().filter(|t| t.completed && !t.verified).count() as f32;
    let verified_rate = if n_ran == 0.0 {
        0.0
    } else {
        ran_results.iter().filter(|t| t.verified).count() as f32 / n_ran
    };
    let false_done_rate = if claims == 0.0 { 0.0 } else { false_dones / claims };
    let mean_steps = if n_ran == 0.0 {
        0.0
    } else {
        ran_results.iter().map(|t| t.steps as f32).sum::<f32>() / n_ran
    };
    let mean_total_tokens = if n_ran == 0.0 {
        0.0
    } else {
        ran_results.iter().map(|t| (t.input_tokens + t.output_tokens) as f32).sum::<f32>() / n_ran
    };

    Ok(RepeatedReport {
        provider: provider.kind.clone(),
        model: provider.model.clone(),
        run_id: run_id.to_string(),
        governance: mode.to_string(),
        repeat,
        task_count,
        ran,
        skipped,
        mean_pass_rate,
        solved_any,
        verified_rate,
        false_done_rate,
        mean_steps,
        mean_total_tokens,
        tasks,
        runs,
    })
}

/// The governance-differential matrix (the number only Orvena can publish):
/// the same task set × the same provider, once per governance mode, plus the
/// baseline-vs-governed differential when `off` and a governed mode are both
/// present. Modes are compared on *identical prompts* — the baseline carries
/// the same writable lists, only enforcement differs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatrixReport {
    pub provider: String,
    pub model: String,
    pub run_id: String,
    pub modes: Vec<RepeatedReport>,
    /// Present when both an `off` baseline and a governed mode ran.
    pub differential: Option<Differential>,
}

/// Baseline vs governed, on ground truth and cost. M1 (containment) needs the
/// independent violation oracle (slice-012) and is not claimed here.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Differential {
    pub baseline: String,
    pub governed: String,
    /// M2: of the runs that claimed done, the fraction whose external verify
    /// failed — per posture.
    pub baseline_false_done_rate: f32,
    pub governed_false_done_rate: f32,
    /// Ground-truth solve rate per posture.
    pub baseline_verified_rate: f32,
    pub governed_verified_rate: f32,
    /// M4: governed cost / baseline cost (>1 = governance overhead). 0 when the
    /// baseline cost is 0 (nothing meaningful to divide).
    pub overhead_steps_ratio: f32,
    pub overhead_tokens_ratio: f32,
}

/// Run the matrix: each mode in `modes` over the whole set (`repeat` runs per
/// task), then derive the differential from the `off` baseline and the
/// *strongest* governed mode present (engineering > light).
pub async fn run_benchmark_matrix(
    set: &BenchTaskSet,
    provider: &ProviderSelection,
    base_dir: &Path,
    run_id: &str,
    modes: &[GovernanceMode],
    repeat: u32,
) -> Result<MatrixReport> {
    let mut reports = Vec::with_capacity(modes.len());
    for mode in modes {
        let mode_run_id = format!("{run_id}-{mode}");
        reports
            .push(run_benchmark_repeated(set, provider, base_dir, &mode_run_id, repeat, *mode).await?);
    }

    let baseline = reports.iter().find(|r| r.governance == "off");
    let governed = reports
        .iter()
        .find(|r| r.governance == "engineering")
        .or_else(|| reports.iter().find(|r| r.governance == "light"));
    let differential = match (baseline, governed) {
        (Some(b), Some(g)) => Some(Differential {
            baseline: b.governance.clone(),
            governed: g.governance.clone(),
            baseline_false_done_rate: b.false_done_rate,
            governed_false_done_rate: g.false_done_rate,
            baseline_verified_rate: b.verified_rate,
            governed_verified_rate: g.verified_rate,
            overhead_steps_ratio: ratio(g.mean_steps, b.mean_steps),
            overhead_tokens_ratio: ratio(g.mean_total_tokens, b.mean_total_tokens),
        }),
        _ => None,
    };

    Ok(MatrixReport {
        provider: provider.kind.clone(),
        model: provider.model.clone(),
        run_id: run_id.to_string(),
        modes: reports,
        differential,
    })
}

fn ratio(num: f32, den: f32) -> f32 {
    if den == 0.0 {
        0.0
    } else {
        num / den
    }
}

/// Serialize a [`MatrixReport`] as pretty JSON to `path`, creating parents.
pub fn write_matrix_report(report: &MatrixReport, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(report)?)?;
    Ok(())
}

/// Serialize a [`RepeatedReport`] as pretty JSON to `path`, creating parents.
pub fn write_repeated_report(report: &RepeatedReport, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(report)?)?;
    Ok(())
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
///
/// Governance mode maps to: `off` → no gates (the baseline never sees a gate;
/// the tier value is then irrelevant), `light`/`engineering` → the solved gate
/// under that tier.
fn bench_config(provider: &ProviderSelection, task: &BenchTask, mode: GovernanceMode) -> Config {
    let (tier, gates) = match mode {
        GovernanceMode::Off => (Tier::Light, vec![]),
        GovernanceMode::Light => (Tier::Light, vec![verify_gate(task)]),
        GovernanceMode::Engineering => (Tier::Engineering, vec![verify_gate(task)]),
    };
    Config {
        agent: AgentConfig {
            provider: provider.clone(),
            tier,
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
        gates: Gates { gates },
        budgets: ContextBudgets::default(),
        commands: Commands::default(),
    }
}
