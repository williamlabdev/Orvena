//! L1 regression metrics. Every run produces a [`RunReport`] with frozen fields
//! a maintainer can diff across daily changes: did it complete, how many tokens,
//! how many steps, how many tool calls. This is a regression test against
//! ourselves — not an external benchmark. (Evidence & Done pillar.)

pub mod baseline;
pub mod evidence;

pub use baseline::{BaselineRecord, GoldenTask};

use serde::{Deserialize, Serialize};

/// The evidence-bundle schema identifier (v1, frozen — see
/// `schemas/evidence.v1.json`). Compatibility policy: additive fields keep v1
/// (consumers must ignore unknown fields); a removal or type change bumps to
/// v2 under a new identifier.
pub const EVIDENCE_SCHEMA_V1: &str = "orvena-evidence-v1";

fn evidence_schema_v1() -> String {
    EVIDENCE_SCHEMA_V1.into()
}

/// Per-action breakdown of a native loop's tool calls, one counter per action
/// kind the model can emit (see [`crate::agent::step::Action`]). These count
/// what the model *emitted*, not what succeeded — same basis as `tool_calls`,
/// so a scope-refused write still counts as a write it tried. Additive: a
/// bundle without it reads back as [`RunReport::action_counts`] = `None`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionCounts {
    #[serde(default)]
    pub write: u32,
    #[serde(default)]
    pub edit: u32,
    #[serde(default)]
    pub read: u32,
    #[serde(default)]
    pub search: u32,
    #[serde(default)]
    pub run: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunReport {
    /// Self-describing schema identifier — an artifact of record must say what
    /// it is without its producer on hand. Bundles written before the field
    /// existed read back as v1 (they are).
    #[serde(default = "evidence_schema_v1")]
    pub schema: String,
    /// What produced this run: provider kind, model id, and — when the config
    /// set an explicit `base_url` — the endpoint origin. A bundle is meant to
    /// "say what it is without its producer on hand", and `provider` alone
    /// stopped identifying the backend once `openai_compat` made the kind
    /// endpoint-agnostic. Credentials never appear here (see
    /// `ProviderSelection::endpoint_origin`). Additive fields — bundles written
    /// before they existed still read as v1.
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub endpoint: Option<String>,
    pub task: String,
    /// True when all gates passed (the run reached "done").
    pub completed: bool,
    pub steps: u32,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub tool_calls: u32,
    /// Which *kinds* of action those tool calls were. `tool_calls` alone cannot
    /// answer the question the ruler keeps asking — "did the loop ever search,
    /// or did it read its way to the answer?" — and slice-024 had to be judged
    /// by hand-reading transcripts for exactly that reason.
    ///
    /// `None` means **not attributable**, which is not the same as all-zero: a
    /// wrapped third-party agent runs its own loop inside one invocation (its
    /// `tool_calls` counts invocations, not actions), and bundles written before
    /// this field existed never recorded it. Consumers must exclude `None` from
    /// their denominators rather than counting it as "never searched".
    #[serde(default)]
    pub action_counts: Option<ActionCounts>,
    pub gate_outcomes: Vec<GateRecord>,
    pub blockers: Vec<String>,
    /// Write paths the enforcement layer refused (scope violations), as
    /// structured data — `blockers` keeps the human-readable message. Lets the
    /// benchmark's independent oracle cross-check enforcement for false blocks
    /// without parsing prose.
    #[serde(default)]
    pub scope_refusals: Vec<String>,
    /// Whether this run's spawned children were confined by an OS sandbox
    /// (ADR-003) — so the bundle can distinguish enforced containment from mere
    /// intention. Bundles written before the field existed read back as
    /// `disabled` (they were).
    #[serde(default)]
    pub sandbox: crate::exec::sandbox::SandboxStatus,
    /// Set when the run ended because the *provider* failed — an outage, a bad
    /// key, an exhausted quota — rather than because of anything the agent or
    /// the enforcement layer did. `blockers` keeps the human-readable message;
    /// this is the structured flag, so a consumer never has to pattern-match
    /// prose to tell "the model misbehaved" from "the model never answered".
    /// The benchmark uses it to exclude such runs from its denominators: a run
    /// that never reached the model is evidence about the API, not about
    /// governance. Additive optional field — bundles written before it existed
    /// read back as `None` (they had no such flag).
    #[serde(default)]
    pub provider_error: Option<String>,
    /// Which agent produced this run: `None` = Orvena's own bounded loop, `Some`
    /// = a wrapped third-party agent and its version (e.g. `"aider 0.86.2"`, see
    /// [`crate::adapter`]). An auditor must be able to tell whose loop the
    /// evidence describes; the enforcement record below (`sandbox`,
    /// `scope_refusals`) is Orvena's either way. Additive optional field —
    /// bundles written before it existed read back as `None` (they were native).
    #[serde(default)]
    pub agent: Option<String>,
    /// Where the token counts above came from. The native loop *observes* them
    /// (it makes the model calls). A wrapped external agent makes its own calls,
    /// so Orvena can only relay what the agent prints — or nothing at all. A
    /// consumer comparing costs must know which, so this is recorded rather than
    /// left to be inferred from a suspicious zero.
    #[serde(default)]
    pub token_accounting: TokenAccounting,
    /// The step budget this run was given. `steps` alone is unreadable without
    /// it: 3 of a possible 4 is a burned budget, 3 of a possible 8 is
    /// convergence. `0` means the bundle predates the field (unrecorded) —
    /// never "no budget". Additive field — stays v1.
    #[serde(default)]
    pub max_steps: u32,
    /// Why the loop stopped, as structured data — `blockers` keeps the
    /// human-readable message. Same discipline as `provider_error`: a consumer
    /// separating "converged on its own" from "was cut off by the budget"
    /// must never have to pattern-match prose. Additive field — bundles
    /// written before it existed read back as `unrecorded`.
    #[serde(default)]
    pub exit: ExitReason,
}

