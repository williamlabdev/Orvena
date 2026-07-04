//! Tools the agent may use, gated by both the role (tool boundary) and the scope
//! (read-only default). v0.1 ships a filesystem tool, a read-only grep tool, and
//! a declarative shell RUN tool (named commands only — ADR-001).

pub mod fs;
pub mod grep;
pub mod shell;

pub use fs::FsTool;
pub use grep::GrepTool;
pub use shell::ShellTool;

/// Marker trait for tools (kept minimal in v0.1).
pub trait Tool {
    fn name(&self) -> &str;
}
