# Orvena Public Positioning

**Status:** canonical local product wording · **Updated:** 2026-08-24

## One-sentence contract

> Orvena is an OS-enforced task-scope governance runtime for AI coding agents:
> bounded execution, verify gates, and frozen evidence by default.

## What Orvena owns

- task envelopes and declared writable scope;
- role/tool boundaries, step and context budgets;
- OS/filesystem containment for the wrapped process;
- automated or human completion gates; and
- a native evidence bundle describing what the run actually attempted.

## What Orvena does not own

- AINE's methodology, research, or reference-runtime roadmap;
- `aine-registry` source-of-truth discovery or `aine-control-plane` decisions;
- Organon's virtual-team, business, or organizational semantics;
- `airt`'s inside-the-LLM/tool-call boundary; or
- a second multi-agent executor, hosted orchestration service, or deployment
  authority.

An explicitly supplied Orvena report may be projected into a report-only
`integration-observation.v1` record. The projection carries normalized claims
and a digest; it does not rewrite the native evidence schema, read a private run
store, or grant execution authority to the consumer.

## Proposed GitHub About text

```text
OS-enforced task-scope governance runtime for AI coding agents — bounded execution, verify gates, and frozen evidence.
```

This is intentionally narrower than “a customizable coding agent”: the native
loop is a reference harness and the product boundary is enforcement around an
agent, whether that agent is native, Aider, Claude, Codex, or another supported
adapter.

## Release synchronization rule

The public repository must not imply that local-only adapter, benchmark, or
retention experiments are released. Before publishing local changes:

1. compare local `HEAD`, `origin/main`, and the remote `HEAD`;
2. classify each delta as product behavior, evidence, documentation, or local
   benchmark output;
3. update `CHANGELOG.md` and the workspace version/tag when behavior is
   intentionally released;
4. run `cargo fmt --all -- --check` and `cargo test --workspace`; and
5. confirm no push or GitHub repository-setting change occurred outside an
   explicit release action.

The pre-release comparison captured on 2026-08-24 is retained in
`docs/release-sync/orvena-public-sync-2026-08-24.json`. It records the release
candidate as unreleased at capture time; after a release, run the checker again
to produce the next synchronization snapshot rather than rewriting this audit
record.
