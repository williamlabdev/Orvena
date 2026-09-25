//! macOS sandbox backend: `sandbox-exec` + a generated SBPL profile (ADR-003).
//!
//! The profile is **subtractive**, not deny-default. A full `(deny default)`
//! SBPL profile is famously brittle — most programs need dozens of mach-lookup
//! and sysctl allowances just to start. Instead we `(allow default)` and then
//! *subtract* the two capabilities we care about:
//!
//! ```text
//! (version 1)
//! (allow default)
//! (deny file-write* (subpath "/"))        ; nothing writes outside root…
//! (allow file-write* (subpath "/dev"))    ; …except devices (/dev/null, ttys)
//! (allow file-write* (subpath "<root>"))  ; …and the project root
//! (allow file-write* (subpath "<tmp>"))   ; …and system temp
//! (deny network*)                          ; when network: deny
//! ```
//!
//! SBPL is last-match-wins, so a write to `/Users/foo` (outside root) matches the
//! broad `(deny … (subpath "/"))` and is refused, while a write under the root
//! matches the later `(allow …)` and succeeds. `/dev` is re-allowed because
//! denying `/dev/null` breaks almost everything and it is not durable exfil.
//!
//! `sandbox-exec` execs the target argv directly (no shell), so the RUN tool's
//! "no shell interpretation" property (ADR-001) survives the wrap.

use super::sandbox::NetworkPolicy;
use super::sandbox::SandboxPolicy;
use std::path::Path;

const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

/// Is `sandbox-exec` present on this host?
pub fn available() -> bool {
    Path::new(SANDBOX_EXEC).exists()
}

/// The argv prefix: `["sandbox-exec", "-p", <profile>]`.
pub fn argv_prefix(policy: &SandboxPolicy) -> Vec<String> {
    vec!["sandbox-exec".to_string(), "-p".to_string(), build_profile(policy)]
}

/// Generate the SBPL profile string for a policy.
pub fn build_profile(policy: &SandboxPolicy) -> String {
    let mut p = String::from("(version 1)\n(allow default)\n");
    // Subtract all out-of-root writes, then re-grant the allowed subtrees.
    p.push_str("(deny file-write* (subpath \"/\"))\n");
    // Devices are not durable storage; denying /dev breaks stdio redirection.
    p.push_str("(allow file-write* (subpath \"/dev\"))\n");
    for w in policy.writable_paths() {
        // Skip a degenerate "/" writable (would undo the whole subtraction).
        let s = w.to_string_lossy();
        if s == "/" || s.is_empty() {
            continue;
        }
        p.push_str(&format!("(allow file-write* (subpath {}))\n", sbpl_string(&s)));
    }
    // Pattern-matched writes (issue #40): a literal `subpath`/`literal` cannot
    // express a path whose middle segment is only known at agent-runtime (a
    // random per-session suffix). SBPL's `regex` predicate can, so these ride
    // alongside the literal allows above rather than widening any of them.
    for pat in &policy.extra_writable_patterns {
        p.push_str(&format!("(allow file-write* (regex {}))\n", sbpl_regex_string(pat)));
    }
    if policy.network == NetworkPolicy::Deny {
        p.push_str("(deny network*)\n");
    }
    p
}

