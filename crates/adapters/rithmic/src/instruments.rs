//! Instrument provider for Rithmic.
//!
//! This module provides functionality to load and cache futures contract
//! definitions from Rithmic exchanges.

mod parse;
mod provider;

pub use provider::RithmicInstrument;
pub use provider::RithmicInstrumentProvider;
