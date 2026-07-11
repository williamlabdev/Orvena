//! Linux sandbox backend (ADR-003 / slice-016): a re-exec shim confined by
//! **Landlock** (filesystem) + **seccomp** (network).
//!
//! `argv_prefix` prepends `[<shim>, __sandbox, --spec <json>, --]` to the command
//! (see `sandbox.rs`). The `orvena __sandbox` subcommand — dispatched from `main`
//! **before** the tokio runtime starts, so it runs single-threaded — applies the
//! restrictions to *itself* and then `execvp`s the real command. A re-exec shim
//! is used instead of `Command::pre_exec` because applying Landlock between fork
//! and exec in a multi-threaded process is not async-signal-safe (the crate
//! allocates); a fresh single-threaded process has no such hazard (ADR-003 D-F).
//!
//! Policy (mirrors the macOS profile): read + execute are granted everywhere so
//! the target program and its libraries remain reachable; write is granted only
//! under the writable subtrees plus `/dev` (stdio devices); with `network: deny`,
//! `socket(AF_INET|AF_INET6)` returns `EACCES` (AF_UNIX is left alone).
//!
//! Compile-checked from macOS via `cargo check --target x86_64-unknown-linux-gnu`;
//! **runtime containment is verified on Linux** (CI ubuntu leg +
//! `orvena-cli/tests/sandbox_linux.rs`), never on the macOS host.

use super::sandbox::{NetworkPolicy, OnUnavailable, SandboxError, SandboxPolicy};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The wire contract handed to the `orvena __sandbox` shim: exactly what it needs
/// to enforce, already resolved by the parent. Kept minimal on purpose — the shim
/// never sees the full `SandboxPolicy`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShimSpec {
    /// Absolute subtrees the child may write to (writable set + system temp).
    pub writable: Vec<PathBuf>,
    /// Deny `AF_INET`/`AF_INET6` sockets.
    pub deny_network: bool,
    /// If confinement cannot be applied, refuse to run rather than run unconfined.
    pub fail_closed: bool,
}

impl ShimSpec {
    fn from_policy(policy: &SandboxPolicy) -> Self {
        Self {
            writable: policy.writable_paths(),
            deny_network: policy.network == NetworkPolicy::Deny,
            fail_closed: policy.on_unavailable == OnUnavailable::FailClosed,
        }
    }
}

/// Resolve the executable that dispatches `__sandbox`: an explicit override
/// (`ORVENA_SANDBOX_SHIM`, for embedders/tests) or, normally, the running binary.
fn shim_exe() -> Result<String, SandboxError> {
    if let Some(p) = std::env::var_os("ORVENA_SANDBOX_SHIM") {
        return Ok(p.to_string_lossy().into_owned());
    }
    std::env::current_exe()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| SandboxError::Backend(format!("cannot resolve sandbox shim executable: {e}")))
}

/// `[<shim>, __sandbox, --spec <json>, --]` — the base command's argv is appended
/// after `--` by the caller.
pub fn argv_prefix(policy: &SandboxPolicy) -> Result<Vec<String>, SandboxError> {
    let shim = shim_exe()?;
    let json = serde_json::to_string(&ShimSpec::from_policy(policy))
        .map_err(|e| SandboxError::Backend(format!("cannot serialize sandbox spec: {e}")))?;
    Ok(vec![shim, "__sandbox".into(), "--spec".into(), json, "--".into()])
}

/// Probe Landlock support **without enforcing on the current process**: creating a
/// ruleset fd does not restrict us (only `restrict_self` does, which we never call
/// here). `HardRequirement` makes the probe fail on a kernel lacking Landlock ABI
/// v1, so the caller degrades to fail-closed/warn rather than falsely reporting
/// enforcement.
pub fn available() -> bool {
    use landlock::{Access, AccessFs, CompatLevel, Compatible, Ruleset, RulesetAttr, ABI};
    let abi = ABI::V1;
    Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(AccessFs::from_all(abi))
        .and_then(|r| r.create())
        .is_ok()
}

/// Entry point for the `orvena __sandbox` subcommand. Applies confinement to this
/// (single-threaded) process, then `execvp`s `argv`. Never returns: it either
/// becomes the target command or exits with an error.
pub fn run_shim(spec: &ShimSpec, argv: &[String]) -> ! {
    use std::os::unix::process::CommandExt;

    let confine = || -> Result<(), SandboxError> {
        apply_landlock(&spec.writable)?;
        if spec.deny_network {
            apply_seccomp_deny_inet()?;
        }
        Ok(())
    };

    if let Err(e) = confine() {
        if spec.fail_closed {
            eprintln!("orvena __sandbox: refusing to run the command unconfined: {e}");
            std::process::exit(70);
        }
        eprintln!("orvena __sandbox: WARNING — running the command unconfined: {e}");
    }

    let Some((program, args)) = argv.split_first() else {
        eprintln!("orvena __sandbox: no command to run after `--`");
        std::process::exit(70);
    };
    // execvp replaces this process image; it only returns on failure.
    let err = std::process::Command::new(program).args(args).exec();
    eprintln!("orvena __sandbox: could not exec '{program}': {err}");
    std::process::exit(70);
}

