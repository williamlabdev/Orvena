#!/usr/bin/env bash
# Run the differential matrix as a queue of chunks, in priority order, so that
# whatever has finished by morning is a legitimate stopping point.
#
# Why chunks instead of one big --repeat: M1 (containment) is a rare event that
# lands on a single task, so its effective sample size is that task's repeat
# count, not the 48 task-runs a bar reports. It wants depth. But no full bar's
# wall-clock has ever been recorded (bench-differential.sh only promises
# "an hour-plus for a 14B model"), so committing to --repeat 12 up front bets on
# a number nobody has measured. Chunks of 3 buy depth incrementally instead, and
# the reports pool: modes[].runs[] keeps every repeat's record, so four chunks of
# 3 carry the same evidence as one run of 12 — provided agent, model and commit
# match, which is what the .meta file next to each chunk is for.
#
# The queue alternates legs on purpose. Publishing discipline is "both legs or
# neither", so it must never be the case that stopping the queue leaves the
# governed leg deeper than the baseline. After every EVEN chunk the two legs are
# equal depth and the state is publishable; odd chunks are mid-step.
#
#   Usage:  scripts/bench-matrix.sh
#
#     STOP_AFTER   HH:MM — do not START a new chunk at or after this local time.
#                  A chunk already running is never killed. Without it the queue
#                  runs to exhaustion, which can mean a chunk starting at 07:55.
#     RUNS_DIR     override the output directory.
#
#   Reports land in bench-runs/<stamp>/, NOT docs/benchmark-results/. That
#   directory means "someone ran and published a bar"; raw chunks are neither
#   until they are pooled and read.
#
#   Run it so a sleeping laptop cannot stop it half way:
#     caffeinate -is nohup scripts/bench-matrix.sh 2>&1 | tee /tmp/bench-matrix.log &

set -uo pipefail   # deliberately NOT -e: one failed chunk must not end the queue

REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"

STAMP="$(date +%Y%m%d-%H%M)"
RUNS_DIR="${RUNS_DIR:-$REPO/bench-runs/$STAMP}"
STOP_AFTER="${STOP_AFTER:-}"
REPEAT=3

# STOP_AFTER is a wall-clock HH:MM, and the queue is an overnight one, so the
# time it names is usually tomorrow. Resolve it to an absolute instant once, at
# start: a naive same-day HH:MM comparison would read "08:00" at 23:50 as
# already past and refuse to start anything.
DEADLINE=""
if [ -n "$STOP_AFTER" ]; then
  if ! DEADLINE="$(date -j -f '%Y-%m-%d %H:%M' "$(date +%F) $STOP_AFTER" +%s 2>/dev/null)"; then
    DEADLINE="$(date -d "$(date +%F) $STOP_AFTER" +%s 2>/dev/null)"   # GNU date
  fi
  if [ -z "$DEADLINE" ]; then
    echo "STOP_AFTER: cannot parse '$STOP_AFTER' — expected HH:MM" >&2
    exit 2
  fi
  [ "$DEADLINE" -le "$(date +%s)" ] && DEADLINE=$((DEADLINE + 86400))
fi

SHA="$(git rev-parse --short HEAD)"
if [ -n "$(git status --porcelain)" ]; then
  DIRTY="dirty"
else
  DIRTY="clean"
fi

mkdir -p "$RUNS_DIR"

# The queue, in priority order: agent model chunk-label.
# Depth on qwen3:14b comes before breadth to a second model, because what is
# damaged is M1's credibility, not the result's generality.
QUEUE=(
  "native qwen3:14b A"
  "aider  qwen3:14b A"
  "native qwen3:14b B"
  "aider  qwen3:14b B"
  "native qwen3:14b C"
  "aider  qwen3:14b C"
  "native qwen3.6:35b A"
  "aider  qwen3.6:35b A"
)

echo "== differential matrix =="
echo "   stamp:    $STAMP"
echo "   commit:   $SHA ($DIRTY)"
echo "   out:      $RUNS_DIR"
echo "   chunks:   ${#QUEUE[@]} × REPEAT=$REPEAT"
if [ -n "$DEADLINE" ]; then
  if ! human="$(date -r "$DEADLINE" '+%F %H:%M' 2>/dev/null)"; then
    human="$(date -d "@$DEADLINE" '+%F %H:%M' 2>/dev/null)"
  fi
  echo "   stop-after: $human (no NEW chunk starts at/after it; a running one is never killed)"
fi
if [ "$DIRTY" = "dirty" ]; then
  echo "   note: working tree is dirty; the sha alone does not identify what ran."
fi
echo

# Preflight. Failing here is free; failing at 03:00 on chunk 4 is not.
missing=""
for m in qwen3:14b qwen3.6:35b; do
  ollama list 2>/dev/null | awk '{print $1}' | grep -qx "$m" || missing="$missing $m"
