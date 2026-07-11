//! `orvena bench` — run the benchmark task set through the bounded loop and
//! report a completion rate. Thin wrapper: it borrows the provider from config
//! (with a `--provider` override + readiness preflight, shared with `run`) and
//! defers all orchestration to `orvena_core::benchmark`.

use std::path::PathBuf;

use super::{config_dir, is_initialized, load_dotenv, preflight_provider, run_timestamp};
use anyhow::{bail, Context, Result};
use orvena_core::benchmark::{
    self, BenchReport, BenchTaskSet, GovernanceMode, MatrixReport, RepeatedReport,
};
use orvena_core::config::Config;

/// The built-in task set, embedded so `orvena bench` works without a fixtures
/// directory on disk (same pattern as the init scaffold).
const DEFAULT_TASKS: &str = include_str!("../benchmarks/tasks.yaml");

pub async fn run(
    provider: Option<String>,
    tasks: Option<PathBuf>,
    out: Option<PathBuf>,
    repeat: u32,
    governance: Option<String>,
) -> Result<()> {
    load_dotenv();

    let dir = config_dir();
    if !is_initialized(&dir) {
        bail!("no config found — run `orvena init` first");
    }
    let mut config = Config::load_dir(&dir)?;
    if let Some(kind) = provider {
        config.agent.provider.kind = kind;
    }
    preflight_provider(&config.agent.provider.kind)?;

    let set: BenchTaskSet = match &tasks {
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .with_context(|| format!("reading task set {}", path.display()))?;
            serde_yaml::from_str(&text)
                .with_context(|| format!("parsing task set {}", path.display()))?
        }
        None => serde_yaml::from_str(DEFAULT_TASKS).expect("embedded default task set is valid"),
    };
    if set.tasks.is_empty() {
        bail!("the task set is empty — nothing to benchmark");
    }

    // Governance postures: default = light (the previous behavior). More than
    // one posture runs the differential matrix.
    let modes: Vec<GovernanceMode> = match &governance {
        None => vec![GovernanceMode::Light],
        Some(list) => {
            let parsed: std::result::Result<Vec<_>, _> =
                list.split(',').map(|m| m.parse::<GovernanceMode>()).collect();
            let parsed = parsed?;
            // Order-preserving dedup: a repeated mode would collide on the same
            // per-mode workdir namespace.
            let mut modes: Vec<GovernanceMode> = Vec::new();
            for m in parsed {
                if !modes.contains(&m) {
                    modes.push(m);
                }
            }
            if modes.is_empty() {
                bail!("--governance given but no mode parsed");
            }
            modes
        }
    };

    let run_id = run_timestamp();
    let base = dir.join("bench");
    let kind = config.agent.provider.kind.clone();
    let report_path = base.join(&run_id).join("report.json");

    if modes.len() > 1 {
        println!(
            "running {} task(s) × {repeat} run(s) × {} governance mode(s) against provider '{kind}'…\n",
            set.tasks.len(),
            modes.len()
        );
        let report = benchmark::run_benchmark_matrix(
            &set,
            &config.agent.provider,
            &base,
            &run_id,
            &modes,
            repeat,
        )
        .await?;
        print_matrix(&report);
        benchmark::write_matrix_report(&report, &report_path)?;
        announce_report(&report_path, out.as_deref())?;
        if let Some(out) = out {
            benchmark::write_matrix_report(&report, &out)?;
        }
    } else if repeat <= 1 {
        let mode = modes[0];
        println!("running {} task(s) against provider '{kind}' [{mode}]…\n", set.tasks.len());
        let report =
            benchmark::run_benchmark(&set, &config.agent.provider, &base, &run_id, mode).await?;
        print_report(&report);
        benchmark::write_report(&report, &report_path)?;
        announce_report(&report_path, out.as_deref())?;
        if let Some(out) = out {
            benchmark::write_report(&report, &out)?;
        }
    } else {
        let mode = modes[0];
        println!(
            "running {} task(s) × {repeat} run(s) against provider '{kind}' [{mode}]…\n",
            set.tasks.len()
        );
        let report = benchmark::run_benchmark_repeated(
            &set,
            &config.agent.provider,
            &base,
            &run_id,
            repeat,
            mode,
        )
        .await?;
        print_repeated(&report);
        benchmark::write_repeated_report(&report, &report_path)?;
        announce_report(&report_path, out.as_deref())?;
        if let Some(out) = out {
            benchmark::write_repeated_report(&report, &out)?;
        }
    }
    Ok(())
}

/// Print where the report(s) landed. Writing the `--out` copy is left to the
/// caller (it differs by report type); this only reports the paths.
fn announce_report(report_path: &std::path::Path, out: Option<&std::path::Path>) -> Result<()> {
    println!("\nreport: {}", report_path.display());
    if let Some(out) = out {
        println!("report: {}", out.display());
    }
    Ok(())
}

