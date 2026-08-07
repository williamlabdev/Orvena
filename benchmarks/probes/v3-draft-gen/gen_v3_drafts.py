#!/usr/bin/env python3
"""Generate benchmarks/probes/capability-v3-drafts.yaml (N1/N3/N4, draft-6).

Lineage: draft-4 (0eba93c, armor chain + guard ordering) x alt (9b91b4b,
uniform/positional corpora, span sentinel, growing-key writable) merged per
the 0807 ruling into draft-5; two zero-context B2 rounds then refuted, in
order: the slot-index anchor (round 1 -> de-anchored corpora), and in round
2 the FIXED-WIDTH SKELETON itself plus N4's unarmored file set:

* fixed answer-width placeholder slots are a free stash: every not-yet-
  checked field is a legal-width carry cell printed back each step, so N3's
  matrix term (4 digits into lane_b) and N1's whole 3-tail candidate set
  (3 toks into 3 free slots) ride the reprint and the fat go-backs the
  tasks exist to measure never happen. draft-6 switches every writable to
  the alt draft's GROWING-KEY construction (condition 13's original form):
  a key line may not exist before the check has asked for it, the keys
  guard runs before every reveal, and the lookahead stash shrinks to the
  single just-asked slot (a 1-in-pool guess, action-log visible).
* N3 tails are now 6-digit while lanes stay {4}-gated: the re-needed term
  no longer fits ANY writable cell, killing the carry outright.
* N4's operative file set is now ARMORED: refs/f1..f6 exist, the span runs
  over f1, f2 plus two cksum-picked files revealed stage by stage — a
  batch reader holds six tails and still cannot compute the final span.
* the window arithmetic now reconstructs gate evidence in the driver's
  REAL format ("[solved] <task-id>: <complaint>", driver.rs:481-485;
  successful EDITs emit no tool evidence) instead of an invented one.

Every corpus token is seeded-random; armor selections are computed with the
REAL `cksum` binary; honest walks, complaint order, stash/keys regressions,
window inequalities and corpus properties are all asserted before the YAML
is emitted. Nothing is hand-written that a script can verify.
"""
import math, os, re, random, shutil, string, subprocess, sys, tempfile

rnd = random.Random(20260809)
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


# ── de-anchored uniform positional corpora (condition 14 + B2 round 1) ────
# Fixed-shape records, ONLY the last line's final field operative. No index
# column (SEARCH matches content; content must not encode position), per-row
# random fields (no position-correlated cycle to learn from a sibling),
# sibling row counts pairwise distinct (a hit's path:line: number cannot be
# certified as an unread file's last line).

def corpus_tok(rows: int, tail: str) -> str:
    """Records end in a 12-char token; last line's token = `tail`."""
    out = []
    for i in range(rows):
        val = tail if i + 1 == rows else tok()
        out.append(f"{rnd.choice(REGIONS):<10}  {rnd.choice(STATES):<5}  "
                   f"{rnd.choice(PHASES):<6}  "
                   f"{rnd.randrange(0x100000, 0xffffff):06x}  "
                   f"{rnd.choice(NOTES):<24}  "
                   f"{rnd.randrange(0x100000, 0xffffff):06x}  {val}")
    body = '\n'.join(out) + '\n'
    assert body.count('\n') <= 100 and len(body) <= 8192, (body.count('\n'), len(body))
    return body


def corpus_num(rows: int, tail: int, width: int = 4) -> str:
    """Records end in a `width`-digit number; last line's number = `tail`."""
    lo, hi = 10 ** (width - 1), 10 ** width - 1
    out = []
    for i in range(rows):
        val = tail if i + 1 == rows else rnd.randrange(lo, hi)
        assert lo <= int(val) <= hi, "leading-zero / width invariant"
        out.append(f"{rnd.choice(REGIONS):<10}  {rnd.choice(STATES):<5}  "
                   f"{rnd.choice(PHASES):<6}  "
                   f"{rnd.randrange(0x100000, 0xffffff):06x}  "
                   f"{rnd.choice(NOTES):<21}  {tok()}  "
                   f"{rnd.randrange(0x100000, 0xffffff):06x}  {val:0{width}d}")
    body = '\n'.join(out) + '\n'
    assert body.count('\n') <= 100 and len(body) <= 8192, (body.count('\n'), len(body))
    return body


