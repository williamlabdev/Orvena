//! Read-only content search. Pure Rust (`regex` + `ignore`) — never shells out
//! to the system `grep`. Role-gated like `fs.rs` (tool name: `grep.search`);
//! no scope is needed because the tool cannot write. The walk stays inside
//! `root`: symlinks are not followed, and `.git/` / `target/` are skipped.

use super::Tool;
use crate::config::roles::Role;
use crate::error::{Error, Result};
use regex::Regex;
use std::path::{Component, Path, PathBuf};

/// Hits are capped so a broad pattern cannot flood the next step's context.
/// Callers can tell a capped result from an exhaustive one by comparing
/// `hits.len()` against this.
pub const MAX_HITS: usize = 200;

/// One matching line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// Path relative to the project root.
    pub path: String,
    /// 1-based line number.
    pub line_no: usize,
    pub text: String,
}

pub struct GrepTool<'a> {
    pub root: PathBuf,
    pub role: &'a Role,
}

impl<'a> GrepTool<'a> {
    pub fn new(root: impl Into<PathBuf>, role: &'a Role) -> Self {
        Self { root: root.into(), role }
    }

    /// Search file contents under `root` (or under `path`, when given) for
    /// lines matching `pattern`. Returns at most [`MAX_HITS`] hits, in
    /// deterministic (path-sorted) order.
    pub fn search(&self, pattern: &str, path: Option<&str>) -> Result<Vec<Hit>> {
        self.require_tool("grep.search")?;

        let re = Regex::new(pattern)
            .map_err(|e| Error::Other(anyhow::anyhow!("invalid search pattern: {e}")))?;

        let base = match path {
            Some(rel) => {
                let rel_path = Path::new(rel);
                if rel_path.is_absolute()
                    || rel_path.components().any(|c| matches!(c, Component::ParentDir))
                {
                    return Err(Error::Scope(format!(
                        "search path '{rel}' escapes the project root"
                    )));
                }
                let base = self.root.join(rel_path);
                // A missing path must be a visible error, not "0 hits" — the
                // model needs to distinguish a typo from a genuine no-match.
                if !base.exists() {
                    return Err(Error::Other(anyhow::anyhow!(
                        "search path '{rel}' does not exist"
                    )));
                }
                base
            }
            None => self.root.clone(),
        };

        let mut hits = Vec::new();
        // `hidden(false)`: dotfiles (e.g. `.orvena/`, `.github/`) are legitimate
        // search targets; only `.git/` and `target/` are excluded.
        let walk = ignore::WalkBuilder::new(&base)
            .follow_links(false)
            .hidden(false)
            .sort_by_file_path(std::cmp::Ord::cmp)
            .filter_entry(|e| {
                let name = e.file_name().to_string_lossy();
                name != ".git" && name != "target"
            })
            .build();

        'files: for entry in walk.flatten() {
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            // Binary / non-UTF-8 files are skipped, not errors.
            let Ok(content) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            let rel = entry
                .path()
                .strip_prefix(&self.root)
                .unwrap_or(entry.path())
                .to_string_lossy()
                .replace('\\', "/");
            for (idx, line) in content.lines().enumerate() {
                if re.is_match(line) {
                    hits.push(Hit {
                        path: rel.clone(),
                        line_no: idx + 1,
                        text: line.trim_end().to_string(),
                    });
                    if hits.len() >= MAX_HITS {
                        break 'files;
                    }
                }
            }
        }
        Ok(hits)
    }

    fn require_tool(&self, tool: &str) -> Result<()> {
        if self.role.tool_allowed(tool) {
            Ok(())
        } else {
            Err(Error::Scope(format!("role '{}' is not allowed to use '{tool}'", self.role.name)))
        }
    }
}

impl<'a> Tool for GrepTool<'a> {
    fn name(&self) -> &str {
        "grep"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn role(allowed: Vec<String>) -> Role {
        Role {
            name: "tester".into(),
            allowed_tools: allowed,
            forbidden_tools: vec![],
            knowledge_scope: vec![],
        }
    }

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("orvena-grep-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/a.txt"), "alpha\nTODO: fix me\n").unwrap();
        std::fs::write(dir.join("b.txt"), "beta TODO\n").unwrap();
        // Hidden dirs are legitimate search targets (config lives there)…
        std::fs::create_dir_all(dir.join(".orvena")).unwrap();
        std::fs::write(dir.join(".orvena/cfg.yaml"), "TODO in hidden config\n").unwrap();
        // …but `.git/` and `target/` must never be searched.
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join(".git/HEAD"), "TODO in git internals\n").unwrap();
        std::fs::create_dir_all(dir.join("target")).unwrap();
        std::fs::write(dir.join("target/gen.txt"), "TODO in build output\n").unwrap();
        dir
    }

    #[test]
    fn finds_hits_including_hidden_but_skips_git_and_target() {
        let root = temp_root("hits");
        let role = role(vec!["grep.search".into()]);
        let hits = GrepTool::new(&root, &role).search("TODO", None).unwrap();
        let paths: Vec<&str> = hits.iter().map(|h| h.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![".orvena/cfg.yaml", "b.txt", "src/a.txt"],
            "sorted; hidden dirs searched; .git/ and target/ excluded"
        );
        assert_eq!(hits[2].line_no, 2);
        assert_eq!(hits[2].text, "TODO: fix me");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn scoped_to_a_subpath() {
        let root = temp_root("scoped");
        let role = role(vec!["grep.search".into()]);
        let hits = GrepTool::new(&root, &role).search("TODO", Some("src")).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "src/a.txt");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn no_hits_is_ok_and_empty() {
        let root = temp_root("empty");
        let role = role(vec!["grep.search".into()]);
        let hits = GrepTool::new(&root, &role).search("no-such-token", None).unwrap();
        assert!(hits.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn invalid_regex_is_an_error_not_a_panic() {
        let root = temp_root("badre");
        let role = role(vec!["grep.search".into()]);
        let err = GrepTool::new(&root, &role).search("(unclosed", None).unwrap_err();
        assert!(err.to_string().contains("invalid search pattern"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn role_without_grep_search_is_denied_with_scope_error() {
        let root = temp_root("denied");
        let role = role(vec!["fs.read".into()]);
        let err = GrepTool::new(&root, &role).search("TODO", None).unwrap_err();
        assert!(matches!(err, Error::Scope(_)));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn nonexistent_path_is_an_error_not_zero_hits() {
        let root = temp_root("nopath");
        let role = role(vec!["grep.search".into()]);
        let err = GrepTool::new(&root, &role).search("TODO", Some("no/such/dir")).unwrap_err();
        assert!(err.to_string().contains("does not exist"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn path_escaping_root_is_rejected() {
        let root = temp_root("escape");
        let role = role(vec!["grep.search".into()]);
        let err = GrepTool::new(&root, &role).search("TODO", Some("../outside")).unwrap_err();
        assert!(matches!(err, Error::Scope(_)));
        let _ = std::fs::remove_dir_all(&root);
    }
}
