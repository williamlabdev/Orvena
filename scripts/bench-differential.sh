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
#     OUT        report path, overriding the date/model/agent-derived one. Use
#                it to keep several chunks of the same cell side by side, or to
#                pin a queue's output when it will cross midnight. The derived
#                path is refused if it already exists — evidence is never
#                overwritten, whatever the source of the name.
#     KEEP_SCRATCH  set to keep the scratch project (and every evidence bundle
#                   and agent transcript in it) instead of deleting it on exit.
#                   A run that times out or refuses in a way you cannot explain
#                   is only diagnosable if its transcript outlived it.
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
# OUT overrides the derived path: a run split into chunks needs to put several
# reports for the same cell side by side, and a queue that crosses midnight must
# not have its date change underneath it.
if [ -z "${OUT:-}" ]; then
  if [ "$AGENT" = "native" ]; then
    OUT="$REPO/docs/benchmark-results/${DATE}-${SAFE_MODEL}-differential.json"
  else
    OUT="$REPO/docs/benchmark-results/${DATE}-${SAFE_MODEL}-${AGENT}-differential.json"
  fi
fi

# Refuse to overwrite. The derived path collides with itself on a same-day
# re-run of the same model, so publishing a number and then re-measuring it
# silently destroys the evidence the published number cites — it has happened
# once already, and was recovered only because the file was in git. An hour of
# machine time is cheaper than a report that no longer matches its own source.
#
# The name is claimed *atomically*, not checked and then written. `[ -e ]`
# followed by a write leaves a TOCTOU window wide enough to lose a sample: on
# 2026-08-03 two chains started 14 seconds apart both passed the existence
# check and raced, and the survivor overwrote a report that read breach 10/30
# (bench-runs/m1-depth-20260803 records the interleaved run_ids). Under
# `set -C` the create itself fails when the file exists, in the kernel, so only
# one chain can ever hold a given OUT no matter how they are launched.
mkdir -p "$(dirname "$OUT")"
if ! (set -C; : > "$OUT") 2>/dev/null; then
  echo "refusing to overwrite an existing report:" >&2
  echo "  $OUT" >&2
  echo "Pass OUT=<path> for this run, or move the existing file aside first." >&2
  exit 1
fi
# Until `bench --out` fills it, the claim is a 0-byte placeholder. A run that
# dies before then must not leave a name that blocks every future run with a
# file holding no evidence, so drop it on the way out. Re-armed below once the
# scratch project exists, because a second `trap ... EXIT` replaces this one.
trap '[ -s "$OUT" ] || rm -f "$OUT"' EXIT

echo "== building orvena (release) =="
( cd "$REPO" && cargo build --release --quiet )
BIN="$REPO/target/release/orvena"

WORK="$(mktemp -d)"
# Thrown away by default, so a matrix run leaves nothing behind. KEEP_SCRATCH
# holds it — including .orvena/bench/, where the per-run evidence bundles and the
# wrapped agent's transcripts live — for runs that have to be explained after the
# fact. It survives a failed run too: that is usually the run worth reading.
if [ -n "${KEEP_SCRATCH:-}" ]; then
  trap '[ -s "$OUT" ] || rm -f "$OUT"; echo; echo "scratch project kept at: $WORK"' EXIT
else
  trap '[ -s "$OUT" ] || rm -f "$OUT"; rm -rf "$WORK"' EXIT
fi
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

# One set of sampling for every model (B1, 0806) — without it the backend
# decides and the two calibration cells are not comparable. slice-029.
. "$REPO/scripts/lib/calibration-sampling.sh"
apply_calibration_sampling .orvena/orvena.yaml "$BIN"

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
if [ -n "${KEEP_SCRATCH:-}" ]; then
  echo "The per-task evidence bundles (and their git baselines) are kept under"
  echo "  $WORK/.orvena/bench/"
  echo "along with the scratch project itself; delete it yourself when done."
else
  echo "The per-task evidence bundles (and their git baselines) live under the"
  echo "scratch project's .orvena/bench/ and are deleted with it; the report JSON"
  echo "retains every per-run result for audit. Set KEEP_SCRATCH=1 to keep them."
fi
