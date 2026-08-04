//! Filesystem tool. Every write passes two checks: the role must allow `fs.write`
//! (Specialized Roles) and the scope must mark the path writable (Bounded Change).
//! Reads are likewise role-gated. Tool names: `fs.read`, `fs.write`, `fs.list`.

use super::Tool;
use crate::config::roles::Role;
use crate::error::{Error, Result};
use crate::governance::scope::{Scope, ScopeDecision};
use std::path::{Component, Path, PathBuf};

pub struct FsTool<'a> {
    pub root: PathBuf,
    pub scope: &'a Scope,
    pub role: &'a Role,
}

impl<'a> FsTool<'a> {
    pub fn new(root: impl Into<PathBuf>, scope: &'a Scope, role: &'a Role) -> Self {
        Self { root: root.into(), scope, role }
    }

    pub fn read(&self, rel: &str) -> Result<String> {
        self.require_tool("fs.read")?;
        // Contained like writes since slice-020 exposed READ to the model: a
        // `../` or through-symlink path would otherwise read outside the root —
        // reads don't mutate, but exfiltrating file contents into the model's
        // context is an escape all the same.
        Ok(std::fs::read_to_string(self.resolve_in_root(rel)?)?)
    }

    /// Read a file, returning `None` if it does not exist yet (for new files).
    pub fn read_opt(&self, rel: &str) -> Result<Option<String>> {
        self.require_tool("fs.read")?;
        let p = self.resolve_in_root(rel)?;
        if !p.exists() {
            return Ok(None);
        }
        Ok(Some(std::fs::read_to_string(p)?))
    }

    /// Resolve `rel` to a target path guaranteed to stay within the project root,
    /// or reject it. This is a hard boundary enforced *regardless of tier*: an
    /// access that escapes the root is never acceptable, even in advisory
    /// `light` — for writes since always, for reads since READ became a model
    /// action (slice-020). Parity with `grep.rs`: reject absolute paths and any
    /// `..` component; and, beyond grep, resolve symlinks — the nearest existing
    /// ancestor must canonicalize to within the canonicalized root, so a symlink
    /// inside the root cannot redirect an access outside it.
    fn resolve_in_root(&self, rel: &str) -> Result<PathBuf> {
        let rel_path = Path::new(rel);
        if rel_path.is_absolute()
            || rel_path.components().any(|c| matches!(c, Component::ParentDir))
        {
            return Err(Error::Scope(format!("path '{rel}' escapes the project root")));
        }
        let target = self.root.join(rel_path);
        let root_canon = self.root.canonicalize().unwrap_or_else(|_| self.root.clone());
        // The target file may not exist yet; walk up to the nearest existing
        // ancestor and canonicalize that (which resolves any symlink on the way).
        let mut probe: &Path = &target;
        loop {
            match probe.canonicalize() {
                Ok(canon) => {
                    if !canon.starts_with(&root_canon) {
                        return Err(Error::Scope(format!(
                            "path '{rel}' resolves outside the project root"
                        )));
                    }
                    break;
                }
                Err(_) => match probe.parent() {
                    Some(parent) if !parent.as_os_str().is_empty() => probe = parent,
                    _ => break,
                },
            }
        }
        Ok(target)
    }

    pub fn write(&self, rel: &str, content: &str) -> Result<()> {
        self.require_tool("fs.write")?;
        // Boundary check first: an escaping path is refused before scope patterns
        // are even consulted, so it can never be written no matter the tier.
        let target = self.resolve_in_root(rel)?;
        match self.scope.decision(rel) {
            ScopeDecision::Allow => {
                if let Some(parent) = target.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(target, content)?;
                Ok(())
            }
            ScopeDecision::ReadOnly => Err(Error::Scope(format!(
                "'{rel}' is read-only (not in allowed_modifications) — report a blocker, \
                 do not expand scope"
            ))),
            ScopeDecision::Excluded => {
                Err(Error::Scope(format!("'{rel}' is excluded from this task's scope")))
            }
        }
    }

