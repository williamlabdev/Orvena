# docs/next — follow-up tickets

Narrow, single-problem tickets that fell out of a slice or a measurement run.
Each file carries its own `> Status:` line at the top; this index is the
one-glance view so nobody mistakes a closed ticket for backlog.

**As of 2026-09-22 every ticket here is closed.** The files stay because
code comments, the CHANGELOG, and the benchmark results link into them.
A new ticket goes in this directory with a `> Status: OPEN` line and a row
below.

| Ticket | Status | Landed |
|---|---|---|
| [tkt-adapter-gate-confined-by-agent-policy](tkt-adapter-gate-confined-by-agent-policy.md) | DONE | `AdapterRun.gate_sandbox` (#27) |
| [tkt-aider-differential-publishable](tkt-aider-differential-publishable.md) | CLOSED, won't fix as scoped | superseded by tkt-m1-null-is-structural |
| [tkt-bench-provider-error-validity](tkt-bench-provider-error-validity.md) | DONE | `provider_error` in the bench runner |
| [tkt-evidence-all-exit-paths](tkt-evidence-all-exit-paths.md) | DONE | atomic bundle creation in `metrics/evidence.rs` |
| [tkt-gate-inherits-host-tmpdir](tkt-gate-inherits-host-tmpdir.md) | DONE | `GateRunner::run_with_env` (#28) |
| [tkt-init-hangs-when-backgrounded](tkt-init-hangs-when-backgrounded.md) | DONE | `init::can_prompt` (#26) |
| [tkt-m1-null-is-structural](tkt-m1-null-is-structural.md) | RULED (c), implemented | `context::scope_rules` (#29) |
| [tkt-remeasure-native-differential](tkt-remeasure-native-differential.md) | DONE | measurement only; results in `docs/benchmark-results.md` |
| [tkt-scope-lock-write-escape](tkt-scope-lock-write-escape.md) | DONE | `FsTool::resolve_in_root` |
