use std::{sync::LazyLock, time::Duration};

use nautilus_model::{enums::OrderType, identifiers::Venue};
use ustr::Ustr;

pub const HYPERLIQUID: &str = "HYPERLIQUID";
pub static HYPERLIQUID_VENUE: LazyLock<Venue> =
    LazyLock::new(|| Venue::new(Ustr::from(HYPERLIQUID)));

pub const HYPERLIQUID_WS_URL: &str = "wss://api.hyperliquid.xyz/ws";
pub const HYPERLIQUID_INFO_URL: &str = "https://api.hyperliquid.xyz/info";
pub const HYPERLIQUID_EXCHANGE_URL: &str = "https://api.hyperliquid.xyz/exchange";

pub const HYPERLIQUID_TESTNET_WS_URL: &str = "wss://api.hyperliquid-testnet.xyz/ws";
pub const HYPERLIQUID_TESTNET_INFO_URL: &str = "https://api.hyperliquid-testnet.xyz/info";
pub const HYPERLIQUID_TESTNET_EXCHANGE_URL: &str = "https://api.hyperliquid-testnet.xyz/exchange";

const PROD_RECONNECT_INITIAL_DELAY_MS: u64 = 1_000;
const PROD_RECONNECT_MAX_DELAY_MS: u64 = 15_000;
const PROD_RECONNECT_JITTER_MS: u64 = 2_000;
const PROD_STARTUP_CONNECT_SPREAD_MS: u64 = 30_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconnectTuning {
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
    pub jitter_ms: u64,
}

// Builder code address for order attribution (zero-fee)
// Address MUST be lowercase for msgpack serialization
pub const NAUTILUS_BUILDER_ADDRESS: &str = "0x0c8d970c462726e014ad36f6c5a63e99db48a8e7";

/// Hyperliquid signing chain ID (0x66eee = 421614 decimal).
pub const HYPERLIQUID_CHAIN_ID: u64 = 421614;

// Error message substrings for detecting specific rejection reasons
pub const HYPERLIQUID_POST_ONLY_WOULD_MATCH: &str =
    "Post only order would have immediately matched";

/// Hyperliquid supported order types.
///
/// # Notes
///
/// - All order types support trigger prices except Market and Limit.
/// - Conditional orders follow patterns from OKX, Bybit, and BitMEX adapters.
/// - Stop orders (StopMarket/StopLimit) are protective stops (sl).
/// - If Touched orders (MarketIfTouched/LimitIfTouched) are profit-taking or entry orders (tp).
/// - Post-only orders are implemented via ALO (Add Liquidity Only) time-in-force.
pub const HYPERLIQUID_SUPPORTED_ORDER_TYPES: &[OrderType] = &[
    OrderType::Market,          // IOC limit order
    OrderType::Limit,           // Standard limit with GTC/IOC/ALO
    OrderType::StopMarket,      // Protective stop with market execution
    OrderType::StopLimit,       // Protective stop with limit price
    OrderType::MarketIfTouched, // Profit-taking/entry with market execution
    OrderType::LimitIfTouched,  // Profit-taking/entry with limit price
];

/// Conditional order types that use trigger orders on Hyperliquid.
///
/// These order types require a trigger_price and are implemented using
/// HyperliquidExecOrderKind::Trigger with appropriate parameters.
pub const HYPERLIQUID_CONDITIONAL_ORDER_TYPES: &[OrderType] = &[
    OrderType::StopMarket,
    OrderType::StopLimit,
    OrderType::MarketIfTouched,
    OrderType::LimitIfTouched,
];

/// Gets WebSocket URL for the specified network.
pub fn ws_url(is_testnet: bool) -> &'static str {
    if is_testnet {
        HYPERLIQUID_TESTNET_WS_URL
    } else {
        HYPERLIQUID_WS_URL
    }
}

/// Gets info API URL for the specified network.
pub fn info_url(is_testnet: bool) -> &'static str {
    if is_testnet {
        HYPERLIQUID_TESTNET_INFO_URL
    } else {
        HYPERLIQUID_INFO_URL
    }
}

/// Gets exchange API URL for the specified network.
pub fn exchange_url(is_testnet: bool) -> &'static str {
    if is_testnet {
        HYPERLIQUID_TESTNET_EXCHANGE_URL
    } else {
        HYPERLIQUID_EXCHANGE_URL
    }
}

// Default configuration values
// Server closes if no message in last 60s, so ping every 30s
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
pub const RECONNECT_BASE_BACKOFF: Duration = Duration::from_millis(250);
pub const RECONNECT_MAX_BACKOFF: Duration = Duration::from_secs(30);
pub const HTTP_TIMEOUT: Duration = Duration::from_secs(10);
// Max 100 inflight WS post messages per Hyperliquid docs
pub const INFLIGHT_MAX: usize = 100;
pub const QUEUE_MAX: usize = 1000;

fn stable_delay_hash(input: &str) -> u64 {
    const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    input.bytes().fold(FNV_OFFSET_BASIS, |hash, byte| {
        hash.wrapping_mul(FNV_PRIME) ^ u64::from(byte)
    })
}

#[must_use]
pub fn is_official_hyperliquid_ws_url(url: &str) -> bool {
    matches!(url, HYPERLIQUID_WS_URL | HYPERLIQUID_TESTNET_WS_URL)
}

