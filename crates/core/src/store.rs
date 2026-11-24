//! Files under `.graft/`: content-addressed blobs and fixed paths.

use crate::error::{io_err, Result};
use crate::hash::sha256_hex;
use std::path::{Path, PathBuf};

/// Name of the project directory inside a repo.
pub const DIR_NAME: &str = ".graft";

/// The project directory of bogo 3.x (bogovolution), before the rename to
/// graft. A repo that still has it and no `.graft/` keeps using it in place,
/// so nothing has to be moved.
pub const LEGACY_DIR_NAME: &str = ".bogo";

/// The project directory of rules-evolve 2.x, before the rename to bogo.
/// It is only looked for to tell the user to rename it.
pub const OLD_DIR_NAME: &str = ".rules-evolve";

/// Every project directory name graft has used. These are never copied into
/// a sandbox and never written by `apply`.
pub const ALL_DIR_NAMES: [&str; 3] = [DIR_NAME, LEGACY_DIR_NAME, OLD_DIR_NAME];

/// Result of checking one blob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlobState {
    Ok,
    Missing,
    Corrupt,
}

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    /// `repo_dir` is the repo root. The store lives in `repo_dir/.graft`,
    /// or in `repo_dir/.bogo` when only that older folder exists.
    pub fn for_repo(repo_dir: &Path) -> Store {
        let current = repo_dir.join(DIR_NAME);
        let legacy = repo_dir.join(LEGACY_DIR_NAME);
        let root = if !current.exists() && legacy.is_dir() {
            legacy
        } else {
            current
        };
        Store { root }
    }

    /// The folder name in use for this store (`.graft` or `.bogo`).
    pub fn dir_name(&self) -> &str {
        self.root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(DIR_NAME)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn blobs_dir(&self) -> PathBuf {
        self.root.join("blobs")
    }
    pub fn config_path(&self) -> PathBuf {
        self.root.join("config.toml")
    }
    pub fn tasks_path(&self) -> PathBuf {
        self.root.join("tasks.toml")
    }

    pub fn create_dirs(&self) -> Result<()> {
        std::fs::create_dir_all(self.blobs_dir()).map_err(io_err(self.blobs_dir()))
    }

    /// Store content by its hash. Returns the hash. Writing the same content
    /// twice is fine.
    pub fn put_blob(&self, content: &[u8]) -> Result<String> {
        let hash = sha256_hex(content);
        let path = self.blobs_dir().join(&hash);
        if !path.exists() {
            std::fs::write(&path, content).map_err(io_err(&path))?;
        }
        Ok(hash)
    }

    pub fn get_blob(&self, hash: &str) -> Result<Vec<u8>> {
        let path = self.blobs_dir().join(hash);
        std::fs::read(&path).map_err(io_err(&path))
    }

    pub fn check_blob(&self, hash: &str) -> BlobState {
        match std::fs::read(self.blobs_dir().join(hash)) {
            Ok(bytes) if sha256_hex(&bytes) == hash => BlobState::Ok,
            Ok(_) => BlobState::Corrupt,
            Err(_) => BlobState::Missing,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn blobs_are_content_addressed_and_checked() {
        let dir = tempdir().unwrap();
        let store = Store::for_repo(dir.path());
        store.create_dirs().unwrap();
        let h1 = store.put_blob(b"hello").unwrap();
        let h2 = store.put_blob(b"hello").unwrap();
        assert_eq!(h1, h2);
        assert_eq!(store.get_blob(&h1).unwrap(), b"hello");
        assert_eq!(store.check_blob(&h1), BlobState::Ok);
        assert_eq!(store.check_blob(&"0".repeat(64)), BlobState::Missing);
        std::fs::write(store.blobs_dir().join(&h1), b"changed").unwrap();
        assert_eq!(store.check_blob(&h1), BlobState::Corrupt);
    }

    #[test]
    fn a_bogo_folder_is_used_when_there_is_no_graft_folder() {
        let dir = tempdir().unwrap();
        assert_eq!(Store::for_repo(dir.path()).dir_name(), DIR_NAME);
        std::fs::create_dir(dir.path().join(LEGACY_DIR_NAME)).unwrap();
        let store = Store::for_repo(dir.path());
        assert_eq!(store.root(), dir.path().join(LEGACY_DIR_NAME));
        assert_eq!(store.dir_name(), LEGACY_DIR_NAME);
        std::fs::create_dir(dir.path().join(DIR_NAME)).unwrap();
        assert_eq!(Store::for_repo(dir.path()).dir_name(), DIR_NAME);
    }
}