    /// Anchored replace (slice-020): `old` must appear **exactly once** in the
    /// file; it is replaced by `new`. Same authorization path as `write` —
    /// role-gated `fs.write` plus the scope decision — because an edit *is* a
    /// write. The file's current content is read internally without the
    /// `fs.read` gate, and none of it may appear in an error message: a role
    /// allowed to write but not read must not be able to use failed edits as a
    /// read side-channel.
    pub fn edit(&self, rel: &str, old: &str, new: &str) -> Result<()> {
        self.require_tool("fs.write")?;
        let target = self.resolve_in_root(rel)?;
        match self.scope.decision(rel) {
            ScopeDecision::Allow => {}
            ScopeDecision::ReadOnly => {
                return Err(Error::Scope(format!(
                    "'{rel}' is read-only (not in allowed_modifications) — report a blocker, \
                     do not expand scope"
                )));
            }
            ScopeDecision::Excluded => {
                return Err(Error::Scope(format!("'{rel}' is excluded from this task's scope")));
            }
        }
        if old.is_empty() {
            return Err(Error::Other(anyhow::anyhow!(
                "EDIT '{rel}': the text to replace is empty — it would match everywhere"
            )));
        }
        if !target.exists() {
            return Err(Error::Other(anyhow::anyhow!(
                "EDIT '{rel}': the file does not exist — use WRITE to create it"
            )));
        }
        let current = std::fs::read_to_string(&target)?;
        match current.matches(old).count() {
            1 => {
                std::fs::write(target, current.replacen(old, new, 1))?;
                Ok(())
            }
            0 => Err(Error::Other(anyhow::anyhow!(
                "EDIT '{rel}': the text to replace was not found — READ the file and \
                 anchor on its exact current content"
            ))),
            n => Err(Error::Other(anyhow::anyhow!(
                "EDIT '{rel}': the text to replace appears {n} times — include enough \
                 surrounding lines to make it unique"
            ))),
        }
    }

    fn require_tool(&self, tool: &str) -> Result<()> {
        if self.role.tool_allowed(tool) {
            Ok(())
        } else {
            Err(Error::Scope(format!("role '{}' is not allowed to use '{tool}'", self.role.name)))
        }
    }
}

