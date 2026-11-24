//! `apply`: write a version from the lineage into the repo's real rules file.
//!
//! This is the only command that changes a file outside `.graft/`.
//! It is careful about it:
//!
//! - by default it is a dry run: it shows the diff and writes nothing;
//! - only an accepted version is written unless `force` is set;
//! - the stored version must still match its hash;
//! - the target must be a plain relative path inside the repo, with no `..`,
//!   no symlink on the way, and not inside `.graft/` or `.git/`;
//! - the file is written to a temporary file next to it and renamed into
//!   place, so a crash never leaves half a file;
//! - every write is recorded as an `applied` event in the hash chained log.

use crate::error::{Error, Result};
use crate::event::{AppliedPayload, Payload, VerdictKind};
use crate::export::unified_diff;
use crate::hash::sha256_hex;
use crate::repo::Repo;
use crate::tree::Tree;
use serde::Serialize;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ApplyOptions {
    /// `current` (the default), `root`, a full hash or a unique prefix.
    pub version: String,
    /// Write the file. Without it nothing is written (dry run).
    pub yes: bool,
    /// Allow a version that was not accepted.
    pub force: bool,
}

/// What `apply` did or would do.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ApplyOutcome {
    pub version: String,
    pub rules_file: String,
    /// `accepted`, `root`, `pending`, `rejected`, `noise` or `overfit`.
    pub status: String,
    /// `dry_run`, `written`, or `unchanged` (the file already holds the version).
    pub outcome: String,
    pub forced: bool,
    /// Git style diff from the file on disk to the version. Empty when unchanged.
    pub diff: String,
    pub previous_sha256: Option<String>,
    /// Sequence number of the `applied` event, when one was written.
    pub event_seq: Option<u64>,
}

/// The status of a version as `apply` sees it.
fn status_of(tree: &Tree, hash: &str) -> &'static str {
    if hash == tree.root {
        return "root";
    }
    match tree.node(hash).and_then(|n| n.verdict) {
        Some(v) => v.as_str(),
        None => "pending",
    }
}

/// Check that `rules_file` is a plain relative path and return where it
/// lives. Refuses absolute paths, `..`, paths into `.graft` or `.git`,
/// and any symlink on the way (the file itself or a folder above it), so the
/// write can never land outside the repo.
pub fn safe_target(repo_dir: &Path, rules_file: &str) -> Result<PathBuf> {
    let rel = Path::new(rules_file);
    let refuse = |why: &str| {
        Err(Error::Input(format!(
            "refusing to write the rules file {rules_file:?}: {why}"
        )))
    };
    if rules_file.is_empty() || rel.is_absolute() {
        return refuse("it must be a path relative to the repo root");
    }
    let mut parts = Vec::new();
    for c in rel.components() {
        match c {
            Component::Normal(p) => parts.push(p.to_owned()),
            Component::CurDir => {}
            _ => return refuse("it must not contain .. or a root"),
        }
    }
    if parts.is_empty() {
        return refuse("it names no file");
    }
    if matches!(
        parts[0].to_str(),
        Some(".graft") | Some(".bogo") | Some(".rules-evolve") | Some(".git")
    ) {
        return refuse("it is inside .graft or .git");
    }
    let root = repo_dir.canonicalize().map_err(|e| Error::Io {
        path: repo_dir.to_path_buf(),
        source: e,
    })?;
    let mut path = root.clone();
    for (i, p) in parts.iter().enumerate() {
        path.push(p);
        match std::fs::symlink_metadata(&path) {
            Ok(m) if m.file_type().is_symlink() => {
                return refuse("a part of the path is a symlink");
            }
            Ok(m) if i + 1 < parts.len() && !m.is_dir() => {
                return refuse("a part of the path is not a folder");
            }
            Ok(m) if i + 1 == parts.len() && !m.is_file() => {
                return refuse("it is not a regular file");
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                if i + 1 < parts.len() {
                    return refuse("its folder does not exist");
                }
            }
            Err(e) => {
                return Err(Error::Io {
                    path: path.clone(),
                    source: e,
                })
            }
        }
    }
    if !path.starts_with(&root) {
        return refuse("it is outside the repo");
    }
    Ok(path)
}

