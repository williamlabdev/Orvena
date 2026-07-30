//! Per-step parsing: the model speaks a tiny action protocol so the loop can
//! apply changes deterministically. v0.1 supports three actions — writing a file
//! in full, a read-only content search, and running a pre-declared command by
//! name (never a free-form command string — see ADR-001):
//!
//! ```text
//! <<<WRITE relative/path
//! <full new file content>
//! >>>
//!
//! <<<SEARCH <regex pattern>
//! [optional relative path to limit the search]
//! >>>
//!
//! <<<RUN <command name>
//! >>>
//! ```

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Write { path: String, content: String },
    Search { pattern: String, path: Option<String> },
    Run { name: String },
}

/// Parse zero or more `<<<WRITE …>>>` / `<<<SEARCH …>>>` / `<<<RUN …>>>` blocks
/// from a model response.
pub fn parse_actions(text: &str) -> Vec<Action> {
    let mut actions = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        if let Some(raw) = trimmed.strip_prefix("<<<WRITE ") {
            let (path, self_closed) = split_header(raw);
            let mut content_lines = Vec::new();
            if !self_closed {
                for body in lines.by_ref() {
                    if body.trim() == ">>>" {
                        break;
                    }
                    content_lines.push(body);
                }
            }
            let mut content = content_lines.join("\n");
            if !content.is_empty() {
                content.push('\n');
            }
            actions.push(Action::Write { path, content });
        } else if let Some(raw) = trimmed.strip_prefix("<<<SEARCH ") {
            let (pattern, self_closed) = split_header(raw);
            // The first non-empty body line (if any) narrows the search path.
            let mut path = None;
            if !self_closed {
                for body in lines.by_ref() {
                    let body = body.trim();
                    if body == ">>>" {
                        break;
                    }
                    if path.is_none() && !body.is_empty() {
                        path = Some(body.to_string());
                    }
                }
            }
            if !pattern.is_empty() {
                actions.push(Action::Search { pattern, path });
            }
        } else if let Some(raw) = trimmed.strip_prefix("<<<RUN ") {
            let (name, self_closed) = split_header(raw);
            // RUN takes only a name; consume the block up to its closing `>>>`
            // (any body lines are ignored — the command's argv lives in config).
            if !self_closed {
                for body in lines.by_ref() {
                    if body.trim() == ">>>" {
                        break;
                    }
                }
            }
            if !name.is_empty() {
                actions.push(Action::Run { name });
            }
        }
    }
    actions
}

