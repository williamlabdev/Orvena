//! The shapes a benchmark result is published in, and their serialization.
//!
//! Deliberately separate from the aggregation that fills them in
//! (`super::aggregate`) and from the runner that produces the raw results
//! (`super::runner`): what a number *means* is defined by the field docs here,
//! and those are the contract for anything reading a committed report JSON.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::metrics::{ActionCounts, ExitReason, TokenAccounting};
use crate::Result;

/// The outcome of one task in the set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskResult {
    pub id: String,
    /// The in-loop outcome. Governed: all gates passed. Ungoverned (`off`): the
    /// model's own claim of done — which is exactly what M2 interrogates.
    pub completed: bool,
    /// Ground truth: the task's `verify` command, run by the harness *after*
    /// the loop finished, independent of any in-loop gate. `completed` without
    /// `verified` is a false done.
    #[serde(default)]
    pub verified: bool,
    /// True when the task was not run because a required toolchain was absent.
    /// A skipped task is excluded from the completion-rate denominator.
    pub skipped: bool,
    pub skip_reason: Option<String>,
    pub steps: u32,
    pub tool_calls: u32,
    /// Which kinds of action those calls were, when the loop could attribute
    /// them (native only — see [`crate::metrics::ActionCounts`]). This is what
    /// makes "did it search or did it read its way there?" a number instead of
    /// a transcript-reading exercise.
    #[serde(default)]
    pub action_counts: Option<crate::metrics::ActionCounts>,
    /// Hits per SEARCH, in order (see [`crate::metrics::RunReport::search_hits`]).
    /// Carried up from the bundle so a summary can separate a loop that searched
    /// for the wrong thing from one that searched right and never acted.
    #[serde(default)]
    pub search_hits: Vec<Option<u32>>,
    pub input_tokens: u32,
    pub output_tokens: u32,
    /// Path to the task's evidence bundle (`None` if skipped, or if the run
    /// errored before one could be written).
    pub evidence_path: Option<PathBuf>,
    pub blockers: Vec<String>,
    /// The independent oracle's verdict (M1): out-of-scope changes + escape
    /// probes found. Empty on a contained run.
    #[serde(default)]
    pub violations: Vec<String>,
    /// Enforcement refusals of paths the contract allowed (false blocks).
    #[serde(default)]
    pub false_blocks: Vec<String>,
    /// True when the oracle ran and found no violations. Meaningless when
    /// `oracle_error` is set — containment aggregates exclude those runs.
    #[serde(default)]
    pub contained: bool,
    /// Why the oracle could not judge this run (e.g. git unavailable). Never
    /// silently counted as contained.
    #[serde(default)]
    pub oracle_error: Option<String>,
    /// M3: the run left an evidence bundle that validates against the frozen
    /// v1 schema. A ran task with no bundle, or an invalid one, is `false`.
    #[serde(default)]
    pub evidence_valid: bool,
    /// Set when the run died on a provider failure (outage, bad key, exhausted
    /// quota). Such a run is **excluded from every metric denominator**: it
    /// measures the API, not the agent, and folding it in would let an outage
    /// masquerade as a result. It stays in `results` so the exclusion is
    /// auditable rather than invisible.
    #[serde(default)]
    pub provider_error: Option<String>,
    /// Where this run's token counts came from. Orvena observes its own model
    /// calls; a wrapped external agent makes its own, so its counts are relayed
    /// or missing. Carried up so a cost comparison never mixes a measured number
    /// with a claimed one.
    #[serde(default)]
    pub token_accounting: TokenAccounting,
    /// Why the run's loop stopped (carried verbatim from the bundle). The one
    /// that matters for M4: `budget_exhausted` marks a right-censored run whose
    /// `steps` is a budget artifact, not a convergence measurement.
    #[serde(default)]
    pub exit: ExitReason,
    /// Evidence-window eviction telemetry, carried verbatim from the bundle
    /// (see [`crate::metrics::RunReport::evictions`]). `None` = unattributable
    /// (wrapped agent, or a pre-field report), never "no evictions".
    #[serde(default)]
    pub evictions: Option<crate::metrics::Evictions>,
    /// READs of already-evicted paths (see
    /// [`crate::metrics::RunReport::dropped_reread`]) — the ordering-death vs
    /// re-read-death divide. Same `None` contract as `evictions`.
    #[serde(default)]
    pub dropped_reread: Option<u32>,
    /// SEARCHes that hit an already-evicted path (see
    /// [`crate::metrics::RunReport::dropped_research`]) — re-acquisition by
    /// SEARCH instead of READ; without it a SEARCH recovery is scored like an
    /// invented value. Same `None` contract as `evictions`.
    #[serde(default)]
    pub dropped_research: Option<u32>,
    /// Peak window occupancy in tokens (see
    /// [`crate::metrics::RunReport::window_peak_tokens`]) — whether the task's
    /// pressure coefficient actually reached the evidence budget. Same `None`
    /// contract as `evictions`.
    #[serde(default)]
    pub window_peak_tokens: Option<u32>,
}

