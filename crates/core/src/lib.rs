//! graft engine.
//!
//! Phase 1 holds the parts that do not call a model: the hash-chained
//! lineage log, the tree rebuilt from it, and project setup.

pub mod apply;
pub mod bench;
pub mod config;
pub mod digest;
pub mod error;
pub mod event;
pub mod export;
pub mod failure;
pub mod git;
pub mod hash;
pub mod init;
pub mod isolation;
pub mod local;
pub mod log;
pub mod model;
pub mod mutate;
pub mod remote;
pub mod replay;
pub mod repo;
pub mod sandbox;
pub mod shadow;
pub mod store;
pub mod tasks;
#[cfg(test)]
mod testutil;
pub mod tree;
pub mod verdict;

pub use error::{Error, Result};
