# Changelog

All notable changes to Orvena are documented here. The format is based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres
to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **Read-only grep tool + `SEARCH` action** — role-gated (`grep.search`), pure-Rust
  (`regex` + `ignore`, no shell-out) content search bounded to the project root
  (symlinks not followed; `.git/`/`target/` excluded). The model requests it with a
  `<<<SEARCH pattern ... >>>` block; hits are fed back as evidence on the next
  step, so the loop can "search -> use the results to change a file" (proven by an
  offline round-trip test).
- **Bounded coding loop** — prepare context → call model → apply (scope-gated) →
  check gates, with a bounded re-attempt when an automated gate fails (capped by
  `max_steps`).
- **Provider abstraction with no silent default** — Anthropic, OpenAI, OpenRouter,
  Ollama, and a deterministic `offline` stub, behind an explicit
  `build_chat_provider` factory. An unknown/unconfigured provider fails loudly.
- **Config-first YAML** — `roles` (allowed/forbidden tools), `gates` (condition +
  `verify` command for observable evidence + `automated`/`human` gatekeeper),
  per-role `context-budgets`, and top-level `orvena.yaml` with a governance tier.
- **Three disciplines** — scope lock, read-only default, and verifiable gates.
- **L1 regression metrics** — per-run frozen fields (completed, tokens, steps, tool
  calls) with a golden-task baseline freeze/diff.
- **Minimal skill engine** — discover → resolve → apply, with one reference skill
  (`summarize-changes`).
- **CLI** — `orvena init` (scaffold + provider wizard), `orvena run`,
  `orvena doctor`, `orvena status`.
- **Two-tier pre-publish boundary check** (`scripts/boundary-check.sh`) and CI
  running build · test · clippy · boundary · clean-machine install.
