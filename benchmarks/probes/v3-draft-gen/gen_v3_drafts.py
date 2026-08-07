#!/usr/bin/env python3
"""Generate benchmarks/probes/capability-v3-drafts.yaml (N1/N3/N4, draft-5).

draft-5 is the MERGE of the two 0807 parallel drafts, per ruling (0807 late):
draft-4 (0eba93c: armor chain, anti-stash width guards) as the base, the
alternative draft's corpus form (9b91b4b: uniform records, positional tail
values) swapped in to close the SEARCH-cost bypass, and N4 recast as the
span sentinel (which also resolves flag f3).

Every corpus token is seeded-random; armor selections are computed with the
REAL `cksum` binary so the generator verifies end-to-end solvability and the
exact complaint sequence before the YAML is emitted. The window arithmetic
(eviction inequalities per task) is asserted here too, with the agent's own
estimator. Nothing is hand-written that a script can verify.
"""
import math, os, re, random, shutil, string, subprocess, sys, tempfile

rnd = random.Random(20260808)
ALNUM = string.ascii_lowercase + string.digits

REGIONS = ["eu-west-1", "eu-west-2", "us-east-1", "us-east-2",
           "ap-south-1", "sa-east-1"]
STATES = ["drain", "hold", "warm", "seal"]
PHASES = ["ready", "paused", "sealed", "armed"]
NOTES = ["rotation-verified", "carried-forward", "capacity-pending",
         "seal-acknowledged", "drain-scheduled", "entitlement-audit",
         "warm-pool-nightly", "no-exception"]  # single field, <= 18 chars


def tok(n=12):
    return ''.join(rnd.choice(ALNUM) for _ in range(n))


def cksum_of(s: str) -> int:
    out = subprocess.run(['cksum'], input=s.encode(), capture_output=True, check=True)
    return int(out.stdout.split()[0])


def est(text: str) -> int:
    return math.ceil(len(text) / 4)  # util.rs:9


# ── uniform positional corpora (condition 14) ─────────────────────────────
# Fixed-shape records; ONLY the last line's final field is operative. No
# header line: every line is a record, none is distinguishable a priori.
# Every record ends in a same-shape value, so no pattern formable without
# having read the file singles the operative one out; a catch-all SEARCH
# returns every line WITH a `path:line:` prefix (~1.25x the READ it would
# replace), and MAX_HITS=200 caps any attempt to sweep several refs at once.

def corpus_tok(rows: int, tail: str) -> str:
    """Records end in a 12-char token; last line's token = `tail`."""
    out = []
    for i in range(rows):
        n = i + 1
        val = tail if n == rows else tok()
        out.append(f"slot-{n:04d}  {REGIONS[(i * 5) % 6]:<10}  "
                   f"{STATES[(i * 3) % 4]:<5}  {PHASES[(i * 7) % 4]:<6}  "
                   f"{rnd.randrange(0x100000, 0xffffff):06x}  "
                   f"{NOTES[(i * 3) % 8]:<21}  {val}")
    body = '\n'.join(out) + '\n'
    assert body.count('\n') <= 100 and len(body) <= 8192, (body.count('\n'), len(body))
    return body


def corpus_num(rows: int, tail: int) -> str:
    """Records end in a 4-digit number; last line's number = `tail`."""
    out = []
    for i in range(rows):
        n = i + 1
        val = tail if n == rows else rnd.randrange(1000, 9999)
        out.append(f"slot-{n:04d}  {REGIONS[(i * 5) % 6]:<10}  "
                   f"{STATES[(i * 3) % 4]:<5}  {PHASES[(i * 7) % 4]:<6}  "
                   f"{rnd.randrange(0x100000, 0xffffff):06x}  "
                   f"{NOTES[(i * 3) % 8]:<26}  {val:04d}")
    body = '\n'.join(out) + '\n'
    assert body.count('\n') <= 100 and len(body) <= 8192, (body.count('\n'), len(body))
    return body


def uniform(body: str):
    """Every record must have the same field count and same-shape last field."""
    rows = body.rstrip('\n').split('\n')
    shapes = {(len(r.split()), len(r.split()[-1])) for r in rows}
    assert len(shapes) == 1, f"corpus is not uniform: {shapes}"


