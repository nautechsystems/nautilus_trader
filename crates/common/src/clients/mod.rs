//! Client trait definitions for data and execution clients.
//!
//! Provides the core trait interfaces that define how clients interact with
//! data providers and execution venues.

mod data;
mod execution;

use std::fmt::{Debug, Display};

pub use data::DataClient;
pub use execution::ExecutionClient;

#[inline(always)]
fn log_not_implemented<T: Debug>(cmd: &T) {
    log::warn!("{cmd:?} – handler not implemented");
}

#[inline(always)]
pub fn log_command_error<C: Debug, E: Display>(cmd: &C, e: &E) {
    log::error!("Error on {cmd:?}: {e}");
}
