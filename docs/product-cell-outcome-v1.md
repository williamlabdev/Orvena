# Native ProductCell outcome contract v1

Orvena can attach an explicit ProductCell outcome contract to a native
`RunReport`. The contract is additive to the existing `orvena-evidence-v1`
bundle and is written inside the same `evidence.json`, so the outcome remains
linked to the bounded run that produced it.

## Contract

`schemas/product-cell-outcome.v1.json` defines three required sections:

- `value_signal`: `PASS`, `FAIL`, `INCONCLUSIVE`, or `BLOCKED`, plus portable
  evidence references. Orvena never upgrades an absent or unverified signal to
  `PASS`.
- `execution_provenance`: provider, model, run count, and portable evidence
  references copied from the native report.
- `rollback`: whether a declared-path rollback rehearsal succeeded and its
  portable evidence reference.

Attach the contract after the run has completed or received an honest terminal
result:

```rust
let report = agent.run(task).await?;
let report = report.with_product_cell_outcome(
    ValueSignal {
        result: ValueSignalResult::Pass,
        source_evidence_refs: vec!["orvena://runs/native-1".into()],
    },
    RollbackEvidence {
        rehearsed: true,
        evidence_ref: "package://rollback/native-1".into(),
    },
    vec!["orvena://runs/native-1".into()],
)?;
evidence::write_bundle(&report, &path)?;
```

### Aggregating a run series

A readiness gate may require provenance for a series of runs (for example the
PF-3 evaluation's `run_count: 3`). `from_native_runs` /
`RunReport::with_product_cell_outcome_runs` build the same contract with an
explicit `run_count`; the provenance refs must then contain at least one
distinct portable ref per aggregated run, so a count is never claimed without
matching evidence. All aggregated runs must share the final run's
provider/model provenance.

From the CLI, the final run of the series attaches the aggregate:

```sh
orvena run "<task>"   --outcome-value PASS   --outcome-evidence-ref "orvena://acceptance/<ref>"   --outcome-run-ref "orvena://runs/<run-1>"   --outcome-run-ref "orvena://runs/<run-2>"   --outcome-run-count 3   --rollback-rehearsed --rollback-evidence-ref "package://rollback/<ref>"
```

This run's own ref is appended automatically; `--outcome-run-ref` lists the
earlier runs.

`RollbackJournal` provides a bounded rehearsal for explicitly declared,
root-relative files. It rejects absolute paths, parent traversal, symlink
roots, and symlink components; it restores the original bytes or absence after
the operation. It does not mutate a Registry, policy, deployment, release, or
external system.

The API is evidence/reporting capability only. Native ProductCell adoption,
promotion, deployment, and release remain outside Orvena's automatic path.

## Verification

The acceptance test
`crates/orvena-core/tests/product_cell_outcome.rs` runs the native offline
agent, attaches the contract, writes an evidence bundle, validates the frozen
evidence validator and the dedicated JSON schema, and checks all three fields
survive the round trip.

The retained bounded run is at
`crates/orvena-core/bench-runs/product-cell-outcome/20260827-native-contract-v1/evidence.json`.
It is linked into the AINE PF-3 lineage report as
`orvena://runs/native-product-cell-acceptance`; the linked report explicitly
remains capability validation, not ProductCell adoption or release.
