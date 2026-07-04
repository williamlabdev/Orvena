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

/// Output cap per model call (input is governed by the context budget).
const MAX_OUTPUT_TOKENS: u32 = 1024;

pub async fn run_loop(agent: &Agent, task: Task) -> Result<RunReport> {
    let cfg = agent.config();
    let role = cfg
        .roles
        .get(&cfg.agent.default_role)
        .ok_or_else(|| Error::Config(format!("role '{}' not found", cfg.agent.default_role)))?
        .clone();

    let scope = Scope::new(task.allowed_modifications.clone(), Vec::new(), cfg.agent.tier);
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
        let resp = agent
            .provider()
            .chat(ChatRequest { messages: ctx.messages, max_tokens: MAX_OUTPUT_TOKENS })
            .await?;
        report.input_tokens += resp.input_tokens;
        report.output_tokens += resp.output_tokens;

        // 3. apply (each write is role- and scope-gated; search is role-gated
        // and read-only, its results feed the next attempt's context)
        let fs = FsTool::new(agent.root(), &scope, &role);
        let grep = GrepTool::new(agent.root(), &role);
        let mut search_evidence = String::new();
        for action in step::parse_actions(&resp.content) {
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
                            search_evidence.push_str(&format!(
                                "SEARCH '{pattern}' → {} hit(s){capped}:\n",
                                hits.len()
                            ));
                            for h in &hits {
                                search_evidence
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
                            search_evidence.push_str(&format!("SEARCH '{pattern}' failed: {e}\n"));
                        }
                    }
                }
            }
        }

        // 4. gate check (observable evidence)
        let mut all_passed = true;
        let mut needs_human = false;
        let mut evidence = String::new();
        report.gate_outcomes.clear();
        for gate in &cfg.gates.gates {
            let outcome = GateRunner::run(gate, agent.root());
            report.gate_outcomes.push(GateRecord {
                gate: outcome.gate.clone(),
                passed: outcome.passed,
                needs_human: outcome.needs_human,
            });
            if outcome.needs_human {
                needs_human = true;
            }
            if !outcome.passed {
                all_passed = false;
                if !outcome.evidence.is_empty() {
                    evidence.push_str(&format!("[{}] {}\n", outcome.gate, outcome.evidence));
                }
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

        // 5. observe → bounded re-attempt (search results + failed-gate output)
        prior_evidence = format!("{search_evidence}{evidence}");
    }

    report.blockers.push(format!(
        "reached max_steps ({max_steps}) without passing all gates"
    ));
    Ok(report.finished(false))
}
