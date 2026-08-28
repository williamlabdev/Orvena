//! `orvena run "<task>"` — load config, resolve any matching skill, run one
//! bounded loop, print the run report (the L1 metric fields), and export an
//! evidence bundle to disk.

use super::{config_dir, load_dotenv, preflight_provider, project_root, run_timestamp};
use anyhow::{bail, Result};
use orvena_core::adapter::{self, AdapterRun};
use orvena_core::config::Config;
use orvena_core::exec::sandbox::Sandbox;
use orvena_core::metrics::{evidence, RollbackEvidence, ValueSignal, ValueSignalResult};
use orvena_core::skills::{self, SkillRegistry};
use orvena_core::{Agent, Task};

/// ProductCell outcome flags for one `orvena run` invocation.
pub struct OutcomeArgs {
    pub value: Option<String>,
    pub evidence_refs: Vec<String>,
    pub run_refs: Vec<String>,
    pub run_count: u32,
    pub rollback_evidence_ref: Option<String>,
    pub rollback_rehearsed: bool,
}

pub async fn run(
    task_text: String,
    write: Vec<String>,
    provider: Option<String>,
    agent: String,
    outcome: OutcomeArgs,
) -> Result<()> {
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

    let root = project_root();
    let run_id = run_timestamp();
    let bundle = evidence::bundle_path(&dir, &run_id);

    let report = match agent.trim().to_ascii_lowercase().as_str() {
        "" | "native" | "orvena" => {
            // Preflight: fail fast with actionable guidance rather than
            // dead-ending on a deep provider/network error.
            preflight_provider(&config.agent.provider)?;
            let native = Agent::new(config, root)?;
            run_native(native, instruction.clone(), write, &bundle).await?
        }
        "codex" => {
            let spec = adapter::codex::spec(&config.agent.provider)?;
            if !adapter::available(&spec) {
                bail!(
                    "--agent codex: `{}` is not on PATH — install it (e.g. `npm install -g \
                     @openai/codex`) and re-run",
                    spec.program
                );
            }
            run_wrapped_external(spec, "codex exec", &config, &root, &write, &instruction)?
        }
        "claude" => {
            let spec = adapter::claude::spec(&config.agent.provider)?;
            if !adapter::available(&spec) {
                bail!(
                    "--agent claude: `{}` is not on PATH — install Claude Code (e.g. `npm \
                     install -g @anthropic-ai/claude-code`) and re-run",
                    spec.program
                );
            }
            run_wrapped_external(spec, "claude -p", &config, &root, &write, &instruction)?
        }
        other => bail!("unknown --agent '{other}' (known for `orvena run`: native, codex, claude)"),
    };

    let report = attach_product_cell_outcome(report, &run_id, outcome)?;

    print_report(&report);

    // Evidence by default: every run leaves an auditable bundle on disk. This
    // must happen BEFORE the completion check below — a run stopped by a gate is
    // exactly when the evidence matters most, so failed runs get a bundle too.
    evidence::write_bundle(&report, &bundle)?;
    println!("\nevidence bundle: {}", bundle.display());

    if !report.completed {
        bail!("run did not complete (see blockers above); evidence: {}", bundle.display());
    }
    Ok(())
}

/// Run one wrapped external agent (Codex, Claude Code) under Orvena's
/// envelope: OS sandbox policy, gates as harness measurement, and the same
/// evidence/report contract as the native loop.
fn run_wrapped_external(
    spec: orvena_core::adapter::AdapterSpec,
    invocation: &str,
    config: &Config,
    root: &std::path::Path,
    write: &[String],
    instruction: &str,
) -> Result<orvena_core::RunReport> {
    println!("agent: {} (wrapped by Orvena; invocation: {invocation})", adapter::identity(&spec));

    let (policy, mut widenings) = adapter::sandbox_policy(
        root,
        write,
        config.agent.tier.enforces(),
        spec.state_writable.clone(),
    );
    for path in &spec.state_writable {
        widenings.push(format!(
            "sandbox widened: agent state path '{}' is writable — {} login/session \
             state requires it; containment stays scoped to the project tree",
            path.display(),
            spec.name,
        ));
    }
    let sandbox = Sandbox::for_policy(policy);
    // Gates are harness measurement, not agent actions. Use the same
    // root-bounded baseline policy as external benchmark runs so a
    // build-based gate can create its own artifacts.
    let gate_sandbox =
        Sandbox::for_policy(adapter::baseline_sandbox_policy(root, spec.state_writable.clone()));
    let gates = config.gates.gates.clone();
    let mut report = adapter::run(
        AdapterRun {
            spec: &spec,
            workdir: root,
            instruction,
            writes: write,
            gates: &gates,
            gate_sandbox: &gate_sandbox,
            max_steps: config.agent.max_steps,
            timeout: adapter::agent_timeout(),
        },
        &sandbox,
    )?
    .with_provenance(&config.agent.provider);
    report.blockers.extend(widenings);
    Ok(report)
}

