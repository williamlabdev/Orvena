//! Executing a task set through the bounded loop.
//!
//! Orchestration only: it seeds a workdir, runs the existing agent loop, checks
//! ground truth with the task's own `verify`, and hands raw results to
//! `super::aggregate`. It adds no execution engine of its own.

use std::path::{Path, PathBuf};

use crate::adapter::{self, AgentSelection};
use crate::agent::{Agent, Task};
use crate::config::agent::{AgentConfig, ProviderSelection, Tier};
use crate::config::commands::Commands;
use crate::config::context_budget::ContextBudgets;
use crate::config::gates::{Gate, Gatekeeper, Gates};
use crate::config::roles::{Role, Roles};
use crate::config::Config;
use crate::exec::sandbox::Sandbox;
use crate::governance::gate::GateRunner;
use crate::metrics::{evidence, RunReport, TokenAccounting};
use crate::{Error, Result};

use super::aggregate::{aggregate, derive_differential, rate, weakest_accounting};
use super::mode::GovernanceMode;
use super::report::{
    default_agent, BenchReport, MatrixReport, RepeatedReport, TaskPassRate, TaskResult,
};
use super::task::{BenchTask, BenchTaskSet};

/// Bounded re-attempts per task. Enough for an observe-failing-check → fix →
/// re-verify loop, still capped.
const MAX_STEPS: u32 = 4;

/// Run every task in `set` against `provider`, each in its own workdir under
/// `base_dir/<run_id>/<task.id>/`, and aggregate the completion rate.
///
/// `agent_selection` selects who does the work: Orvena's own bounded loop, or a
/// wrapped external CLI agent ([`crate::adapter`]). Everything *around* the
/// loop — seeding, the git baseline, the independent oracle, the external
/// verify, the evidence bundle — is identical either way. That is the claim an
/// adapter run tests: the envelope is separable from the agent.
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
    agent_selection: &AgentSelection,
) -> Result<BenchReport> {
    let mut results = Vec::with_capacity(set.tasks.len());
    // Probed once per report rather than per task: it spawns a process, and the
    // answer cannot change mid-run.
    let agent_label = match agent_selection.spec() {
        None => "native".to_string(),
        Some(spec) => {
            // A missing agent is systemic, exactly like a missing provider key:
            // every task would fail the same way. Surface it once, up front,
            // rather than emitting a benchmark full of identical zeros.
            if !adapter::available(spec) {
                return Err(Error::Config(format!(
                    "agent '{}': program `{}` not found on PATH",
                    spec.name, spec.program
                )));
            }
            adapter::identity(spec)
        }
    };

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
                violations: Vec::new(),
                false_blocks: Vec::new(),
                contained: false,
                oracle_error: None,
                evidence_valid: false,
                provider_error: None,
                token_accounting: TokenAccounting::default(),
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

        // Baseline for the independent judge (M1): what did the seed look like
        // before the agent ran? A snapshot failure is recorded, never ignored.
        let snapshot_err = super::oracle::snapshot(&workdir).err().map(|e| e.to_string());

        let run = match agent_selection.spec() {
            None => {
                let config = bench_config(provider, task, mode);
                // Building the provider can fail (e.g. a missing key); that is
                // systemic, so surface it rather than scoring every task as a
                // failure.
                let agent = Agent::new(config, &workdir)?;
                let bench_task = Task::new(task.instruction.clone(), task.writes.clone());
                match mode {
                    GovernanceMode::Off => agent.run_ungoverned_baseline(bench_task).await,
                    _ => agent.run(bench_task).await,
                }
            }
            Some(spec) => run_external_task(spec, task, &workdir, mode),
        };

        // The independent judge (M1) runs FIRST: what changed vs what was
        // declared, before the external verify below can add its own side
        // effects to the workdir (clean attribution).
        let scope_refusals = run.as_ref().map(|r| r.scope_refusals.clone()).unwrap_or_default();
        let (violations, false_blocks, contained, oracle_error) = match &snapshot_err {
            Some(e) => (Vec::new(), Vec::new(), false, Some(format!("snapshot: {e}"))),
            None => {
                match super::oracle::judge(
                    &workdir,
                    &task.writes,
                    &task.escape_probes,
                    &scope_refusals,
                ) {
                    Ok(v) => (v.violations, v.false_blocks, v.contained, None),
                    Err(e) => (Vec::new(), Vec::new(), false, Some(e.to_string())),
                }
            }
        };

        // Ground truth, in every mode: the harness runs the task's `verify`
        // itself after the loop — independent of any in-loop gate. This is what
        // makes a "claimed done" comparable to an actual done (M2), and it
        // cross-checks the gate in governed modes (a gate-passed run whose
        // external verify fails would be a harness/gate bug).
        // The oracle's verify is trusted measurement infrastructure (it checks
        // the agent's ground truth), not an agent-triggered command — so it runs
        // unsandboxed regardless of the project's sandbox posture.
        let verified = GateRunner::run(
            &verify_gate(task),
            &workdir,
            &crate::exec::sandbox::Sandbox::disabled(),
        )
        .passed;

        results.push(match run {
            Ok(report) => {
                let evidence_path = workdir.join(evidence::BUNDLE_FILE);
                evidence::write_bundle(&report, &evidence_path)?;
                // M3: the bundle just written must validate against the frozen
                // v1 schema — measured on every run, not assumed.
                let evidence_valid = evidence::validate_bundle(&evidence_path)
                    .map(|p| p.is_empty())
                    .unwrap_or(false);
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
                    violations,
                    false_blocks,
                    contained,
                    oracle_error,
                    evidence_valid,
                    provider_error: report.provider_error,
                    token_accounting: report.token_accounting,
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
                violations,
                false_blocks,
                contained,
                oracle_error,
                // A ran task that left no bundle is an M3 failure by definition.
                evidence_valid: false,
                // Deliberately not a provider error: after the P1 fix a provider
                // failure returns `Ok` with the flag set, so reaching `Err` here
                // means the harness itself broke (config, IO). That is a real
                // failure and stays in the denominator — excluding it would hide
                // a harness bug behind the same door as an API outage.
                provider_error: None,
                token_accounting: TokenAccounting::default(),
            },
        });
    }

    Ok(aggregate(
        provider.kind.clone(),
        provider.model.clone(),
        provider.endpoint_origin(),
        run_id.to_string(),
        mode.to_string(),
        agent_label,
        results,
    ))
}

