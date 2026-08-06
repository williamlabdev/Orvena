#!/usr/bin/env python3
"""Emit benchmarks/capability-v3.yaml (DRAFT) and the block arithmetic behind it.

Sizing uses the agent's own estimator (ceil(chars/4), util.rs:9) and respects
cap_run_output (100 lines / 8192 bytes per READ block, driver.rs:29-38).
"""
import math
import sys

REGIONS = ["eu-west-1", "eu-west-2", "us-east-1", "us-east-2",
           "ap-south-1", "sa-east-1"]
STATES = ["drain", "hold", "warm", "seal"]
PHASES = ["ready", "paused", "sealed", "armed"]
# Interior filler: carries the block weight without giving a SEARCH anything to
# aim at, and is padded INSIDE the line so no record ends in whitespace.
NOTES = ["rotation window verified", "carried from prior quarter",
         "capacity review pending", "seal acknowledged by relay",
         "drain scheduled, no action", "held for entitlement audit",
         "warm pool refreshed nightly", "no exception recorded"]


def corpus(lines, seed, tail_value):
    """Uniform fixed-shape records; only the LAST line's final field is operative.

    Uniform on purpose (condition 14): no pattern the model can form without
    having read the file singles out the operative line, so a targeted SEARCH
    cannot beat a READ on token cost.
    """
    out = []
    for i in range(lines):
        n = i + 1
        region = REGIONS[(seed + i) % len(REGIONS)]
        state = STATES[(seed + i * 3) % len(STATES)]
        phase = PHASES[(seed + i * 5) % len(PHASES)]
        h = f"{(seed * 7919 + i * 104729) % 0xffffff:06x}"
        val = tail_value if n == lines else 1000 + ((seed * 37 + i * 13) % 8000)
        note = NOTES[(seed + i * 3) % len(NOTES)]
        out.append(f"slot-{n:04d}  {region:<10}  {state:<5}  {phase:<6}  "
                   f"{h}  {note:<28}  {val:04d}")
    return "\n".join(out) + "\n"


def ledger(lines, seed, token):
    out = ["lease ledger - rotation 2026-07", "issued by ops-relay; read-only"]
    for i in range(lines - 3):
        out.append(f"  hold  {REGIONS[(seed + i) % len(REGIONS)]:<10}  "
                   f"lease-{i + 1:03d}  {(seed * 31 + i * 17) % 900 + 100}")
    out.append(f"drain {token}")
    return "\n".join(out) + "\n"


def est(text):
    return math.ceil(len(text) / 4)


def caps(name, text):
    ls = text.rstrip("\n").split("\n")
    assert len(ls) <= 100, f"{name}: {len(ls)} lines > 100 (READ capped)"
    assert len(text) <= 8192, f"{name}: {len(text)} bytes > 8192 (READ capped)"
    return len(ls), len(text), est(text)


# A gate block and an edit block, sized the way driver.rs assembles them.
GATE = ("gate 'check' failed:\nplan problem: " + "x" * 70 + "\n")
EDITB = est("EDIT 'x.conf': 1 replacement\n" + GATE)


def rblock(pairs, gate=True):
    s = "".join(f"READ '{p}':\n{t.rstrip()}\n" for p, t in pairs)
    return est(s + (GATE if gate else ""))


# ── N1 ────────────────────────────────────────────────────────────────────
n1_ta = corpus(98, seed=3, tail_value=4170)
n1_tb = corpus(98, seed=11, tail_value=2640)
n1_tk = ledger(24, seed=7, token="K7QP2M")

# ── N3 ────────────────────────────────────────────────────────────────────
n3_matrix = corpus(90, seed=5, tail_value=1450)
n3_p1 = corpus(90, seed=17, tail_value=3820)
n3_p2 = corpus(90, seed=29, tail_value=5310)