impl<'a> Tool for FsTool<'a> {
    fn name(&self) -> &str {
        "fs"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::agent::Tier;

    fn role() -> Role {
        Role {
            name: "tester".into(),
            allowed_tools: vec!["fs.write".into(), "fs.read".into()],
            forbidden_tools: vec![],
            knowledge_scope: vec![],
        }
    }

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("orvena-fs-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("src")).unwrap();
        dir
    }

    #[test]
    fn write_inside_allowed_scope_succeeds() {
        let root = temp_root("ok");
        let scope = Scope::new(vec!["src".into()], vec![], Tier::Light);
        let role = role();
        let fs = FsTool::new(&root, &scope, &role);
        fs.write("src/ok.txt", "hello").unwrap();
        assert_eq!(std::fs::read_to_string(root.join("src/ok.txt")).unwrap(), "hello");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn edit_replaces_a_unique_anchor() {
        let root = temp_root("edit-ok");
        std::fs::write(root.join("src/a.rs"), "let x = 1;\nlet y = 2;\n").unwrap();
        let scope = Scope::new(vec!["src".into()], vec![], Tier::Light);
        let role = role();
        let fs = FsTool::new(&root, &scope, &role);
        fs.edit("src/a.rs", "let x = 1;", "let x = 9;").unwrap();
        assert_eq!(
            std::fs::read_to_string(root.join("src/a.rs")).unwrap(),
            "let x = 9;\nlet y = 2;\n"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn edit_failures_name_the_problem_but_never_the_content() {
        // 0 matches, N matches, empty anchor, missing file — each is a distinct,
        // actionable error, and none may echo what the file contains: a role
        // with fs.write but not fs.read must not read via failed edits.
        let root = temp_root("edit-fail");
        let secret = "SECRET-CONTENT-marker\nSECRET-CONTENT-marker\n";
        std::fs::write(root.join("src/a.rs"), secret).unwrap();
        let scope = Scope::new(vec!["src".into()], vec![], Tier::Light);
        let role = role();
        let fs = FsTool::new(&root, &scope, &role);

        let cases = [
            ("src/a.rs", "not present", "not found"),
            ("src/a.rs", "SECRET-CONTENT-marker", "2 times"),
            ("src/a.rs", "", "empty"),
            ("src/missing.rs", "x", "does not exist"),
        ];
        for (path, old, expect) in cases {
            let err = fs.edit(path, old, "replacement").unwrap_err().to_string();
            assert!(err.contains(expect), "expected '{expect}' in: {err}");
            assert!(!err.contains("SECRET-CONTENT"), "content leaked: {err}");
        }
        // None of the failures touched the file.
        assert_eq!(std::fs::read_to_string(root.join("src/a.rs")).unwrap(), secret);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn edit_outside_scope_is_a_scope_error() {
        let root = temp_root("edit-scope");
        std::fs::write(root.join("src/a.rs"), "x\n").unwrap();
        let scope = Scope::new(vec!["docs".into()], vec![], Tier::Light);
        let role = role();
        let fs = FsTool::new(&root, &scope, &role);
        let err = fs.edit("src/a.rs", "x", "y").unwrap_err();
        assert!(matches!(err, Error::Scope(_)), "expected a scope error, got {err:?}");
        assert_eq!(std::fs::read_to_string(root.join("src/a.rs")).unwrap(), "x\n");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn read_escaping_root_is_blocked() {
        // READ is a model action since slice-020; a `../` path must not read
        // outside the root even though nothing is written.
        let root = temp_root("read-escape");
        let scope = Scope::new(vec!["src".into()], vec![], Tier::Light);
        let role = role();
        let fs = FsTool::new(&root, &scope, &role);
        let err = fs.read("../read-escape-target.txt").unwrap_err();
        assert!(matches!(err, Error::Scope(_)), "expected a scope error, got {err:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn write_escaping_root_via_dotdot_is_blocked() {
        let root = temp_root("dotdot");
        // Allow-list "src" would prefix-match the escaping path under the old
        // string compare; the boundary guard must still reject it.
        let scope = Scope::new(vec!["src".into()], vec![], Tier::Light);
        let role = role();
        let fs = FsTool::new(&root, &scope, &role);

        let sentinel = root.parent().unwrap().join("orvena-escape-sentinel.txt");
        let _ = std::fs::remove_file(&sentinel);
        let err = fs.write("src/../../orvena-escape-sentinel.txt", "pwned").unwrap_err();
        assert!(matches!(err, Error::Scope(_)), "expected a scope error, got {err:?}");
        assert!(!sentinel.exists(), "escaping write must not create a file outside root");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn write_escaping_root_via_symlink_is_blocked() {
        let root = temp_root("symlink");
        // A symlink inside the root that points outside it must not become a
        // write conduit out of the project.
        let outside = std::env::temp_dir().join(format!("orvena-outside-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&outside);
        std::fs::create_dir_all(&outside).unwrap();
        let link = root.join("src/link");
        let _ = std::fs::remove_file(&link);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &link).unwrap();

        let scope = Scope::new(vec!["src".into()], vec![], Tier::Light);
        let role = role();
        let fs = FsTool::new(&root, &scope, &role);

        #[cfg(unix)]
        {
            let err = fs.write("src/link/evil.txt", "pwned").unwrap_err();
            assert!(matches!(err, Error::Scope(_)), "expected a scope error, got {err:?}");
            assert!(
                !outside.join("evil.txt").exists(),
                "symlinked write must not land outside root"
            );
        }
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }
}