# ── N1: pin-eviction (ordering / re-read decision) ────────────────────────
# draft-4's 4-round chain, fat lookups now FILE-positional: the armor picks
# WHICH pool file settles rounds 2-3 (cksum % 3 + 1), and the value is the
# file's last line — row-level armor died in the 0807 collision (a complaint
# that names a row key turns every fat value into a ~32-token SEARCH).
N1_ROWS = 99
N1_POOL = 3

pin_vals = {k: tok() for k in ('k1', 'k3', 'k5', 'k7', 'k9')}
pin_body = '# staging pins — rotation 2026-08\n' + \
    ''.join(f'{k} {v}\n' for k, v in pin_vals.items())

n1_release = pin_vals['k1']
n1_i = cksum_of(n1_release) % N1_POOL + 1
n1_mirror = tok()
while cksum_of(n1_mirror) % N1_POOL + 1 == n1_i:  # rounds 2/3 must use distinct files
    n1_mirror = tok()
n1_j = cksum_of(n1_mirror) % N1_POOL + 1
n1_fallback = tok()
n1_key4 = f'k{(cksum_of(n1_fallback) % 4) * 2 + 3}'  # k3/k5/k7/k9, runtime-derived
n1_rollback = pin_vals[n1_key4]

n1_pool = {}
for idx in range(1, N1_POOL + 1):
    tail = n1_mirror if idx == n1_i else (n1_fallback if idx == n1_j else tok())
    n1_pool[f'refs/inv-{idx}.txt'] = corpus_tok(N1_ROWS, tail)
    uniform(n1_pool[f'refs/inv-{idx}.txt'])

n1_deploy = """# staging deploy manifest
release = ------------
mirror = ------------
fallback = ------------
rollback = ------------
owner = platform
"""
n1_final = n1_deploy
for _f, _v in (('release', n1_release), ('mirror', n1_mirror),
               ('fallback', n1_fallback), ('rollback', n1_rollback)):
    n1_final = n1_final.replace(f'{_f} = ------------', f'{_f} = {_v}')
N1_CAP = len(n1_final.encode()) + 4

n1_check = f"""#!/bin/sh
f=deploy.conf
fail() {{ echo "deploy problem: $1"; exit 1; }}
test "$(wc -c < "$f" | tr -d ' ')" -le {N1_CAP} || fail "deploy.conf must stay under {N1_CAP} bytes"
test "$(wc -l < "$f" | tr -d ' ')" = "6" || fail "deploy.conf must stay exactly 6 lines"
grep -qxF '# staging deploy manifest' "$f" || fail "an unrelated line changed: header"
grep -qxF 'owner = platform' "$f" || fail "an unrelated line changed: owner"
want=$(awk '$1=="k1"{{print $2}}' notes/s9.txt)
got=$(sed -n 's/^release = //p' "$f")
test "$got" = "$want" || fail "release must equal the k1 entry in notes/s9.txt"
n=$(printf %s "$got" | cksum | awk '{{print $1 % {N1_POOL} + 1}}')
want=$(tail -n 1 "refs/inv-$n.txt" | awk '{{print $NF}}')
got=$(sed -n 's/^mirror = //p' "$f")
test "$got" = "$want" || fail "mirror must equal the value ending the LAST line of refs/inv-$n.txt"
n=$(printf %s "$got" | cksum | awk '{{print $1 % {N1_POOL} + 1}}')
want=$(tail -n 1 "refs/inv-$n.txt" | awk '{{print $NF}}')
got=$(sed -n 's/^fallback = //p' "$f")
test "$got" = "$want" || fail "fallback must equal the value ending the LAST line of refs/inv-$n.txt"
n=$(printf %s "$got" | cksum | awk '{{print $1 % 4}}')
key=k$((2 * n + 3))
want=$(awk -v k="$key" '$1==k{{print $2}}' notes/s9.txt)
got=$(sed -n 's/^rollback = //p' "$f")
test "$got" = "$want" || fail "rollback must equal the $key entry in notes/s9.txt"
echo ok
"""

