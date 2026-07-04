//! Per-step parsing: the model speaks a tiny action protocol so the loop can
//! apply changes deterministically. v0.1 supports two actions — writing a file
//! in full, and a read-only content search:
//!
//! ```text
//! <<<WRITE relative/path
//! <full new file content>
//! >>>
//!
//! <<<SEARCH <regex pattern>
//! [optional relative path to limit the search]
//! >>>
//! ```

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Write { path: String, content: String },
    Search { pattern: String, path: Option<String> },
}

/// Parse zero or more `<<<WRITE …>>>` / `<<<SEARCH …>>>` blocks from a model
/// response.
pub fn parse_actions(text: &str) -> Vec<Action> {
    let mut actions = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        if let Some(path) = trimmed.strip_prefix("<<<WRITE ") {
            let path = path.trim().to_string();
            let mut content_lines = Vec::new();
            for body in lines.by_ref() {
                if body.trim() == ">>>" {
                    break;
                }
                content_lines.push(body);
            }
            let mut content = content_lines.join("\n");
            if !content.is_empty() {
                content.push('\n');
            }
            actions.push(Action::Write { path, content });
        } else if let Some(pattern) = trimmed.strip_prefix("<<<SEARCH ") {
            let pattern = pattern.trim().to_string();
            // The first non-empty body line (if any) narrows the search path.
            let mut path = None;
            for body in lines.by_ref() {
                let body = body.trim();
                if body == ">>>" {
                    break;
                }
                if path.is_none() && !body.is_empty() {
                    path = Some(body.to_string());
                }
            }
            if !pattern.is_empty() {
                actions.push(Action::Search { pattern, path });
            }
        }
    }
    actions
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
            vec![Action::Write {
                path: "src/a.txt".into(),
                content: "hello\nworld\n".into()
            }]
        );
    }

    #[test]
    fn parses_none_when_absent() {
        assert!(parse_actions("just prose, no actions").is_empty());
    }

    #[test]
    fn parses_a_search_without_path() {
        let actions = parse_actions("<<<SEARCH fn main\n>>>");
        assert_eq!(
            actions,
            vec![Action::Search { pattern: "fn main".into(), path: None }]
        );
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
}
