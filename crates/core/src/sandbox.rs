//! The sandboxed worktree for one task attempt.
//!
//! For every attempt the harness:
//!
//! 1. copies the repo into a fresh temp folder, leaving out the benchmark
//!    check files (`hidden`), `.git`, `.rules-evolve`, other excluded paths,
//!    and every symlink;
//! 2. writes the rules version under test into that copy;
//! 3. lets the model work from that copy (the model never gets the check
//!    command either);
//! 4. applies the model's edits, refusing any that touch hidden or protected
//!    paths;
//! 5. puts the hidden files back from pristine copies kept outside the
//!    worktree, removing whatever the model left at those paths (links are
//!    removed, never followed);
//! 6. runs the check command in the copy through an isolation
//!    [`Runner`](crate::isolation::Runner) (bwrap, docker, podman, or none),
//!    with a timeout and a cleared environment. The isolation backends mount
//!    the hidden paths read only from the pristine copies.
//!
//! Threat model, and its limits, are in the README. In short: steps 1 to 5 keep
//! a model from READING the tests while it produces its answer, and the runner
//! keeps code that runs during the check away from the rest of the machine. A
//! sandbox built with [`Sandbox::scan`] alone uses no isolation (`none`); the
//! benchmark harness always sets a runner chosen by the isolation policy.

use crate::error::{io_err, Result};
pub use crate::isolation::CheckStatus;
use crate::isolation::{Protected, ReadOnlyMount, Runner};
use crate::model::FileEdit;
use crate::tasks::SandboxConfig;
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

/// Folders that are never copied into a worktree.
const ALWAYS_EXCLUDED: [&str; 2] = [".git", ".rules-evolve"];

/// Largest single file a model may write.
pub const MAX_EDIT_BYTES: usize = 1_000_000;

fn segment_match(pat: &[u8], text: &[u8]) -> bool {
    match (pat.first(), text.first()) {
        (None, None) => true,
        (Some(b'*'), _) => {
            segment_match(&pat[1..], text) || (!text.is_empty() && segment_match(pat, &text[1..]))
        }
        (Some(b'?'), Some(_)) => segment_match(&pat[1..], &text[1..]),
        (Some(a), Some(b)) if a == b => segment_match(&pat[1..], &text[1..]),
        _ => false,
    }
}

fn segments_match(pat: &[&str], path: &[&str]) -> bool {
    match pat.first() {
        // The pattern is used up. It named this path or a folder above it.
        None => true,
        Some(&"**") => {
            segments_match(&pat[1..], path) || (!path.is_empty() && segments_match(pat, &path[1..]))
        }
        Some(p) => {
            !path.is_empty()
                && segment_match(p.as_bytes(), path[0].as_bytes())
                && segments_match(&pat[1..], &path[1..])
        }
    }
}

/// Does `pattern` name `rel_path`, or a folder above it? Both are relative to
/// the repo root and use `/`.
pub fn path_matches(pattern: &str, rel_path: &str) -> bool {
    let pattern = pattern.trim_start_matches("./").trim_end_matches('/');
    if pattern.is_empty() {
        return false;
    }
    let pat: Vec<&str> = pattern.split('/').collect();
    let path: Vec<&str> = rel_path.split('/').collect();
    segments_match(&pat, &path)
}

/// A list of files found in the repo, sorted into what the model may see and
/// what it may not.
#[derive(Debug, Clone)]
pub struct Sandbox {
    repo_dir: PathBuf,
    rules_file: String,
    hidden: Vec<String>,
    exclude: Vec<String>,
    env_passthrough: Vec<String>,
    visible_files: Vec<String>,
    hidden_files: Vec<String>,
    /// Top-most folders that match `hidden`. Everything in them is hidden.
    hidden_dirs: Vec<String>,
    /// Pristine copies of the hidden files, outside any worktree. The check
    /// sees these (read only under an isolation backend) and `restore_hidden`
    /// copies from here, so the original tests survive whatever the model did.
    pristine: Arc<TempDir>,
    protected: Protected,
    skipped_symlinks: usize,
    runner: Runner,
}

fn is_normal_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\0')
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

