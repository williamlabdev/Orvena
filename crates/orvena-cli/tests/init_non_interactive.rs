//! `orvena init` must be safe to run unattended.
//!
//! Two things are pinned here:
//!
//!   - **`--provider` sets the provider outright and never prompts**, so scripts
//!     do not have to depend on interactivity detection at all — and a bad
//!     `--provider` is an error rather than a silent downgrade to the scaffold
//!     default, the same standard the config parser holds.
//!   - **A process that cannot safely read its terminal falls back to printing
//!     next steps instead of blocking.** This is the regression that motivated
//!     the change: `stdin().is_terminal()` says "interactive" for a
//!     *backgrounded* process, whose first read on the tty earns SIGTTIN and
//!     stops it with no error, no exit, and no output.
//!
//! These drive the built binary (`CARGO_BIN_EXE_orvena`), so they exercise the
//! real argument → init wiring.

use std::path::PathBuf;
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_orvena");

fn temp_dir(tag: &str) -> PathBuf {
    let pid = std::process::id();
    let dir = std::env::temp_dir().join(format!("orvena-init-{tag}-{pid}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn provider_flag_sets_the_provider_without_prompting() {
    let dir = temp_dir("flag");
    let out = Command::new(BIN)
        .args(["init", "--provider", "ollama", "--model", "qwen3:14b"])
        .current_dir(&dir)
        .output()
        .expect("init runs");
    assert!(out.status.success(), "stderr: {}", String::from_utf8_lossy(&out.stderr));

    let cfg = std::fs::read_to_string(dir.join(".orvena/orvena.yaml")).unwrap();
    assert!(cfg.contains("kind: ollama"), "provider not written:\n{cfg}");
    assert!(cfg.contains("qwen3:14b"), "model not written:\n{cfg}");
}

#[test]
fn an_unknown_provider_is_an_error_not_a_silent_default() {
    let dir = temp_dir("unknown");
    let out = Command::new(BIN)
        .args(["init", "--provider", "gemeni"]) // typo on purpose
        .current_dir(&dir)
        .output()
        .expect("init runs");
    assert!(!out.status.success(), "a typo'd provider must not succeed");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("unknown provider"), "unhelpful error: {err}");
    assert!(err.contains("openai_compat"), "error should list known kinds: {err}");

    // The scaffold still lands, but it must not have been given the bad kind.
    let cfg = std::fs::read_to_string(dir.join(".orvena/orvena.yaml")).unwrap();
    assert!(!cfg.contains("gemeni"), "bad kind leaked into config:\n{cfg}");
}

#[test]
fn openai_compat_without_a_base_url_is_refused_up_front() {
    let dir = temp_dir("nobaseurl");
    let out = Command::new(BIN)
        .args(["init", "--provider", "openai_compat"])
        .current_dir(&dir)
        .output()
        .expect("init runs");
    assert!(!out.status.success(), "openai_compat has no default endpoint to fall back to");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("--base-url"), "error should name the missing flag: {err}");
}

/// The regression test for the hang.
///
/// We hand `init` a **real terminal** on stdin that it is not the foreground of,
/// which is what a backgrounded process sees. Before the fix, `is_terminal()`
/// reported "interactive", init entered the provider picker, and the read
/// blocked forever (as SIGTTIN when a foreground group exists to conflict with,
/// and as a plain indefinite block otherwise) — either way the process never
/// returned. It must now detect that it cannot prompt and exit.
///
/// This does not reproduce the exact SIGTTIN signalling, which needs a session
/// leader and a controlling-terminal handoff; it reproduces the property that
/// matters — a tty on stdin that this process may not read to completion.
#[cfg(unix)]
#[test]
fn a_terminal_we_do_not_control_does_not_hang_init() {
    use std::os::unix::io::FromRawFd;
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    use std::time::{Duration, Instant};

    let dir = temp_dir("bgtty");

    // SAFETY: standard posix_openpt handshake; every fd is checked before use.
    let (master, slave) = unsafe {
        let master = libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY);
        assert!(master >= 0, "posix_openpt failed");
        assert_eq!(libc::grantpt(master), 0, "grantpt failed");
        assert_eq!(libc::unlockpt(master), 0, "unlockpt failed");
        let name = libc::ptsname(master);
        assert!(!name.is_null(), "ptsname failed");
        let slave = libc::open(name, libc::O_RDWR | libc::O_NOCTTY);
        assert!(slave >= 0, "opening the pty slave failed");
        (master, slave)
    };

    let mut cmd = Command::new(BIN);
    cmd.arg("init")
        .current_dir(&dir)
        // SAFETY: `slave` is a valid fd we own; Stdio takes ownership of it.
        .stdin(unsafe { Stdio::from_raw_fd(slave) })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Put the child in its own process group: it now has a terminal on stdin
    // that it is not the foreground of — the backgrounded case.
    unsafe {
        cmd.pre_exec(|| match libc::setpgid(0, 0) {
            0 => Ok(()),
            _ => Err(std::io::Error::last_os_error()),
        });
    }
    let mut child = cmd.spawn().expect("init spawns");

    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        match child.try_wait().expect("try_wait") {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                unsafe { libc::close(master) };
                panic!(
                    "init hung on a terminal it does not control — it must fall back to \
                     printing next steps instead of prompting"
                );
            }
            None => std::thread::sleep(Duration::from_millis(100)),
        }
    };
    unsafe { libc::close(master) };

    assert!(status.success(), "init should exit cleanly, got {status:?}");
    assert!(dir.join(".orvena/orvena.yaml").exists(), "the scaffold should still land");
}