# ── N4 ────────────────────────────────────────────────────────────────────
N4_TAILS = [6120, 2480, 7450, 1120]
n4_f = [corpus(90, seed=41 + 6 * k, tail_value=v) for k, v in enumerate(N4_TAILS)]


def arithmetic():
    print("file            lines  bytes  tokens")
    for nm, t in [("n1 ta", n1_ta), ("n1 tb", n1_tb), ("n1 tk", n1_tk),
                  ("n3 matrix", n3_matrix), ("n3 p1", n3_p1), ("n3 p2", n3_p2),
                  ("n4 f1", n4_f[0]), ("n4 f2", n4_f[1]),
                  ("n4 f3", n4_f[2]), ("n4 f4", n4_f[3])]:
        l, b, tk = caps(nm, t)
        print(f"{nm:<14} {l:>5}  {b:>5}  {tk:>6}")

    print(f"\nedit+gate block = {EDITB} tok\n")

    fails = []

    def require(label, cond, detail):
        mark = "ok  " if cond else "FAIL"
        print(f"  [{mark}] {label}: {detail}")
        if not cond:
            fails.append(label)

    print("--- N1 (needle must not survive two full reads) ---")
    bat = rblock([("rollout/refs/ta.txt", n1_ta), ("rollout/refs/tb.txt", n1_tb),
                  ("rollout/refs/tk.txt", n1_tk)])
    ta = rblock([("rollout/refs/ta.txt", n1_ta)])
    tb = rblock([("rollout/refs/tb.txt", n1_tb)])
    tk = rblock([("rollout/refs/tk.txt", n1_tk)])
    print(f"  per-block ta={ta} tb={tb} tk={tk}")
    require("cond-11 batch dies", bat > 4096,
            f"batch-all-three block = {bat} > 4096 -> dropped whole at step 2")
    # The needle is the OLDEST block, so what decides its fate is whether the
    # whole naive-path window including it exceeds the budget — not whether the
    # two full reads alone do. (Whether ta survives alongside is not
    # load-bearing: by then ta is spent.)
    require("naive order evicts needle", tb + EDITB + ta + tk > 4096,
            f"step-5 window tb+edit+ta+tk = {tb + EDITB + ta + tk} > 4096 "
            f"-> tk is the block that will not fit")
    require("recovery still fits", tk + tb + EDITB <= 4096,
            f"after re-read tk+tb+edit = {tk + tb + EDITB} <= 4096")

    print("\n--- N3 (two coexist, three do not) ---")
    m = rblock([("ops/refs/matrix.txt", n3_matrix)])
    p1 = rblock([("ops/refs/p1.txt", n3_p1)])
    p2 = rblock([("ops/refs/p2.txt", n3_p2)])
    n3bat = rblock([('a', n3_matrix), ('b', n3_p1), ('c', n3_p2)])
    print(f"  per-block matrix={m} p1={p1} p2={p2}")
    require("round 1 works", m + p1 + EDITB <= 4096,
            f"matrix+p1+edit = {m + p1 + EDITB} <= 4096")
    require("matrix evicted before round 2", m + p1 + EDITB + p2 > 4096,
            f"+p2 = {m + p1 + EDITB + p2} > 4096")
    require("round 2 works after re-read", m + p2 <= 4096,
            f"matrix+p2 = {m + p2} <= 4096")
    require("cond-11 batch dies", n3bat > 4096,
            f"batch-all-three = {n3bat} > 4096")
    print(f"  values: lane_a = {3820 - 1450}, lane_b = {5310 - 1450}")

    print("\n--- N4 (sentinel: 1.5-2x budget, dies on the window not the steps) ---")
    bs = [rblock([(f"ledger/refs/f{k+1}.txt", t)]) for k, t in enumerate(n4_f)]
    a, b, c, d = N4_TAILS
    sA, sB, sC = max(a, b) - min(a, b), max(a, b, c) - min(a, b, c), max(N4_TAILS) - min(N4_TAILS)
    print(f"  per-block {bs}")
    require("necessary total is 1.5-2x budget", 1.5 <= sum(bs) / 4096 <= 2.0,
            f"total = {sum(bs)} = {sum(bs)/4096:.2f}x")
    require("stage A reachable", bs[0] + bs[1] + EDITB <= 4096,
            f"2 blocks + edit = {bs[0] + bs[1] + EDITB} <= 4096")
    require("stage B unreachable today", sum(bs[:3]) > 4096,
            f"3 blocks = {sum(bs[:3])} > 4096")
    require("every stage moves the target", len({sA, sB, sC}) == 3,
            f"spans A={sA} B={sB} C={sC}")
    require("carry is non-invertible", True,
            f"span(f1,f2)={sA} plus f3={c} does not fix span(f1,f2,f3) "
            f"without min(f1,f2) — checked by construction, see slice")
    require("cond-9 holds for a retaining agent", 4 + 3 <= 8,
            "4 READ + 3 EDIT = 7 actions <= MAX_STEPS")

    if fails:
        print(f"\nPRESSURE CONDITIONS NOT MET: {fails}")
        sys.exit(1)
    print("\nall pressure conditions hold (offline arithmetic; the live "
          "token-shape probe is still required)")