impl Sandbox {
    /// Walk the repo once and sort its files.
    pub fn scan(repo_dir: &Path, rules_file: &str, cfg: &SandboxConfig) -> Result<Sandbox> {
        let mut sb = Sandbox {
            repo_dir: repo_dir.to_path_buf(),
            rules_file: rules_file.to_string(),
            hidden: cfg.hidden.clone(),
            exclude: cfg.exclude.clone(),
            env_passthrough: cfg.env_passthrough.clone(),
            visible_files: Vec::new(),
            hidden_files: Vec::new(),
            hidden_dirs: Vec::new(),
            pristine: Arc::new(
                tempfile::Builder::new()
                    .prefix("rules-evolve-pristine-")
                    .tempdir()
                    .map_err(io_err(std::env::temp_dir()))?,
            ),
            protected: Protected::default(),
            skipped_symlinks: 0,
            runner: Runner::unsafe_none(),
        };
        sb.walk(repo_dir, "")?;
        sb.visible_files.sort();
        sb.hidden_files.sort();
        sb.hidden_dirs.sort();
        sb.keep_pristine_copies()?;
        sb.protected = sb.build_protected();
        Ok(sb)
    }

    /// Copy every hidden file (and make every hidden folder) into the pristine
    /// store, once, so later changes in a worktree or the repo cannot reach it.
    fn keep_pristine_copies(&self) -> Result<()> {
        let root = self.pristine.path();
        for rel in &self.hidden_files {
            copy_into(&self.repo_dir, root, rel)?;
        }
        for rel in &self.hidden_dirs {
            let d = root.join(rel);
            std::fs::create_dir_all(&d).map_err(io_err(&d))?;
        }
        Ok(())
    }

    fn under_hidden_dir(&self, rel: &str) -> bool {
        self.hidden_dirs
            .iter()
            .any(|d| rel == d || rel.starts_with(&format!("{d}/")))
    }

    /// The read only mounts and pinned folders for an isolation backend.
    fn build_protected(&self) -> Protected {
        let root = self.pristine.path();
        let mut entries: Vec<&String> = self.hidden_dirs.iter().collect();
        entries.extend(
            self.hidden_files
                .iter()
                .filter(|f| !self.under_hidden_dir(f)),
        );
        let mut pins: BTreeSet<(usize, String)> = BTreeSet::new();
        for rel in &entries {
            let parts: Vec<&str> = rel.split('/').collect();
            for n in 1..parts.len() {
                let anc = parts[..n].join("/");
                pins.insert((n, anc));
            }
        }
        let mut read_only: Vec<ReadOnlyMount> = entries
            .iter()
            .map(|rel| ReadOnlyMount {
                rel: (*rel).clone(),
                source: root.join(rel),
            })
            .collect();
        read_only.sort_by(|a, b| a.rel.cmp(&b.rel));
        Protected {
            pin_dirs: pins.into_iter().map(|(_, p)| p).collect(),
            read_only,
        }
    }

    /// Folders that match `hidden` (the top-most ones).
    pub fn hidden_dirs(&self) -> &[String] {
        &self.hidden_dirs
    }

    /// What a backend protects in the worktree during the check.
    pub fn protected(&self) -> &Protected {
        &self.protected
    }

    /// Run checks with this runner. Without it a sandbox uses no isolation,
    /// which is only for tests and the mock and replay providers.
    pub fn with_runner(mut self, runner: Runner) -> Sandbox {
        self.runner = runner;
        self
    }

    pub fn runner(&self) -> &Runner {
        &self.runner
    }

    fn is_excluded(&self, rel: &str) -> bool {
        ALWAYS_EXCLUDED.iter().any(|p| path_matches(p, rel))
            || self.exclude.iter().any(|p| path_matches(p, rel))
    }

    fn is_hidden(&self, rel: &str) -> bool {
        self.hidden.iter().any(|p| path_matches(p, rel))
    }

