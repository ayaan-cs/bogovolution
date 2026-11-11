//! rules-evolve engine.
//!
//! Phase 1 holds the parts that do not call a model: the hash-chained
//! lineage log, the tree rebuilt from it, and project setup.

pub mod config;
pub mod error;
pub mod event;
pub mod git;
pub mod hash;
pub mod init;
pub mod log;
pub mod repo;
pub mod store;
pub mod tree;

pub use error::{Error, Result};
