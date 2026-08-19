#!/usr/bin/env bash
# Official reading for the capability v3 set (window-management axis,
# FROZEN-3, slice-032), per the ruler protocol: engineering posture only,
# B1 calibration sampling, one bundle per model cell.
#
#   Usage:  scripts/bench-capv3.sh [REPEAT] [MODELS...]
#     REPEAT  runs per task per cell   (default 3)
#     MODELS  Ollama models to drive   (default: qwen3.6:35b qwen3:14b)
#
# Cells run serially on purpose: every cell drives the same local Ollama,
# and parallel writers racing for a report name have already cost this repo
# a day of data (see bench-v2-agents.sh). An existing report is refused,
# not overwritten — delete it yourself if you really mean to re-measure.
#
# Expect an hour or more for the default pair. Run it under tmux or nohup —
# a closed terminal should not be able to end a measurement this long.

set -uo pipefail

REPEAT="${1:-3}"
shift || true
if [ "$#" -gt 0 ]; then
  MODELS=("$@")
else
  MODELS=("qwen3.6:35b" "qwen3:14b")
fi

REPO="$(cd "$(dirname "$0")/.." && pwd)"
TASKS="$REPO/benchmarks/capability-v3.yaml"
DATE="$(date +%Y%m%d)"

echo "== building orvena (release) once, up front =="
( cd "$REPO" && cargo build --release --quiet ) || { echo "build failed" >&2; exit 1; }
BIN="$REPO/target/release/orvena"

for MODEL in "${MODELS[@]}"; do
  SAFE="$(printf '%s' "$MODEL" | tr '/:' '--')"
  OUT="$REPO/bench-runs/${DATE}-capability-v3-${SAFE}.json"
  if [ -e "$OUT" ]; then
    echo "!! $OUT already exists — refusing to overwrite" >&2
    exit 1
  fi

  WORK="$(mktemp -d)"
  echo "== cell $MODEL — scratch at $WORK =="
  cd "$WORK"
  "$BIN" init >/dev/null
  sed -i.bak "s/kind: anthropic/kind: ollama/; s#model: claude-opus-4-8#model: ${MODEL}#" \
    .orvena/orvena.yaml && rm -f .orvena/orvena.yaml.bak
  . "$REPO/scripts/lib/calibration-sampling.sh"
  apply_calibration_sampling .orvena/orvena.yaml "$BIN" || exit 1

  echo "== offline sanity =="
  "$BIN" bench --provider offline --tasks "$TASKS" --repeat 1 || exit 1

  echo "== official: ollama / $MODEL x $REPEAT reps, engineering =="
  "$BIN" bench --provider ollama --tasks "$TASKS" --repeat "$REPEAT" \
    --governance engineering --out "$OUT" || exit 1
  echo "== cell $MODEL done -> $OUT =="
done

echo "ALL_CELLS_DONE"
