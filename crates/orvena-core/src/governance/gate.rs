//! Gate evaluation. An automated gate runs its `verify` command in the project
//! root and treats exit 0 as a pass, capturing the command output as observable
//! evidence (the local analogue of "re-run CI until green"). A human gate cannot
//! be auto-confirmed: it escalates and stops the loop.
//!
//! The `verify` command runs through the shared [`CommandRunner`], so — like the
//! RUN tool — it is now bounded by a timeout (a gate that outruns it counts as a
//! verify failure, per ADR-001, rather than hanging the loop forever). The gate
//! keeps the `sh -c` path because its command string is human-authored.

use crate::config::gates::{Gate, Gatekeeper};
use crate::exec::sandbox::Sandbox;
use crate::exec::{CommandRunner, RunError};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct GateOutcome {
    pub gate: String,
    pub passed: bool,
    /// Observable evidence (command output, or why it could not be auto-decided).
    pub evidence: String,
    /// True when the gate needs a human (judgment, not mechanics).
    pub needs_human: bool,
}

pub struct GateRunner;

impl GateRunner {
    pub fn run(gate: &Gate, cwd: &Path, sandbox: &Sandbox) -> GateOutcome {
        match gate.gatekeeper {
            Gatekeeper::Human => GateOutcome {
                gate: gate.name.clone(),
                passed: false,
                evidence: format!("'{}' requires human judgment", gate.condition),
                needs_human: true,
            },
            Gatekeeper::Automated => match &gate.verify {
                None => GateOutcome {
                    gate: gate.name.clone(),
                    passed: false,
                    evidence:
                        "automated gate has no `verify` command — cannot produce evidence".into(),
                    needs_human: false,
                },
                Some(cmd) => Self::run_verify(&gate.name, cmd, cwd, gate.timeout(), sandbox),
            },
        }
    }

    fn run_verify(
        name: &str,
        cmd: &str,
        cwd: &Path,
        timeout: std::time::Duration,
        sandbox: &Sandbox,
    ) -> GateOutcome {
        match CommandRunner::with_sandbox(cwd, timeout, sandbox.clone()).run_shell(cmd) {
            Ok(out) if out.timed_out => GateOutcome {
                gate: name.to_string(),
                passed: false,
                evidence: format!("verify timed out after {}s", timeout.as_secs()),
                needs_human: false,
            },
            Ok(out) => {
                let mut captured = String::new();
                captured.push_str(&out.stdout);
                captured.push_str(&out.stderr);
                let captured = captured.trim();
                // Never emit empty evidence for a *failure*: a silent verify
                // (`test -f x`, `grep -q`, `diff -q`) prints nothing on failure,
                // and an empty string would leave the loop's next attempt with an
                // unchanged context — nothing to act on. Synthesize the exit
                // status so the model at least knows the check ran and failed.
                let evidence = if out.success() || !captured.is_empty() {
                    captured.to_string()
                } else {
                    match out.exit_code {
                        Some(code) => format!("verify exited {code} with no output"),
                        None => "verify exited abnormally with no output".to_string(),
                    }
                };
                GateOutcome {
                    gate: name.to_string(),
                    passed: out.success(),
                    evidence,
                    needs_human: false,
                }
            }
            Err(RunError::Spawn(e)) => GateOutcome {
                gate: name.to_string(),
                passed: false,
                evidence: format!("could not run verify command: {e}"),
                needs_human: false,
            },
            // Sandbox fail-closed: the verify never ran because the OS sandbox
            // was unavailable. That is a verify failure (never a silent pass),
            // and the reason is captured as evidence.
            Err(RunError::Sandbox(e)) => GateOutcome {
                gate: name.to_string(),
                passed: false,
                evidence: format!("could not run verify command: {e}"),
                needs_human: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::gates::Gatekeeper;

    fn gate(verify: &str, timeout_secs: Option<u64>) -> Gate {
        Gate {
            name: "check".into(),
            condition: "the check passes".into(),
            verify: Some(verify.into()),
            gatekeeper: Gatekeeper::Automated,
            timeout_secs,
        }
    }

    #[test]
    fn exit_zero_passes() {
        let outcome = GateRunner::run(&gate("true", None), &std::env::temp_dir(), &Sandbox::disabled());
        assert!(outcome.passed);
    }

    #[test]
    fn nonzero_exit_fails_with_evidence() {
        let outcome =
            GateRunner::run(&gate("echo boom 1>&2; exit 1", None), &std::env::temp_dir(), &Sandbox::disabled());
        assert!(!outcome.passed);
        assert!(outcome.evidence.contains("boom"));
    }

    #[test]
    fn a_gate_that_outruns_its_timeout_fails_verify() {
        // The deliberate behavior change from unifying on CommandRunner: a hung
        // verify is a verify failure (passed = false), not an infinite hang.
        let outcome = GateRunner::run(&gate("sleep 30", Some(1)), &std::env::temp_dir(), &Sandbox::disabled());
        assert!(!outcome.passed, "a timed-out gate must not pass");
        assert!(outcome.evidence.contains("timed out"), "evidence: {}", outcome.evidence);
        assert!(!outcome.needs_human);
    }

    #[test]
    fn silent_failure_synthesizes_exit_status() {
        // A verify that fails with no output must still yield actionable evidence
        // (the exit code), or the re-attempt loop has nothing to converge on.
        let outcome = GateRunner::run(&gate("exit 7", None), &std::env::temp_dir(), &Sandbox::disabled());
        assert!(!outcome.passed);
        assert!(
            outcome.evidence.contains("exited 7"),
            "silent failure must report the exit status: {}",
            outcome.evidence
        );
    }

    #[test]
    fn missing_verify_command_fails_closed() {
        // An automated gate with no `verify` cannot produce evidence — it must
        // fail closed (never silently pass), and it is not a human escalation.
        let g = Gate {
            name: "no-verify".into(),
            condition: "something observable".into(),
            verify: None,
            gatekeeper: Gatekeeper::Automated,
            timeout_secs: None,
        };
        let outcome = GateRunner::run(&g, &std::env::temp_dir(), &Sandbox::disabled());
        assert!(!outcome.passed, "a verify-less automated gate must not pass");
        assert!(!outcome.needs_human);
        assert!(outcome.evidence.contains("verify"), "evidence: {}", outcome.evidence);
    }

    #[test]
    fn human_gate_escalates() {
        let g = Gate {
            name: "review".into(),
            condition: "a human reviewed it".into(),
            verify: None,
            gatekeeper: Gatekeeper::Human,
            timeout_secs: None,
        };
        let outcome = GateRunner::run(&g, &std::env::temp_dir(), &Sandbox::disabled());
        assert!(outcome.needs_human);
        assert!(!outcome.passed);
    }
}