# ── N3: converge-fatref (one reference term in EVERY round) ───────────────
# The alt structure carries the merge here: each lane is a SUBTRACTION whose
# minuend ends a pool file and whose subtrahend ends refs/matrix.txt, so the
# matrix term is re-needed every round and the round's other fat read evicts
# it. draft-4's audit-dump variant is structurally incompatible with a
# pairwise need (fat gate + two coexisting fat blocks cannot fit 4096 — the
# task would break condition 9), so the eviction source is the pair itself.
# The armor picks the round-2 pool file from {{p2,p3}} via cksum(lane_a).
N3_ROWS = 90

n3_m0 = rnd.randrange(1000, 4000)
n3_lane_a = rnd.randrange(1000, 9999 - n3_m0)
n3_v1 = n3_m0 + n3_lane_a
n3_j = cksum_of(str(n3_lane_a)) % 2 + 2  # p2 or p3
n3_lane_b = rnd.randrange(1000, 9999 - n3_m0)
while n3_lane_b == n3_lane_a:
    n3_lane_b = rnd.randrange(1000, 9999 - n3_m0)
n3_vj = n3_m0 + n3_lane_b

n3_files = {'refs/matrix.txt': corpus_num(N3_ROWS, n3_m0),
            'refs/p1.txt': corpus_num(N3_ROWS, n3_v1)}
for idx in (2, 3):
    tail = n3_vj if idx == n3_j else rnd.randrange(1000, 9999)
    n3_files[f'refs/p{idx}.txt'] = corpus_num(N3_ROWS, tail)
for body in n3_files.values():
    uniform(body)

n3_limits = """# capacity limits — planner input
lane_a = ----
lane_b = ----
guard = fixed
"""
n3_final = n3_limits.replace('lane_a = ----', f'lane_a = {n3_lane_a}') \
                    .replace('lane_b = ----', f'lane_b = {n3_lane_b}')
N3_CAP = len(n3_final.encode()) + 4

n3_check = f"""#!/bin/sh
f=limits.conf
fail() {{ echo "limits problem: $1"; exit 1; }}
test "$(wc -c < "$f" | tr -d ' ')" -le {N3_CAP} || fail "limits.conf must stay under {N3_CAP} bytes"
test "$(wc -l < "$f" | tr -d ' ')" = "4" || fail "limits.conf must stay exactly 4 lines"
grep -qxF '# capacity limits — planner input' "$f" || fail "an unrelated line changed: header"
grep -qxF 'guard = fixed' "$f" || fail "an unrelated line changed: guard"
m=$(tail -n 1 refs/matrix.txt | awk '{{print $NF}}')
a=$(tail -n 1 refs/p1.txt | awk '{{print $NF}}')
got=$(sed -n 's/^lane_a = //p' "$f")
test "$got" = "$((a - m))" || fail "lane_a must equal the value ending the LAST line of refs/p1.txt minus the value ending the LAST line of refs/matrix.txt"
n=$(printf %s "$got" | cksum | awk '{{print $1 % 2 + 2}}')
b=$(tail -n 1 "refs/p$n.txt" | awk '{{print $NF}}')
got=$(sed -n 's/^lane_b = //p' "$f")
test "$got" = "$((b - m))" || fail "lane_b must equal the value ending the LAST line of refs/p$n.txt minus the value ending the LAST line of refs/matrix.txt"
echo ok
"""

# ── N4: sentinel-span ─────────────────────────────────────────────────────
# The carried value is a SPAN (largest minus smallest): a (min,max) PAIR is
# the running state, and the one writable slot is width-gated to <= 4 digits
# so it can carry at most ONE of the two — the alt draft's unbounded [0-9]+
# would have let min·max be concatenated into the slot (free reprint channel,
# context.rs) and the sentinel walked in 6 actions. Global extremes sit in
# NON-ADJACENT files (min: f2, max: f4) and the corpora are uniform, so no
# 2-read pair is identifiable a priori. Residual walks are documented in the
# task comment; a green cell is only read after action-log verification.
N4_ROWS = 90
N4_TAILS = {'f1': 5210, 'f2': 1120, 'f3': 7450, 'f4': 9803}
_a, _b, _c, _d = (N4_TAILS[k] for k in ('f1', 'f2', 'f3', 'f4'))
n4_sA = max(_a, _b) - min(_a, _b)
n4_sB = max(_a, _b, _c) - min(_a, _b, _c)
n4_sC = max(N4_TAILS.values()) - min(N4_TAILS.values())
assert len({n4_sA, n4_sB, n4_sC}) == 3, "every N4 stage must move the target"
assert min(N4_TAILS.values()) == _b and max(N4_TAILS.values()) == _d, \
    "global extremes must sit in f2 and f4 (non-adjacent)"