done
if [ -n "$missing" ]; then
  echo "preflight: ollama is missing model(s):$missing" >&2
  echo "           chunks needing them will fail; pull them or edit QUEUE." >&2
fi
HAVE_AIDER=1
if ! command -v aider >/dev/null 2>&1; then
  HAVE_AIDER=0
  echo "preflight: aider not on PATH — every aider chunk will be SKIPPED." >&2
  echo "           that breaks the both-legs rule; nothing here will be publishable." >&2
fi
echo

past_deadline() {
  [ -z "$DEADLINE" ] && return 1
  [ "$(date +%s)" -ge "$DEADLINE" ]
}

run_chunk() {
  local agent="$1" model="$2" chunk="$3"
  local safe_model name out log meta
  safe_model="$(printf '%s' "$model" | tr '/:' '--')"
  name="${agent}-${safe_model}-chunk${chunk}"
  out="$RUNS_DIR/${name}.json"
  log="$RUNS_DIR/${name}.log"
  meta="$RUNS_DIR/${name}.meta"

  # Resume-friendly: a chunk that already produced a report is not re-run, so
  # restarting the queue after a crash costs nothing already paid for.
  if [ -e "$out" ]; then
    echo "-- $name: report already present, skipping"
    return 0
  fi

  if [ "$agent" = "aider" ] && [ "$HAVE_AIDER" = "0" ]; then
    echo "-- $name: SKIPPED (aider not on PATH)"
    printf 'chunk=%s\nstatus=skipped-no-aider\n' "$name" > "$meta"
    return 0
  fi

  local started elapsed rc
  started="$(date +%FT%T%z)"
  echo "-- $name: start $started"

  # Each chunk is one ordinary differential run; this script adds queueing and
  # bookkeeping, not measurement logic. ORVENA_AGENT_TIMEOUT_SECS matches the
  # value the 35b aider smoke needed (HANDOFF_GATE_TMPDIR_0802.md).
  SECONDS=0
  if [ "$agent" = "aider" ]; then
    OUT="$out" AGENT=aider ORVENA_AGENT_TIMEOUT_SECS=1800 \
      scripts/bench-differential.sh "$REPEAT" "$model" > "$log" 2>&1
  else
    OUT="$out" AGENT=native \
      scripts/bench-differential.sh "$REPEAT" "$model" > "$log" 2>&1
  fi
  rc=$?
  elapsed=$SECONDS

  # No completed bar's duration has ever been recorded in this repo. Record it,
  # so the next queue is scheduled from measurement instead of from the script's
  # own "expect an hour-plus".
  {
    printf 'chunk=%s\nagent=%s\nmodel=%s\nrepeat=%s\n' "$name" "$agent" "$model" "$REPEAT"
    printf 'commit=%s\nworktree=%s\n' "$SHA" "$DIRTY"
    printf 'started=%s\nfinished=%s\nelapsed_secs=%s\nexit=%s\n' \
      "$started" "$(date +%FT%T%z)" "$elapsed" "$rc"
  } > "$meta"

  if [ "$rc" -eq 0 ]; then
    echo "-- $name: done in $((elapsed / 60))m"
  else
    echo "-- $name: FAILED (exit $rc) after $((elapsed / 60))m — see $log"
  fi
  return 0   # the queue continues either way
}

trap 'echo; echo "interrupted — finished chunks remain in $RUNS_DIR"; exit 130' INT TERM

i=0
for entry in "${QUEUE[@]}"; do
  i=$((i + 1))
  # shellcheck disable=SC2086
  set -- $entry
  if past_deadline; then
    echo "-- stopping: past STOP_AFTER=$STOP_AFTER, $((${#QUEUE[@]} - i + 1)) chunk(s) not started"
    break
  fi
  echo "[$i/${#QUEUE[@]}]"
  run_chunk "$1" "$2" "$3"
  echo
done

echo "== summary =="
printf '%-32s %-10s %-8s %s\n' CHUNK STATUS ELAPSED REPORT
for meta in "$RUNS_DIR"/*.meta; do
  [ -e "$meta" ] || continue
  # shellcheck disable=SC1090
  ( set -a; . "$meta"; set +a
    if [ "${status:-}" = "skipped-no-aider" ]; then
      printf '%-32s %-10s %-8s %s\n' "$chunk" "skipped" "-" "-"
    elif [ "${exit:-1}" = "0" ]; then
      printf '%-32s %-10s %-8s %s\n' "$chunk" "ok" "$((elapsed_secs / 60))m" "$chunk.json"
    else
      printf '%-32s %-10s %-8s %s\n' "$chunk" "FAILED" "$((elapsed_secs / 60))m" "$chunk.log"
    fi )
done
echo
echo "Reports: $RUNS_DIR"
echo "Read them at an EVEN chunk boundary — that is where the two legs are equal"
echo "depth. Pool same-cell chunks before drawing any conclusion from M1."
