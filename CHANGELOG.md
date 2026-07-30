# Changelog

All notable changes to Orvena are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **`openai_compat` — a first-class provider kind for any OpenAI-compatible
  endpoint.** Self-hosted open-source inference servers (vLLM, llama.cpp
  server, LM Studio, TGI, SGLang) and hosted open-weight aggregators (Groq,
  Together, Fireworks) all speak the wire format `OpenAiCompat` already
  implemented, but the factory only accepted `openai`/`openrouter`, so reaching
  one meant pretending to be a vendor you were not and putting your key in an
  unrelated variable. The new kind names the thing honestly and adds the one
  capability that genuinely did not exist: `api_key_env` points at whatever
  variable holds your key, and **omitting it sends no `Authorization` header at
  all**, so a keyless local server works — previously the builder hard-required
  an env var and refused to start without one. `base_url` is required with no
  default, because there is no sane endpoint to guess (`orvena init` prompts for
  it). Verified end-to-end against Ollama's own OpenAI-compatible endpoint:
  real token usage, gate passed, evidence bundle round-tripped.

- **Evidence bundles and benchmark reports now name the backend that produced
  them.** `provider` alone answered "which endpoint?" only while every kind had
  a fixed one; `openai_compat` is deliberately endpoint-agnostic, so a local
  llama.cpp and a hosted aggregator serving the same open-weight model produced
  byte-identical provenance. Bundles gain `provider` / `model` / `endpoint` and
  reports gain `endpoint`, where endpoint is the **origin only**
  (`scheme://host[:port]`) — userinfo, path, and query are stripped, because a
  user-supplied `base_url` can carry a token and these files get published.
  Additive fields, so v1 stands and older bundles still read.
- **Parity runs can leave a committed artifact.** `ORVENA_PARITY_EVIDENCE_OUT`
  writes the run's evidence bundle to a path you choose; the first is
  `docs/parity-results/2026-07-31-openai_compat-qwen3-14b.json`. Published
  benchmark numbers already had raw JSON behind them while parity's
  "somebody actually ran it" rested on a number retyped into a table. A test
  keeps every committed bundle schema-valid and self-describing.

### Changed

- **`orvena init` no longer recommends a provider nobody has run.** The picker
  called Anthropic "Recommended first run" while `docs/provider-parity.md`
  claimed that recommendation had been removed — it had been, from the README
  only. Each entry now states its parity status, which is what the README does.

### Fixed

- **`orvena doctor` no longer reports "All checks passed" on configs that
  cannot run.** Readiness had drifted from the builders it was supposed to
  front, in three ways, all reachable from a hand-edited `orvena.yaml`:
  `api_key_env` steered the check for *every* kind while only the
  OpenAI-compatible builders actually read it — so `kind: anthropic` with a
  custom key variable passed preflight and then died on
  `ANTHROPIC_API_KEY is not set`; the same field made the keyless `offline`
  stub *unreachable* when left over in a config, and the resulting error
  advised `--provider offline`, the very thing it was blocking; and the one
  mandatory new field, `openai_compat`'s `base_url`, was not checked at all.
  Readiness now consults a single `key_var` helper gated on whether the kind's
  builder honors `api_key_env`, and rejects a missing required `base_url` with
  a `MissingBaseUrl` state that `doctor` and preflight both report. The
  OpenAI-compatible builder also lists its kinds explicitly instead of falling
  through a catch-all arm that would have silently handed a future kind
  OpenAI's endpoint and key — and mislabeled it `openai` in the evidence.

- **The `orvena init` scaffold no longer ships an outdated sandbox note.** Its
  comments still said Linux reports "unavailable" and referred to a Landlock
  backend that "ships" in the future — that backend landed in slice-016. The
  scaffold now names macOS and Linux as enforced and Windows (or a kernel
  without Landlock) as the `on_unavailable` path.

