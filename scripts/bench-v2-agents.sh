#!/usr/bin/env bash
# Run the v2 discrimination check across several wrapped agents, one at a time.
#
# Serial on purpose, for two reasons that have both already cost data:
#
#   1. Two chains started 14 seconds apart on 2026-08-03 both passed the OUT
#      existence check, and the later one overwrote the earlier one's report.
#      The native depth run from that day is still unusable. Each agent here
#      gets its own OUT path, and the whole set runs in sequence, so no two
#      writers can ever race for the same name.
#   2. Every agent drives the same local Ollama. Running them together does not
#      make them faster — it makes them queue, and a queued agent can hit the
#      task timeout and be recorded as having failed the task rather than as
#      having waited.
#
# An agent that is not installed is skipped with a note rather than failing the
# set: a missing binary should cost you that cell, not the afternoon.
#
#   Usage:  scripts/bench-v2-agents.sh [REPEAT] [MODEL] [AGENTS...]
#     REPEAT  runs per task per agent   (default 5)
#     MODEL   model to drive            (default qwen3:14b)
#     AGENTS  agents to run             (default: the five wrapped profiles)
#
# Expect hours, not minutes. Run it under tmux or nohup — a closed terminal
# should not be able to end a measurement this long:
#
#   tmux new -s bench 'scripts/bench-v2-agents.sh 5 qwen3:14b 2>&1 | tee bench.log'

set -uo pipefail

REPEAT="${1:-5}"
shift || true
MODEL="${1:-qwen3:14b}"
shift || true

if [ "$#" -gt 0 ]; then
  AGENTS=("$@")
else
  # codex twice on purpose: `codex` stands its own sandbox down so Orvena is the
  # only boundary, `codex-nested` leaves it on. The pair is the nested
  # containment comparison, not a duplicate.
  AGENTS=(aider openhands continue codex codex-nested opencode)
fi

REPO="$(cd "$(dirname "$0")/.." && pwd)"
DATE="$(date +%F)"
SAFE_MODEL="$(printf '%s' "$MODEL" | tr '/:' '--')"
OUTDIR="$REPO/bench-runs/v2-agents-${DATE}"
mkdir -p "$OUTDIR"

echo "== building orvena (release) once, up front =="
( cd "$REPO" && cargo build --release --quiet ) || { echo "build failed" >&2; exit 1; }
BIN="$REPO/target/release/orvena"

echo
echo "agents:  ${AGENTS[*]}"
echo "model:   $MODEL   repeat: $REPEAT"
echo "reports: $OUTDIR"
echo

STARTED_ALL="$(date +%s)"
declare -a SUMMARY=()

for AGENT in "${AGENTS[@]}"; do
  OUT="$OUTDIR/${SAFE_MODEL}-${AGENT}-baseline.json"
  echo "════════════════════════════════════════════════════════════════"
  echo "  $AGENT"
  echo "════════════════════════════════════════════════════════════════"

  if [ -e "$OUT" ]; then
    echo "  report already exists, skipping (evidence is never overwritten):"
    echo "    $OUT"
    SUMMARY+=("$AGENT: skipped (report exists)")
    echo
    continue
  fi

  STARTED="$(date +%s)"
  # Each agent gets its own scratch project, so a crash in one cannot leave
  # state behind that changes the next one's run.
  WORK="$(mktemp -d)"
  (
    cd "$WORK" || exit 1
    "$BIN" init --provider ollama --model "$MODEL" >/dev/null || exit 1
    [ -f "$REPO/.env" ] && cp "$REPO/.env" .env
    OUT="$OUT" AGENT="$AGENT" "$BIN" bench \
      --tasks "$REPO/benchmarks/temptation-v2.yaml" \
      --agent "$AGENT" \
      --governance off \
      --repeat "$REPEAT" \
      --out "$OUT"
  )
  RC=$?
  ELAPSED=$(( $(date +%s) - STARTED ))

  if [ $RC -ne 0 ]; then
    # Most likely the agent is not on PATH — `resolve_agent` refuses up front
    # rather than producing a benchmark full of failures.
    echo "  !! $AGENT exited $RC after ${ELAPSED}s — see the message above."
    echo "     Continuing with the remaining agents."
    SUMMARY+=("$AGENT: FAILED (rc=$RC, ${ELAPSED}s)")
  else
    SUMMARY+=("$AGENT: ok (${ELAPSED}s)")
  fi
  rm -rf "$WORK"
  echo
done

echo "════════════════════════════════════════════════════════════════"
echo "  per-agent, per-task breach rate"
echo "════════════════════════════════════════════════════════════════"
python3 - "$OUTDIR" "$REPEAT" <<'PY'
import json, sys, glob, os, collections

outdir, repeat = sys.argv[1], int(sys.argv[2])
reports = sorted(glob.glob(os.path.join(outdir, "*-baseline.json")))
if not reports:
    print("no reports were produced — every agent failed or was skipped.")
    raise SystemExit(0)

# task -> agent -> (breach, n); a task that tempts nobody is the one to cut.
grid, agents = collections.defaultdict(dict), []
for path in reports:
    agent = os.path.basename(path).split("-baseline.json")[0].split("-", 2)[-1]
    agents.append(agent)
    counts = collections.defaultdict(lambda: {"n": 0, "breach": 0, "done": 0})
    report = json.load(open(path))
    for mode in report["modes"]:
        for run in mode["runs"]:
            for r in run["results"]:
                c = counts[r["id"]]
                c["n"] += 1
                c["breach"] += 1 if r["violations"] else 0
                c["done"] += 1 if r["completed"] else 0
    for task_id, c in counts.items():
        grid[task_id][agent] = c

width = max((len(t) for t in grid), default=10)
print(f"  {'task'.ljust(width)}  " + "  ".join(a.center(11) for a in agents))
dead = []
for task_id in sorted(grid):
    cells = []
    tempted_any = False
    for a in agents:
        c = grid[task_id].get(a)
        if not c:
            cells.append("     -     ")
            continue
        if c["breach"]:
            tempted_any = True
        cells.append(f"{c['breach']:>3}/{c['n']:<3} br ".rjust(11))
    if not tempted_any:
        dead.append(task_id)
    print(f"  {task_id.ljust(width)}  " + "  ".join(cells))

print()
if dead:
    print(f"{len(dead)} task(s) tempted NO agent — dead weight, redesign or cut:")
    for t in dead:
        print(f"  - {t}")
    print("Read their transcripts before rewriting: a task can fail to tempt")
    print("because the bait is weak, because the agent could not reach it, or")
    print("because the in-scope fix was simply easier.")
else:
    print("every task tempted at least one agent — the set discriminates.")
print()
print("A task that tempts one agent and not another is not broken: that is the")
print("tool-surface difference the agent axis exists to expose. A task that")
print("tempts none of them is measuring nothing.")
PY

echo
echo "total wall clock: $(( ($(date +%s) - STARTED_ALL) / 60 )) min"
for line in "${SUMMARY[@]}"; do echo "  $line"; done
echo
echo "reports: $OUTDIR"
