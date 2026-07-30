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

REPO="$(cd "$(dirname "$0")/.." && pwd)"
TASKS="$REPO/benchmarks/temptation.yaml"
DATE="$(date +%F)"
SAFE_MODEL="$(printf '%s' "$MODEL" | tr '/:' '--')"
OUT="$REPO/docs/benchmark-results/${DATE}-${SAFE_MODEL}-differential.json"

echo "== building orvena (release) =="
( cd "$REPO" && cargo build --release --quiet )
BIN="$REPO/target/release/orvena"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
cd "$WORK"

echo "== scaffolding a throwaway project in $WORK =="
"$BIN" init >/dev/null
sed -i.bak "s/kind: anthropic/kind: ${PROVIDER}/; s#model: claude-opus-4-8#model: ${MODEL}#" \
  .orvena/orvena.yaml && rm -f .orvena/orvena.yaml.bak
if [ -n "$BASE_URL" ]; then
  # The scaffold ships `base_url` commented out; replace that line so the
  # override lands inside the provider block rather than appended at the end.
  sed -i.bak "s|^  # base_url:.*|  base_url: ${BASE_URL}|" .orvena/orvena.yaml
  rm -f .orvena/orvena.yaml.bak
  grep -q "^  base_url: " .orvena/orvena.yaml \
    || { echo "error: failed to set base_url in the scratch config" >&2; exit 1; }
fi
if [ -n "$API_KEY_ENV" ]; then
  # Same trick for the commented-out api_key_env line.
  sed -i.bak "s|^  # api_key_env:.*|  api_key_env: ${API_KEY_ENV}|" .orvena/orvena.yaml
  rm -f .orvena/orvena.yaml.bak
  grep -q "^  api_key_env: " .orvena/orvena.yaml \
    || { echo "error: failed to set api_key_env in the scratch config" >&2; exit 1; }
fi

# The key lives in the repo's .env (git-ignored); the scratch project has none.
[ -f "$REPO/.env" ] && cp "$REPO/.env" .env

echo
echo "== sanity: offline dry-run (fast, deterministic, no model) =="
"$BIN" bench --provider offline --tasks "$TASKS" --governance off,engineering

echo
echo "== differential: ${PROVIDER} / ${MODEL} × ${REPEAT} runs × (off, engineering) =="
if [ -n "${ORVENA_MIN_REQUEST_INTERVAL_MS:-}" ]; then
  echo "   (pacing: ${ORVENA_MIN_REQUEST_INTERVAL_MS}ms minimum between requests)"
fi
echo "   (this calls the model many times; expect an hour-plus for a 14B model)"
"$BIN" bench --tasks "$TASKS" --governance off,engineering --repeat "$REPEAT" --out "$OUT"

echo
echo "Differential report written to:"
echo "  $OUT"
echo "The per-task evidence bundles (and their git baselines) live under the"
echo "scratch project's .orvena/bench/ and are deleted with it; the report JSON"
echo "retains every per-run result for audit."
