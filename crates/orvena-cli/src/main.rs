//! `orvena` — a thin CLI over `orvena-core`. All real logic lives in the core
//! library; these commands only parse arguments and print results.

mod cli;
mod commands;
mod sandbox_shim;

fn main() {
    // The OS-sandbox re-exec shim (ADR-003) must be dispatched BEFORE the tokio
    // runtime starts: it runs on a single thread so a future Linux backend can
    // apply Landlock/seccomp without async-signal-safety hazards, then execvp the
    // wrapped command. `orvena __sandbox …` is hidden — only ever produced
    // internally as a command prefix, never invoked by users.
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("__sandbox")) {
        sandbox_shim::dispatch(); // never returns
    }

    // Everything else runs on the normal multi-threaded runtime (the previous
    // `#[tokio::main]` default), now built explicitly so the shim can be
    // intercepted ahead of it.
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build the tokio runtime");
    std::process::exit(runtime.block_on(cli::run()));
}