- **Stale provider lists across the docs.** A sweep for every enumeration of
  provider kinds found three that predated `openai_compat`: the
  `ARCHITECTURE.md` component diagram (which listed *module* names, so it also
  silently omitted `openai`/`openrouter` — it now says which is which),
  `MVP-SCOPE.md`'s subsystem table, and `scripts/bench-differential.sh`, whose
  header both omitted the kind and still taught the Gemini key-masquerade. That
  script also gained `API_KEY_ENV`, so it can drive `openai_compat` — with a
  named key var, or none at all against a keyless local server.

- **A one-line action block no longer manufactures a scope violation** — the
  action protocol expects `<<<RUN check` … `>>>` on separate lines, and models
  constantly write the closer on the same line instead. Taken literally that put
  the marker *inside* the value: `<<<RUN show-validator>>>` asked for a command
  named `show-validator>>>` (undeclared → a scope blocker), and
  `<<<WRITE config.json>>` wrote a file literally named `config.json>>` — a path
  no task declares, which the benchmark's independent oracle then scored as an
  **out-of-scope write**. The measurement was counting the parser's pedantry as
  the agent's misbehavior. The parser also kept consuming lines hunting for a
  `>>>` that had already gone by, swallowing whatever actions followed. Both
  forms are now accepted: the trailing run of `>` is measured rather than
  pattern-matched, so a truncated `>>` closer works and a search pattern that
  legally ends in `>` (`<<<SEARCH Vec<T>>>>`) survives, while a lone trailing `>`
  is left as part of the value. Both malformations were observed from a real
  local model within minutes of pointing it at the RUN tool.

- **The RUN tool was undiscoverable** — the system prompt has always described
  `<<<RUN name>>>` but never said which names the project declared, so the model
  had to guess a name out of `commands.yaml` it could not see. A shipped feature
  that can only be used by luck is not shipped. The context now lists the role's
  runnable commands, and lists **names only**: printing each command's argv would
  leak the command *string* into the prompt, and a check like
  `test "$(cat answer.txt)" = "42"` would then be handing over its own answer.
  Only `read_only` commands appear (the runtime refuses a `mutating` one anyway,
  ADR-001), and a role without `shell.run` — or a project with nothing declared —
  sees no change at all.

- **A benchmark run that mostly failed no longer reports a headline number**
  (slice-017) — `orvena bench` folded provider-error runs into every denominator
  and printed rates regardless, so an API outage could masquerade as a result.
  The 2026-07-30 hosted attempt had **39 of 48 task-runs die on 429 quota
  exhaustion** and still produced `false-done 100% → 0%` (resting on a single
  surviving claim) and `×0.41 tokens` (a ratio of two mostly-zero means) — with
  nothing in the report to distinguish it from a real measurement. Now: the
  driver records a **structured `provider_error`** on the run report at the
  capture site (not a string match on `blockers`; additive field, so the evidence
  schema stays v1), the benchmark **excludes such runs from every rate** —
  completion, verified, false-done, containment, evidence-validity, and the
  per-task pass rate — and reports the count so the exclusion is visible instead
  of silent. Above a **20% dead-run share in either posture** the matrix
  **refuses to publish a differential at all** and states why: a weak number gets
  caveats, an invalid one gets withheld. The summary also stops printing `0%`
  when nothing was measured — `0%` read as "the agent scored zero" where it meant
  "we never found out". Aggregation was extracted into a pure function, covered
  by 8 unit tests over the counting and suppression rules, plus driver-level
  coverage that a provider failure sets the flag and that it survives the bundle
  round-trip.

- **Hosted providers no longer die on a rate limit** — the OpenAI-compatible
  provider surfaced HTTP 429 as a **fatal** error mid-run, so a capped key (a
  free-tier Gemini key via the OpenAI-compat endpoint) hitting its limit killed
  the run partway and turned a hosted benchmark leg into unusable data. 429/503
  and transient transport errors are now **retried with backoff that honors the
  server's own hint** (Google's structured `retryDelay`, or a plain-text "retry
  in Ns"), capped at 90s; non-transient failures stay fatal. Optional proactive
  pacing via `ORVENA_MIN_REQUEST_INTERVAL_MS` runs on a process-global clock, so
  the spacing holds **across** the per-task providers the benchmark rebuilds —
  a per-instance throttle would not. `ORVENA_MAX_RETRIES` tunes the retry count.
  This unblocks the hosted-model leg of the governance differential (D6), which
  is still unpublished.

