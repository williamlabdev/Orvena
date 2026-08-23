#!/usr/bin/env python3
"""Validate capability-v2.yaml checks: seed must FAIL, golden fix must PASS.

For convergence tasks, also walk the reveal sequence and assert each expected
first-problem message appears in order.
"""
import subprocess, sys, tempfile, shutil
from pathlib import Path

try:
    import yaml
except ImportError:
    sys.exit("pyyaml missing")

REPO = Path(__file__).resolve().parents[1]
SET = yaml.safe_load((REPO / "benchmarks/capability-v2.yaml").read_text())
tasks = {t["id"]: t for t in SET["tasks"]}

def seed_dir(task):
    d = Path(tempfile.mkdtemp(prefix=task["id"] + "-"))
    for s in task["seed"]:
        p = d / s["path"]
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(s["contents"])
    return d

def check(d):
    r = subprocess.run(["sh", "tests/check.sh"], cwd=d, capture_output=True, text=True)
    return r.returncode, (r.stdout + r.stderr).strip()

def edit(d, path, old, new, count=1):
    p = d / path
    t = p.read_text()
    assert t.count(old) >= 1, f"{path}: anchor not found: {old!r}"
    p.write_text(t.replace(old, new, count))

failures = []

def run_case(tid, fixes, expect_msgs=None):
    """fixes: list of (path, old, new). expect_msgs: expected first-problem
    substrings, one per check invocation before each fix (convergence walk)."""
    t = tasks[tid]
    d = seed_dir(t)
    try:
        rc, out = check(d)
        if rc == 0:
            failures.append(f"{tid}: check PASSES on the unfixed seed (task is broken)")
            return
        if expect_msgs:
            for i, (msg, fix) in enumerate(zip(expect_msgs, fixes)):
                rc, out = check(d)
                if rc == 0:
                    failures.append(f"{tid}: check passed early at stage {i}")
                    return
                if msg not in out:
                    failures.append(f"{tid}: stage {i} expected {msg!r}, got {out!r}")
                    return
                edit(d, *fix)
        else:
            for fix in fixes:
                edit(d, *fix)
        rc, out = check(d)
        if rc != 0:
            failures.append(f"{tid}: golden fix does not pass: {out!r}")
        elif "ok" not in out:
            failures.append(f"{tid}: passing check printed {out!r}")
    finally:
        shutil.rmtree(d)

# A: point setup guide at docs/misc.md
run_case("capv2-locate-by-content",
         [("handbook.md", "setup guide: docs/notes-b.md", "setup guide: docs/misc.md")])

# B: rename in all three, decoy untouched
run_case("capv2-rename-propagate",
         [("app/api.env", "DB_HOST=", "PRIMARY_DB_HOST="),
          ("app/worker.env", "DB_HOST=", "PRIMARY_DB_HOST="),
          ("app/report.env", "DB_HOST=pg-eu2.internal", "PRIMARY_DB_HOST=pg-eu2.internal")])

# B-negative: a global replace that hits the decoy must FAIL
t = tasks["capv2-rename-propagate"]; d = seed_dir(t)
for f in ["app/api.env", "app/worker.env", "app/report.env"]:
    p = d / f
    p.write_text(p.read_text().replace("DB_HOST", "PRIMARY_DB_HOST"))
rc, out = check(d)
if rc == 0:
    failures.append("capv2-rename-propagate: global replace over the decoy PASSES (decoy is toothless)")
shutil.rmtree(d)

# C: ordered reveal — rotate first, keep second
run_case("capv2-migration-order",
         [("rotation.conf", "rotate = daily", "rotate = hourly"),
          ("rotation.conf", "keep = 14", "keep = 336")],
         expect_msgs=["rotate must be 'hourly'", "keep = 336"])

# D: fill from site-b (active)
run_case("capv2-active-config",
         [("deploy.plan", "endpoint =", "endpoint = edge-fra2.orvena.net:9443"),
          ("deploy.plan", "pool =", "pool = 36"),
          ("deploy.plan", "ttl =", "ttl = 600")])

# D-negative: filling from staged site-a must FAIL
t = tasks["capv2-active-config"]; d = seed_dir(t)
p = d / "deploy.plan"
p.write_text("# x\nendpoint = edge-ams1.orvena.net:9443\npool = 24\nttl = 300\n")
rc, out = check(d)
if rc == 0:
    failures.append("capv2-active-config: values from the STAGED site pass (verification is fake)")
shutil.rmtree(d)

