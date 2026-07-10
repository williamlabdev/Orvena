# Ticket: Evidence bundle must be written on every exit path

> Status: PLANNED · 2026-07-10 · diagnosis-to-fix, no code changed

## Problem

The core promise codified in `crates/orvena-core/src/metrics/evidence.rs:3-4` —
"every run — completed or stopped by a gate — leaves an auditable artifact" —
only holds today when a run is stopped by a gate. It does not hold when a run
aborts for any other reason, and the artifact it does write is neither atomic
nor complete.

- **Bundle is only written on the `Ok` path.** `crates/orvena-cli/src/commands/run.rs:44-53`
  calls `agent.run(...).await?` (line 45) and only reaches `evidence::write_bundle`
  (line 53) if the run returned `Ok`. Any error propagated out of the bounded loop
  in `crates/orvena-core/src/agent/driver.rs:55-226` — a provider error via `?`
  (driver.rs:87), a `panic!`, or a Ctrl-C interrupt — short-circuits the `?` at
  run.rs:45 and bails *before* `write_bundle` ever runs. Result: no bundle. The
  "evidence by default" guarantee silently degrades to "evidence only when the
  loop returns Ok".

- **The write is non-atomic.** `crates/orvena-core/src/metrics/evidence.rs:37-43`
  serializes and writes straight to the final path (`std::fs::write` at
  evidence.rs:41) with no temp-file + rename. A crash or `kill -9` mid-write can
  leave a truncated / invalid-JSON `evidence.json`.

- **Only the last step's gate outcomes survive.** `crates/orvena-core/src/agent/driver.rs:184`
  calls `report.gate_outcomes.clear()` at the top of every step's gate check, so a
  multi-step run's bundle retains only the final round's gate history; earlier
  rounds are discarded.

- **Run directories can collide.** `crates/orvena-cli/src/commands/mod.rs:79-85`
  (`run_timestamp`) names each run directory by milliseconds since the epoch. Two
  back-to-back runs that land in the same millisecond resolve to the same
  `runs/<timestamp>/` path, so one run's evidence overwrites the other's.

## Fix direction

- Ensure a bundle (at minimum a partial one from the report held so far) is
  written on error, panic, and interrupt exits — not only on the `Ok` path.
- Make the on-disk write atomic (write to a temp file, then rename into place) so
  an interrupted write never yields an invalid-JSON artifact.
- Preserve per-step gate outcomes across the whole run rather than retaining only
  the final step's.
- Make run-directory names collision-proof (e.g. add a sequence, pid, and/or
  random suffix) so same-millisecond runs cannot overwrite each other.

## Acceptance criteria

- [ ] A run aborted by a provider error still leaves an evidence bundle on disk.
- [ ] A run interrupted by Ctrl-C still leaves an evidence bundle on disk.
- [ ] A `kill -9` during the bundle write never leaves an invalid / partially
      written `evidence.json` at the final path.
- [ ] A multi-step run's bundle retains the gate outcomes from every step, not
      only the last.
- [ ] Two runs that start within the same millisecond produce two distinct run
      directories; neither overwrites the other.
