//! # orvena-core
//!
//! The embeddable core of **Orvena** — a customizable, config-first coding agent
//! that treats AI as a *bounded* team member rather than an unsupervised code
//! generator. All behavior lives in this library so a larger AI runtime can link
//! it directly; the `orvena` CLI is only a thin frontend.
//!
//! v0.1 embodies the minimal core of the methodology's five pillars:
//! - **Bounded change** — [`governance::scope`] (scope lock + read-only default)
//! - **Specialized roles** — [`config::roles`] (allowed/forbidden tools)
//! - **Controlled context** — [`config::context_budget`] + [`agent::context`]
//! - **Verifiable gates** — [`governance::gate`] (run a check, observe evidence)
//! - **Evidence & done** — [`metrics`] (frozen run fields; stop on a passed gate)
//!
//! Two meta-mechanisms: **config-first** (everything behavioral is YAML) and a
//! minimal **tiered governance** ([`config::agent::Tier`]).
//!
//! The same guarantees can also be applied to an agent Orvena did not write:
//! [`adapter`] spawns a third-party CLI agent inside the OS sandbox and feeds it
//! through the identical gate / oracle / evidence path (ADR-004). The native
//! loop stays the reference implementation — deterministic, offline-reproducible,
//! and dependency-free.

pub mod adapter;
pub mod agent;
pub mod benchmark;
pub mod config;
pub mod error;
pub mod exec;
pub mod governance;
pub mod metrics;
pub mod provider;
pub mod skills;
pub mod tools;
pub mod util;

pub use agent::{Agent, Task};
pub use benchmark::{run_benchmark, BenchReport, BenchTaskSet};
pub use config::Config;
pub use error::{Error, Result};
pub use metrics::RunReport;
pub use provider::{build_chat_provider, ChatRequest, ChatResponse, Message, Provider};