# E: amber's token
run_case("capv2-two-hop-registry",
         [("client.conf", "feed_token =", "feed_token = 7b3e58d90f12")])

# E-negative: a plausible token from the wrong feed must FAIL
t = tasks["capv2-two-hop-registry"]; d = seed_dir(t)
edit(d, "client.conf", "feed_token =", "feed_token = 9d02f6c1a473")
rc, out = check(d)
if rc == 0:
    failures.append("capv2-two-hop-registry: the previous feed's token passes (index is decorative)")
shutil.rmtree(d)

# F: widen anchor to the replay block
run_case("capv2-anchor-triplet",
         [("routes.toml",
           "[route.replay]\npriority = high\nweight = 10",
           "[route.replay]\npriority = high\nweight = 40")])

# G: name the metrics service (and accept the lenient spellings)
for spelling in ["metrics", "metrics.conf", "services/metrics.conf"]:
    run_case("capv2-needle-semantic",
             [("remediation.txt", "service =", f"service = {spelling}")])

# G-negative: naming a service on a current policy must FAIL
t = tasks["capv2-needle-semantic"]; d = seed_dir(t)
edit(d, "remediation.txt", "service =", "service = billing")
rc, out = check(d)
if rc == 0:
    failures.append("capv2-needle-semantic: a wrong service passes")
shutil.rmtree(d)

# H: reveal order metrics -> archive -> total
run_case("capv2-converge-quota",
         [("quotas.conf", "team-metrics = 40", "team-metrics = 100"),
          ("quotas.conf", "team-archive = 80", "team-archive = 150"),
          ("quotas.conf", "total = 260", "total = 390")],
         expect_msgs=["team-metrics does not match", "team-archive does not match",
                      "total must equal"])

# I: point drill guide at briefs/tan.md
run_case("capv2-locate-by-anchor",
         [("runbook.md", "drill guide: briefs/red.md", "drill guide: briefs/tan.md")])

# J: rename three, west pinned
run_case("capv2-rename-with-exempt",
         [("sync/north.env", "ENDPOINT_URL=", "SERVICE_ENDPOINT="),
          ("sync/south.env", "ENDPOINT_URL=", "SERVICE_ENDPOINT="),
          ("sync/east.env", "ENDPOINT_URL=", "SERVICE_ENDPOINT=")])

# J-negative: renaming west too must FAIL
t = tasks["capv2-rename-with-exempt"]; d = seed_dir(t)
for f in ["sync/north.env", "sync/south.env", "sync/east.env", "sync/west.env"]:
    p = d / f
    p.write_text(p.read_text().replace("ENDPOINT_URL=", "SERVICE_ENDPOINT="))
rc, out = check(d)
if rc == 0:
    failures.append("capv2-rename-with-exempt: renaming the frozen line passes (exemption is toothless)")
shutil.rmtree(d)

# K: east (live) token, not north (disabled)
run_case("capv2-two-hop-stale",
         [("client.conf", "feed_token =", "feed_token = 5e97b30dc244")])

# K-negative: the disabled source's token must FAIL
t = tasks["capv2-two-hop-stale"]; d = seed_dir(t)
edit(d, "client.conf", "feed_token =", "feed_token = a81f24c9e630")
rc, out = check(d)
if rc == 0:
    failures.append("capv2-two-hop-stale: the disabled source's token passes (status column is decorative)")
shutil.rmtree(d)

# L: three fixes, linear reveal
run_case("capv2-converge-three",
         [("alerts.conf", "threshold_cpu = 95", "threshold_cpu = 80"),
          ("alerts.conf", "notify = none", "notify = pager"),
          ("alerts.conf", "window = 5m", "escalate_after = 15m\nwindow = 5m")],
         expect_msgs=["threshold_cpu must be 80", "notify must be 'pager'",
                      "escalate_after is missing"])

# M: four defects incl. the missing relay line, then total = 450
run_case("capv2-converge-quota-four",
         [("quotas.conf", "team-metrics = 40", "team-metrics = 100"),
          ("quotas.conf", "team-archive = 80", "team-archive = 150"),
          ("quotas.conf", "reserved = 20", "team-relay = 60\nreserved = 20"),
          ("quotas.conf", "total = 260", "total = 450")],
         expect_msgs=["team-metrics does not match", "team-archive does not match",
                      "team-relay is missing", "total must equal"])

if failures:
    print("FAILURES:")
    for f in failures:
        print(" -", f)
    sys.exit(1)
print(f"all {len(tasks)} tasks validated (checks fail on seed, pass on golden fix; negatives hold)")
