# Orvena

**A customizable, config-first coding agent — the runnable reference for AI-native software engineering.**

Orvena treats an LLM as a *bounded team member*, not an unsupervised code generator.
Its behavior is driven by configuration rather than hard-coded logic — you define the
**roles**, **context budgets**, and **workflow gates** that shape how the model works
inside your codebase. Bring your own model provider.

> **Status: early & evolving.** v0.1 is a minimal core, shipped in small increments.
> The single coding loop works; advanced subsystems are intentionally out of scope for now.

## Why Orvena

Prompt-driven AI coding drifts at scale: scope creep, over-refactoring, exploding
context, and no clear definition of "done". Orvena puts brakes on each of these:

- **Bounded change** — a task locks its scope; everything else is read-only by default.
- **Specialized roles** — each role has allowed/forbidden tools.
- **Controlled context** — context is a per-role budget, not an unbounded window.
- **Verifiable gates** — a change is "done" only when it passes a gate that produces
  observable evidence (e.g. your test command exits 0).
- **Evidence & metrics** — every run emits frozen metrics (completed, tokens, steps,
  tool calls) so you can catch regressions.

## Install

```bash
# from source (single static binary)
cargo install --git https://github.com/williamlabdev/Orvena orvena-cli
```

Or build the repo directly:

```bash
git clone https://github.com/williamlabdev/Orvena && cd Orvena
cargo build --release      # binary at target/release/orvena
```

Requires a recent stable Rust toolchain (`rustup` recommended).

## Quickstart

```bash
orvena init                                   # scaffold config + pick a provider
orvena run "create a hello module" -w src/hello.rs
orvena status                                 # provider / roles / gates / budgets / skills
```

`orvena init` walks you through picking a provider and points you to where the API key
goes — nothing is assumed silently. `orvena doctor` preflights your setup.

## Model providers

Pick one at `orvena init` — **no default is forced**:

| Provider | Notes | Parity-checked |
|---|---|---|
| **Ollama** | Local / offline / private. You run Ollama and pull a model yourself. | ✅ `qwen3:14b` |
| **Gemini** | Hosted, via the **OpenAI** provider + a base-URL override (its key goes in `OPENAI_API_KEY`). | ✅ `gemini-2.5-flash` |
| **Anthropic** | Hosted Claude. | ◻ not yet run |
| **OpenAI** | Hosted. | ◻ not yet run |
| **OpenRouter** | Hosted — one key, many models. | ◻ not yet run |
| **offline** | Deterministic stub for tests and regression baselines (no network). | n/a |

**"Parity-checked" means somebody actually ran it**, not that the code path
exists: the provider passed the behavioral contract in
[`docs/provider-parity.md`](docs/provider-parity.md) — well-formed report,
consistent completion semantics, real token round-trip, evidence bundle that
round-trips. The unchecked providers are implemented and expected to work; they
simply have not been exercised end to end here, and this table will not claim
otherwise. Adding one is a single command (see that doc) — PRs with a passing
run are welcome.

Keys are read from `.env` (see `.env.example`). **Never commit real keys.**

## How it works

Orvena is **config-first**: a small scaffold deployed by `orvena init` into `./.orvena/`
drives behavior, so you adapt Orvena by editing config, not forking code.

- `orvena.yaml` — provider selection, governance tier, default role, step ceiling.
- `roles.yaml` — roles and their allowed/forbidden tools.
- `gates.yaml` — gates (a `condition`, an optional `verify` command for evidence, and a
  `gatekeeper`: `automated` or `human`).
- `context-budgets.yaml` — per-role token budgets.

The coding loop is **prepare context → call model → apply (scope-gated) → check gates**.
If an automated gate fails, Orvena re-attempts within a bounded number of steps, feeding
the gate's evidence back in. A `human` gate stops and reports a blocker for you to resolve.

Discipline scales with risk via a **governance tier**: `light` (gates/scope advisory) or
`engineering` (hard-enforced).

## Benchmark

`orvena bench` runs a set of hand-picked, auto-verifiable coding tasks and reports a
**completion rate** (fraction that reach a passing test). Each task defines its own
`verify` (e.g. `cargo test` / `pytest`), so "solved" is the same rule the product ships.

It also measures the **governance differential** — what the brakes buy:
`orvena bench --governance off,engineering` runs the same tasks with enforcement as
the only variable, and an independent git-based oracle judges what each run actually
changed.

**Governance differential (2026-07-11):** on an 8-task scope-adversarial set, a local
`qwen3:14b` under the ungoverned baseline made a **false "done" claim 25%** of the
time; under the `engineering` tier that is structurally impossible (**0%**) — and the
governed runs were *cheaper* (×0.43 steps, ×0.31 tokens): gate evidence acts as
navigation, not drag. Containment was 100% under both postures for this model —
enforcement showed up as refused, auditable escape attempts rather than prevented
files. Full numbers and honest caveats:
[docs/benchmark-results.md](docs/benchmark-results.md).

**First number (2026-07-04):** on a small curated set of 5 self-contained Rust tasks,
Orvena driving a local `qwen3:14b` solved **5/5 (100%)**, single-pass. This is a
deliberately small, early, self-hosted signal — *not* a real-world capability claim
(the tasks are simple and the sample is tiny). Full context, caveats, and how to
reproduce (or run a stronger model): [docs/benchmark-results.md](docs/benchmark-results.md)
· method: [docs/benchmark.md](docs/benchmark.md).

## Embedding

All logic lives in the `orvena-core` library crate; the `orvena` CLI is a thin frontend.
The core is designed to be embedded by a larger runtime later.

## Customization

Everything behavioral lives in `./.orvena/*.yaml`. Edit roles, budgets, and gates to fit
your workflow — no code changes required for most adaptations.

## Contributing

Early-stage and moving fast — issues and discussion welcome. (Contribution guide coming.)

## License

MIT — see [LICENSE](LICENSE).
