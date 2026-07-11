//! Hidden `orvena __sandbox` dispatch: the OS-sandbox re-exec confinement shim
//! (ADR-003 / slice-016). It is produced only as an internally-generated command
//! prefix — never typed by users — and only on platforms whose sandbox backend
//! uses a re-exec shim (Linux). It is dispatched from `main` BEFORE the tokio
//! runtime starts, so restrictions can be applied on a single thread (which is
//! async-signal-safe), then the real command is `execvp`-ed in its place.
//!
//! The Linux shim body (Landlock + seccomp) lands in slice-016. Until then this
//! is the structural hook only: it **fails closed** — it never `exec`s the
//! wrapped command unconfined — so wiring the real backend in later cannot
//! accidentally degrade to an unconfined run. In the current build no backend
//! emits this prefix, so reaching here is not expected.

/// Handle an `orvena __sandbox …` invocation. Never returns: a finished backend
/// will either `execvp` the wrapped command under confinement or exit with an
/// error; today it always exits fail-closed.
pub fn dispatch() -> ! {
    // slice-016 will, on Linux: parse `--spec <json>` and the `--`-separated
    // target argv, apply Landlock + seccomp per the spec, then execvp the target.
    eprintln!(
        "orvena __sandbox: the re-exec sandbox shim is not implemented in this \
         build yet (pending slice-016) — refusing to run the wrapped command \
         unconfined."
    );
    // EX_SOFTWARE: an internal-only entrypoint was reached that no current
    // backend should produce.
    std::process::exit(70);
}
