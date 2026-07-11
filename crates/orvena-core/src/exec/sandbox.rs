//! OS-level sandbox for spawned children (ADR-003). Every child that
//! [`crate::exec::CommandRunner`] spawns — a `<<<RUN>>>` command or a gate's
//! `verify` — is wrapped in a least-privilege OS sandbox so a command that
//! *claims* `read_only` but tries to write outside the project root or open the
//! network is stopped by the OS, not merely trusted.
//!
//! The施力點 is the single spawn choke-point: `CommandRunner` prepends a
//! platform *argv prefix* to the base command. On macOS that prefix is
//! `sandbox-exec -p <profile>`; both platforms `exec` the target argv directly,
//! so the RUN tool's "no shell interpretation" property (ADR-001) is preserved.
//!
//! Backend status by platform in this slice:
//! - **macOS** — fully implemented via `sandbox-exec` + a subtractive SBPL
//!   profile ("allow default, then deny writes outside root, deny network").
//! - **Linux / other** — reported *unavailable*; behavior then follows the
//!   policy's `on_unavailable` (fail-closed under engineering, warn under light).
//!   The Landlock+seccomp backend is a focused follow-up (see `sandbox_linux`).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Whether a confined child may touch the network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkPolicy {
    Deny,
    Allow,
}

/// Which subtrees a confined child may write to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FsPolicy {
    /// Writable = the whole project-root subtree (+ `extra_writable`).
    RootWrite,
    /// Writable = exactly these paths (+ `extra_writable`).
    Strict { writable: Vec<PathBuf> },
}

/// What to do when no platform sandbox mechanism is available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnUnavailable {
    /// Refuse to spawn (a run marked enforced must never silently degrade).
    FailClosed,
    /// Spawn unconfined, but surface the degradation.
    Warn,
}

/// The fully-resolved, runtime sandbox policy (built from `SandboxConfig`).
/// Paths here are absolute and canonicalized.
#[derive(Debug, Clone)]
pub struct SandboxPolicy {
    /// Canonicalized project root — the writable subtree under `RootWrite`.
    pub root: PathBuf,
    pub network: NetworkPolicy,
    pub filesystem: FsPolicy,
    /// Always-writable extras (e.g. system temp).
    pub extra_writable: Vec<PathBuf>,
    pub on_unavailable: OnUnavailable,
}

impl SandboxPolicy {
    /// Every subtree the child may write to (writable set + extras).
    pub fn writable_paths(&self) -> Vec<PathBuf> {
        let mut v = match &self.filesystem {
            FsPolicy::RootWrite => vec![self.root.clone()],
            FsPolicy::Strict { writable } => writable.clone(),
        };
        v.extend(self.extra_writable.iter().cloned());
        v
    }
}

/// A record of whether this run's children were actually confined — written into
/// the evidence bundle so an auditor can tell enforcement from intention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxStatus {
    /// Children ran inside an OS sandbox.
    Enforced,
    /// No sandbox was applied — either not enabled, or unavailable under `warn`.
    #[default]
    Disabled,
    /// The sandbox was required but unavailable, and the policy fails closed —
    /// commands were refused rather than run unconfined.
    Unavailable,
}

/// Why a sandbox could not wrap a command.
#[derive(Debug, Clone)]
pub enum SandboxError {
    /// Fail-closed: the sandbox is unavailable and the policy refuses to run
    /// unconfined.
    Refused(String),
    /// The sandbox invocation could not be built (e.g. profile generation).
    Backend(String),
}

impl std::fmt::Display for SandboxError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SandboxError::Refused(m) => write!(f, "sandbox unavailable, refused to run unconfined: {m}"),
            SandboxError::Backend(m) => write!(f, "sandbox backend error: {m}"),
        }
    }
}

/// A resolved sandbox, ready to wrap commands. Cheap to clone (a policy struct).
#[derive(Debug, Clone)]
pub struct Sandbox {
    mode: Mode,
}

#[derive(Debug, Clone)]
enum Mode {
    /// No confinement (disabled, or unavailable-but-warned → ran unconfined).
    Disabled,
    /// Wrap every command with the platform backend.
    Confined(SandboxPolicy),
    /// Backend unavailable. `fail_closed` decides refuse-to-spawn vs run-and-warn.
    Unavailable { reason: String, fail_closed: bool },
}

