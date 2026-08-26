//! Containment proof for the OS sandbox (slice-015 / ADR-003). The unit tests in
//! `exec/sandbox*.rs` prove the *plumbing* (prefix shape, profile text, status);
//! this file proves the *guarantee*: a command wrapped by the real platform
//! backend actually cannot write outside its writable set or (on macOS, under
//! `network: deny`) reach the network — while a legitimate in-root write still
//! succeeds.
//!
//! The macOS assertions are the headline: they run the real `sandbox-exec`
//! backend end-to-end through `CommandRunner`. On platforms without a backend
//! yet (Linux/other), the assertion is the *fail-closed* contract instead — an
//! enforced run refuses to spawn rather than run a child unconfined.

use orvena_core::exec::sandbox::{
    FsPolicy, NetworkPolicy, OnUnavailable, Sandbox, SandboxBackend, SandboxPolicy, SandboxStatus,
};
use orvena_core::exec::CommandRunner;
#[cfg(not(target_os = "macos"))]
use orvena_core::exec::RunError;
use std::path::PathBuf;
use std::time::Duration;

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

/// A project root under the system temp, plus a sibling "outside" dir that is
/// deliberately NOT in the writable set (we omit system temp from the policy so
/// the sibling is genuinely out-of-bounds — a clean negative target).
struct Fixture {
    root: PathBuf,
    // Only the macOS tests write an out-of-root sentinel; on other platforms the
    // field would be dead code (which `-D warnings` rejects).
    #[cfg(target_os = "macos")]
    outside: PathBuf,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let base = std::env::temp_dir().join(format!("orvena-sbx-{tag}-{}", std::process::id()));
        let root = base.join("proj");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&root).unwrap();
        // Canonicalize so the policy's root matches what the sandbox sees.
        let root = root.canonicalize().unwrap();
        #[cfg(target_os = "macos")]
        {
            let outside = base.join("outside");
            std::fs::create_dir_all(&outside).unwrap();
            Self { root, outside: outside.canonicalize().unwrap() }
        }
        #[cfg(not(target_os = "macos"))]
        {
            Self { root }
        }
    }

    /// Policy whose ONLY writable subtree is the project root (system temp is
    /// intentionally omitted, so `outside/` — a temp sibling — is out of bounds).
    fn policy(&self, network: NetworkPolicy) -> SandboxPolicy {
        SandboxPolicy {
            root: self.root.clone(),
            network,
            filesystem: FsPolicy::RootWrite,
            extra_writable: vec![],
            on_unavailable: OnUnavailable::FailClosed,
            backend: SandboxBackend::Seatbelt,
        }
    }

    fn cleanup(&self) {
        if let Some(base) = self.root.parent() {
            let _ = std::fs::remove_dir_all(base);
        }
    }
}

#[cfg(target_os = "macos")]
#[test]
fn macos_write_outside_root_is_denied_but_in_root_write_succeeds() {
    let fx = Fixture::new("fs");
    let sandbox = Sandbox::for_policy(fx.policy(NetworkPolicy::Deny));
    assert_eq!(sandbox.status(), SandboxStatus::Enforced, "sandbox-exec must be enforcing");

    let runner = CommandRunner::with_sandbox(&fx.root, Duration::from_secs(30), sandbox);

    // 1. A write OUTSIDE the writable set must be blocked by the OS. The command
    //    itself is a plain `sh -c` redirect — the containment is the sandbox's,
    //    not the command's.
    let sentinel = fx.outside.join("pwned.txt");
    let out = runner
        .run_argv(&s(&["sh", "-c", &format!("echo pwned > {}", sentinel.display())]))
        .unwrap();
    assert!(!out.success(), "the out-of-root write must fail under the sandbox");
    assert!(!sentinel.exists(), "no file may appear outside the writable root");

    // 2. A write INSIDE the root must still succeed — the sandbox confines, it
    //    does not cripple legitimate build/test writes (AC-V3 dogfood shape).
    let allowed = fx.root.join("ok.txt");
    let out =
        runner.run_argv(&s(&["sh", "-c", &format!("echo hi > {}", allowed.display())])).unwrap();
    assert!(out.success(), "an in-root write must succeed: {}{}", out.stdout, out.stderr);
    assert_eq!(std::fs::read_to_string(&allowed).unwrap().trim(), "hi");

    fx.cleanup();
}