    fn walk(&mut self, dir: &Path, rel_dir: &str) -> Result<()> {
        let entries = std::fs::read_dir(dir).map_err(io_err(dir))?;
        for entry in entries {
            let entry = entry.map_err(io_err(dir))?;
            let name = entry.file_name().to_string_lossy().to_string();
            let rel = if rel_dir.is_empty() {
                name
            } else {
                format!("{rel_dir}/{name}")
            };
            let meta = std::fs::symlink_metadata(entry.path()).map_err(io_err(entry.path()))?;
            if meta.file_type().is_symlink() {
                self.skipped_symlinks += 1;
                continue;
            }
            if self.is_excluded(&rel) {
                continue;
            }
            if meta.is_dir() {
                if self.is_hidden(&rel) && !self.under_hidden_dir(&rel) {
                    self.hidden_dirs.push(rel.clone());
                }
                self.walk(&entry.path(), &rel)?;
            } else if meta.is_file() {
                if rel == self.rules_file {
                    continue; // written from the version under test
                }
                if self.is_hidden(&rel) {
                    self.hidden_files.push(rel);
                } else {
                    self.visible_files.push(rel);
                }
            }
        }
        Ok(())
    }

    /// Files the model's copy will contain (not counting the rules file).
    pub fn visible_files(&self) -> &[String] {
        &self.visible_files
    }

    /// Files held back from the model.
    pub fn hidden_files(&self) -> &[String] {
        &self.hidden_files
    }

    pub fn skipped_symlinks(&self) -> usize {
        self.skipped_symlinks
    }

    /// Build a fresh worktree with the given rules version in place.
    pub fn prepare(&self, rules_content: &[u8]) -> Result<Worktree<'_>> {
        let dir = tempfile::Builder::new()
            .prefix("rules-evolve-")
            .tempdir()
            .map_err(io_err(std::env::temp_dir()))?;
        for rel in &self.visible_files {
            copy_into(&self.repo_dir, dir.path(), rel)?;
        }
        let rules_path = dir.path().join(&self.rules_file);
        if let Some(parent) = rules_path.parent() {
            std::fs::create_dir_all(parent).map_err(io_err(parent))?;
        }
        std::fs::write(&rules_path, rules_content).map_err(io_err(&rules_path))?;
        Ok(Worktree { dir, sandbox: self })
    }

    /// Why a model may not write `path`, or `None` if it may.
    pub fn edit_violation(&self, path: &str) -> Option<String> {
        if !is_normal_relative(path) {
            return Some(format!(
                "path {path:?} must be relative with no .. or absolute parts"
            ));
        }
        if self.is_excluded(path) {
            return Some(format!("path {path:?} is protected"));
        }
        if self.is_hidden(path) {
            return Some(format!("path {path:?} is a benchmark file"));
        }
        if path == self.rules_file {
            return Some("the rules file under test cannot be edited by the model".into());
        }
        None
    }
}

fn copy_into(from_root: &Path, to_root: &Path, rel: &str) -> Result<()> {
    let from = from_root.join(rel);
    let to = to_root.join(rel);
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(io_err(parent))?;
    }
    std::fs::copy(&from, &to).map_err(io_err(&from))?;
    Ok(())
}

/// Make every folder above `rel` a real folder under `root`. A symlink or a
/// file in the way is removed first (a symlink is never followed).
fn ensure_real_parents(root: &Path, rel: &str) -> Result<()> {
    let parts: Vec<&str> = rel.split('/').collect();
    let mut cur = root.to_path_buf();
    for part in &parts[..parts.len().saturating_sub(1)] {
        cur.push(part);
        match std::fs::symlink_metadata(&cur) {
            Ok(m) if m.is_dir() => {}
            Ok(_) => {
                std::fs::remove_file(&cur).map_err(io_err(&cur))?;
                std::fs::create_dir(&cur).map_err(io_err(&cur))?;
            }
            Err(_) => std::fs::create_dir(&cur).map_err(io_err(&cur))?,
        }
    }
    Ok(())
}

/// Remove whatever is at `path`: a file, a link (not followed) or a folder.
fn remove_any(path: &Path) -> Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(path).map_err(io_err(path)),
        Ok(_) => std::fs::remove_file(path).map_err(io_err(path)),
        Err(_) => Ok(()),
    }
}

