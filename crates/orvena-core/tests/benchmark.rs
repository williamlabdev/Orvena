//! The benchmark harness, exercised deterministically with the offline provider
//! (no network). The offline stub writes the first writable target with
//! boilerplate content, so a mixed task set has a *known* outcome:
//!
//!   - a "file exists" task is solved (the write satisfies `test -f`);
//!   - a "contents must include DONE" task is not (boilerplate ≠ DONE).
//!
//! → a completion rate of exactly 0.5, which pins the aggregation, the per-task
//! pass/fail flags, and the per-task evidence bundles.

use orvena_core::benchmark::{self, BenchTask, BenchTaskSet, SeedFile};
use orvena_core::config::agent::ProviderSelection;
use orvena_core::RunReport;

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("orvena-bench-{tag}-{pid}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[tokio::test]
async fn offline_benchmark_reports_a_known_completion_rate() {
    let base = temp_dir("known-rate");
    let set = BenchTaskSet {
        tasks: vec![
            // Solved by offline: writing a.txt satisfies the verify.
            BenchTask {
                id: "make-a".into(),
                instruction: "Create a file named a.txt".into(),
                writes: vec!["a.txt".into()],
                verify: "test -f a.txt".into(),
                seed: vec![],
                timeout_secs: None,
            },
            // Not solved by offline: it overwrites b.txt with boilerplate, which
            // does not contain DONE — a deterministic non-completion.
            BenchTask {
                id: "fix-b".into(),
                instruction: "Edit b.txt so it contains DONE".into(),
                writes: vec!["b.txt".into()],
                verify: "grep -q DONE b.txt".into(),
                seed: vec![SeedFile { path: "b.txt".into(), contents: "TODO\n".into() }],
                timeout_secs: None,
            },
        ],
    };
    let provider =
        ProviderSelection { kind: "offline".into(), model: "stub".into(), base_url: None };

    let report = benchmark::run_benchmark(&set, &provider, &base, "testrun").await.unwrap();

    // Aggregate: one of two tasks solved.
    assert_eq!(report.task_count, 2);
    assert_eq!(report.passed, 1);
    assert!((report.completion_rate - 0.5).abs() < f32::EPSILON, "rate: {}", report.completion_rate);
    assert_eq!(report.provider, "offline");

    // Per-task flags are correct.
    let a = report.results.iter().find(|r| r.id == "make-a").unwrap();
    let b = report.results.iter().find(|r| r.id == "fix-b").unwrap();
    assert!(a.completed, "the file-exists task should be solved");
    assert!(!b.completed, "the contents task should not be solved by the offline stub");

    // Every solved task left an auditable evidence bundle that round-trips.
    let path = a.evidence_path.as_ref().expect("a solved task has an evidence bundle");
    assert!(path.exists(), "evidence bundle should exist at {}", path.display());
    let reloaded: RunReport =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).expect("bundle deserializes");
    assert!(reloaded.completed);

    // The JSON report writes and round-trips too.
    let report_path = base.join("report.json");
    benchmark::write_report(&report, &report_path).unwrap();
    let reloaded_report: benchmark::BenchReport =
        serde_json::from_str(&std::fs::read_to_string(&report_path).unwrap()).unwrap();
    assert_eq!(reloaded_report.passed, 1);
    assert_eq!(reloaded_report.results.len(), 2);

    let _ = std::fs::remove_dir_all(&base);
}
