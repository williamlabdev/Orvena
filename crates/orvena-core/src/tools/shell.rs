//! Declarative shell RUN tool (ADR-001). The model never supplies a command
//! string — it references a command the human pre-declared in `commands.yaml` by
//! `name`, and this tool spawns that command's **fixed argv** directly (no shell
//! interpretation). Role-gated like `fs.rs`/`grep.rs` (tool name: `shell.run`).
//!
//! Authorization is checked in a fixed order, and every denial is an
//! `Error::Scope` (the same class the driver routes to a hard blocker):
//!   1. the role must allow `shell.run`,
//!   2. the command `name` must be declared, and
//!   3. its `intent` must be `read_only` — a `mutating` command may be declared
//!      (for humans/gates) but the model may never trigger it.

use super::Tool;
use crate::config::commands::{Commands, Intent};
use crate::config::roles::Role;
use crate::error::{Error, Result};
use crate::exec::sandbox::Sandbox;
use crate::exec::{CommandOutput, CommandRunner, RunError};
use std::path::PathBuf;

pub struct ShellTool<'a> {
    pub root: PathBuf,
    pub role: &'a Role,
    pub commands: &'a Commands,
    /// OS sandbox applied to every command this tool spawns (ADR-003).
    pub sandbox: &'a Sandbox,
}

impl<'a> ShellTool<'a> {
    pub fn new(
        root: impl Into<PathBuf>,
        role: &'a Role,
        commands: &'a Commands,
        sandbox: &'a Sandbox,
    ) -> Self {
        Self { root: root.into(), role, commands, sandbox }
    }

    /// Run the declared command `name` in the project root and return its captured
    /// output. Authorization failures are `Error::Scope`; a `read_only` command
    /// exiting non-zero (or timing out) is **not** an error — it is returned as a
    /// [`CommandOutput`] for the caller to feed back as evidence.
    pub fn run(&self, name: &str) -> Result<CommandOutput> {
        // 1. role boundary
        self.require_tool("shell.run")?;

        // 2. the name must be declared
        let cmd = self.commands.get(name).ok_or_else(|| {
            Error::Scope(format!("command '{name}' is not declared in commands.yaml"))
        })?;

        // 3. the model may only trigger read-only commands
        if cmd.intent == Intent::Mutating {
            return Err(Error::Scope(format!(
                "command '{name}' is declared intent: mutating — the model may not trigger it \
                 (mutating commands are for humans/gates only)"
            )));
        }

        let runner = CommandRunner::with_sandbox(&self.root, cmd.timeout(), self.sandbox.clone());
        runner.run_argv(&cmd.argv).map_err(|e| match e {
            RunError::Spawn(e) => Error::Other(anyhow::anyhow!("cannot run '{name}': {e}")),
            RunError::Sandbox(e) => Error::Other(anyhow::anyhow!("cannot run '{name}': {e}")),
        })
    }

    fn require_tool(&self, tool: &str) -> Result<()> {
        if self.role.tool_allowed(tool) {
            Ok(())
        } else {
            Err(Error::Scope(format!("role '{}' is not allowed to use '{tool}'", self.role.name)))
        }
    }
}

impl<'a> Tool for ShellTool<'a> {
    fn name(&self) -> &str {
        "shell"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::commands::Command;

    fn role(allowed: Vec<&str>) -> Role {
        Role {
            name: "developer".into(),
            allowed_tools: allowed.into_iter().map(String::from).collect(),
            forbidden_tools: vec![],
            knowledge_scope: vec![],
        }
    }

    fn cmd(name: &str, argv: &[&str], intent: Intent) -> Command {
        Command {
            name: name.into(),
            argv: argv.iter().map(|s| s.to_string()).collect(),
            intent,
            timeout_secs: Some(30),
        }
    }

    fn commands() -> Commands {
        Commands {
            commands: vec![
                cmd("ok", &["sh", "-c", "printf out; printf err 1>&2"], Intent::ReadOnly),
                cmd("fail", &["sh", "-c", "exit 7"], Intent::ReadOnly),
                cmd("fmt-fix", &["sh", "-c", "true"], Intent::Mutating),
            ],
        }
    }

    #[test]
    fn read_only_command_runs_and_captures_output() {
        let cmds = commands();
        let role = role(vec!["shell.run"]);
        let out = ShellTool::new(std::env::temp_dir(), &role, &cmds, &Sandbox::disabled())
            .run("ok")
            .unwrap();
        assert!(out.success());
        assert_eq!(out.stdout, "out");
        assert_eq!(out.stderr, "err");
    }

    #[test]
    fn nonzero_exit_is_returned_not_errored() {
        let cmds = commands();
        let role = role(vec!["shell.run"]);
        let out = ShellTool::new(std::env::temp_dir(), &role, &cmds, &Sandbox::disabled())
            .run("fail")
            .unwrap();
        assert!(!out.success());
        assert_eq!(out.exit_code, Some(7));
    }

    #[test]
    fn role_without_shell_run_is_denied_with_scope_error() {
        let cmds = commands();
        let role = role(vec!["fs.read"]);
        let err = ShellTool::new(std::env::temp_dir(), &role, &cmds, &Sandbox::disabled())
            .run("ok")
            .unwrap_err();
        assert!(matches!(err, Error::Scope(_)));
    }

    #[test]
    fn undeclared_name_is_a_scope_error() {
        let cmds = commands();
        let role = role(vec!["shell.run"]);
        let err = ShellTool::new(std::env::temp_dir(), &role, &cmds, &Sandbox::disabled())
            .run("deploy")
            .unwrap_err();
        assert!(matches!(err, Error::Scope(_)));
        assert!(err.to_string().contains("not declared"));
    }

    #[test]
    fn mutating_command_is_denied_even_when_declared() {
        let cmds = commands();
        let role = role(vec!["shell.run"]);
        let err = ShellTool::new(std::env::temp_dir(), &role, &cmds, &Sandbox::disabled())
            .run("fmt-fix")
            .unwrap_err();
        assert!(matches!(err, Error::Scope(_)));
        assert!(err.to_string().contains("mutating"));
    }

    #[test]
    fn role_gate_is_checked_before_name_lookup() {
        // A role without shell.run asking for an undeclared name is still a role
        // denial — the boundary check comes first.
        let cmds = commands();
        let role = role(vec!["fs.read"]);
        let err = ShellTool::new(std::env::temp_dir(), &role, &cmds, &Sandbox::disabled())
            .run("does-not-exist")
            .unwrap_err();
        assert!(err.to_string().contains("not allowed to use 'shell.run'"));
    }
}