/// The identity of a run, as opposed to its configuration.
///
/// Three invocations of one probe on 0805–06 agreed on every field the report
/// then carried — provider, model, endpoint, governance, agent — and one of
/// them still read 3/3 on a task the other two read 0/12 on. The record could
/// not say whether they had measured the same thing, and the scratch dir that
/// might have answered it was gone. Everything here exists to make that
/// question answerable next time (slice-029).
///
/// Provenance is **identity, never a reading**: nothing here enters the
/// numerator or denominator of any rate. Its only job is to let a reader decide
/// whether two reports may be compared at all.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RunProvenance {
    #[serde(flatten)]
    pub backend: crate::provider::ProviderProvenance,
    /// The sampling this run asked for. `None` means **inherited** — the
    /// backend's own defaults applied, so the numbers in this report are not
    /// repo-controlled and a later run under a changed Modelfile would differ
    /// with nothing in the record to show it.
    pub sampling: Option<crate::config::agent::Sampling>,
}

/// The aggregate benchmark result. Every rate divides by `measured`, i.e.
/// `task_count - skipped - provider_errors`: a task whose toolchain was absent
/// was never attempted, and a run the provider killed never reached the model.
/// Neither is evidence about the agent, so neither is counted against it — and
/// both counts are reported so the exclusions are visible.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BenchReport {
    pub provider: String,
    pub model: String,
    /// Endpoint origin (`scheme://host[:port]`) when the run used an explicit
    /// `base_url`. `provider` alone stopped identifying the backend once
    /// `openai_compat` made the kind endpoint-agnostic — a local llama.cpp and
    /// a hosted aggregator serving the same open-weight model are otherwise
    /// indistinguishable in a published report. Credentials are stripped
    /// (`ProviderSelection::endpoint_origin`). Additive: keeps the report
    /// readable by consumers that predate the field.
    #[serde(default)]
    pub endpoint: Option<String>,
    pub run_id: String,
    /// What actually ran, as opposed to what was configured — backend version,
    /// weight digest, effective context, and the sampling this run requested.
    /// `None` on reports written before slice-029, which is the honest reading:
    /// those runs are not known to be comparable to anything, including each
    /// other. See [`RunProvenance`].
    #[serde(default)]
    pub provenance: Option<RunProvenance>,
    /// Governance posture this report was measured under.
    #[serde(default = "default_governance")]
    pub governance: String,
    /// Which agent drove the runs: `native` (Orvena's own bounded loop) or a
    /// wrapped external agent and its version (`crate::adapter`). Reports
    /// written before adapters existed read back as `native` (they were).
    #[serde(default = "default_agent")]
    pub agent: String,
    pub task_count: u32,
    pub passed: u32,
    pub skipped: u32,
    /// Runs that died on a provider failure. Excluded from every rate below —
    /// the denominator is `measured = task_count - skipped - provider_errors`.
    /// Reported so a partly-failed benchmark is visible without opening the
    /// per-task results and counting blockers by hand.
    #[serde(default)]
    pub provider_errors: u32,
    pub completion_rate: f32,
    /// Tasks whose external `verify` (ground truth) passed, and the rate over ran.
    #[serde(default)]
    pub verified: u32,
    #[serde(default)]
    pub verified_rate: f32,
    /// Claimed done but ground truth failed (M2). Rate is over *claims*
    /// (`passed`), not over ran — "of the runs that said done, how many lied".
    #[serde(default)]
    pub false_done: u32,
    #[serde(default)]
    pub false_done_rate: f32,
    /// M1: runs the oracle judged contained, over runs the oracle could judge
    /// (`ran - oracle_errors`). Oracle failures are counted, never assumed.
    #[serde(default)]
    pub contained: u32,
    #[serde(default)]
    pub containment_rate: f32,
    #[serde(default)]
    pub false_blocks: u32,
    #[serde(default)]
    pub oracle_errors: u32,
    /// M3: ran tasks whose bundle exists and validates against schema v1.
    #[serde(default)]
    pub evidence_valid: u32,
    #[serde(default)]
    pub evidence_valid_rate: f32,
    pub results: Vec<TaskResult>,
}

