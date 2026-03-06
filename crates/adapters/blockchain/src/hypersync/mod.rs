//! HyperSync client integration for efficient blockchain data indexing.
//!
//! This module provides a client interface for [HyperSync](https://envio.dev/#hypersync),
//! a high-performance blockchain data indexing service that enables efficient querying
//! of historical blockchain data across multiple networks.

pub mod client;
pub mod helpers;
pub mod transform;

/// Type alias for HyperSync log entries.
pub type HypersyncLog = hypersync_client::simple_types::Log;
