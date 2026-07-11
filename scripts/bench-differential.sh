#!/usr/bin/env bash
# Produce the governance-differential number (M1–M4): the temptation set run
# under the ungoverned baseline AND the engineering tier, same model, same
# prompts — only enforcement differs. Run from anywhere; uses a throwaway
# scratch project for the .orvena config (your repo config is untouched).
#
#   Usage:  scripts/bench-differential.sh [REPEAT] [MODEL]
#     REPEAT  runs per task per mode      (default 3)
#     MODEL   Ollama model to drive       (default qwen3:14b)
#
#   Requires a local Ollama serving MODEL. For a hosted model, set the provider
#   and key per docs/provider-parity.md and adjust PROVIDER below.
#
# Expect this to take a while: the ungoverned baseline tends to burn its full
# step budget (no gate ever tells it to stop), so a matrix run costs roughly
# (tasks × modes × REPEAT × max_steps) model calls.

set -euo pipefail

REPEAT="${1:-3}"
MODEL="${2:-qwen3:14b}"
PROVIDER="ollama"

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

echo
echo "== sanity: offline dry-run (fast, deterministic, no model) =="
"$BIN" bench --provider offline --tasks "$TASKS" --governance off,engineering

echo
echo "== differential: ${PROVIDER} / ${MODEL} × ${REPEAT} runs × (off, engineering) =="
echo "   (this calls the model many times; expect an hour-plus for a 14B model)"
"$BIN" bench --tasks "$TASKS" --governance off,engineering --repeat "$REPEAT" --out "$OUT"

echo
echo "Differential report written to:"
echo "  $OUT"
echo "The per-task evidence bundles (and their git baselines) live under the"
echo "scratch project's .orvena/bench/ and are deleted with it; the report JSON"
echo "retains every per-run result for audit."