pub fn apply(repo: &Repo, opts: &ApplyOptions) -> Result<ApplyOutcome> {
    repo.recover_interrupted_write()?;
    let tree = Tree::from_events(&repo.events()?)?;
    let version = tree.resolve(&opts.version)?;
    let status = status_of(&tree, &version);
    let accepted = tree.node(&version).and_then(|n| n.verdict) == Some(VerdictKind::Accepted);
    if !accepted && !opts.force {
        return Err(Error::Input(format!(
            "version {} is {status}, not accepted. Only accepted versions are applied. Pass --force to write it anyway",
            &version[..12]
        )));
    }
    let bytes = repo.store().get_blob(&version)?;
    if sha256_hex(&bytes) != version {
        return Err(Error::Input(format!(
            "the stored copy of {} does not match its hash. Run verify-log",
            &version[..12]
        )));
    }
    let new_text = String::from_utf8(bytes.clone())
        .map_err(|_| Error::Input("the stored version is not UTF-8 text".into()))?;
    let target = safe_target(repo.dir(), &tree.rules_file)?;
    let old = match std::fs::read(&target) {
        Ok(b) => Some(b),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            return Err(Error::Io {
                path: target.clone(),
                source: e,
            })
        }
    };
    let previous_sha256 = old.as_ref().map(|b| sha256_hex(b));
    let old_text = old
        .as_ref()
        .map(|b| String::from_utf8_lossy(b).to_string())
        .unwrap_or_default();
    let mut out = ApplyOutcome {
        version: version.clone(),
        rules_file: tree.rules_file.clone(),
        status: status.to_string(),
        outcome: "dry_run".into(),
        forced: !accepted,
        diff: String::new(),
        previous_sha256: previous_sha256.clone(),
        event_seq: None,
    };
    if old.as_deref() == Some(bytes.as_slice()) {
        out.outcome = "unchanged".into();
        return Ok(out);
    }
    out.diff = unified_diff(&tree.rules_file, &old_text, &new_text);
    if !opts.yes {
        return Ok(out);
    }
    let tmp = target.with_file_name(format!(
        ".{}.graft-apply.tmp",
        target
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default()
    ));
    std::fs::write(&tmp, &bytes).map_err(|e| Error::Io {
        path: tmp.clone(),
        source: e,
    })?;
    if let Err(e) = std::fs::rename(&tmp, &target) {
        let _ = std::fs::remove_file(&tmp);
        return Err(Error::Io {
            path: target,
            source: e,
        });
    }
    let event = repo.append(&Payload::Applied(AppliedPayload {
        version,
        rules_file: tree.rules_file.clone(),
        previous_sha256,
        status: status.to_string(),
        forced: !accepted,
    }))?;
    out.outcome = "written".into();
    out.event_seq = Some(event.seq);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{MutationProposedPayload, VerdictPayload};
    use crate::init::{init, InitOptions};
    use tempfile::{tempdir, TempDir};

    fn project() -> (TempDir, Repo, String, String) {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "base\n").unwrap();
        init(&InitOptions {
            dir: dir.path().to_path_buf(),
            rules_file: None,
            model: "m".into(),
        })
        .unwrap();
        let repo = Repo::open(dir.path()).unwrap();
        let root = Tree::from_events(&repo.events().unwrap()).unwrap().root;
        let add = |text: &str| {
            let h = repo.store().put_blob(text.as_bytes()).unwrap();
            repo.append(&Payload::MutationProposed(MutationProposedPayload {
                parent: root.clone(),
                child: h.clone(),
                hypothesis: "h".into(),
                ..Default::default()
            }))
            .unwrap();
            h
        };
        let good = add("base\ngood\n");
        let bad = add("base\nbad\n");
        for (child, v) in [
            (&good, VerdictKind::Accepted),
            (&bad, VerdictKind::Rejected),
        ] {
            repo.append(&Payload::Verdict(VerdictPayload {
                parent: root.clone(),
                child: child.clone(),
                verdict: v,
                parent_median: Some(0.5),
                child_median: Some(0.5),
                paired_wins: Some(0),
                repeats: Some(5),
                note: None,
                held_out_parent_median: None,
                held_out_child_median: None,
                held_out_paired_wins: None,
            }))
            .unwrap();
        }
        (dir, repo, good, bad)
    }

    fn opts(v: &str, yes: bool, force: bool) -> ApplyOptions {
        ApplyOptions {
            version: v.into(),
            yes,
            force,
        }
    }

    fn rules(repo: &Repo) -> String {
        std::fs::read_to_string(repo.dir().join("AGENTS.md")).unwrap()
    }

    #[test]
    fn dry_run_by_default_then_write_with_yes() {
        let (_d, repo, good, _) = project();
        let events = repo.events().unwrap().len();
        let o = apply(&repo, &opts("current", false, false)).unwrap();
        assert_eq!(o.version, good);
        assert_eq!(o.outcome, "dry_run");
        assert!(o.diff.contains("+good"), "{}", o.diff);
        assert_eq!(rules(&repo), "base\n");
        assert_eq!(repo.events().unwrap().len(), events);

        let o = apply(&repo, &opts("current", true, false)).unwrap();
        assert_eq!(o.outcome, "written");
        assert_eq!(rules(&repo), "base\ngood\n");
        assert_eq!(
            o.previous_sha256.as_deref(),
            Some(sha256_hex(b"base\n").as_str())
        );
        let tree = Tree::from_events(&repo.events().unwrap()).unwrap();
        assert_eq!(tree.applied.len(), 1);
        assert_eq!(tree.applied[0].version, good);
        assert!(repo.verify().unwrap().valid);
        // no temp file left behind
        let names: Vec<_> = std::fs::read_dir(repo.dir())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().to_string())
            .collect();
        assert!(!names.iter().any(|n| n.contains(".tmp")), "{names:?}");

        // again: nothing to do, no new event
        let n = repo.events().unwrap().len();
        let o = apply(&repo, &opts("current", true, false)).unwrap();
        assert_eq!(o.outcome, "unchanged");
        assert_eq!(repo.events().unwrap().len(), n);
    }

    #[test]
    fn non_accepted_versions_need_force() {
        let (_d, repo, _, bad) = project();
        let err = apply(&repo, &opts(&bad, true, false))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("rejected, not accepted") && err.contains("--force"),
            "{err}"
        );
        let err = apply(&repo, &opts("root", true, false))
            .unwrap_err()
            .to_string();
        assert!(err.contains("root, not accepted"), "{err}");
        assert_eq!(rules(&repo), "base\n");
        let o = apply(&repo, &opts(&bad, true, true)).unwrap();
        assert!(o.forced);
        assert_eq!(o.status, "rejected");
        assert_eq!(rules(&repo), "base\nbad\n");
        let tree = Tree::from_events(&repo.events().unwrap()).unwrap();
        assert!(tree.applied[0].forced);
    }

    #[test]
    fn a_corrupt_blob_is_not_written() {
        let (_d, repo, good, _) = project();
        std::fs::write(repo.store().blobs_dir().join(&good), "tampered\n").unwrap();
        let err = apply(&repo, &opts(&good, true, false))
            .unwrap_err()
            .to_string();
        assert!(err.contains("does not match its hash"), "{err}");
        assert_eq!(rules(&repo), "base\n");
    }

    #[test]
    fn targets_outside_the_repo_are_refused() {
        let dir = tempdir().unwrap();
        let d = dir.path();
        for bad in [
            "/etc/passwd",
            "../x.md",
            "a/../../x.md",
            ".graft/config.toml",
            ".git/config",
            "",
        ] {
            assert!(safe_target(d, bad).is_err(), "{bad}");
        }
        assert!(safe_target(d, "AGENTS.md").is_ok());
        assert!(safe_target(d, "missing/AGENTS.md").is_err());
        std::fs::create_dir(d.join("sub")).unwrap();
        assert!(safe_target(d, "sub/CLAUDE.md").is_ok());
        assert!(safe_target(d, "sub").is_err(), "a folder is not a file");
        #[cfg(unix)]
        {
            let outside = tempdir().unwrap();
            std::fs::write(outside.path().join("t.md"), "x").unwrap();
            std::os::unix::fs::symlink(outside.path().join("t.md"), d.join("LINK.md")).unwrap();
            std::os::unix::fs::symlink(outside.path(), d.join("linkdir")).unwrap();
            assert!(safe_target(d, "LINK.md").is_err());
            assert!(safe_target(d, "linkdir/t.md").is_err());
        }
    }

    #[test]
    fn a_symlinked_rules_file_is_never_followed() {
        let (_d, repo, _, _) = project();
        let outside = tempdir().unwrap();
        let victim = outside.path().join("victim.md");
        std::fs::write(&victim, "keep me\n").unwrap();
        std::fs::remove_file(repo.dir().join("AGENTS.md")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&victim, repo.dir().join("AGENTS.md")).unwrap();
        #[cfg(unix)]
        {
            assert!(apply(&repo, &opts("current", true, false)).is_err());
            assert_eq!(std::fs::read_to_string(&victim).unwrap(), "keep me\n");
        }
    }
}
