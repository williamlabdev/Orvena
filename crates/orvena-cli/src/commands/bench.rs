//! `orvena bench` — run the benchmark task set through the bounded loop and
//! report a completion rate. Thin wrapper: it borrows the provider from config
//! (with a `--provider` override + readiness preflight, shared with `run`) and
//! defers all orchestration to `orvena_core::benchmark`.

use std::path::PathBuf;

use super::{config_dir, is_initialized, load_dotenv, preflight_provider, run_timestamp};
use anyhow::{bail, Context, Result};
use orvena_core::adapter::{self, AgentSelection};
use orvena_core::benchmark::{
    self, BenchReport, BenchTaskSet, GovernanceMode, MatrixReport, RepeatedReport,
};
use orvena_core::config::agent::ProviderSelection;
use orvena_core::config::Config;
use orvena_core::metrics::TokenAccounting;

/// The built-in task set, embedded so `orvena bench` works without a fixtures
/// directory on disk (same pattern as the init scaffold).
const DEFAULT_TASKS: &str = include_str!("../benchmarks/tasks.yaml");

pub async fn run(
    provider: Option<String>,
    tasks: Option<PathBuf>,
    out: Option<PathBuf>,
    repeat: u32,
    governance: Option<String>,
    agent: String,
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
    preflight_provider(&config.agent.provider)?;

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

    let agent_selection = resolve_agent(&agent, &config.agent.provider)?;

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
            &agent_selection,
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
        let report = benchmark::run_benchmark(
            &set,
            &config.agent.provider,
            &base,
            &run_id,
            mode,
            &agent_selection,
        )
        .await?;
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
            &agent_selection,
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

/// Resolve `--agent` into a selection, failing fast (before any task runs) when
/// a wrapped agent is asked for but not installed or not mappable onto the
/// configured provider. The same posture as `preflight_provider`: a missing
/// prerequisite is a loud error up front, never a benchmark full of failures.
fn resolve_agent(agent: &str, provider: &ProviderSelection) -> Result<AgentSelection> {
    match agent.trim().to_ascii_lowercase().as_str() {
        "" | "native" | "orvena" => Ok(AgentSelection::Native),
        adapter::aider::NAME => {
            let spec = adapter::aider::spec(provider)?;
            if !adapter::available(&spec) {
                bail!(
                    "--agent aider: `{}` is not on PATH. Install it (e.g. `pipx install aider-chat` \
                     or `python -m pip install aider-chat`) and re-run; Orvena wraps the agent, it \
                     does not bundle one",
                    spec.program
                );
            }
            println!("agent: {} (wrapped by Orvena)", adapter::identity(&spec));
            Ok(AgentSelection::External(Box::new(spec)))
        }
        other => bail!("unknown --agent '{other}' (known: native, aider)"),
    }
}

/// One line describing where a token figure came from, or nothing when Orvena
/// counted it itself (the unremarkable case).
fn token_note(accounting: TokenAccounting) -> &'static str {
    match accounting {
        TokenAccounting::Observed => "",
        TokenAccounting::AgentReported => "  (tokens: self-reported by the agent)",
        TokenAccounting::Unavailable => "  (tokens: NOT COUNTED — 0 means unknown, not free)",
    }
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
    if let Some(endpoint) = &r.endpoint {
        println!("endpoint:  {endpoint}");
    }
    println!("agent:     {}", r.agent);
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
    let ran = r.task_count - r.skipped - r.provider_errors;
    if ran == 0 {
        // Same reasoning as the repeated report: 0% and "we never found out"
        // must not look alike.
        println!(
            "\nno measurable result: 0 of {} task(s) reached the model",
            r.task_count - r.skipped
        );
        print_provider_errors(r.provider_errors, r.task_count - r.skipped);
        return;
    }
    print!("\ncompletion rate: {}/{} ran = {:.0}%", r.passed, ran, r.completion_rate * 100.0);
    if r.skipped > 0 {
        print!(" ({} skipped)", r.skipped);
    }
    println!();
    print_provider_errors(r.provider_errors, r.task_count - r.skipped);
    print!("verified (ground truth): {}/{} = {:.0}%", r.verified, ran, r.verified_rate * 100.0);
    if r.false_done > 0 {
        print!(
            "  |  FALSE DONE: {}/{} claims = {:.0}%",
            r.false_done,
            r.passed,
            r.false_done_rate * 100.0
        );
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
        r.evidence_valid,
        ran,
        r.evidence_valid_rate * 100.0
    );
}

