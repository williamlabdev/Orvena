//! Linux end-to-end containment proof (slice-016). Drives the real `orvena`
//! binary's hidden `__sandbox` shim so the Landlock + seccomp confinement is
//! exercised through the exact path production uses (the shim needs the CLI's
//! `__sandbox` dispatch, which a library test binary does not have).
//!
//! Linux-only and control-gated: if the CI runner's kernel does not actually
//! enforce Landlock, the hard assertions are skipped rather than false-failing —
//! the same pattern as the macOS network test. Real enforcement is asserted only
//! once a control confirms the kernel confines.

#![cfg(target_os = "linux")]

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_orvena");

fn fixture(tag: &str) -> (PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("orvena-sbxlnx-{tag}-{}", std::process::id()));
    let root = base.join("proj");
    let outside = base.join("outside");
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    (root.canonicalize().unwrap(), outside.canonicalize().unwrap())
}

/// Invoke `orvena __sandbox --spec <json> -- <cmd…>` in `root`. `writable` is the
/// sole writable root (system temp is intentionally omitted so `outside/` is out
/// of bounds — a clean negative target).
fn shim(root: &Path, writable: &Path, deny_network: bool, fail_closed: bool, cmd: &[&str]) -> Output {
    let spec = format!(
        r#"{{"writable":["{}"],"deny_network":{deny_network},"fail_closed":{fail_closed}}}"#,
        writable.display(),
    );
    let mut c = Command::new(BIN);
    c.arg("__sandbox").arg("--spec").arg(spec).arg("--").args(cmd).current_dir(root);
    c.output().expect("shim runs")
}

#[test]
fn filesystem_and_network_are_confined() {
    let (root, outside) = fixture("fs");

    // Probe enforcement WITHOUT failing closed: attempt an out-of-root write in
    // warn mode. If Landlock enforces, the write is blocked (sentinel absent); if
    // the runner's kernel lacks Landlock, warn mode runs unconfined and the
    // sentinel appears — in which case skip the hard assertions.
    let probe = outside.join("probe.txt");
    let _ = shim(&root, &root, false, false, &["sh", "-c", &format!("echo x > {}", probe.display())]);
    if probe.exists() {
        eprintln!("SANDBOX-LINUX: SKIPPED — Landlock is not enforcing on this runner");
        let _ = std::fs::remove_dir_all(root.parent().unwrap());
        return;
    }
    eprintln!("SANDBOX-LINUX: Landlock is enforcing — running containment assertions");

    // 1. Out-of-root write is denied by the OS (enforced, fail-closed policy).
    let sentinel = outside.join("pwned.txt");
    let out = shim(&root, &root, true, true, &["sh", "-c", &format!("echo x > {}", sentinel.display())]);
    assert!(!out.status.success(), "out-of-root write must fail under the sandbox");
    assert!(!sentinel.exists(), "no file may appear outside the writable root");

    // 2. In-root write still succeeds — confinement, not crippling.
    let ok = root.join("ok.txt");
    let out = shim(&root, &root, true, true, &["sh", "-c", &format!("echo hi > {}", ok.display())]);
    assert!(out.status.success(), "in-root write must succeed: {}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(std::fs::read_to_string(&ok).unwrap().trim(), "hi");

    // 3. network: deny blocks a connection to an actually-open port. Control-gated:
    //    only assert the deny once a no-deny run confirms the port is reachable.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let connect = format!(
        "python3 -c 'import socket,sys; socket.create_connection((\"127.0.0.1\",{port}),2); print(\"ok\")'"
    );
    let control = shim(&root, &root, false, true, &["sh", "-c", &connect]);
    if control.status.success() {
        let denied = shim(&root, &root, true, true, &["sh", "-c", &connect]);
        assert!(!denied.status.success(), "network:deny must block a connection to an open port");
        eprintln!("SANDBOX-LINUX: network:deny blocked a connection to an open port");
    } else {
        eprintln!("SANDBOX-LINUX: network assertion skipped (control connect did not succeed)");
    }

    eprintln!("SANDBOX-LINUX: containment VERIFIED (out-of-root write + in-root write)");
    drop(listener);
    let _ = std::fs::remove_dir_all(root.parent().unwrap());
}
