#!/usr/bin/env bash
# Produce a pass-rate benchmark number for Orvena (repeated runs de-noise a
# stochastic model). Run this from the repo root; it does NOT touch your repo's
# config — it uses a throwaway scratch project for the .orvena config.
#
#   Usage:  scripts/bench-passrate.sh [REPEAT] [MODEL]
#     REPEAT  number of runs per task   (default 5)
#     MODEL   Ollama model to drive     (default qwen3:14b)
#
#   Requires a local Ollama serving MODEL. To benchmark a hosted model instead,
#   see docs/benchmark.md (set the provider + key and adjust below).

set -euo pipefail

REPEAT="${1:-5}"
MODEL="${2:-qwen3:14b}"
PROVIDER="ollama"

REPO="$(cd "$(dirname "$0")/.." && pwd)"
TASKS="$REPO/benchmarks/realworld.yaml"
DATE="$(date +%F)"
SAFE_MODEL="$(printf '%s' "$MODEL" | tr '/:' '--')"
OUT="$REPO/docs/benchmark-results/${DATE}-${SAFE_MODEL}-passrate.json"

echo "== building orvena (release) =="
( cd "$REPO" && cargo build --release --quiet )
BIN="$REPO/target/release/orvena"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
cd "$WORK"

echo "== scaffolding a throwaway project in $WORK =="
"$BIN" init >/dev/null
# point the scratch config at your local Ollama model (does not affect your repo)
sed -i.bak "s/kind: anthropic/kind: ${PROVIDER}/; s#model: claude-opus-4-8#model: ${MODEL}#" \
  .orvena/orvena.yaml && rm -f .orvena/orvena.yaml.bak

echo
echo "== sanity: offline dry-run (fast, deterministic, no model) =="
"$BIN" bench --provider offline --tasks "$TASKS" --repeat 2

echo
echo "== pass-rate: ${PROVIDER} / ${MODEL} × ${REPEAT} runs per task =="
echo "   (this calls the model many times; expect several minutes)"
"$BIN" bench --provider "$PROVIDER" --tasks "$TASKS" --repeat "$REPEAT" --out "$OUT"

echo
echo "Pass-rate report written to:"
echo "  $OUT"
echo "Review it, then paste the summary back and I'll update docs/benchmark-results.md."
