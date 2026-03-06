//! Betfair Exchange Stream API client and message types.
//!
//! Covers both request messages (authentication, subscriptions) and response
//! messages (MCM, OCM, connection, status), as well as the raw TLS client.

pub mod client;
pub mod config;
pub mod error;
pub mod messages;
pub mod parse;
