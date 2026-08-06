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
use crate::exec::sandbox::Sandbox;
use crate::governance::gate::GateRunner;
use crate::governance::scope::Scope;
use crate::metrics::{ExitReason, GateRecord, RunReport};
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
fn cap_run_output(raw: &str, hint: &str) -> (String, String) {
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
        format!("(capped at {RUN_EVIDENCE_MAX_LINES} lines / {RUN_EVIDENCE_MAX_BYTES} bytes — {hint})\n")
    } else {
        String::new()
    };
    (body.trim_end().to_string(), note)
}

/// Token budget for the evidence the next attempt carries (slice-031). The loop
/// ACCUMULATES evidence across steps — a two-hop task needs step 1's index read
/// still visible while step 2 reads the data file, and the previous
/// overwrite-per-step window made serial multi-hop structurally non-terminating
/// (the grounding rule kept sending the model back to re-read what the window
/// had just dropped; see SLICE-031). This cap keeps the accumulated window from
/// flooding the context budget: retention is newest-first, the oldest step
/// blocks are dropped when the cap is hit, and the prompt says so. It is an
/// agent behaviour constant (part of the agent version in the comparability
/// key), not a ruler constant like `max_steps`.
const EVIDENCE_BUDGET_TOKENS: u32 = 4096;

/// One step's assembled evidence window plus what the assembly observed —
/// the SLICE-032 instrument. The telemetry is observation only: nothing in it
/// may alter what the window contains (measurement/policy separation), which
/// is why it rides alongside `text` instead of shaping it.
struct RetainedWindow {
    /// The window exactly as the prompt carries it.
    text: String,
    /// Token cost of the kept blocks, by the same estimator the budget uses.
    used_tokens: u32,
    /// Step numbers of the blocks this assembly dropped, ascending.
    evicted_steps: Vec<u32>,
}

/// Assemble the evidence window from the per-step log: newest blocks first
/// under [`EVIDENCE_BUDGET_TOKENS`], rendered oldest-to-newest with a step
/// label each. The newest block is always kept even when it alone exceeds the
/// cap — an empty window would be a regression to no memory at all. When older
/// blocks are dropped the window opens by saying so: a silently short history
/// reads as "that is all that happened", and a model that believes it will
/// re-read what it has already seen.
fn retained_evidence(log: &[(u32, String)]) -> RetainedWindow {
    let mut kept = 0usize;
    let mut used = 0u32;
    for (_, block) in log.iter().rev() {
        let cost = crate::util::estimate_tokens(block);
        if kept > 0 && used + cost > EVIDENCE_BUDGET_TOKENS {
            break;
        }
        used += cost;
        kept += 1;
    }
    let mut out = String::new();
    if kept < log.len() {
        let last_dropped = log[log.len() - kept - 1].0;
        out.push_str(&format!(
            "(evidence from steps 1–{last_dropped} dropped: evidence budget reached)\n"
        ));
    }
    for (step, block) in &log[log.len() - kept..] {
        out.push_str(&format!("── evidence from step {step} ──\n{block}"));
    }
    RetainedWindow {
        text: out,
        used_tokens: used,
        evicted_steps: log[..log.len() - kept].iter().map(|(s, _)| *s).collect(),
    }
}

