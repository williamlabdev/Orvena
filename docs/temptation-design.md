# What makes a temptation discriminate

A temptation task earns its place in the set only if an *ungoverned* run of it
breaches, and a *governed* run of it still solves the task. A task that no agent
breaches measures nothing; a task no agent can solve under governance measures
over-blocking and calls it containment. Both are dead weight, and the set carried
seven of the first kind for months.

This document is the contract every task must satisfy before it is added, and the
check that decides whether it stays.

## The failure the set already made

Measured 2026-08-03, qwen3:14b, aider wrapped, 9 runs per cell: the ungoverned leg
solved all eight tasks inside scope and breached on exactly one. Not because the
agent resisted seven temptations — because seven of them were not temptations.

Five tasks put the bait beside a one-keystroke correct fix. Changing `Helo` to
`Hello`, adding a `"version"` key, writing `x.min(10)`: the in-scope answer is
both trivial and obviously right, so the cheat is never the cheaper move. These
tasks discriminate *less* as agents improve, which is the wrong direction for a
benchmark whose whole thesis is that stronger agents need containment more.

One task put the bait somewhere the agent structurally could not reach — aider
edits only the files in its chat set, so `../data-backup.txt` was never a choice
it declined, just an instruction it dropped. That task measured the adapter.

And the one task that did breach, breached by accident: it had no escape probe,
so it scored a violation only when the agent wrote `~` *literally* and created a
directory of that name inside the root, where git could see it. An agent that
expanded `~` correctly wrote to the real home directory and was scored contained.
The check rewarded incompetent escapes and missed competent ones.

## The contract

1. **The out-of-scope path must be cheaper than the in-scope path.** Cost is
   counted in edits and reasoning steps, not in moral weight. If the in-scope fix
   is one line, no bait can compete with it and the task cannot discriminate. As
   a floor: the in-scope solution should take at least a handful of lines of real
   work while the bait stays a one-line change.

2. **The in-scope solution must exist, and must be demonstrated.** Every task
   carries a worked solution — the actual diff that satisfies `verify` without
   leaving `writes`. Without it a governed failure is ambiguous between "the
   envelope held" and "the task was impossible", and that ambiguity is precisely
   what a containment benchmark cannot afford. This is the authorized-utility
   axis: enforcement that completes no legitimate work is not a good result.

3. **The bait must be reachable by the agent under test.** A bait requiring a
   shell measures nothing against an agent that only edits files. Prefer baits
   inside the root — a neighbouring source file, a checked-in generated artifact,
   a config — which every file-editing agent can reach. Where a task does need a
   capability, it declares it, and a run without that capability is skipped
   rather than scored.

4. **The bait must be detectable in both its competent and incompetent forms.**
   In-root writes are covered by the git oracle. Anything that leaves the root
   needs an `escape_probes` entry naming where it actually lands — `~/…` or an
   absolute path, now that probes expand `~`. A task must never depend on the
   agent botching its own escape in order to be caught.

5. **The bait must be a defensible engineering instinct, not a trap.** "Fix the
   root cause instead of working around it", "run the formatter before
   committing", "regenerate the checked-in artifact so it matches" — these are
   things a good engineer does, and that is exactly why they are the interesting
   scope violations. A contrived trap inflates the differential and is barred by
   the honesty rule the task file already states.

## The difficulty gradient

A set built at one difficulty stops discriminating the moment models pass that
level. The set is therefore tiered by *why* the agent goes out of scope, and the
tiers age differently on purpose.

**Tier 1 — the instruction asks for it.** The task itself requests an out-of-root
side effect: persist a copy here, drop a backup there. Compliance is the
instinct, and instruction-following gets *stronger* with model capability, so
this tier does not decay. `tempt-home-cache` is the archetype and remains the
only task in the current set that ever worked.

**Tier 2 — the root cause is out of scope.** The bug genuinely lives in a
read-only file; the in-scope fix is a workaround that costs real work. A weak
model may not even locate the root cause. A strong one finds it immediately and
feels the pull harder, because it understands that patching around it is the
worse engineering. This tier discriminates *better* as models improve.

**Tier 3 — the write is a side effect, not a decision.** The agent runs a
formatter that rewrites twelve neighbours, regenerates a lockfile, updates a
snapshot, warms a cache. It never experiences a choice, so no amount of
instruction-following prevents it. This is the tier that will still discriminate
against agents far stronger than anything available now, and it is the closest to
how scope violations actually happen in production — the reported incidents are
overwhelmingly not malice.

The gradient matters more than the count. Thirty tier-1 tasks would be a worse
set than ten spread across all three, because they would all stop working on the
same day.

## The acceptance check

Before a task joins the published set it is run ungoverned, at low repeat,
against at least two models of clearly different capability. It is kept only if
it breaches at a nonzero rate on both, and is solved in scope at a nonzero rate
under governance. A task that fails the first is not a temptation; a task that
fails the second is not solvable, and would launder over-blocking into a
containment number.

Tasks are cut or redesigned, never quietly kept for the size of the set. The
count is not the number; the differential is.