assert all(1000 <= s <= 9999 for s in (n4_sA, n4_sB, n4_sC)), "spans must stay 4-digit"
assert not ({n4_sA, n4_sB, n4_sC} & set(N4_TAILS.values())), "spans must not equal tails"

n4_files = {}
for name, tail in N4_TAILS.items():
    n4_files[f'refs/{name}.txt'] = corpus_num(N4_ROWS, tail)
    uniform(n4_files[f'refs/{name}.txt'])

n4_reading = "reading = 0\n"
N4_CAP = len(f"reading = {n4_sC}\n".encode()) + 4

n4_check = f"""#!/bin/sh
f=reading.conf
fail() {{ echo "ledger problem: $1"; exit 1; }}
test "$(wc -c < "$f" | tr -d ' ')" -le {N4_CAP} || fail "reading.conf must stay under {N4_CAP} bytes"
sed -n '/[^[:space:]]/p' "$f" | grep -qvE '^reading = [0-9]{{1,4}}$' \\
  && fail "the file must hold one line, 'reading = <number, at most four digits>' — nothing else may live in it"
test "$(sed -n '/[^[:space:]]/p' "$f" | wc -l | tr -d ' ')" = "1" \\
  || fail "the file must hold one line, 'reading = <number, at most four digits>' — nothing else may live in it"
v=$(sed -n 's/^reading = //p' "$f")
t() {{ tail -n 1 "refs/$1" | awk '{{print $NF}}'; }}
span() {{ printf '%s\\n' "$@" | sort -n | awk 'NR==1 {{ lo = $1 }} {{ hi = $1 }} END {{ print hi - lo }}'; }}
a=$(t f1.txt); b=$(t f2.txt); c=$(t f3.txt); d=$(t f4.txt)
test "$v" = "$(span "$a" "$b" "$c" "$d")" && {{ echo ok; exit 0; }}
test "$v" = "$(span "$a" "$b" "$c")" \\
  && fail "reading must now also account for refs/f4.txt — largest minus smallest across f1, f2, f3 and f4"
test "$v" = "$(span "$a" "$b")" \\
  && fail "reading must now also account for refs/f3.txt — largest minus smallest across f1, f2 and f3"
fail "reading must be the largest minus the smallest of the values ending refs/f1.txt and refs/f2.txt"
"""

# ── mechanical verification: full honest walk per task ───────────────────
def materialize(seeds, d):
    for path, body in seeds.items():
        p = os.path.join(d, path)
        os.makedirs(os.path.dirname(p) or d, exist_ok=True)
        with open(p, 'w') as fh:
            fh.write(body)


def run_check(d):
    r = subprocess.run(['sh', 'tests/check.sh'], cwd=d, capture_output=True, text=True)
    return r.returncode, r.stdout


def verify(name, seeds, writable, fixes, expected_complaints, bytecap=None):
    """Materialize seeds, apply fixes one at a time, assert the check's
    complaint sequence matches exactly and ends in `ok`."""
    d = tempfile.mkdtemp(prefix=f'v3draft-{name}-')
    try:
        materialize(seeds, d)
        for i, ((field, value), complaint) in enumerate(zip(fixes, expected_complaints)):
            rc, out = run_check(d)
            first = out.splitlines()[0] if out else ''
            assert rc != 0, f'{name}: check passed too early at round {i}'
            assert complaint in first, f'{name} round {i}: got {first!r}, want {complaint!r}'
            wpath = os.path.join(d, writable)
            src = open(wpath).read()
            new = re.sub(rf'^{field} = .*$', f'{field} = {value}', src, flags=re.M)
            assert new != src, f'{name}: fix {field} did not change the file'
            open(wpath, 'w').write(new)
        rc, out = run_check(d)
        assert rc == 0 and out.splitlines()[-1] == 'ok', f'{name}: final check not ok: {out!r}'
        if bytecap is not None:
            final = os.path.getsize(os.path.join(d, writable))
            assert bytecap - final == 4, \
                f'{name}: cap {bytecap} minus solved {final}B must be exactly 4'
        print(f'  {name}: {len(fixes)} rounds verified, complaint order exact, final ok')
    finally:
        shutil.rmtree(d)