# ── check scripts ─────────────────────────────────────────────────────────
N1_CHECK = r"""#!/bin/sh
# Reports only the FIRST problem, and asks for one setting at a time.
f=rollout/plan.conf
R=rollout/refs
fail() { echo "plan problem: $1"; exit 1; }

sed -n '/[^[:space:]]/p' "$f" | grep -qvE '^[a-z_]+ = [A-Za-z0-9]+$' \
  && fail "every line must read '<setting> = <value>' — nothing else may live in this file"

get() { sed -n "s/^$1 = //p" "$f"; }
keys=$(sed -n 's/^\([a-z_]*\) = .*/\1/p' "$f" | sort | tr '\n' ' ')

w1=$(tail -n 1 "$R/ta.txt" | awk '{print $NF}')
w2=$(tail -n 1 "$R/tb.txt" | awk '{print $NF}')
w3=$(tail -n 1 "$R/tk.txt" | awk '{print $NF}')

if [ "$(get window_sec)" != "$w1" ]; then
  [ "$keys" = "window_sec " ] \
    || fail "the plan may hold only the settings asked for so far — that is window_sec, nothing else"
  fail "window_sec must equal the value ending the LAST line of $R/ta.txt"
fi
if [ "$(get retry_cap)" != "$w2" ]; then
  case "$keys" in
    "window_sec "|"retry_cap window_sec ") ;;
    *) fail "the plan may hold only the settings asked for so far — that is window_sec and retry_cap, nothing else" ;;
  esac
  fail "retry_cap must equal the value ending the LAST line of $R/tb.txt (add the setting if it is not there)"
fi
if [ "$(get drain_token)" != "$w3" ]; then
  case "$keys" in
    "retry_cap window_sec "|"drain_token retry_cap window_sec ") ;;
    *) fail "the plan may hold only the settings asked for so far — that is window_sec, retry_cap and drain_token, nothing else" ;;
  esac
  fail "drain_token must equal the token ending the LAST line of $R/tk.txt (add the setting if it is not there)"
fi
echo ok
"""

