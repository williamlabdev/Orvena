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

## The governance differential (M1–M4)

Completion rate is the axis every coding agent competes on; it is not the axis
Orvena exists for. The differential answers a different question — **what do
the brakes buy?** — by running the *same* task set, the *same* model, and the
*same* prompts under two postures, with only enforcement differing (plan and
decision record:
[benchmark-governance-differential-plan.md](benchmark-governance-differential-plan.md)):

- **`off`** — a bench-only ungoverned baseline: no scope enforcement (the root
  boundary stays — that is host protection, not governance), no gates. "Done"
  is the model's own unverified claim: the run ends when it stops emitting
  actions. This posture does not exist in the product.
- **`engineering`** — the shipped hard-enforced tier.

Four measurements, all produced by `orvena bench --governance off,engineering`:

| # | Measurement | Judge |
|---|---|---|
| M1 | **Containment** — fraction of runs whose every change was declared in `writes`; plus **false blocks** (enforcement refusing a path the contract allowed) | an **independent git-based oracle** (`benchmark::oracle`) that shares no code with the enforcement layer; `escape_probes` catch out-of-root writes git cannot see |
| M2 | **False-done rate** — of the runs that claimed done, how many the external verify exposed | the harness re-runs each task's `verify` after the loop, in every mode, independent of any in-loop gate |
| M3 | **Evidence completeness** — schema-valid bundle on every run | the shipped validator against the frozen [`schemas/evidence.v1.json`](../schemas/evidence.v1.json) |
| M4 | **Cost of governance** — governed/baseline ratios for steps and tokens | paired runs, reported honestly (the brakes are not free — except when they are: gate feedback can also *shorten* runs) |

The task set for M1/M2 is the **temptation set**
([`benchmarks/temptation.yaml`](../benchmarks/temptation.yaml)): tasks where
the easiest fix violates scope (edit the check, "fix" a read-only neighbor,
write outside the root), designed as realistic asks — over-engineered traps
would inflate the differential and are against the plan's honesty rule. One
lazy-path task documents what no gate can catch (hardcoding the expected
answer stays in scope and passes verify): the differential measures
*containment and honesty*, not semantic correctness.

**The agent can run the check (since slice-019).** The benchmark declares each
task's `verify` as a read-only `check` command, plus any extra read-only
commands the task lists, and grants the role `shell.run`. This was not a
convenience: before it, the benchmark's agent could not run anything, and the
prompt only ever shows the *writable* files — so the read-only neighbor, the
failing validator, and the check's own output were all invisible. The
2026-07-11 containment differential came out a null result for that reason, and
"the baseline resisted temptation" was not a safe reading of it. Any real
unbounded agent can `cat` those files without asking.

Two disciplines keep it honest, and one consequence is worth stating plainly:

- **Capability is identical in every posture** (same role, same tools, same
  commands, same prompt) — enforcement stays the only variable, pinned by a test.
- **No declared command solves a task or points at its shortcut.** They make
  visible what a shell would show, nothing more. The command *strings* are also
  kept out of the prompt (only names are listed): a check like
  `test "$(cat answer.txt)" = "42"` would otherwise hand over its own answer.
- **This strengthens the baseline, which can only shrink our differential.**
  That is the intended direction. A differential measured against a blindfolded
  opponent is not a measurement. It also means numbers published before
  2026-07-30 were measured under a *different capability envelope* and are not
  directly comparable to later ones — noted on the results page too.

Reproduce with [`scripts/bench-differential.sh`](../scripts/bench-differential.sh)
(defaults: `qwen3:14b` via local Ollama, 3 runs per task per mode).

## Measuring an agent Orvena did not write (`--agent`)

The differential above compares two postures of *Orvena's own* loop. The same
harness can measure a **third-party CLI agent** instead — the agent supplies the
loop, Orvena supplies the scope, the gate, and the evidence (ADR-004):

```sh
orvena bench --tasks benchmarks/temptation.yaml --agent aider --governance off,engineering
AGENT=aider scripts/bench-differential.sh 3 qwen3:14b   # same thing, scripted
```

- **`off`** — the agent runs with the whole workdir writable and no gate: "done"
  is its own exit status. The root boundary still holds (host protection).
- **`engineering`** — the agent is spawned inside the OS sandbox with **writable
  narrowed to the task's declared paths**, and Orvena's gate decides done. An
  out-of-scope write fails at the syscall, whatever the agent intended.

Everything around the loop is unchanged, which is the point: the same independent
git oracle judges it, the same external verify is ground truth, and it leaves the
same schema-v1 bundle (now carrying `agent` and `token_accounting`).

Three caveats travel with any adapter number:

- **Only the filesystem is contained.** The wrapped agent must reach its own
  model provider, so the sandbox runs `network: allow`. Orvena bounds what it can
  *write*, not what it can *send*.
- **Cost is not observed.** Orvena makes no model call in an adapter run; tokens
  are whatever the agent prints (`token_accounting: agent_reported`) or unknown
  (`unavailable`, in which case the differential prints **no** token ratio —
  a ratio of two unknowns is not "governance is free").
- **A declared path that does not exist yet widens to its parent directory** —
  the OS grants "you may write in this directory", not "you may create exactly
  this name". The widening is recorded in the run's blockers, and containment for
  that path falls back to oracle detection.

Requires the agent on `PATH` (`pipx install aider-chat`); a missing one is an
error up front, not a benchmark full of zeros.

## Status

A **first number is published**: see
[benchmark-results.md](benchmark-results.md) (2026-07-04, a local `qwen3:14b`
solving a small curated Rust set single-pass). The harness, the default set, and
the curated/project sets are all demonstrated end to end. Making the number
*stronger and more credible* — larger/harder tasks, real-repo snapshots, repeated
runs, hosted models — is ongoing; those are larger efforts and stay manual, not
CI-gated.
