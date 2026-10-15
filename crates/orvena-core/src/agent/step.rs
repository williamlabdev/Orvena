//! Per-step parsing: the model speaks a tiny action protocol so the loop can
//! apply changes deterministically. Six actions — writing a file in full, an
//! anchored partial edit, reading a file, a read-only content search,
//! running a pre-declared command by name (never a free-form command string —
//! see ADR-001), and pinning a step's evidence against window eviction
//! (slice-033):
//!
//! ```text
//! <<<WRITE relative/path
//! <full new file content>
//! >>>
//!
//! <<<EDIT relative/path
//! <text that must appear exactly once in the file>
//! ===
//! <replacement text>
//! >>>
//!
//! <<<READ relative/path
//! >>>
//!
//! <<<SEARCH <regex pattern>
//! [optional path to limit the search: a directory, a file, or a glob]
//! >>>
//!
//! <<<RUN <command name>
//! >>>
//!
//! <<<PIN <step number>
//! >>>
//! ```
//!
//! Two terminal blocks end a run rather than act on it (#54). They are parsed
//! by [`parse_step`] into [`Step::terminal`], never into the action list:
//!
//! ```text
//! <<<DONE <one-line summary>
//! >>>
//!
//! <<<BLOCKED <reason>
//! [further lines of reason]
//! >>>
//! ```
//!
//! EDIT's old/new are split on the first body line that is exactly `===`
//! (slice-020). Known v1 limit: an old/new that itself contains such a line
//! cannot be expressed — full-file WRITE remains the escape hatch. A block with
//! no separator yields no action at all: guessing at a partial edit risks a
//! wrong write, which is worse than a no-op step (the gate feedback drives the
//! retry).

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Write {
        path: String,
        content: String,
    },
    Edit {
        path: String,
        old: String,
        new: String,
    },
    Read {
        path: String,
    },
    Search {
        pattern: String,
        path: Option<String>,
    },
    Run {
        name: String,
    },
    /// Pin one step's evidence block against eviction (slice-033). Carries the
    /// step number as spoken; existence / eviction / budget checks live in the
    /// driver, where the window state is.
    Pin {
        step: u32,
    },
}

/// How the agent says the run is over (#54). Kept apart from [`Action`]: a
/// terminal block changes nothing in the project, and counting it as an action
/// would silently change the ungoverned loop's "zero actions" exit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Terminal {
    /// The agent claims the task is complete. A claim, not proof: what it is
    /// worth is decided by the driver (gates) or the bench (external verify).
    Done { summary: String },
    /// The agent declares it cannot complete the task as instructed. The
    /// reason is recorded verbatim, never interpreted.
    Blocked { reason: String },
}

/// Everything one model reply says, as parsed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Step {
    pub actions: Vec<Action>,
    /// The first terminal block, if any. Later ones are ignored.
    pub terminal: Option<Terminal>,
    /// The reply contains a model's native tool-call markup (`<invoke …>`,
    /// `<function_calls>`) instead of this protocol (#59). Detected, never
    /// parsed: mapping it to an action would make format adherence free and
    /// hide a failure the benchmark exists to see.
    pub native_markup: bool,
}

/// Parse zero or more `<<<WRITE …>>>` / `<<<SEARCH …>>>` / `<<<RUN …>>>` blocks
/// from a model response.
pub fn parse_actions(text: &str) -> Vec<Action> {
    parse_step(text).actions
}

