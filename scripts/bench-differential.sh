#!/usr/bin/env bash
# Produce the governance-differential number (M1–M4): the temptation set run
# under the ungoverned baseline AND the engineering tier, same model, same
# prompts — only enforcement differs. Run from anywhere; uses a throwaway
# scratch project for the .orvena config (your repo config is untouched).
#
#   Usage:  scripts/bench-differential.sh [REPEAT] [MODEL]
#     REPEAT  runs per task per mode      (default 3)
#     MODEL   model to drive              (default qwen3:14b)
#
#   Provider (env, so the positional args stay backward-compatible):
#     PROVIDER   anthropic | openai | openrouter | ollama | openai_compat
#                                                            (default ollama)
#     BASE_URL   endpoint override; required for openai_compat
#     API_KEY_ENV  env var holding the key (openai/openrouter/openai_compat).
#                  Omit on openai_compat for a keyless local server.
#     AGENT      native | aider                             (default native)
#
#   AGENT=aider measures a third-party CLI agent inside Orvena's envelope
#   (ADR-004): same tasks, same model, `off` = the agent unwrapped, `engineering`
#   = the agent spawned inside the OS sandbox with writable narrowed to the
#   task's declared paths. Needs `aider` on PATH. Note that only the filesystem
#   is contained — the wrapped agent must reach its own model provider — and that
#   its token counts are self-reported, not observed.
#
#   Local (default) — needs a local Ollama serving MODEL:
#     scripts/bench-differential.sh 3 qwen3:14b
#
#   Hosted — the key goes in .env / the environment, per docs/provider-parity.md.
#   Gemini rides `openai_compat` with a base-URL override and its own key var:
#     PROVIDER=openai_compat \
#     BASE_URL=https://generativelanguage.googleapis.com/v1beta/openai \
#     API_KEY_ENV=GEMINI_API_KEY \
#     ORVENA_MIN_REQUEST_INTERVAL_MS=6000 \
#       scripts/bench-differential.sh 3 gemini-2.5-flash
#
#   Self-hosted OSS server (vLLM, llama.cpp, LM Studio) with no auth:
#     PROVIDER=openai_compat BASE_URL=http://localhost:8000/v1 \
#       scripts/bench-differential.sh 3 <model-id>
#
#   On a rate-capped key set ORVENA_MIN_REQUEST_INTERVAL_MS: it paces requests on
#   a process-global clock, so the spacing holds across the per-task providers
#   the matrix rebuilds. 429/503 are retried honoring the server's own hint
#   regardless; the throttle just avoids earning them. ORVENA_MAX_RETRIES tunes
#   the retry count.
#
# Expect this to take a while: the ungoverned baseline tends to burn its full
# step budget (no gate ever tells it to stop), so a matrix run costs roughly
# (tasks × modes × REPEAT × max_steps) model calls.

set -euo pipefail

REPEAT="${1:-3}"
MODEL="${2:-qwen3:14b}"
PROVIDER="${PROVIDER:-ollama}"
BASE_URL="${BASE_URL:-}"
API_KEY_ENV="${API_KEY_ENV:-}"
AGENT="${AGENT:-native}"

REPO="$(cd "$(dirname "$0")/.." && pwd)"
TASKS="$REPO/benchmarks/temptation.yaml"
DATE="$(date +%F)"
SAFE_MODEL="$(printf '%s' "$MODEL" | tr '/:' '--')"
# The agent is part of the number's identity, so it is part of the filename.
if [ "$AGENT" = "native" ]; then
  OUT="$REPO/docs/benchmark-results/${DATE}-${SAFE_MODEL}-differential.json"
else
  OUT="$REPO/docs/benchmark-results/${DATE}-${SAFE_MODEL}-${AGENT}-differential.json"
fi

echo "== building orvena (release) =="
( cd "$REPO" && cargo build --release --quiet )
BIN="$REPO/target/release/orvena"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
cd "$WORK"

echo "== scaffolding a throwaway project in $WORK =="
# `init` writes the provider block itself, so the config is never pattern-matched
# into shape. It also refuses an unknown kind or a missing base_url up front,
# which is a better failure than a scratch project that quietly runs the wrong
# provider. `--provider` never prompts, so this is safe when backgrounded.
INIT_ARGS=(init --provider "$PROVIDER" --model "$MODEL")
if [ -n "$BASE_URL" ]; then
  INIT_ARGS+=(--base-url "$BASE_URL")
fi
if [ -n "$API_KEY_ENV" ]; then
  INIT_ARGS+=(--api-key-env "$API_KEY_ENV")
fi
"$BIN" "${INIT_ARGS[@]}" >/dev/null

# The key lives in the repo's .env (git-ignored); the scratch project has none.
[ -f "$REPO/.env" ] && cp "$REPO/.env" .env

echo
echo "== sanity: offline dry-run (fast, deterministic, no model) =="
if [ "$AGENT" = "native" ]; then
  "$BIN" bench --provider offline --tasks "$TASKS" --governance off,engineering
else
  echo "   (skipped: the offline stub is a native-loop fixture; an external agent brings its own model client)"
fi

echo
echo "== differential: ${AGENT} agent / ${PROVIDER} / ${MODEL} × ${REPEAT} runs × (off, engineering) =="
if [ -n "${ORVENA_MIN_REQUEST_INTERVAL_MS:-}" ]; then
  echo "   (pacing: ${ORVENA_MIN_REQUEST_INTERVAL_MS}ms minimum between requests)"
fi
echo "   (this calls the model many times; expect an hour-plus for a 14B model)"
"$BIN" bench --tasks "$TASKS" --agent "$AGENT" --governance off,engineering --repeat "$REPEAT" --out "$OUT"

echo
echo "Differential report written to:"
echo "  $OUT"
echo "The per-task evidence bundles (and their git baselines) live under the"
echo "scratch project's .orvena/bench/ and are deleted with it; the report JSON"
echo "retains every per-run result for audit."
