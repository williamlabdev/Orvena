# Ticket: a benchmark run that mostly failed still reports a headline number

> Status: DONE · 2026-07-30 · slice-017 · found while attempting the hosted
> differential leg (D6)

## Problem

`orvena bench` computes and prints its headline metrics — pass rate, ground-truth
verified rate, false-done rate, containment, and the governance differential —
without regard to whether the runs behind them actually reached the model. A run
that died on a provider error is folded into every denominator as if it were a
legitimate result.

This is not hypothetical. The 2026-07-30 attempt at the hosted leg
(`gemini-2.5-flash`, 8 tasks × 2 postures × 3 repeats) exhausted a free-tier
quota partway through. **39 of 48 task-runs ended in `429 Too Many Requests`**
(18/24 under `off`, 21/24 under `engineering` — 18 and 19 of them never spent a
single token). The harness reported this without a murmur:

```
mean pass rate: 4%   ground truth: 17% verified   false-done: 100% of claims
mean pass rate: 12%  ground truth: 12% verified   false-done: 0% of claims
── governance differential (engineering vs off) ──
false-done: off: 100% of claims → engineering: 0% of claims
overhead:   ×0.74 steps, ×0.41 tokens (governed / baseline)
```

Every one of those figures is an artifact of the outage. "False-done 100% → 0%"
rests on a single surviving claim. "×0.41 tokens" is the ratio of two mostly-zero
means. A reader — including a future maintainer reading a committed report JSON —
has no way to tell this apart from a real result without opening the raw file and
counting blockers by hand.

The blockers *are* recorded per task-run, so the data is not lost. The defect is
that nothing propagates it into the aggregate, and nothing refuses to publish.

Note that the retry work merged in #12 is not at fault and did its job: transient
429s are retried with the server's own hint. A *quota-exhausted* 429 is not
transient — no backoff can fix it, and the run should be abandoned, not averaged.

## Fix direction

- Count provider-error task-runs per posture and carry the count into
  `BenchReport` / `RepeatedReport` / `MatrixReport` as a first-class field
  (alongside `skipped`, which already has this shape and precedent).
- Exclude them from the metric denominators, the way `skipped` already is — a run
  that never reached the model is not evidence about governance.
- Above a threshold of dead runs, **refuse to report a differential at all**
  rather than print a degraded one. The de-glamorization posture in
  `docs/benchmark-results.md` says a weak number gets published with caveats; a
  number that measures an API outage is not weak, it is invalid, and belongs
  behind a hard stop.
- Print the count in the CLI summary unconditionally, so a partly-failed run is
  visible without opening the JSON.

## Acceptance criteria

- [x] `BenchReport`/`RepeatedReport` carry a provider-error count per posture,
      and it round-trips through the report JSON. Detection is a structured
      `RunReport.provider_error` set by the driver at the capture site — not a
      string match on `blockers`. Additive optional field, so the evidence
      schema stays v1 (its own policy: additive keeps v1).
- [x] Provider-error runs are excluded from pass-rate, verified, false-done, and
      containment denominators — plus evidence-validity and the per-task pass
      rate, which had the same flaw. Aggregation was extracted into a pure
      `aggregate()` so the counting rules are testable without a provider.
- [x] A matrix whose dead-run share exceeds the threshold reports no differential
      and says why (`MatrixReport.differential_suppressed`, threshold
      `MAX_DEAD_RUN_SHARE = 20%`, checked on **both** postures — one healthy side
      cannot carry a broken one).
- [x] The CLI summary prints the provider-error count whenever it is non-zero.
- [x] Regression tests: 8 unit tests over the pure aggregation and suppression
      rules, plus a driver-level assertion that a failing provider sets the
      structured flag and that it survives the bundle round-trip.

> Tested at the aggregation layer rather than by stubbing a partially-failing
> provider end to end: `run_benchmark` builds its own agent, so an end-to-end
> fault would have needed a fault-injection seam on a shipped provider. The
> pure-function tests cover the rules that were actually wrong, and the driver
> test covers detection. The full path was also exercised by hand against a
> deliberately invalid key — 8/8 dead, differential correctly withheld.

## One thing the fix surfaced

Suppressing the differential was not enough. With every run dead, the summary
still printed `containment (oracle): 0%` and `evidence valid: 0%` — 0% reading as
"the agent scored zero" when it meant "we never found out". That is the same
defect one level down, so the summary now refuses to print any rate when nothing
was measured, and per-task lines read `— (no run reached the model)`.

## Also blocked, same session

The **hosted differential leg (D6)** remains unpublished. The available key is a
free-tier Gemini key whose quota (`generate_content_free_tier_requests`,
limit 20) cannot sustain a 48-task-run matrix even at 6s pacing. Closing D6 needs
a key with real quota — any hosted provider — not more retry logic.

## Settled: the role allowlist in the ungoverned baseline (not a bias)

Seven `off`-posture runs recorded `scope violation: role 'developer' is not
allowed to use 'shell.run'`, which raised the question of whether the
"ungoverned" baseline is really ungoverned. Traced through:

- The per-role tool allowlist is enforced in the **tool layer**
  (`tools/shell.rs:71`, `fs.rs:102`, `grep.rs:116` — `require_tool`), which never
  consults `LoopOptions.ungoverned`. That flag lifts only the scope lists and
  skips gates (`agent/driver.rs:83`, `:239`). So yes — the allowlist still
  applies to the baseline.
- **But it applies identically to every posture.** `benchmark::bench_config`
  builds the same role for `off`, `light`, and `engineering`:
  `allowed_tools: [fs.read, fs.write, grep.search]`. `shell.run` is granted to
  none of them.

So the allowlist is a **constant across the comparison, not a variable** — it
does not bias the differential in either direction, and the published
2026-07-11 number is unaffected. The blockers are just the model reaching for a
tool the benchmark's role never grants, under either posture.

What remains is a **task-design** question, not a correctness one: a baseline
that cannot invoke a shell is a weaker stand-in for "an agent with no brakes"
than one that can. Granting the baseline more tools would raise its ceiling for
misbehavior and could widen M1 — worth considering when the temptation set is
next revised, and worth stating in any page that publishes a containment number.

> **Resolved 2026-07-30 (slice-019).** The benchmark role now has `shell.run`,
> and every task declares its `verify` as a read-only `check` command (plus
> per-task observation commands) — **identically in every posture**, so the
> allowlist stays a constant of the comparison rather than becoming a variable.
> It went further than "more tools": the prompt only ever showed the *writable*
> files, so a read-only neighbor was invisible even to a shell-capable role. The
> practical effect is that the baseline can now see and run the check it is asked
> to fix. That makes it stronger and shrinks our own differential, which is the
> honest direction. `docs/benchmark-results.md` carries the note that numbers
> measured before this are not comparable to ones measured after.
