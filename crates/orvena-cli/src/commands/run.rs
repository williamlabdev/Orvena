//! `orvena run "<task>"` — load config, resolve any matching skill, run one
//! bounded loop, print the run report (the L1 metric fields), and export an
//! evidence bundle to disk.

use super::{config_dir, load_dotenv, preflight_provider, project_root, run_timestamp};
use anyhow::{bail, Result};
use orvena_core::config::Config;
use orvena_core::metrics::evidence;
use orvena_core::skills::{self, SkillRegistry};
use orvena_core::{Agent, Task};

pub async fn run(task_text: String, write: Vec<String>, provider: Option<String>) -> Result<()> {
    load_dotenv();

    let dir = config_dir();
    if !super::is_initialized(&dir) {
        bail!("no config found — run `orvena init` first");
    }
    let mut config = Config::load_dir(&dir)?;

    // A per-run provider override (e.g. `--provider offline` to see the loop with
    // no key or network). It affects only this run — the config on disk is left
    // untouched.
    if let Some(kind) = provider {
        config.agent.provider.kind = kind;
    }

    // Preflight: fail fast with actionable guidance rather than dead-ending on a
    // deep provider/network error — the first run must never get stuck on setup.
    preflight_provider(&config.agent.provider)?;

    // Resolve a skill from the task text (engine ships in v0.1; content grows
    // one reviewed skill at a time).
    let registry = SkillRegistry::discover(dir.join("skills"))?;
    let active_role = config.agent.default_role.clone();
    let instruction = match skills::resolve(&registry, &task_text, &active_role) {
        Some(skill) => {
            println!("(applying skill '{}')", skill.name);
            skills::apply(skill, &task_text)
        }
        None => task_text.clone(),
    };

    let agent = Agent::new(config, project_root())?;

    // Evidence by default must survive an operator interrupt too: race the run
    // against Ctrl-C and, if interrupted, still write an auditable bundle before
    // exiting rather than leaving nothing on disk.
    let report = tokio::select! {
        r = agent.run(Task::new(instruction.clone(), write)) => r?,
        _ = tokio::signal::ctrl_c() => {
            let bundle = evidence::bundle_path(&dir, &run_timestamp());
            let mut interrupted = orvena_core::RunReport::new(&instruction);
            interrupted.blockers.push("run interrupted (Ctrl-C) before completion".into());
            let interrupted = interrupted.finished(false);
            evidence::write_bundle(&interrupted, &bundle)?;
            eprintln!("\ninterrupted — evidence bundle: {}", bundle.display());
            bail!("run interrupted (Ctrl-C); evidence: {}", bundle.display());
        }
    };

    print_report(&report);

    // Evidence by default: every run leaves an auditable bundle on disk. This
    // must happen BEFORE the completion check below — a run stopped by a gate is
    // exactly when the evidence matters most, so failed runs get a bundle too.
    let bundle = evidence::bundle_path(&dir, &run_timestamp());
    evidence::write_bundle(&report, &bundle)?;
    println!("\nevidence bundle: {}", bundle.display());

    if !report.completed {
        bail!("run did not complete (see blockers above); evidence: {}", bundle.display());
    }
    Ok(())
}

fn print_report(report: &orvena_core::RunReport) {
    println!("\n── run report ──");
    println!("completed:     {}", report.completed);
    println!("steps:         {}", report.steps);
    println!("tool calls:    {}", report.tool_calls);
    println!(
        "tokens:        {} in / {} out ({} total)",
        report.input_tokens,
        report.output_tokens,
        report.total_tokens()
    );
    if !report.gate_outcomes.is_empty() {
        println!("gates:");
        for g in &report.gate_outcomes {
            let mark = if g.passed {
                "pass"
            } else if g.needs_human {
                "human"
            } else {
                "fail"
            };
            println!("  - {:<20} {}", g.gate, mark);
        }
    }
    if !report.blockers.is_empty() {
        println!("blockers:");
        for b in &report.blockers {
            println!("  - {b}");
        }
    }
}