fn print_report(r: &BenchReport) {
    println!("── benchmark [{}] ──", r.governance);
    println!("provider:  {} / {}", r.provider, r.model);
    for res in &r.results {
        if res.skipped {
            let why = res.skip_reason.as_deref().unwrap_or("skipped");
            println!("  {:<18} SKIP   {why}", res.id);
        } else {
            let mark = if res.completed { "pass" } else { "FAIL" };
            println!(
                "  {:<18} {}   {} steps, {} tok",
                res.id,
                mark,
                res.steps,
                res.input_tokens + res.output_tokens
            );
        }
    }
    let ran = r.task_count - r.skipped;
    print!("\ncompletion rate: {}/{} ran = {:.0}%", r.passed, ran, r.completion_rate * 100.0);
    if r.skipped > 0 {
        print!(" ({} skipped)", r.skipped);
    }
    println!();
    print!("verified (ground truth): {}/{} = {:.0}%", r.verified, ran, r.verified_rate * 100.0);
    if r.false_done > 0 {
        print!("  |  FALSE DONE: {}/{} claims = {:.0}%", r.false_done, r.passed, r.false_done_rate * 100.0);
    }
    println!();
    let judged = ran - r.oracle_errors;
    print!(
        "containment (oracle): {}/{} judged = {:.0}%",
        r.contained,
        judged,
        r.containment_rate * 100.0
    );
    if r.false_blocks > 0 {
        print!("  |  false blocks: {}", r.false_blocks);
    }
    if r.oracle_errors > 0 {
        print!("  |  UNJUDGED: {} (oracle errors — not counted as contained)", r.oracle_errors);
    }
    println!();
    println!(
        "evidence (schema v1): {}/{} valid = {:.0}%",
        r.evidence_valid, ran, r.evidence_valid_rate * 100.0
    );
}

fn print_repeated(r: &RepeatedReport) {
    println!("── benchmark ({} runs/task) [{}] ──", r.repeat, r.governance);
    println!("provider:  {} / {}", r.provider, r.model);
    for t in &r.tasks {
        if t.skipped {
            println!("  {:<18} SKIP", t.id);
        } else {
            println!("  {:<18} {}/{} solved  ({:.0}%)", t.id, t.solved, t.runs, t.pass_rate * 100.0);
        }
    }
    print!(
        "\nmean pass rate: {:.0}%  (over {} ran task(s), {} runs each)  |  solved ≥once: {}/{}",
        r.mean_pass_rate * 100.0,
        r.ran,
        r.repeat,
        r.solved_any,
        r.ran
    );
    if r.skipped > 0 {
        print!("  |  {} skipped", r.skipped);
    }
    println!();
    println!(
        "ground truth: {:.0}% verified  |  false-done: {:.0}% of claims  |  mean {:.1} steps, {:.0} tok",
        r.verified_rate * 100.0,
        r.false_done_rate * 100.0,
        r.mean_steps,
        r.mean_total_tokens
    );
    print!("containment (oracle): {:.0}%", r.containment_rate * 100.0);
    if r.false_blocks > 0 {
        print!("  |  false blocks: {}", r.false_blocks);
    }
    if r.oracle_errors > 0 {
        print!("  |  UNJUDGED: {} (oracle errors)", r.oracle_errors);
    }
    println!("  |  evidence valid: {:.0}%", r.evidence_valid_rate * 100.0);
}

fn print_matrix(m: &MatrixReport) {
    for mode in &m.modes {
        print_repeated(mode);
        println!();
    }
    if let Some(d) = &m.differential {
        println!("── governance differential ({} vs {}) ──", d.governed, d.baseline);
        println!(
            "containment: {}: {:.0}%  →  {}: {:.0}%",
            d.baseline,
            d.baseline_containment_rate * 100.0,
            d.governed,
            d.governed_containment_rate * 100.0
        );
        println!(
            "false-done:  {}: {:.0}% of claims  →  {}: {:.0}% of claims",
            d.baseline,
            d.baseline_false_done_rate * 100.0,
            d.governed,
            d.governed_false_done_rate * 100.0
        );
        println!(
            "verified:    {}: {:.0}%  →  {}: {:.0}%",
            d.baseline,
            d.baseline_verified_rate * 100.0,
            d.governed,
            d.governed_verified_rate * 100.0
        );
        println!(
            "overhead:    ×{:.2} steps, ×{:.2} tokens (governed / baseline)",
            d.overhead_steps_ratio, d.overhead_tokens_ratio
        );
    } else {
        println!("(no differential: run with both `off` and a governed mode)");
    }
}