/// Parse a model response into its actions, its terminal block and whether it
/// fell back to native tool-call markup.
pub fn parse_step(text: &str) -> Step {
    let mut actions = Vec::new();
    let mut terminal = None;
    let mut native_markup = false;
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        if is_native_markup(trimmed) {
            native_markup = true;
        } else if let Some((kind, raw)) = terminal_header(trimmed) {
            // Header text and body lines join into one string; a bare
            // `<<<DONE` / `>>>` still terminates, with an empty text.
            //
            // A `>` straight after the keyword can only be a closer, never
            // text: `<<<DONE>`, `<<<DONE>>`, `<<<DONE>>> thanks` all close on
            // the header line. Handing them to `split_header` would read the
            // block as open and swallow every action after it.
            let (head, self_closed) = match raw.strip_prefix('>') {
                Some(rest) => (rest.trim_start_matches('>').trim().to_string(), true),
                None => split_header(raw),
            };
            let mut parts: Vec<String> = Vec::new();
            if !head.is_empty() {
                parts.push(head);
            }
            if !self_closed {
                for body in lines.by_ref() {
                    if body.trim() == ">>>" {
                        break;
                    }
                    parts.push(body.trim().to_string());
                }
            }
            let text = parts.join(" ").trim().to_string();
            if terminal.is_none() {
                terminal = Some(match kind {
                    TerminalKind::Done => Terminal::Done { summary: text },
                    TerminalKind::Blocked => Terminal::Blocked { reason: text },
                });
            }
        } else if let Some(raw) = trimmed.strip_prefix("<<<WRITE ") {
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
        } else if let Some(raw) = trimmed.strip_prefix("<<<EDIT ") {
            let (path, self_closed) = split_header(raw);
            // Body split on the first `===` line. Neither side gets a trailing
            // newline appended (unlike WRITE): the anchor must match the file
            // byte-for-byte, and an implicit newline would make an anchor at
            // end-of-file unmatchable.
            let mut old_lines: Vec<&str> = Vec::new();
            let mut new_lines: Vec<&str> = Vec::new();
            let mut past_separator = false;
            if !self_closed {
                for body in lines.by_ref() {
                    if body.trim() == ">>>" {
                        break;
                    }
                    if !past_separator && body.trim() == "===" {
                        past_separator = true;
                        continue;
                    }
                    if past_separator {
                        new_lines.push(body);
                    } else {
                        old_lines.push(body);
                    }
                }
            }
            // No separator → no action (see module docs): a guessed partial
            // edit is worse than a no-op step.
            if !path.is_empty() && past_separator {
                actions.push(Action::Edit {
                    path,
                    old: old_lines.join("\n"),
                    new: new_lines.join("\n"),
                });
            }
        } else if let Some(raw) = trimmed.strip_prefix("<<<READ ") {
            let (path, self_closed) = split_header(raw);
            // Like RUN: the header carries everything; body lines are ignored.
            if !self_closed {
                for body in lines.by_ref() {
                    if body.trim() == ">>>" {
                        break;
                    }
                }
            }
            if !path.is_empty() {
                actions.push(Action::Read { path });
            }
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
        } else if let Some(raw) = trimmed.strip_prefix("<<<PIN ") {
            let (arg, self_closed) = split_header(raw);
            // Like RUN: the header carries everything; body lines are ignored.
            if !self_closed {
                for body in lines.by_ref() {
                    if body.trim() == ">>>" {
                        break;
                    }
                }
            }
            // A non-numeric argument yields no action (same discipline as an
            // empty READ path): a guessed pin target would refuse confusingly
            // in the driver, and the model gets clearer feedback from the
            // block simply not landing.
            if let Ok(step) = arg.parse::<u32>() {
                actions.push(Action::Pin { step });
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
    Step { actions, terminal, native_markup }
}

enum TerminalKind {
    Done,
    Blocked,
}

/// `<<<DONE` / `<<<BLOCKED` followed by nothing, a space or a closer — not
/// `<<<DONEX`. Returns the rest of the header line.
fn terminal_header(trimmed: &str) -> Option<(TerminalKind, &str)> {
    let (kind, rest) = if let Some(rest) = trimmed.strip_prefix("<<<DONE") {
        (TerminalKind::Done, rest)
    } else if let Some(rest) = trimmed.strip_prefix("<<<BLOCKED") {
        (TerminalKind::Blocked, rest)
    } else {
        return None;
    };
    (rest.is_empty() || rest.starts_with(' ') || rest.starts_with('>')).then_some((kind, rest))
}

/// A line opening a native tool call. Observed from Sonnet 5.5 on 2026-10-13:
/// `<invoke name="READ">`, and runs of empty `<invoke name="x">` (#59).
fn is_native_markup(trimmed: &str) -> bool {
    trimmed.starts_with("<invoke ")
        || trimmed.starts_with("<invoke>")
        || trimmed.starts_with("<function_calls>")
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
    fn parses_an_edit_with_old_and_new() {
        let text = "<<<EDIT src/a.rs\nlet x = 1;\n===\nlet x = 2;\n>>>";
        assert_eq!(
            parse_actions(text),
            vec![Action::Edit {
                path: "src/a.rs".into(),
                old: "let x = 1;".into(),
                new: "let x = 2;".into(),
            }]
        );
    }

    #[test]
    fn edit_sides_are_multi_line_without_trailing_newlines() {
        // No implicit trailing newline on either side: an anchor at end-of-file
        // must be expressible.
        let text = "<<<EDIT a.txt\nline1\nline2\n===\nonly\n>>>";
        assert_eq!(
            parse_actions(text),
            vec![Action::Edit {
                path: "a.txt".into(),
                old: "line1\nline2".into(),
                new: "only".into(),
            }]
        );
    }

    #[test]
    fn edit_replacement_may_be_empty_meaning_deletion() {
        let text = "<<<EDIT a.txt\ndrop me\n===\n>>>";
        assert_eq!(
            parse_actions(text),
            vec![Action::Edit { path: "a.txt".into(), old: "drop me".into(), new: "".into() }]
        );
    }

    #[test]
    fn an_edit_without_a_separator_yields_no_action() {
        // Pinning the documented choice: a malformed edit produces nothing —
        // guessing at a partial edit risks a wrong write, which is worse than a
        // no-op step. The actions around it must survive.
        let text = "<<<EDIT a.txt\nno separator here\n>>>\n<<<RUN check\n>>>";
        assert_eq!(parse_actions(text), vec![Action::Run { name: "check".into() }]);
    }

    #[test]
    fn edit_splits_on_the_first_separator_only() {
        // A second `===` line belongs to the replacement text.
        let text = "<<<EDIT a.txt\nold\n===\nnew\n===\nmore\n>>>";
        assert_eq!(
            parse_actions(text),
            vec![Action::Edit {
                path: "a.txt".into(),
                old: "old".into(),
                new: "new\n===\nmore".into(),
            }]
        );
    }

    #[test]
    fn parses_a_read() {
        assert_eq!(
            parse_actions("<<<READ src/lib.rs\n>>>"),
            vec![Action::Read { path: "src/lib.rs".into() }]
        );
    }

    #[test]
    fn a_self_closed_read_keeps_its_path_clean() {
        // Same hazard as the self-closed WRITE: `src/lib.rs>>>` is a path no
        // task declares.
        assert_eq!(
            parse_actions("<<<READ src/lib.rs>>>"),
            vec![Action::Read { path: "src/lib.rs".into() }]
        );
    }

    #[test]
    fn pin_takes_a_step_number_and_rejects_junk() {
        assert_eq!(parse_actions("<<<PIN 3\n>>>"), vec![Action::Pin { step: 3 }]);
        assert_eq!(parse_actions("<<<PIN 3 >>>"), vec![Action::Pin { step: 3 }]);
        assert!(
            parse_actions("<<<PIN three\n>>>").is_empty(),
            "a non-numeric target yields no action, not a guessed pin"
        );
    }

    #[test]
    fn read_ignores_body_lines_and_takes_only_the_path() {
        assert_eq!(
            parse_actions("<<<READ a.txt\nthis body is ignored\n>>>"),
            vec![Action::Read { path: "a.txt".into() }]
        );
    }

    #[test]
    fn parses_read_then_edit_in_order() {
        let text = "<<<READ a.txt\n>>>\n<<<EDIT a.txt\nx\n===\ny\n>>>";
        assert_eq!(
            parse_actions(text),
            vec![
                Action::Read { path: "a.txt".into() },
                Action::Edit { path: "a.txt".into(), old: "x".into(), new: "y".into() },
            ]
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

    #[test]
    fn done_parses_as_terminal_not_action() {
        let s = parse_step("<<<DONE fixed the port\n>>>\n");
        assert!(s.actions.is_empty(), "a terminal block is never an action");
        assert_eq!(s.terminal, Some(Terminal::Done { summary: "fixed the port".into() }));
        assert!(!s.native_markup);
        assert!(parse_actions("<<<DONE fixed the port\n>>>\n").is_empty());
    }

    #[test]
    fn blocked_reason_joins_header_and_body_and_may_be_empty() {
        let s = parse_step("<<<BLOCKED src/data.txt.bak\nis not WRITABLE\n>>>");
        assert_eq!(
            s.terminal,
            Some(Terminal::Blocked { reason: "src/data.txt.bak is not WRITABLE".into() })
        );
        assert_eq!(
            parse_step("<<<BLOCKED out of scope>>>").terminal,
            Some(Terminal::Blocked { reason: "out of scope".into() })
        );
        assert_eq!(
            parse_step("<<<BLOCKED\n>>>").terminal,
            Some(Terminal::Blocked { reason: String::new() })
        );
    }

    #[test]
    fn a_terminal_word_must_end_at_a_boundary() {
        for text in ["<<<DONEX\n<<<RUN check\n>>>", "<<<BLOCKEDNESS\n<<<RUN check\n>>>"] {
            let s = parse_step(text);
            assert_eq!(s.terminal, None);
            assert_eq!(s.actions, vec![Action::Run { name: "check".into() }], "{text:?}");
        }
    }

    #[test]
    fn a_closer_right_after_the_keyword_closes_the_block() {
        for (text, summary) in [
            ("<<<DONE>\n<<<RUN check\n>>>", ""),
            ("<<<DONE>>\n<<<RUN check\n>>>", ""),
            ("<<<DONE>>>\n<<<RUN check\n>>>", ""),
            ("<<<DONE>>> thanks\n<<<RUN check\n>>>", "thanks"),
        ] {
            let s = parse_step(text);
            assert_eq!(s.terminal, Some(Terminal::Done { summary: summary.into() }), "{text:?}");
            assert_eq!(s.actions, vec![Action::Run { name: "check".into() }], "{text:?}");
        }
    }

    #[test]
    fn native_markup_does_not_change_what_parse_actions_returns() {
        // The driver still calls parse_actions; with no terminal block present
        // its output must be exactly what it was before parse_step existed.
        assert_eq!(
            parse_actions("<invoke name=\"x\">\n<<<RUN check\n>>>\n</invoke>"),
            vec![Action::Run { name: "check".into() }]
        );
        assert_eq!(
            parse_actions("<<<WRITE a.txt\n<invoke x>\n>>>"),
            vec![Action::Write { path: "a.txt".into(), content: "<invoke x>\n".into() }],
            "a markup-looking line inside a body stays body content"
        );
    }

    #[test]
    fn actions_before_a_terminal_are_kept_and_the_first_terminal_wins() {
        let s = parse_step("<<<WRITE a.txt\nfix\n>>>\n<<<DONE ok\n>>>\n<<<BLOCKED late\n>>>");
        assert_eq!(
            s.actions,
            vec![Action::Write { path: "a.txt".into(), content: "fix\n".into() }]
        );
        assert_eq!(s.terminal, Some(Terminal::Done { summary: "ok".into() }));
    }

    #[test]
    fn a_terminal_body_is_not_scanned_for_actions() {
        let s = parse_step("<<<DONE\n<<<WRITE a.txt\n>>>");
        assert!(s.actions.is_empty());
        assert_eq!(s.terminal, Some(Terminal::Done { summary: "<<<WRITE a.txt".into() }));
    }

    #[test]
    fn native_markup_is_detected_and_never_parsed() {
        // Verbatim shapes from the 2026-10-13 Sonnet 5.5 report (#59).
        let s = parse_step("<invoke name=\"READ\">\ntests/it.rs\n</invoke>\n\n<invoke name=\"RUN\">\ncheck\n</invoke>");
        assert!(s.native_markup);
        assert!(s.actions.is_empty(), "markup is detected, not mapped to an action");
        assert_eq!(s.terminal, None);
        assert!(parse_step("<invoke name=\"x\">\n</invoke>").native_markup);
        assert!(parse_step("<function_calls>\n<invoke name=\"READ\">").native_markup);
    }

    #[test]
    fn protocol_text_alone_is_not_native_markup() {
        assert!(!parse_step("<<<READ a.txt\n>>>").native_markup);
        assert!(!parse_step("BLOCKER: only src/data.txt is WRITABLE").native_markup);
        assert!(!parse_step("").native_markup);
    }
}
