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
    /// Benchmark-only ungoverned baseline (D2): every in-root path is writable,
    /// regardless of the lists above. `allowed_modifications` is still carried
    /// so the prompt is identical to a governed run — only enforcement differs.
    /// Root escape (`..`, symlinks) is host protection, not governance, and is
    /// NOT lifted. Unreachable from the CLI/config surface.
    pub(crate) unrestricted: bool,
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
        Self { allowed_modifications, excluded, tier, unrestricted: false }
    }

    /// Benchmark-only ungoverned baseline (D2): same lists (same prompt), no
    /// enforcement. Crate-private so no product path can construct it.
    pub(crate) fn unrestricted_baseline(
        allowed_modifications: Vec<String>,
        tier: Tier,
    ) -> Self {
        Self { allowed_modifications, excluded: Vec::new(), tier, unrestricted: true }
    }

    /// Decide whether a relative path may be written.
    pub fn decision(&self, rel: &str) -> ScopeDecision {
        let rel = normalize(rel);
        // A path that climbs out of the root (`..`) is never writable, no matter
        // how its prefix matches the allow-list. The fs tool also hard-rejects
        // these at the write boundary; this keeps `decision` honest on its own so
        // an escaping path is never reported as `Allow`. This holds even for the
        // unrestricted baseline: root escape is host protection, not governance.
        if rel.split('/').any(|seg| seg == "..") {
            return ScopeDecision::ReadOnly;
        }
        if self.unrestricted {
            return ScopeDecision::Allow;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unrestricted_baseline_allows_unlisted_paths() {
        let s = Scope::unrestricted_baseline(vec!["src/lib.rs".into()], Tier::Light);
        assert_eq!(s.decision("src/lib.rs"), ScopeDecision::Allow);
        assert_eq!(s.decision("tests/it.rs"), ScopeDecision::Allow, "baseline lifts the lists");
        assert_eq!(s.decision("Cargo.toml"), ScopeDecision::Allow);
    }

    #[test]
    fn unrestricted_baseline_still_blocks_root_escape() {
        // Host protection is not governance: `..` never becomes writable, even
        // in the bench-only ungoverned baseline.
        let s = Scope::unrestricted_baseline(vec![], Tier::Light);
        assert_eq!(s.decision("../outside.txt"), ScopeDecision::ReadOnly);
        assert_eq!(s.decision("a/../../outside.txt"), ScopeDecision::ReadOnly);
    }

    #[test]
    fn governed_scope_is_unchanged_by_the_new_field() {
        let s = Scope::new(vec!["src".into()], vec!["src/gen".into()], Tier::Engineering);
        assert_eq!(s.decision("src/lib.rs"), ScopeDecision::Allow);
        assert_eq!(s.decision("src/gen/x.rs"), ScopeDecision::Excluded);
        assert_eq!(s.decision("README.md"), ScopeDecision::ReadOnly);
    }
}
