# Changelog

All notable changes to Orvena are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed

- **Verify-gate feedback no longer goes silent** (slice-004) — a `verify` command
  that **fails without output** (`test -f x`, `grep -q`, `diff -q`, and most real
  checks print nothing on failure) used to feed back an *empty* evidence string,
  leaving the loop's next attempt with an unchanged context — it would re-emit the
  same output and spin to `max_steps` without ever converging. Now a failed
  automated gate always contributes an actionable line: the gate's **target
  condition** plus either the command output or a synthesized `verify exited <n>
  with no output`. This is what makes "done = your verify command exits 0" hold on
  real projects. Backed by new regression tests: driver-level coverage for the
  all-gates-must-pass conjunction, silent-failure convergence (fail → fix from the
  fed-back condition → pass), human-gate escalation, and `max_steps` exhaustion;
  plus unit coverage for a verify-less automated gate failing closed and the
  synthesized exit status.

### Added

- **Benchmark pass rate over repeated runs** (slice-010) — `orvena bench --repeat
  N` runs every task `N` times and reports a **per-task pass rate** plus a **mean
  pass rate** (the expected single-pass completion rate, de-noised against model
  nondeterminism) and a `solved ≥once` count; `--repeat 1` (default) is the
  unchanged single-pass path. Core `run_benchmark_repeated`/`RepeatedReport`
  layer over the existing single-pass runner (each repeat gets its own workdir
  namespace, and the underlying per-repeat reports are retained for audit);
  skips stay excluded from the denominator. Aggregation is regression-tested
  deterministically with the offline provider. A convenience script
  `scripts/bench-passrate.sh` runs a scratch-config + offline sanity + the real
  repeated run for a chosen local model.
- **Curated benchmark task set + first published number** (slice-009, MVP+1) — a
  curated, self-contained task set (`benchmarks/realworld.yaml`) with non-trivial
  multi-file bugs (off-by-one, empty-input, out-of-bounds, a bug in a second
  module) verified by real `cargo test`/`pytest`, plus an `orvena bench --out
  <path>` flag to land the JSON report where it's published. **First published
  number** ([docs/benchmark-results.md](docs/benchmark-results.md), 2026-07-04):
  Orvena driving a local `qwen3:14b` solved **5/5 (100%)** of the ran Rust tasks
  single-pass (2 Python tasks skipped, `pytest` absent). The results page is
  written to *deflate* — it states plainly that 100% on a tiny, simple, curated
  set is a weak early signal, not a real-world capability claim, and lists the
  path to a stronger number (harder/larger tasks, real-repo snapshots, repeated
  runs, hosted models). README gains a Benchmark section linking it.
- **Seeded project benchmark tasks with real test runners** (slice-008) — an
  opt-in task set (`benchmarks/projects.yaml`, run via `orvena bench --tasks`)
  where each task seeds a small **buggy project** and its `verify` runs a real
  test runner — the model must fix the code so `cargo test` / `pytest` exits 0
  (the actual "done = your tests pass" claim, not just file creation).
  **Skip-aware:** a task declares `requires` (e.g. `cargo`, `pytest`); if a
  required command is absent the task is **skipped**, not failed, and excluded
  from the completion-rate denominator (`rate = passed / ran`) — a missing
  toolchain never reads as "0% because it isn't installed". Skips are reported
  (`BenchReport.skipped` + per-task `skip_reason`). Test files are read-only
  (only the implementation is writable) so a task can't be gamed by deleting its
  test; Rust fixtures carry an empty `[workspace]` so `cargo test` in the bench
  workdir doesn't try to join a parent workspace. Regression-tested
  deterministically (a missing-toolchain task skips and the rate is over ran
  tasks only); demonstrated end to end against a real local `qwen3:14b` (fixed
  the seeded Rust bug so `cargo test` passed; the Python task skipped with
  `pytest` absent). See [docs/benchmark.md](docs/benchmark.md).
- **Minimal benchmark harness** (slice-007, MVP+1) — `orvena bench [--provider
  <kind>] [--tasks <file>]` runs a set of hand-picked, auto-verifiable coding
  tasks through the bounded loop and reports a **completion rate** (fraction that
  reached a passing `verify`), writing a per-task table + `report.json` and a
  per-task evidence bundle under `.orvena/bench/<run_id>/`. Each task carries its
  **own** `verify` (its success criterion — a shared always-pass gate would make
  the number meaningless); the built-in set is toolchain-free (`test`/`grep`) and
  includes a seeded "fix until the check passes" task so the rate reflects real
  editing, not just file creation. Adds no execution engine — it orchestrates the
  existing loop and aggregates; a task that errors is counted as a non-completion
  (with its error as a blocker), not an abort. Core logic lives in
  `orvena_core::benchmark` (`run_benchmark`/`write_report`), the CLI is a thin
  wrapper sharing `run`'s provider override + readiness preflight. Method and
  honesty caveats in [docs/benchmark.md](docs/benchmark.md); the deterministic
  offline path is regression-tested (a mixed set yields a known 0.5 rate).
  Demonstrated end to end against a real local model (`qwen3:14b`). *Publishing*
  the number stays a manual step (MVP+1 exit boxes remain unchecked).