/// Drive one task with a wrapped external agent under the posture's confinement.
///
/// The posture maps onto *enforcement*, exactly as it does natively:
///
/// - `off` — the whole workdir is writable and no gate is consulted; "done" is
///   the agent's own exit status. The root boundary still holds, because that is
///   host protection, not governance ([`adapter::baseline_sandbox_policy`]).
/// - `light` / `engineering` — writable is narrowed to the task's declared paths
///   and the gate decides done. Under engineering an unavailable sandbox backend
///   refuses to run rather than pretend to confine.
fn run_external_task(
    spec: &adapter::AdapterSpec,
    task: &BenchTask,
    workdir: &Path,
    mode: GovernanceMode,
) -> Result<RunReport> {
    let extra_writable = temp_extra_writable(workdir);

    let (policy, widenings) = match mode {
        GovernanceMode::Off => (adapter::baseline_sandbox_policy(workdir, extra_writable), vec![]),
        GovernanceMode::Light => {
            adapter::sandbox_policy(workdir, &task.writes, false, extra_writable)
        }
        GovernanceMode::Engineering => {
            adapter::sandbox_policy(workdir, &task.writes, true, extra_writable)
        }
    };
    let sandbox = Sandbox::for_policy(policy);
    let gates = match mode {
        GovernanceMode::Off => vec![],
        _ => vec![verify_gate(task)],
    };

    let mut report = adapter::run(
        adapter::AdapterRun {
            spec,
            workdir,
            instruction: &task.instruction,
            writes: &task.writes,
            gates: &gates,
            max_steps: MAX_STEPS,
            timeout: adapter::agent_timeout(),
        },
        &sandbox,
    )?;
    // A widened writable set is a weakened guarantee for that task; it travels
    // with the evidence rather than living only in this function's head.
    report.blockers.extend(widenings);
    Ok(report)
}

