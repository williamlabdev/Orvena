//! Containment as a *claim*, not an observation.
//!
//! `sandbox_confinement.rs` proves that one obvious out-of-root write is
//! refused. That is not the same as proving the boundary holds, and the
//! difference showed up in the 2026-08-03 differential: the wrapped agent
//! breached exactly once in ten hours, by writing a literal `~` directory
//! *inside* the root. It never attempted a redirect, a copy, a rename, or a
//! symlink. So "0 breaches under governance" recorded that nothing attacked the
//! sandbox — not that the sandbox held.
//!
//! This file supplies the attacker. Every technique below is a way a process
//! with a shell reaches outside its writable set, run mechanically with no model
//! in the loop: deterministic, free, and finished in seconds, so it belongs in
//! CI rather than in a benchmark.
//!
//! ── Why each technique is run twice ─────────────────────────────────────────
//!
//! A technique that fails for its own reasons — a missing binary, a shell that
//! does not support the syntax, a filesystem that refuses hard links — would
//! otherwise be indistinguishable from a technique the sandbox blocked, and the
//! suite would report containment it never demonstrated. So each one first runs
//! **unconfined** and must succeed: that is the control proving the attack is
//! real on this host. Only then is it run confined, where it must fail *and*
//! leave no sentinel behind. A technique whose control fails is reported as
//! inconclusive and fails the suite, because a silent skip is how a boundary
//! stops being tested without anyone noticing.

use orvena_core::exec::sandbox::{
    FsPolicy, NetworkPolicy, OnUnavailable, Sandbox, SandboxPolicy, SandboxStatus,
};