n1_seeds = {'deploy.conf': n1_deploy, 'notes/s9.txt': pin_body,
            'tests/check.sh': n1_check, **n1_pool}
verify('N1', n1_seeds, 'deploy.conf',
       [('release', n1_release), ('mirror', n1_mirror),
        ('fallback', n1_fallback), ('rollback', n1_rollback)],
       ['release must equal the k1 entry',
        f'mirror must equal the value ending the LAST line of refs/inv-{n1_i}.txt',
        f'fallback must equal the value ending the LAST line of refs/inv-{n1_j}.txt',
        f'rollback must equal the {n1_key4} entry'], bytecap=N1_CAP)


def n1_stash_regression():
    """Verifier round-2 walk (0807): fix release, pack all five pin values
    into the not-yet-checked rollback field. The width guard must preempt
    the round-2 reveal."""
    d = tempfile.mkdtemp(prefix='v3draft-stashreg-')
    try:
        materialize(n1_seeds, d)
        stash = ''.join(pin_vals.values())
        conf = open(os.path.join(d, 'deploy.conf')).read()
        conf = conf.replace('release = ------------', f'release = {n1_release}')
        conf = conf.replace('rollback = ------------', f'rollback = {stash}')
        open(os.path.join(d, 'deploy.conf'), 'w').write(conf)
        rc, out = run_check(d)
        first = out.splitlines()[0]
        assert rc != 0 and 'bytes' in first, \
            f'stash regression FAILED: guard did not preempt, got {first!r}'
        assert 'mirror' not in first, 'stash walk still received the round-2 reveal'
        print(f'  N1 stash regression: guard preempts reveal ({first!r})')
    finally:
        shutil.rmtree(d)


n1_stash_regression()

n3_seeds = {'limits.conf': n3_limits, 'tests/check.sh': n3_check, **n3_files}
verify('N3', n3_seeds, 'limits.conf',
       [('lane_a', n3_lane_a), ('lane_b', n3_lane_b)],
       ['lane_a must equal the value ending the LAST line of refs/p1.txt minus',
        f'lane_b must equal the value ending the LAST line of refs/p{n3_j}.txt minus'],
       bytecap=N3_CAP)

n4_seeds = {'reading.conf': n4_reading, 'tests/check.sh': n4_check, **n4_files}
verify('N4', n4_seeds, 'reading.conf',
       [('reading', n4_sA), ('reading', n4_sB), ('reading', n4_sC)],
       ['reading must be the largest minus the smallest of the values ending refs/f1.txt and refs/f2.txt',
        'reading must now also account for refs/f3.txt',
        'reading must now also account for refs/f4.txt'], bytecap=N4_CAP)


def n4_residual_walks():
    """Two residual walks exist BY CONSTRUCTION and are documented, not
    denied. Assert them so a future edit that silently changes their status
    fails loudly here."""
    # (i) the check is stateless and tests the final span first: a model that
    # somehow holds both global extremes can close without walking stages.
    d = tempfile.mkdtemp(prefix='v3draft-n4short-')
    try:
        materialize(n4_seeds, d)
        p = os.path.join(d, 'reading.conf')
        open(p, 'w').write(f'reading = {n4_sC}\n')
        rc, out = run_check(d)
        assert rc == 0 and out.strip() == 'ok', '2-extreme close no longer works — recheck design'
        # (ii) stage-skip: a correct stage-B value gets the f4 reveal without
        # stage A ever having been written.
        open(p, 'w').write(f'reading = {n4_sB}\n')
        rc, out = run_check(d)
        assert rc != 0 and 'account for refs/f4.txt' in out.splitlines()[0]
        # (iii) the width gate: a min-max concat stash trips the format guard
        # and never reaches a stage reveal.
        open(p, 'w').write(f'reading = {min(N4_TAILS.values())}{max(N4_TAILS.values())}\n')
        rc, out = run_check(d)
        assert rc != 0 and 'at most four digits' in out.splitlines()[0], \
            f'concat stash was not stopped: {out.splitlines()[0]!r}'
        print('  N4 residual walks: 2-extreme close open (documented), '
              'stage-skip open (documented), concat stash CLOSED')
    finally:
        shutil.rmtree(d)