fn default_governance() -> String {
    "light".into()
}

pub(super) fn default_agent() -> String {
    "native".into()
}

/// How one run's searching went, as a class rather than a rate.
///
/// The classes exist because the rates kept collapsing into each other:
/// `search_yield_rate` turned out to be the pass rate in another notation (in
/// 30 probe runs, every run with a hit solved and every run without one failed
/// — slice-026), so the usable signal is which *kind* of search failure a run
/// died in, not what fraction of searches hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchOutcome {
    /// At least one SEARCH came back with a hit.
    Hit,
    /// Searches looked at files and every recorded outcome was zero hits —
    /// the loop searched for the wrong thing.
    Miss,
    /// Searches were emitted but not one ever looked at a file (every outcome
    /// errored at the tool boundary). This is the death slice-027 fixed for
    /// globs; keeping it distinct from `Miss` is what would make a recurrence
    /// visible as a tool problem instead of a model problem.
    Blocked,
    /// The loop never searched.
    NoSearch,
    /// The run's actions were not Orvena's to attribute (wrapped agent).
    /// Never folded into `NoSearch`: a gap in the record is not a finding
    /// about behaviour.
    Unattributable,
}

impl SearchOutcome {
    /// Classify one run from its action counts and per-search hits.
    ///
    /// `counts = None` means unattributable, **not** "zero searches" — the
    /// same contract as [`TaskResult::action_counts`]. Errored searches record
    /// `None` in `hits` and are never read as misses; a run whose every search
    /// errored is `Blocked`, because nothing was ever actually searched.
    pub fn classify(counts: Option<&ActionCounts>, hits: &[Option<u32>]) -> Self {
        let Some(counts) = counts else {
            return Self::Unattributable;
        };
        if hits.iter().flatten().any(|h| *h > 0) {
            Self::Hit
        } else if hits.iter().any(|h| h.is_some()) {
            Self::Miss
        } else if counts.search > 0 {
            Self::Blocked
        } else {
            Self::NoSearch
        }
    }
}

/// One measured run's line in a task's death table — how it ended, what it
/// spent, and which actions it spent that on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeathRow {
    /// Index into [`RepeatedReport::runs`], so a row traces back to its full
    /// [`TaskResult`] and evidence bundle.
    pub rep: u32,
    /// In-loop outcome — the same definition [`TaskPassRate::solved`] counts.
    pub solved: bool,
    /// Ground truth (the external `verify`).
    pub verified: bool,
    pub exit: ExitReason,
    pub steps: u32,
    pub total_tokens: u32,
    /// `None` = unattributable, never "all zero" — same contract as
    /// [`TaskResult::action_counts`].
    pub action_counts: Option<ActionCounts>,
    /// Hits per SEARCH in order, verbatim from the run (`None` = errored).
    pub search_hits: Vec<Option<u32>>,
    pub search: SearchOutcome,
    /// Window telemetry (SLICE-032 instrument), verbatim from the run — the
    /// v3 death classification reads these, not `action_counts`, to separate
    /// an ordering death from a re-read death. `None` = unattributable, same
    /// contract as `action_counts`.
    #[serde(default)]
    pub evictions: Option<crate::metrics::Evictions>,
    #[serde(default)]
    pub dropped_reread: Option<u32>,
    #[serde(default)]
    pub dropped_research: Option<u32>,
    #[serde(default)]
    pub window_peak_tokens: Option<u32>,
}

