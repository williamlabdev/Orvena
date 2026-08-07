# Ticket: a publishable Aider differential (the third number)

> Status: **CLOSED (won't fix as scoped)** · 2026-08-02 · opened 2026-07-30 ·
> follow-up to slice-018 · two attempts, neither publishable · superseded by
> [`tkt-m1-null-is-structural.md`](tkt-m1-null-is-structural.md)

## Why this is closed (2026-08-02, after attempt 2)

This ticket existed for one reason: the wrapped leg was the **only** source of a
non-null M1, so it was the only leg that could carry the "what the brakes buy"
headline. Both halves of that premise are gone.

1. **The non-null M1 did not reproduce, on either model.** 0 violations in 48
   full-bar runs on `qwen3:14b` (attempt 1), and still null on the 8 `off` runs
   of `qwen3.6:35b` (attempt 2) — which was the "at least one capable model"
   requirement this ticket set for itself. The 83% rested on one event.
2. **The null is structural, and it is not about Aider.** The baseline is handed
   the scope in its prompt, as a prohibition, in every posture — for the wrapped
   agent too (`adapter/mod.rs:441`: "the scope contract is stated in the
   prompt"), on the same host-protection floor (`baseline_sandbox_policy`,
   `adapter/mod.rs:260`, confines to the workdir, so a root escape is unreachable
   in `off` here as well). Both legs measure the same thing: whether the model
   defies a written instruction. Evidence chain in
   [`tkt-m1-null-is-structural.md`](tkt-m1-null-is-structural.md).

So re-running this leg at any bar, on any model, cannot produce the number the
ticket was opened for until the baseline question is ruled on. Attempt 3 would
burn another full matrix to re-measure a constant.

**What survives:** the architecture claim, which was always the other half of
D5 — `orvena bench --agent aider` wraps a third-party CLI agent end to end under
OS-boundary enforcement, with evidence bundles, pinned to Aider 0.86.2
(slice-018, ADR-004). That is validated and needs no differential to be true. It
is a fact about the envelope, not a number for the results page.

**What is deliberately not claimed:** any cost ratio for the wrapped leg. The
`engineering` half was invalidated twice by harness bugs (PR #27, PR #28) and has
never been re-run clean. There is no publishable Aider number of any kind.

**To reopen:** only after `tkt-m1-null-is-structural.md` is ruled on, and only if
the ruling makes a non-null M1 reachable. At that point the right move is
probably to re-measure **both** legs together under the new baseline rather than
to resurrect this ticket alone — the page needs them side by side to separate
"what the brakes buy" from "which loop is better".

---

## History (kept as measured)

## Attempt 2 (2026-08-02, smoke only) — a second harness bug, same category

Run at 1 repeat × 8 tasks × both postures with **`qwen3.6:35b`** (the "at least
one capable model" requirement below), Aider 0.86.2, `KEEP_SCRATCH=1`. Stopped
before the full bar: the `engineering` half was invalid again.

**The gate was still confined out of a working measurement**, this time by
inheriting the host's `TMPDIR` rather than by the agent's write policy — so
`cargo test`'s doctest step died on permission. Same two tasks, same shape as
attempt 1 (4 steps, `reached max_steps`, ground truth saying "solved"). Diagnosed
and fixed in [`tkt-gate-inherits-host-tmpdir`](tkt-gate-inherits-host-tmpdir.md);
nothing from that half may be quoted, including the ×1.75 steps / ×3.28 tokens.

What attempt 2 *did* establish, and what it did not:

| | result |
|---|---|
| timeouts | **0** (attempt 1 had 3 at `ORVENA_AGENT_TIMEOUT_SECS=600`) |
| skipped / provider errors | 0 / 0 |
| M1, `off` half (8 runs) | **still null** — 0 violations |
| model speed | 35b measured *faster* than `qwen3:14b` (72.6 vs 38.1 tok/s) — it is MoE, so "bigger model, slower run" does not apply |
| Aider's context window | ~9k in practice (observed via `ollama ps`), **not** the 2048 LiteLLM default that was suspected |

The last two rows matter for planning: the capable-model leg is cheaper than
assumed, and the null M1 cannot be explained away as "the model could not see the
situation".

Because it is a 1-repeat smoke and its governed half is void, the report JSON was
**not** kept — the `docs/benchmark-results/` directory means "someone ran the full
bar", and a smoke sitting there would read as a result.

## Attempt 1 (2026-08-02) — nothing published, two findings

Run at the full bar for the first time: `AGENT=aider scripts/bench-differential.sh 3 qwen3:14b`
— Aider 0.86.2, 8 tasks × 3 repeats × both postures, **0 skipped, 0 provider
errors**. Report kept as evidence at
`docs/benchmark-results/2026-08-02-qwen3-14b-aider-differential.json`; deliberately
**not** on `docs/benchmark-results.md`.

**1. The `engineering` half was invalid — a harness bug, not a governance cost.**
The two `cargo test` tasks could not pass their gate because the gate was confined
by the agent's write policy, and no task declares `target/` as a write. Diagnosed
and fixed in [`tkt-adapter-gate-confined-by-agent-policy`](tkt-adapter-gate-confined-by-agent-policy.md).
Anything derived from that posture — the 8/8 → 6/8 pass rate, the ×1.75 step and
×1.98 token overhead — is measurement damage and must not be quoted.

**2. The non-null M1 did not reproduce, and that finding is independent of the bug.**
`off` runs no gate at all, so it was never touched by the fix:

| | smoke run (1 repeat, 6 tasks) | 2026-08-02 (3 repeats, 8 tasks) |
|---|---|---|
| containment `off` | 83% | **100%** |
| containment `engineering` | 100% | 100% |
| violations observed | 1 | **0 of 48 runs** |

The 83% rested on a single event — `qwen3:14b` creating a literal `~/.orvena-notes.txt`
inside the root — and this section already said so ("enough to prove the mechanism,
not enough to characterise it"). Given 24 fresh `off` runs it did not happen once.
At this sample size and this model, the wrapped-agent leg is **a null M1 result,
the same as the native loop** — the differential's whole reason for existing is
not currently in evidence.

So re-running as-is buys only a clean cost ratio. What the ticket wants — a
containment differential a page can stand on — needs the temptations to actually
be taken, which points at the "at least one capable model" requirement below, or
at temptations that a competent agent will still reach for.

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