/// Why a run's loop stopped — see [`RunReport::exit`]. An observation field:
/// it must never feed back into loop behavior (measurement/policy separation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExitReason {
    /// Governed: every gate passed (native and adapter legs alike).
    GatesPassed,
    /// Ungoverned: the run ended on the agent's own, unverified claim of done
    /// (native: zero actions emitted; adapter: the single invocation exited 0).
    ClaimedDone,
    /// The loop ran out of `max_steps` before any other terminal condition —
    /// the run is right-censored, and its `steps` is a budget artifact.
    BudgetExhausted,
    /// A human gate stopped the run (tiered governance).
    NeedsHuman,
    /// Enforcement hard-stopped the run (engineering-tier scope violation).
    HardBlocked,
    /// The provider failed (outage, bad key, quota) — the run measures the
    /// API, not the agent, and is excluded from metric denominators.
    ProviderError,
    /// A wrapped agent could not run or failed outright (refused sandbox
    /// spawn, or an ungoverned single invocation exiting non-zero).
    AgentError,
    /// The bundle predates this field.
    #[default]
    Unrecorded,
}

/// Provenance of a run's token counts — see [`RunReport::token_accounting`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenAccounting {
    /// Orvena made the model calls and counted the tokens itself.
    #[default]
    Observed,
    /// A wrapped external agent made the calls; the counts are what it reported.
    AgentReported,
    /// Nobody could account for them — the counts are `0` and mean *unknown*,
    /// not *free*.
    Unavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateRecord {
    /// The loop step (1-based) this outcome was recorded on. Lets a bundle with
    /// accumulated multi-step gate history be read step-by-step.
    #[serde(default)]
    pub step: u32,
    pub gate: String,
    pub passed: bool,
    pub needs_human: bool,
}

impl RunReport {
    pub fn new(task: impl Into<String>) -> Self {
        Self {
            schema: EVIDENCE_SCHEMA_V1.into(),
            provider: None,
            model: None,
            endpoint: None,
            task: task.into(),
            completed: false,
            steps: 0,
            input_tokens: 0,
            output_tokens: 0,
            tool_calls: 0,
            // `None` until a loop that can attribute actions claims it — the
            // native driver does so on entry, wrapped agents never do.
            action_counts: None,
            gate_outcomes: Vec::new(),
            blockers: Vec::new(),
            scope_refusals: Vec::new(),
            sandbox: crate::exec::sandbox::SandboxStatus::default(),
            provider_error: None,
            agent: None,
            token_accounting: TokenAccounting::default(),
            max_steps: 0,
            exit: ExitReason::default(),
        }
    }

    /// Stamp what produced this run, so the bundle identifies its own backend.
    pub fn with_provenance(mut self, sel: &crate::config::agent::ProviderSelection) -> Self {
        self.provider = Some(sel.kind.clone());
        self.model = Some(sel.model.clone());
        self.endpoint = sel.endpoint_origin();
        self
    }

    /// Seal the report with its completion status.
    pub fn finished(mut self, completed: bool) -> Self {
        self.completed = completed;
        self
    }

    pub fn total_tokens(&self) -> u32 {
        self.input_tokens + self.output_tokens
    }
}
