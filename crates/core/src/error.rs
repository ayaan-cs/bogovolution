//! Error type for the engine.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not a bogo project: {0} does not exist (run `bogo init` first)")]
    NotInitialized(PathBuf),
    #[error(
        "{0} is a project from rules-evolve 2.x. bogo 3 keeps its files in .bogo/: rename the folder (mv .rules-evolve .bogo, or on Windows: ren .rules-evolve .bogo) and run the command again"
    )]
    OldProjectDir(PathBuf),
    #[error("already initialized: {0} exists")]
    AlreadyInitialized(PathBuf),
    #[error("lineage log is not valid, refusing to continue: {0}")]
    LogInvalid(String),
    #[error("invalid lineage: {0}")]
    InvalidLineage(String),
    #[error("{0}")]
    Input(String),
    #[error("{0}")]
    ReplayMiss(String),
    /// A model answered, but not in a form the harness can use.
    #[error("{0}")]
    BadReply(String),
    /// Model calls failed during a run (for example the server died), so the
    /// run was thrown away and nothing was recorded for it (v2.7.2).
    #[error("{0}")]
    ModelUnavailable(String),
    #[error("io error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not read {path}: {message}")]
    Parse { path: PathBuf, message: String },
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn io_err(path: impl Into<PathBuf>) -> impl FnOnce(std::io::Error) -> Error {
    let path = path.into();
    move |source| Error::Io { path, source }
}
