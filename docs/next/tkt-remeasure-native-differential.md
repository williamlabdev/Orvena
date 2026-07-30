# Ticket: re-measure the native differential under the revised envelope

> Status: OPEN · opened 2026-07-30 · follow-up to slice-019

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
