# Benchmark — the completion-rate number

**Status:** minimal harness in place (slice-007) · number not yet published
**Updated:** 2026-07-04

MVP+1 (see [MVP-SCOPE.md](../MVP-SCOPE.md) §1) is to publish an **honest,
reproducible completion rate**: run a set of hand-picked, auto-verifiable coding
tasks through Orvena and report the fraction that reached a passing `verify`.
This document defines the method; `orvena bench` produces the number.

## Method

- **Task set** — hand-picked tasks in a YAML file
  ([`crates/orvena-cli/src/benchmarks/tasks.yaml`](../crates/orvena-cli/src/benchmarks/tasks.yaml),
  embedded as the default). Each task is: an `instruction`, the paths it may
  modify (`writes`), an optional `seed` (files placed in the workdir first), and
  its **own** `verify` command.
- **Solved = `verify` exits 0** — the same "done" rule the product ships. Each
  task carries its own criterion; a shared always-pass gate would make the
  number meaningless. Checks are toolchain-free (`test`/`grep`) so the set runs
  anywhere.
- **Completion rate = solved / total**, one run per task.
- **Isolation + evidence** — each task runs in its own workdir under
  `.orvena/bench/<run_id>/<task-id>/` and leaves an `evidence.json` bundle; the
  aggregate is written to `.orvena/bench/<run_id>/report.json`. Every number is
  auditable down to the per-task run.

## How to run it

```sh
# Deterministic smoke (no key, no network) — exercises the harness, not a real
# number: the offline stub only writes boilerplate, so most tasks fail.
orvena bench --provider offline

# A real number against a local model.
orvena bench --provider ollama            # provider.model in .orvena/orvena.yaml

# A real number against a hosted model (key in the environment).
orvena bench --provider openai            # e.g. Gemini via OpenAI-compat, see provider-parity.md
```

`--tasks <file.yaml>` runs your own task set instead of the built-in one.
`--provider <kind>` overrides the configured provider for the run only, and the
same readiness preflight as `orvena run` applies (a missing key fails fast).
`--out <path>` also writes the JSON report where you want to publish it.

**Pass rate (`--repeat N`).** A single pass of a stochastic model is noisy.
`--repeat N` runs every task `N` times and reports a **per-task pass rate** plus a
**mean pass rate** (the expected single-pass completion rate, de-noised) and
`solved ≥once`. The convenience script
[`scripts/bench-passrate.sh`](../scripts/bench-passrate.sh) does a throwaway-scratch
setup + an offline sanity run + the real repeated run:

```sh
scripts/bench-passrate.sh 5 qwen3:14b   # 5 runs/task against a local Ollama model
```

## Project tasks with real test runners (opt-in)

The built-in set is toolchain-free (`test`/`grep`) so it runs anywhere. A heavier
**opt-in** set seeds small buggy projects whose `verify` runs a real test runner
— the model must fix the code so `cargo test` / `pytest` exits 0:

```sh
orvena bench --tasks benchmarks/projects.yaml --provider <kind>
```

- **Skip-aware.** Each task declares `requires` (e.g. `cargo`, `pytest`). If a
  required command is absent, the task is **skipped**, not failed, and is
  **excluded from the completion-rate denominator** — the rate is `passed / ran`,
  so a missing toolchain never reads as "0% because it isn't installed". Skips
  are reported (`report.json` carries `skipped` + per-task `skip_reason`).
- **Not gameable.** The test file is read-only (only the implementation is in
  `writes`), so a task can't be "solved" by deleting its test.
- **Rust fixtures need an empty `[workspace]`** in their `Cargo.toml`, or
  `cargo test` in the bench workdir tries to join a parent workspace and errors.
- Slower and toolchain-dependent by design; grow the set via your own `--tasks`.

## Honesty caveats (不美化)

- **Runs vary.** Real-provider numbers vary run to run (LLM nondeterminism). A
  single pass is one sample; use `--repeat N` for a de-noised pass rate. Always
  report the provider + model and how many runs (all in `report.json`).
- **Failures are counted, not hidden.** A task that fails its `verify` — or
  whose run errors out — is a non-completion in the rate, with its blockers in
  the report.
- **`offline` is a smoke, not a benchmark.** The deterministic stub proves the
  harness works; being "consistent with a stub" says nothing about real
  capability (MVP-SCOPE §5). A published number must come from a real provider.
- **The default set is small and file-oriented by design** (v0.1) so it runs
  anywhere; it includes a seeded "fix until the check passes" task so the rate
  reflects real editing, not only file creation. Heavier seeded projects with
  real test runners live in the opt-in `benchmarks/projects.yaml` (above); grow
  either via your own `--tasks`.

## Status

A **first number is published**: see
[benchmark-results.md](benchmark-results.md) (2026-07-04, a local `qwen3:14b`
solving a small curated Rust set single-pass). The harness, the default set, and
the curated/project sets are all demonstrated end to end. Making the number
*stronger and more credible* — larger/harder tasks, real-repo snapshots, repeated
runs, hosted models — is ongoing; those are larger efforts and stay manual, not
CI-gated.
