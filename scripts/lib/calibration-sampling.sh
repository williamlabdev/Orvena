# Sampling for repo-controlled measurement runs. Sourced by the bench scripts;
# usable by hand for a probe run (see SLICE-026, calibration protocol).
#
# Why this file exists at all: a scratch project scaffolded by `orvena init`
# leaves `sampling` unset, which means *inherited* — the backend decides, and
# for Ollama that is the model's Modelfile, a file this repo neither versions
# nor can see from a published report. Measured 0806: `qwen3:14b` ships
# `temperature 0.6` while `qwen3.6:27b` and `qwen3.6:35b` ship `temperature 1`,
# so the floor cell and the ceiling cells of the capability ladder were never
# sampled under equal conditions (slice-029).
#
# Ruled 0806 (william), B1 — one set of sampling for every model. The whole
# premise of a two-cell calibration is "everything but the model is the same";
# per-model values would keep that premise permanently false.
#
# The numbers are `qwen3:14b`'s existing Modelfile values. Any B1 choice moves
# some cell off its old condition; this one moves the two ceiling cells and
# leaves the floor cell — and the 0.1.0->0.4.0 same-model ladder, the longest
# fully-consistent series this repo has — continuous.
CALIBRATION_TEMPERATURE=0.6
CALIBRATION_TOP_P=0.95
CALIBRATION_TOP_K=20
# No seed, on purpose. A fixed seed makes `--repeat` measure nothing: every
# repeat returns the same sample, and how stable a model is under resampling is
# the reading slice-028 was built to take. A seeded regression run belongs
# beside `repeat`, never in place of it.

# apply_calibration_sampling [CONFIG] [BIN]
#   CONFIG  path to the scratch project's orvena.yaml  (default .orvena/orvena.yaml)
#   BIN     orvena binary, used to parse-check the result (default: orvena on PATH)
#
# Inserting keys into a config by pattern is exactly what the scaffold path
# avoids elsewhere, so the result is parsed back before returning: a config that
# does not load fails here, at second zero, rather than at hour four of a
# calibration run.
apply_calibration_sampling() {
  local cfg="${1:-.orvena/orvena.yaml}"
  local bin="${2:-orvena}"

  if [ ! -f "$cfg" ]; then
    echo "apply_calibration_sampling: no config at $cfg" >&2
    return 1
  fi
  # A config that already carries sampling is a condition this script did not
  # set and cannot vouch for. Refuse rather than run a calibration whose header
  # would claim values nobody here chose.
  if grep -qE '^[[:space:]]+sampling:' "$cfg"; then
    echo "apply_calibration_sampling: $cfg already sets sampling — refusing to run" >&2
    echo "  a measurement run must not inherit a sampling block from elsewhere." >&2
    return 1
  fi
  if ! grep -qE '^provider:' "$cfg"; then
    echo "apply_calibration_sampling: no provider block in $cfg" >&2
    return 1
  fi

  awk -v t="$CALIBRATION_TEMPERATURE" -v p="$CALIBRATION_TOP_P" -v k="$CALIBRATION_TOP_K" '
    /^provider:/ && !ins {
      print
      print "  # Repo-controlled sampling for measurement runs (B1, 0806) —"
      print "  # scripts/lib/calibration-sampling.sh. Without this block the"
      print "  # backend decides, and the report says so: `sampling: inherited`."
      print "  sampling:"
      print "    temperature: " t
      print "    top_p: " p
      print "    top_k: " k
      ins = 1
      next
    }
    { print }
  ' "$cfg" > "$cfg.tmp" || return 1
  mv "$cfg.tmp" "$cfg"

  # Parse-check. `status` only loads the config — it needs no API key, so a
  # failure here is a malformed config and nothing else.
  local dir
  dir="$(dirname "$(dirname "$cfg")")"
  if ! ( cd "$dir" && "$bin" status >/dev/null ); then
    echo "apply_calibration_sampling: $cfg no longer parses after the insert" >&2
    return 1
  fi

  echo "sampling: temperature $CALIBRATION_TEMPERATURE  top_p $CALIBRATION_TOP_P  top_k $CALIBRATION_TOP_K  (no seed)"
}
