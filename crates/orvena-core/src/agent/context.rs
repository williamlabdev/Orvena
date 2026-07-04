//! Context assembly under a per-role token budget. High-value items first
//! (the task, the writable targets and their current contents), trimmed when the
//! budget is exhausted. (Controlled Context pillar.)

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
) -> BuiltContext {
    let system = system_prompt(role);

    let mut user = String::new();
    user.push_str(&format!("Task: {instruction}\n\n"));

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

    BuiltContext {
        messages: vec![Message::system(system), Message::user(user)],
        used_tokens: used,
    }
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
         - Do not write prose outside action blocks.",
        role = role.name
    )
}
