//! `orvena bench` — run the benchmark task set through the bounded loop and
//! report a completion rate. Thin wrapper: it borrows the provider from config
//! (with a `--provider` override + readiness preflight, shared with `run`) and
//! defers all orchestration to `orvena_core::benchmark`.

use std::path::PathBuf;

use super::{config_dir, is_initialized, load_dotenv, preflight_provider, run_timestamp};
use anyhow::{bail, Context, Result};
use orvena_core::benchmark::{self, BenchReport, BenchTaskSet};
use orvena_core::config::Config;

/// The built-in task set, embedded so `orvena bench` works without a fixtures
/// directory on disk (same pattern as the init scaffold).
const DEFAULT_TASKS: &str = include_str!("../benchmarks/tasks.yaml");

pub async fn run(provider: Option<String>, tasks: Option<PathBuf>) -> Result<()> {
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
    println!(
        "running {} task(s) against provider '{}'…\n",
        set.tasks.len(),
        config.agent.provider.kind
    );

    let report = benchmark::run_benchmark(&set, &config.agent.provider, &base, &run_id).await?;
    print_report(&report);

    let report_path = base.join(&run_id).join("report.json");
    benchmark::write_report(&report, &report_path)?;
    println!("\nreport: {}", report_path.display());
    Ok(())
}

fn print_report(r: &BenchReport) {
    println!("── benchmark ──");
    println!("provider:  {} / {}", r.provider, r.model);
    for res in &r.results {
        let mark = if res.completed { "pass" } else { "FAIL" };
        println!(
            "  {:<18} {}   {} steps, {} tok",
            res.id,
            mark,
            res.steps,
            res.input_tokens + res.output_tokens
        );
    }
    println!(
        "\ncompletion rate: {}/{} = {:.0}%",
        r.passed,
        r.task_count,
        r.completion_rate * 100.0
    );
}
