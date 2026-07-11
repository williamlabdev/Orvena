# Benchmark results

> **First number — 2026-07-04:** on a curated set of **5 self-contained Rust
> tasks**, Orvena driving **Ollama `qwen3:14b`** solved **5/5 (100%)** in a
> **single pass**. 2 Python tasks were **skipped** (`pytest` not installed).

That headline needs its caveats read with it. **This is a deliberately small,
early, self-hosted signal — not a capability claim.** See "How to read this".

## The run

| | |
|---|---|
| Date | 2026-07-04 |
| Provider / model | `ollama` / `qwen3:14b` (local) |
| Task set | [`benchmarks/realworld.yaml`](../benchmarks/realworld.yaml) — curated, self-contained |
| Runs per task | 1 (single-pass) |
| Result | **5 / 5 ran solved = 100%**, 2 skipped |
| Raw report | [`benchmark-results/2026-07-04-qwen3-14b.json`](benchmark-results/2026-07-04-qwen3-14b.json) |

| Task | Result | Steps |
|---|---|---|
| `rust-inclusive-range` (off-by-one `1..n` → `1..=n`) | ✅ pass | 1 |
| `rust-average-empty` (divide-by-zero on empty slice) | ✅ pass | 1 |
| `rust-fizzbuzz-swapped` (Fizz/Buzz swapped) | ✅ pass | 1 |
| `rust-last-oob` (`xs[xs.len()]` out of bounds) | ✅ pass | 1 |
| `rust-kv-parse` (bug in `src/parser.rs`, multi-file) | ✅ pass | 1 |
| `python-average-empty` | ⏭ skipped — no `pytest` | — |
| `python-parse` | ⏭ skipped — no `pytest` | — |

Each solve was verified by a **real `cargo test` exiting 0** — the model edited
the implementation and the project's own tests passed.

## How to read this (不美化)

- **The set is small (5 ran) and the bugs are simple** — single-function or
  single-file fixes (an off-by-one, an empty-input guard, a swapped branch, an
  out-of-bounds index, a wrong split char). A capable model clearing all of them
  in one shot is expected. **100% here means "the tasks are easy and the model is
  competent", not "Orvena solves real coding".**
- **These are curated tasks, not real-world repos.** No GitHub issues, no
  SWE-bench, no large multi-module codebases. Real difficulty lives there.
- **Single pass, nondeterministic.** One run of a stochastic model; a re-run may
  differ. This is not a pass-rate over repeats.
- **Model- and machine-specific.** `qwen3:14b` locally; a smaller model would
  score lower, a stronger hosted model likely higher.
- **Failures would be shown here too** — nothing is hidden or cherry-picked
  (method: [`benchmark.md`](benchmark.md)).

## Reproduce

```sh
# needs a local Ollama serving qwen3:14b (or swap the model/provider)
orvena bench --tasks benchmarks/realworld.yaml --provider ollama \
  --out docs/benchmark-results/$(date +%F)-qwen3-14b.json
```

To run the Python tasks too, install `pytest` (else they skip and are excluded
from the rate). To get a hosted-model number, point `--provider` at
`openai`/`anthropic` with a key (see [provider-parity.md](provider-parity.md)).

## Toward a stronger number

The credible next steps, in order of value: **more and harder tasks** (multi-
module, larger context), **vendored snapshots of real OSS projects** at pinned
commits with real historical bugs, and eventually a **SWE-bench-style dataset**;
plus **repeated runs** for a pass-rate and **hosted models** for a headline.
Those are larger efforts (some need docker/dataset/network) — this page is the
honest v0.1 starting point, re-runnable at any time.
