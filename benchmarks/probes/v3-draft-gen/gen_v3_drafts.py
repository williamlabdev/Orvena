#!/usr/bin/env python3
"""Generate benchmarks/probes/capability-v3-drafts.yaml (N1/N3/N4 drafts).

Every corpus token is seeded-random; the armor chain (cksum-derived slot
selection) is computed with the REAL `cksum` binary so the generator can
verify end-to-end solvability and the exact complaint sequence before the
YAML is emitted. Nothing is hand-written that a script can verify.
"""
import random, subprocess, os, re, shutil, string, sys, tempfile

rnd = random.Random(20260807)
ALNUM = string.ascii_lowercase + string.digits

def tok(n=12):
    return ''.join(rnd.choice(ALNUM) for _ in range(n))

def cksum_of(s: str) -> int:
    out = subprocess.run(['cksum'], input=s.encode(), capture_output=True, check=True)
    return int(out.stdout.split()[0])

ROWS = 97  # data rows per fat ref; slot index = cksum % 97 + 1

def fat_ref(prefix: str, header: str):
    """97 rows `<prefix>NN  <tok12>  <pad>` padded to ~78 chars, + 1 header."""
    lines = [header]
    vals = {}
    for i in range(1, ROWS + 1):
        slot = f'{prefix}{i:02d}'
        v = tok()
        vals[slot] = v
        pad = tok(60)
        lines.append(f'{slot}  {v}  {pad}')
    body = '\n'.join(lines) + '\n'
    assert len(body) <= 8000 and body.count('\n') <= 98, (len(body), body.count('\n'))
    return body, vals

def chain_slot(prefix: str, value: str) -> str:
    return f'{prefix}{cksum_of(value) % ROWS + 1:02d}'

# ── N1: pin-eviction ──────────────────────────────────────────────────────
pin_vals = {k: tok() for k in ('k1', 'k3', 'k5', 'k7', 'k9')}
pin_body = '# staging pins — rotation 2026-08\n' + \
    ''.join(f'{k} {v}\n' for k, v in pin_vals.items())
inv_a, inv_a_vals = fat_ref('a', '# slot inventory A — provisioning source of record')
inv_b, inv_b_vals = fat_ref('b', '# slot inventory B — provisioning source of record')

n1_release = pin_vals['k1']
n1_slot_a = chain_slot('a', n1_release)
n1_mirror = inv_a_vals[n1_slot_a]
n1_slot_b = chain_slot('b', n1_mirror)
n1_fallback = inv_b_vals[n1_slot_b]
n1_key4 = f'k{(cksum_of(n1_fallback) % 4) * 2 + 3}'  # k3/k5/k7/k9, runtime-derived
n1_rollback = pin_vals[n1_key4]

