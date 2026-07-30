//! Minimal benchmark harness (MVP+1). Run a set of hand-picked, auto-verifiable
//! coding tasks through the bounded loop and report a **completion rate** — the
//! fraction that reached a passing `verify` (`completed: true`).
//!
//! This adds no execution engine: it orchestrates the existing loop over a task
//! set and aggregates. Each task carries its **own** `verify` command (its
//! success criterion) — a shared always-pass gate would make the number
//! meaningless. Every task runs in an isolated workdir and leaves an evidence
//! bundle, so a published number is auditable per task.
//!
//! One run per task; real-provider numbers vary run-to-run (see
//! `docs/benchmark.md`). The `offline` provider makes the harness itself
//! deterministically testable, but is only a smoke — not a real number.
//!
//! Layout — the four concerns are separated so the one that decides what a
//! published number *means* can be read and tested on its own:
//!
//! - [`mode`] — the governance-posture axis (`off` / `light` / `engineering`).
//! - [`task`] — what is measured: instruction, writable paths, `verify`.
//! - [`report`] — the published shapes and their field contracts.
//! - [`aggregate`] — raw results → rates, including every denominator exclusion.
//! - [`runner`] — executing a set through the bounded loop.
//! - [`oracle`] — the independent containment judge.
//!
//! Everything public is re-exported here, so `crate::benchmark::BenchReport`
//! and friends keep working regardless of which file they live in.

pub mod aggregate;
pub mod mode;
pub mod oracle;
pub mod report;
pub mod runner;
pub mod task;

pub use mode::GovernanceMode;
pub use report::{
    write_matrix_report, write_repeated_report, write_report, BenchReport, Differential,
    MatrixReport, RepeatedReport, TaskPassRate, TaskResult,
};
pub use runner::{run_benchmark, run_benchmark_matrix, run_benchmark_repeated};
pub use task::{BenchTask, BenchTaskSet, SeedFile};
