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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunReport {
    /// Self-describing schema identifier — an artifact of record must say what
    /// it is without its producer on hand. Bundles written before the field
    /// existed read back as v1 (they are).
    #[serde(default = "evidence_schema_v1")]
    pub schema: String,
    pub task: String,
    /// True when all gates passed (the run reached "done").
    pub completed: bool,
    pub steps: u32,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub tool_calls: u32,
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
            task: task.into(),
            completed: false,
            steps: 0,
            input_tokens: 0,
            output_tokens: 0,
            tool_calls: 0,
            gate_outcomes: Vec::new(),
            blockers: Vec::new(),
            scope_refusals: Vec::new(),
            sandbox: crate::exec::sandbox::SandboxStatus::default(),
            provider_error: None,
        }
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