def uniform(body: str):
    """Every record must have the same field count and same-shape last field."""
    rows = body.rstrip('\n').split('\n')
    shapes = {(len(r.split()), len(r.split()[-1])) for r in rows}
    assert len(shapes) == 1, f"corpus is not uniform: {shapes}"


def deanchored(task, files):
    """B2 round-1 regression: no cross-file positional anchor.

    (a) pairwise-distinct row counts; (b) no zero-padded own-line-index
    field (the slot-#### anchor stays dead); (c) last-row categorical
    tuples pairwise distinct across the family; (d) a catch-all SEARCH
    renders dearer than the READ it would replace.
    """
    counts = {p: b.count('\n') for p, b in files.items()}
    assert len(set(counts.values())) == len(counts), f"{task}: row counts collide: {counts}"
    for p, b in files.items():
        rows = b.rstrip('\n').split('\n')
        for ln, r in enumerate(rows, 1):
            assert f'{ln:04d}' not in r.split()[0], f"{task}:{p}: index anchor at line {ln}"
        render = sum(len(f'  {p}:{ln}: {r}\n') for ln, r in enumerate(rows, 1))
        assert est('x' * render) > est(b), f"{task}:{p}: catch-all cheaper than READ"
    tuples = {p: tuple(files[p].rstrip('\n').split('\n')[-1].split()[:3]) for p in files}
    assert len(set(tuples.values())) == len(tuples), \
        f"{task}: last-row tuples collide (bump seed): {tuples}"


# ── N1: pin-eviction (ordering / re-read decision) ────────────────────────
N1_POOL = 3
N1_POOL_ROWS = {1: 99, 2: 98, 3: 97}  # pairwise distinct (deanchored)

pin_vals = {k: tok() for k in ('k1', 'k3', 'k5', 'k7', 'k9')}
pin_body = '# staging pins — rotation 2026-08\n' + \
    ''.join(f'{k} {v}\n' for k, v in pin_vals.items())

n1_release = pin_vals['k1']
n1_i = cksum_of(n1_release) % N1_POOL + 1
n1_mirror = tok()
while cksum_of(n1_mirror) % N1_POOL + 1 == n1_i:  # rounds 2/3 use distinct files
    n1_mirror = tok()
n1_j = cksum_of(n1_mirror) % N1_POOL + 1
n1_fallback = tok()
n1_key4 = f'k{(cksum_of(n1_fallback) % 4) * 2 + 3}'  # k3/k5/k7/k9, runtime-derived
n1_rollback = pin_vals[n1_key4]

n1_pool = {}
for idx in range(1, N1_POOL + 1):
    tail = n1_mirror if idx == n1_i else (n1_fallback if idx == n1_j else tok())
    n1_pool[f'refs/inv-{idx}.txt'] = corpus_tok(N1_POOL_ROWS[idx], tail)
    uniform(n1_pool[f'refs/inv-{idx}.txt'])
deanchored('N1', n1_pool)

# Growing-key writable: field lines may not exist before they are asked for.
n1_deploy = """# staging deploy manifest
owner = platform
"""
n1_final = ('# staging deploy manifest\n'
            f'release = {n1_release}\n'
            f'mirror = {n1_mirror}\n'
            f'fallback = {n1_fallback}\n'
            f'rollback = {n1_rollback}\n'
            'owner = platform\n')
N1_CAP = len(n1_final.encode())  # exact — a solved+4 slack was itself carry room

