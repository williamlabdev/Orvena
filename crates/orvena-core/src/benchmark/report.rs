//! The shapes a benchmark result is published in, and their serialization.
//!
//! Deliberately separate from the aggregation that fills them in
//! (`super::aggregate`) and from the runner that produces the raw results
//! (`super::runner`): what a number *means* is defined by the field docs here,
//! and those are the contract for anything reading a committed report JSON.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::metrics::{ExitReason, TokenAccounting};
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

/// Per-task pass rate across repeated runs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskPassRate {
    pub id: String,
    pub runs: u32,
    pub solved: u32,
    pub skipped: bool,
    pub pass_rate: f32,
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