impl DeathRow {
    /// The row for one measured run. `rep` is its index in
    /// [`RepeatedReport::runs`].
    pub fn of(rep: u32, r: &TaskResult) -> Self {
        Self {
            rep,
            solved: r.completed,
            verified: r.verified,
            exit: r.exit,
            steps: r.steps,
            total_tokens: r.input_tokens + r.output_tokens,
            action_counts: r.action_counts,
            search_hits: r.search_hits.clone(),
            search: SearchOutcome::classify(r.action_counts.as_ref(), &r.search_hits),
            evictions: r.evictions.clone(),
            dropped_reread: r.dropped_reread,
            dropped_research: r.dropped_research,
            window_peak_tokens: r.window_peak_tokens,
        }
    }
}

/// Solved/failed counts for one cell of [`SearchSolveTable`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolveSplit {
    pub solved: u32,
    pub failed: u32,
}

/// How run-level search outcomes pair with solving, per task.
///
/// In 30 probe runs the correspondence was exact — hit ⇔ solved, without
/// exception (slice-026) — which is why `search_yield_rate` cannot serve as a
/// second reading. This table is what makes that correspondence, or its
/// breakdown (which would be a new finding: a loop that found the answer and
/// did not act, a shape never yet observed), checkable per task instead of by
/// reading transcripts.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchSolveTable {
    pub hit: SolveSplit,
    pub miss: SolveSplit,
    pub blocked: SolveSplit,
    pub no_search: SolveSplit,
    pub unattributable: SolveSplit,
}

impl SearchSolveTable {
    pub fn tally(rows: &[DeathRow]) -> Self {
        let mut table = Self::default();
        for row in rows {
            let cell = match row.search {
                SearchOutcome::Hit => &mut table.hit,
                SearchOutcome::Miss => &mut table.miss,
                SearchOutcome::Blocked => &mut table.blocked,
                SearchOutcome::NoSearch => &mut table.no_search,
                SearchOutcome::Unattributable => &mut table.unattributable,
            };
            if row.solved {
                cell.solved += 1;
            } else {
                cell.failed += 1;
            }
        }
        table
    }
}

/// Per-task pass rate across repeated runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskPassRate {
    pub id: String,
    pub runs: u32,
    pub solved: u32,
    pub skipped: bool,
    pub pass_rate: f32,
    /// Fraction of this task's measured runs that ended `budget_exhausted`.
    /// Per task, unlike [`RepeatedReport::budget_exhaustion_rate`]: a floor
    /// model dies differently on different tasks, and a set-level mean hides
    /// exactly the per-task shape calibration reads (slice-026).
    #[serde(default)]
    pub exhaustion_rate: f32,
    /// The death table (slice-026): one row per measured run, in repeat order.
    /// What a calibration run publishes about a task is not a percentage but
    /// how its runs died — the pass rate needs ~96 runs to stabilize, while
    /// the death classification was identical across three invocations whose
    /// pass rates differed by 59 points. Empty on reports written before the
    /// field, and on skipped tasks.
    #[serde(default)]
    pub deaths: Vec<DeathRow>,
    /// See [`SearchSolveTable`]. All-zero on pre-field reports — telling that
    /// apart from "measured, nothing searched" is what `deaths` being empty
    /// or not is for.
    #[serde(default)]
    pub search_vs_solved: SearchSolveTable,
}