assert n1_key4 in ('k3', 'k5', 'k7', 'k9'), n1_key4
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
n=$(printf %s "$got" | cksum | awk '{{print $1 % {ROWS} + 1}}')
slot=$(printf 'a%02d' "$n")
want=$(awk -v s="$slot" '$1==s{{print $2}}' refs/inv-a.txt)
got=$(sed -n 's/^mirror = //p' "$f")
test "$got" = "$want" || fail "mirror must equal the value of slot $slot in refs/inv-a.txt"
n=$(printf %s "$got" | cksum | awk '{{print $1 % {ROWS} + 1}}')
slot=$(printf 'b%02d' "$n")
want=$(awk -v s="$slot" '$1==s{{print $2}}' refs/inv-b.txt)
got=$(sed -n 's/^fallback = //p' "$f")
test "$got" = "$want" || fail "fallback must equal the value of slot $slot in refs/inv-b.txt"
n=$(printf %s "$got" | cksum | awk '{{print $1 % 4}}')
key=k$((2 * n + 3))
want=$(awk -v k="$key" '$1==k{{print $2}}' notes/s9.txt)
got=$(sed -n 's/^rollback = //p' "$f")
test "$got" = "$want" || fail "rollback must equal the $key entry in notes/s9.txt"
echo ok
"""

# ── N3: converge-fatref ───────────────────────────────────────────────────
matrix, matrix_vals = fat_ref('m', '# allocation matrix — planner source of record')
n3_s1 = matrix_vals['m41']
n3_slot2 = chain_slot('m', n3_s1)
n3_s2 = matrix_vals[n3_slot2]
n3_slot3 = chain_slot('m', n3_s2)
n3_s3 = matrix_vals[n3_slot3]
assert len({'m41', n3_slot2, n3_slot3}) == 3, "N3 chain slots must be distinct"

audit_lines = '\n'.join(
    f'audit {i:03d}  {tok(56)}  ok' for i in range(1, 85))
n3_alloc = """# capacity allocations — planner input
s1 = ------------
s2 = ------------
s3 = ------------
guard = fixed
"""
n3_final = n3_alloc
for _f, _v in (('s1', n3_s1), ('s2', n3_s2), ('s3', n3_s3)):
    n3_final = n3_final.replace(f'{_f} = ------------', f'{_f} = {_v}')
N3_CAP = len(n3_final.encode()) + 4
n3_check = f"""#!/bin/sh
f=alloc.conf
audit() {{ cat <<'EOF'
{audit_lines}
EOF
}}
fail() {{ echo "alloc problem: $1"; audit; exit 1; }}
test "$(wc -c < "$f" | tr -d ' ')" -le {N3_CAP} || fail "alloc.conf must stay under {N3_CAP} bytes"
test "$(wc -l < "$f" | tr -d ' ')" = "5" || fail "alloc.conf must stay exactly 5 lines"
grep -qxF '# capacity allocations — planner input' "$f" || fail "an unrelated line changed: header"
grep -qxF 'guard = fixed' "$f" || fail "an unrelated line changed: guard"
want=$(awk '$1=="m41"{{print $2}}' refs/matrix.txt)
got=$(sed -n 's/^s1 = //p' "$f")
test "$got" = "$want" || fail "s1 must equal the value of slot m41 in refs/matrix.txt"
n=$(printf %s "$got" | cksum | awk '{{print $1 % {ROWS} + 1}}')
slot=$(printf 'm%02d' "$n")
want=$(awk -v s="$slot" '$1==s{{print $2}}' refs/matrix.txt)
got=$(sed -n 's/^s2 = //p' "$f")
test "$got" = "$want" || fail "s2 must equal the value of slot $slot in refs/matrix.txt"
n=$(printf %s "$got" | cksum | awk '{{print $1 % {ROWS} + 1}}')
slot=$(printf 'm%02d' "$n")
want=$(awk -v s="$slot" '$1==s{{print $2}}' refs/matrix.txt)
got=$(sed -n 's/^s3 = //p' "$f")
test "$got" = "$want" || fail "s3 must equal the value of slot $slot in refs/matrix.txt"
echo ok
"""

# ── N4: sentinel-fullset ──────────────────────────────────────────────────
pool_a, pa_vals = fat_ref('pa', '# rollout pool A — capacity source of record')
pool_b, pb_vals = fat_ref('pb', '# rollout pool B — capacity source of record')
pool_c, pc_vals = fat_ref('pc', '# rollout pool C — capacity source of record')

n4_t1 = pa_vals['pa19']
n4_s2 = chain_slot('pb', n4_t1); n4_t2 = pb_vals[n4_s2]
n4_s3 = chain_slot('pc', n4_t2); n4_t3 = pc_vals[n4_s3]
n4_s4 = chain_slot('pa', n4_t3); n4_t4 = pa_vals[n4_s4]
n4_s5 = chain_slot('pb', n4_t4); n4_t5 = pb_vals[n4_s5]
assert n4_s4 != 'pa19', "N4 t4 slot must differ from the bootstrap slot"
assert n4_s5 != n4_s2, "N4 t5 slot must differ from t2's slot"

n4_rollout = """# rollout manifest — release train
t1 = ------------
t2 = ------------
t3 = ------------
t4 = ------------
t5 = ------------
owner = release-eng
"""
n4_final = n4_rollout
for _f, _v in (('t1', n4_t1), ('t2', n4_t2), ('t3', n4_t3),
               ('t4', n4_t4), ('t5', n4_t5)):
    n4_final = n4_final.replace(f'{_f} = ------------', f'{_f} = {_v}')
N4_CAP = len(n4_final.encode()) + 4
n4_check = f"""#!/bin/sh
f=rollout.conf
fail() {{ echo "rollout problem: $1"; exit 1; }}
test "$(wc -c < "$f" | tr -d ' ')" -le {N4_CAP} || fail "rollout.conf must stay under {N4_CAP} bytes"
test "$(wc -l < "$f" | tr -d ' ')" = "7" || fail "rollout.conf must stay exactly 7 lines"
grep -qxF '# rollout manifest — release train' "$f" || fail "an unrelated line changed: header"
grep -qxF 'owner = release-eng' "$f" || fail "an unrelated line changed: owner"
want=$(awk '$1=="pa19"{{print $2}}' refs/pool-a.txt)
got=$(sed -n 's/^t1 = //p' "$f")
test "$got" = "$want" || fail "t1 must equal the value of slot pa19 in refs/pool-a.txt"
n=$(printf %s "$got" | cksum | awk '{{print $1 % {ROWS} + 1}}')
slot=$(printf 'pb%02d' "$n")
want=$(awk -v s="$slot" '$1==s{{print $2}}' refs/pool-b.txt)
got=$(sed -n 's/^t2 = //p' "$f")
test "$got" = "$want" || fail "t2 must equal the value of slot $slot in refs/pool-b.txt"
n=$(printf %s "$got" | cksum | awk '{{print $1 % {ROWS} + 1}}')
slot=$(printf 'pc%02d' "$n")
want=$(awk -v s="$slot" '$1==s{{print $2}}' refs/pool-c.txt)
got=$(sed -n 's/^t3 = //p' "$f")
test "$got" = "$want" || fail "t3 must equal the value of slot $slot in refs/pool-c.txt"
n=$(printf %s "$got" | cksum | awk '{{print $1 % {ROWS} + 1}}')
slot=$(printf 'pa%02d' "$n")
want=$(awk -v s="$slot" '$1==s{{print $2}}' refs/pool-a.txt)
got=$(sed -n 's/^t4 = //p' "$f")
test "$got" = "$want" || fail "t4 must equal the value of slot $slot in refs/pool-a.txt"
n=$(printf %s "$got" | cksum | awk '{{print $1 % {ROWS} + 1}}')
slot=$(printf 'pb%02d' "$n")
want=$(awk -v s="$slot" '$1==s{{print $2}}' refs/pool-b.txt)
got=$(sed -n 's/^t5 = //p' "$f")
test "$got" = "$want" || fail "t5 must equal the value of slot $slot in refs/pool-b.txt"
echo ok
"""

# ── mechanical verification: full honest walk per task ───────────────────
def verify(name, seeds, writable, fixes, expected_complaints, bytecap=None):
    """Materialize seeds, apply fixes one at a time, assert the check's
    complaint sequence matches exactly and ends in `ok`."""
    d = tempfile.mkdtemp(prefix=f'v3draft-{name}-')
    try:
        for path, body in seeds.items():
            p = os.path.join(d, path)
            os.makedirs(os.path.dirname(p), exist_ok=True)
            with open(p, 'w') as fh:
                fh.write(body)
        def run_check():
            r = subprocess.run(['sh', 'tests/check.sh'], cwd=d,
                               capture_output=True, text=True)
            return r.returncode, r.stdout
        for i, ((field, value), complaint) in enumerate(zip(fixes, expected_complaints)):
            rc, out = run_check()
            first = out.splitlines()[0] if out else ''
            assert rc != 0, f'{name}: check passed too early at round {i}'
            assert complaint in first, f'{name} round {i}: got {first!r}, want {complaint!r}'
            wpath = os.path.join(d, writable)
            src = open(wpath).read()
            new = re.sub(rf'^{field} = .*$', f'{field} = {value}', src, flags=re.M)
            assert new != src, f'{name}: fix {field} did not change the file'
            open(wpath, 'w').write(new)
        rc, out = run_check()
        assert rc == 0 and out.splitlines()[-1] == 'ok', f'{name}: final check not ok: {out!r}'
        if bytecap is not None:
            final = os.path.getsize(os.path.join(d, writable))
            assert bytecap - final == 4, \
                f'{name}: cap {bytecap} minus solved {final}B must be exactly 4'
        print(f'  {name}: {len(fixes)} rounds verified, complaint order exact, final ok')
    finally:
        shutil.rmtree(d)

n1_seeds = {'deploy.conf': n1_deploy, 'notes/s9.txt': pin_body,
            'refs/inv-a.txt': inv_a, 'refs/inv-b.txt': inv_b,
            'tests/check.sh': n1_check}
verify('N1', n1_seeds, 'deploy.conf',
       [('release', n1_release), ('mirror', n1_mirror),
        ('fallback', n1_fallback), ('rollback', n1_rollback)],
       ['release must equal the k1 entry',
        f'mirror must equal the value of slot {n1_slot_a}',
        f'fallback must equal the value of slot {n1_slot_b}',
        f'rollback must equal the {n1_key4} entry'], bytecap=N1_CAP)

def stash_regression():
    d = tempfile.mkdtemp(prefix='v3draft-stashreg-')
    try:
        for path, body in n1_seeds.items():
            p = os.path.join(d, path)
            os.makedirs(os.path.dirname(p), exist_ok=True)
            open(p, 'w').write(body)
        # verifier round-2 walk: fix release, pack all five pin values into
        # the not-yet-checked rollback field (line count and frozen lines
        # untouched)
        stash = ''.join(pin_vals.values())
        conf = open(os.path.join(d, 'deploy.conf')).read()
        conf = conf.replace('release = ------------', f'release = {n1_release}')
        conf = conf.replace('rollback = ------------', f'rollback = {stash}')
        open(os.path.join(d, 'deploy.conf'), 'w').write(conf)
        r = subprocess.run(['sh', 'tests/check.sh'], cwd=d, capture_output=True, text=True)
        first = r.stdout.splitlines()[0]
        assert r.returncode != 0 and 'bytes' in first, \
            f'stash regression FAILED: guard did not preempt, got {first!r}'
        assert 'mirror' not in first, 'stash walk still received the round-2 reveal'
        print(f'  N1 stash regression: guard preempts reveal ({first!r})')
    finally:
        shutil.rmtree(d)
stash_regression()

n3_seeds = {'alloc.conf': n3_alloc, 'refs/matrix.txt': matrix,
            'tests/check.sh': n3_check}
verify('N3', n3_seeds, 'alloc.conf',
       [('s1', n3_s1), ('s2', n3_s2), ('s3', n3_s3)],
       ['s1 must equal the value of slot m41',
        f's2 must equal the value of slot {n3_slot2}',
        f's3 must equal the value of slot {n3_slot3}'], bytecap=N3_CAP)

n4_seeds = {'rollout.conf': n4_rollout, 'refs/pool-a.txt': pool_a,
            'refs/pool-b.txt': pool_b, 'refs/pool-c.txt': pool_c,
            'tests/check.sh': n4_check}
verify('N4', n4_seeds, 'rollout.conf',
       [('t1', n4_t1), ('t2', n4_t2), ('t3', n4_t3),
        ('t4', n4_t4), ('t5', n4_t5)],
       ['t1 must equal the value of slot pa19',
        f't2 must equal the value of slot {n4_s2}',
        f't3 must equal the value of slot {n4_s3}',
        f't4 must equal the value of slot {n4_s4}',
        f't5 must equal the value of slot {n4_s5}'], bytecap=N4_CAP)

# size report (token estimate = ceil(chars/4), util.rs:9)
import math
for label, body in [('inv-a', inv_a), ('inv-b', inv_b), ('matrix', matrix),
                    ('pool-a', pool_a), ('pool-b', pool_b), ('pool-c', pool_c),
                    ('n3-audit-block', audit_lines)]:
    print(f'  {label}: {len(body)} chars ≈ {math.ceil(len(body)/4)} tokens, '
          f'{body.count(chr(10))+ (0 if body.endswith(chr(10)) else 1)} lines')

# ── emit YAML ─────────────────────────────────────────────────────────────
def yblock(body, indent=10):
    pad = ' ' * indent
    return '\n'.join(pad + ln if ln else pad.rstrip() for ln in body.rstrip('\n').split('\n'))

def seed_yaml(seeds):
    out = []
    for path, body in seeds.items():
        out.append(f'      - path: {path}\n        contents: |\n{yblock(body)}')
    return '\n'.join(out)

HEADER = open(os.path.join(os.path.dirname(os.path.abspath(__file__)),
                           'v3_drafts_header.txt')).read()

tasks_yaml = []
for tid, instr, writable, seeds, taskcmt in [
    ('capv3-pin-eviction',
     "tests/check.sh rejects deploy.conf but reports only the FIRST problem each time it runs. Reference data lives under notes/ and refs/ (read-only). Fix deploy.conf until the check passes. Lines it does not complain about must stay unchanged.",
     'deploy.conf', n1_seeds, 'N1_COMMENT'),
    ('capv3-converge-fatref',
     "tests/check.sh rejects alloc.conf but reports only the FIRST problem each time it runs. Reference data lives under refs/ (read-only). Fix alloc.conf until the check passes. Lines it does not complain about must stay unchanged.",
     'alloc.conf', n3_seeds, 'N3_COMMENT'),
    ('capv3-sentinel-fullset',
     "tests/check.sh rejects rollout.conf but reports only the FIRST problem each time it runs. Reference data lives under refs/ (read-only). Fix rollout.conf until the check passes. Lines it does not complain about must stay unchanged.",
     'rollout.conf', n4_seeds, 'N4_COMMENT'),
]:
    cmt = open(os.path.join(os.path.dirname(os.path.abspath(__file__)),
                            f'{taskcmt}.txt')).read().rstrip('\n')
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
