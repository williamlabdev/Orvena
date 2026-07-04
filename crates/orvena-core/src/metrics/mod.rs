//! L1 regression metrics. Every run produces a [`RunReport`] with frozen fields
//! a maintainer can diff across daily changes: did it complete, how many tokens,
//! how many steps, how many tool calls. This is a regression test against
//! ourselves — not an external benchmark. (Evidence & Done pillar.)

pub mod baseline;
pub mod evidence;

pub use baseline::{BaselineRecord, GoldenTask};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunReport {
    pub task: String,
    /// True when all gates passed (the run reached "done").
    pub completed: bool,
    pub steps: u32,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub tool_calls: u32,
    pub gate_outcomes: Vec<GateRecord>,
    pub blockers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateRecord {
    pub gate: String,
    pub passed: bool,
    pub needs_human: bool,
}

impl RunReport {
    pub fn new(task: impl Into<String>) -> Self {
        Self {
            task: task.into(),
            completed: false,
            steps: 0,
            input_tokens: 0,
            output_tokens: 0,
            tool_calls: 0,
            gate_outcomes: Vec::new(),
            blockers: Vec::new(),
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
