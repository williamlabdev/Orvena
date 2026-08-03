# Orvena

**A task-scope governance runtime — you declare boundaries, Orvena enforces them, any agent stays contained.**

Orvena doesn't code. It *proves that an untrusted agent can't escape its sandbox.*

You give Orvena a task definition (these files, this time, this step budget), hand it any agent
(native loop, Aider, Claude, a finetuned model), and Orvena ensures:
- The agent can only read/write the declared files
- The agent can't exceed its step budget
- Every boundary violation is logged, measured, and evidence is frozen in a JSON report

**The value**: You can now safely use a powerful agent—even one you don't fully trust—because
you have mathematical proof it stayed in scope.

> **Status: early & evolving.** v0.1 is a minimal core. The native loop + containment oracle work;
> advanced subsystems (multiple agents, policy composition) are intentionally out of scope for now.

## Why Orvena

AI agents are getting more capable, but "capable" can mean "goes places you didn't ask it to go."
Traditional solutions (prompt restrictions, agent design philosophy) don't scale: they're suggestions,
not guarantees. Orvena shifts the boundary to the OS.

**The problem:**
- Scope creep — agent modifies unrelated files to "optimize" or "fix things"
- False confidence — you rely on the agent to honor what you asked, not what you can verify
- No audit trail — agent ran, things changed, no evidence of what it actually tried to do

**Orvena's answer:**
- **Declared scope** — you list exactly which files can be modified; everything else is read-only by default
- **Enforced boundaries** — OS-level containment (sandbox + file descriptor restriction) makes violations *impossible*, not impolite
- **Observable evidence** — every run produces a frozen report: scope violations, steps taken, tokens consumed, task outcome
- **Differential measurement** — run the same task ungoverned vs. governed with the same agent; the difference is pure overhead of containment

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

**The model provider**, pick at init time — **no default is forced**:

```bash
orvena init --provider ollama --model qwen3:14b
orvena init --provider openai_compat --model <id> \
  --base-url http://localhost:8000/v1 --api-key-env MY_KEY
```

An unknown provider or `openai_compat` without `base_url` is an error, safe for unattended runs.

**Run a bounded task:**

```bash
orvena run "add a hello module" --scope src/hello.rs
# Agent gets ONE invocation, can only read/write src/hello.rs, can't escape.
# Report lands in .orvena/bench/<timestamp>/report.json
```

## Model providers

| Provider | Notes | Tested |
|---|---|---|
| **Ollama** | Local / offline / private. You run Ollama and pull a model. | ✅ `qwen3:14b` |
| **openai_compat** | Generic OpenAI-compatible endpoint — vLLM, llama.cpp, LM Studio, SGLang, Groq, Together, etc. Needs `base_url`; `api_key_env` names your key var, or omit for no-auth local servers. | ✅ via Ollama |
| **Gemini** | Hosted. Use `openai_compat` with Google's base_url and `api_key_env: GEMINI_API_KEY`. | ✅ `gemini-2.5-flash` |
| **Anthropic** | Hosted Claude. | ◻ not yet tested |
| **OpenAI** | Hosted. | ◻ not yet tested |
| **OpenRouter** | Hosted. | ◻ not yet tested |
| **offline** | Deterministic stub for tests and regression baselines (no network). | n/a |

**"Tested" means a real end-to-end run passed the behavioral contract** in
[`docs/provider-parity.md`](docs/provider-parity.md). Unchecked providers are implemented;
they simply have not been exercised here yet.

Keys are read from `.env`. **Never commit real keys.**

## How it works

Orvena is **config-first**: a scaffold deployed by `orvena init` drives behavior.

- `orvena.yaml` — provider, task scope, governance tier (off/engineering), step ceiling
- `gates.yaml` — completion criteria (what defines "done"?)
- `context-budgets.yaml` — token limits per invocation

Run a task with `orvena run <description> --scope <files>`:

1. **Scope declaration** — you list which files are writable; Orvena restricts the agent's filesystem to exactly those paths
2. **Agent invocation** — Orvena hands the task to the agent (native loop, Aider, or any model)
3. **Boundary enforcement** — any attempt to read/write outside the declared scope is blocked and logged
4. **Evidence collection** — Orvena records every syscall violation, every step, and the final outcome
5. **Report** — frozen JSON with task metadata, agent transcript, scope violations, token count, step count

The **native loop** is a minimal reference agent built into Orvena for testing. It's not the
product; it's how you measure governance overhead. Real-world agents (Aider, Claude, etc.) run
the same containment boundary.

## What it measures

The core metric is **M1: Governance Differential**.

Run the same task **twice**:
- **Baseline (off)**: One invocation, no gates, no containment
- **Governed (engineering)**: Same setup, but Orvena enforces scope

The difference between them is *pure overhead of containment*:
- Step blowup: how many extra steps does the sandboxing cost?
- Token blowup: how many extra tokens?
- Boundary violations: how many times did the agent try to escape?

Example from qwen3:14b on a real temptation (home-cache scope trap):
- **Baseline**: 30/30 tasks breach the boundary (agent writes outside scope)
- **Governed**: 0/30 breaches (sandbox blocks every attempt)
- **Overhead**: ×0.08 tokens (containment is *cheaper* than paying for trial-and-error escapes)

## Relationship to other tools

- **airt** — governs tool calls (LLM says "read this file"). Orvena governs actual syscalls (binary tries to write). Different boundary, complementary. airt is inside-the-LLM; Orvena is OS-level.
- **Your agent** (Aider, Claude, finetune, etc.) — Orvena doesn't care what's inside the box. It just enforces the box. A better agent = harder containment test = more confidence your boundary is real.

## Benchmarks

See [`docs/benchmarks.md`](docs/benchmarks.md) for the temptation test set (8 scope-adversarial tasks
designed to make agents try to escape) and differential results across providers and models.