/// Run Orvena's own bounded loop while preserving the interrupt evidence
/// guarantee used by the original `orvena run` path.
async fn run_native(
    agent: Agent,
    instruction: String,
    write: Vec<String>,
    bundle: &std::path::Path,
) -> Result<orvena_core::RunReport> {
    tokio::select! {
        r = agent.run(Task::new(instruction.clone(), write)) => Ok(r?),
        _ = tokio::signal::ctrl_c() => {
            let mut interrupted = orvena_core::RunReport::new(&instruction);
            interrupted.blockers.push("run interrupted (Ctrl-C) before completion".into());
            let interrupted = interrupted.finished(false);
            evidence::write_bundle(&interrupted, bundle)?;
            eprintln!("\ninterrupted — evidence bundle: {}", bundle.display());
            bail!("run interrupted (Ctrl-C); evidence: {}", bundle.display());
        }
    }
}

fn attach_product_cell_outcome(
    report: orvena_core::RunReport,
    run_id: &str,
    outcome: OutcomeArgs,
) -> Result<orvena_core::RunReport> {
    let OutcomeArgs {
        value: outcome_value,
        evidence_refs: outcome_evidence_refs,
        run_refs: outcome_run_refs,
        run_count: outcome_run_count,
        rollback_evidence_ref,
        rollback_rehearsed,
    } = outcome;
    if outcome_value.is_none()
        && (!outcome_evidence_refs.is_empty()
            || !outcome_run_refs.is_empty()
            || outcome_run_count != 1
            || rollback_evidence_ref.is_some()
            || rollback_rehearsed)
    {
        bail!("outcome metadata requires --outcome-value");
    }
    let Some(outcome_value) = outcome_value else {
        return Ok(report);
    };

    let value_signal = match outcome_value.to_ascii_uppercase().as_str() {
        "PASS" => ValueSignalResult::Pass,
        "FAIL" => ValueSignalResult::Fail,
        "INCONCLUSIVE" => ValueSignalResult::Inconclusive,
        "BLOCKED" => ValueSignalResult::Blocked,
        other => bail!(
            "invalid --outcome-value '{other}'; expected PASS, FAIL, INCONCLUSIVE, or BLOCKED"
        ),
    };
    if matches!(value_signal, ValueSignalResult::Pass)
        && (outcome_evidence_refs.is_empty() || !rollback_rehearsed)
    {
        bail!("PASS outcome requires at least one --outcome-evidence-ref and --rollback-rehearsed");
    }

    // The run ref is generated from the same id as the on-disk bundle, making
    // the contract portable while preserving a direct correlation to this
    // actual native execution. No ref is invented for a missing external
    // value or rollback observation: the honest defaults remain INCONCLUSIVE
    // and not rehearsed.
    let run_ref = format!("orvena://runs/{run_id}");
    let value_refs = if outcome_evidence_refs.is_empty() {
        vec![run_ref.clone()]
    } else {
        outcome_evidence_refs
    };
    let rollback = RollbackEvidence {
        rehearsed: rollback_rehearsed,
        evidence_ref: rollback_evidence_ref
            .unwrap_or_else(|| "orvena://rollback/not-rehearsed".into()),
    };
    // Provenance refs are run refs: the earlier runs of an aggregate series
    // (supplied via --outcome-run-ref) plus this run's own ref. The core
    // contract enforces one distinct ref per claimed run.
    let mut provenance_refs = outcome_run_refs;
    if !provenance_refs.iter().any(|r| r.trim() == run_ref) {
        provenance_refs.push(run_ref);
    }
    report
        .with_product_cell_outcome_runs(
            ValueSignal { result: value_signal, source_evidence_refs: value_refs },
            rollback,
            provenance_refs,
            outcome_run_count,
        )
        .map_err(anyhow::Error::from)
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