impl Sandbox {
    /// No sandbox — the runner spawns children exactly as before.
    pub fn disabled() -> Self {
        Self { mode: Mode::Disabled }
    }

    /// Resolve a policy against this host: probe the platform backend and pick
    /// confined / warn / fail-closed accordingly.
    pub fn for_policy(policy: SandboxPolicy) -> Self {
        match backend_availability(&policy) {
            Availability::Available => Self { mode: Mode::Confined(policy) },
            Availability::Unavailable(reason) => Self {
                mode: Mode::Unavailable {
                    reason,
                    fail_closed: policy.on_unavailable == OnUnavailable::FailClosed,
                },
            },
        }
    }

    /// The argv to prepend to the base command before spawning. Empty when there
    /// is nothing to wrap (disabled, or unavailable-but-warned). `Err` when the
    /// policy fails closed on an unavailable backend, or the backend cannot build
    /// its invocation.
    pub fn argv_prefix(&self) -> Result<Vec<String>, SandboxError> {
        match &self.mode {
            Mode::Disabled => Ok(Vec::new()),
            Mode::Confined(policy) => backend_argv_prefix(policy),
            Mode::Unavailable { fail_closed: false, .. } => Ok(Vec::new()),
            Mode::Unavailable { reason, fail_closed: true } => {
                Err(SandboxError::Refused(reason.clone()))
            }
        }
    }

    /// Auditable status for the run report.
    pub fn status(&self) -> SandboxStatus {
        match &self.mode {
            Mode::Confined(_) => SandboxStatus::Enforced,
            Mode::Disabled => SandboxStatus::Disabled,
            Mode::Unavailable { fail_closed: false, .. } => SandboxStatus::Disabled,
            Mode::Unavailable { fail_closed: true, .. } => SandboxStatus::Unavailable,
        }
    }

    /// A human-readable degradation notice, when the sandbox is not enforcing as
    /// requested (surfaced into the report so it is never silent). `None` when
    /// enforced or cleanly disabled.
    pub fn warning(&self) -> Option<String> {
        match &self.mode {
            Mode::Unavailable { reason, fail_closed: false } => {
                Some(format!("sandbox unavailable — ran children unconfined: {reason}"))
            }
            Mode::Unavailable { reason, fail_closed: true } => {
                Some(format!("sandbox unavailable — refused to run children unconfined: {reason}"))
            }
            _ => None,
        }
    }
}

enum Availability {
    Available,
    Unavailable(String),
}

