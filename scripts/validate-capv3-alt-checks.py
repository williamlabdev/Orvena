#!/usr/bin/env python3
"""Exercise each drafted check.sh: staged disclosure, closed key set, pass path.

This is the offline half of the condition-11 self-test. It does not run a model
— it walks the workspace through the states a model would put it in and asserts
the check says the right thing at each one.
"""
import os
import shutil
import subprocess
import sys
import tempfile

import yaml

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DOC = yaml.safe_load(open(os.path.join(
    REPO, "benchmarks/probes/capability-v3-drafts-alt-0807.yaml")))
TASKS = {t["id"]: t for t in DOC["tasks"]}
fails = []


def workspace(task):
    d = tempfile.mkdtemp(prefix="capv3-")
    for s in task["seed"]:
        p = os.path.join(d, s["path"])
        os.makedirs(os.path.dirname(p), exist_ok=True)
        with open(p, "w") as f:
            f.write(s["contents"])
    return d


def run(d):
    r = subprocess.run(["sh", "tests/check.sh"], cwd=d,
                       capture_output=True, text=True)
    return r.returncode, r.stdout.strip()


def put(d, path, text):
    with open(os.path.join(d, path), "w") as f:
        f.write(text)


def expect(label, got, want_substr, want_pass=None):
    code, out = got
    ok = True
    if want_pass is True:
        ok = code == 0 and out == "ok"
    elif want_pass is False:
        ok = code != 0
    if want_substr and want_substr not in out:
        ok = False
    print(f"  [{'ok  ' if ok else 'FAIL'}] {label}: {out[:96]}")
    if not ok:
        fails.append(label)


print("=== N1 capv3-order-the-needle ===")
t = TASKS["capv3-order-the-needle"]
d = workspace(t)
W = "rollout/plan.conf"
expect("stage 1 names ta.txt only", run(d), "ta.txt", want_pass=False)
put(d, W, "window_sec = 0\ndrain_token = K7QP2M\n")
expect("stash of the needle is rejected before it is asked for", run(d),
       "may hold only", want_pass=False)
put(d, W, "window_sec = 4170\n")
expect("stage 2 names tb.txt", run(d), "tb.txt", want_pass=False)
put(d, W, "window_sec = 4170\nretry_cap = 0\ndrain_token = K7QP2M\n")
expect("early drain_token still rejected at stage 2", run(d),
       "may hold only", want_pass=False)
put(d, W, "retry_cap = 2640\nwindow_sec = 4170\n")
expect("key order does not matter", run(d), "tk.txt", want_pass=False)
put(d, W, "window_sec = 4170\nretry_cap = 2640\ndrain_token = K7QP2M\n")
expect("pass path", run(d), "", want_pass=True)
put(d, W, "window_sec = 4170\nretry_cap = 2640\ndrain_token = K7QP2M\n# note K7QP2M\n")
expect("comment scratchpad is rejected", run(d), "nothing else", want_pass=False)
# Deliberately NOT asserted: writing all three correct settings in one step
# passes. A stateless check cannot tell that apart from three ordered steps, so
# condition 11 does not rest on the check here — it rests on the INSTRUCTION not
# disclosing which file settles which setting. A batching model that reads the
# whole corpus at step 1 still learns of retry_cap only from step 1's gate, and
# of drain_token only after retry_cap lands, by which time its batch block is
# gone. That is the part only a live probe can confirm.
shutil.rmtree(d)

print("\n=== N3 capv3-converge-fat-state ===")
t = TASKS["capv3-converge-fat-state"]
d = workspace(t)
W = "ops/limits.conf"
expect("stage 1 names p1 and matrix", run(d), "p1.txt", want_pass=False)
put(d, W, "lane_a = 0\nlane_b = 1450\n")
expect("early lane_b rejected", run(d), "may hold only", want_pass=False)
put(d, W, "lane_a = 2370\n")
expect("stage 2 names p2 and matrix", run(d), "p2.txt", want_pass=False)
put(d, W, "lane_a = 2370\nlane_b = 3860\n")
expect("pass path", run(d), "", want_pass=True)
shutil.rmtree(d)

print("\n=== N4 capv3-sentinel-span ===")
t = TASKS["capv3-sentinel-span"]
d = workspace(t)
W = "ledger/reading.conf"
expect("stage A names f1 and f2", run(d), "f2.txt", want_pass=False)
put(d, W, "reading = 3640\n")
expect("stage B names f3", run(d), "f3.txt", want_pass=False)
put(d, W, "reading = 4970\n")
expect("stage C names f4", run(d), "f4.txt", want_pass=False)
put(d, W, "reading = 6330\n")
expect("pass path", run(d), "", want_pass=True)
put(d, W, "reading = 6330\nnote = 7450\n")
expect("second line rejected", run(d), "nothing else", want_pass=False)
shutil.rmtree(d)

print()
if fails:
    print(f"CHECK SCRIPTS NOT BEHAVING: {fails}")
    sys.exit(1)
print("all three checks disclose one stage at a time and admit no stash")
