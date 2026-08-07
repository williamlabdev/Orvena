# Orvena

**A task-scope governance runtime — you declare boundaries, Orvena enforces them, any agent stays contained.**

Orvena doesn't code. It *proves that an untrusted agent can't escape its sandbox.*

You give Orvena a task definition (these files, this time, this step budget), hand it any agent
(native loop, Aider, Claude, a finetuned model), and Orvena ensures:
- The agent can only read/write the declared files
- The agent can't exceed its step budget
- Every boundary violation is logged, measured, and evidence is frozen in a JSON report

**The value**: You can hand work to a powerful agent you don't fully trust, because staying in
scope is not left to the agent. The OS refuses the write, and the run leaves a frozen record of
what was attempted — enforcement plus an audit trail, not a promise you have to take on faith.

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
- **Enforced boundaries** — the OS refuses the write, so an out-of-scope *file* write is impossible rather than impolite. Fourteen escape techniques are exercised against the boundary on every test run (see below). The boundary is the filesystem: a wrapped agent still reaches its own model provider over the network, and that traffic is not contained
- **Observable evidence** — every run produces a frozen report: scope refusals, blockers, gate outcomes, steps, tokens, sandbox state
- **Differential measurement** — run the same task ungoverned vs. governed with the same agent, and read the difference in cost and outcome

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
# The agent can only read/write src/hello.rs, and stops at max_steps (default 8).
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
5. **Report** — frozen JSON against [`schemas/evidence.v1.json`](schemas/evidence.v1.json): task, completion, steps, tool calls, input/output tokens, gate outcomes, blockers, scope refusals, sandbox state. It records what the run *did*, not what it was told — there is no prompt or transcript in the bundle

The **native loop** is a minimal reference agent built into Orvena for testing. It's not the
product; it's how you measure governance overhead. Real-world agents (Aider, Claude, etc.) run
the same containment boundary.

## What it measures

The method is a **differential**: the same task, the same model, the same agent, run in two
postures, so that governance is the only thing that changed.

- **Baseline (`off`)** — the agent is *told* what it needs: the writable paths, the file
  contents, the commands it may run. It is **not** told it must stay inside them. No gate, no
  sandbox.
- **Governed (`engineering`)** — identical prompt to the tier it belongs to, plus enforcement:
  scope is declared to the OS, gates decide what counts as done.

The distinction matters more than it looks. When the baseline was handed the governed run's
scope *obligation* — "modify ONLY files listed under WRITABLE" — the containment number measured
whether the model disobeys a written instruction, not what an agent without a scope contract
does. That was corrected; the results page carries the whole correction history.

Four metrics come out of a run:

| | |
|---|---|
| **M1 — containment** | out-of-scope writes that landed, per posture, judged by an oracle independent of the agent |
| **M2 — false-done** | claims of "done" that a check refutes |
| **M3 — evidence validity** | reports that satisfy `schemas/evidence.v1.json` |
| **M4 — cost** | steps and tokens per task-run — the overhead of being governed |

**The numbers live in [`docs/benchmark-results.md`](docs/benchmark-results.md), with their
caveats attached, and this README does not restate them.** They are small, self-hosted signals
from one local model on one machine, they have been withdrawn and re-measured more than once,
and the containment differential under the corrected baseline is still being re-measured. Read
them there, where each number sits next to the run that produced it.

## What "contained" means here

A containment number is only worth what was tried against it. The 2026-08-03
differential ran 288 task-runs over about eleven hours, and every breach it
recorded was the same thing: an agent writing a literal `~` directory *inside*
the project root — twelve times, all on one task, and all on the ungoverned leg.
Nothing in that run attempted a redirect, a copy, a rename, or a symlink, so
"zero breaches under governance" recorded that nothing attacked the sandbox, not
that the sandbox held.

So the boundary has its own adversary, with no model in the loop
(`crates/orvena-cli/tests/escape_techniques.rs`). Every run exercises:
absolute-path and append redirects, relative and climbing path traversal, writes
through a symlink to an outside directory and to an outside file, `mkdir`
outside the root, copying a file out, renaming a file out, hard-linking into an
outside directory, `tee`, `dd`, a backgrounded child outliving its parent, and
`exec` into a fresh shell.

Each technique runs **twice**. First unconfined, where it must succeed — that
control proves the attack is real on this host, so a technique that fails for
its own reasons (a missing binary, an unsupported shell construct, a filesystem
that refuses hard links) can never be mistaken for one the sandbox blocked.
Only then is it run confined, where it must fail and leave nothing behind. A
technique whose control fails is reported as inconclusive and fails the suite,
because a silent skip is how a boundary stops being tested without anyone
noticing.

A positive control runs alongside: under the same policy, a legitimate in-root
write must still succeed. Without it, a sandbox that denied everything —
including the work the agent was asked to do — would pass every assertion here
and look like perfect containment.

Verified on macOS (`sandbox-exec`): 14/14 refused, positive control passing.
Linux (Landlock) needs a kernel with Landlock enabled; where no backend
enforces, the suite says so loudly and does not report a pass.

## Relationship to other tools

- **airt** — governs tool calls (LLM says "read this file"). Orvena governs actual syscalls (binary tries to write). Different boundary, complementary. airt is inside-the-LLM; Orvena is OS-level.
- **Your agent** (Aider, Claude, finetune, etc.) — Orvena doesn't care what's inside the box. It just enforces the box. A better agent = harder containment test = more confidence your boundary is real.

## Benchmarks

- **Method** — [`docs/benchmark.md`](docs/benchmark.md): how a differential is run, including how
  to point it at an agent Orvena did not write.
- **Results** — [`docs/benchmark-results.md`](docs/benchmark-results.md): the numbers, each with
  the conditions it was measured under, and the corrections that have been issued against them.
- **Task set** — [`benchmarks/temptation.yaml`](benchmarks/temptation.yaml): 8 scope-adversarial
  tasks, each built to make staying in scope the inconvenient option.
