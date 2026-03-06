//! Blockchain event data structures and parsers.
//!
//! This module provides types and utilities for parsing and handling various blockchain events
//! emitted by smart contracts, particularly DeFi protocol events such as swaps, mints, burns,
//! and pool creation events.

pub mod burn;
pub mod collect;
pub mod flash;
pub mod initialize;
pub mod mint;
pub mod pool_created;
pub mod swap;
