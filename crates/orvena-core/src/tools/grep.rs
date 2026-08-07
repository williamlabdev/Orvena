//! Read-only content search. Pure Rust (`regex` + `ignore` + `globset`) — never
//! shells out to the system `grep`. Role-gated like `fs.rs` (tool name:
//! `grep.search`); no scope is needed because the tool cannot write. The walk
//! stays inside `root`: symlinks are not followed, `.git/` / `target/` are
//! skipped, and a glob search path is confined the same way a literal one is.

use super::Tool;
use crate::config::roles::Role;
use crate::error::{Error, Result};
use globset::{GlobBuilder, GlobMatcher};
use regex::Regex;
use std::path::{Component, Path, PathBuf};

/// Hits are capped so a broad pattern cannot flood the next step's context.
/// Callers can tell a capped result from an exhaustive one by comparing
/// `hits.len()` against this.
pub const MAX_HITS: usize = 200;

/// What makes a search path a pattern rather than a location. `[` and `{` are
/// included because globset treats them as syntax: a path carrying one and not
/// compiled as a glob would silently match nothing.
const GLOB_META: &[char] = &['*', '?', '[', '{'];

fn is_glob(s: &str) -> bool {
    s.contains(GLOB_META)
}

/// Compile a search path into a matcher over root-relative paths.
///
/// `literal_separator(true)` keeps shell semantics: `svc/*.conf` matches the
/// direct children of `svc/` and nothing deeper — the model that wrote it meant
/// the same thing its shell would have done. `svc/**/*.conf` is how you recurse.
fn compile_glob(rel: &str) -> Result<GlobMatcher> {
    GlobBuilder::new(rel)
        .literal_separator(true)
        .build()
        .map(|g| g.compile_matcher())
        .map_err(|e| Error::Other(anyhow::anyhow!("invalid search path '{rel}': {e}")))
}

/// The deepest leading run of literal components — where the walk starts, so a
/// glob costs the subtree it names and not the whole repository.
fn literal_prefix(rel: &Path) -> PathBuf {
    let mut prefix = PathBuf::new();
    for c in rel.components() {
        match c {
            Component::Normal(s) if !is_glob(&s.to_string_lossy()) => prefix.push(s),
            _ => break,
        }
    }
    prefix
}

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

        let mut selector = None;
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
                // A model reaching for `svc/*.conf` is applying a shell habit, not
                // making a mistake. Rejecting it costs a whole step of an 8-step
                // budget and the loop has no way to learn the accepted form from
                // "does not exist" — so accept the glob instead of teaching it.
                let base = if is_glob(rel) {
                    selector = Some(compile_glob(rel)?);
                    self.root.join(literal_prefix(rel_path))
                } else {
                    self.root.join(rel_path)
                };
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

        let mut files_selected = 0usize;
        'files: for entry in walk.flatten() {
            if !entry.file_type().is_some_and(|t| t.is_file()) {
                continue;
            }
            let rel = entry
                .path()
                .strip_prefix(&self.root)
                .unwrap_or(entry.path())
                .to_string_lossy()
                .replace('\\', "/");
            if let Some(sel) = &selector {
                if !sel.is_match(rel.as_str()) {
                    continue;
                }
            }
            files_selected += 1;
            // Binary / non-UTF-8 files are skipped, not errors.
            let Ok(content) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
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
        // Same invariant as a missing path, one level down: a glob that selected
        // nothing is a typo, and reporting it as "no match" would send the loop
        // hunting for a pattern that was never searched for.
        if selector.is_some() && files_selected == 0 {
            let rel = path.unwrap_or_default();
            return Err(Error::Other(anyhow::anyhow!("search path '{rel}' matched no files")));
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

    // ── slice-027: the search path accepts a glob ──────────────────────────
    // Measured, not assumed: every one of qwen3:14b's failures on the
    // search-scale probe died emitting `svc/*.conf` and burning the budget on
    // "does not exist". The shell habit is the model's prior; the tool meets it.

    /// A workspace shaped like the probe: same-suffix siblings plus a decoy.
    fn glob_root(tag: &str) -> PathBuf {
        let dir = temp_root(tag);
        std::fs::create_dir_all(dir.join("svc/nested")).unwrap();
        std::fs::write(dir.join("svc/one.conf"), "retention = TODO-one\n").unwrap();
        std::fs::write(dir.join("svc/two.conf"), "retention = TODO-two\n").unwrap();
        std::fs::write(dir.join("svc/notes.txt"), "TODO-decoy\n").unwrap();
        std::fs::write(dir.join("svc/nested/deep.conf"), "TODO-deep\n").unwrap();
        dir
    }

    #[test]
    fn a_glob_path_selects_the_files_it_names() {
        let root = glob_root("glob");
        let role = role(vec!["grep.search".into()]);
        let hits = GrepTool::new(&root, &role).search("TODO", Some("svc/*.conf")).unwrap();
        let paths: Vec<&str> = hits.iter().map(|h| h.path.as_str()).collect();
        assert_eq!(
            paths,
            vec!["svc/one.conf", "svc/two.conf"],
            "the .txt sibling is not selected, and `*` does not cross a separator"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_recursive_glob_is_how_you_cross_a_separator() {
        let root = glob_root("globrec");
        let role = role(vec!["grep.search".into()]);
        let hits = GrepTool::new(&root, &role).search("TODO", Some("svc/**/*.conf")).unwrap();
        let paths: Vec<&str> = hits.iter().map(|h| h.path.as_str()).collect();
        assert!(paths.contains(&"svc/nested/deep.conf"), "got {paths:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_glob_matching_no_file_is_an_error_not_zero_hits() {
        // The same invariant as a missing path: "no match" would send the loop
        // hunting for a pattern in files that were never searched.
        let root = glob_root("globmiss");
        let role = role(vec!["grep.search".into()]);
        let err = GrepTool::new(&root, &role).search("TODO", Some("svc/*.rs")).unwrap_err();
        assert!(err.to_string().contains("matched no files"), "{err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_glob_walks_only_the_subtree_it_names() {
        // `svc/*.conf` must not pay for the whole repository: the walk starts at
        // the deepest literal prefix. Observable as the decoy outside svc/ never
        // being read even though its name would match a looser glob.
        assert_eq!(literal_prefix(Path::new("svc/*.conf")), PathBuf::from("svc"));
        assert_eq!(literal_prefix(Path::new("a/b/c-*/d")), PathBuf::from("a/b"));
        assert_eq!(literal_prefix(Path::new("*.conf")), PathBuf::new());
    }

    #[test]
    fn a_glob_cannot_escape_the_root_either() {
        let root = glob_root("globescape");
        let role = role(vec!["grep.search".into()]);
        let err = GrepTool::new(&root, &role).search("TODO", Some("../*.conf")).unwrap_err();
        assert!(matches!(err, Error::Scope(_)), "{err}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_glob_whose_literal_prefix_is_missing_still_says_so() {
        let root = glob_root("globnodir");
        let role = role(vec!["grep.search".into()]);
        let err = GrepTool::new(&root, &role).search("TODO", Some("nope/*.conf")).unwrap_err();
        assert!(err.to_string().contains("does not exist"), "{err}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
