//! Context assembly under a per-role token budget. High-value items first
//! (the task, the writable targets and their current contents), trimmed when the
//! budget is exhausted. (Controlled Context pillar.)

use crate::config::commands::{Commands, Intent};
use crate::config::roles::Role;
use crate::governance::scope::Scope;
use crate::provider::Message;
use crate::tools::fs::FsTool;
use crate::util::estimate_tokens;
use std::path::Path;

/// The assembled prompt plus how many tokens it is estimated to use.
pub struct BuiltContext {
    pub messages: Vec<Message>,
    pub used_tokens: u32,
}

/// Build the system + user messages for one loop iteration.
///
/// `prior_evidence` carries a failed gate's output back into the prompt so the
/// model can fix it on the next bounded attempt (observe → re-attempt).
pub fn build(
    root: &Path,
    scope: &Scope,
    role: &Role,
    budget_tokens: u32,
    instruction: &str,
    prior_evidence: &str,
    commands: &Commands,
) -> BuiltContext {
    let system = system_prompt(role);

    let mut user = String::new();
    user.push_str(&format!("Task: {instruction}\n\n"));

    // Which commands the model may actually run. The system prompt has always
    // described `<<<RUN name>>>`, but never said which names exist — leaving the
    // tool discoverable only by guessing, which is the same as not shipping it.
    // Only `read_only` commands are listed: the runtime refuses a `mutating` one
    // anyway (ADR-001), so advertising it would only invite a blocked call.
    user.push_str(&runnable_commands(role, commands));

    // Machine-readable list of writable targets (also consumed by the offline
    // provider). Everything else is read-only.
    user.push_str("WRITABLE:\n");
    if scope.allowed_modifications.is_empty() {
        user.push_str("(none — read-only task)\n");
    } else {
        for p in &scope.allowed_modifications {
            user.push_str(&format!("- {p}\n"));
        }
    }
    user.push('\n');

    // Current contents of the writable files, high-value first, within budget.
    let fs = FsTool::new(root, scope, role);
    let mut used = estimate_tokens(&system) + estimate_tokens(&user);
    user.push_str("Current files in scope:\n");
    for p in &scope.allowed_modifications {
        let body = match fs.read_opt(p) {
            Ok(Some(c)) => c,
            Ok(None) => "(new file)".to_string(),
            Err(_) => "(unreadable)".to_string(),
        };
        let block = format!("--- {p} ---\n{body}\n");
        let cost = estimate_tokens(&block);
        if used + cost > budget_tokens {
            user.push_str("(remaining files omitted: context budget reached)\n");
            break;
        }
        used += cost;
        user.push_str(&block);
    }

    if !prior_evidence.trim().is_empty() {
        let note = format!(
            "\nEvidence from the previous attempt (search results and/or failed-gate \
             output) — use it to complete the task:\n{}\n",
            prior_evidence.trim()
        );
        used += estimate_tokens(&note);
        user.push_str(&note);
    }

    BuiltContext { messages: vec![Message::system(system), Message::user(user)], used_tokens: used }
}

/// The `RUNNABLE:` block — the names this role may pass to `<<<RUN …>>>`. Empty
/// string when the role has no `shell.run` or nothing read-only is declared, so
/// a project that grants neither sees no change at all.
///
/// **Names only, deliberately.** Printing each command's argv would be friendlier
/// — and would leak the command *string* into the prompt. A check like
/// `test "$(cat answer.txt)" = "42"` would then be handing over its own answer,
/// which is not a hypothetical: the benchmark declares every task's `verify` as a
/// runnable command. A name is enough to invoke; the output is what the model is
/// supposed to learn from.
fn runnable_commands(role: &Role, commands: &Commands) -> String {
    if !role.tool_allowed("shell.run") {
        return String::new();
    }
    let names: Vec<&str> = commands
        .commands
        .iter()
        .filter(|c| c.intent == Intent::ReadOnly)
        .map(|c| c.name.as_str())
        .collect();
    if names.is_empty() {
        return String::new();
    }
    format!(
        "RUNNABLE (read-only commands; emit <<<RUN name>>> to see their output):\n{}\n\n",
        names.iter().map(|n| format!("- {n}")).collect::<Vec<_>>().join("\n")
    )
}

