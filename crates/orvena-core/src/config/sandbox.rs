//! OS-level sandbox config (`sandbox:` block in `orvena.yaml`, ADR-003). This is
//! the *policy declaration* the user edits; the runtime translation into a
//! platform-enforced [`SandboxPolicy`] lives in [`crate::exec::sandbox`].
//!
//! `intent` (ADR-001) decides whether the model may *trigger* a command; the
//! sandbox decides what a triggered command can *do*. Two orthogonal layers —
//! this one is the OS-enforced floor under the trust declaration.
//!
//! The struct `Default` is **disabled** — an embedder that constructs `Config`
//! without a sandbox block, or a pre-slice-015 `orvena.yaml`, keeps the previous
//! (unconfined) behavior. Secure-by-default is a *scaffold* decision: `orvena
//! init` writes `enabled: true`, so new projects are confined out of the box.

use crate::error::{Error, Result};
use crate::exec::sandbox::{FsPolicy, NetworkPolicy, OnUnavailable, SandboxPolicy};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The `sandbox:` block. Every field is optional (serde `default`) so a project
/// may declare only what it wants to change.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SandboxConfig {
    /// Master switch. `Default` is `false`; the scaffold sets it `true`.
    #[serde(default)]
    pub enabled: bool,
    /// Whether a confined child may open the network. Defaults to `deny`.
    #[serde(default)]
    pub network: NetworkMode,
    /// Which paths a confined child may write. Defaults to `root_write`.
    #[serde(default)]
    pub filesystem: FsMode,
    /// Writable roots for `filesystem: strict` (relative to the project root).
    /// Ignored under `root_write`.
    #[serde(default)]
    pub strict_writable: Vec<String>,
    /// What to do when the platform sandbox mechanism is unavailable. `None`
    /// derives from the governance tier: engineering → `fail_closed`, light →
    /// `warn` (see [`SandboxConfig::to_policy`]).
    #[serde(default)]
    pub on_unavailable: Option<OnUnavailableMode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkMode {
    #[default]
    Deny,
    Allow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FsMode {
    /// Writable = the whole project-root subtree (+ system temp). Zero friction:
    /// `cargo test`/`npm test` write build artifacts under the root as usual.
    #[default]
    RootWrite,
    /// Writable = only `strict_writable` (+ system temp). The regulated posture.
    Strict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OnUnavailableMode {
    /// Refuse to run rather than run a child unconfined (a run marked enforced
    /// must never silently degrade). The engineering-tier default.
    FailClosed,
    /// Run unconfined, but surface the degradation in evidence + report. The
    /// light-tier default.
    Warn,
}

impl SandboxConfig {
    /// Cheap structural checks at config-load time (surfaced by `doctor`).
    pub fn validate(&self) -> Result<()> {
        if self.enabled && self.filesystem == FsMode::Strict && self.strict_writable.is_empty() {
            return Err(Error::Config(
                "sandbox.filesystem is 'strict' but sandbox.strict_writable is empty — \
                 nothing outside temp would be writable; list the paths the task may write"
                    .into(),
            ));
        }
        Ok(())
    }

    /// Translate the declaration into a runtime [`SandboxPolicy`], or `None` when
    /// the sandbox is disabled (the caller then runs unconfined, as before).
    ///
    /// `root` is the project root; it is canonicalized here so the platform
    /// backend gets an absolute, symlink-resolved subtree. `tier_enforces` is
    /// `Tier::enforces()` — it decides the fail-closed default when the user did
    /// not set `on_unavailable` explicitly.
    pub fn to_policy(&self, root: &Path, tier_enforces: bool) -> Option<SandboxPolicy> {
        if !self.enabled {
            return None;
        }
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let filesystem = match self.filesystem {
            FsMode::RootWrite => FsPolicy::RootWrite,
            FsMode::Strict => FsPolicy::Strict {
                writable: self.strict_writable.iter().map(|p| root.join(p)).collect(),
            },
        };
        let network = match self.network {
            NetworkMode::Deny => NetworkPolicy::Deny,
            NetworkMode::Allow => NetworkPolicy::Allow,
        };
        let on_unavailable = match self.on_unavailable {
            Some(OnUnavailableMode::FailClosed) => OnUnavailable::FailClosed,
            Some(OnUnavailableMode::Warn) => OnUnavailable::Warn,
            None if tier_enforces => OnUnavailable::FailClosed,
            None => OnUnavailable::Warn,
        };
        // System temp is always writable: build/test tools scribble there, and
        // denying it breaks far more than it contains (temp is not exfil-durable).
        let extra_writable: Vec<PathBuf> = {
            let t = std::env::temp_dir();
            vec![t.canonicalize().unwrap_or(t)]
        };
        Some(SandboxPolicy { root, network, filesystem, extra_writable, on_unavailable })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_disabled() {
        let c = SandboxConfig::default();
        assert!(!c.enabled);
        assert_eq!(c.network, NetworkMode::Deny);
        assert_eq!(c.filesystem, FsMode::RootWrite);
        assert!(c.to_policy(Path::new("/tmp"), true).is_none());
    }

    #[test]
    fn strict_without_writable_is_a_config_error() {
        let c = SandboxConfig {
            enabled: true,
            filesystem: FsMode::Strict,
            strict_writable: vec![],
            ..Default::default()
        };
        let err = c.validate().unwrap_err();
        assert!(matches!(err, Error::Config(_)));
        assert!(err.to_string().contains("strict"));
    }

    #[test]
    fn strict_with_writable_validates_and_builds_policy() {
        let c = SandboxConfig {
            enabled: true,
            filesystem: FsMode::Strict,
            strict_writable: vec!["src".into()],
            ..Default::default()
        };
        c.validate().unwrap();
        let policy = c.to_policy(Path::new("/tmp"), false).unwrap();
        assert!(matches!(policy.filesystem, FsPolicy::Strict { .. }));
    }

    #[test]
    fn on_unavailable_derives_from_tier_when_unset() {
        let c = SandboxConfig { enabled: true, ..Default::default() };
        assert_eq!(
            c.to_policy(Path::new("/tmp"), true).unwrap().on_unavailable,
            OnUnavailable::FailClosed,
            "engineering (enforces=true) fails closed"
        );
        assert_eq!(
            c.to_policy(Path::new("/tmp"), false).unwrap().on_unavailable,
            OnUnavailable::Warn,
            "light (enforces=false) warns"
        );
    }

    #[test]
    fn explicit_on_unavailable_overrides_tier() {
        let c = SandboxConfig {
            enabled: true,
            on_unavailable: Some(OnUnavailableMode::Warn),
            ..Default::default()
        };
        assert_eq!(
            c.to_policy(Path::new("/tmp"), true).unwrap().on_unavailable,
            OnUnavailable::Warn,
            "explicit warn beats the engineering-tier fail_closed default"
        );
    }

    #[test]
    fn network_and_fs_defaults_map_through() {
        let c = SandboxConfig { enabled: true, ..Default::default() };
        let p = c.to_policy(Path::new("/tmp"), false).unwrap();
        assert_eq!(p.network, NetworkPolicy::Deny);
        assert!(matches!(p.filesystem, FsPolicy::RootWrite));
        assert!(!p.extra_writable.is_empty(), "system temp is always writable");
    }
}
