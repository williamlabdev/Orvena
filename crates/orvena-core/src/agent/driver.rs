//! The bounded coding loop:
//!
//! ```text
//! prepare context → call model → apply (scope-gated) → gate check
//!        ▲                                                   │
//!        └──────── observe evidence, re-attempt ◄───────────┘   (capped by max_steps)
//! ```
//!
//! A passed set of gates means "done" (stop). A human gate stops and reports a
//! blocker. Hitting `max_steps` stops with a blocker rather than looping forever.

use super::{context, step, Agent, Task};
use crate::error::{Error, Result};
use crate::governance::gate::GateRunner;
use crate::governance::scope::Scope;
use crate::metrics::{GateRecord, RunReport};
use crate::provider::ChatRequest;
use crate::tools::fs::FsTool;
use crate::tools::grep::GrepTool;
use crate::tools::shell::ShellTool;

/// Output cap per model call (input is governed by the context budget).
const MAX_OUTPUT_TOKENS: u32 = 1024;

/// Caller-side caps on RUN evidence so a chatty command cannot flood the next
/// step's context (the runner itself returns untruncated output — the truncation
/// is a driver concern, mirroring how SEARCH caps hits at the call site).
const RUN_EVIDENCE_MAX_LINES: usize = 100;
const RUN_EVIDENCE_MAX_BYTES: usize = 8 * 1024;

/// Trim `raw` to the RUN evidence caps. Returns the (possibly truncated) body and
/// a note describing the cap when one was hit (empty otherwise).
fn cap_run_output(raw: &str) -> (String, String) {
    let mut body = String::new();
    let mut truncated = false;
    for (idx, line) in raw.trim_end().lines().enumerate() {
        if idx >= RUN_EVIDENCE_MAX_LINES || body.len() + line.len() + 1 > RUN_EVIDENCE_MAX_BYTES {
            truncated = true;
            break;
        }
        body.push_str(line);
        body.push('\n');
    }
    let note = if truncated {
        format!(
            "(capped at {RUN_EVIDENCE_MAX_LINES} lines / {RUN_EVIDENCE_MAX_BYTES} bytes — \
             narrow the command for the rest)\n"
        )
    } else {
        String::new()
    };
    (body.trim_end().to_string(), note)
}

/// Bench-only loop options (D2: the ungoverned baseline is a bench flag, not a
/// product tier). Crate-private — unreachable from the CLI/config surface.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LoopOptions {
    /// Ungoverned baseline: scope lists are not enforced (root escape still is),
    /// gates are never consulted, and the run ends when the model emits zero
    /// actions — its own, unverified claim of "done" — or at `max_steps`. The
    /// prompt is identical to a governed run; only enforcement differs.
    pub ungoverned: bool,
}

pub async fn run_loop(agent: &Agent, task: Task) -> Result<RunReport> {
    run_loop_with(agent, task, LoopOptions::default()).await
}

