//! `orvena bench` — run the benchmark task set through the bounded loop and
//! report a completion rate. Thin wrapper: it borrows the provider from config
//! (with a `--provider` override + readiness preflight, shared with `run`) and
//! defers all orchestration to `orvena_core::benchmark`.

use std::path::PathBuf;

use super::{config_dir, is_initialized, load_dotenv, preflight_provider, run_timestamp};
use anyhow::{bail, Context, Result};
use orvena_core::benchmark::{self, BenchReport, BenchTaskSet, RepeatedReport};
use orvena_core::config::Config;

/// The built-in task set, embedded so `orvena bench` works without a fixtures
/// directory on disk (same pattern as the init scaffold).
const DEFAULT_TASKS: &str = include_str!("../benchmarks/tasks.yaml");

pub async fn run(
    provider: Option<String>,
    tasks: Option<PathBuf>,
    out: Option<PathBuf>,
    repeat: u32,
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

    let run_id = run_timestamp();
    let base = dir.join("bench");
    let kind = config.agent.provider.kind.clone();
    let report_path = base.join(&run_id).join("report.json");

    if repeat <= 1 {
        println!("running {} task(s) against provider '{kind}'…\n", set.tasks.len());
        let report = benchmark::run_benchmark(&set, &config.agent.provider, &base, &run_id).await?;
        print_report(&report);
        benchmark::write_report(&report, &report_path)?;
        announce_report(&report_path, out.as_deref())?;
        if let Some(out) = out {
            benchmark::write_report(&report, &out)?;
        }
    } else {
        println!("running {} task(s) × {repeat} run(s) against provider '{kind}'…\n", set.tasks.len());
        let report =
            benchmark::run_benchmark_repeated(&set, &config.agent.provider, &base, &run_id, repeat)
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
    println!("── benchmark ──");
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
}

fn print_repeated(r: &RepeatedReport) {
    println!("── benchmark ({} runs/task) ──", r.repeat);
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
}
