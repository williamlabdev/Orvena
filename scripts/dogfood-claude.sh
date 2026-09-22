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
  printf 'scaffolded %s (provider anthropic, tier engineering)\n' "$CFG"
  exit 0
fi

task=${1:-}; [ -n "$task" ] || die "usage: $0 \"<task>\" <writable-path>...   |   $0 --init"
shift
[ $# -gt 0 ] || die "declare at least one writable path — everything else is read-only"

[ -e "$CFG" ] || die "no $CFG — run \`$0 --init\` first"
# `--provider anthropic` at run time would override only the kind and keep the
# configured model, so the config itself must already point at Claude.
grep -Eq '^\s*kind:\s*anthropic' "$CFG" || die "$CFG provider.kind is not anthropic — the Claude profile only drives Anthropic models"
grep -Eq '^tier:\s*engineering' "$CFG" || die "$CFG tier is not engineering — only that tier enforces the sandbox"

args=()
for p in "$@"; do args+=(--write "$p"); done

exec "$ORVENA" run --agent claude "$task" "${args[@]}"