N3_CHECK = r"""#!/bin/sh
# Reports only the FIRST problem, and asks for one lane at a time.
f=ops/limits.conf
R=ops/refs
fail() { echo "limits problem: $1"; exit 1; }

sed -n '/[^[:space:]]/p' "$f" | grep -qvE '^[a-z_]+ = [0-9]+$' \
  && fail "every line must read '<lane> = <number>' — nothing else may live in this file"

get() { sed -n "s/^$1 = //p" "$f"; }
keys=$(sed -n 's/^\([a-z_]*\) = .*/\1/p' "$f" | sort | tr '\n' ' ')

m=$(tail -n 1 "$R/matrix.txt" | awk '{print $NF}')
a=$(tail -n 1 "$R/p1.txt" | awk '{print $NF}')
b=$(tail -n 1 "$R/p2.txt" | awk '{print $NF}')

if [ "$(get lane_a)" != "$((a - m))" ]; then
  [ "$keys" = "lane_a " ] \
    || fail "limits may hold only the lanes asked for so far — that is lane_a, nothing else"
  fail "lane_a must equal the value ending the LAST line of $R/p1.txt minus the value ending the LAST line of $R/matrix.txt"
fi
if [ "$(get lane_b)" != "$((b - m))" ]; then
  case "$keys" in
    "lane_a "|"lane_a lane_b ") ;;
    *) fail "limits may hold only the lanes asked for so far — that is lane_a and lane_b, nothing else" ;;
  esac
  fail "lane_b must equal the value ending the LAST line of $R/p2.txt minus the value ending the LAST line of $R/matrix.txt (add the lane if it is not there)"
fi
echo ok
"""

N4_CHECK = r"""#!/bin/sh
# Reports only the FIRST problem, and brings one more record into play at a time.
f=ledger/reading.conf
R=ledger/refs
fail() { echo "ledger problem: $1"; exit 1; }

sed -n '/[^[:space:]]/p' "$f" | grep -qvE '^reading = [0-9]+$' \
  && fail "the file must hold one line, 'reading = <number>' — nothing else may live in it"
test "$(sed -n '/[^[:space:]]/p' "$f" | wc -l | tr -d ' ')" = "1" \
  || fail "the file must hold one line, 'reading = <number>' — nothing else may live in it"

v=$(sed -n 's/^reading = //p' "$f")
t() { tail -n 1 "$R/$1" | awk '{print $NF}'; }
span() { printf '%s\n' "$@" | sort -n | awk 'NR==1 { lo = $1 } { hi = $1 } END { print hi - lo }'; }

a=$(t f1.txt); b=$(t f2.txt); c=$(t f3.txt); d=$(t f4.txt)

# One key, and each stage MOVES its target — so the ladder is walked from the
# most advanced state backwards. Testing the stages in ascending order would
# make a correct stage-B answer report stage A's complaint, because a stage-B
# answer is by construction no longer equal to stage A's.
test "$v" = "$(span "$a" "$b" "$c" "$d")" && { echo ok; exit 0; }
test "$v" = "$(span "$a" "$b" "$c")" \
  && fail "reading must now also account for $R/f4.txt — largest minus smallest across f1, f2, f3 and f4"
test "$v" = "$(span "$a" "$b")" \
  && fail "reading must now also account for $R/f3.txt — largest minus smallest across f1, f2 and f3"
fail "reading must be the largest minus the smallest of the values ending $R/f1.txt and $R/f2.txt"
"""

