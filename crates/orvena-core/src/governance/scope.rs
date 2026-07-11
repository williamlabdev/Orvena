//! Scope lock + read-only default. Anything not in `allowed_modifications` is
//! read-only; anything in `excluded` is off-limits entirely. Paths are relative
//! to the project root; an entry matches a path exactly or as a directory prefix.

use crate::config::agent::Tier;

#[derive(Debug, Clone)]
pub struct Scope {
    /// Relative paths the task may write (directories match by prefix).
    pub allowed_modifications: Vec<String>,
    /// Relative paths explicitly off-limits.
    pub excluded: Vec<String>,
    pub tier: Tier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeDecision {
    /// Writable — listed in `allowed_modifications`.
    Allow,
    /// Read-only by default (not listed). A write here is a blocker.
    ReadOnly,
    /// Explicitly excluded from the task.
    Excluded,
}

impl Scope {
    pub fn new(allowed_modifications: Vec<String>, excluded: Vec<String>, tier: Tier) -> Self {
        Self { allowed_modifications, excluded, tier }
    }

    /// Decide whether a relative path may be written.
    pub fn decision(&self, rel: &str) -> ScopeDecision {
        let rel = normalize(rel);
        // A path that climbs out of the root (`..`) is never writable, no matter
        // how its prefix matches the allow-list. The fs tool also hard-rejects
        // these at the write boundary; this keeps `decision` honest on its own so
        // an escaping path is never reported as `Allow`.
        if rel.split('/').any(|seg| seg == "..") {
            return ScopeDecision::ReadOnly;
        }
        if self.excluded.iter().any(|e| path_matches(&rel, e)) {
            return ScopeDecision::Excluded;
        }
        if self.allowed_modifications.iter().any(|a| path_matches(&rel, a)) {
            return ScopeDecision::Allow;
        }
        ScopeDecision::ReadOnly
    }
}

fn normalize(rel: &str) -> String {
    rel.trim_start_matches("./").replace('\\', "/")
}

/// `rel` matches `entry` if equal, or `rel` is under directory `entry`.
fn path_matches(rel: &str, entry: &str) -> bool {
    let entry = normalize(entry);
    rel == entry || rel.starts_with(&format!("{entry}/"))
}