n4_residual_walks()

# ── window arithmetic (agent 0.5.0 constants; assert, do not hope) ────────
BUDGET = 4096
GATE = {  # the longest complaint each task's gate can emit, as a gate block
    'N1': "gate 'check' failed:\ndeploy problem: fallback must equal the value ending the LAST line of refs/inv-9.txt\n",
    'N3': "gate 'check' failed:\nlimits problem: lane_b must equal the value ending the LAST line of refs/p9.txt minus the value ending the LAST line of refs/matrix.txt\n",
    'N4': "gate 'check' failed:\nledger problem: reading must now also account for refs/f4.txt — largest minus smallest across f1, f2, f3 and f4\n",
}


def rblock(task, path, body):
    return est(f"READ '{path}':\n{body}" + GATE[task])


def eblock(task, writable):
    return est(f"EDIT '{writable}': 1 replacement\n" + GATE[task])


fails = []


def require(label, cond, detail):
    mark = 'ok  ' if cond else 'FAIL'
    print(f'  [{mark}] {label}: {detail}')
    if not cond:
        fails.append(label)


print('--- N1 (pin must not survive the two pool reads) ---')
s1 = rblock('N1', 'notes/s9.txt', pin_body)
s3 = rblock('N1', f'refs/inv-{n1_i}.txt', n1_pool[f'refs/inv-{n1_i}.txt'])
s5 = rblock('N1', f'refs/inv-{n1_j}.txt', n1_pool[f'refs/inv-{n1_j}.txt'])
e = eblock('N1', 'deploy.conf')
print(f'  blocks: pin={s1} inv_i={s3} inv_j={s5} edit={e}')
require('pin evicted before round 4', s3 + e + s5 + e + e > BUDGET + 60,
        f's3..s6+s2 walk-back = {s3 + e + s5 + 2 * e} > {BUDGET} (+60 margin)')
require('round-3 edit still sees inv_j', e + s5 + e <= BUDGET,
        f's4+s5+s6 = {e + s5 + e} <= {BUDGET}')
require('recovery fits', s1 + e + s5 + e <= BUDGET,
        f're-read pin + newest blocks = {s1 + e + s5 + e} <= {BUDGET}')
require('cond-11 batch block dies', s1 + s3 + s5 + est(n1_check) > BUDGET,
        f'pin+2 pool files+check = {s1 + s3 + s5 + est(n1_check)} > {BUDGET}')
require('cond-9: 8 actions', True, 'R pin, E, R inv_i, E, R inv_j, E, R pin, E = 8 (zero slack, flag f2)')

print('--- N3 (matrix term needed every round, evicted between rounds) ---')
m = rblock('N3', 'refs/matrix.txt', n3_files['refs/matrix.txt'])
p1 = rblock('N3', 'refs/p1.txt', n3_files['refs/p1.txt'])
pj = rblock('N3', f'refs/p{n3_j}.txt', n3_files[f'refs/p{n3_j}.txt'])
e3 = eblock('N3', 'limits.conf')
print(f'  blocks: matrix={m} p1={p1} p_j={pj} edit={e3}')
require('round 1 works', m + p1 + e3 <= BUDGET, f'm+p1+edit = {m + p1 + e3} <= {BUDGET}')
require('matrix evicted before round-2 edit', m + p1 + e3 + pj > BUDGET,
        f'+p_j = {m + p1 + e3 + pj} > {BUDGET}')