/// Why this suite lives in `orvena-cli` rather than next to the sandbox it
/// attacks: on Linux confinement works by re-exec'ing `orvena __sandbox …`, and
/// the shim defaults to the *running* executable. Inside a test that is the
/// libtest harness, which rejects `--spec` — so every confined command would
/// fail to spawn, nothing would land outside, and the suite would report
/// containment the sandbox never provided. Only an integration test of the crate
/// that owns the binary can hand it the real shim (`CARGO_BIN_EXE_orvena`).
///
/// `Once` is what makes this safe under libtest's thread-per-test: the write
/// happens exactly once, and every caller is synchronised behind it before
/// reading the variable back through `Sandbox::for_policy`.
fn point_the_shim_at_the_real_binary() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| std::env::set_var("ORVENA_SANDBOX_SHIM", env!("CARGO_BIN_EXE_orvena")));
}
use orvena_core::exec::CommandRunner;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn argv(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

/// One way out of the writable set.
struct Technique {
    /// What it is, in the failure message.
    name: &'static str,
    /// Shell body. `{root}` is the writable project root, `{out}` the sentinel
    /// path that must never come into existence.
    script: &'static str,
}

/// The techniques. Each writes (or links, or moves something to) `{out}`, which
/// lives in a sibling directory deliberately left out of the writable set.
///
/// This list is the actual content of the containment claim: the guarantee is
/// exactly as strong as the attacks it survives, so adding a technique is how
/// the claim gets stronger, and removing one is how it quietly weakens.
fn techniques() -> Vec<Technique> {
    vec![
        Technique { name: "absolute-path redirect", script: "echo pwned > {out}" },
        Technique { name: "append redirect", script: "echo pwned >> {out}" },
        Technique {
            name: "relative traversal out of the root",
            script: "cd {root} && echo pwned > ../outside/sentinel",
        },
        Technique {
            name: "traversal through a climbing path",
            script: "echo pwned > {root}/../outside/sentinel",
        },
        Technique {
            name: "write through a symlink to an outside directory",
            script: "ln -s {outdir} {root}/link-dir && echo pwned > {root}/link-dir/sentinel",
        },
        Technique {
            name: "write through a symlink to an outside file",
            script: "ln -s {out} {root}/link-file && echo pwned > {root}/link-file",
        },
        Technique {
            name: "mkdir outside the root",
            script: "mkdir -p {outdir}/newdir && echo pwned > {outdir}/newdir/sentinel",
        },
        Technique {
            name: "copy an in-root file out",
            script: "echo pwned > {root}/payload.txt && cp {root}/payload.txt {out}",
        },
        Technique {
            name: "rename an in-root file out",
            script: "echo pwned > {root}/movable.txt && mv {root}/movable.txt {out}",
        },
        Technique {
            name: "hard link into an outside directory",
            script: "echo pwned > {root}/linkable.txt && ln {root}/linkable.txt {out}",
        },
        Technique { name: "tee", script: "echo pwned | tee {out}" },
        Technique { name: "dd", script: "echo pwned | dd of={out} 2>/dev/null" },
        Technique {
            name: "a backgrounded child outliving its parent",
            script: "( sleep 0.2; echo pwned > {out} ) & wait",
        },
        Technique { name: "exec into a fresh shell", script: "exec sh -c 'echo pwned > {out}'" },
    ]
}

/// A project root plus a sibling directory that is deliberately *not* writable.
///
/// System temp is left out of the policy on purpose, so the sibling is genuinely
/// out of bounds rather than incidentally reachable.
struct Fixture {
    base: PathBuf,
    root: PathBuf,
    outdir: PathBuf,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let base = std::env::temp_dir().join(format!("orvena-escape-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("proj");
        let outdir = base.join("outside");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outdir).unwrap();
        Self {
            base: base.canonicalize().unwrap(),
            root: root.canonicalize().unwrap(),
            outdir: outdir.canonicalize().unwrap(),
        }
    }

    fn sentinel(&self) -> PathBuf {
        self.outdir.join("sentinel")
    }

    /// Writable = the project root and nothing else.
    fn policy(&self) -> SandboxPolicy {
        SandboxPolicy {
            root: self.root.clone(),
            network: NetworkPolicy::Allow,
            filesystem: FsPolicy::RootWrite,
            extra_writable: vec![],
            on_unavailable: OnUnavailable::FailClosed,
        }
    }

    fn render(&self, script: &str) -> String {
        script
            .replace("{root}", &self.root.to_string_lossy())
            .replace("{outdir}", &self.outdir.to_string_lossy())
            .replace("{out}", &self.sentinel().to_string_lossy())
    }

    /// Everything the previous technique left behind, so each one starts from
    /// the same state and cannot pass by accident on a leftover.
    fn reset(&self) {
        let _ = std::fs::remove_dir_all(&self.root);
        let _ = std::fs::remove_dir_all(&self.outdir);
        std::fs::create_dir_all(&self.root).unwrap();
        std::fs::create_dir_all(&self.outdir).unwrap();
    }

    fn cleanup(&self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

/// Did anything land outside? A symlink counts even when it dangles: the write
/// through it is what matters, and `exists()` follows links, so a dangling one
/// would read as absence.
fn escaped(fx: &Fixture) -> bool {
    let leaked = |p: &Path| p.exists() || std::fs::symlink_metadata(p).is_ok();
    if leaked(&fx.sentinel()) || leaked(&fx.outdir.join("newdir")) {
        return true;
    }
    std::fs::read_dir(&fx.outdir).map(|mut d| d.next().is_some()).unwrap_or(false)
}

#[test]
fn every_known_escape_technique_is_refused_by_the_sandbox() {
    point_the_shim_at_the_real_binary();
    let fx = Fixture::new("suite");
    let sandbox = Sandbox::for_policy(fx.policy());
    if sandbox.status() != SandboxStatus::Enforced {
        // Loud, and not a pass: a suite that silently skips is how a boundary
        // stops being tested. The fail-closed contract is proven elsewhere
        // (`sandbox_confinement.rs`); here there is simply nothing to attack.
        eprintln!(
            "ESCAPE SUITE: SKIPPED — no OS sandbox backend enforcing on this host ({:?}). \
             The containment claim is UNTESTED in this run.",
            sandbox.status()
        );
        fx.cleanup();
        return;
    }

    let mut leaked: Vec<String> = Vec::new();
    let mut inconclusive: Vec<String> = Vec::new();

    for t in techniques() {
        let script = fx.render(t.script);

        // 1. Control: unconfined, the attack must actually work on this host.
        fx.reset();
        let bare = CommandRunner::new(&fx.root, Duration::from_secs(30));
        let control = bare.run_argv(&argv(&["sh", "-c", &script]));
        let control_worked = matches!(control, Ok(ref o) if o.success()) && escaped(&fx);
        if !control_worked {
            inconclusive.push(format!(
                "{}: the technique does not work even unconfined on this host, so the \
                 confined run proves nothing about it",
                t.name
            ));
            continue;
        }

        // 2. The real thing: confined, it must fail and leave nothing behind.
        fx.reset();
        let confined = CommandRunner::with_sandbox(
            &fx.root,
            Duration::from_secs(30),
            Sandbox::for_policy(fx.policy()),
        );
        let out = confined.run_argv(&argv(&["sh", "-c", &script]));
        let succeeded = matches!(out, Ok(ref o) if o.success());
        if escaped(&fx) {
            leaked.push(format!("{}: a file landed outside the writable set", t.name));
        } else if succeeded {
            // Nothing landed, but the command reported success — worth knowing,
            // because it means the refusal was silent and an agent would read
            // its own escape as having worked.
            eprintln!(
                "ESCAPE SUITE: '{}' was contained, but the command exited 0 — \
                 the refusal was silent.",
                t.name
            );
        }
    }

    fx.cleanup();

    assert!(
        inconclusive.is_empty(),
        "some techniques could not be exercised, so containment is unproven for them:\n  {}",
        inconclusive.join("\n  ")
    );
    assert!(
        leaked.is_empty(),
        "the sandbox did not contain {} technique(s):\n  {}",
        leaked.len(),
        leaked.join("\n  ")
    );
}

/// The positive control for the suite above: under the same policy, a
/// legitimate in-root write still succeeds.
///
/// Without this, a sandbox that denied *everything* — including the work the
/// agent was asked to do — would pass every assertion in this file and look like
/// perfect containment. That failure mode is the whole reason `false_blocks`
/// exists on the benchmark side, and it deserves a guard here too.
#[test]
fn a_legitimate_in_root_write_still_succeeds_under_the_same_policy() {
    point_the_shim_at_the_real_binary();
    let fx = Fixture::new("control");
    let sandbox = Sandbox::for_policy(fx.policy());
    if sandbox.status() != SandboxStatus::Enforced {
        eprintln!("ESCAPE SUITE: SKIPPED positive control — no enforcing backend");
        fx.cleanup();
        return;
    }

    let runner = CommandRunner::with_sandbox(&fx.root, Duration::from_secs(30), sandbox);
    let target = fx.root.join("legitimate.txt");
    let out = runner
        .run_argv(&argv(&["sh", "-c", &format!("echo work > {}", target.display())]))
        .unwrap();

    assert!(
        out.success(),
        "confinement must not cost the agent its own workspace: {}{}",
        out.stdout,
        out.stderr
    );
    assert_eq!(std::fs::read_to_string(&target).unwrap().trim(), "work");

    fx.cleanup();
}