n1_check = f"""#!/bin/sh
f=deploy.conf
fail() {{ echo "deploy problem: $1"; exit 1; }}
test "$(wc -c < "$f" | tr -d ' ')" -le {N1_CAP} || fail "deploy.conf must stay within {N1_CAP} bytes"
grep -qxF '# staging deploy manifest' "$f" || fail "an unrelated line changed: header"
grep -qxF 'owner = platform' "$f" || fail "an unrelated line changed: owner"
sed -n '/^[a-z]/p' "$f" | grep -v '^owner = platform$' | grep -qvE '^(release|mirror|fallback|rollback) = [a-z0-9]{{12}}$' \\
  && fail "every field line must read '<field> = <12-character value>' — nothing else may live in this file"
get() {{ sed -n "s/^$1 = //p" "$f"; }}
keys=$(sed -n 's/^\\([a-z]*\\) = .*/\\1/p' "$f" | grep -v '^owner$' | sort | tr '\\n' ' ')
want=$(awk '$1=="k1"{{print $2}}' notes/s9.txt)
if [ "$(get release)" != "$want" ]; then
  case "$keys" in ""|"release ") ;; *) fail "the manifest may hold only the fields asked for so far — that is release, nothing else" ;; esac
  fail "release must equal the k1 entry in notes/s9.txt (that file is the source of record; add the line if it is not there)"
fi
n=$(printf %s "$(get release)" | cksum | awk '{{print $1 % {N1_POOL} + 1}}')
want=$(tail -n 1 "refs/inv-$n.txt" | awk '{{print $NF}}')
if [ "$(get mirror)" != "$want" ]; then
  case "$keys" in "release "|"mirror release ") ;; *) fail "the manifest may hold only the fields asked for so far — that is release and mirror, nothing else" ;; esac
  fail "mirror must equal the value ending the LAST line of refs/inv-$n.txt (that file is the source of record; add the line if it is not there)"
fi
n=$(printf %s "$(get mirror)" | cksum | awk '{{print $1 % {N1_POOL} + 1}}')
want=$(tail -n 1 "refs/inv-$n.txt" | awk '{{print $NF}}')
if [ "$(get fallback)" != "$want" ]; then
  case "$keys" in "mirror release "|"fallback mirror release ") ;; *) fail "the manifest may hold only the fields asked for so far — that is release, mirror and fallback, nothing else" ;; esac
  fail "fallback must equal the value ending the LAST line of refs/inv-$n.txt (that file is the source of record; add the line if it is not there)"
fi
n=$(printf %s "$(get fallback)" | cksum | awk '{{print $1 % 4}}')
key=k$((2 * n + 3))
want=$(awk -v k="$key" '$1==k{{print $2}}' notes/s9.txt)
if [ "$(get rollback)" != "$want" ]; then
  case "$keys" in "fallback mirror release "|"fallback mirror release rollback ") ;; *) fail "the manifest may hold only the fields asked for so far — that is release, mirror, fallback and rollback, nothing else" ;; esac
  fail "rollback must equal the $key entry in notes/s9.txt (that file is the source of record; add the line if it is not there)"
fi
echo ok
"""

# ── N3: converge-fatref (one reference term in EVERY round) ───────────────
# Tails are 6-digit; lanes are {4}-gated differences. The re-needed matrix
# term does not fit any writable cell (B2 round 2 killed the 4-digit carry).
N3_FILE_ROWS = {'matrix': 84, 'p1': 83, 'p2': 82, 'p3': 81}  # pairwise distinct
N3_W = 6

n3_m0 = rnd.randrange(100000, 999999 - 9999)
n3_lane_a = rnd.randrange(1000, 9999)
n3_v1 = n3_m0 + n3_lane_a
n3_j = cksum_of(str(n3_lane_a)) % 2 + 2  # p2 or p3
n3_lane_b = rnd.randrange(1000, 9999)
while n3_lane_b == n3_lane_a:
    n3_lane_b = rnd.randrange(1000, 9999)
n3_vj = n3_m0 + n3_lane_b

n3_files = {'refs/matrix.txt': corpus_num(N3_FILE_ROWS['matrix'], n3_m0, N3_W),
            'refs/p1.txt': corpus_num(N3_FILE_ROWS['p1'], n3_v1, N3_W)}
for idx in (2, 3):
    tail = n3_vj if idx == n3_j else rnd.randrange(100000, 999999)
    n3_files[f'refs/p{idx}.txt'] = corpus_num(N3_FILE_ROWS[f'p{idx}'], tail, N3_W)
for body in n3_files.values():
    uniform(body)
deanchored('N3', n3_files)
assert all(100000 <= v <= 999999 for v in (n3_m0, n3_v1, n3_vj)), "6-digit invariant"
assert all(1000 <= v <= 9999 for v in (n3_lane_a, n3_lane_b)), "4-digit lanes"

n3_limits = """# capacity limits — planner input
guard = fixed
"""
n3_final = ('# capacity limits — planner input\n'
            f'lane_a = {n3_lane_a}\n'
            f'lane_b = {n3_lane_b}\n'
            'guard = fixed\n')
N3_CAP = len(n3_final.encode())