/// Aggregate of `repeat` benchmark runs — a de-noised completion rate that
/// tolerates a stochastic model, unlike a single pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepeatedReport {
    pub provider: String,
    pub model: String,
    /// Endpoint origin (`scheme://host[:port]`) when the run used an explicit
    /// `base_url`. `provider` alone stopped identifying the backend once
    /// `openai_compat` made the kind endpoint-agnostic — a local llama.cpp and
    /// a hosted aggregator serving the same open-weight model are otherwise
    /// indistinguishable in a published report. Credentials are stripped
    /// (`ProviderSelection::endpoint_origin`). Additive: keeps the report
    /// readable by consumers that predate the field.
    #[serde(default)]
    pub endpoint: Option<String>,
    pub run_id: String,
    /// What actually ran, as opposed to what was configured — backend version,
    /// weight digest, effective context, and the sampling this run requested.
    /// `None` on reports written before slice-029, which is the honest reading:
    /// those runs are not known to be comparable to anything, including each
    /// other. See [`RunProvenance`].
    #[serde(default)]
    pub provenance: Option<RunProvenance>,
    /// Governance posture this report was measured under.
    #[serde(default = "default_governance")]
    pub governance: String,
    /// Which agent drove the runs (`native`, or a wrapped agent + version).
    #[serde(default = "default_agent")]
    pub agent: String,
    pub repeat: u32,
    pub task_count: u32,
    pub ran: u32,
    pub skipped: u32,
    /// Task-runs (not tasks) that died on a provider failure, across every
    /// repeat. Excluded from every rate below; reported so a partly-failed
    /// matrix is visible at a glance.
    #[serde(default)]
    pub provider_errors: u32,
    /// Mean of per-task pass rates over ran tasks — the expected single-pass
    /// completion rate, averaged over `repeat` attempts to cut model noise.
    pub mean_pass_rate: f32,
    /// Tasks solved in at least one run (an optimistic pass@k upper bound).
    pub solved_any: u32,
    /// Ground truth across all task-runs: externally-verified rate, and the
    /// false-done rate over claims (M2).
    #[serde(default)]
    pub verified_rate: f32,
    #[serde(default)]
    pub false_done_rate: f32,
    /// M1 across all judged task-runs, plus total false blocks and how many
    /// runs the oracle could not judge.
    #[serde(default)]
    pub containment_rate: f32,
    #[serde(default)]
    pub false_blocks: u32,
    #[serde(default)]
    pub oracle_errors: u32,
    /// M3 across all ran task-runs: schema-valid bundle rate.
    #[serde(default)]
    pub evidence_valid_rate: f32,
    /// Cost per ran task-run (M4): mean steps and mean total tokens.
    #[serde(default)]
    pub mean_steps: f32,
    /// The fraction of measured runs (same denominator as `mean_steps`) that
    /// ended on `budget_exhausted` — i.e. how much of `mean_steps` is
    /// right-censored by the step budget rather than measured convergence. A
    /// posture that burns its whole budget has a `mean_steps` that tracks
    /// `max_steps`, not behavior; this rate is what makes that legible.
    #[serde(default)]
    pub budget_exhaustion_rate: f32,
    #[serde(default)]
    pub mean_total_tokens: f32,
    /// The fraction of *attributable* runs that emitted at least one SEARCH.
    /// Denominator is runs with `action_counts` present, not all ran runs: a
    /// wrapped agent's actions are not Orvena's to attribute, and folding those
    /// in as "never searched" would read as a finding about the loop when it is
    /// only a gap in the record. `None` when nothing was attributable.
    #[serde(default)]
    pub search_use_rate: Option<f32>,
    /// Of the SEARCHes that ran, the fraction that came back with at least one
    /// hit. `search_use_rate` says the loop looked; this says looking worked.
    /// The pair is what separates the two failures they used to be confused for:
    /// a low yield means the loop searched for the wrong thing, a high yield with
    /// a low pass rate means it found the answer and did not act on it.
    /// Denominator is searches with a recorded outcome — errored ones (`null`)
    /// are excluded, not counted as misses. `None` when nothing searched.
    #[serde(default)]
    pub search_yield_rate: Option<f32>,
    /// Provenance of `mean_total_tokens` — the weakest accounting among the
    /// measured runs. `unavailable` means the mean is **0 because nobody
    /// counted**, not because the runs were free; a reader must be able to tell
    /// those apart without knowing which agent produced the report.
    #[serde(default)]
    pub token_accounting: TokenAccounting,
    pub tasks: Vec<TaskPassRate>,
    /// The underlying per-repeat reports, for full auditability.
    pub runs: Vec<BenchReport>,
}

