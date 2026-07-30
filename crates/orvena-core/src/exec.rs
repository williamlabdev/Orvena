//! Shared command execution primitive. Both the `verify` gate and the shell RUN
//! tool run a child process in the project root, capture its stdout/stderr, and
//! bound it with a timeout — the two paths differ only in *who authored the
//! command string*:
//!
//! - **Gate** — `run_shell` runs a human-authored string via `sh -c` (the human
//!   put it in `gates.yaml`, so shell interpretation is acceptable).
//! - **RUN tool** — `run_argv` spawns a fixed argv directly, with **no shell**,
//!   so a model can only reference a pre-declared command by name — never inject
//!   a string.
//!
//! `std::process::Command` has no timeout, so we drive the wait with the
//! `wait-timeout` crate and drain the pipes on background threads (reading the
//! pipes inline while waiting can deadlock if the child fills a pipe buffer).

pub mod sandbox;
#[cfg(target_os = "linux")]
mod sandbox_linux;
#[cfg(target_os = "macos")]
mod sandbox_macos;

pub use sandbox::{Sandbox, SandboxError, SandboxStatus};

use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;
use wait_timeout::ChildExt;

/// Default per-command wall-clock ceiling when none is declared (seconds).
pub const DEFAULT_TIMEOUT_SECS: u64 = 300;

/// The captured result of running a child process. `exit_code` is `None` when the
/// process was killed (including by our timeout) rather than exiting on its own.
#[derive(Debug, Clone)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    /// True when the process was killed because it outran the timeout.
    pub timed_out: bool,
}

impl CommandOutput {
    /// A clean success: ran to completion (not timed out) and exited 0.
    pub fn success(&self) -> bool {
        !self.timed_out && self.exit_code == Some(0)
    }
}

/// Why a command could not be run at all (distinct from running and failing).
#[derive(Debug)]
pub enum RunError {
    /// The child could not be spawned or waited on (e.g. program not found).
    Spawn(std::io::Error),
    /// The OS sandbox refused to run the command (fail-closed) or could not build
    /// its invocation. The command never ran — treated like a spawn failure by
    /// callers, but named distinctly so evidence can say *why*.
    Sandbox(SandboxError),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RunError::Spawn(e) => write!(f, "{e}"),
            RunError::Sandbox(e) => write!(f, "{e}"),
        }
    }
}

/// Runs commands in `cwd` under a shared `timeout`, each child confined by
/// `sandbox` (see [`sandbox`]). `new` leaves the sandbox `Disabled` (backward
/// compatible); `with_sandbox` injects a resolved policy.
pub struct CommandRunner {
    cwd: PathBuf,
    timeout: Duration,
    sandbox: Sandbox,
    env: Vec<(String, String)>,
}

impl CommandRunner {
    /// Unconfined runner (the pre-slice-015 behavior). Existing callers and tests
    /// that do not opt into a sandbox keep spawning children exactly as before.
    pub fn new(cwd: impl Into<PathBuf>, timeout: Duration) -> Self {
        Self { cwd: cwd.into(), timeout, sandbox: Sandbox::disabled(), env: Vec::new() }
    }

    /// Runner whose children are wrapped by `sandbox` (ADR-003).
    pub fn with_sandbox(cwd: impl Into<PathBuf>, timeout: Duration, sandbox: Sandbox) -> Self {
        Self { cwd: cwd.into(), timeout, sandbox, env: Vec::new() }
    }

    /// Add environment variables to every child this runner spawns (on top of the
    /// inherited environment). Used by [`crate::adapter`] to point a wrapped
    /// external agent at its model endpoint and to keep its scratch files out of
    /// the workdir; the gate and RUN paths do not set any.
    pub fn with_env(mut self, env: Vec<(String, String)>) -> Self {
        self.env = env;
        self
    }

