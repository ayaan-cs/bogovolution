//! A rules-evolve project on disk: log, blobs, config, and the rebuilt tree.

use crate::config::Config;
use crate::error::{Error, Result};
use crate::event::{Event, EventKind, Payload};
use crate::log::{Log, Problem, VerifyReport};
use crate::store::{BlobState, Store};
use crate::tasks::TasksFile;
use crate::tree::Tree;
use serde::Serialize;
use std::path::{Path, PathBuf};

pub struct Repo {
    dir: PathBuf,
    store: Store,
    log: Log,
}

/// Result of `verify-log`: the chain check plus a check of stored blobs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FullVerify {
    /// True only when the chain and every blob are fine.
    pub valid: bool,
    pub chain: VerifyReport,
    pub blob_problems: Vec<Problem>,
}

/// Result of `status`.
#[derive(Debug, Clone, Serialize)]
pub struct StatusReport {
    pub repo_root: String,
    pub valid: bool,
    pub chain: VerifyReport,
    pub blob_problems: Vec<Problem>,
    /// `None` when the log is not valid.
    pub tree: Option<Tree>,
    pub config: Option<Config>,
}

impl Repo {
    /// Open an initialized project in `dir`.
    pub fn open(dir: &Path) -> Result<Repo> {
        let store = Store::for_repo(dir);
        let log = Log::new(store.root());
        if !log.exists() {
            return Err(Error::NotInitialized(store.root().to_path_buf()));
        }
        Ok(Repo {
            dir: dir.to_path_buf(),
            store,
            log,
        })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }
    pub fn store(&self) -> &Store {
        &self.store
    }
    pub fn log(&self) -> &Log {
        &self.log
    }

    pub fn config(&self) -> Result<Config> {
        Config::load(&self.store.config_path())
    }

    pub fn tasks(&self) -> Result<TasksFile> {
        TasksFile::load(&self.store.tasks_path())
    }

    /// Append an event, but only if the lineage stays consistent.
    pub fn append(&self, payload: &Payload) -> Result<Event> {
        let events = self.log.read_events()?;
        let mut tree = Tree::from_events(&events)?;
        tree.apply(events.len() as u64, payload)?;
        self.log.append(payload)
    }

    /// Check the chain and every blob the tree points at.
    pub fn verify(&self) -> Result<FullVerify> {
        let (chain, events) = self.log.verify()?;
        let mut blob_problems = Vec::new();
        if chain.valid {
            if let Ok(tree) = Tree::from_events(&events) {
                for node in &tree.nodes {
                    match self.store.check_blob(&node.hash) {
                        BlobState::Ok => {}
                        BlobState::Missing => blob_problems.push(Problem {
                            code: "blob_missing".into(),
                            seq: Some(node.created_seq),
                            line: None,
                            message: format!("blob {} is missing", node.hash),
                        }),
                        BlobState::Corrupt => blob_problems.push(Problem {
                            code: "blob_corrupt".into(),
                            seq: Some(node.created_seq),
                            line: None,
                            message: format!("blob {} does not match its hash", node.hash),
                        }),
                    }
                }
            }
        }
        Ok(FullVerify {
            valid: chain.valid && blob_problems.is_empty(),
            chain,
            blob_problems,
        })
    }

    /// All events, failing if the chain is broken.
    pub fn events(&self) -> Result<Vec<Event>> {
        self.log.read_events()
    }

    /// Events of one kind, or all when `kind` is `None`.
    pub fn events_of(&self, kind: Option<EventKind>) -> Result<Vec<Event>> {
        let all = self.events()?;
        Ok(match kind {
            Some(k) => all.into_iter().filter(|e| e.kind == k).collect(),
            None => all,
        })
    }

    pub fn status(&self) -> Result<StatusReport> {
        let (chain, events) = self.log.verify()?;
        let tree = if chain.valid {
            Some(Tree::from_events(&events)?)
        } else {
            None
        };
        let full = self.verify()?;
        let config = Config::load(&self.store.config_path()).ok();
        Ok(StatusReport {
            repo_root: self.dir.display().to_string(),
            valid: full.valid,
            chain: full.chain,
            blob_problems: full.blob_problems,
            tree,
            config,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{MutationProposedPayload, VerdictKind, VerdictPayload};
    use crate::init::{init, InitOptions};
    use tempfile::tempdir;

    fn setup() -> (tempfile::TempDir, Repo, String) {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "v0").unwrap();
        let report = init(&InitOptions {
            dir: dir.path().to_path_buf(),
            rules_file: None,
            model: "m".into(),
        })
        .unwrap();
        let repo = Repo::open(dir.path()).unwrap();
        (dir, repo, report.root_hash)
    }

    #[test]
    fn append_checks_lineage_before_writing() {
        let (_d, repo, root) = setup();
        let bad = Payload::Verdict(VerdictPayload {
            parent: root.clone(),
            child: "f".repeat(64),
            verdict: VerdictKind::Accepted,
            parent_median: None,
            child_median: None,
            paired_wins: None,
            repeats: None,
            note: None,
        });
        assert!(repo.append(&bad).is_err());
        assert_eq!(repo.events().unwrap().len(), 1);

        let child = repo.store().put_blob(b"v1").unwrap();
        repo.append(&Payload::MutationProposed(MutationProposedPayload {
            parent: root,
            child,
            hypothesis: "shorter".into(),
        }))
        .unwrap();
        let status = repo.status().unwrap();
        assert!(status.valid);
        assert_eq!(status.tree.unwrap().summary().pending, 1);
    }

    #[test]
    fn verify_reports_missing_and_corrupt_blobs() {
        let (_d, repo, root) = setup();
        std::fs::write(repo.store().blobs_dir().join(&root), "tampered").unwrap();
        let v = repo.verify().unwrap();
        assert!(!v.valid);
        assert!(v.chain.valid);
        assert_eq!(v.blob_problems[0].code, "blob_corrupt");
        std::fs::remove_file(repo.store().blobs_dir().join(&root)).unwrap();
        let v = repo.verify().unwrap();
        assert_eq!(v.blob_problems[0].code, "blob_missing");
    }

    #[test]
    fn status_without_tree_when_log_is_broken() {
        let (_d, repo, _root) = setup();
        let text = std::fs::read_to_string(repo.log().path()).unwrap();
        std::fs::write(
            repo.log().path(),
            text.replace("test", "tost").replace("\"m\"", "\"x\""),
        )
        .unwrap();
        let status = repo.status().unwrap();
        assert!(!status.valid);
        assert!(status.tree.is_none());
    }

    #[test]
    fn open_fails_when_not_initialized() {
        let dir = tempdir().unwrap();
        assert!(matches!(
            Repo::open(dir.path()),
            Err(Error::NotInitialized(_))
        ));
    }
}