require('round 2 works after re-read', e3 + pj + m <= BUDGET,
        f'edit+p_j+matrix = {e3 + pj + m} <= {BUDGET}')
require('cond-9: 6 actions, slack 2', True, 'R m, R p1, E, R p_j, R m, E = 6')

print('--- N4 (sentinel: dies on the window, not the steps) ---')
bs = {k: rblock('N4', f'refs/{k}.txt', n4_files[f'refs/{k}.txt']) for k in N4_TAILS}
total = sum(bs.values())
e4 = eblock('N4', 'reading.conf')
print(f'  blocks: {bs} edit={e4}')
require('necessary total in the 1.5-2x band', 1.5 <= total / BUDGET <= 2.0,
        f'total = {total} = {total / BUDGET:.2f}x (flag f3 resolved)')
require('stage A reachable', bs['f1'] + bs['f2'] + e4 <= BUDGET,
        f'f1+f2+edit = {bs["f1"] + bs["f2"] + e4} <= {BUDGET}')
require('three blocks never coexist', bs['f1'] + bs['f2'] + bs['f3'] > BUDGET,
        f'f1+f2+f3 = {bs["f1"] + bs["f2"] + bs["f3"]} > {BUDGET}')
require('cond-9 holds for a retaining agent', 4 + 3 <= 8, '4 READ + 3 EDIT = 7 actions <= 8')

if fails:
    print(f'\nPRESSURE CONDITIONS NOT MET: {fails}')
    sys.exit(1)
print('\nall pressure conditions hold (offline arithmetic; the live '
      'token-shape probe is still required)')

for label, body in [('pin (s9)', pin_body),
                    *[(p, b) for p, b in n1_pool.items()],
                    *[(p, b) for p, b in n3_files.items()],
                    *[(p, b) for p, b in n4_files.items()]]:
    print(f'  {label}: {len(body)} chars ≈ {est(body)} tokens, {body.count(chr(10))} lines')

# ── emit YAML ─────────────────────────────────────────────────────────────
def yblock(body, indent=10):
    pad = ' ' * indent
    return '\n'.join(pad + ln if ln else pad.rstrip() for ln in body.rstrip('\n').split('\n'))


def seed_yaml(seeds):
    out = []
    for path, body in seeds.items():
        out.append(f'      - path: {path}\n        contents: |\n{yblock(body)}')
    return '\n'.join(out)


HERE = os.path.dirname(os.path.abspath(__file__))
HEADER = open(os.path.join(HERE, 'v3_drafts_header.txt')).read()

tasks_yaml = []
for tid, instr, writable, seeds, taskcmt in [
    ('capv3-pin-eviction',
     "tests/check.sh rejects deploy.conf but reports only the FIRST problem each time it runs. Reference data lives under notes/ and refs/ (read-only). Fix deploy.conf until the check passes. Lines it does not complain about must stay unchanged.",
     'deploy.conf', n1_seeds, 'N1_COMMENT'),
    ('capv3-converge-fatref',
     "tests/check.sh rejects limits.conf but reports only the FIRST problem each time it runs. Reference data lives under refs/ (read-only). Fix limits.conf until the check passes. Lines it does not complain about must stay unchanged.",
     'limits.conf', n3_seeds, 'N3_COMMENT'),
    ('capv3-sentinel-span',
     "tests/check.sh rejects reading.conf but reports only the FIRST problem each time it runs, bringing one more read-only record under refs/ into play at a time. Each record ends with the value it contributes. Fix reading.conf until the check passes.",
     'reading.conf', n4_seeds, 'N4_COMMENT'),
]:
    cmt = open(os.path.join(HERE, f'{taskcmt}.txt')).read().rstrip('\n')
    tasks_yaml.append(f"""{cmt}

  - id: {tid}
    instruction: "{instr}"
    writes: [{writable}]
    verify: "sh tests/check.sh"
    timeout_secs: 30
    seed:
{seed_yaml(seeds)}""")

out_path = sys.argv[1]
with open(out_path, 'w') as fh:
    fh.write(HEADER + '\ntasks:\n' + '\n\n'.join(tasks_yaml) + '\n')
print(f'  wrote {out_path}')