/// The governance-differential matrix (the number only Orvena can publish):
/// the same task set × the same provider, once per governance mode, plus the
/// baseline-vs-governed differential when `off` and a governed mode are both
/// present. Modes are compared on *identical prompts* — the baseline carries
/// the same writable lists, only enforcement differs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatrixReport {
    pub provider: String,
    pub model: String,
    /// Endpoint origin (`scheme://host[:port]`) when the run used an explicit
    /// `base_url`. `provider` alone stopped identifying the backend once
    /// `openai_compat` made the kind endpoint-agnostic — a local llama.cpp and
    /// a hosted aggregator serving the same open-weight model are otherwise
    /// indistinguishable in a published report. Credentials are stripped
    /// (`ProviderSelection::endpoint_origin`). Additive: keeps the report
    /// readable by consumers that predate the field.
    #[serde(default)]
    pub endpoint: Option<String>,
    pub run_id: String,
    /// Backend identity for the whole matrix. The differential's entire claim is
    /// that only *enforcement* differed between postures — a claim that needs the
    /// backend to have been the same thing throughout, which is exactly what was
    /// unrecorded until slice-029. See [`RunProvenance`].
    #[serde(default)]
    pub provenance: Option<RunProvenance>,
    /// Which agent drove every posture in this matrix (`native`, or a wrapped
    /// agent + version). The differential compares postures, never agents — one
    /// matrix, one agent.
    #[serde(default = "default_agent")]
    pub agent: String,
    pub modes: Vec<RepeatedReport>,
    /// Present when both an `off` baseline and a governed mode ran **and** the
    /// run was healthy enough to compare. `None` with `differential_suppressed`
    /// set means the postures ran but the result was not fit to publish.
    pub differential: Option<Differential>,
    /// Why no differential is reported, when a comparison was otherwise
    /// possible. A weak number gets published with caveats; a number computed
    /// from a mostly-dead run is not weak, it is invalid — so it is withheld
    /// with the reason attached rather than printed with a footnote.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub differential_suppressed: Option<String>,
}

/// Baseline vs governed, on containment, ground truth, and cost.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Differential {
    pub baseline: String,
    pub governed: String,
    /// M1: fraction of judged runs whose every change was declared — per
    /// posture, from the independent oracle.
    #[serde(default)]
    pub baseline_containment_rate: f32,
    #[serde(default)]
    pub governed_containment_rate: f32,
    /// M2: of the runs that claimed done, the fraction whose external verify
    /// failed — per posture.
    pub baseline_false_done_rate: f32,
    pub governed_false_done_rate: f32,
    /// Ground-truth solve rate per posture.
    pub baseline_verified_rate: f32,
    pub governed_verified_rate: f32,
    /// M4: governed cost / baseline cost (>1 = governance overhead). 0 when the
    /// baseline cost is 0 (nothing meaningful to divide).
    pub overhead_steps_ratio: f32,
    /// How much of each posture's `mean_steps` is right-censored: the fraction
    /// of measured runs that ended on `budget_exhausted`. The steps ratio above
    /// is a budget artifact to exactly this extent — a baseline that burns its
    /// whole budget puts `max_steps` in the denominator, and enlarging the
    /// budget would then "improve" the ratio with zero behavioral change. The
    /// ratio is published with its censoring visible, never alone.
    #[serde(default)]
    pub baseline_budget_exhaustion_rate: f32,
    #[serde(default)]
    pub governed_budget_exhaustion_rate: f32,
    /// `None` when either posture's tokens were never accounted for — a wrapped
    /// external agent makes its own model calls, and a ratio of two unknowns is
    /// not a cheaper number, it is not a number. Steps stay observable either
    /// way (Orvena spawns the invocations).
    pub overhead_tokens_ratio: Option<f32>,
    /// Provenance of the token figures behind the ratio, weakest of the two
    /// postures — so a relayed cost claim is never read as a measured one.
    #[serde(default)]
    pub token_accounting: TokenAccounting,
}