/// Say out loud when runs died on the provider. Silence here is what let an
/// outage read as a result: the rates below exclude these runs, but a reader
/// who cannot see the count has no way to judge how much sample is left.
fn print_provider_errors(provider_errors: u32, attempted: u32) {
    if provider_errors == 0 {
        return;
    }
    let share =
        if attempted == 0 { 0.0 } else { provider_errors as f32 / attempted as f32 * 100.0 };
    println!(
        "!! provider errors: {provider_errors}/{attempted} task-run(s) never reached the model \
         ({share:.0}%) — excluded from every rate above"
    );
}

fn print_repeated(r: &RepeatedReport) {
    println!("── benchmark ({} runs/task) [{}] ──", r.repeat, r.governance);
    println!("provider:  {} / {}", r.provider, r.model);
    if let Some(endpoint) = &r.endpoint {
        println!("endpoint:  {endpoint}");
    }
    println!("agent:     {}", r.agent);
    for t in &r.tasks {
        if t.skipped {
            println!("  {:<18} SKIP", t.id);
        } else if t.runs == 0 {
            println!("  {:<18} —      (no run reached the model)", t.id);
        } else {
            println!(
                "  {:<18} {}/{} solved  ({:.0}%)",
                t.id,
                t.solved,
                t.runs,
                t.pass_rate * 100.0
            );
        }
    }
    // With nothing measured, every rate below would print 0% — which reads as
    // "the agent scored zero" when it means "we never found out". Refuse to
    // print the numbers at all; that ambiguity is the whole defect this guard
    // exists to close.
    let measured = measured_task_runs(r);
    if measured == 0 {
        println!(
            "\nno measurable result: 0 of {} task-run(s) reached the model",
            r.provider_errors
        );
        print_provider_errors(r.provider_errors, r.provider_errors);
        return;
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
        "ground truth: {:.0}% verified  |  false-done: {:.0}% of claims  |  mean {:.1} steps, {:.0} tok{}",
        r.verified_rate * 100.0,
        r.false_done_rate * 100.0,
        r.mean_steps,
        r.mean_total_tokens,
        token_note(r.token_accounting)
    );
    print!("containment (oracle): {:.0}%", r.containment_rate * 100.0);
    if r.false_blocks > 0 {
        print!("  |  false blocks: {}", r.false_blocks);
    }
    if r.oracle_errors > 0 {
        print!("  |  UNJUDGED: {} (oracle errors)", r.oracle_errors);
    }
    println!("  |  evidence valid: {:.0}%", r.evidence_valid_rate * 100.0);
    print_provider_errors(r.provider_errors, r.provider_errors + measured_task_runs(r));
}

/// Task-runs in a repeated report that actually reached the model — the
/// denominator the rates above were computed over.
fn measured_task_runs(r: &RepeatedReport) -> u32 {
    r.runs
        .iter()
        .flat_map(|b| b.results.iter())
        .filter(|t| !t.skipped && t.provider_error.is_none())
        .count() as u32
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
        match d.overhead_tokens_ratio {
            Some(tokens) => println!(
                "overhead:    ×{:.2} steps, ×{:.2} tokens (governed / baseline){}",
                d.overhead_steps_ratio,
                tokens,
                token_note(d.token_accounting)
            ),
            // Never "×0.00 tokens": an unmeasured cost is not a free one.
            None => println!(
                "overhead:    ×{:.2} steps (governed / baseline)  |  tokens: not counted — the \
                 wrapped agent makes its own model calls",
                d.overhead_steps_ratio
            ),
        }
    } else if let Some(reason) = &m.differential_suppressed {
        println!("── governance differential ──");
        println!("{reason}");
    } else {
        println!("(no differential: run with both `off` and a governed mode)");
    }
}
