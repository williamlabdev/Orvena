//! Error types. Hard failures surface as [`Error`]; recoverable "can't proceed
//! within scope" situations surface as a [`crate::governance::Blocker`] recorded
//! on the run report (with a recovery hint), per the Evidence & Done pillar.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("config error: {0}")]
    Config(String),

    #[error("provider error: {0}")]
    Provider(String),

    /// A write or tool use that the scope lock / role boundary forbids.
    #[error("scope violation: {0}")]
    Scope(String),

    /// The tier-independent project-root guard refused a path: an absolute
    /// path or any `..` component (`fs.rs` `resolve_in_root`, `grep.rs`
    /// search path). Behaves exactly like [`Error::Scope`] everywhere — same
    /// message, same handling (use [`Error::is_scope`]); it is a separate
    /// variant only so the benchmark can count guard refusals where they fire
    /// (#47) without matching on message text. The symlink check ("resolves
    /// outside the project root") stays a plain `Scope`.
    #[error("scope violation: {0}")]
    HostGuard(String),

    /// A gate could not be evaluated (not the same as a gate failing).
    #[error("gate error: {0}")]
    Gate(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("yaml error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl Error {
    /// A scope/role refusal, including the host guard's (which is a `Scope`
    /// refusal that additionally knows which check fired).
    pub fn is_scope(&self) -> bool {
        matches!(self, Error::Scope(_) | Error::HostGuard(_))
    }

    /// The tier-independent lexical root guard fired (#47).
    pub fn is_host_guard(&self) -> bool {
        matches!(self, Error::HostGuard(_))
    }
}

pub type Result<T> = std::result::Result<T, Error>;