fn system_prompt(role: &Role) -> String {
    format!(
        "You are Orvena, a disciplined coding agent operating as role '{role}'.\n\
         Rules:\n\
         - Bounded change: modify ONLY files listed under WRITABLE. All other files are read-only.\n\
         - If you need to change a file that is not WRITABLE, STOP and report a blocker — never expand scope.\n\
         - Emit changes ONLY as action blocks, each in this exact format:\n\
         \x20 <<<WRITE relative/path\n\
         \x20 <full new file content>\n\
         \x20 >>>\n\
         - To search file contents (read-only), emit a search block; the hits are\n\
         \x20 returned as evidence on your next step:\n\
         \x20 <<<SEARCH <regex pattern>\n\
         \x20 [optional relative path to limit the search]\n\
         \x20 >>>\n\
         - To run a pre-declared command (e.g. tests), emit a run block; its output\n\
         \x20 is returned as evidence on your next step. You may only reference a\n\
         \x20 command the project declared by NAME — you cannot pass a command string:\n\
         \x20 <<<RUN <command name>\n\
         \x20 >>>\n\
         - Do not write prose outside action blocks.",
        role = role.name
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::agent::Tier;
    use crate::config::commands::Command;

    fn role(allowed: &[&str]) -> Role {
        Role {
            name: "developer".into(),
            allowed_tools: allowed.iter().map(|s| s.to_string()).collect(),
            forbidden_tools: vec![],
            knowledge_scope: vec![],
        }
    }

    fn commands() -> Commands {
        Commands {
            commands: vec![
                Command {
                    name: "check".into(),
                    // The kind of argv that must never reach the prompt: it
                    // carries the answer the task is supposed to compute.
                    argv: vec!["sh".into(), "-c".into(), "test \"$(cat a.txt)\" = \"42\"".into()],
                    intent: Intent::ReadOnly,
                    timeout_secs: None,
                },
                Command {
                    name: "deploy".into(),
                    argv: vec!["make".into(), "deploy".into()],
                    intent: Intent::Mutating,
                    timeout_secs: None,
                },
            ],
        }
    }

    fn build_user(role: &Role, commands: &Commands) -> String {
        let scope = Scope::new(vec!["a.txt".into()], Vec::new(), Tier::Light);
        let ctx = build(Path::new("."), &scope, role, 4096, "do the thing", "", commands);
        ctx.messages.iter().map(|m| m.content.clone()).collect::<Vec<_>>().join("\n")
    }

    #[test]
    fn a_role_with_shell_run_is_told_which_commands_exist() {
        // Without this the RUN tool is undiscoverable: the system prompt explains
        // the syntax but never says which names the project declared, so using it
        // means guessing.
        let prompt = build_user(&role(&["fs.write", "shell.run"]), &commands());
        assert!(prompt.contains("RUNNABLE"));
        assert!(prompt.contains("- check"));
    }

    #[test]
    fn the_command_string_itself_never_reaches_the_prompt() {
        // Printing argv would be friendlier and would hand over the expected
        // answer of every check that hardcodes one.
        let prompt = build_user(&role(&["fs.write", "shell.run"]), &commands());
        assert!(!prompt.contains("42"), "a check's own answer must not leak into the prompt");
        assert!(!prompt.contains("cat a.txt"));
    }

    #[test]
    fn a_mutating_command_is_not_advertised() {
        // The runtime refuses it anyway (ADR-001); listing it would only invite a
        // call that comes back as a scope violation.
        let prompt = build_user(&role(&["fs.write", "shell.run"]), &commands());
        assert!(!prompt.contains("deploy"));
    }

    #[test]
    fn a_role_without_shell_run_sees_no_block_at_all() {
        let prompt = build_user(&role(&["fs.write"]), &commands());
        assert!(!prompt.contains("RUNNABLE"), "no shell.run, no listing");
    }

    #[test]
    fn no_declared_commands_means_no_empty_header() {
        let prompt = build_user(&role(&["fs.write", "shell.run"]), &Commands::default());
        assert!(!prompt.contains("RUNNABLE"));
    }
}