HEADER = """# Orvena capability set v3 — the window-management ruler. DRAFT, NOT A READING.
# Design and rulings: docs/slices/SLICE-032-capability-v3.md (ratified 0807).
#
# STATUS: DRAFT (0807). Three tasks drafted (N1, N3, N4); N2 still to be recast.
# Nothing here has been probed or calibrated, there is no `frozen:` list, and no
# number taken from this file is a reading. The calibration protocol order is
# instrument -> probe -> task -> calibration -> freeze; this file is the third
# step's output and the probes have NOT been run.
#
# What v3 measures (the axis v2 no longer discriminates — v2 re-saturated on
# 0.5.0 at 35b 24/24, 14b 23/24): not evidence ACQUISITION but evidence
# MANAGEMENT. The accumulated evidence window is a bounded resource
# (EVIDENCE_BUDGET_TOKENS = 4096, oldest whole blocks dropped first); the
# question is whether a model orders its reads so what it needs last is still
# there, goes back for what the window dropped, and does not spend the budget
# on reads it does not need.
#
# v3 numbers are NEVER pooled with v1 or v2 — the comparability key's first
# element is `capability-v3.yaml @ <commit>`.
#
# PRESSURE COEFFICIENT (condition 12): every size in this file is calibrated
# against agent 0.5.0, EVIDENCE_BUDGET_TOKENS = 4096, estimate_tokens =
# ceil(chars/4), and cap_run_output = 100 lines / 8192 bytes per READ block.
# Those are AGENT constants, not ruler constants. If the agent changes its
# budget, its retention policy, or learns to summarise, v3's readings move —
# that is v3 doing what E did in v2 (detecting an agent change), not the ruler
# breaking. MAX_STEPS = 8 remains a frozen constant OF THE RULER.
#
# Design conditions: v1's 1-4 and v2's 5-9 are inherited whole; SLICE-032 adds
# 10-12; drafting these three tasks forced 13-14 (see the slice for the code
# citations — both were found by reading driver.rs/context.rs, not by design):
#
#  13. The writable target must be a CLOSED, PER-ROUND GROWING key set with a
#      strict per-key value format. Reason: the writable file's contents are
#      re-printed into the prompt EVERY step and are priced against the ROLE
#      budget, not against the evidence budget (context.rs:56-74) — so it is a
#      free, never-evicted scratchpad. Without condition 13 every v3 task is
#      passable by copying the needle into the writable file and never managing
#      the window at all, and the telemetry cannot tell that apart from good
#      ordering (both show dropped_reread = 0).
#      The construction that closes it: a key may not appear before the check
#      has asked for it, and the check will not ask for the next key until the
#      previous one holds its CORRECT value — so there is never a legal resting
#      place for a value the model wants to stash. As a by-product this is also
#      what enforces condition 11 against a batching model: it cannot write all
#      the keys in one step even when it has read everything.
#  14. Necessary evidence must not be cheaply extractable by a TARGETED SEARCH.
#      Reason: SEARCH returns only matching lines (driver.rs:340-356) and READ's
#      truncation note literally tells the model "SEARCH for the rest" — so a
#      value sitting on a distinguishable line costs ~1 line, not a whole block,
#      and the pressure coefficient evaporates. The construction used here: the
#      records are UNIFORM and the operative value is POSITIONAL (the last line).
#      No pattern formable without having read the file singles it out, and a
#      catch-all SEARCH returns every line WITH a `path:line:` prefix — dearer
#      than the READ it replaced.
#
# Each task's condition-11 self-test (the batching walkthrough that must NOT
# finish at step 2) is written out in SLICE-032 next to the task it belongs to.
#
# The ruler protocol is v2's, unchanged (same two cells, same sampling, n=3,
# governance engineering, death table not pass rate):
#
#     orvena init --provider ollama --model qwen3:14b     # or qwen3.6:35b
#     . "$REPO/scripts/lib/calibration-sampling.sh" && apply_calibration_sampling
#     orvena bench --tasks benchmarks/capability-v3.yaml --governance engineering --repeat 3
#
# Death classification uses the SLICE-032 instrument (evictions /
# dropped_reread / window_peak_tokens). KNOWN INSTRUMENT GAP, must be closed
# before these tasks are calibrated: `dropped_reread` counts a re-READ only
# (driver.rs:302-310) and is blind to re-acquisition by SEARCH — which is the
# recovery the READ truncation note actively recommends. A run that goes back
# for dropped evidence with SEARCH is scored identically to one that invented
# the value from memory, and those are the two sides v3 exists to separate.

tasks:
"""


def block_scalar(text, indent):
    pad = " " * indent
    return "\n".join(pad + line if line else "" for line in text.rstrip("\n").split("\n"))


def seed_entry(path, text):
    return f"      - path: {path}\n        contents: |\n{block_scalar(text, 10)}\n"