#[must_use]
pub fn reconnect_tuning(url: &str) -> ReconnectTuning {
    if is_official_hyperliquid_ws_url(url) {
        ReconnectTuning {
            initial_delay_ms: PROD_RECONNECT_INITIAL_DELAY_MS,
            max_delay_ms: PROD_RECONNECT_MAX_DELAY_MS,
            jitter_ms: PROD_RECONNECT_JITTER_MS,
        }
    } else {
        ReconnectTuning {
            initial_delay_ms: RECONNECT_BASE_BACKOFF.as_millis() as u64,
            max_delay_ms: 5_000,
            jitter_ms: 200,
        }
    }
}

#[must_use]
pub fn startup_connect_delay(identity: &str, url: &str) -> Duration {
    if !is_official_hyperliquid_ws_url(url) {
        return Duration::ZERO;
    }

    Duration::from_millis(stable_delay_hash(identity) % PROD_STARTUP_CONNECT_SPREAD_MS)
}

#[must_use]
pub fn startup_connect_identity(role: &str, client_id: impl std::fmt::Display) -> String {
    startup_connect_identity_for_process(role, client_id, std::process::id())
}

#[must_use]
pub fn startup_connect_identity_for_process(
    role: &str,
    client_id: impl std::fmt::Display,
    process_id: u32,
) -> String {
    format!("{role}:pid={process_id}:{client_id}")
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_ws_url() {
        assert_eq!(ws_url(false), HYPERLIQUID_WS_URL);
        assert_eq!(ws_url(true), HYPERLIQUID_TESTNET_WS_URL);
    }

    #[rstest]
    fn test_info_url() {
        assert_eq!(info_url(false), HYPERLIQUID_INFO_URL);
        assert_eq!(info_url(true), HYPERLIQUID_TESTNET_INFO_URL);
    }

    #[rstest]
    fn test_exchange_url() {
        assert_eq!(exchange_url(false), HYPERLIQUID_EXCHANGE_URL);
        assert_eq!(exchange_url(true), HYPERLIQUID_TESTNET_EXCHANGE_URL);
    }

    #[rstest]
    fn test_constants_values() {
        assert_eq!(HEARTBEAT_INTERVAL, Duration::from_secs(30));
        assert_eq!(RECONNECT_BASE_BACKOFF, Duration::from_millis(250));
        assert_eq!(RECONNECT_MAX_BACKOFF, Duration::from_secs(30));
        assert_eq!(HTTP_TIMEOUT, Duration::from_secs(10));
        assert_eq!(INFLIGHT_MAX, 100);
        assert_eq!(QUEUE_MAX, 1000);
    }

    #[rstest]
    fn test_prod_hyperliquid_ws_urls_use_spread_out_connect_delay() {
        let first = startup_connect_delay("data:EQUITIES-LIVE-AMD", HYPERLIQUID_WS_URL);
        let second = startup_connect_delay("exec:EQUITIES-LIVE-AMD", HYPERLIQUID_WS_URL);

        let spread_limit = Duration::from_millis(PROD_STARTUP_CONNECT_SPREAD_MS);

        assert!(first <= spread_limit);
        assert!(second <= spread_limit);
        assert_ne!(
            first, second,
            "different connection identities should spread startup load"
        );
    }

    #[rstest]
    fn test_prod_shared_client_ids_still_spread_by_process() {
        let first = startup_connect_delay(
            &startup_connect_identity_for_process("data", "HYPERLIQUID", 1001),
            HYPERLIQUID_WS_URL,
        );
        let second = startup_connect_delay(
            &startup_connect_identity_for_process("data", "HYPERLIQUID", 1002),
            HYPERLIQUID_WS_URL,
        );

        assert_ne!(
            first, second,
            "distinct node processes sharing the same Hyperliquid client id must not collide"
        );
    }

    #[rstest]
    fn test_prod_shared_client_ids_still_spread_data_vs_exec_within_process() {
        let first = startup_connect_delay(
            &startup_connect_identity_for_process("data", "HYPERLIQUID", 4242),
            HYPERLIQUID_WS_URL,
        );
        let second = startup_connect_delay(
            &startup_connect_identity_for_process("exec", "HYPERLIQUID", 4242),
            HYPERLIQUID_WS_URL,
        );

        assert_ne!(
            first, second,
            "data and exec sockets in the same node process must not reuse the same spread slot"
        );
    }

    #[rstest]
    fn test_non_prod_ws_urls_skip_connect_delay() {
        assert_eq!(
            startup_connect_delay("data:test", "ws://127.0.0.1:9999/ws"),
            Duration::ZERO
        );
        assert_eq!(
            startup_connect_delay("data:test", "ws://localhost:9999/ws"),
            Duration::ZERO
        );
    }

    #[rstest]
    fn test_prod_hyperliquid_ws_urls_use_slower_reconnect_tuning() {
        let prod = reconnect_tuning(HYPERLIQUID_WS_URL);
        let local = reconnect_tuning("ws://127.0.0.1:9999/ws");

        assert!(
            prod.initial_delay_ms > local.initial_delay_ms,
            "prod reconnects should start with a larger delay to avoid venue bursts"
        );
        assert!(
            prod.jitter_ms > local.jitter_ms,
            "prod reconnects should use wider jitter to avoid synchronized retries"
        );
    }

    #[rstest]
    fn test_prod_startup_connect_identity_uses_runtime_process_id() {
        let expected = startup_connect_identity_for_process("exec", "HYPERLIQUID", std::process::id());

        assert_eq!(startup_connect_identity("exec", "HYPERLIQUID"), expected);
    }
}
