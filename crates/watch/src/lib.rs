//! Live terminal view for bogo lineages.
//!
//! This crate is optional and sits on top of `bogovolution-core`. The core has
//! no terminal dependencies. Layers, from the bottom:
//!
//! - [`state`]: events in, tree rows, run progress, verdict feed and details out.
//! - [`view`]: focus, selection, scrolling and key handling.
//! - [`ui`]: painting a frame. Pure, so it can render to a test backend.
//! - [`svg`]: a colored picture of a rendered frame, for docs.
//! - [`app`]: the terminal loop that tails the log and runs a child process.

pub mod app;
pub mod state;
pub mod svg;
pub mod ui;
pub mod view;

#[cfg(test)]
mod testkit;
