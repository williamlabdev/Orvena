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
    pub fn run(gate: &Gate, cwd: &Path) -> GateOutcome {
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
                Some(cmd) => Self::run_verify(&gate.name, cmd, cwd, gate.timeout()),
            },
        }
    }

    fn run_verify(
        name: &str,
        cmd: &str,
        cwd: &Path,
        timeout: std::time::Duration,
    ) -> GateOutcome {
        match CommandRunner::new(cwd, timeout).run_shell(cmd) {
            Ok(out) if out.timed_out => GateOutcome {
                gate: name.to_string(),
                passed: false,
                evidence: format!("verify timed out after {}s", timeout.as_secs()),
                needs_human: false,
            },
            Ok(out) => {
                let mut evidence = String::new();
                evidence.push_str(&out.stdout);
                evidence.push_str(&out.stderr);
                GateOutcome {
                    gate: name.to_string(),
                    passed: out.success(),
                    evidence: evidence.trim().to_string(),
                    needs_human: false,
                }
            }
            Err(RunError::Spawn(e)) => GateOutcome {
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
        let outcome = GateRunner::run(&gate("true", None), &std::env::temp_dir());
        assert!(outcome.passed);
    }

    #[test]
    fn nonzero_exit_fails_with_evidence() {
        let outcome =
            GateRunner::run(&gate("echo boom 1>&2; exit 1", None), &std::env::temp_dir());
        assert!(!outcome.passed);
        assert!(outcome.evidence.contains("boom"));
    }

    #[test]
    fn a_gate_that_outruns_its_timeout_fails_verify() {
        // The deliberate behavior change from unifying on CommandRunner: a hung
        // verify is a verify failure (passed = false), not an infinite hang.
        let outcome = GateRunner::run(&gate("sleep 30", Some(1)), &std::env::temp_dir());
        assert!(!outcome.passed, "a timed-out gate must not pass");
        assert!(outcome.evidence.contains("timed out"), "evidence: {}", outcome.evidence);
        assert!(!outcome.needs_human);
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
        let outcome = GateRunner::run(&g, &std::env::temp_dir());
        assert!(outcome.needs_human);
        assert!(!outcome.passed);
    }
}
