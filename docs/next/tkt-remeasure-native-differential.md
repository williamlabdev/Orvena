# Ticket: re-measure the native differential under the revised envelope

> Status: DONE · 2026-08-02 · opened 2026-07-30 · follow-up to slice-019

## Outcome (2026-08-02)

Measured at the stated bar (`scripts/bench-differential.sh 3 qwen3:14b`, 8 tasks
× 3 repeats × both postures, 0 skipped, 0 provider errors) and published as a
new dated section on `docs/benchmark-results.md`; the 2026-07-11 section is
unedited behind a kept-as-history banner.

The ticket's central bet — "if M2 collapses, publish that" — is what happened,
with a twist worth recording: **M2 did not shrink, it vanished** (25% → 0%
became 0% → 0%), and the cause is not a more honest baseline but a
**non-claiming** one. 18 of the baseline's 24 runs hit `max_steps` still
emitting actions without ever declaring done; 12 of those had already produced
verifiably correct files. Giving the ungoverned agent a shell gave it more to do
on the way to exhausting its budget, not a sense of when it was finished.

Also recorded: the old 25% was 2 false claims out of 8 — a denominator the
results page never disclosed. It does now, in both sections.

What replaced the lost headline: **ground-truth solve rate 75% → 92%** (79% in
both postures on 2026-07-11), at **×0.36 steps / ×0.24 tokens**.

And a conjecture retired: **M1 stayed 100%/100%**. The 2026-07-30 note on the
results page guessed the containment null was partly an artifact of the weak
envelope. The envelope is fixed and the null persists for the native loop on
this model, so that note has been revised again rather than left standing. The
non-null containment number remains the wrapped-Aider one — a fact about that
agent, not evidence about this measurement.

Follow-up: [`tkt-aider-differential-publishable.md`](tkt-aider-differential-publishable.md)
— the wrapped-agent leg.

**Corrected (2026-08-02, later the same day):** this section originally read "the
two now differ on M1 under an identical envelope". They do not. The wrapped leg
came back null at the full bar too, and the shared cause was then traced to the
baseline being handed the scope as a prompt-level prohibition in both postures —
see [`tkt-m1-null-is-structural.md`](tkt-m1-null-is-structural.md). The M4 and
solve-rate results above are unaffected; only the M1 reading changes.

## Why

The published governance differential (2026-07-11, `qwen3:14b`) was measured
when the benchmark's agent **could not run any command** and never saw the
read-only files it was being tempted by. Since slice-019 it gets a read-only
`check` (its own `verify`) plus per-task observation commands, identically in
every posture — and slice-019 also fixed an action-parser bug that turned a
model's one-line `<<<WRITE config.json>>` into an out-of-scope path, i.e. into a
**containment violation that never happened**.

Both changes move the numbers, in opposite directions and for different reasons:

- a baseline that can run the check is **stronger** → M2 (false-done) should
  shrink, possibly a lot: the old story was "without a gate the model cannot
  tell when it is done", and now it can go look;
- a parser that stops inventing paths **removes false violations** → M1 gets
  cleaner in both postures;
- steps/tokens change on both sides (an agent that runs the check spends more
  per attempt and may need fewer attempts), so M4 is not comparable either.

`docs/benchmark-results.md` currently carries a superseded-conditions note. That
is the honest interim state; the fix is a fresh measurement, not a caveat.

## What to do

- `scripts/bench-differential.sh 3 qwen3:14b` (the same bar as the published
  number: full 8-task set, 3 repeats, both postures).
- Publish as a **new dated section** on `docs/benchmark-results.md` rather than
  editing the old one — the 2026-07-11 numbers were honestly produced under the
  conditions of the time and stay on the page as history, with the note pointing
  forward to the new section.
- Say explicitly what changed between them (capability envelope + parser), so a
  reader can see why two numbers from the same model and set differ.
- If M2 collapses, publish that. A shrinking differential measured against a
  fairer baseline is the number being right, not the product being worse.

## Related

- `docs/next/tkt-aider-differential-publishable.md` — the wrapped-agent leg of
  the same page; ideally both land together so the page can separate "what the
  brakes buy" from "which loop is better".