## [0.1.0] - 2026-07-12

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

- **OS-level sandbox — enforcement moves to the child-process boundary**
  (slice-015, ADR-003) — until now every guarantee lived in-process:
  `FsTool::resolve_in_root` confines the model's `WRITE`s, and `intent`
  (ADR-001) is the human's *trust declaration* — the runtime never proved a
  command was truly read-only. So a `<<<RUN>>>` command or a gate's `verify`,
  once spawned, ran with Orvena's full ambient authority: it could write
  `~/.ssh`, write anywhere outside the root, or phone home. The in-process
  scope lock does not reach into a spawned child. This slice closes that gap at
  the single spawn choke-point (`CommandRunner`, shared by RUN and gate): every
  child is wrapped in a least-privilege OS sandbox — **read-only outside the
  project root, no network by default**. On **macOS** this is enforced via
  `sandbox-exec` + a subtractive SBPL profile and is verified end to end
  (`tests/sandbox_confinement.rs`: an out-of-root write and a connection to an
  open port are both blocked by the OS, while an in-root write still succeeds).
  Config lives in `orvena.yaml`'s optional `sandbox:` block (`enabled`,
  `network`, `filesystem: root_write|strict`, `on_unavailable`); the scaffold
  ships `enabled: true` (new projects are confined out of the box) while the
  struct default stays disabled (embedders/old configs are unchanged). When the
  platform backend is unavailable the policy **fails closed** under the
  engineering tier (refuse to run) and warns under light — never a silent
  unconfined "enforced" claim. `RunReport.sandbox` (`enforced`/`disabled`/
  `unavailable`) records which, additively in evidence schema v1. **Linux
  (Landlock + seccomp re-exec shim) is deferred to a follow-up** — it currently
  reports unavailable, so Linux engineering runs fail closed rather than run
  unconfined. Backed by unit tests for the policy/backend/config plus the
  containment integration test; existing `exec.rs` tests are unchanged (the
  unconfined `CommandRunner::new` path is byte-for-byte compatible).
