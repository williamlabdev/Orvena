#!/usr/bin/env python3
"""Judge a capv3 token-shape probe report against the generator's thresholds.

Usage: judge-v3-tokenshape.py THRESHOLDS.json REPORT.json [REPORT.json ...]

The criterion this implements is the 0816 ruling (SLICE-032, "token 形狀探針
判準改判"): the retired "window_peak_tokens near budget" test is not reachable
by an honest walk (READ evidence is capped at ~2048 tokens by the RUN caps,
half the budget) and IS reachable by the condition-11 batch walk (the newest
block is retained unconditionally, so one giant read posts a peak above the
budget). Absolute occupancy therefore ranks the dead walk above the honest
one. What replaces it, per task:

  peak_floor        window_peak_tokens >= the task's smallest fat block —
                    a fat file really entered the window.
  first_step_max    evictions.first_step <= N when the task derives one from
                    its honest walk (null = no dynamic bound for this task;
                    the static generator assertion still applies).
  go_back_required  dropped_reread + dropped_research > 0 — the model went
                    back for something the window dropped, which is the
                    observable form of "the evicted block was a needed one"
                    (the driver keeps no per-step path log to ask directly).
  peak <= budget    sanity: a peak ABOVE the budget is the batch-walk
                    signature (newest-block exemption) — flagged, because a
                    pass with that signature is the walk condition 11 kills,
                    not the behavior under measurement.

Exit 0 iff every judged run of every judged task passes all applicable
checks. Runs whose telemetry is None (wrapped agent / pre-field report) are
counted as NOT JUDGEABLE and fail loudly rather than passing silently.
"""
import json
import sys


def judge_run(run, spec, budget):
    """Return (verdicts, ok) for one run against one task's thresholds."""
    verdicts = []
    ok = True

    peak = run.get('window_peak_tokens')
    ev = run.get('evictions')
    reread = run.get('dropped_reread')
    research = run.get('dropped_research')

    if peak is None or ev is None or reread is None or research is None:
        return (['window telemetry is None — run not judgeable '
                 '(wrapped agent or pre-field report)'], False)

    floor = spec['peak_floor']
    line = f'peak_floor: window_peak_tokens {peak} >= {floor}'
    if peak >= floor:
        verdicts.append(f'  [ok  ] {line}')
    else:
        verdicts.append(f'  [FAIL] {line} — no fat block entered the window')
        ok = False

    line = f'peak <= budget: {peak} <= {budget}'
    if peak <= budget:
        verdicts.append(f'  [ok  ] {line}')
    else:
        verdicts.append(f'  [FAIL] {line} — batch-walk signature '
                        '(newest-block exemption); this is the condition-11 '
                        'walk, not the measured behavior')
        ok = False

    fsm = spec.get('first_step_max')
    if fsm is not None:
        fs = ev.get('first_step')
        line = f'first_step_max: evictions.first_step {fs} <= {fsm}'
        if fs is not None and fs <= fsm:
            verdicts.append(f'  [ok  ] {line}')
        else:
            verdicts.append(f'  [FAIL] {line} — the eviction the honest walk '
                            'depends on did not happen in time')
            ok = False
    else:
        verdicts.append('  [ --  ] first_step_max: none derived for this task '
                        '(static generator assertion still applies)')

    if spec.get('go_back_required'):
        total = reread + research
        line = (f'go_back_required: dropped_reread {reread} + '
                f'dropped_research {research} = {total} > 0')
        if total > 0:
            verdicts.append(f'  [ok  ] {line}')
        else:
            verdicts.append(f'  [FAIL] {line} — nothing evicted was gone back '
                            'for; the pressure did not land on a needed block')
            ok = False

    return verdicts, ok


def main(argv):
    if len(argv) < 3:
        print(__doc__.strip().splitlines()[2])
        return 2
    thresholds = json.load(open(argv[1]))
    budget = thresholds['budget']
    specs = thresholds['tasks']

    all_ok = True
    judged_any = False
    for path in argv[2:]:
        report = json.load(open(path))
        # A repeated report nests runs; a single-run report IS the run set.
        runs = report.get('runs') or [report]
        print(f'== {path} ==')
        for run in runs:
            for res in run.get('results', []):
                spec = specs.get(res.get('id'))
                if spec is None:
                    continue
                judged_any = True
                rep = run.get('run_id', '?')
                print(f'-- {res["id"]}  (run {rep}) --')
                verdicts, ok = judge_run(res, spec, budget)
                print('\n'.join(verdicts))
                all_ok = all_ok and ok

    if not judged_any:
        print('no judgeable capv3 task results found in the given reports')
        return 2
    print('\ntoken-shape probe: ' + ('PASS' if all_ok else 'FAIL'))
    return 0 if all_ok else 1


if __name__ == '__main__':
    sys.exit(main(sys.argv))
