#!/usr/bin/env bash
# Wrap ONE Claude Code task in Orvena's enforced envelope, inside the repo you
# are standing in. Day-to-day dogfooding: the point is to see what the evidence
# bundle records when a real agent does real work — scope refusals, blockers,
# paths the sandbox blocked that the agent needed.
#
#   scripts/dogfood-claude.sh "<task>" <writable-path>...
#   scripts/dogfood-claude.sh --init            # scaffold .orvena/ for this
#
# Env:
#   ORVENA               binary to use (default: `orvena` on PATH)
#   ORVENA_CLAUDE_MODEL  model id/alias written by --init (default: sonnet)
#
# Read the evidence with: jq . "$(ls -td .orvena/runs/* | head -1)/evidence.json"
set -euo pipefail

ORVENA=${ORVENA:-orvena}
CFG=.orvena/orvena.yaml

die() { printf 'dogfood-claude: %s\n' "$*" >&2; exit 2; }

command -v "$ORVENA" >/dev/null || die "\`$ORVENA\` not found — build with \`cargo build --release\` and set ORVENA=target/release/orvena"
command -v claude >/dev/null   || die "\`claude\` not on PATH — install Claude Code first"

# A stray ANTHROPIC_API_KEY silently switches the CLI from subscription to
# metered API billing (see crates/orvena-core/src/adapter/claude.rs). Refuse
# rather than spend money by accident.
[ -z "${ANTHROPIC_API_KEY:-}" ] || die "ANTHROPIC_API_KEY is set; unset it so the CLI stays on subscription auth"

if [ "${1:-}" = "--init" ]; then
  [ -e "$CFG" ] && die "$CFG already exists — edit it instead (provider.kind: anthropic, tier: engineering)"
  "$ORVENA" init --provider anthropic --model "${ORVENA_CLAUDE_MODEL:-sonnet}" --non-interactive
  # Only `engineering` enforces the OS sandbox; `light` is advisory and would
  # measure nothing.
  sed -i.bak -E 's/^tier: .*/tier: engineering/' "$CFG" && rm -f "$CFG.bak"
  grep -q '^tier: engineering' "$CFG" || die "could not set tier: engineering in $CFG"
  # The scaffold's gate is `true`, which measures nothing. Fill in the obvious
  # check for the toolchain we can see; edit .orvena/gates.yaml to taste.
  gate=""
  [ -f go.mod ]       && gate="go build ./... && go vet ./..."
  [ -f package.json ] && gate="pnpm test"
  [ -f Cargo.toml ]   && gate="cargo test"
  if [ -n "$gate" ]; then
    # `&` is special in a sed replacement; escape it.
    sed -i.bak -E "s|^([[:space:]]*verify:) \"true\".*|\\1 \"${gate//&/\\&}\"|" .orvena/gates.yaml && rm -f .orvena/gates.yaml.bak
    grep -qF "verify: \"$gate\"" .orvena/gates.yaml || die "could not write the gate into .orvena/gates.yaml"
  fi
  # Keep the scaffold and the agent scratch dir out of the repo's diff.
  for ignore in .orvena/ .orvena-agent/; do
    grep -qxF "$ignore" .gitignore 2>/dev/null || printf '%s\n' "$ignore" >> .gitignore
  done
  printf 'scaffolded %s (provider anthropic, tier engineering, gate: %s)\n' "$CFG" "${gate:-true — edit .orvena/gates.yaml}"
  exit 0
fi

task=${1:-}; [ -n "$task" ] || die "usage: $0 \"<task>\" <writable-path>...   |   $0 --init"
shift
[ $# -gt 0 ] || die "declare at least one writable path — everything else is read-only"

[ -e "$CFG" ] || die "no $CFG — run \`$0 --init\` first"
# A stray scaffold in $HOME turns a run into a fishing trip: the agent wanders
# the filesystem, hits the sandbox on the real repo, and a `verify: "true"`
# gate then reports the nothing it did as completed. Refuse both up front.
[ -d .git ] || die "$PWD is not a git repository root — cd into the project you mean to govern"
grep -Eq '^[[:space:]]*verify:[[:space:]]*"true"' .orvena/gates.yaml && die ".orvena/gates.yaml still has the placeholder gate (verify: \"true\") — set a real test command or re-run --init"
# `--provider anthropic` at run time would override only the kind and keep the
# configured model, so the config itself must already point at Claude.
grep -Eq '^\s*kind:\s*anthropic' "$CFG" || die "$CFG provider.kind is not anthropic — the Claude profile only drives Anthropic models"
grep -Eq '^tier:\s*engineering' "$CFG" || die "$CFG tier is not engineering — only that tier enforces the sandbox"

# Paths may be given bare (`dir1 dir2`) or already flagged (`--write dir1`);
# both spell the same declaration.
args=()
for p in "$@"; do
  [ "$p" = "--write" ] && continue
  args+=(--write "$p")
done
[ ${#args[@]} -gt 0 ] || die "declare at least one writable path — everything else is read-only"

# Go writes its build cache and link scratch outside the repo by default
# (~/Library/Caches/go-build, $TMPDIR), which the sandbox refuses. Point both
# at the agent scratch dir, which is always writable. Go creates GOCACHE itself
# but not GOTMPDIR. The first build in a repo is cold (~10s on a mid-size Go
# service); later runs reuse it. Deps must already be in the module cache —
# the sandbox has no network for the toolchain.
if [ -f go.mod ]; then
  mkdir -p .orvena-agent/go-cache .orvena-agent/go-tmp
  export GOCACHE="$PWD/.orvena-agent/go-cache" GOTMPDIR="$PWD/.orvena-agent/go-tmp" GOPROXY=off
fi

exec "$ORVENA" run --agent claude "$task" "${args[@]}"
