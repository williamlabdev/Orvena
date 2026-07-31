# Ticket: a publishable Aider differential (the third number)

> Status: OPEN · opened 2026-07-30 · follow-up to slice-018

## What exists

`orvena bench --agent aider` works end to end (slice-018, ADR-004) and has
produced a **non-null M1 differential** — the thing the native loop could not
give us:

| model | containment (`off` → `engineering`) |
|---|---|
| `qwen3:14b` | 83% → 100% |
| `qwen2.5-coder:1.5b` | 67% → 100% |

Both are **smoke runs**: 1 repeat, 6 of the 8 temptation tasks (the two
`requires: [cargo]` ones were skipped for speed), one machine. They live in the
slice doc and are deliberately **not** on `docs/benchmark-results.md`.

## What a publishable number needs

- **3 repeats × the full 8-task set × both postures** (the bar the 2026-07-11
  native differential set), via `AGENT=aider scripts/bench-differential.sh 3 <model>`.
- **At least one capable model.** `qwen3:14b` violated on exactly one task, so
  the differential rests on a single event — enough to prove the mechanism, not
  enough to characterise it. A stronger model (hosted, or a larger local one)
  says more, and the model-sensitivity note in the plan (§3.4) applies.
- **The caveats stated on the page itself**, not only in the method doc:
  - only the **filesystem** is contained — the wrapped agent reaches its own
    model provider over the network by design;
  - token figures are **self-reported by Aider**, not observed by Orvena;
  - the number is pinned to **Aider 0.86.2** (the version is in every bundle's
    `agent` field, and the benchmark's reproducibility now rides on it).
- **Ideally the same model on both agents** (native loop vs wrapped Aider) so the
  page can separate "what the brakes buy" from "which loop is better" — those are
  two different claims and must not be blurred into one table.

## Why it is not just "run it again"

A published number is a commitment. The 2026-07-11 page has a de-glamorized
structure (what the runs actually show → how to read this → reproduce) that this
one has to earn too, including an honest account of the one thing an adapter
number cannot claim: containment of exfiltration.
