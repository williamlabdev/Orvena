#!/usr/bin/env bash
# Wrapped-agent cells for the capability v3 set (FROZEN-3, slice-032) —
# ceiling probes, NOT official ruler readings.
#
#   Usage:  scripts/bench-capv3-adapter.sh AGENT KIND MODEL [REPEAT]
#     AGENT   adapter profile (claude | codex | codex-nested | …)
#     KIND    orvena provider kind the profile maps (anthropic | openai)
#     MODEL   model string the profile passes through
#     REPEAT  runs per task (default 3)
#
# Two reasons these numbers never pool with the native cells:
#   - sampling is the vendor's own (no B1 calibration is possible), so the
#     report's `sampling: not recorded` line is load-bearing, not a gap;
#   - a wrapped-agent "step" is one whole agent session (its own inner loop),
#     while a native step is one model call — the step budget means a
#     different thing on each side of that line.
# The independent git oracle and the evidence schema are the same, which is
# exactly what makes the ceiling probe worth having.

set -uo pipefail

AGENT="${1:?usage: bench-capv3-adapter.sh AGENT KIND MODEL [REPEAT]}"
KIND="${2:?usage: bench-capv3-adapter.sh AGENT KIND MODEL [REPEAT]}"
MODEL="${3:?usage: bench-capv3-adapter.sh AGENT KIND MODEL [REPEAT]}"
REPEAT="${4:-3}"

REPO="$(cd "$(dirname "$0")/.." && pwd)"
TASKS="$REPO/benchmarks/capability-v3.yaml"
DATE="$(date +%Y%m%d)"
SAFE="$(printf '%s-%s' "$AGENT" "$MODEL" | tr '/:' '--')"
OUT="$REPO/bench-runs/${DATE}-capability-v3-agent-${SAFE}.json"
if [ -e "$OUT" ]; then
  echo "!! $OUT already exists — refusing to overwrite" >&2
  exit 1
fi

echo "== building orvena (release) once, up front =="
( cd "$REPO" && cargo build --release --quiet ) || { echo "build failed" >&2; exit 1; }
BIN="$REPO/target/release/orvena"

WORK="$(mktemp -d)"
echo "== cell ${AGENT}/${MODEL} — scratch at $WORK =="
cd "$WORK"
"$BIN" init >/dev/null
sed -i.bak "s/kind: anthropic/kind: ${KIND}/; s#model: claude-opus-4-8#model: ${MODEL}#" \
  .orvena/orvena.yaml && rm -f .orvena/orvena.yaml.bak

echo "== offline sanity =="
"$BIN" bench --provider offline --tasks "$TASKS" --repeat 1 || exit 1

echo "== ceiling probe: ${AGENT} / ${MODEL} x $REPEAT reps, engineering =="
"$BIN" bench --agent "$AGENT" --tasks "$TASKS" --repeat "$REPEAT" \
  --governance engineering --out "$OUT" || exit 1
echo "== cell ${AGENT}/${MODEL} done -> $OUT =="
echo "ALL_CELLS_DONE"
