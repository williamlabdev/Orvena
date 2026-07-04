//! First-run ergonomics (slice-005). A brand-new user must be able to go from
//! `init` to a working loop + evidence bundle without getting stuck on setup:
//!
//!   - `run --provider offline` gives a zero-setup first run (no key, no
//!     network) that completes and exports an evidence bundle; and
//!   - a run against a not-ready provider fails fast with actionable guidance
//!     instead of a deep provider/network error.
//!
//! These drive the built `orvena` binary end to end (Cargo exposes its path via
//! `CARGO_BIN_EXE_orvena`), so they exercise the real init → run wiring.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_orvena");

fn temp_dir(tag: &str) -> PathBuf {
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("orvena-firstrun-{tag}-{pid}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// `orvena init` in a fresh dir. Stdin is not a TTY here, so init takes its
/// non-interactive path: deploy the scaffold (provider defaults to anthropic).
fn init(dir: &Path) {
    let out = Command::new(BIN).arg("init").current_dir(dir).output().expect("init runs");
    assert!(out.status.success(), "init should succeed: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn offline_override_gives_a_zero_setup_first_run_with_evidence() {
    let dir = temp_dir("offline");
    init(&dir);

    // The scaffold `tests-pass` gate is `verify: "true"`, so the offline provider
    // writing the target satisfies it — the first run completes with no setup.
    let out = Command::new(BIN)
        .args(["run", "--provider", "offline", "create a greeting", "-w", "hello.txt"])
        .current_dir(&dir)
        .output()
        .expect("run executes");

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "offline first run should complete; stdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("evidence bundle"), "the bundle path is printed: {stdout}");
    let runs = dir.join(".orvena/runs");
    let landed = std::fs::read_dir(&runs).map(|rd| rd.count() > 0).unwrap_or(false);
    assert!(landed, "an evidence bundle should exist under {}", runs.display());

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn missing_key_fails_fast_with_actionable_guidance() {
    let dir = temp_dir("nokey");
    init(&dir); // scaffold default provider is anthropic, which needs a key

    let out = Command::new(BIN)
        .args(["run", "create a greeting", "-w", "hello.txt"])
        .current_dir(&dir)
        .env_remove("ANTHROPIC_API_KEY") // ensure the preflight, not ambient env, decides
        .output()
        .expect("run executes");

    assert!(!out.status.success(), "a not-ready provider must not start a run");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("not ready"), "guidance names the problem: {stderr}");
    assert!(
        stderr.contains("doctor") || stderr.contains(".env") || stderr.contains("--provider offline"),
        "guidance points somewhere actionable: {stderr}"
    );
    // Fail-fast: it must not have produced an evidence bundle from a real run.
    assert!(!dir.join(".orvena/runs").exists(), "no run should have happened");

    let _ = std::fs::remove_dir_all(&dir);
}
