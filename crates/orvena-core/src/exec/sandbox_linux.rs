//! Linux sandbox backend — **not yet implemented** (ADR-003, D-F).
//!
//! The planned design is a re-exec shim: `CommandRunner` prepends
//! `[orvena, __sandbox, --policy <json>]`, and that hidden CLI subcommand applies
//! a Landlock ruleset (root subtree + temp writable, rest read-only) plus a
//! seccomp filter (deny `socket(2)` when `network: deny`), then `execvp`s the
//! real argv. A re-exec shim is used instead of `Command::pre_exec` because
//! applying Landlock between fork and exec in a multi-threaded (tokio) process is
//! not async-signal-safe — the crate allocates.
//!
//! Until that lands, this host reports the backend as unavailable. Per the
//! policy's `on_unavailable`, an engineering-tier run therefore **fails closed**
//! (refuses to run children unconfined) and a light-tier run warns — never a
//! silent unconfined "enforced" claim.

/// Why the Linux backend is currently unavailable.
pub fn unavailable_reason() -> String {
    "Linux Landlock/seccomp sandbox backend is not implemented yet (slice-015 follow-up)".into()
}
