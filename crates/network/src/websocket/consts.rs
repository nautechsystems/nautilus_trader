//! Constants for WebSocket protocol handling.

/// Standard text ping message.
pub const TEXT_PING: &str = "ping";

/// Standard text pong message.
pub const TEXT_PONG: &str = "pong";

/// Default authentication timeout in seconds.
pub const AUTHENTICATION_TIMEOUT_SECS: u64 = 10;

/// Connection state check interval in milliseconds.
pub(crate) const CONNECTION_STATE_CHECK_INTERVAL_MS: u64 = 10;

/// Send operation check interval in milliseconds.
pub(crate) const SEND_OPERATION_CHECK_INTERVAL_MS: u64 = 1;

/// Graceful shutdown delay in milliseconds.
pub(crate) const GRACEFUL_SHUTDOWN_DELAY_MS: u64 = 100;

/// Reconnect stabilization delay in milliseconds before post-reconnect callbacks fire.
pub(crate) const RECONNECT_STABILIZATION_DELAY_MS: u64 = 100;

/// Graceful shutdown timeout in seconds.
pub(crate) const GRACEFUL_SHUTDOWN_TIMEOUT_SECS: u64 = 5;