/// Copy a folder of plain files and folders. Anything else is ignored.
fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to).map_err(io_err(to))?;
    for entry in std::fs::read_dir(from).map_err(io_err(from))? {
        let entry = entry.map_err(io_err(from))?;
        let meta = std::fs::symlink_metadata(entry.path()).map_err(io_err(entry.path()))?;
        let dest = to.join(entry.file_name());
        if meta.is_dir() {
            copy_tree(&entry.path(), &dest)?;
        } else if meta.is_file() {
            std::fs::copy(entry.path(), &dest).map_err(io_err(entry.path()))?;
        }
    }
    Ok(())
}

/// A temp copy of the repo. Deleted when dropped.
pub struct Worktree<'a> {
    dir: TempDir,
    sandbox: &'a Sandbox,
}

impl Worktree<'_> {
    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Apply the model's edits. If any edit is not allowed, none are applied
    /// and the reason is returned.
    pub fn apply_edits(&self, edits: &[FileEdit]) -> std::result::Result<(), String> {
        for e in edits {
            if let Some(why) = self.sandbox.edit_violation(&e.path) {
                return Err(why);
            }
            if e.content.len() > MAX_EDIT_BYTES {
                return Err(format!(
                    "edit to {:?} is larger than {MAX_EDIT_BYTES} bytes",
                    e.path
                ));
            }
        }
        for e in edits {
            let target = self.path().join(&e.path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
            }
            std::fs::write(&target, &e.content).map_err(|err| err.to_string())?;
        }
        Ok(())
    }

    /// Put the pristine hidden files and folders back, whatever the model left
    /// at those paths: a changed file, a deleted one, a symlink in its place, a
    /// symlink or a plain file where a parent folder should be. Anything in the
    /// way is removed (links are removed, never followed) and the parents are
    /// made real folders again. The copies come from the pristine store, not
    /// from the live repo. If anything cannot be put back this fails, so the
    /// check never runs against something else.
    pub fn restore_hidden(&self) -> Result<()> {
        let sb = self.sandbox;
        let pristine = sb.pristine.path();
        for rel in &sb.hidden_dirs {
            let target = self.path().join(rel);
            ensure_real_parents(self.path(), rel)?;
            remove_any(&target)?;
            copy_tree(&pristine.join(rel), &target)?;
        }
        for rel in sb.hidden_files.iter().filter(|f| !sb.under_hidden_dir(f)) {
            let target = self.path().join(rel);
            ensure_real_parents(self.path(), rel)?;
            remove_any(&target)?;
            copy_into(pristine, self.path(), rel)?;
        }
        Ok(())
    }

    /// Run a shell command in the worktree through the sandbox's runner, with
    /// a timeout. Output is discarded. The environment is cleared except for
    /// the names in `env_passthrough` (and `PATH` and `HOME`, which the
    /// backend sets itself).
    pub fn run_check(&self, command: &str, timeout: Duration) -> Result<CheckStatus> {
        let env: Vec<(String, String)> = self
            .sandbox
            .env_passthrough
            .iter()
            .filter_map(|n| std::env::var(n).ok().map(|v| (n.clone(), v)))
            .collect();
        self.sandbox
            .runner
            .run(self.path(), command, &env, &self.sandbox.protected, timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    use tempfile::tempdir;

    fn write(root: &Path, rel: &str, content: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, content).unwrap();
    }

    fn fixture() -> (tempfile::TempDir, SandboxConfig) {
        let d = tempdir().unwrap();
        write(d.path(), "AGENTS.md", "rules on disk");
        write(d.path(), "src/lib.txt", "source");
        write(d.path(), "tests/secret.txt", "SECRET TEST");
        write(d.path(), "tests/deep/more.txt", "SECRET TOO");
        write(d.path(), "check.sh", "exit 0");
        write(d.path(), "pkg/a_test.txt", "glob hidden");
        write(d.path(), ".git/config", "git");
        write(d.path(), ".rules-evolve/log.jsonl", "log");
        write(d.path(), "target/big.bin", "build output");
        let cfg = SandboxConfig {
            hidden: vec!["tests".into(), "check.sh".into(), "**/*_test.txt".into()],
            ..SandboxConfig::default()
        };
        (d, cfg)
    }

    #[test]
    fn glob_matching() {
        assert!(path_matches("tests", "tests"));
        assert!(path_matches("tests", "tests/a/b.txt"));
        assert!(path_matches("tests/", "tests/a.txt"));
        assert!(!path_matches("tests", "testsuite/a.txt"));
        assert!(path_matches("*.sh", "check.sh"));
        assert!(!path_matches("*.sh", "dir/check.sh"));
        assert!(path_matches("**/*.sh", "dir/sub/check.sh"));
        assert!(path_matches("**/*.sh", "check.sh"));
        assert!(path_matches("src/**/test_*.py", "src/a/b/test_x.py"));
        assert!(!path_matches("src/**/test_*.py", "src/a/b/x.py"));
        assert!(path_matches("./check.sh", "check.sh"));
        assert!(!path_matches("", "anything"));
        assert!(path_matches("a?c", "abc"));
    }

    #[test]
    fn scan_sorts_files_and_skips_protected_ones() {
        let (d, cfg) = fixture();
        let sb = Sandbox::scan(d.path(), "AGENTS.md", &cfg).unwrap();
        assert_eq!(sb.visible_files(), ["src/lib.txt"]);
        assert_eq!(
            sb.hidden_files(),
            [
                "check.sh",
                "pkg/a_test.txt",
                "tests/deep/more.txt",
                "tests/secret.txt"
            ]
        );
    }

    #[test]
    fn model_worktree_has_no_check_files_or_git_or_log() {
        let (d, cfg) = fixture();
        let sb = Sandbox::scan(d.path(), "AGENTS.md", &cfg).unwrap();
        let wt = sb.prepare(b"version under test").unwrap();
        let p = wt.path();
        assert!(p.join("src/lib.txt").is_file());
        assert!(!p.join("tests").exists());
        assert!(!p.join("check.sh").exists());
        assert!(!p.join("pkg/a_test.txt").exists());
        assert!(!p.join(".git").exists());
        assert!(!p.join(".rules-evolve").exists());
        assert!(!p.join("target").exists());
        // No file anywhere in the copy contains the secret text.
        fn all_text(dir: &Path, out: &mut String) {
            for e in std::fs::read_dir(dir).unwrap() {
                let e = e.unwrap().path();
                if e.is_dir() {
                    all_text(&e, out);
                } else {
                    out.push_str(&std::fs::read_to_string(&e).unwrap_or_default());
                }
            }
        }
        let mut text = String::new();
        all_text(p, &mut text);
        assert!(!text.contains("SECRET"));
        assert!(!text.contains("rules on disk"));
        // The rules file is the version under test, not what is on disk.
        assert_eq!(
            std::fs::read_to_string(p.join("AGENTS.md")).unwrap(),
            "version under test"
        );
    }

    #[test]
    fn hidden_files_come_back_for_the_check() {
        let (d, cfg) = fixture();
        let sb = Sandbox::scan(d.path(), "AGENTS.md", &cfg).unwrap();
        let wt = sb.prepare(b"r").unwrap();
        wt.restore_hidden().unwrap();
        assert_eq!(
            std::fs::read_to_string(wt.path().join("tests/secret.txt")).unwrap(),
            "SECRET TEST"
        );
        assert!(wt.path().join("check.sh").is_file());
        assert!(!wt.path().join(".git").exists());
    }

    #[test]
    fn symlinks_are_not_copied() {
        let (d, cfg) = fixture();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc/hostname", d.path().join("link.txt")).unwrap();
            std::os::unix::fs::symlink(d.path().join("tests"), d.path().join("tests_link"))
                .unwrap();
        }
        let sb = Sandbox::scan(d.path(), "AGENTS.md", &cfg).unwrap();
        assert_eq!(sb.visible_files(), ["src/lib.txt"]);
        assert_eq!(sb.skipped_symlinks(), 2);
    }

    #[test]
    fn edits_to_protected_paths_are_refused() {
        let (d, cfg) = fixture();
        let sb = Sandbox::scan(d.path(), "AGENTS.md", &cfg).unwrap();
        let wt = sb.prepare(b"r").unwrap();
        let edit = |p: &str| FileEdit {
            path: p.into(),
            content: "x".into(),
        };
        for bad in [
            "tests/secret.txt",
            "tests/new.txt",
            "check.sh",
            "pkg/b_test.txt",
            "../escape.txt",
            "/abs/path.txt",
            "a/../../b.txt",
            ".git/hooks/pre-commit",
            ".rules-evolve/log.jsonl",
            "AGENTS.md",
            "",
            "target/x",
        ] {
            assert!(
                wt.apply_edits(&[edit(bad)]).is_err(),
                "{bad} should be refused"
            );
        }
        // One bad edit blocks the whole batch.
        assert!(wt.apply_edits(&[edit("ok.txt"), edit("tests/x")]).is_err());
        assert!(!wt.path().join("ok.txt").exists());
        // A normal edit works, including new folders.
        wt.apply_edits(&[edit("out/new/file.txt")]).unwrap();
        assert_eq!(
            std::fs::read_to_string(wt.path().join("out/new/file.txt")).unwrap(),
            "x"
        );
        assert!(wt
            .apply_edits(&[FileEdit {
                path: "big.txt".into(),
                content: "x".repeat(MAX_EDIT_BYTES + 1)
            }])
            .is_err());
    }

    #[test]
    fn check_runs_in_worktree_with_restored_tests() {
        let (d, cfg) = fixture();
        let sb = Sandbox::scan(d.path(), "AGENTS.md", &cfg).unwrap();
        let wt = sb.prepare(b"r").unwrap();
        wt.apply_edits(&[FileEdit {
            path: "answer.txt".into(),
            content: "42".into(),
        }])
        .unwrap();
        wt.restore_hidden().unwrap();
        let t = Duration::from_secs(10);
        assert_eq!(
            wt.run_check(
                "grep -q 42 answer.txt && grep -q 'SECRET TEST' tests/secret.txt",
                t
            )
            .unwrap(),
            CheckStatus::Passed
        );
        assert_eq!(wt.run_check("exit 3", t).unwrap(), CheckStatus::Failed);
        assert_eq!(
            wt.run_check("test -f src/lib.txt", t).unwrap(),
            CheckStatus::Passed
        );
    }

    #[test]
    fn check_timeout_kills_the_command() {
        let (d, cfg) = fixture();
        let sb = Sandbox::scan(d.path(), "AGENTS.md", &cfg).unwrap();
        let wt = sb.prepare(b"r").unwrap();
        let start = Instant::now();
        let status = wt
            .run_check("sleep 30", Duration::from_millis(200))
            .unwrap();
        assert_eq!(status, CheckStatus::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn check_environment_is_cleared_except_passthrough() {
        std::env::set_var("RULES_EVOLVE_TEST_SECRET", "hunter2");
        std::env::set_var("RULES_EVOLVE_TEST_ALLOWED", "yes");
        let (d, mut cfg) = fixture();
        cfg.env_passthrough = vec!["RULES_EVOLVE_TEST_ALLOWED".into()];
        let sb = Sandbox::scan(d.path(), "AGENTS.md", &cfg).unwrap();
        let wt = sb.prepare(b"r").unwrap();
        let t = Duration::from_secs(10);
        assert_eq!(
            wt.run_check("test -z \"$RULES_EVOLVE_TEST_SECRET\"", t)
                .unwrap(),
            CheckStatus::Passed
        );
        assert_eq!(
            wt.run_check("test \"$RULES_EVOLVE_TEST_ALLOWED\" = yes", t)
                .unwrap(),
            CheckStatus::Passed
        );
    }

    #[test]
    fn rules_file_in_a_subfolder_is_written_there() {
        let d = tempdir().unwrap();
        write(d.path(), ".cursor/rules/main.mdc", "disk");
        write(d.path(), "a.txt", "a");
        let sb = Sandbox::scan(
            d.path(),
            ".cursor/rules/main.mdc",
            &SandboxConfig::default(),
        )
        .unwrap();
        let wt = sb.prepare(b"new").unwrap();
        assert_eq!(
            std::fs::read_to_string(wt.path().join(".cursor/rules/main.mdc")).unwrap(),
            "new"
        );
    }

    // ---- hidden paths: pristine copies, read only mounts, restore ----

    #[test]
    fn scan_works_out_hidden_folders_pins_and_read_only_mounts() {
        let (d, cfg) = fixture();
        let sb = Sandbox::scan(d.path(), "AGENTS.md", &cfg).unwrap();
        // Only the top-most hidden folder is listed.
        assert_eq!(sb.hidden_dirs(), ["tests".to_string()]);
        let p = sb.protected();
        let rels: Vec<&str> = p.read_only.iter().map(|m| m.rel.as_str()).collect();
        // Files under a hidden folder are covered by the folder mount.
        assert_eq!(rels, vec!["check.sh", "pkg/a_test.txt", "tests"]);
        // The folder that holds a hidden file is pinned. Hidden folders
        // themselves and the root are not.
        assert_eq!(p.pin_dirs, vec!["pkg".to_string()]);
        // Sources are pristine copies outside the repo and outside any worktree.
        for m in &p.read_only {
            assert!(!m.source.starts_with(d.path()), "{:?}", m.source);
            assert!(m.source.exists(), "{:?}", m.source);
        }
        assert_eq!(
            std::fs::read_to_string(&p.read_only[1].source).unwrap(),
            "glob hidden"
        );
        assert_eq!(
            std::fs::read_to_string(p.read_only[2].source.join("deep/more.txt")).unwrap(),
            "SECRET TOO"
        );
    }

    #[test]
    fn pins_cover_every_folder_above_a_hidden_path() {
        let d = tempdir().unwrap();
        write(d.path(), "AGENTS.md", "r");
        write(d.path(), "a/b/c/test_x.txt", "x");
        write(d.path(), "a/b/vis.txt", "v");
        write(d.path(), "m/n/suite/t.txt", "t");
        let cfg = SandboxConfig {
            hidden: vec!["a/b/c/test_x.txt".into(), "m/n/suite".into()],
            ..SandboxConfig::default()
        };
        let sb = Sandbox::scan(d.path(), "AGENTS.md", &cfg).unwrap();
        assert_eq!(
            sb.protected().pin_dirs,
            ["a", "m", "a/b", "m/n", "a/b/c"].map(String::from).to_vec()
        );
    }

    #[test]
    fn nothing_hidden_means_nothing_protected() {
        let d = tempdir().unwrap();
        write(d.path(), "AGENTS.md", "r");
        write(d.path(), "x.txt", "x");
        let sb = Sandbox::scan(d.path(), "AGENTS.md", &SandboxConfig::default()).unwrap();
        assert!(sb.protected().is_empty());
    }

    #[test]
    fn restore_puts_back_the_original_after_any_edit_delete_or_replacement() {
        use std::os::unix::fs::symlink;
        let (d, cfg) = fixture();
        let sb = Sandbox::scan(d.path(), "AGENTS.md", &cfg).unwrap();
        let outside = tempdir().unwrap();
        write(outside.path(), "keep.txt", "outside stays");
        let wt = sb.prepare(b"r").unwrap();
        let w = wt.path();
        // 1. Changed content.
        write(w, "check.sh", "evil");
        // 2. A hidden file replaced by a symlink to a file outside.
        std::fs::create_dir_all(w.join("pkg")).unwrap();
        symlink(outside.path().join("keep.txt"), w.join("pkg/a_test.txt")).unwrap();
        // 3. A hidden folder replaced by a symlink to a folder outside.
        symlink(outside.path(), w.join("tests")).unwrap();
        wt.restore_hidden().unwrap();
        for (rel, want) in [
            ("check.sh", "exit 0"),
            ("pkg/a_test.txt", "glob hidden"),
            ("tests/secret.txt", "SECRET TEST"),
            ("tests/deep/more.txt", "SECRET TOO"),
        ] {
            let p = w.join(rel);
            assert!(!std::fs::symlink_metadata(&p)
                .unwrap()
                .file_type()
                .is_symlink());
            assert_eq!(std::fs::read_to_string(&p).unwrap(), want, "{rel}");
        }
        assert!(std::fs::symlink_metadata(w.join("tests"))
            .unwrap()
            .file_type()
            .is_dir());
        // Nothing was written through a link into the outside folder.
        assert_eq!(
            std::fs::read_to_string(outside.path().join("keep.txt")).unwrap(),
            "outside stays"
        );
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 1);
    }

    #[test]
    fn restore_fixes_a_parent_folder_that_was_replaced() {
        use std::os::unix::fs::symlink;
        let (d, cfg) = fixture();
        let sb = Sandbox::scan(d.path(), "AGENTS.md", &cfg).unwrap();
        let outside = tempdir().unwrap();
        // The parent of a hidden file is a symlink to an outside folder.
        let wt = sb.prepare(b"r").unwrap();
        std::fs::remove_dir_all(wt.path().join("pkg")).ok();
        symlink(outside.path(), wt.path().join("pkg")).unwrap();
        wt.restore_hidden().unwrap();
        assert!(std::fs::symlink_metadata(wt.path().join("pkg"))
            .unwrap()
            .file_type()
            .is_dir());
        assert_eq!(
            std::fs::read_to_string(wt.path().join("pkg/a_test.txt")).unwrap(),
            "glob hidden"
        );
        assert_eq!(std::fs::read_dir(outside.path()).unwrap().count(), 0);
        // The parent is a plain file instead.
        let wt = sb.prepare(b"r").unwrap();
        std::fs::remove_dir_all(wt.path().join("pkg")).ok();
        std::fs::write(wt.path().join("pkg"), "i am a file").unwrap();
        wt.restore_hidden().unwrap();
        assert_eq!(
            std::fs::read_to_string(wt.path().join("pkg/a_test.txt")).unwrap(),
            "glob hidden"
        );
        // A hidden file replaced by a folder.
        let wt = sb.prepare(b"r").unwrap();
        std::fs::create_dir_all(wt.path().join("check.sh/inner")).unwrap();
        wt.restore_hidden().unwrap();
        assert!(wt.path().join("check.sh").is_file());
    }

    #[test]
    fn extra_files_the_model_put_in_a_hidden_folder_do_not_survive_restore() {
        let (d, cfg) = fixture();
        let sb = Sandbox::scan(d.path(), "AGENTS.md", &cfg).unwrap();
        let wt = sb.prepare(b"r").unwrap();
        write(wt.path(), "tests/conftest.txt", "shadow");
        wt.restore_hidden().unwrap();
        assert!(!wt.path().join("tests/conftest.txt").exists());
    }

    #[test]
    fn restore_uses_the_pristine_copy_not_the_live_repo() {
        let (d, cfg) = fixture();
        let sb = Sandbox::scan(d.path(), "AGENTS.md", &cfg).unwrap();
        // The repo changes after the scan. The scan's copy is what counts.
        write(d.path(), "check.sh", "changed in the repo");
        std::fs::remove_dir_all(d.path().join("tests")).unwrap();
        let wt = sb.prepare(b"r").unwrap();
        wt.restore_hidden().unwrap();
        assert_eq!(
            std::fs::read_to_string(wt.path().join("check.sh")).unwrap(),
            "exit 0"
        );
        assert_eq!(
            std::fs::read_to_string(wt.path().join("tests/secret.txt")).unwrap(),
            "SECRET TEST"
        );
    }

    #[test]
    fn under_none_a_check_can_still_write_to_hidden_files() {
        // Documented behavior: `none` restores the pristine copy before the
        // check but does not stop writes during it.
        let (d, cfg) = fixture();
        let sb = Sandbox::scan(d.path(), "AGENTS.md", &cfg).unwrap();
        let wt = sb.prepare(b"r").unwrap();
        wt.restore_hidden().unwrap();
        let st = wt
            .run_check("echo evil > check.sh", Duration::from_secs(10))
            .unwrap();
        assert_eq!(st, CheckStatus::Passed);
        assert_eq!(
            std::fs::read_to_string(wt.path().join("check.sh")).unwrap(),
            "evil\n"
        );
    }
}
