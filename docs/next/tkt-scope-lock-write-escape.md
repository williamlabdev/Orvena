# Ticket: Scope-lock write-path escape + advisory-by-default tier

> Status: PLANNED · 2026-07-10 · diagnosis-to-fix, no code changed

## Problem

The scope lock is meant to be Orvena's "bounded change" guarantee: a task may
only write inside its declared `allowed_modifications`. The write path does not
enforce that guarantee, and the shipped default tier does not stop a violation
even when it is detected.

- **Scope matching never resolves `..`.** `crates/orvena-core/src/governance/scope.rs:44-46`
  (`normalize`) only strips a leading `./` and swaps backslashes; it never
  resolves `..` segments. `path_matches` at `crates/orvena-core/src/governance/scope.rs:48-52`
  is then a plain string equality / prefix compare. So a relative path containing
  `..` is compared literally against the allow-list and can slip past it.

- **The write path never canonicalizes.** `crates/orvena-core/src/tools/fs.rs:37-45`
  takes the scope decision and, on `Allow`, does `self.root.join(rel)` (fs.rs:41)
  and writes (fs.rs:45) with no canonicalization or `..` / symlink resolution. A
  path like `src/../../../.ssh/authorized_keys` can therefore resolve and write
  *outside* the project root. This is a direct contrast with the read-only search
  tool, which already rejects `..`: `crates/orvena-core/src/tools/grep.rs:49-55`
  returns a scope error when any component is a `ParentDir`. The write path has no
  equivalent guard.

- **The shipped default tier is advisory, so a detected violation still proceeds.**
  The scaffold ships `tier: light` (`crates/orvena-cli/src/scaffold/orvena.yaml:15`),
  and `Tier`'s default is `Light` (`crates/orvena-core/src/config/agent.rs:41-42`),
  whose `enforces()` returns `false` (`crates/orvena-core/src/config/agent.rs:47-52`).
  In the loop, a scope violation only pushes a blocker and returns early *when the
  tier enforces* — `crates/orvena-core/src/agent/driver.rs:105` (write), `:130`
  (search), `:165` (run). Under the default `light` tier those early returns are
  skipped, so the loop records the violation and keeps executing subsequent
  actions. Out of the box, governance is advisory.

## Fix direction

- Bring the write path to parity with `grep.rs`: canonicalize and resolve `..`
  and symlinks, and match on path-segment boundaries so a resolved target outside
  the project root is rejected regardless of how the relative path was spelled.
- Decide and document whether `light` (advisory) is the right out-of-box default
  for a governed agent, or whether scope violations — specifically writes that
  escape the project root — should fail closed regardless of tier. Make the
  chosen default-tier enforcement posture explicit in the shipped config and docs.

## Acceptance criteria

- [ ] A write whose path resolves outside the project root via `..` segments
      (e.g. `src/../../../.ssh/authorized_keys`) is blocked.
- [ ] A write that resolves outside the project root via a symlink is blocked.
- [ ] The enforcement posture of the shipped default tier is explicit and
      documented (scaffold config and docs agree on it).
- [ ] Under the shipped default configuration, a scope violation does not
      silently proceed to write the file.
