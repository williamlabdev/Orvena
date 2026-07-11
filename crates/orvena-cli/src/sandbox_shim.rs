//! Hidden `orvena __sandbox` dispatch: the OS-sandbox re-exec confinement shim
//! (ADR-003 / slice-016). It is produced only as an internally-generated command
//! prefix — never typed by users — and only on platforms whose sandbox backend
//! uses a re-exec shim (Linux). It is dispatched from `main` BEFORE the tokio
//! runtime starts, so restrictions are applied on a single thread (which is
//! async-signal-safe), then the real command is `execvp`-ed in its place.
//!
//! Invocation shape (built by `sandbox_linux::argv_prefix`):
//!
//! ```text
//! orvena __sandbox --spec <json> -- <program> <args…>
//! ```
//!
//! The heavy lifting (parse spec → apply Landlock + seccomp → execvp) lives in
//! `orvena_core::exec::sandbox::run_linux_shim`, which fails closed on platforms
//! without a backend — so this never runs the wrapped command unconfined.

/// Handle an `orvena __sandbox …` invocation. Never returns.
pub fn dispatch() -> ! {
    let args: Vec<String> = std::env::args().collect();

    // args[0] = orvena, args[1] = __sandbox. Scan for `--spec <json>` and the
    // `--` that separates our flags from the wrapped command's argv.
    let mut spec_json: Option<String> = None;
    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--spec" => {
                spec_json = args.get(i + 1).cloned();
                i += 2;
            }
            "--" => {
                i += 1;
                break;
            }
            _ => i += 1,
        }
    }
    let wrapped: Vec<String> = args.get(i..).map(<[String]>::to_vec).unwrap_or_default();

    let Some(spec) = spec_json else {
        eprintln!("orvena __sandbox: missing --spec");
        std::process::exit(70);
    };

    orvena_core::exec::sandbox::run_linux_shim(&spec, &wrapped)
}