pub(crate) async fn run_loop_with(
    agent: &Agent,
    task: Task,
    opts: LoopOptions,
) -> Result<RunReport> {
    let cfg = agent.config();
    let role = cfg
        .roles
        .get(&cfg.agent.default_role)
        .ok_or_else(|| Error::Config(format!("role '{}' not found", cfg.agent.default_role)))?
        .clone();

    let scope = if opts.ungoverned {
        Scope::unrestricted_baseline(task.allowed_modifications.clone(), cfg.agent.tier)
    } else {
        Scope::new(task.allowed_modifications.clone(), Vec::new(), cfg.agent.tier)
    };
    let budget = cfg.budgets.for_role(&role.name);
    let max_steps = cfg.agent.max_steps;

    let mut report = RunReport::new(&task.instruction);
    let mut prior_evidence = String::new();

    for step_no in 1..=max_steps {
        report.steps = step_no;

        // 1. prepare context (re-built each attempt; carries prior gate evidence)
        let ctx = context::build(
            agent.root(),
            &scope,
            &role,
            budget,
            &task.instruction,
            &prior_evidence,
        );

        // 2. call model
        // A provider error (outage, bad key, network) must NOT bail out of the
        // loop before evidence is produced. Capture it as a blocker and finish
        // the run so the caller still writes a bundle — "evidence by default"
        // must hold on the error path too, not only on Ok.
        let resp = match agent
            .provider()
            .chat(ChatRequest { messages: ctx.messages, max_tokens: MAX_OUTPUT_TOKENS })
            .await
        {
            Ok(resp) => resp,
            Err(e) => {
                report.blockers.push(format!("provider error: {e}"));
                return Ok(report.finished(false));
            }
        };
        report.input_tokens += resp.input_tokens;
        report.output_tokens += resp.output_tokens;

        // 3. apply (each write is role- and scope-gated; search and run are
        // role-gated and read-only, their output feeds the next attempt's context)
        let fs = FsTool::new(agent.root(), &scope, &role);
        let grep = GrepTool::new(agent.root(), &role);
        let shell = ShellTool::new(agent.root(), &role, &cfg.commands);
        let mut tool_evidence = String::new();
        let actions = step::parse_actions(&resp.content);
        let action_count = actions.len();
        for action in actions {
            report.tool_calls += 1;
            match action {
                step::Action::Write { path, content } => {
                    if let Err(e) = fs.write(&path, &content) {
                        // In engineering tier a scope violation is a hard blocker; in
                        // light tier it is advisory (recorded, loop continues).
                        report.blockers.push(e.to_string());
                        if cfg.agent.tier.enforces() {
                            return Ok(report.finished(false));
                        }
                    }
                }
                step::Action::Search { pattern, path } => {
                    match grep.search(&pattern, path.as_deref()) {
                        Ok(hits) => {
                            let capped = if hits.len() >= crate::tools::grep::MAX_HITS {
                                " (capped — narrow the pattern or path for the rest)"
                            } else {
                                ""
                            };
                            tool_evidence.push_str(&format!(
                                "SEARCH '{pattern}' → {} hit(s){capped}:\n",
                                hits.len()
                            ));
                            for h in &hits {
                                tool_evidence
                                    .push_str(&format!("  {}:{}: {}\n", h.path, h.line_no, h.text));
                            }
                        }
                        Err(e @ Error::Scope(_)) => {
                            // Role boundary: same handling as a forbidden write.
                            report.blockers.push(e.to_string());
                            if cfg.agent.tier.enforces() {
                                return Ok(report.finished(false));
                            }
                        }
                        Err(e) => {
                            // e.g. an invalid regex — recorded and fed back so the
                            // model can correct it on the next bounded attempt.
                            report.blockers.push(e.to_string());
                            tool_evidence.push_str(&format!("SEARCH '{pattern}' failed: {e}\n"));
                        }
                    }
                }
                step::Action::Run { name } => {
                    match shell.run(&name) {
                        Ok(out) => {
                            // Execution failure (exit != 0 or timeout) is NOT a
                            // blocker — like a failed gate, its output is fed back
                            // as evidence and the loop continues (engineering does
                            // not hard-stop), so the model can fix and re-run.
                            let exit = match out.exit_code {
                                Some(code) => code.to_string(),
                                None if out.timed_out => "timeout".to_string(),
                                None => "killed".to_string(),
                            };
                            let raw = format!("{}{}", out.stdout, out.stderr);
                            let (body, capped) = cap_run_output(&raw);
                            tool_evidence.push_str(&format!("RUN '{name}' → exit {exit}:\n{body}\n"));
                            if !capped.is_empty() {
                                tool_evidence.push_str(&capped);
                            }
                        }
                        Err(e @ Error::Scope(_)) => {
                            // Authorization failure (undeclared / mutating / role):
                            // same handling as a forbidden write.
                            report.blockers.push(e.to_string());
                            if cfg.agent.tier.enforces() {
                                return Ok(report.finished(false));
                            }
                        }
                        Err(e) => {
                            // Could not run the command at all (e.g. program not
                            // found) — recorded and fed back like a failed search.
                            report.blockers.push(e.to_string());
                            tool_evidence.push_str(&format!("RUN '{name}' failed: {e}\n"));
                        }
                    }
                }
            }
        }

        // Ungoverned baseline (bench-only): no gate is consulted — that is the
        // point of the baseline. The model emitting zero actions is its own
        // claim of "done"; `completed` here means *claimed*, not verified. The
        // benchmark harness measures ground truth with an external verify.
        if opts.ungoverned {
            if action_count == 0 {
                return Ok(report.finished(true));
            }
            prior_evidence = tool_evidence;
            continue;
        }

        // 4. gate check (observable evidence)
        let mut all_passed = true;
        let mut needs_human = false;
        let mut evidence = String::new();
        // Accumulate gate outcomes across every step (do NOT clear each step), so
        // a multi-step run's bundle records the full gate history — how the run
        // converged — rather than only the final round. Each record is tagged
        // with its step for disambiguation.
        for gate in &cfg.gates.gates {
            let outcome = GateRunner::run(gate, agent.root());
            report.gate_outcomes.push(GateRecord {
                step: step_no,
                gate: outcome.gate.clone(),
                passed: outcome.passed,
                needs_human: outcome.needs_human,
            });
            if outcome.needs_human {
                needs_human = true;
            }
            if !outcome.passed {
                all_passed = false;
                // Feed back the gate's target condition alongside its evidence so
                // the re-attempt knows *what to satisfy* — not just that a check
                // failed. gate.rs guarantees non-empty evidence on failure, so
                // even a silent verify yields an actionable line here.
                evidence.push_str(&format!(
                    "[{}] {}: {}\n",
                    outcome.gate, gate.condition, outcome.evidence
                ));
            }
        }

        if all_passed {
            return Ok(report.finished(true));
        }
        if needs_human {
            report
                .blockers
                .push("a gate requires human judgment — stopping (tiered governance)".into());
            return Ok(report.finished(false));
        }

        // 5. observe → bounded re-attempt (tool output + failed-gate output)
        prior_evidence = format!("{tool_evidence}{evidence}");
    }

    report.blockers.push(if opts.ungoverned {
        format!("reached max_steps ({max_steps}) still emitting actions (never claimed done)")
    } else {
        format!("reached max_steps ({max_steps}) without passing all gates")
    });
    Ok(report.finished(false))
}