/// Quote a path as an SBPL string literal: wrap in double quotes and escape `\`
/// and `"`. SBPL `subpath` needs an absolute, quoted path.
fn sbpl_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        if c == '\\' || c == '"' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// Quote a POSIX-ERE source string as an SBPL `regex` literal: `#"pattern"`.
/// Same escaping as [`sbpl_string`] — the payload is still a double-quoted
/// string, just parsed as a regex by the sandbox rather than compared
/// literally — so `\` and `"` in the pattern source need the same backslash
/// escaping and everything else (including regex metacharacters) passes
/// through untouched.
fn sbpl_regex_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 3);
    out.push_str("#\"");
    for c in s.chars() {
        if c == '\\' || c == '"' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::super::sandbox::{FsPolicy, OnUnavailable, SandboxBackend};
    use super::*;
    use std::path::PathBuf;

    fn policy(network: NetworkPolicy) -> SandboxPolicy {
        SandboxPolicy {
            root: PathBuf::from("/tmp/orvena-root"),
            network,
            filesystem: FsPolicy::RootWrite,
            extra_writable: vec![PathBuf::from("/private/tmp")],
            extra_writable_patterns: vec![],
            on_unavailable: OnUnavailable::FailClosed,
            backend: SandboxBackend::Seatbelt,
        }
    }

    #[test]
    fn profile_denies_root_then_reallows_subtrees() {
        let prof = build_profile(&policy(NetworkPolicy::Deny));
        assert!(prof.contains("(allow default)"));
        assert!(prof.contains("(deny file-write* (subpath \"/\"))"));
        assert!(prof.contains("(allow file-write* (subpath \"/tmp/orvena-root\"))"));
        assert!(prof.contains("(allow file-write* (subpath \"/private/tmp\"))"));
        assert!(prof.contains("(allow file-write* (subpath \"/dev\"))"));
        // Ordering matters: the broad deny must precede the root allow (last wins).
        let deny_at = prof.find("(deny file-write* (subpath \"/\"))").unwrap();
        let allow_at = prof.find("(allow file-write* (subpath \"/tmp/orvena-root\"))").unwrap();
        assert!(deny_at < allow_at, "deny-root must come before allow-root");
    }

    /// Issue #40: Claude Code's Bash tool tracks its subprocess cwd in
    /// `/tmp/claude-<random-hex>-cwd` — a filename whose middle segment
    /// changes per session, so it cannot be granted as a literal `subpath`.
    /// The generated profile must carry a `regex` allow for it instead.
    #[test]
    fn extra_writable_patterns_become_regex_allows() {
        let p = SandboxPolicy {
            extra_writable_patterns: vec![
                r"^/tmp/claude-[0-9a-f]+-cwd$".to_string(),
                r"^/private/tmp/claude-[0-9a-f]+-cwd$".to_string(),
            ],
            ..policy(NetworkPolicy::Deny)
        };
        let prof = build_profile(&p);
        assert!(
            prof.contains(r#"(allow file-write* (regex #"^/tmp/claude-[0-9a-f]+-cwd$"))"#),
            "{prof}"
        );
        assert!(
            prof.contains(r#"(allow file-write* (regex #"^/private/tmp/claude-[0-9a-f]+-cwd$"))"#),
            "{prof}"
        );
    }

    #[test]
    fn network_deny_adds_rule_allow_omits_it() {
        assert!(build_profile(&policy(NetworkPolicy::Deny)).contains("(deny network*)"));
        assert!(!build_profile(&policy(NetworkPolicy::Allow)).contains("(deny network*)"));
    }

    #[test]
    fn argv_prefix_shape() {
        let pre = argv_prefix(&policy(NetworkPolicy::Deny));
        assert_eq!(pre[0], "sandbox-exec");
        assert_eq!(pre[1], "-p");
        assert!(pre[2].starts_with("(version 1)"));
    }

    #[test]
    fn sbpl_string_escapes_quotes_and_backslashes() {
        assert_eq!(sbpl_string("/a/b"), "\"/a/b\"");
        assert_eq!(sbpl_string("/a\"b"), "\"/a\\\"b\"");
        assert_eq!(sbpl_string("/a\\b"), "\"/a\\\\b\"");
    }

    #[test]
    fn degenerate_root_slash_is_skipped() {
        let p = SandboxPolicy {
            filesystem: FsPolicy::Strict { writable: vec![PathBuf::from("/")] },
            extra_writable: vec![],
            ..policy(NetworkPolicy::Deny)
        };
        let prof = build_profile(&p);
        // The only "/" reference is the deny line, not an allow that would undo it.
        assert!(!prof.contains("(allow file-write* (subpath \"/\"))"));
    }
}
