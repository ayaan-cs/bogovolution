//! Small git helpers. They only read git state and never touch the network.
//!
//! `.rules-evolve/` is meant to be committed so the lineage has git history,
//! so `init` checks that git does not ignore it.

use serde::Serialize;
use std::path::Path;
use std::process::Command;

/// Whether git would ignore a path.
#[derive(Debug, Clone, PartialEq)]
pub enum IgnoreCheck {
    /// Not ignored.
    NotIgnored,
    /// Ignored. The text is the matching rule, for example `.gitignore:3:.rules-evolve/`.
    Ignored(String),
    /// The directory is not inside a git repo.
    NotAGitRepo,
    /// The `git` program could not be run.
    GitUnavailable,
}

/// Ask git whether `rel_path` (relative to `dir`) is ignored.
pub fn check_ignored(dir: &Path, rel_path: &str) -> IgnoreCheck {
    let out = match Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["check-ignore", "-v", "--", rel_path])
        .output()
    {
        Ok(o) => o,
        Err(_) => return IgnoreCheck::GitUnavailable,
    };
    match out.status.code() {
        Some(0) => {
            let text = String::from_utf8_lossy(&out.stdout);
            let rule = text.split('\t').next().unwrap_or("").trim().to_string();
            IgnoreCheck::Ignored(rule)
        }
        Some(1) => IgnoreCheck::NotIgnored,
        _ => IgnoreCheck::NotAGitRepo,
    }
}

/// What `init` found and did about gitignore.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct GitignoreReport {
    /// `ok`, `fixed`, `still_ignored`, `not_a_git_repo`, or `git_unavailable`.
    pub status: String,
    /// The ignore rule that matched before the fix, if any.
    pub rule: Option<String>,
    /// Human readable detail.
    pub detail: String,
}

const NEGATION: &str = "!.rules-evolve/";

/// Make sure `.rules-evolve/` is not ignored. If it is, add a negation rule to
/// `<dir>/.gitignore` and check again.
pub fn ensure_not_ignored(dir: &Path) -> crate::Result<GitignoreReport> {
    let probe = ".rules-evolve/log.jsonl";
    match check_ignored(dir, probe) {
        IgnoreCheck::NotAGitRepo => Ok(GitignoreReport {
            status: "not_a_git_repo".into(),
            rule: None,
            detail: "not inside a git repo; commit .rules-evolve/ once you have one".into(),
        }),
        IgnoreCheck::GitUnavailable => Ok(GitignoreReport {
            status: "git_unavailable".into(),
            rule: None,
            detail: "git could not be run, so ignore rules were not checked".into(),
        }),
        IgnoreCheck::NotIgnored => Ok(GitignoreReport {
            status: "ok".into(),
            rule: None,
            detail: ".rules-evolve/ is not ignored; commit it".into(),
        }),
        IgnoreCheck::Ignored(rule) => {
            let path = dir.join(".gitignore");
            let mut text = std::fs::read_to_string(&path).unwrap_or_default();
            if !text.is_empty() && !text.ends_with('\n') {
                text.push('\n');
            }
            text.push_str("# rules-evolve: keep the lineage in git history\n");
            text.push_str(NEGATION);
            text.push('\n');
            std::fs::write(&path, text).map_err(crate::error::io_err(&path))?;
            match check_ignored(dir, probe) {
                IgnoreCheck::Ignored(still) => Ok(GitignoreReport {
                    status: "still_ignored".into(),
                    rule: Some(rule),
                    detail: format!(
                        "added {NEGATION} to .gitignore but git still ignores .rules-evolve/ ({still}); fix this by hand"
                    ),
                }),
                _ => Ok(GitignoreReport {
                    status: "fixed".into(),
                    rule: Some(rule.clone()),
                    detail: format!("{rule} ignored .rules-evolve/, so {NEGATION} was added to .gitignore"),
                }),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn git_ok() -> bool {
        Command::new("git").arg("--version").output().is_ok()
    }

    fn git_init(dir: &Path) {
        assert!(Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["init", "-q"])
            .status()
            .unwrap()
            .success());
    }

    #[test]
    fn not_a_repo_is_reported() {
        if !git_ok() {
            return;
        }
        let d = tempdir().unwrap();
        let r = ensure_not_ignored(d.path()).unwrap();
        assert_eq!(r.status, "not_a_git_repo");
    }

    #[test]
    fn plain_repo_is_ok() {
        if !git_ok() {
            return;
        }
        let d = tempdir().unwrap();
        git_init(d.path());
        assert_eq!(ensure_not_ignored(d.path()).unwrap().status, "ok");
    }

    #[test]
    fn ignored_dir_gets_negation_rule() {
        if !git_ok() {
            return;
        }
        let d = tempdir().unwrap();
        git_init(d.path());
        std::fs::write(d.path().join(".gitignore"), "target\n.rules-evolve/").unwrap();
        let r = ensure_not_ignored(d.path()).unwrap();
        assert_eq!(r.status, "fixed");
        let text = std::fs::read_to_string(d.path().join(".gitignore")).unwrap();
        assert!(text.contains("target\n.rules-evolve/\n"));
        assert!(text.contains("!.rules-evolve/"));
        assert_eq!(
            check_ignored(d.path(), ".rules-evolve/log.jsonl"),
            IgnoreCheck::NotIgnored
        );
    }

    #[test]
    fn dotfile_wildcard_is_also_fixed() {
        if !git_ok() {
            return;
        }
        let d = tempdir().unwrap();
        git_init(d.path());
        std::fs::write(d.path().join(".gitignore"), ".*\n").unwrap();
        assert_eq!(ensure_not_ignored(d.path()).unwrap().status, "fixed");
    }
}