#[cfg(target_os = "macos")]
#[test]
fn macos_network_deny_blocks_a_reachable_port() {
    use std::net::TcpListener;

    // Open a real listener so a connection *would* succeed but for the sandbox —
    // any failure is then attributable to network confinement, not a closed port.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let fx = Fixture::new("net");
    let addr = format!("127.0.0.1:{port}");

    // Control: without the sandbox, connecting to the open port must succeed. If
    // it does not (missing `nc`, odd environment), skip rather than false-fail.
    let control = CommandRunner::new(&fx.root, Duration::from_secs(5)).run_argv(&s(&[
        "nc",
        "-z",
        "-w",
        "2",
        "127.0.0.1",
        &port.to_string(),
    ]));
    let control_ok = matches!(&control, Ok(o) if o.success());
    if !control_ok {
        eprintln!("skipping network-deny assertion: control connect did not succeed ({control:?})");
        fx.cleanup();
        return;
    }

    // Under `network: deny`, the same connect must fail — the OS blocks the
    // socket even though the port is open.
    let sandbox = Sandbox::for_policy(fx.policy(NetworkPolicy::Deny));
    assert_eq!(sandbox.status(), SandboxStatus::Enforced);
    let out = CommandRunner::with_sandbox(&fx.root, Duration::from_secs(5), sandbox)
        .run_argv(&s(&["nc", "-z", "-w", "2", "127.0.0.1", &port.to_string()]))
        .unwrap();
    assert!(!out.success(), "network:deny must block a connection to an open port ({addr})");

    drop(listener);
    fx.cleanup();
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
#[test]
fn without_a_backend_enforced_runs_fail_closed() {
    // The fail-closed contract on a platform with NO sandbox backend: an enforced
    // policy must refuse to spawn — never run a child unconfined. (Linux now has a
    // backend, so it is covered by the Landlock-aware test below instead.)
    let fx = Fixture::new("failclosed");
    let sandbox = Sandbox::for_policy(fx.policy(NetworkPolicy::Deny));
    assert_eq!(sandbox.status(), SandboxStatus::Unavailable);

    let runner = CommandRunner::with_sandbox(&fx.root, Duration::from_secs(5), sandbox);
    let err = runner.run_argv(&s(&["echo", "hi"])).unwrap_err();
    assert!(matches!(err, RunError::Sandbox(_)), "must refuse via a sandbox error, got {err:?}");

    fx.cleanup();
}

#[cfg(target_os = "linux")]
#[test]
fn linux_backend_is_enforced_or_fails_closed_never_unconfined() {
    // The Landlock backend's outcome depends on the kernel: available → enforced;
    // absent → fail-closed refusal. Either way an enabled+enforced policy must
    // never degrade to a silently-unconfined run. Real end-to-end containment (an
    // out-of-root write / a socket actually blocked) is proven against the real
    // `orvena` binary in `orvena-cli/tests/sandbox_linux.rs`, since the shim needs
    // the CLI's `__sandbox` dispatch, not this test binary.
    let fx = Fixture::new("linux");
    let sandbox = Sandbox::for_policy(fx.policy(NetworkPolicy::Deny));
    match sandbox.status() {
        SandboxStatus::Enforced => {
            // Backend present: the prefix must build cleanly. We do not spawn here
            // — that would re-exec *this* test binary with `__sandbox`.
            let runner = CommandRunner::with_sandbox(&fx.root, Duration::from_secs(5), sandbox);
            let _ = runner;
        }
        SandboxStatus::Unavailable => {
            let runner = CommandRunner::with_sandbox(&fx.root, Duration::from_secs(5), sandbox);
            let err = runner.run_argv(&s(&["echo", "hi"])).unwrap_err();
            assert!(matches!(err, RunError::Sandbox(_)), "fail-closed must refuse, got {err:?}");
        }
        SandboxStatus::Disabled => {
            panic!("an enabled policy must not resolve to Disabled on linux")
        }
    }
    fx.cleanup();
}