/// Platform probe: is a real confinement backend usable on this host for this
/// policy? macOS needs `sandbox-exec`; everywhere else is unavailable until the
/// Landlock backend lands.
fn backend_availability(_policy: &SandboxPolicy) -> Availability {
    #[cfg(target_os = "macos")]
    {
        if super::sandbox_macos::available() {
            Availability::Available
        } else {
            Availability::Unavailable("`sandbox-exec` not found on this macOS host".into())
        }
    }
    #[cfg(target_os = "linux")]
    {
        if super::sandbox_linux::available() {
            Availability::Available
        } else {
            Availability::Unavailable(
                "Landlock is unavailable on this kernel (needs 5.13+ with Landlock enabled)".into(),
            )
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Availability::Unavailable("no OS sandbox backend for this platform".into())
    }
}

/// Build the argv prefix for a confined policy on this platform.
#[allow(unused_variables)]
fn backend_argv_prefix(policy: &SandboxPolicy) -> Result<Vec<String>, SandboxError> {
    #[cfg(target_os = "macos")]
    {
        Ok(super::sandbox_macos::argv_prefix(policy))
    }
    #[cfg(target_os = "linux")]
    {
        super::sandbox_linux::argv_prefix(policy)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        // `Confined` is only ever constructed when a backend is available, so
        // this is unreachable in practice. Fail closed rather than silently run
        // unconfined if that invariant ever changes.
        Err(SandboxError::Backend(
            "confined mode has no argv backend on this platform".into(),
        ))
    }
}

/// Entry point for the hidden `orvena __sandbox` subcommand (dispatched from the
/// CLI's `main` before the tokio runtime starts, so it is single-threaded). Parse
/// the spec JSON and hand off to the Linux re-exec shim, which applies Landlock +
/// seccomp and then `execvp`s the wrapped command. Never returns. On a platform
/// with no re-exec backend it fails closed rather than run the command unconfined.
pub fn run_linux_shim(spec_json: &str, argv: &[String]) -> ! {
    #[cfg(target_os = "linux")]
    {
        match serde_json::from_str::<super::sandbox_linux::ShimSpec>(spec_json) {
            Ok(spec) => super::sandbox_linux::run_shim(&spec, argv),
            Err(e) => {
                eprintln!("orvena __sandbox: invalid --spec: {e}");
                std::process::exit(70);
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (spec_json, argv);
        eprintln!("orvena __sandbox: no re-exec sandbox backend on this platform");
        std::process::exit(70);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn policy(on_unavailable: OnUnavailable) -> SandboxPolicy {
        SandboxPolicy {
            root: PathBuf::from("/tmp/orvena-root"),
            network: NetworkPolicy::Deny,
            filesystem: FsPolicy::RootWrite,
            extra_writable: vec![PathBuf::from("/tmp")],
            on_unavailable,
        }
    }

    #[test]
    fn disabled_has_empty_prefix_and_disabled_status() {
        let s = Sandbox::disabled();
        assert!(s.argv_prefix().unwrap().is_empty());
        assert_eq!(s.status(), SandboxStatus::Disabled);
        assert!(s.warning().is_none());
    }

    #[test]
    fn writable_paths_include_root_and_extras() {
        let p = policy(OnUnavailable::Warn);
        let w = p.writable_paths();
        assert!(w.contains(&PathBuf::from("/tmp/orvena-root")));
        assert!(w.contains(&PathBuf::from("/tmp")));
    }

    #[test]
    fn strict_writable_paths_exclude_root() {
        let p = SandboxPolicy {
            filesystem: FsPolicy::Strict { writable: vec![PathBuf::from("/tmp/orvena-root/src")] },
            ..policy(OnUnavailable::Warn)
        };
        let w = p.writable_paths();
        assert!(w.contains(&PathBuf::from("/tmp/orvena-root/src")));
        assert!(!w.contains(&PathBuf::from("/tmp/orvena-root")), "strict does not grant the whole root");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_confined_prefix_is_sandbox_exec() {
        let s = Sandbox::for_policy(policy(OnUnavailable::FailClosed));
        assert_eq!(s.status(), SandboxStatus::Enforced);
        let prefix = s.argv_prefix().unwrap();
        assert_eq!(prefix.first().map(String::as_str), Some("sandbox-exec"));
        assert_eq!(prefix.get(1).map(String::as_str), Some("-p"));
        assert!(prefix.get(2).unwrap().contains("(version 1)"), "third arg is the SBPL profile");
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    #[test]
    fn unsupported_platform_fail_closed_refuses() {
        let s = Sandbox::for_policy(policy(OnUnavailable::FailClosed));
        assert_eq!(s.status(), SandboxStatus::Unavailable);
        assert!(matches!(s.argv_prefix(), Err(SandboxError::Refused(_))));
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    #[test]
    fn no_backend_warn_runs_unconfined() {
        // On a host without a backend, `warn` degrades to running unconfined with
        // an empty prefix and a surfaced warning — never a silent enforced claim.
        // (Linux has a backend, so `warn` there may resolve to Confined instead.)
        let s = Sandbox::for_policy(policy(OnUnavailable::Warn));
        assert_eq!(s.status(), SandboxStatus::Disabled);
        assert!(s.argv_prefix().unwrap().is_empty());
        assert!(s.warning().is_some());
    }

    #[test]
    fn error_display_is_descriptive() {
        let e = SandboxError::Refused("no backend".into());
        assert!(e.to_string().contains("refused to run unconfined"));
    }

    // Keep `Path` import used on all platforms.
    #[test]
    fn policy_root_is_a_path() {
        let p = policy(OnUnavailable::Warn);
        assert!(p.root.starts_with(Path::new("/tmp")));
    }
}