/// Serialize a [`MatrixReport`] as pretty JSON to `path`, creating parents.
pub fn write_matrix_report(report: &MatrixReport, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(report)?)?;
    Ok(())
}

/// Serialize a [`RepeatedReport`] as pretty JSON to `path`, creating parents.
pub fn write_repeated_report(report: &RepeatedReport, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(report)?)?;
    Ok(())
}

/// Serialize a [`BenchReport`] as pretty JSON to `path`, creating parents.
pub fn write_report(report: &BenchReport, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string_pretty(report)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(search: u32) -> ActionCounts {
        ActionCounts { search, ..Default::default() }
    }

    #[test]
    fn a_run_with_any_hit_is_hit_even_among_errors_and_misses() {
        let hits = vec![None, Some(0), Some(3)];
        assert_eq!(SearchOutcome::classify(Some(&counts(3)), &hits), SearchOutcome::Hit);
    }

    #[test]
    fn all_zero_outcomes_are_a_miss_not_a_block() {
        // It looked — in the wrong places. A model problem, not a tool problem.
        let hits = vec![Some(0), Some(0)];
        assert_eq!(SearchOutcome::classify(Some(&counts(2)), &hits), SearchOutcome::Miss);
    }

    #[test]
    fn every_search_erroring_is_blocked_not_a_miss() {
        // The slice-027 death: searches emitted, none ever looked at a file.
        // Reading it as Miss would blame the model for a tool boundary.
        let hits = vec![None, None, None];
        assert_eq!(SearchOutcome::classify(Some(&counts(3)), &hits), SearchOutcome::Blocked);
    }

    #[test]
    fn never_searching_and_unattributable_are_different_classes() {
        assert_eq!(SearchOutcome::classify(Some(&counts(0)), &[]), SearchOutcome::NoSearch);
        // None counts = a wrapped agent's run. "No record" is not "no search".
        assert_eq!(SearchOutcome::classify(None, &[]), SearchOutcome::Unattributable);
    }

    #[test]
    fn the_tally_pairs_each_outcome_with_solving() {
        let row = |solved: bool, search: SearchOutcome| DeathRow {
            rep: 0,
            solved,
            verified: solved,
            exit: ExitReason::GatesPassed,
            steps: 2,
            total_tokens: 100,
            action_counts: Some(counts(1)),
            search_hits: Vec::new(),
            search,
            evictions: None,
            dropped_reread: None,
            dropped_research: None,
            window_peak_tokens: None,
        };
        let table = SearchSolveTable::tally(&[
            row(true, SearchOutcome::Hit),
            row(false, SearchOutcome::Miss),
            row(false, SearchOutcome::Miss),
            row(false, SearchOutcome::Blocked),
        ]);
        assert_eq!(table.hit, SolveSplit { solved: 1, failed: 0 });
        assert_eq!(table.miss, SolveSplit { solved: 0, failed: 2 });
        assert_eq!(table.blocked, SolveSplit { solved: 0, failed: 1 });
        assert_eq!(table.no_search, SolveSplit::default());
    }

    #[test]
    fn a_pre_death_table_report_still_reads_back() {
        // The committed reports this field postdates must stay readable, and
        // must read as "not recorded" (empty deaths), never as measured zeros.
        let old = r#"{"id":"t","runs":3,"solved":1,"skipped":false,"pass_rate":0.33}"#;
        let t: TaskPassRate = serde_json::from_str(old).unwrap();
        assert!(t.deaths.is_empty());
        assert_eq!(t.exhaustion_rate, 0.0);
        assert_eq!(t.search_vs_solved, SearchSolveTable::default());
    }
}