/// System temp as an extra writable subtree for a wrapped agent — **unless the
/// workdir itself lives under it**.
///
/// Granting temp is normally harmless (toolchains scribble there and it is not
/// durable exfil, the same reasoning as the product's sandbox). But a benchmark
/// is routinely run out of a `mktemp -d` scratch project — `bench-differential.sh`
/// does exactly that — and there the grant would cover the workdir, its read-only
/// files, and every escape probe: strict confinement would resolve to "everything
/// is writable" while still reporting `enforced`. A containment number measured
/// that way would be worthless *and* indistinguishable from a real one, so the
/// grant is dropped instead. The agent's own scratch dir (`TMPDIR` points there)
/// covers what temp was for.
fn temp_extra_writable(workdir: &Path) -> Vec<PathBuf> {
    let t = std::env::temp_dir();
    let temp = t.canonicalize().unwrap_or(t);
    let work = workdir.canonicalize().unwrap_or_else(|_| workdir.to_path_buf());
    if work.starts_with(&temp) {
        Vec::new()
    } else {
        vec![temp]
    }
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
    agent_selection: &AgentSelection,
) -> Result<RepeatedReport> {
    let repeat = repeat.max(1);
    let mut runs = Vec::with_capacity(repeat as usize);
    for i in 0..repeat {
        runs.push(
            run_benchmark(
                set,
                provider,
                base_dir,
                &format!("{run_id}-rep{i}"),
                mode,
                agent_selection,
            )
            .await?,
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
        // Count only the repeats where the model actually answered. A repeat
        // that died on a provider error is not a failed attempt at the task —
        // it is a missing attempt, and dividing by it would silently deflate
        // the pass rate by however much the API was down.
        let measured_runs = runs
            .iter()
            .filter(|rep| rep.results.iter().any(|r| r.id == t.id && r.provider_error.is_none()))
            .count() as u32;
        let solved = runs
            .iter()
            .filter(|rep| {
                rep.results
                    .iter()
                    .any(|r| r.id == t.id && r.completed && r.provider_error.is_none())
            })
            .count() as u32;
        tasks.push(TaskPassRate {
            id: t.id.clone(),
            runs: measured_runs,
            solved,
            skipped: false,
            pass_rate: rate(solved, measured_runs),
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
    let provider_errors = runs
        .iter()
        .flat_map(|r| r.results.iter())
        .filter(|t| !t.skipped && t.provider_error.is_some())
        .count() as u32;

    // Ground-truth and cost aggregates over the task-runs that actually reached
    // the model (M2/M4) — provider failures are excluded here for the same
    // reason they are in `aggregate`: they measure the API, not the agent.
    let ran_results: Vec<&TaskResult> = runs
        .iter()
        .flat_map(|r| r.results.iter())
        .filter(|t| !t.skipped && t.provider_error.is_none())
        .collect();
    let n_ran = ran_results.len() as f32;
    let claims = ran_results.iter().filter(|t| t.completed).count() as f32;
    let false_dones = ran_results.iter().filter(|t| t.completed && !t.verified).count() as f32;
    let verified_rate = if n_ran == 0.0 {
        0.0
    } else {
        ran_results.iter().filter(|t| t.verified).count() as f32 / n_ran
    };
    let false_done_rate = if claims == 0.0 { 0.0 } else { false_dones / claims };
    let oracle_errors = ran_results.iter().filter(|t| t.oracle_error.is_some()).count() as u32;
    let judged = ran_results.len() as f32 - oracle_errors as f32;
    let containment_rate = if judged == 0.0 {
        0.0
    } else {
        ran_results.iter().filter(|t| t.contained).count() as f32 / judged
    };
    let false_blocks = ran_results.iter().map(|t| t.false_blocks.len() as u32).sum::<u32>();
    let evidence_valid_rate = if n_ran == 0.0 {
        0.0
    } else {
        ran_results.iter().filter(|t| t.evidence_valid).count() as f32 / n_ran
    };
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
    // The weakest accounting wins: one unaccounted run makes the mean a partial
    // number, and calling it "observed" would be the flattering read.
    let token_accounting = ran_results
        .iter()
        .map(|t| t.token_accounting)
        .fold(TokenAccounting::Observed, weakest_accounting);

    Ok(RepeatedReport {
        provider: provider.kind.clone(),
        model: provider.model.clone(),
        endpoint: provider.endpoint_origin(),
        run_id: run_id.to_string(),
        governance: mode.to_string(),
        agent: runs.first().map(|r| r.agent.clone()).unwrap_or_else(default_agent),
        repeat,
        task_count,
        ran,
        skipped,
        provider_errors,
        mean_pass_rate,
        solved_any,
        verified_rate,
        false_done_rate,
        containment_rate,
        false_blocks,
        oracle_errors,
        evidence_valid_rate,
        mean_steps,
        mean_total_tokens,
        token_accounting,
        tasks,
        runs,
    })
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
    agent_selection: &AgentSelection,
) -> Result<MatrixReport> {
    let mut reports = Vec::with_capacity(modes.len());
    for mode in modes {
        let mode_run_id = format!("{run_id}-{mode}");
        reports.push(
            run_benchmark_repeated(
                set,
                provider,
                base_dir,
                &mode_run_id,
                repeat,
                *mode,
                agent_selection,
            )
            .await?,
        );
    }

    let (differential, differential_suppressed) = derive_differential(&reports);

    Ok(MatrixReport {
        provider: provider.kind.clone(),
        model: provider.model.clone(),
        endpoint: provider.endpoint_origin(),
        run_id: run_id.to_string(),
        agent: reports.first().map(|r| r.agent.clone()).unwrap_or_else(default_agent),
        modes: reports,
        differential,
        differential_suppressed,
    })
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
            sandbox: Default::default(),
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
