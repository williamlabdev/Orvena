# Provider parity — the Anthropic + Ollama consistency check

**Status:** harness in place · Ollama + Gemini + openai_compat demonstrated · **Anthropic never run**
**Updated:** 2026-07-31 (openai_compat leg added)

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

# openai_compat (generic OpenAI-compatible endpoint) — checked here against
# Ollama's own OpenAI-compat endpoint: no separate server to stand up, no key.
ORVENA_PARITY_PROVIDER=openai_compat \
  ORVENA_PARITY_BASE_URL=http://localhost:11434/v1 \
  ORVENA_PARITY_MODEL=qwen3:14b \
  cargo test -p orvena-core --test provider_parity -- --ignored --nocapture

# openai_compat against a real self-hosted OSS server (vLLM, llama.cpp server,
# LM Studio, ...) or a hosted open-weight aggregator (Groq shown): set
# ORVENA_PARITY_API_KEY_ENV to the env var holding the key, or omit it
# entirely for a no-auth local server.
GROQ_API_KEY=gsk_... \
  ORVENA_PARITY_PROVIDER=openai_compat \
  ORVENA_PARITY_BASE_URL=https://api.groq.com/openai/v1 \
  ORVENA_PARITY_MODEL=llama-3.3-70b-versatile \
  ORVENA_PARITY_API_KEY_ENV=GROQ_API_KEY \
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
Ollama host, or required for `openai_compat`), `ORVENA_PARITY_API_KEY_ENV`
(optional — `openai_compat` only, names the env var holding the key; omit for
a no-auth endpoint). With no `ORVENA_PARITY_PROVIDER` set the test skips
cleanly, so `cargo test -- --ignored` never fails just because parity env is
absent.

## Current status

| Provider  | Status | Evidence |
| :-------- | :----- | :------- |
| **Ollama** (local model) | ✅ demonstrated · re-verified 2026-07-30 | golden task with `qwen3:14b` completes: gate `hello-exists` passes, real token usage reported (`steps=1`, 373 tok), evidence bundle round-trips |
| **openai_compat** (generic, checked via Ollama's OpenAI-compat endpoint) | ✅ demonstrated 2026-07-31 | `qwen3:14b` via `http://localhost:11434/v1`, no `api_key_env` set (unauthenticated), passes the same contract (`steps=1`, 403 tok, gate `hello-exists` passes) |
| **Gemini** (hosted, OpenAI-compat) | ✅ demonstrated · re-verified 2026-07-30 | `gemini-2.5-flash` via Google's OpenAI-compatible endpoint passes the same contract (`steps=1`, 281 tok; Gemini key in `OPENAI_API_KEY`) |
| **Anthropic** (hosted)   | ◻ **never run** | no `ANTHROPIC_API_KEY` has been available on a bench machine to date. The code path exists and is expected to work — but nobody has executed it, and this table will not imply otherwise |
| **OpenAI** (hosted, native endpoint) | ◻ never run | exercised only via the base-URL override above (Gemini), never against OpenAI's own endpoint |
| **OpenRouter** (hosted) | ◻ never run | — |
| **openai_compat against a real self-hosted OSS server** (vLLM, llama.cpp server, LM Studio, ...) | ◻ never run | only checked so far against Ollama's own OpenAI-compat endpoint (a real server, but not a genuinely different backend from the `ollama` leg above) |

**Cross-provider consistency is demonstrated:** the harness passes the same
behavioral contract on three *real* providers — two local (Ollama native and
`openai_compat` via Ollama) and one hosted (Gemini) — which satisfies the
MVP-exit consistency check (Gemini stands in for Anthropic). The Ollama and
Gemini legs were re-verified 2026-07-30 (see their table rows); the
`openai_compat` leg was added and run 2026-07-31.

**On Anthropic specifically.** MVP-SCOPE §1 names Anthropic by name, and the
README used to call it the recommended first run — while it had never actually
been executed. That is exactly the kind of unearned claim this project's
benchmark pages refuse to make, so the recommendation is gone: the README now
marks which providers were parity-checked and which were not. Closing this is
one command with a key in the environment; until someone runs it, "never run"
is the honest entry.