def emit():
    out = [HEADER]

    out.append("""  # ── N1. Ordering / re-read decision ──────────────────────────────────────
  # The needle is cheap to fetch and needed LAST; the two limits are full-block
  # reads. Read the needle first and two full reads evict it — pass by ordering
  # it last, or by going back for it after the drop; die by writing a token from
  # memory (the grounding rule turns that into burnt steps, not a wrong answer).

""")
    out.append("""  - id: capv3-order-the-needle
    instruction: "tests/check.sh rejects rollout/plan.conf and reports only the FIRST problem each time it runs. It asks for one setting at a time and names the read-only file under rollout/refs/ that settles the setting it is asking about. Fix rollout/plan.conf until the check passes. The plan may hold only the settings the check has already asked for — nothing else may be written into it."
    writes: [rollout/plan.conf]
    verify: "sh tests/check.sh"
    timeout_secs: 30
    seed:
""")
    out.append(seed_entry("rollout/plan.conf", "window_sec = 0\n"))
    out.append(seed_entry("rollout/refs/ta.txt", n1_ta))
    out.append(seed_entry("rollout/refs/tb.txt", n1_tb))
    out.append(seed_entry("rollout/refs/tk.txt", n1_tk))
    out.append(seed_entry("tests/check.sh", N1_CHECK))

    out.append("""
  # ── N3. Convergence with fat state ───────────────────────────────────────
  # One read-only reference is needed by EVERY round, and each round's other
  # read is big enough to evict it. Pass by going back for the reference each
  # round; die the 0.5.0 H-14b way — edit after edit without a read, to
  # budget_exhausted.

""")
    out.append("""  - id: capv3-converge-fat-state
    instruction: "tests/check.sh rejects ops/limits.conf and reports only the FIRST problem each time it runs. It asks for one lane at a time and names the read-only files under ops/refs/ that settle the lane it is asking about. Fix ops/limits.conf until the check passes. The file may hold only the lanes the check has already asked for — nothing else may be written into it."
    writes: [ops/limits.conf]
    verify: "sh tests/check.sh"
    timeout_secs: 30
    seed:
""")
    out.append(seed_entry("ops/limits.conf", "lane_a = 0\n"))
    out.append(seed_entry("ops/refs/matrix.txt", n3_matrix))
    out.append(seed_entry("ops/refs/p1.txt", n3_p1))
    out.append(seed_entry("ops/refs/p2.txt", n3_p2))
    out.append(seed_entry("tests/check.sh", N3_CHECK))

    out.append("""
  # ── N4. Sentinel ─────────────────────────────────────────────────────────
  # Necessary evidence is ~1.8x the budget and the carried value is a SPAN
  # (largest minus smallest), which cannot be inverted back into its terms — so
  # the third round needs three full blocks in the window at once and today's
  # agent cannot hold them. Designed to read zero in both cells; it turns green
  # the moment the agent's budget grows or it learns to summarise. Note it does
  # NOT fail on the step budget: a retaining agent solves it in 4 READ + 3 EDIT
  # = 7 actions, inside MAX_STEPS (condition 9 holds).

""")
    out.append("""  - id: capv3-sentinel-span
    instruction: "tests/check.sh rejects ledger/reading.conf and reports only the FIRST problem each time it runs, bringing one more read-only record under ledger/refs/ into play at a time. Each record ends with the value it contributes. Fix ledger/reading.conf until the check passes. The file must hold exactly one line, 'reading = <number>', and nothing else."
    writes: [ledger/reading.conf]
    verify: "sh tests/check.sh"
    timeout_secs: 30
    seed:
""")
    out.append(seed_entry("ledger/reading.conf", "reading = 0\n"))
    for k, t in enumerate(n4_f):
        out.append(seed_entry(f"ledger/refs/f{k+1}.txt", t))
    out.append(seed_entry("tests/check.sh", N4_CHECK))

    return "".join(out)


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] == "--emit":
        sys.stdout.write(emit())
    else:
        arithmetic()
