# How wrong is `estimate_tokens`?

> Measured 2026-07-30 against the 0.1.0 tree. Reproduce with
> `scripts/measure-token-estimate.py`.

`crates/orvena-core/src/util.rs::estimate_tokens` is `ceil(chars / 4)`. It is the
**only** input to the per-role context budget in `agent/context.rs`, which decides
which in-scope files are truncated out of the prompt:

```rust
let cost = estimate_tokens(&block);
if used + cost > budget_tokens {
    user.push_str("(remaining files omitted: context budget reached)\n");
    break;
}
```

So the accuracy of this one function is the accuracy of the Controlled Context
pillar. This document measures it rather than assuming it.

## Method

`scripts/measure-token-estimate.py` with no arguments: the 97 files of this repo
(`.md`, `.rs`, `.py`, `.yaml`, 400 B – 200 kB), encoded with `tiktoken`
`cl100k_base` (GPT-4/3.5) and `o200k_base` (GPT-4o), compared against the
heuristic. Numbers below are from that default run so they can be reproduced
exactly; pass extra directories to widen the corpus.

A caveat that matters for interpretation: **there is no single ground truth.**
Orvena's parity-checked providers are Ollama (`qwen3:14b`) and Gemini
(`gemini-2.5-flash`); neither uses a tiktoken encoding. These two encodings are
a proxy — good enough to establish the *shape* and *magnitude* of the error, not
to calibrate a constant against.

`ratio = heuristic / actual`. **Below 1.0 is the dangerous direction**: the budget
believes it has room it does not have, admits another file, and the assembled
prompt overruns the real budget.

## Result

| Content | ratio (cl100k) | ratio (o200k) | Reading |
|---|---|---|---|
| Rust source | 1.01 | 1.01 | Essentially exact |
| English prose (README) | 0.98 | 1.00 | Essentially exact |
| YAML | 0.97 | 0.97 | Fine |
| Python source | 1.12 | 1.12 | Over-estimates — safe direction |
| **Docs containing Chinese** | **0.37 – 0.54** | **0.48 – 0.68** | **Under-estimates 2–2.7×** |

Aggregate over the corpus: **0.878** (cl100k) / **0.922** (o200k). Widening it to
both repos (2,622 files, mostly source) moves it to 0.947 / 0.959 — the aggregate
is dominated by whichever content type is most numerous, which is exactly why the
per-category rows above matter more than the total.

### The failure mode is CJK, and it is not marginal

`chars / 4` encodes an assumption about *English*. A Chinese character costs
roughly 1–1.5 tokens on its own, so the heuristic under-counts it by 4–6×.

| File | chars | heuristic | cl100k actual | ratio |
|---|---|---|---|---|
| `docs/benchmark-governance-differential-plan.md` | 5,627 | 1,407 | 3,768 | 0.37 |
| `MVP-SCOPE.md` | 5,141 | 1,286 | 3,313 | 0.39 |
| `docs/adr/ADR-001-shell-tool-security-model.md` | 6,706 | 1,677 | 4,002 | 0.42 |
| `README.md` | 9,402 | 2,351 | 4,320 | 0.54 |

These files are only 14–27 % Chinese by character count and are still 2–2.7×
under-counted, because the Chinese characters dominate the token count even when
they are the minority of characters.

This is not hypothetical for this repo: `MVP-SCOPE.md`, all three ADRs and the
SLICE documents are written in Chinese. Any of them entering a role's context
scope is silently charged less than half its real cost.

Source code — the dominant case for a coding agent — is accurate to ~1 %. **The
heuristic is fine for what Orvena mostly does and wrong for this project's own
design documents.**

## A fix that needs no dependency

Weighting CJK characters at 1 token and everything else at ¼:

```rust
pub fn estimate_tokens(text: &str) -> u32 {
    let (cjk, other) = text.chars().fold((0u32, 0u32), |(c, o), ch| {
        if is_cjk(ch) { (c + 1, o) } else { (c, o + 1) }
    });
    (cjk as f32 + other as f32 / 4.0).ceil() as u32
}
```

Measured against the same corpus:

| | aggregate ratio (cl100k) | aggregate ratio (o200k) |
|---|---|---|
| current `chars/4` | 0.878 | 0.922 |
| CJK-aware, weight 1.0 | 0.946 | 0.993 |
| CJK-aware, weight 1.3 | 0.974 | 1.022 |

Weight 1.0 is the principled pick (one CJK char ≈ one token is the standard rule
of thumb) and lands `o200k_base` within 1 % of parity. Weight 1.3 closes the
`cl100k_base` gap further but starts over-estimating on `o200k_base`.

It stays offline, deterministic and dependency-free, so the properties `util.rs`
was designed around are preserved.

## Why this has not been changed yet

`util.rs` argues that for L1 regression metrics *consistency* matters more than
absolute accuracy — and that is right. Every recorded baseline under
`data/metrics/baseline/` was captured with `chars/4`, including the in-flight
`abl-gate-003` cells. Changing the accounting mid-study makes new token numbers
incomparable with recorded ones.

Recommendation, in order:

1. **Do not change it during the current ablation wave.** The bias is systematic
   and applies equally to both arms of a governed/ungoverned comparison, so it
   does not distort the differential the study measures.
2. **Adopt the CJK-aware version at the next baseline reset**, and record the
   changeover in `CHANGELOG.md` so old and new token figures are not compared by
   accident.
3. **Leave tiktoken out.** It would only be accurate for providers Orvena has
   never run a parity check against, in exchange for a build-time dependency and
   the loss of offline determinism. The measurement above does not justify it.