/// Split a block header into its value and whether the block closed on the same
/// line (`<<<RUN check>>>`).
///
/// Models write the one-line form constantly, and taking it literally is worse
/// than a parse failure: the closing marker ends up *inside* the value, so
/// `<<<RUN check>>>` asks for a command named `check>>>` (undeclared → a scope
/// blocker) and `<<<WRITE config.json>>` writes a file literally named
/// `config.json>>` — a path the task never declared, which the independent
/// oracle then counts as a **containment violation**. Both were observed from a
/// real model within minutes of the benchmark first granting its role a shell:
/// the measurement was scoring the parser's pedantry as the agent's misbehavior.
/// The parser would also keep eating lines hunting for a `>>>` that had already
/// gone by, swallowing the actions that followed.
///
/// A truncated closer (`>>`) is accepted for the same reason. The trailing run of
/// `>` is measured rather than pattern-matched, so a search pattern that legally
/// ends in `>` survives: `<<<SEARCH Vec<T>>>>` has a run of four, three of which
/// are the closer. A lone trailing `>` is left alone — one character is likelier
/// to belong to the value than to be half a marker.
fn split_header(raw: &str) -> (String, bool) {
    let raw = raw.trim();
    let closers = raw.chars().rev().take_while(|c| *c == '>').count();
    let strip = match closers {
        0 | 1 => 0,
        2 => 2,
        _ => 3,
    };
    if strip == 0 {
        return (raw.to_string(), false);
    }
    (raw[..raw.len() - strip].trim().to_string(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_single_write() {
        let text = "preamble\n<<<WRITE src/a.txt\nhello\nworld\n>>>\ntrailer";
        let actions = parse_actions(text);
        assert_eq!(
            actions,
            vec![Action::Write { path: "src/a.txt".into(), content: "hello\nworld\n".into() }]
        );
    }

    #[test]
    fn parses_none_when_absent() {
        assert!(parse_actions("just prose, no actions").is_empty());
    }

    #[test]
    fn parses_a_search_without_path() {
        let actions = parse_actions("<<<SEARCH fn main\n>>>");
        assert_eq!(actions, vec![Action::Search { pattern: "fn main".into(), path: None }]);
    }

    #[test]
    fn parses_a_search_with_path() {
        let actions = parse_actions("<<<SEARCH TODO\nsrc\n>>>");
        assert_eq!(
            actions,
            vec![Action::Search { pattern: "TODO".into(), path: Some("src".into()) }]
        );
    }

    #[test]
    fn parses_mixed_search_and_write_in_order() {
        let text = "<<<SEARCH TODO\n>>>\nsome prose\n<<<WRITE a.txt\ndone\n>>>";
        let actions = parse_actions(text);
        assert_eq!(
            actions,
            vec![
                Action::Search { pattern: "TODO".into(), path: None },
                Action::Write { path: "a.txt".into(), content: "done\n".into() },
            ]
        );
    }

    #[test]
    fn parses_a_single_run() {
        let actions = parse_actions("<<<RUN test\n>>>");
        assert_eq!(actions, vec![Action::Run { name: "test".into() }]);
    }

    #[test]
    fn run_ignores_body_lines_and_takes_only_the_name() {
        let actions = parse_actions("<<<RUN clippy\nthis body is ignored\n>>>");
        assert_eq!(actions, vec![Action::Run { name: "clippy".into() }]);
    }

    #[test]
    fn a_run_that_closes_on_its_own_line_is_still_a_run() {
        // Observed from a real model the first time the benchmark gave its role a
        // shell: it emitted the one-line form, the closing marker became part of
        // the name, and the call came back as `command 'show-validator>>>' is not
        // declared` — a capability that looked broken because of punctuation.
        assert_eq!(
            parse_actions("<<<RUN show-validator>>>"),
            vec![Action::Run { name: "show-validator".into() }]
        );
    }

    #[test]
    fn a_self_closed_block_does_not_swallow_the_actions_after_it() {
        // The old parser kept reading past a self-closed header looking for a
        // `>>>` that had already gone by, eating whatever came next.
        let text = "<<<RUN check>>>\n<<<WRITE a.txt\nfixed\n>>>";
        assert_eq!(
            parse_actions(text),
            vec![
                Action::Run { name: "check".into() },
                Action::Write { path: "a.txt".into(), content: "fixed\n".into() },
            ]
        );
    }

    #[test]
    fn a_self_closed_write_does_not_invent_an_out_of_scope_path() {
        // `src/a.txt>>>` is a path no task ever declares — the parser would have
        // manufactured a containment violation out of a formatting quirk.
        assert_eq!(
            parse_actions("<<<WRITE src/a.txt>>>"),
            vec![Action::Write { path: "src/a.txt".into(), content: String::new() }]
        );
    }

    #[test]
    fn a_self_closed_search_keeps_its_pattern_clean() {
        assert_eq!(
            parse_actions("<<<SEARCH TODO>>>"),
            vec![Action::Search { pattern: "TODO".into(), path: None }]
        );
    }

    #[test]
    fn a_truncated_closer_is_accepted_too() {
        // Also observed live: `<<<WRITE config.json>>`. Taken literally it writes
        // `config.json>>`, which the independent oracle scores as an out-of-scope
        // file — a containment violation manufactured by a missing character.
        assert_eq!(
            parse_actions("<<<WRITE config.json>>\n"),
            vec![Action::Write { path: "config.json".into(), content: String::new() }]
        );
        assert_eq!(parse_actions("<<<RUN check>>"), vec![Action::Run { name: "check".into() }]);
    }

    #[test]
    fn a_pattern_that_legally_ends_in_an_angle_bracket_survives() {
        // `Vec<T>` + `>>>` is a run of four; only the closer comes off.
        assert_eq!(
            parse_actions("<<<SEARCH Vec<T>>>>"),
            vec![Action::Search { pattern: "Vec<T>".into(), path: None }]
        );
        // A lone trailing `>` belongs to the value, not to a marker.
        assert_eq!(
            parse_actions("<<<SEARCH fn f() ->\n>>>"),
            vec![Action::Search { pattern: "fn f() ->".into(), path: None }]
        );
    }

    #[test]
    fn parses_write_then_run_in_order() {
        let text = "<<<WRITE a.txt\nfix\n>>>\n<<<RUN test\n>>>";
        let actions = parse_actions(text);
        assert_eq!(
            actions,
            vec![
                Action::Write { path: "a.txt".into(), content: "fix\n".into() },
                Action::Run { name: "test".into() },
            ]
        );
    }
}