n3_check = f"""#!/bin/sh
f=limits.conf
fail() {{ echo "limits problem: $1"; exit 1; }}
test "$(wc -c < "$f" | tr -d ' ')" -le {N3_CAP} || fail "limits.conf must stay within {N3_CAP} bytes"
grep -qxF '# capacity limits — planner input' "$f" || fail "an unrelated line changed: header"
grep -qxF 'guard = fixed' "$f" || fail "an unrelated line changed: guard"
sed -n '/^[a-z]/p' "$f" | grep -v '^guard = fixed$' | grep -qvE '^lane_[ab] = [0-9]{{4}}$' \\
  && fail "every lane line must read 'lane_<x> = <4-digit number>' — nothing else may live in this file"
get() {{ sed -n "s/^$1 = //p" "$f"; }}
keys=$(sed -n 's/^\\(lane_[a-z]*\\) = .*/\\1/p' "$f" | sort | tr '\\n' ' ')
m=$(tail -n 1 refs/matrix.txt | awk '{{print $NF}}')
a=$(tail -n 1 refs/p1.txt | awk '{{print $NF}}')
if [ "$(get lane_a)" != "$((a - m))" ]; then
  case "$keys" in ""|"lane_a ") ;; *) fail "limits may hold only the lanes asked for so far — that is lane_a, nothing else" ;; esac
  fail "lane_a must equal the value ending the LAST line of refs/p1.txt minus the value ending the LAST line of refs/matrix.txt (add the line if it is not there)"
fi
n=$(printf %s "$(get lane_a)" | cksum | awk '{{print $1 % 2 + 2}}')
b=$(tail -n 1 "refs/p$n.txt" | awk '{{print $NF}}')
if [ "$(get lane_b)" != "$((b - m))" ]; then
  case "$keys" in "lane_a "|"lane_a lane_b ") ;; *) fail "limits may hold only the lanes asked for so far — that is lane_a and lane_b, nothing else" ;; esac
  fail "lane_b must equal the value ending the LAST line of refs/p$n.txt minus the value ending the LAST line of refs/matrix.txt (add the line if it is not there)"
fi
echo ok
"""

# ── N4: sentinel-span with an ARMORED file set ────────────────────────────
# refs/f1..f6 exist; the span runs over f1, f2 and TWO cksum-picked files
# revealed stage by stage. A batch reader holds all six tails and still
# cannot compute the final span (B2 round 2 killed the unarmored set: read
# four named files at s1, write the global span at s2, done in two steps).
N4_FILE_ROWS = {'f1': 84, 'f2': 83, 'f3': 82, 'f4': 81, 'f5': 80, 'f6': 79}

n4_t1, n4_t2 = 5210, 1120                      # f2 carries the global min
n4_sA = abs(n4_t1 - n4_t2)
n4_nB = cksum_of(str(n4_sA)) % 4 + 3           # stage-B file index in f3..f6
n4_tB = 7450                                   # interim max
n4_sB = max(n4_t1, n4_t2, n4_tB) - min(n4_t1, n4_t2, n4_tB)
n4_nC = cksum_of(str(n4_sB)) % 4 + 3
while n4_nC == n4_nB:                          # stage-C file must differ
    n4_tB += 1
    n4_sB = max(n4_t1, n4_t2, n4_tB) - min(n4_t1, n4_t2, n4_tB)
    n4_nC = cksum_of(str(n4_sB)) % 4 + 3
n4_tC = 9803                                   # global max
n4_sC = max(n4_t1, n4_t2, n4_tB, n4_tC) - min(n4_t1, n4_t2, n4_tB, n4_tC)

n4_tails = {'f1': n4_t1, 'f2': n4_t2, f'f{n4_nB}': n4_tB, f'f{n4_nC}': n4_tC}
for name in N4_FILE_ROWS:
    if name not in n4_tails:
        # Decoys must MOVE the stage-B span too — an interior decoy's
        # candidate span collapses onto sA and shrinks the guess space.
        cand = rnd.randrange(5500, 9700)
        while (max(n4_t1, n4_t2, cand) - min(n4_t1, n4_t2, cand)
               in {n4_sA, n4_sB, n4_sC}) or cand in (n4_t1, n4_t2, n4_tB, n4_tC):
            cand = rnd.randrange(5500, 9700)
        n4_tails[name] = cand
