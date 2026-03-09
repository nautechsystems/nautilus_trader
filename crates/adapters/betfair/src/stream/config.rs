//! Configuration for the Betfair stream client.

use crate::common::consts::{BETFAIR_STREAM_HOST, BETFAIR_STREAM_PORT};

/// Configuration for the Betfair Exchange Stream API client.
#[derive(Debug, Clone)]
pub struct BetfairStreamConfig {
    /// Stream host (default: `stream-api.betfair.com`).
    pub host: String,
    /// Stream TLS port (default: 443).
    pub port: u16,
    /// Interval between client heartbeat messages in milliseconds (default: 5 000).
    pub heartbeat_ms: u64,
    /// Idle read timeout in milliseconds; triggers reconnection if no data arrives (default: 60 000).
    pub idle_timeout_ms: u64,
    /// Initial reconnection back-off delay in milliseconds (default: 2 000).
    pub reconnect_delay_initial_ms: u64,
    /// Maximum reconnection back-off delay in milliseconds (default: 30 000).
    pub reconnect_delay_max_ms: u64,
    /// Use TLS (default: true). Override with `false` only for local testing.
    #[doc(hidden)]
    pub use_tls: bool,
}

impl Default for BetfairStreamConfig {
    fn default() -> Self {
        Self {
            host: BETFAIR_STREAM_HOST.to_string(),
            port: BETFAIR_STREAM_PORT,
            heartbeat_ms: 5_000,
            idle_timeout_ms: 60_000,
            reconnect_delay_initial_ms: 2_000,
            reconnect_delay_max_ms: 30_000,
            use_tls: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_stream_config_defaults() {
        let config = BetfairStreamConfig::default();
        assert_eq!(config.host, BETFAIR_STREAM_HOST);
        assert_eq!(config.port, BETFAIR_STREAM_PORT);
        assert_eq!(config.heartbeat_ms, 5_000);
        assert_eq!(config.idle_timeout_ms, 60_000);
        assert_eq!(config.reconnect_delay_initial_ms, 2_000);
        assert_eq!(config.reconnect_delay_max_ms, 30_000);
        assert!(config.use_tls);
    }
}