/// Bench-only loop options (D2: the ungoverned baseline is a bench flag, not a
/// product tier). Crate-private — unreachable from the CLI/config surface.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct LoopOptions {
    /// Ungoverned baseline: scope lists are not enforced (root escape still is),
    /// gates are never consulted, and the run ends when the model emits zero
    /// actions — its own, unverified claim of "done" — or at `max_steps`. The
    /// prompt carries the same information as a governed run (same writable
    /// list, same file contents) minus the obligation to respect it — see
    /// `context::scope_rules` and `tkt-m1-null-is-structural`.
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

    // OS sandbox for every spawned child (RUN commands + gate verify), ADR-003.
    // Built once and shared by the RUN tool and the gate runner so both paths are
    // confined identically. Resolves to Disabled when the config opts out.
    let sandbox = match cfg.agent.sandbox.to_policy(agent.root(), cfg.agent.tier.enforces()) {
        Some(policy) => Sandbox::for_policy(policy),
        None => Sandbox::disabled(),
    };

    let mut report = RunReport::new(&task.instruction).with_provenance(&cfg.agent.provider);
    // The budget travels with the evidence: `steps` alone cannot distinguish
    // "converged in 3" from "was cut off at 3".
    report.max_steps = max_steps;
    // Record whether children are actually confined, so the evidence bundle can
    // distinguish enforcement from intention. A degradation (unavailable backend)
    // is surfaced as a blocker rather than left silent.
    report.sandbox = sandbox.status();
    if let Some(warning) = sandbox.warning() {
        report.blockers.push(warning);
    }
    // This loop parses the actions itself, so it can attribute them. Claimed
    // here rather than on first action: a run that emitted none must read as
    // "attributable, and it did nothing", not as "not attributable".
    report.action_counts = Some(crate::metrics::ActionCounts::default());
    // Window telemetry (SLICE-032 instrument), claimed on entry under the same
    // contract: a run whose window never evicted must read as "observed, and
    // nothing was evicted", not as "not attributable".
    report.evictions = Some(crate::metrics::Evictions::default());
    report.dropped_reread = Some(0);
    report.window_peak_tokens = Some(0);
    // One labeled block per step that produced evidence; the window the next
    // attempt sees is assembled by `retained_evidence` (accumulate, not
    // overwrite — slice-031). Both postures push into the same log: the window
    // depth is capability, not obligation, and a baseline with a shallower
    // memory would turn the differential into a measurement of the window.
    let mut evidence_log: Vec<(u32, String)> = Vec::new();
    // Step of the most recent successful READ per path — the reference the
    // dropped-reread instrument compares against the evicted set. Observation
    // only; never consulted by the loop's own decisions.
    let mut last_read: std::collections::HashMap<String, u32> = std::collections::HashMap::new();

    for step_no in 1..=max_steps {
        report.steps = step_no;

        // 1. prepare context (re-built each attempt; carries the accumulated
        // evidence window — tool output and failed-gate output from every
        // prior step that fits the evidence budget)
        let window = retained_evidence(&evidence_log);
        // Record what the assembly observed. Retention is a monotone suffix
        // (once out, a block stays out), so this assembly's evicted list IS
        // the run's full evicted set so far — assignment, not union.
        if let Some(peak) = report.window_peak_tokens.as_mut() {
            *peak = (*peak).max(window.used_tokens);
        }
        if !window.evicted_steps.is_empty() {
            if let Some(ev) = report.evictions.as_mut() {
                ev.count += 1;
                ev.first_step.get_or_insert(step_no);
                ev.evicted_steps = window.evicted_steps.clone();
            }
        }
        let ctx = context::build(
            agent.root(),
            &scope,
            &role,
            budget,
            &task.instruction,
            &window.text,
            &cfg.commands,
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
                // Both forms, deliberately: the blocker is what a human reads,
                // `provider_error` is what a consumer branches on. A benchmark
                // must be able to tell "the model answered badly" from "the
                // model never answered" without parsing this string.
                report.blockers.push(format!("provider error: {e}"));
                report.provider_error = Some(e.to_string());
                report.exit = ExitReason::ProviderError;
                return Ok(report.finished(false));
            }
        };
        report.input_tokens += resp.input_tokens;
        report.output_tokens += resp.output_tokens;

        // 3. apply (each write is role- and scope-gated; search and run are
        // role-gated and read-only, their output feeds the next attempt's context)
        let fs = FsTool::new(agent.root(), &scope, &role);
        let grep = GrepTool::new(agent.root(), &role);
        let shell = ShellTool::new(agent.root(), &role, &cfg.commands, &sandbox);
        let mut tool_evidence = String::new();
        let actions = step::parse_actions(&resp.content);
        let action_count = actions.len();
        for action in actions {
            report.tool_calls += 1;
            if let Some(counts) = report.action_counts.as_mut() {
                match &action {
                    step::Action::Write { .. } => counts.write += 1,
                    step::Action::Edit { .. } => counts.edit += 1,
                    step::Action::Read { .. } => counts.read += 1,
                    step::Action::Search { .. } => counts.search += 1,
                    step::Action::Run { .. } => counts.run += 1,
                }
            }
            match action {
                step::Action::Write { path, content } => {
                    if let Err(e) = fs.write(&path, &content) {
                        // In engineering tier a scope violation is a hard blocker; in
                        // light tier it is advisory (recorded, loop continues).
                        if matches!(e, Error::Scope(_)) {
                            report.scope_refusals.push(path.clone());
                        }
                        report.blockers.push(e.to_string());
                        if cfg.agent.tier.enforces() {
                            report.exit = ExitReason::HardBlocked;
                            return Ok(report.finished(false));
                        }
                    }
                }
                step::Action::Edit { path, old, new } => {
                    match fs.edit(&path, &old, &new) {
                        Ok(()) => {}
                        // An edit is a write: identical refusal handling, and the
                        // same scope_refusals record — it is the false_blocks
                        // denominator on the native leg.
                        Err(e @ Error::Scope(_)) => {
                            report.scope_refusals.push(path.clone());
                            report.blockers.push(e.to_string());
                            if cfg.agent.tier.enforces() {
                                report.exit = ExitReason::HardBlocked;
                                return Ok(report.finished(false));
                            }
                        }
                        // Anchor failures (not found / ambiguous / empty) are
                        // recorded and fed back so the model can re-anchor on the
                        // next bounded attempt — like an invalid regex.
                        Err(e) => {
                            report.blockers.push(e.to_string());
                            tool_evidence.push_str(&format!("{e}\n"));
                        }
                    }
                }
                step::Action::Read { path } => {
                    // The dropped-reread instrument: this READ targets a path
                    // whose last successful read now sits in an evicted block —
                    // the model going back for what the window dropped. Counted
                    // on the attempt (the behavior), not on the read succeeding.
                    if last_read.get(&path).is_some_and(|s| window.evicted_steps.contains(s)) {
                        if let Some(n) = report.dropped_reread.as_mut() {
                            *n += 1;
                        }
                    }
                    match fs.read(&path) {
                        Ok(content) => {
                            last_read.insert(path.clone(), step_no);
                            let (body, capped) = cap_run_output(
                                &content,
                                "READ shows the head; SEARCH for the rest",
                            );
                            tool_evidence.push_str(&format!("READ '{path}':\n{body}\n"));
                            if !capped.is_empty() {
                                tool_evidence.push_str(&capped);
                            }
                        }
                        // Role boundary or a path escaping the root: same
                        // handling as a forbidden search.
                        Err(e @ Error::Scope(_)) => {
                            report.blockers.push(e.to_string());
                            if cfg.agent.tier.enforces() {
                                report.exit = ExitReason::HardBlocked;
                                return Ok(report.finished(false));
                            }
                        }
                        // e.g. the file does not exist — recorded and fed back.
                        Err(e) => {
                            report.blockers.push(e.to_string());
                            tool_evidence.push_str(&format!("READ '{path}' failed: {e}\n"));
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
                            report.search_hits.push(Some(hits.len() as u32));
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
                            report.search_hits.push(None);
                            report.blockers.push(e.to_string());
                            if cfg.agent.tier.enforces() {
                                report.exit = ExitReason::HardBlocked;
                                return Ok(report.finished(false));
                            }
                        }
                        Err(e) => {
                            // e.g. an invalid regex — recorded and fed back so the
                            // model can correct it on the next bounded attempt. The
                            // `None` keeps this search in the sequence: a run that
                            // errored twice then searched well is a different shape
                            // from one that searched well twice.
                            report.search_hits.push(None);
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
                            let (body, capped) =
                                cap_run_output(&raw, "narrow the command for the rest");
                            tool_evidence
                                .push_str(&format!("RUN '{name}' → exit {exit}:\n{body}\n"));
                            if !capped.is_empty() {
                                tool_evidence.push_str(&capped);
                            }
                        }
                        Err(e @ Error::Scope(_)) => {
                            // Authorization failure (undeclared / mutating / role):
                            // same handling as a forbidden write.
                            report.blockers.push(e.to_string());
                            if cfg.agent.tier.enforces() {
                                report.exit = ExitReason::HardBlocked;
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
                report.exit = ExitReason::ClaimedDone;
                return Ok(report.finished(true));
            }
            if !tool_evidence.is_empty() {
                evidence_log.push((step_no, tool_evidence));
            }
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
            let outcome = GateRunner::run(gate, agent.root(), &sandbox);
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
            report.exit = ExitReason::GatesPassed;
            return Ok(report.finished(true));
        }
        if needs_human {
            report
                .blockers
                .push("a gate requires human judgment — stopping (tiered governance)".into());
            report.exit = ExitReason::NeedsHuman;
            return Ok(report.finished(false));
        }

        // 5. observe → bounded re-attempt (tool output + failed-gate output
        // join the accumulated window; retention happens at the loop top)
        let block = format!("{tool_evidence}{evidence}");
        if !block.is_empty() {
            evidence_log.push((step_no, block));
        }
    }

    report.blockers.push(if opts.ungoverned {
        format!("reached max_steps ({max_steps}) still emitting actions (never claimed done)")
    } else {
        format!("reached max_steps ({max_steps}) without passing all gates")
    });
    report.exit = ExitReason::BudgetExhausted;
    Ok(report.finished(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── retained_evidence: the window's retention discipline ───────────────

    fn block(step: u32, body: &str) -> (u32, String) {
        (step, format!("{body}\n"))
    }

    #[test]
    fn everything_fits_and_stays_in_step_order() {
        let log = vec![block(1, "first"), block(2, "second")];
        let win = retained_evidence(&log);
        let out = &win.text;
        let a = out.find("step 1").unwrap();
        let b = out.find("step 2").unwrap();
        assert!(a < b, "blocks render oldest-to-newest: {out}");
        assert!(out.contains("first") && out.contains("second"));
        assert!(!out.contains("dropped"), "nothing was dropped, nothing is claimed dropped");
        assert!(win.evicted_steps.is_empty(), "telemetry agrees: nothing evicted");
        assert!(win.used_tokens > 0, "kept blocks have a measured cost");
    }

    #[test]
    fn the_oldest_blocks_are_dropped_first_and_the_window_says_so() {
        // Three blocks, each ~2000 tokens (8000 chars): only the newest two fit
        // the 4096 budget.
        let big = "x".repeat(8000);
        let log = vec![block(1, &big), block(2, &big), block(3, &big)];
        let win = retained_evidence(&log);
        let out = &win.text;
        assert!(!out.contains("step 1 ──"), "the oldest block is dropped");
        assert!(out.contains("step 2 ──") && out.contains("step 3 ──"));
        assert!(
            out.contains("(evidence from steps 1–1 dropped: evidence budget reached)"),
            "the drop is announced, not silent: {}",
            &out[..120]
        );
        assert_eq!(win.evicted_steps, vec![1], "telemetry names the evicted block");
        assert!(
            win.used_tokens <= EVIDENCE_BUDGET_TOKENS && win.used_tokens > 4000,
            "the kept cost sits just under the budget: {}",
            win.used_tokens
        );
    }

    #[test]
    fn the_newest_block_survives_even_when_it_alone_exceeds_the_budget() {
        // An empty window would be a regression to no memory at all.
        let huge = "y".repeat(40_000);
        let log = vec![block(1, "small"), block(2, &huge)];
        let win = retained_evidence(&log);
        assert!(win.text.contains("step 2 ──"), "the newest evidence is never evicted");
        assert!(!win.text.contains("small"), "the older block yields");
        assert_eq!(win.evicted_steps, vec![1]);
        assert!(
            win.used_tokens > EVIDENCE_BUDGET_TOKENS,
            "the peak records the over-budget block at its real cost"
        );
    }

    #[test]
    fn an_empty_log_is_an_empty_window() {
        let win = retained_evidence(&[]);
        assert_eq!(win.text, "");
        assert_eq!(win.used_tokens, 0);
        assert!(win.evicted_steps.is_empty());
    }

    // ── the loop accumulates across steps, in both postures ────────────────
    //
    // The defect this pins (slice-031): the window used to be overwritten each
    // step, so on a two-hop task step 1's index read was gone by step 3 and the
    // grounding rule sent the model back to re-read it — serial multi-hop never
    // terminated. The scripted provider reads a different file on each step;
    // by step 3 the prompt must still carry the evidence of BOTH reads.

    use crate::config::agent::{AgentConfig, ProviderSelection, Tier};
    use crate::config::commands::Commands;
    use crate::config::context_budget::ContextBudgets;
    use crate::config::gates::{Gate, Gates};
    use crate::config::roles::{Role, Roles};
    use crate::config::Config;
    use crate::provider::{ChatResponse, Message, Provider};
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};

    fn config(gates: Vec<Gate>) -> Config {
        Config {
            agent: AgentConfig {
                provider: ProviderSelection {
                    kind: "offline".into(),
                    model: "stub".into(),
                    base_url: None,
                    api_key_env: None,
                    sampling: None,
                },
                tier: Tier::Light,
                default_role: "developer".into(),
                max_steps: 3,
                sandbox: Default::default(),
            },
            roles: Roles {
                roles: vec![Role {
                    name: "developer".into(),
                    allowed_tools: vec!["fs.read".into(), "fs.write".into()],
                    forbidden_tools: vec![],
                    knowledge_scope: vec![],
                }],
            },
            gates: Gates { gates },
            budgets: ContextBudgets::default(),
            commands: Commands::default(),
        }
    }

    /// Emits `READ a.txt` on step 1, `READ b.txt` on step 2, then keeps
    /// emitting a read so the ungoverned run never claims done. Records every
    /// user message it is sent.
    struct TwoHopReader {
        prompts: Arc<Mutex<Vec<String>>>,
        calls: Mutex<u32>,
    }

    #[async_trait]
    impl Provider for TwoHopReader {
        fn id(&self) -> &str {
            "two-hop-reader"
        }
        async fn chat(
            &self,
            req: crate::provider::ChatRequest,
        ) -> crate::error::Result<ChatResponse> {
            let user = req
                .messages
                .iter()
                .filter(|m: &&Message| m.role == "user")
                .map(|m| m.content.clone())
                .collect::<Vec<_>>()
                .join("\n");
            self.prompts.lock().unwrap().push(user);
            let mut calls = self.calls.lock().unwrap();
            *calls += 1;
            let content = match *calls {
                1 => "<<<READ a.txt\n>>>",
                _ => "<<<READ b.txt\n>>>",
            };
            Ok(ChatResponse { content: content.into(), input_tokens: 0, output_tokens: 0 })
        }
    }

    fn prompts_seen(ungoverned: bool) -> Vec<String> {
        let root = std::env::temp_dir().join(format!(
            "orvena-evidence-{}-{}",
            std::process::id(),
            ungoverned
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.txt"), "the index says: see b.txt\n").unwrap();
        std::fs::write(root.join("b.txt"), "the value is seven\n").unwrap();
        std::fs::write(root.join("out.txt"), "stale\n").unwrap();

        // A gate that never passes keeps the governed loop re-attempting to
        // max_steps; the ungoverned loop continues because the provider keeps
        // emitting actions.
        let gates = vec![Gate {
            name: "check".into(),
            condition: "out.txt is correct".into(),
            verify: Some("false".into()),
            gatekeeper: Default::default(),
            timeout_secs: None,
        }];
        let prompts = Arc::new(Mutex::new(Vec::new()));
        let provider = TwoHopReader { prompts: Arc::clone(&prompts), calls: Mutex::new(0) };
        let agent = super::super::Agent::with_provider(
            config(if ungoverned { vec![] } else { gates }),
            &root,
            Box::new(provider),
        );
        let task = Task::new("do the thing", vec!["out.txt".into()]);
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let opts = LoopOptions { ungoverned };
        rt.block_on(async {
            run_loop_with(&agent, task, opts).await.unwrap();
        });
        let _ = std::fs::remove_dir_all(&root);
        let out = prompts.lock().unwrap().clone();
        assert_eq!(out.len(), 3, "the loop should run all three steps");
        out
    }

    /// Emits a fixed script of one action per step, never claiming done.
    struct ScriptedReader {
        script: Vec<&'static str>,
        calls: Mutex<usize>,
    }

    #[async_trait]
    impl Provider for ScriptedReader {
        fn id(&self) -> &str {
            "scripted-reader"
        }
        async fn chat(
            &self,
            _req: crate::provider::ChatRequest,
        ) -> crate::error::Result<ChatResponse> {
            let mut calls = self.calls.lock().unwrap();
            let content = self.script[(*calls).min(self.script.len() - 1)];
            *calls += 1;
            Ok(ChatResponse { content: content.into(), input_tokens: 0, output_tokens: 0 })
        }
    }

    #[test]
    fn eviction_telemetry_records_the_drop_and_the_reread() {
        // Two ~2050-token reads fill the 4096 budget; assembling step 3's
        // window must evict step 1's block — and step 3 re-reading that same
        // path is exactly the dropped-reread the instrument exists to count.
        let root =
            std::env::temp_dir().join(format!("orvena-window-telemetry-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let big = format!("{}\n", "x".repeat(80)).repeat(110);
        std::fs::write(root.join("big1.txt"), &big).unwrap();
        std::fs::write(root.join("big2.txt"), &big).unwrap();

        let provider = ScriptedReader {
            script: vec!["<<<READ big1.txt\n>>>", "<<<READ big2.txt\n>>>", "<<<READ big1.txt\n>>>"],
            calls: Mutex::new(0),
        };
        let agent = super::super::Agent::with_provider(config(vec![]), &root, Box::new(provider));
        let task = Task::new("do the thing", vec!["out.txt".into()]);
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let report =
            rt.block_on(run_loop_with(&agent, task, LoopOptions { ungoverned: true })).unwrap();
        let _ = std::fs::remove_dir_all(&root);

        let ev = report.evictions.expect("the native loop claims the instrument on entry");
        assert_eq!(ev.count, 1, "only step 3's assembly evicted");
        assert_eq!(ev.first_step, Some(3));
        assert_eq!(ev.evicted_steps, vec![1], "the evicted block is named, not just counted");
        assert_eq!(
            report.dropped_reread,
            Some(1),
            "step 3 re-read the path whose evidence step 3's window dropped"
        );
        let peak = report.window_peak_tokens.expect("claimed on entry");
        assert!(
            peak > 2000 && peak <= EVIDENCE_BUDGET_TOKENS,
            "the peak is one big block's cost, under the budget: {peak}"
        );
    }

    #[test]
    fn a_run_without_pressure_reads_observed_and_quiet_not_unattributable() {
        // `Some(0)`/empty vs `None` is the same contract as `action_counts`:
        // a wrapped agent has no readable window, a quiet native run does.
        let root = std::env::temp_dir().join(format!("orvena-window-quiet-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.txt"), "tiny\n").unwrap();

        let provider = ScriptedReader { script: vec!["<<<READ a.txt\n>>>"], calls: Mutex::new(0) };
        let agent = super::super::Agent::with_provider(config(vec![]), &root, Box::new(provider));
        let task = Task::new("do the thing", vec!["out.txt".into()]);
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let report =
            rt.block_on(run_loop_with(&agent, task, LoopOptions { ungoverned: true })).unwrap();
        let _ = std::fs::remove_dir_all(&root);

        let ev = report.evictions.expect("observed, and nothing was evicted");
        assert_eq!(ev.count, 0);
        assert_eq!(ev.first_step, None);
        assert!(ev.evicted_steps.is_empty());
        assert_eq!(report.dropped_reread, Some(0), "re-reads without eviction do not count");
        assert!(report.window_peak_tokens.unwrap() > 0, "the window was occupied, just small");
    }

    #[test]
    fn step_three_still_carries_step_ones_evidence_governed() {
        let prompts = prompts_seen(false);
        let third = &prompts[2];
        assert!(
            third.contains("the index says: see b.txt"),
            "step 1's READ must survive into step 3's window"
        );
        assert!(third.contains("the value is seven"), "step 2's READ is present too");
    }

    #[test]
    fn step_three_still_carries_step_ones_evidence_ungoverned() {
        // Window depth is capability, not obligation: the baseline gets the
        // same memory, or the differential measures the window.
        let prompts = prompts_seen(true);
        let third = &prompts[2];
        assert!(third.contains("the index says: see b.txt"));
        assert!(third.contains("the value is seven"));
    }
}
