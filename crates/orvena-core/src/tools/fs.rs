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
        Ok(std::fs::read_to_string(self.root.join(rel))?)
    }

    /// Read a file, returning `None` if it does not exist yet (for new files).
    pub fn read_opt(&self, rel: &str) -> Result<Option<String>> {
        self.require_tool("fs.read")?;
        let p = self.root.join(rel);
        if !p.exists() {
            return Ok(None);
        }
        Ok(Some(std::fs::read_to_string(p)?))
    }

    /// Resolve `rel` to a target path guaranteed to stay within the project root,
    /// or reject it. This is a hard boundary enforced *regardless of tier*: a
    /// write that escapes the root is never acceptable, even in advisory `light`.
    /// Parity with `grep.rs`: reject absolute paths and any `..` component; and,
    /// beyond grep, resolve symlinks — the nearest existing ancestor must
    /// canonicalize to within the canonicalized root, so a symlink inside the
    /// root cannot redirect a write outside it.
    fn resolve_in_root(&self, rel: &str) -> Result<PathBuf> {
        let rel_path = Path::new(rel);
        if rel_path.is_absolute()
            || rel_path.components().any(|c| matches!(c, Component::ParentDir))
        {
            return Err(Error::Scope(format!("write path '{rel}' escapes the project root")));
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
                            "write path '{rel}' resolves outside the project root"
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