assert len({n4_sA, n4_sB, n4_sC}) == 3, "every N4 stage must move the target"
assert all(1000 <= s <= 9999 for s in (n4_sA, n4_sB, n4_sC)), "spans must stay 4-digit"
assert all(1000 <= t <= 9999 for t in n4_tails.values()), "tails must stay 4-digit"
assert not ({n4_sA, n4_sB, n4_sC} & set(n4_tails.values())), "spans must not equal tails"
# guessability floor: the four candidate stage-B spans must be distinct
_sB_cands = {max(n4_t1, n4_t2, n4_tails[f'f{k}']) - min(n4_t1, n4_t2, n4_tails[f'f{k}'])
             for k in range(3, 7)}
assert len(_sB_cands) == 4, "stage-B candidates must be pairwise distinct"

n4_files = {}
for name, rows in N4_FILE_ROWS.items():
    n4_files[f'refs/{name}.txt'] = corpus_num(rows, n4_tails[name])
    uniform(n4_files[f'refs/{name}.txt'])
deanchored('N4', n4_files)

n4_reading = "reading = 0\n"
N4_CAP = len(f"reading = {n4_sC}\n".encode())

n4_check = f"""#!/bin/sh
f=reading.conf
fail() {{ echo "ledger problem: $1"; exit 1; }}
test "$(wc -c < "$f" | tr -d ' ')" -le {N4_CAP} || fail "reading.conf must stay within {N4_CAP} bytes"
sed -n '/[^[:space:]]/p' "$f" | grep -qvE '^reading = [0-9]{{1,4}}$' \\
  && fail "the file must hold one line, 'reading = <number, at most four digits>' — nothing else may live in it"
test "$(sed -n '/[^[:space:]]/p' "$f" | wc -l | tr -d ' ')" = "1" \\
  || fail "the file must hold one line, 'reading = <number, at most four digits>' — nothing else may live in it"
v=$(sed -n 's/^reading = //p' "$f")
t() {{ tail -n 1 "refs/$1" | awk '{{print $NF}}'; }}
span() {{ printf '%s\\n' "$@" | sort -n | awk 'NR==1 {{ lo = $1 }} {{ hi = $1 }} END {{ print hi - lo }}'; }}
a=$(t f1.txt); b=$(t f2.txt)
sA=$(span "$a" "$b")
nB=$(printf %s "$sA" | cksum | awk '{{print $1 % 4 + 3}}')
c=$(t "f$nB.txt")
sB=$(span "$a" "$b" "$c")
nC=$(printf %s "$sB" | cksum | awk '{{print $1 % 4 + 3}}')
d=$(t "f$nC.txt")
test "$v" = "$(span "$a" "$b" "$c" "$d")" && {{ echo ok; exit 0; }}
test "$v" = "$sB" \\
  && fail "reading must now also account for refs/f$nC.txt — largest minus smallest across f1, f2, f$nB and f$nC"
test "$v" = "$sA" \\
  && fail "reading must now also account for refs/f$nB.txt — largest minus smallest across f1, f2 and f$nB"
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


def set_field(d, writable, field, value, anchor):
    """Honest edit: set `field` if present, else insert it before `anchor`
    (growing-key files start without their field lines)."""
    wpath = os.path.join(d, writable)
    src = open(wpath).read()
    if re.search(rf'^{field} = ', src, flags=re.M):
        new = re.sub(rf'^{field} = .*$', f'{field} = {value}', src, flags=re.M)
    elif anchor:
        new = src.replace(anchor, f'{field} = {value}\n{anchor}')
    else:
        new = src + f'{field} = {value}\n'
    assert new != src, f'fix {field} did not change the file'
    open(wpath, 'w').write(new)


def verify(name, seeds, writable, fixes, expected_complaints, anchor, bytecap):
    """Materialize seeds, apply fixes one at a time, assert the check's
    complaint sequence matches exactly and ends in `ok` at the exact cap."""
    d = tempfile.mkdtemp(prefix=f'v3draft-{name}-')
    try:
        materialize(seeds, d)
        for i, ((field, value), complaint) in enumerate(zip(fixes, expected_complaints)):
            rc, out = run_check(d)
            first = out.splitlines()[0] if out else ''
            assert rc != 0, f'{name}: check passed too early at round {i}'
            assert complaint in first, f'{name} round {i}: got {first!r}, want {complaint!r}'
            set_field(d, writable, field, value, anchor)
        rc, out = run_check(d)
        assert rc == 0 and out.splitlines()[-1] == 'ok', f'{name}: final check not ok: {out!r}'
        final = os.path.getsize(os.path.join(d, writable))
        assert bytecap == final, f'{name}: cap {bytecap} must equal solved size {final}'
        print(f'  {name}: {len(fixes)} rounds verified, complaint order exact, final ok at exact cap')
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
        f'rollback must equal the {n1_key4} entry'],
       anchor='owner = platform', bytecap=N1_CAP)


def n1_keys_regressions():
    """B2 round-2 walk: the fixed skeleton let a batch reader stash the
    whole 3-tail candidate set in free slots. The growing-key guard must
    (a) preempt any reveal when more than the just-asked key is added, and
    (b) leave exactly the single-slot lookahead open (documented residue),
    and (c) preempt the old 1-in-4 pin pre-fill outright."""
    d = tempfile.mkdtemp(prefix='v3draft-n1keys-')
    try:
        materialize(n1_seeds, d)
        set_field(d, 'deploy.conf', 'release', n1_release, 'owner = platform')
        set_field(d, 'deploy.conf', 'mirror', tok(), 'owner = platform')
        set_field(d, 'deploy.conf', 'fallback', tok(), 'owner = platform')
        rc, out = run_check(d)
        first = out.splitlines()[0]
        assert rc != 0 and 'only the fields asked for so far' in first, \
            f'keys guard did not preempt: {first!r}'
        assert 'inv-' not in first, 'multi-slot stash still received a reveal'
        # single-slot lookahead: release fixed + a wrong mirror guess DOES
        # get the mirror reveal — the 1-in-3 residue, action-log visible
        open(os.path.join(d, 'deploy.conf'), 'w').write(n1_deploy)
        set_field(d, 'deploy.conf', 'release', n1_release, 'owner = platform')
        set_field(d, 'deploy.conf', 'mirror', tok(), 'owner = platform')
        rc, out = run_check(d)
        first = out.splitlines()[0]
        assert rc != 0 and f'refs/inv-{n1_i}.txt' in first, \
            f'single-slot lookahead unexpectedly blocked: {first!r}'
        # early rollback line (the old 1-in-4 pin pre-fill) trips the guard
        open(os.path.join(d, 'deploy.conf'), 'w').write(n1_deploy)
        set_field(d, 'deploy.conf', 'release', n1_release, 'owner = platform')
        set_field(d, 'deploy.conf', 'rollback', pin_vals['k5'], 'owner = platform')
        rc, out = run_check(d)
        first = out.splitlines()[0]
        assert rc != 0 and 'only the fields asked for so far' in first and 'inv-' not in first, \
            f'pin pre-fill was not preempted: {first!r}'
        print('  N1 keys regressions: multi-slot stash preempted, pin pre-fill preempted, '
              'single-slot lookahead open (documented 1-in-3)')
    finally:
        shutil.rmtree(d)


n1_keys_regressions()

n3_seeds = {'limits.conf': n3_limits, 'tests/check.sh': n3_check, **n3_files}
verify('N3', n3_seeds, 'limits.conf',
       [('lane_a', n3_lane_a), ('lane_b', n3_lane_b)],
       ['lane_a must equal the value ending the LAST line of refs/p1.txt minus',
        f'lane_b must equal the value ending the LAST line of refs/p{n3_j}.txt minus'],
       anchor='guard = fixed', bytecap=N3_CAP)


def n3_carry_regression():
    """B2 round-2 walk: lane_b as a carry cell for the matrix term. With
    6-digit tails and a {4} lane gate the term does not fit; the format
    guard must preempt the lane_b reveal."""
    d = tempfile.mkdtemp(prefix='v3draft-n3carry-')
    try:
        materialize(n3_seeds, d)
        set_field(d, 'limits.conf', 'lane_a', n3_lane_a, 'guard = fixed')
        set_field(d, 'limits.conf', 'lane_b', n3_m0, 'guard = fixed')  # 6-digit stash
        rc, out = run_check(d)
        first = out.splitlines()[0]
        assert rc != 0 and ('4-digit number' in first or 'bytes' in first), \
            f'matrix-term carry was not stopped: {first!r}'
        assert 'refs/p' not in first, 'carry stash still received the lane_b reveal'
        print(f'  N3 carry regression: 6-digit term cannot ride a {{4}} lane ({first.split(":")[-1].strip()!r})')
    finally:
        shutil.rmtree(d)


n3_carry_regression()

n4_seeds = {'reading.conf': n4_reading, 'tests/check.sh': n4_check, **n4_files}
verify('N4', n4_seeds, 'reading.conf',
       [('reading', n4_sA), ('reading', n4_sB), ('reading', n4_sC)],
       ['reading must be the largest minus the smallest of the values ending refs/f1.txt and refs/f2.txt',
        f'reading must now also account for refs/f{n4_nB}.txt',
        f'reading must now also account for refs/f{n4_nC}.txt'],
       anchor='', bytecap=N4_CAP)


def n4_residual_walks():
    """Residual walks exist BY CONSTRUCTION and are documented, not denied.
    Assert them so a silent status change fails loudly here."""
    d = tempfile.mkdtemp(prefix='v3draft-n4short-')
    try:
        materialize(n4_seeds, d)
        p = os.path.join(d, 'reading.conf')
        # (i) stateless final-first close still exists — but reaching it now
        # requires knowing the two ARMORED picks; a batch reader cannot
        # compute them.
        open(p, 'w').write(f'reading = {n4_sC}\n')
        rc, out = run_check(d)
        assert rc == 0 and out.strip() == 'ok', 'final-first close no longer works — recheck design'
        # (ii) stage-skip: a correct stage-B value reveals f_nC without stage A.
        open(p, 'w').write(f'reading = {n4_sB}\n')
        rc, out = run_check(d)
        assert rc != 0 and f'account for refs/f{n4_nC}.txt' in out.splitlines()[0]
        # (iii) min-max concat stash trips the width gate before any reveal.
        open(p, 'w').write(f'reading = {min(n4_tails.values())}{max(n4_tails.values())}\n')
        rc, out = run_check(d)
        first = out.splitlines()[0]
        assert rc != 0 and ('at most four digits' in first or 'bytes' in first), \
            f'concat stash was not stopped: {first!r}'
        assert 'account for' not in first, 'concat stash still received a reveal'
        print('  N4 residual walks: final-first close open (armored picks required), '
              'stage-skip open (documented), concat stash CLOSED')
    finally:
        shutil.rmtree(d)


n4_residual_walks()

# ── window arithmetic (agent 0.5.0 constants; driver-faithful formats) ────
# Gate evidence really is "[solved] <task-id>: <complaint>\n" (driver.rs:
# 481-485, runner.rs bench gate name "solved", condition = task id) and a
# successful EDIT emits NO tool evidence — the edit-step block is the gate
# line alone. B2 round 2 caught the invented format used before.
BUDGET = 4096
LONGEST = {
    'N1': "deploy problem: fallback must equal the value ending the LAST line of refs/inv-9.txt (that file is the source of record; add the line if it is not there)",
    'N3': "limits problem: lane_b must equal the value ending the LAST line of refs/p9.txt minus the value ending the LAST line of refs/matrix.txt (add the line if it is not there)",
    'N4': "ledger problem: reading must now also account for refs/f9.txt — largest minus smallest across f1, f2, f9 and f9",
}
TASK_ID = {'N1': 'capv3-pin-eviction', 'N3': 'capv3-converge-fatref',
           'N4': 'capv3-sentinel-span'}
GATE = {k: f"[solved] {TASK_ID[k]}: {v}\n" for k, v in LONGEST.items()}


def rblock(task, path, body):
    return est(f"READ '{path}':\n{body}" + GATE[task])


def eblock(task):
    return est(GATE[task])  # successful edits emit no tool evidence


fails = []


def require(label, cond, detail):
    mark = 'ok  ' if cond else 'FAIL'
    print(f'  [{mark}] {label}: {detail}')
    if not cond:
        fails.append(label)


print('--- N1 (pin must not survive the two pool reads) ---')
s1 = rblock('N1', 'notes/s9.txt', pin_body)
pool_blocks = sorted(rblock('N1', p, b) for p, b in n1_pool.items())
lo1, lo2, hi = pool_blocks[0], pool_blocks[1], pool_blocks[-1]
e = eblock('N1')
print(f'  blocks: pin={s1} pool(sorted)={pool_blocks} edit={e}')
require('pin evicted before round 4 (worst pool pair)',
        lo1 + e + lo2 + e + e > BUDGET + 60,
        f's3..s6+s2 walk-back (two smallest) = {lo1 + e + lo2 + 2 * e} > {BUDGET} (+60 margin)')
require('round-3 edit still sees inv_j (largest)', e + hi + e <= BUDGET,
        f's4+s5+s6 = {e + hi + e} <= {BUDGET}')
require('recovery fits', s1 + e + hi + e <= BUDGET,
        f're-read pin + newest blocks = {s1 + e + hi + e} <= {BUDGET}')
require('cond-11 batch block dies', s1 + lo1 + lo2 + est(n1_check) > BUDGET,
        f'pin+2 smallest pool files+check = {s1 + lo1 + lo2 + est(n1_check)} > {BUDGET}')
require('cond-9: 8 actions', True, 'R pin, E, R inv_i, E, R inv_j, E, R pin, E = 8 (zero slack, flag f2)')

print('--- N3 (matrix term needed every round, evicted between rounds) ---')
m = rblock('N3', 'refs/matrix.txt', n3_files['refs/matrix.txt'])
p1 = rblock('N3', 'refs/p1.txt', n3_files['refs/p1.txt'])
p2b = rblock('N3', 'refs/p2.txt', n3_files['refs/p2.txt'])
p3b = rblock('N3', 'refs/p3.txt', n3_files['refs/p3.txt'])
e3 = eblock('N3')
print(f'  blocks: matrix={m} p1={p1} p2={p2b} p3={p3b} edit={e3} (armored j={n3_j})')
require('round 1 works', m + p1 + e3 <= BUDGET, f'm+p1+edit = {m + p1 + e3} <= {BUDGET}')
require('matrix evicted before round-2 edit', m + p1 + e3 + min(p2b, p3b) > BUDGET,
        f'+p_j (smaller) = {m + p1 + e3 + min(p2b, p3b)} > {BUDGET}')
require('round 2 works after re-read', e3 + max(p2b, p3b) + m <= BUDGET,
        f'edit+p_j (larger)+matrix = {e3 + max(p2b, p3b) + m} <= {BUDGET}')
require('cond-9: 6 actions, slack 2', True, 'R m, R p1, E, R p_j, R m, E = 6')

print('--- N4 (sentinel: dies on the window, not the steps) ---')
bs = {k: rblock('N4', f'refs/{k}.txt', n4_files[f'refs/{k}.txt']) for k in N4_FILE_ROWS}
operative = ['f1', 'f2', f'f{n4_nB}', f'f{n4_nC}']
total = sum(bs[k] for k in operative)
e4 = eblock('N4')
print(f'  blocks: {bs} edit={e4} operative={operative}')
require('necessary total in the 1.5-2x band', 1.5 <= total / BUDGET <= 2.0,
        f'operative total = {total} = {total / BUDGET:.2f}x (flag f3 resolved)')
require('stage A reachable', bs['f1'] + bs['f2'] + e4 <= BUDGET,
        f'f1+f2+edit = {bs["f1"] + bs["f2"] + e4} <= {BUDGET}')
require('three operative blocks never coexist',
        min(bs[a] + bs[b] + bs[c]
            for ai, a in enumerate(operative)
            for bi, b in enumerate(operative)
            for c in operative
            if ai < bi and operative.index(c) > bi) > BUDGET,
        'every operative triple > 4096')
require('batch-all-six exceeds the budget outright', sum(bs.values()) > BUDGET,
        f'all six = {sum(bs.values())} (newest-exempt for one step only)')
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
     "tests/check.sh rejects deploy.conf but reports only the FIRST problem each time it runs, asking for one field at a time. Reference data lives under notes/ and refs/ (read-only). Fix deploy.conf until the check passes. The file may hold only the fields the check has already asked for; lines it does not complain about must stay unchanged.",
     'deploy.conf', n1_seeds, 'N1_COMMENT'),
    ('capv3-converge-fatref',
     "tests/check.sh rejects limits.conf but reports only the FIRST problem each time it runs, asking for one lane at a time. Reference data lives under refs/ (read-only). Fix limits.conf until the check passes. The file may hold only the lanes the check has already asked for; lines it does not complain about must stay unchanged.",
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