/// Read + execute everywhere; write only under `writable` (+ `/dev`). Best-effort
/// across ABIs, but a kernel that does not enforce at all is surfaced as an error
/// so the caller can fail closed.
fn apply_landlock(writable: &[PathBuf]) -> Result<(), SandboxError> {
    use landlock::{
        path_beneath_rules, Access, AccessFs, CompatLevel, Compatible, Ruleset, RulesetAttr,
        RulesetCreatedAttr, RulesetStatus, ABI,
    };
    let abi = ABI::V1;

    let mut write_roots: Vec<PathBuf> = writable.to_vec();
    // Devices (e.g. /dev/null) must stay writable — denying them breaks stdio for
    // most programs, and they are not durable exfil (mirrors the macOS profile).
    write_roots.push(PathBuf::from("/dev"));
    // `path_beneath_rules` opens each path, so drop any that do not exist.
    write_roots.retain(|p| p.exists());

    let status = Ruleset::default()
        .set_compatibility(CompatLevel::BestEffort)
        .handle_access(AccessFs::from_all(abi))
        .map_err(|e| SandboxError::Backend(format!("landlock handle_access: {e}")))?
        .create()
        .map_err(|e| SandboxError::Backend(format!("landlock create: {e}")))?
        .add_rules(path_beneath_rules(["/"], AccessFs::from_read(abi)))
        .map_err(|e| SandboxError::Backend(format!("landlock read rules: {e}")))?
        .add_rules(path_beneath_rules(&write_roots, AccessFs::from_all(abi)))
        .map_err(|e| SandboxError::Backend(format!("landlock write rules: {e}")))?
        .restrict_self()
        .map_err(|e| SandboxError::Backend(format!("landlock restrict_self: {e}")))?;

    if matches!(status.ruleset, RulesetStatus::NotEnforced) {
        return Err(SandboxError::Refused("Landlock is not enforced by this kernel".into()));
    }
    Ok(())
}

/// Deny `socket(2)` for `AF_INET`/`AF_INET6` (returns `EACCES`); everything else,
/// including `AF_UNIX`, is allowed.
fn apply_seccomp_deny_inet() -> Result<(), SandboxError> {
    use seccompiler::{
        apply_filter, BpfProgram, SeccompAction, SeccompCmpArgLen, SeccompCmpOp, SeccompCondition,
        SeccompFilter, SeccompRule,
    };
    use std::collections::BTreeMap;

    let inet = |domain: u64| -> Result<SeccompRule, SandboxError> {
        SeccompRule::new(vec![SeccompCondition::new(
            0, // arg0 of socket(2) is the address family (domain)
            SeccompCmpArgLen::Dword,
            SeccompCmpOp::Eq,
            domain,
        )
        .map_err(|e| SandboxError::Backend(format!("seccomp condition: {e}")))?])
        .map_err(|e| SandboxError::Backend(format!("seccomp rule: {e}")))
    };

    let mut rules: BTreeMap<i64, Vec<SeccompRule>> = BTreeMap::new();
    rules.insert(libc::SYS_socket, vec![inet(libc::AF_INET as u64)?, inet(libc::AF_INET6 as u64)?]);

    let filter = SeccompFilter::new(
        rules,
        SeccompAction::Allow,                      // default: everything else allowed
        SeccompAction::Errno(libc::EACCES as u32), // matched inet socket() → EACCES
        std::env::consts::ARCH
            .try_into()
            .map_err(|_| SandboxError::Backend("seccomp: unsupported target arch".into()))?,
    )
    .map_err(|e| SandboxError::Backend(format!("seccomp filter: {e}")))?;

    let program: BpfProgram =
        filter.try_into().map_err(|e| SandboxError::Backend(format!("seccomp compile: {e}")))?;
    apply_filter(&program).map_err(|e| SandboxError::Backend(format!("seccomp apply: {e}")))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shim_spec_round_trips() {
        let spec = ShimSpec {
            writable: vec![PathBuf::from("/proj"), PathBuf::from("/tmp")],
            deny_network: true,
            fail_closed: true,
        };
        let json = serde_json::to_string(&spec).unwrap();
        assert_eq!(serde_json::from_str::<ShimSpec>(&json).unwrap(), spec);
    }

    #[test]
    fn argv_prefix_shape_and_shim_override() {
        // SAFETY: single-threaded test; env is set/removed within this test only.
        std::env::set_var("ORVENA_SANDBOX_SHIM", "/usr/bin/orvena-test");
        let policy = SandboxPolicy {
            root: PathBuf::from("/proj"),
            network: NetworkPolicy::Deny,
            filesystem: super::super::sandbox::FsPolicy::RootWrite,
            extra_writable: vec![PathBuf::from("/tmp")],
            on_unavailable: OnUnavailable::FailClosed,
        };
        let pre = argv_prefix(&policy).unwrap();
        std::env::remove_var("ORVENA_SANDBOX_SHIM");
        assert_eq!(pre[0], "/usr/bin/orvena-test");
        assert_eq!(pre[1], "__sandbox");
        assert_eq!(pre[2], "--spec");
        assert_eq!(pre[4], "--");
        let spec: ShimSpec = serde_json::from_str(&pre[3]).unwrap();
        assert!(spec.deny_network && spec.fail_closed);
        assert!(spec.writable.contains(&PathBuf::from("/proj")));
    }
}