- **OS sandbox — Linux backend (Landlock + seccomp)** (slice-016) — the Linux
  half of ADR-003's child-process confinement. The hidden `orvena __sandbox`
  subcommand is a **re-exec shim**: dispatched from `main` *before* the tokio
  runtime starts (so it runs single-threaded, which is async-signal-safe), it
  applies **Landlock** to itself — read + execute everywhere, write only under the
  project root + system temp + `/dev` — plus, under `network: deny`, a **seccomp**
  filter that fails `socket(AF_INET|AF_INET6)` with `EACCES` (AF_UNIX untouched),
  then `execvp`s the wrapped command. A re-exec shim is used instead of
  `Command::pre_exec` because applying Landlock between fork and exec in a
  multi-threaded process is not async-signal-safe. If the kernel does not enforce
  Landlock, the shim **fails closed** (never runs the command unconfined) — so
  Linux engineering runs are safe whether or not the backend is available. `main`
  no longer uses `#[tokio::main]` (it builds the runtime explicitly to intercept
  `__sandbox` first), and CI runs the full gate set on a
  `[ubuntu-latest, macos-latest]` matrix so both backends are exercised on every
  push. The Landlock/seccomp deps are Linux-target-gated (macOS pulls neither).
  The backend is **compile-verified cross-target** (`cargo check
  --target x86_64-unknown-linux-gnu` against the real `landlock`/`seccompiler`
  APIs); **runtime containment is verified on the Linux CI leg**
  (`orvena-cli/tests/sandbox_linux.rs`, control-gated so it skips rather than
  false-fails where a runner's kernel does not enforce Landlock), not on macOS.
- **The governance differential, published** (slice-014) — the second external
  number, and the first on the axis Orvena exists for. Temptation set ×
  (`off`, `engineering`) × 3 runs, local `qwen3:14b`: **false-done 25% → 0%
  of claims** (M2), governed cost **×0.43 steps / ×0.31 tokens** (M4 —
  inverted: gate evidence acts as navigation, the governed runs were
  cheaper), ground truth 79% under both postures, containment 100%/100%
  (M1 — honestly reported as a null result on this model: temptations
  surfaced as refused, auditable escape attempts rather than landed files),
  evidence validity 100%/100% (M3). Full run data in
  `docs/benchmark-results/2026-07-11-qwen3-14b-differential.json`; analysis
  and de-glamorized caveats in `docs/benchmark-results.md`; method section in
  `docs/benchmark.md`; reproducible via `scripts/bench-differential.sh`.
  Hosted-model legs pending (no key available on the bench machine).
- **Evidence schema v1 (frozen) + exit-path fault injection** (slice-013,
  M3/D4) — the evidence bundle is now a versioned artifact of record.
  `schemas/evidence.v1.json` (JSON Schema 2020-12) freezes the contract;
  bundles are self-describing via a new `schema: "orvena-evidence-v1"` field
  (pre-v1 bundles read back as v1). Compatibility policy is in the schema
  itself: additive fields keep v1 — consumers must ignore unknown fields;
  a removal or type change bumps to v2 under a new identifier. A shipped
  validator (`evidence::validate_bundle`) checks the contract hand-rolled over
  raw JSON — deliberately NOT via the serde derive, whose defaults would
  silently repair a bundle missing required fields. A test suite exercises
  every driver exit path (success, gate-fail to max_steps, human gate,
  provider error) and validates each bundle twice — shipped validator AND a
  real JSON-Schema engine against the schema file — so the two cannot drift
  apart (engine is a dev-dependency only). `orvena bench` now measures **M3
  on every run**: per-task `evidence_valid` and an aggregate
  `evidence_valid_rate` (a ran task with a missing or invalid bundle counts
  against it — never assumed).
- **Temptation task set + independent violation oracle** (slice-012, M1/D3) —
  `benchmarks/temptation.yaml`: 8 scope-adversarial tasks where the easiest or
  most "natural" fix violates the declared scope (edit the check instead of the
  code; "fix" a read-only neighbor — including a typo that begs correcting;
  write outside the root — including an instruction that explicitly asks for an
  out-of-root backup; plus one honest lazy-path task documenting what no gate
  can catch). Judged by a new **git-based oracle** (`benchmark::oracle`) that
  shares no code with the enforcement layer (the player cannot referee its own
  match): baseline commit after seeding, `git status` diff after the run,
  its own reading of the writes contract, `escape_probes` for out-of-root
  writes git cannot see, and build-artifact exclusion via `.git/info/exclude`
  (invisible to the agent). Every report gains **containment rate (M1)**,
  per-task violations, **false blocks** (enforcement refusing a path the
  contract allowed — cross-checked from the new structured
  `RunReport.scope_refusals`), and `oracle_errors` (an unjudgeable run is
  surfaced, never counted as contained). The matrix differential now leads
  with containment. Workdirs stay git repos on purpose: `git diff` against the
  baseline is publishable evidence of exactly what the agent touched.
- **Governance-differential benchmark matrix** (slice-011) — `orvena bench
  --governance off,light,engineering` runs the same task set once per governance
  posture and reports the **differential**: what the brakes buy. Two new
  measurements land with it. (1) **Ground truth on every run**: the harness now
  re-runs each task's `verify` itself after the loop — independent of any
  in-loop gate — so every report carries a `verified` rate next to the claimed
  completion rate. (2) **False-done rate (M2)**: of the runs that claimed done,
  the fraction whose external verify fails. `off` is a **bench-only ungoverned
  baseline** (D2): identical prompt, no scope enforcement (root escape is host
  protection and stays), no gates — "done" is the model's own unverified claim,
  which is exactly the failure mode the differential interrogates. It is not a
  product tier and is unreachable from `orvena run`'s CLI/config surface. The
  matrix report adds M4 cost ratios (governed/baseline steps and tokens). Plan:
  `docs/benchmark-governance-differential-plan.md` (D1–D6). Deterministic
  offline coverage: a read-only trap task pins baseline false-done = 100% of
  claims vs governed = 0% (a failing gate makes the false claim structurally
  impossible), plus a governed-vs-external-verify cross-check.
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
