# Dogfooding: your own Claude Code sessions inside Orvena

The cheapest way to learn where the product bites is to wrap real work, not
benchmark tasks. `scripts/dogfood-claude.sh` runs one Claude Code task inside
Orvena's enforced envelope in whatever repo you are standing in, and leaves the
usual evidence bundle under `.orvena/runs/<id>/evidence.json`.

```bash
cd ~/some/repo
~/dev/orvena/scripts/dogfood-claude.sh --init      # once: .orvena/ with provider anthropic, tier engineering
~/dev/orvena/scripts/dogfood-claude.sh \
  "rename the config loader to load_config and update its callers" \
  src/config.rs src/main.rs
jq . "$(ls -td .orvena/runs/* | head -1)/evidence.json"
```

What to look at after a week of runs, in this order:

1. `scope_refusals` — did the agent actually reach outside the declared paths?
   Zero across many runs is a finding too (the boundary is never tested), not
   a pass.
2. `blockers` and `agent_terminal` — runs that did not complete. Separate
   "the sandbox blocked a path the CLI needs to function" (a profile gap, like
   the cache paths added in 0.8.0) from "the task legitimately needed a file
   you did not declare" (a scope declaration habit to build).
3. `sandbox` state — confirm every run says enforced. An advisory run measures
   nothing.

Hard rules the script enforces:

- `ANTHROPIC_API_KEY` must be unset. The Claude profile inherits the
  environment, and a key would silently move the CLI from subscription to
  metered billing.
- `tier: engineering` in `.orvena/orvena.yaml`. `light` is advisory.
- `provider.kind: anthropic`. The profile refuses every other provider by
  design; a run-time `--provider` override would keep the wrong model.
