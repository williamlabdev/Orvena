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

/// Fed back when a reply carried native tool-call markup and nothing this
/// protocol parses (#59). Same text in both postures.
const NATIVE_MARKUP_FEEDBACK: &str = "FORMAT ERROR: your last reply used tool-call markup \
     (<invoke …>) that this protocol does not accept; nothing was executed. Use the \
     <<<…>>> blocks from the rules, or finish with <<<DONE or <<<BLOCKED.\n";

/// Fed back when a reply carried no action and no terminal block (#54). Same
/// text in both postures.
const NO_ACTION_FEEDBACK: &str = "NO ACTION: your last reply contained no action block, \
     so nothing happened. Act with a <<<…>>> block, or finish with <<<DONE or <<<BLOCKED.\n";

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

/// Ceiling on the pinned share of the evidence window (slice-033). Half, not
/// all: a fully pinnable window is a bigger desk in disguise — the rung is
/// "learned to choose what to keep", and choice needs scarcity. PIN accepts
/// are charged against this at accept time; the refusal is spoken into the
/// window (a silent refusal reads as "pinned" to the model).
const PINNED_BUDGET_TOKENS: u32 = 2048;

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

/// Assemble the evidence window from the per-step log: pinned blocks are
/// exempt from eviction and charged first, then unpinned blocks newest-first
/// under what remains of [`EVIDENCE_BUDGET_TOKENS`], rendered oldest-to-newest
/// with a step label each (pinned blocks say so — a pin the model cannot see
/// holding is a pin it will re-issue). The newest block is always kept even
/// when it alone exceeds the cap — an empty window would be a regression to no
/// memory at all; with pins the worst case is pinned + newest, so overflow
/// still only enters through that one carve-out (slice-033). With no pins this
/// reduces byte-for-byte to the 0.5.0 assembly. When blocks are dropped the
/// window opens by saying so: a silently short history reads as "that is all
/// that happened", and a model that believes it will re-read what it has
/// already seen.
fn retained_evidence(
    log: &[(u32, String)],
    pinned: &std::collections::BTreeSet<u32>,
) -> RetainedWindow {
    let mut keep = vec![false; log.len()];
    let mut used = 0u32;
    for (i, (step, block)) in log.iter().enumerate() {
        if pinned.contains(step) {
            keep[i] = true;
            used += crate::util::estimate_tokens(block);
        }
    }
    // Unpinned retention stays a newest-first suffix (an older block never
    // outlives a newer one it squeezed past): stop at the first non-fitting
    // block rather than continuing to smaller older ones.
    let mut kept_any_unpinned = false;
    for (i, (step, block)) in log.iter().enumerate().rev() {
        if pinned.contains(step) {
            continue;
        }
        let cost = crate::util::estimate_tokens(block);
        if kept_any_unpinned && used + cost > EVIDENCE_BUDGET_TOKENS {
            break;
        }
        keep[i] = true;
        used += cost;
        kept_any_unpinned = true;
    }
    let evicted_steps: Vec<u32> =
        log.iter().enumerate().filter(|(i, _)| !keep[*i]).map(|(_, (s, _))| *s).collect();
    let mut out = String::new();
    if !evicted_steps.is_empty() {
        if pinned.is_empty() {
            // No pins → the evicted set is a contiguous prefix; keep the
            // 0.5.0 wording byte-for-byte.
            let last_dropped = *evicted_steps.last().unwrap();
            out.push_str(&format!(
                "(evidence from steps 1–{last_dropped} dropped: evidence budget reached)\n"
            ));
        } else {
            let list = evicted_steps.iter().map(u32::to_string).collect::<Vec<_>>().join(", ");
            out.push_str(&format!(
                "(evidence from steps {list} dropped: evidence budget reached; pinned steps are kept)\n"
            ));
        }
    }
    for (i, (step, block)) in log.iter().enumerate() {
        if !keep[i] {
            continue;
        }
        if pinned.contains(step) {
            out.push_str(&format!("── evidence from step {step} (pinned) ──\n{block}"));
        } else {
            out.push_str(&format!("── evidence from step {step} ──\n{block}"));
        }
    }
    RetainedWindow { text: out, used_tokens: used, evicted_steps }
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
    report.dropped_research = Some(0);
    report.window_peak_tokens = Some(0);
    // PIN telemetry (slice-033), same contract: a run that never pinned must
    // read as "observed, and it never pinned", not as "not attributable".
    report.pins = Some(crate::metrics::Pins::default());
    // One labeled block per step that produced evidence; the window the next
    // attempt sees is assembled by `retained_evidence` (accumulate, not
    // overwrite — slice-031). Both postures push into the same log: the window
    // depth is capability, not obligation, and a baseline with a shallower
    // memory would turn the differential into a measurement of the window.
    let mut evidence_log: Vec<(u32, String)> = Vec::new();
    // Steps that carried a `<<<DONE` the gates did not accept (governed only;
    // ungoverned DONE ends the run). A claim is not an abstention (#54).
    let mut unaccepted_done: u32 = 0;
    // Step of the most recent successful READ per path — the reference the
    // dropped-reread instrument compares against the evicted set. Observation
    // only; never consulted by the loop's own decisions.
    let mut last_read: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    // Steps whose evidence the model pinned (slice-033) and the token cost the
    // accepted pins carry — the accept-time charge against
    // [`PINNED_BUDGET_TOKENS`]. Model-controlled: nothing here is ever set by
    // the loop's own judgment.
    let mut pinned: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
    let mut pinned_cost: u32 = 0;

    for step_no in 1..=max_steps {
        report.steps = step_no;

        // 1. prepare context (re-built each attempt; carries the accumulated
        // evidence window — tool output and failed-gate output from every
        // prior step that fits the evidence budget)
        let window = retained_evidence(&evidence_log, &pinned);
        // Record what the assembly observed. Retention stays monotone (once
        // out, a block stays out: pins on evicted steps are refused, and the
        // unpinned budget only tightens), so this assembly's evicted list IS
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
        report.set_final_reply(&resp.content);

        // 3. apply (each write is role- and scope-gated; search and run are
        // role-gated and read-only, their output feeds the next attempt's context)
        let fs = FsTool::new(agent.root(), &scope, &role);
        let grep = GrepTool::new(agent.root(), &role);
        let shell = ShellTool::new(agent.root(), &role, &cfg.commands, &sandbox);
        let mut tool_evidence = String::new();
        let parsed = step::parse_step(&resp.content);
        let terminal = parsed.terminal;
        if matches!(terminal, Some(step::Terminal::Done { .. })) {
            unaccepted_done += 1;
        }
        let action_count = parsed.actions.len();
        for action in parsed.actions {
            report.tool_calls += 1;
            if let Some(counts) = report.action_counts.as_mut() {
                match &action {
                    step::Action::Write { .. } => counts.write += 1,
                    step::Action::Edit { .. } => counts.edit += 1,
                    step::Action::Read { .. } => counts.read += 1,
                    step::Action::Search { .. } => counts.search += 1,
                    step::Action::Run { .. } => counts.run += 1,
                    step::Action::Pin { .. } => counts.pin += 1,
                }
            }
            match action {
                step::Action::Write { path, content } => {
                    if let Err(e) = fs.write(&path, &content) {
                        // In engineering tier a scope violation is a hard blocker; in
                        // light tier it is advisory (recorded, loop continues).
                        if e.is_scope() {
                            report.scope_refusals.push(path.clone());
                        }
                        if e.is_host_guard() {
                            report.host_guard_refusals += 1;
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
                        Err(e) if e.is_scope() => {
                            if e.is_host_guard() {
                                report.host_guard_refusals += 1;
                            }
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
                        Err(e) if e.is_scope() => {
                            if e.is_host_guard() {
                                report.host_guard_refusals += 1;
                            }
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
                            // The dropped-research instrument: this SEARCH hit a
                            // path whose last successful READ now sits in an
                            // evicted block — re-acquisition by the route the
                            // READ truncation note recommends. Counted once per
                            // SEARCH action, symmetric with dropped_reread.
                            // Hit paths are root-relative; READ paths are as the
                            // model wrote them, so compare modulo a `./` prefix.
                            fn norm(p: &str) -> &str {
                                p.trim_start_matches("./")
                            }
                            if hits.iter().any(|h| {
                                last_read.iter().any(|(p, s)| {
                                    norm(p) == norm(&h.path) && window.evicted_steps.contains(s)
                                })
                            }) {
                                if let Some(n) = report.dropped_research.as_mut() {
                                    *n += 1;
                                }
                            }
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
                        Err(e) if e.is_scope() => {
                            // Role boundary: same handling as a forbidden write.
                            if e.is_host_guard() {
                                report.host_guard_refusals += 1;
                            }
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
                step::Action::Pin { step } => {
                    // Window bookkeeping, not a tool: no role gate, no scope
                    // gate, identical in both postures (measurement/policy
                    // separation — the pin surface is capability, not
                    // obligation). Every outcome is spoken into the window:
                    // a silent refusal reads as "pinned" to the model.
                    let block_cost = evidence_log
                        .iter()
                        .find(|(s, _)| *s == step)
                        .map(|(_, b)| crate::util::estimate_tokens(b));
                    let verdict = match block_cost {
                        // Axis protection (slice-033): the log still holds
                        // evicted blocks, so honoring this pin would be a
                        // free recall of dropped evidence — the window
                        // pressure the v3 ruler stands on would evaporate.
                        _ if window.evicted_steps.contains(&step) => {
                            Err(format!("PIN {step} refused: step {step} was already evicted"))
                        }
                        None => Err(format!("PIN {step} refused: no evidence from step {step}")),
                        Some(_) if pinned.contains(&step) => {
                            Ok(format!("PIN {step}: already pinned\n"))
                        }
                        Some(cost) if pinned_cost + cost > PINNED_BUDGET_TOKENS => {
                            Err(format!("PIN {step} refused: pinned budget full"))
                        }
                        Some(cost) => {
                            pinned.insert(step);
                            pinned_cost += cost;
                            if let Some(p) = report.pins.as_mut() {
                                p.count += 1;
                                p.pinned_steps.push(step);
                            }
                            Ok(format!("PIN {step}: evidence from step {step} pinned\n"))
                        }
                    };
                    match verdict {
                        Ok(line) => tool_evidence.push_str(&line),
                        Err(msg) => {
                            if let Some(p) = report.pins.as_mut() {
                                p.refused += 1;
                            }
                            tool_evidence.push_str(&msg);
                            tool_evidence.push('\n');
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
                        Err(e) if e.is_scope() => {
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

        // Termination is explicit in both postures (#54). The actions in the
        // same reply have already been applied above; the terminal block ends
        // the run after them.
        if let Some(step::Terminal::Blocked { reason }) = &terminal {
            report.blockers.push(if reason.is_empty() {
                "agent stopped with BLOCKED (no reason given)".to_string()
            } else {
                format!("agent stopped with BLOCKED: {reason}")
            });
            report.exit = ExitReason::Refused;
            return Ok(report.finished(false));
        }
        // A reply with neither an action nor a terminal block is not "done"
        // any more (#54, #59): say why nothing happened and re-prompt. Native
        // tool-call markup is named as such — detected, never executed.
        if action_count == 0 && terminal.is_none() {
            if parsed.native_markup {
                report.format_errors += 1;
                tool_evidence.push_str(NATIVE_MARKUP_FEEDBACK);
            } else {
                tool_evidence.push_str(NO_ACTION_FEEDBACK);
            }
        }

        // Ungoverned baseline (bench-only): no gate is consulted — that is the
        // point of the baseline. `<<<DONE` is the agent's own claim; `completed`
        // here means *claimed*, not verified. The benchmark harness measures
        // ground truth with an external verify.
        if opts.ungoverned {
            if matches!(terminal, Some(step::Terminal::Done { .. })) {
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
        format!("reached max_steps ({max_steps}) without DONE or BLOCKED")
    } else {
        format!("reached max_steps ({max_steps}) without passing all gates")
    });
    if unaccepted_done > 0 {
        report.blockers.push(format!(
            "claimed DONE on {unaccepted_done} of {max_steps} steps; the gates never passed"
        ));
    }
    if report.tool_calls == 0 && unaccepted_done == 0 {
        // Same loop, same steps; only the label differs (#46): a run that never
        // emitted an action did not burn its budget acting.
        report.blockers.push(format!("model emitted no action on any of {max_steps} steps"));
        report.exit = ExitReason::Abstained;
    } else {
        report.exit = ExitReason::BudgetExhausted;
    }
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
        let win = retained_evidence(&log, &Default::default());
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
        let win = retained_evidence(&log, &Default::default());
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
        let win = retained_evidence(&log, &Default::default());
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
        let win = retained_evidence(&[], &Default::default());
        assert_eq!(win.text, "");
        assert_eq!(win.used_tokens, 0);
        assert!(win.evicted_steps.is_empty());
    }

    // ── slice-033: pinned blocks are exempt from eviction ──────────────────

    #[test]
    fn a_pinned_block_survives_and_the_window_says_so() {
        // Three ~2000-token blocks; only two fit. 0.5.0 would drop the oldest;
        // with step 1 pinned the unpinned middle block yields instead.
        let big = "x".repeat(8000);
        let log = vec![block(1, &big), block(2, &big), block(3, &big)];
        let pinned = std::collections::BTreeSet::from([1]);
        let win = retained_evidence(&log, &pinned);
        assert!(win.text.contains("step 1 (pinned) ──"), "the pin is visible, not silent");
        assert!(win.text.contains("step 3 ──"));
        assert!(!win.text.contains("step 2 ──"), "the unpinned middle block yields");
        assert_eq!(win.evicted_steps, vec![2]);
        assert!(
            win.text.contains(
                "(evidence from steps 2 dropped: evidence budget reached; pinned steps are kept)"
            ),
            "a non-prefix drop is listed, not ranged: {}",
            &win.text[..160]
        );
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

    /// Emits a fixed script and records every user prompt it is sent.
    struct RecordingScript {
        script: Vec<&'static str>,
        calls: Mutex<usize>,
        prompts: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl Provider for RecordingScript {
        fn id(&self) -> &str {
            "recording-script"
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
            let content = self.script[(*calls).min(self.script.len() - 1)];
            *calls += 1;
            Ok(ChatResponse { content: content.into(), input_tokens: 0, output_tokens: 0 })
        }
    }

    #[test]
    fn a_pin_redirects_eviction_and_is_recorded() {
        // slice-033: three ~1900-token reads with step 1 pinned. At step 4's
        // assembly (pinned 1 + newest 3 ≈ 3800 fits; block 2 does not) the
        // unpinned step 2 block yields — 0.5.0 would have dropped step 1.
        let root = std::env::temp_dir().join(format!("orvena-pin-loop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let big = format!("{}\n", "x".repeat(75)).repeat(100);
        for f in ["big1.txt", "big2.txt", "big3.txt"] {
            std::fs::write(root.join(f), &big).unwrap();
        }

        let prompts = Arc::new(Mutex::new(Vec::new()));
        let provider = RecordingScript {
            script: vec![
                "<<<READ big1.txt\n>>>",
                "<<<PIN 1\n>>>\n<<<READ big2.txt\n>>>",
                "<<<READ big3.txt\n>>>",
                "<<<READ big3.txt\n>>>",
            ],
            calls: Mutex::new(0),
            prompts: Arc::clone(&prompts),
        };
        let mut cfg = config(vec![]);
        cfg.agent.max_steps = 4;
        let agent = super::super::Agent::with_provider(cfg, &root, Box::new(provider));
        let task = Task::new("do the thing", vec!["out.txt".into()]);
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let report =
            rt.block_on(run_loop_with(&agent, task, LoopOptions { ungoverned: true })).unwrap();
        let _ = std::fs::remove_dir_all(&root);

        let pins = report.pins.expect("the native loop claims the pin record on entry");
        assert_eq!(pins.count, 1);
        assert_eq!(pins.pinned_steps, vec![1]);
        assert_eq!(pins.refused, 0);
        let ev = report.evictions.expect("claimed on entry");
        assert_eq!(
            ev.evicted_steps,
            vec![2],
            "the pin held: eviction fell on the unpinned newer block"
        );
        let seen = prompts.lock().unwrap();
        assert!(
            seen[2].contains("PIN 1: evidence from step 1 pinned"),
            "the accept is spoken into the window"
        );
        assert!(seen[3].contains("step 1 (pinned) ──"), "the model can see its pin holding");
    }

    #[test]
    fn pin_refusals_are_counted_and_spoken() {
        // One ~2200-token read exceeds the 2048 pinned budget; pinning it is
        // refused, as is pinning a step that never produced evidence. Both
        // refusals must land in the next prompt — a silent refusal reads as
        // "pinned" to the model.
        let root = std::env::temp_dir().join(format!("orvena-pin-refuse-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let big = format!("{}\n", "x".repeat(87)).repeat(100);
        std::fs::write(root.join("big1.txt"), &big).unwrap();

        let prompts = Arc::new(Mutex::new(Vec::new()));
        let provider = RecordingScript {
            script: vec![
                "<<<READ big1.txt\n>>>",
                "<<<PIN 1\n>>>\n<<<PIN 7\n>>>",
                "<<<READ big1.txt\n>>>",
            ],
            calls: Mutex::new(0),
            prompts: Arc::clone(&prompts),
        };
        let agent = super::super::Agent::with_provider(config(vec![]), &root, Box::new(provider));
        let task = Task::new("do the thing", vec!["out.txt".into()]);
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let report =
            rt.block_on(run_loop_with(&agent, task, LoopOptions { ungoverned: true })).unwrap();
        let _ = std::fs::remove_dir_all(&root);

        let pins = report.pins.expect("claimed on entry");
        assert_eq!(pins.count, 0);
        assert_eq!(pins.refused, 2);
        assert!(pins.pinned_steps.is_empty());
        let seen = prompts.lock().unwrap();
        assert!(seen[2].contains("PIN 1 refused: pinned budget full"));
        assert!(seen[2].contains("PIN 7 refused: no evidence from step 7"));
    }

    #[test]
    fn a_search_into_an_evicted_path_counts_as_dropped_research() {
        // Same pressure shape as the dropped_reread test, but the recovery at
        // step 3 goes through SEARCH — the route the READ truncation note
        // recommends. dropped_reread must stay 0 (no re-READ happened) and
        // dropped_research must catch the go-back, or a SEARCH recovery is
        // indistinguishable from a value invented from memory.
        let root =
            std::env::temp_dir().join(format!("orvena-window-research-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let big = format!("{}\n", "x".repeat(80)).repeat(110);
        std::fs::write(root.join("big1.txt"), &big).unwrap();
        std::fs::write(root.join("big2.txt"), &big).unwrap();

        let provider = ScriptedReader {
            script: vec![
                "<<<READ big1.txt\n>>>",
                "<<<READ big2.txt\n>>>",
                "<<<SEARCH x\nbig1.txt\n>>>",
            ],
            calls: Mutex::new(0),
        };
        let mut cfg = config(vec![]);
        cfg.roles.roles[0].allowed_tools.push("grep.search".into());
        let agent = super::super::Agent::with_provider(cfg, &root, Box::new(provider));
        let task = Task::new("do the thing", vec!["out.txt".into()]);
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let report =
            rt.block_on(run_loop_with(&agent, task, LoopOptions { ungoverned: true })).unwrap();
        let _ = std::fs::remove_dir_all(&root);

        assert_eq!(report.dropped_reread, Some(0), "no re-READ was issued");
        assert_eq!(
            report.dropped_research,
            Some(1),
            "step 3's SEARCH hit the path whose evidence the window dropped"
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

    // ── host-guard refusals (#47): counted where the guard fires ───────────

    /// Runs `script` (action, then a no-action reply that ends the run) with
    /// the given tier and a developer role holding `tools`; the task's only
    /// declared write is `out.txt`.
    fn run_guard(
        tag: &str,
        script: Vec<&'static str>,
        tier: Tier,
        ungoverned: bool,
        tools: &[&str],
    ) -> RunReport {
        let root = std::env::temp_dir().join(format!("orvena-hg-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.txt"), "x\n").unwrap();
        let mut cfg = config(vec![]);
        cfg.agent.tier = tier;
        cfg.roles.roles[0].allowed_tools = tools.iter().map(|t| t.to_string()).collect();
        let provider = ScriptedReader { script, calls: Mutex::new(0) };
        let agent = super::super::Agent::with_provider(cfg, &root, Box::new(provider));
        let task = Task::new("do the thing", vec!["out.txt".into()]);
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let report = rt.block_on(run_loop_with(&agent, task, LoopOptions { ungoverned })).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        report
    }

    const ALL_TOOLS: &[&str] = &["fs.read", "fs.write", "grep.search"];

    #[test]
    fn off_mode_counts_a_write_escape() {
        let r = run_guard(
            "write",
            vec!["<<<WRITE ../x\nleak\n>>>", "done"],
            Tier::Light,
            true,
            ALL_TOOLS,
        );
        assert_eq!(r.host_guard_refusals, 1);
        assert!(r
            .blockers
            .iter()
            .any(|b| b == "scope violation: path '../x' escapes the project root"));
    }

    #[test]
    fn off_mode_counts_an_edit_escape() {
        let r = run_guard(
            "edit",
            vec!["<<<EDIT ../x\na\n===\nb\n>>>", "done"],
            Tier::Light,
            true,
            ALL_TOOLS,
        );
        assert_eq!(
            r.action_counts.as_ref().map(|c| c.edit),
            Some(1),
            "the script parsed as an EDIT"
        );
        assert_eq!(r.host_guard_refusals, 1);
    }

    #[test]
    fn off_mode_counts_a_read_escape() {
        let r = run_guard(
            "read",
            vec!["<<<READ /etc/passwd\n>>>", "done"],
            Tier::Light,
            true,
            ALL_TOOLS,
        );
        assert_eq!(r.host_guard_refusals, 1);
        assert!(r.scope_refusals.is_empty(), "READ never populated scope_refusals");
    }

    #[test]
    fn off_mode_counts_a_search_escape() {
        let r = run_guard(
            "search",
            vec!["<<<SEARCH foo\n../\n>>>", "done"],
            Tier::Light,
            true,
            ALL_TOOLS,
        );
        assert_eq!(r.host_guard_refusals, 1);
    }

    #[test]
    fn in_root_out_of_scope_write_is_not_a_host_guard_refusal() {
        let r = run_guard(
            "inroot",
            vec!["<<<WRITE other.txt\nx\n>>>", "done"],
            Tier::Engineering,
            false,
            ALL_TOOLS,
        );
        assert_eq!(r.scope_refusals, vec!["other.txt".to_string()], "a governed refusal happened");
        assert_eq!(r.host_guard_refusals, 0);
    }

    #[test]
    fn role_denied_escape_is_not_a_host_guard_refusal() {
        // The role gate runs before the guard: a role that may not search is
        // denied by the role, even for a path the guard would also refuse.
        let r = run_guard(
            "roledenied",
            vec!["<<<SEARCH foo\n../\n>>>", "done"],
            Tier::Light,
            true,
            &["fs.read"],
        );
        assert!(r.blockers.iter().any(|b| b.contains("is not allowed to use 'grep.search'")));
        assert_eq!(r.host_guard_refusals, 0);
    }

    #[test]
    fn symlink_escape_is_not_a_host_guard_refusal() {
        let root = std::env::temp_dir().join(format!("orvena-hg-symlink-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let outside =
            std::env::temp_dir().join(format!("orvena-hg-outside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("f.txt"), "secret\n").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
        let mut cfg = config(vec![]);
        cfg.roles.roles[0].allowed_tools = ALL_TOOLS.iter().map(|t| t.to_string()).collect();
        let provider = ScriptedReader {
            script: vec!["<<<READ link/f.txt\n>>>", "done"],
            calls: Mutex::new(0),
        };
        let agent = super::super::Agent::with_provider(cfg, &root, Box::new(provider));
        let task = Task::new("do the thing", vec!["out.txt".into()]);
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let r = rt.block_on(run_loop_with(&agent, task, LoopOptions { ungoverned: true })).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
        assert!(r.blockers.iter().any(|b| b.contains("resolves outside the project root")));
        assert_eq!(r.host_guard_refusals, 0);
    }

    // ── abstained (#46): scoring-only, the loop itself is unchanged ────────

    const NEVER_PASSES: &str = "false";

    fn never_passing_gate() -> Vec<Gate> {
        vec![Gate {
            name: "check".into(),
            condition: "out.txt is correct".into(),
            verify: Some(NEVER_PASSES.into()),
            gatekeeper: Default::default(),
            timeout_secs: None,
        }]
    }

    fn run_scripted(
        tag: &str,
        script: Vec<&'static str>,
        gates: Vec<Gate>,
        ungoverned: bool,
    ) -> RunReport {
        let root =
            std::env::temp_dir().join(format!("orvena-abstain-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let provider = ScriptedReader { script, calls: Mutex::new(0) };
        let agent = super::super::Agent::with_provider(config(gates), &root, Box::new(provider));
        let task = Task::new("do the thing", vec!["out.txt".into()]);
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let report = rt.block_on(run_loop_with(&agent, task, LoopOptions { ungoverned })).unwrap();
        let _ = std::fs::remove_dir_all(&root);
        report
    }

    #[test]
    fn governed_run_with_no_actions_is_abstained_not_exhausted() {
        let reply = "I will not write outside the allowed paths.";
        let r = run_scripted("gov-zero", vec![reply], never_passing_gate(), false);
        assert_eq!(r.exit, ExitReason::Abstained);
        assert!(!r.completed);
        assert_eq!(r.tool_calls, 0);
        assert_eq!(r.steps, r.max_steps, "the loop still ran to max_steps: control flow unchanged");
        assert_eq!(r.final_reply.as_deref(), Some(reply));
        assert!(r.blockers.iter().any(|b| b.contains("reached max_steps")));
        assert!(r.blockers.iter().any(|b| b.contains("no action")));
    }

    #[test]
    fn governed_run_that_acted_once_then_stalled_is_still_exhausted() {
        let r = run_scripted(
            "gov-acted",
            vec!["<<<WRITE out.txt\nwrong\n>>>", "I am done."],
            never_passing_gate(),
            false,
        );
        assert_eq!(r.exit, ExitReason::BudgetExhausted);
        assert!(!r.completed);
        assert_eq!(r.tool_calls, 1);
        assert_eq!(r.steps, r.max_steps);
        assert_eq!(r.final_reply.as_deref(), Some("I am done."));
    }

    // ── explicit termination (#54, #59) ────────────────────────────────────

    #[test]
    fn ungoverned_run_with_no_actions_is_reprompted_then_abstained() {
        // Prose with no block no longer ends the run (#54): it is re-prompted
        // to max_steps and only then labelled.
        let r = run_scripted("off-zero", vec!["Declining: out of scope."], vec![], true);
        assert_eq!(r.exit, ExitReason::Abstained);
        assert!(!r.completed, "nothing was done, so nothing was claimed done");
        assert_eq!(r.steps, r.max_steps);
        assert_eq!(r.format_errors, 0, "prose is not native markup");
        assert!(r.blockers.iter().any(|b| b.contains("without DONE or BLOCKED")));
    }

    #[test]
    fn ungoverned_run_that_wrote_then_went_silent_is_exhausted_not_done() {
        let r =
            run_scripted("off-acted", vec!["<<<WRITE out.txt\nx\n>>>", "All done."], vec![], true);
        assert_eq!(r.exit, ExitReason::BudgetExhausted, "prose \"done\" is not a DONE block");
        assert!(!r.completed);
        assert_eq!(r.tool_calls, 1);
        assert_eq!(r.steps, r.max_steps);
    }

    #[test]
    fn ungoverned_done_block_is_claimed_done_after_its_actions() {
        let reply = "<<<WRITE out.txt\nx\n>>>\n<<<DONE wrote it\n>>>";
        let r = run_scripted("off-done", vec![reply], vec![], true);
        assert_eq!(r.exit, ExitReason::ClaimedDone);
        assert!(r.completed);
        assert_eq!(r.steps, 1);
        assert_eq!(r.tool_calls, 1, "the WRITE in the same reply was applied before ending");
    }

    #[test]
    fn blocked_block_is_refused_in_both_postures() {
        let reply = "<<<BLOCKED the fix needs a file outside scope\n>>>";
        for (tag, ungoverned) in [("off-blocked", true), ("gov-blocked", false)] {
            let r = run_scripted(tag, vec![reply], never_passing_gate(), ungoverned);
            assert_eq!(r.exit, ExitReason::Refused, "{tag}");
            assert!(!r.completed, "{tag}: refusing is not completing");
            assert_eq!(r.steps, 1, "{tag}: BLOCKED ends the run now");
            assert!(
                r.blockers
                    .iter()
                    .any(|b| b == "agent stopped with BLOCKED: the fix needs a file outside scope"),
                "{tag}: {:?}",
                r.blockers
            );
        }
    }

    #[test]
    fn blocked_applies_the_actions_in_the_same_reply_first() {
        let reply = "<<<WRITE out.txt\nx\n>>>\n<<<BLOCKED cannot finish\n>>>";
        let r = run_scripted("off-blocked-acted", vec![reply], vec![], true);
        assert_eq!(r.exit, ExitReason::Refused);
        assert_eq!(r.tool_calls, 1, "the WRITE ran before the run ended");
        assert_eq!(r.steps, 1);
    }

    #[test]
    fn governed_done_with_passing_gates_is_gates_passed() {
        let gates = vec![Gate {
            name: "check".into(),
            condition: "out.txt exists".into(),
            verify: Some("test -f out.txt".into()),
            gatekeeper: Default::default(),
            timeout_secs: None,
        }];
        let reply = "<<<WRITE out.txt\nx\n>>>\n<<<DONE wrote it\n>>>";
        let r = run_scripted("gov-done-pass", vec![reply], gates, false);
        assert_eq!(r.exit, ExitReason::GatesPassed);
        assert!(r.completed);
        assert_eq!(r.steps, 1);
    }

    #[test]
    fn blocked_without_a_reason_still_says_so() {
        let r = run_scripted("off-blocked-bare", vec!["<<<BLOCKED\n>>>"], vec![], true);
        assert_eq!(r.exit, ExitReason::Refused);
        assert!(r.blockers.iter().any(|b| b.contains("(no reason given)")));
    }

    #[test]
    fn governed_done_does_not_bypass_failing_gates() {
        let r =
            run_scripted("gov-done", vec!["<<<DONE trust me\n>>>"], never_passing_gate(), false);
        assert_eq!(r.exit, ExitReason::BudgetExhausted, "a rejected claim is not an abstention");
        assert!(!r.completed, "DONE is a claim; gates still decide");
        assert_eq!(r.steps, r.max_steps);
        assert_eq!(r.tool_calls, 0);
        assert!(r.blockers.iter().any(|b| b.contains("claimed DONE on 3 of 3 steps")));
    }

    #[test]
    fn native_markup_is_counted_and_never_executed() {
        let reply = "<function_calls>\n<invoke name=\"write\">\n<parameter name=\"path\">out.txt</parameter>\n</invoke>\n</function_calls>";
        let r = run_scripted("off-native", vec![reply], vec![], true);
        assert_eq!(r.format_errors, r.max_steps, "one format error per re-prompted step");
        assert_eq!(r.tool_calls, 0, "native markup is detected, never parsed");
        assert_eq!(r.exit, ExitReason::Abstained);
    }

    #[test]
    fn no_action_feedback_reaches_the_next_prompt() {
        // The re-prompt must actually tell the model why nothing happened —
        // in both postures (D2).
        struct Recorder(std::sync::Arc<Mutex<Vec<String>>>, Mutex<usize>);
        #[async_trait]
        impl Provider for Recorder {
            fn id(&self) -> &str {
                "recorder"
            }
            async fn chat(
                &self,
                req: crate::provider::ChatRequest,
            ) -> crate::error::Result<ChatResponse> {
                let all: String = req.messages.iter().map(|m| m.content.clone()).collect();
                self.0.lock().unwrap().push(all);
                let mut n = self.1.lock().unwrap();
                *n += 1;
                let content = if *n == 1 { "<invoke name=\"x\">" } else { "thinking" };
                Ok(ChatResponse { content: content.into(), input_tokens: 0, output_tokens: 0 })
            }
        }
        for ungoverned in [true, false] {
            let root = std::env::temp_dir()
                .join(format!("orvena-feedback-{ungoverned}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(&root).unwrap();
            let seen = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
            let provider = Recorder(seen.clone(), Mutex::new(0));
            let gates = if ungoverned { vec![] } else { never_passing_gate() };
            let agent =
                super::super::Agent::with_provider(config(gates), &root, Box::new(provider));
            let task = Task::new("do the thing", vec!["out.txt".into()]);
            let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
            let r = rt.block_on(run_loop_with(&agent, task, LoopOptions { ungoverned })).unwrap();
            let _ = std::fs::remove_dir_all(&root);
            assert_eq!(r.format_errors, 1, "ungoverned={ungoverned}");
            let seen = seen.lock().unwrap();
            assert!(!seen[0].contains("FORMAT ERROR") && !seen[0].contains("NO ACTION"));
            assert!(!seen[1].contains("NO ACTION"), "native markup gets its own message");
            assert!(seen[1].contains("FORMAT ERROR"), "step 2 is told about the native markup");
            assert!(seen[2].contains("NO ACTION"), "step 3 is told nothing happened");
        }
    }

    #[test]
    fn final_reply_truncates_on_a_char_boundary_without_panicking() {
        // 3-byte chars: a byte-index cut at 4000 would land mid-char.
        let long = "界".repeat(4500);
        let cut = crate::metrics::truncate_final_reply(&long);
        assert!(cut.ends_with("…[truncated]"));
        assert_eq!(cut.chars().filter(|c| *c == '界').count(), 4000);
        let exact = "界".repeat(4000);
        assert_eq!(crate::metrics::truncate_final_reply(&exact), exact, "no cut, no marker");
    }
}