    /// Run a fixed argv directly, with no shell interpretation. `argv[0]` is the
    /// program; the rest are literal arguments.
    pub fn run_argv(&self, argv: &[String]) -> Result<CommandOutput, RunError> {
        if argv.is_empty() {
            // Config validation rejects empty argv before we get here; guard anyway.
            return Err(RunError::Spawn(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "empty argv",
            )));
        }
        self.spawn_and_wait(argv.to_vec())
    }

    /// Run a human-authored command string via `sh -c` (the gate path).
    pub fn run_shell(&self, cmd_str: &str) -> Result<CommandOutput, RunError> {
        self.spawn_and_wait(vec!["sh".to_string(), "-c".to_string(), cmd_str.to_string()])
    }

    /// Prepend the sandbox argv prefix (if any) to the base command, then spawn.
    /// On macOS the prefix is `sandbox-exec -p <profile>`, which execs the target
    /// argv directly — so the base command's argv is passed through literally
    /// (no shell), preserving the RUN tool's injection-free property.
    fn spawn_and_wait(&self, base_argv: Vec<String>) -> Result<CommandOutput, RunError> {
        let prefix = self.sandbox.argv_prefix().map_err(RunError::Sandbox)?;
        let mut argv = prefix;
        argv.extend(base_argv);
        let (program, rest) = argv.split_first().ok_or_else(|| {
            RunError::Spawn(std::io::Error::new(std::io::ErrorKind::InvalidInput, "empty argv"))
        })?;
        let mut cmd = Command::new(program);
        cmd.args(rest);
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        cmd.current_dir(&self.cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(RunError::Spawn)?;

        // Drain both pipes on threads so a chatty child cannot deadlock the wait.
        let mut out_pipe = child.stdout.take().expect("stdout piped");
        let mut err_pipe = child.stderr.take().expect("stderr piped");
        let out_handle = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = out_pipe.read_to_end(&mut buf);
            buf
        });
        let err_handle = std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = err_pipe.read_to_end(&mut buf);
            buf
        });

        let (exit_code, timed_out) =
            match child.wait_timeout(self.timeout).map_err(RunError::Spawn)? {
                Some(status) => (status.code(), false),
                None => {
                    // Outran the timeout: kill and reap so the drain threads can finish.
                    let _ = child.kill();
                    let _ = child.wait();
                    (None, true)
                }
            };

        let stdout = out_handle.join().unwrap_or_default();
        let stderr = err_handle.join().unwrap_or_default();
        Ok(CommandOutput {
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
            exit_code,
            timed_out,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn argv_runs_with_fixed_args_and_captures_output() {
        let runner = CommandRunner::new(std::env::temp_dir(), Duration::from_secs(30));
        let out = runner.run_argv(&s(&["echo", "hello"])).unwrap();
        assert!(out.success());
        assert_eq!(out.stdout.trim(), "hello");
        assert!(!out.timed_out);
    }

    #[test]
    fn argv_is_not_shell_interpreted() {
        // If this went through a shell, `$HOME` would expand and `;` would chain.
        let runner = CommandRunner::new(std::env::temp_dir(), Duration::from_secs(30));
        let out = runner.run_argv(&s(&["echo", "$HOME; rm"])).unwrap();
        assert_eq!(out.stdout.trim(), "$HOME; rm", "argv must be passed literally");
    }

    #[test]
    fn nonzero_exit_is_captured_not_an_error() {
        let runner = CommandRunner::new(std::env::temp_dir(), Duration::from_secs(30));
        let out = runner.run_argv(&s(&["sh", "-c", "exit 3"])).unwrap();
        assert!(!out.success());
        assert_eq!(out.exit_code, Some(3));
    }

    #[test]
    fn timeout_kills_and_reports_timed_out() {
        let runner = CommandRunner::new(std::env::temp_dir(), Duration::from_millis(200));
        let out = runner.run_shell("sleep 5").unwrap();
        assert!(out.timed_out, "the runner must kill a command that outruns its timeout");
        assert!(!out.success());
        assert_eq!(out.exit_code, None);
    }

    #[test]
    fn missing_program_is_a_spawn_error() {
        let runner = CommandRunner::new(std::env::temp_dir(), Duration::from_secs(30));
        let err = runner.run_argv(&s(&["orvena-no-such-binary-xyz"])).unwrap_err();
        assert!(matches!(err, RunError::Spawn(_)));
    }
}
