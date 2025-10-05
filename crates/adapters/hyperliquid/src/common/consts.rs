// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

use std::{sync::LazyLock, time::Duration};

use nautilus_model::{enums::OrderType, identifiers::Venue};
use ustr::Ustr;

pub const HYPERLIQUID: &str = "HYPERLIQUID";
pub static HYPERLIQUID_VENUE: LazyLock<Venue> =
    LazyLock::new(|| Venue::new(Ustr::from(HYPERLIQUID)));

// Mainnet URLs
pub const HYPERLIQUID_WS_URL: &str = "wss://api.hyperliquid.xyz/ws";
pub const HYPERLIQUID_INFO_URL: &str = "https://api.hyperliquid.xyz/info";
pub const HYPERLIQUID_EXCHANGE_URL: &str = "https://api.hyperliquid.xyz/exchange";

// Testnet URLs
pub const HYPERLIQUID_TESTNET_WS_URL: &str = "wss://api.hyperliquid-testnet.xyz/ws";
pub const HYPERLIQUID_TESTNET_INFO_URL: &str = "https://api.hyperliquid-testnet.xyz/info";
pub const HYPERLIQUID_TESTNET_EXCHANGE_URL: &str = "https://api.hyperliquid-testnet.xyz/exchange";

/// Hyperliquid supported order types.
///
/// # Notes
///
/// - All order types support trigger prices except Market and Limit.
/// - Conditional orders follow patterns from OKX, Bybit, and BitMEX adapters.
/// - Stop orders (StopMarket/StopLimit) are protective stops (sl).
/// - If Touched orders (MarketIfTouched/LimitIfTouched) are profit-taking or entry orders (tp).
/// - Post-only orders are implemented via ALO (Add Liquidity Only) time-in-force.
///
/// # Trigger Semantics
///
/// Hyperliquid uses last traded price for trigger evaluation.
/// Future enhancement: Add support for mark/index price triggers if API supports it.
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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

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
}
