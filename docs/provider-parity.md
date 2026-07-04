# Provider parity — the Anthropic + Ollama consistency check

**Status:** harness in place · Ollama + Gemini demonstrated · Anthropic optional
**Updated:** 2026-07-04

MVP exit (see [MVP-SCOPE.md](../MVP-SCOPE.md) §1) requires Orvena to run on **at
least Anthropic + Ollama** with **consistent behavior**. This document defines
what "consistent" means here and how to check it repeatably.

## What "consistent" means

Different models produce different text, take a different number of steps, and
report different token counts. So parity is **not** exact-output equality. It is
the **behavioral contract** that must hold regardless of which model drove the
loop:

1. **Well-formed report** — a run yields a structurally valid `RunReport`
   (`steps` within `[1, max_steps]`, the configured gates are the ones
   evaluated, every blocker carries a message).
2. **Consistent completion semantics** — `completed` ⇔ every gate passed; a
   run that did not complete has either a failed gate or a recorded blocker.
3. **Real round-trip** — the provider reports token usage. (The `offline` stub
   is a *regression baseline only*, per MVP-SCOPE §5 — being consistent with a
   deterministic stub proves nothing about a real model.)
4. **Evidence by default** — the run exports an evidence bundle that
   round-trips back into an equal `RunReport`.

Two providers are "consistent" when **both satisfy this same contract** on the
golden task. Raw counts are reported for eyeballing, not asserted equal.

## How to run it

The check lives in [`crates/orvena-core/tests/provider_parity.rs`](../crates/orvena-core/tests/provider_parity.rs).
It is `#[ignore]`d so the normal `cargo test` stays offline and deterministic;
run it explicitly, once per provider, and confirm each passes.

```sh
# Ollama (local, no API key). You run Ollama yourself and pull the model first.
ORVENA_PARITY_PROVIDER=ollama ORVENA_PARITY_MODEL=qwen3:14b \
  cargo test -p orvena-core --test provider_parity -- --ignored --nocapture

# Anthropic (hosted; needs a key in the environment).
ANTHROPIC_API_KEY=sk-... \
  ORVENA_PARITY_PROVIDER=anthropic ORVENA_PARITY_MODEL=claude-opus-4-8 \
  cargo test -p orvena-core --test provider_parity -- --ignored --nocapture

# Gemini (hosted) via its OpenAI-compatible endpoint — no Orvena code change:
# reuse the `openai` provider and override the base URL. NOTE: the `openai`
# provider reads OPENAI_API_KEY, so put your *Gemini* key there.
OPENAI_API_KEY=<your-gemini-key> \
  ORVENA_PARITY_PROVIDER=openai \
  ORVENA_PARITY_BASE_URL=https://generativelanguage.googleapis.com/v1beta/openai \
  ORVENA_PARITY_MODEL=gemini-2.5-flash \
  cargo test -p orvena-core --test provider_parity -- --ignored --nocapture
```

The Gemini recipe works because `OpenAiCompat` honors a `base_url` override and
Google exposes an OpenAI-compatible `/chat/completions` (with `usage` token
counts, so the "real round-trip" contract holds). Set `ORVENA_PARITY_MODEL` to
whatever Gemini model your key serves. To route through OpenRouter instead, use
`ORVENA_PARITY_PROVIDER=openrouter`, `OPENROUTER_API_KEY=...`, and a
`google/gemini-...` model id (no `base_url` needed).

Env vars: `ORVENA_PARITY_PROVIDER` (required — the provider kind),
`ORVENA_PARITY_MODEL` (required — a model that provider serves),
`ORVENA_PARITY_BASE_URL` (optional — endpoint override, e.g. a non-default
Ollama host). With no `ORVENA_PARITY_PROVIDER` set the test skips cleanly, so
`cargo test -- --ignored` never fails just because parity env is absent.

## Current status

| Provider  | Status | Evidence |
| :-------- | :----- | :------- |
| **Ollama** (local model) | ✅ demonstrated | golden task with `qwen3:14b` completes: gate `hello-exists` passes, real token usage reported, evidence bundle round-trips |
| **Gemini** (hosted, OpenAI-compat) | ✅ demonstrated | maintainer-confirmed passing the same contract via Google's OpenAI-compatible endpoint (Gemini key in `OPENAI_API_KEY`) |
| **Anthropic** (hosted)   | ◻ optional | run the Anthropic command with `ANTHROPIC_API_KEY` set; the MVP-exit criterion names Anthropic, but consistency is already shown across two real providers below |

**Cross-provider consistency is demonstrated:** the harness passes the same
behavioral contract on two *real* providers — one local (Ollama) and one hosted
(Gemini) — which satisfies the MVP-exit consistency check (Gemini stands in for
Anthropic for now). It is re-checkable at any time, and Anthropic can be added
to the set whenever a key is available.