- **Cross-provider parity harness** (slice-006) — the repeatable, operational
  form of the MVP-exit criterion "Anthropic + Ollama behave consistently". A new
  `#[ignore]`d integration test (`crates/orvena-core/tests/provider_parity.rs`,
  selected by `ORVENA_PARITY_PROVIDER`/`ORVENA_PARITY_MODEL`) runs a golden task
  against a **real** provider and asserts the behavioral **contract** that must
  hold regardless of model — a well-formed `RunReport`, consistent completion
  semantics (`completed` ⇔ every gate passed), a real round-trip (token usage
  reported; the `offline` stub is only a regression baseline per MVP-SCOPE §5),
  and an evidence bundle that round-trips — **not** exact step/token equality,
  which legitimately varies. Ignored by default so `cargo test` stays offline and
  deterministic. Demonstrated end-to-end against a real local Ollama model
  (`qwen3:14b`: golden task completes, gate passes, real token usage, bundle
  round-trips); the Anthropic side is runnable with a key. See
  [docs/provider-parity.md](docs/provider-parity.md).
- **Painless first run** (slice-005) — a brand-new user can go from `init` to a
  working loop + evidence bundle without getting stuck on setup. `orvena run`
  now takes `--provider/-p <kind>` to **override the configured provider for one
  run** (config on disk untouched): `orvena run --provider offline "<task>"`
  runs the whole loop against the deterministic stub with **no API key and no
  network**, completing against the scaffold's `verify: "true"` gate and
  exporting an evidence bundle — so the core "evidence by default" deliverable
  is visible before committing to a real provider. `run` also **preflights
  provider readiness** (reusing the same `registry::readiness` check as
  `doctor`, so the two never drift): a missing key or unknown provider now fails
  fast with actionable guidance (point to `.env.example`, `orvena doctor`, and
  the offline shortcut) instead of dead-ending on a deep provider/network error.
  `orvena doctor` additionally notes the evidence-bundle path on success. Proven
  by CLI integration tests driving the built binary end to end (zero-setup
  offline run lands a bundle; a not-ready provider fails fast with guidance and
  writes nothing).
- **Evidence-bundle exporter** (slice-003, per ADR-002) — the minimal, provable
  form of "evidence by default". After a run, the `RunReport` (which already
  derives `Serialize`) is written to disk as a single **pretty-printed JSON**
  file at `.orvena/runs/<timestamp>/evidence.json` — carrying `completed`,
  `gate_outcomes`, `blockers`, and the frozen `steps`/`tool_calls`/token counts —
  and its path is printed to stdout (the `print_report()` summary is kept). The
  bundle is written **before** the `!completed` bail, so a run stopped by a gate
  leaves an audit trail too: **failed runs get a bundle just like completed ones**
  (the evidence matters most exactly then). `timestamp` is Unix-epoch
  milliseconds (no date-library dependency in v0.1; ADR-002 records the location
  and format rationale). This is *not* a new subsystem — a persistent event log
  stays deferred; this only serializes the report already in memory. Proven by an
  offline round-trip test covering both a completed and an incomplete run
  (bundle written → deserializes back into an equal `RunReport` → `completed` /
  `gate_outcomes` / `blockers` intact).
- **Declarative shell `RUN` tool + `CommandRunner`** (slice-002, per ADR-001) — the
  model never supplies a command string; it references a command the human declared
  in `commands.yaml` by name (`<<<RUN name>>>`), and the runtime spawns that
  command's **fixed argv** directly with no shell interpretation. Role-gated
  (`shell.run`); authorization is a fixed order — role → declared name → `read_only`
  intent — and every denial is an `Error::Scope` (undeclared names and `mutating`
  commands are refused even when a role allows `shell.run`). An authorized
  `read_only` command that exits non-zero is **evidence-only** (fed back like a
  failed gate, never a `report.blocker`, no engineering hard-stop), so the loop can
  "run tests → read the failure → fix → re-run" (proven by an offline round-trip
  test). A shared `CommandRunner` now backs both the RUN tool (fixed argv) and the
  `verify` gate (`sh -c`, human-authored), adding a **timeout** to both: a gate that
  outruns its `timeout_secs` (default 300s) now fails verify instead of hanging.
  `orvena init` scaffolds `commands.yaml` with `test`/`build`/`clippy` (all
  `read_only`) and grants `developer` the `shell.run` tool.
- **Read-only grep tool + `SEARCH` action** — role-gated (`grep.search`), pure-Rust
  (`regex` + `ignore`, no shell-out) content search bounded to the project root
  (symlinks not followed; `.git/`/`target/` excluded). The model requests it with a
  `<<<SEARCH pattern ... >>>` block; hits are fed back as evidence on the next
  step, so the loop can "search -> use the results to change a file" (proven by an
  offline round-trip test).
- **Bounded coding loop** — prepare context → call model → apply (scope-gated) →
  check gates, with a bounded re-attempt when an automated gate fails (capped by
  `max_steps`).
- **Provider abstraction with no silent default** — Anthropic, OpenAI, OpenRouter,
  Ollama, and a deterministic `offline` stub, behind an explicit
  `build_chat_provider` factory. An unknown/unconfigured provider fails loudly.
- **Config-first YAML** — `roles` (allowed/forbidden tools), `gates` (condition +
  `verify` command for observable evidence + `automated`/`human` gatekeeper),
  per-role `context-budgets`, and top-level `orvena.yaml` with a governance tier.
- **Three disciplines** — scope lock, read-only default, and verifiable gates.
- **L1 regression metrics** — per-run frozen fields (completed, tokens, steps, tool
  calls) with a golden-task baseline freeze/diff.
- **Minimal skill engine** — discover → resolve → apply, with one reference skill
  (`summarize-changes`).
- **CLI** — `orvena init` (scaffold + provider wizard), `orvena run`,
  `orvena doctor`, `orvena status`.
- **Two-tier pre-publish boundary check** (`scripts/boundary-check.sh`) and CI
  running build · test · clippy · boundary · clean-machine install.
