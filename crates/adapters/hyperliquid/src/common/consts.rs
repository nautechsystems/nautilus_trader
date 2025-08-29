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

//! Constants and configuration for Hyperliquid API.

use std::{env, sync::LazyLock, time::Duration};

use nautilus_model::identifiers::Venue;
use ustr::Ustr;

pub const HYPERLIQUID: &str = "HYPERLIQUID";
pub static HYPERLIQUID_VENUE: LazyLock<Venue> =
    LazyLock::new(|| Venue::new(Ustr::from(HYPERLIQUID)));

/// Represents the network configuration for Hyperliquid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Network {
    Mainnet,
    Testnet,
}

impl Network {
    /// Loads network from environment variable `HYPERLIQUID_NET`.
    ///
    /// Defaults to `Mainnet` if not set or invalid.
    pub fn from_env() -> Self {
        match env::var("HYPERLIQUID_NET")
            .unwrap_or_else(|_| "mainnet".to_string())
            .to_lowercase()
            .as_str()
        {
            "testnet" | "test" => Network::Testnet,
            _ => Network::Mainnet,
        }
    }
}

// Mainnet URLs
pub const HYPERLIQUID_WS_URL: &str = "wss://api.hyperliquid.xyz/ws";
pub const HYPERLIQUID_INFO_URL: &str = "https://api.hyperliquid.xyz/info";
pub const HYPERLIQUID_EXCHANGE_URL: &str = "https://api.hyperliquid.xyz/exchange";

// Testnet URLs
pub const HYPERLIQUID_TESTNET_WS_URL: &str = "wss://api.hyperliquid-testnet.xyz/ws";
pub const HYPERLIQUID_TESTNET_INFO_URL: &str = "https://api.hyperliquid-testnet.xyz/info";
pub const HYPERLIQUID_TESTNET_EXCHANGE_URL: &str = "https://api.hyperliquid-testnet.xyz/exchange";

/// Gets WebSocket URL for the specified network.
pub fn ws_url(network: Network) -> &'static str {
    match network {
        Network::Mainnet => HYPERLIQUID_WS_URL,
        Network::Testnet => HYPERLIQUID_TESTNET_WS_URL,
    }
}

/// Gets info API URL for the specified network.
pub fn info_url(network: Network) -> &'static str {
    match network {
        Network::Mainnet => HYPERLIQUID_INFO_URL,
        Network::Testnet => HYPERLIQUID_TESTNET_INFO_URL,
    }
}

/// Gets exchange API URL for the specified network.
pub fn exchange_url(network: Network) -> &'static str {
    match network {
        Network::Mainnet => HYPERLIQUID_EXCHANGE_URL,
        Network::Testnet => HYPERLIQUID_TESTNET_EXCHANGE_URL,
    }
}

/// WebSocket channel names from Hyperliquid docs
pub mod channels {
    pub const SUBSCRIPTION_RESPONSE: &str = "subscriptionResponse";
    pub const TRADES: &str = "trades";
    pub const L2_BOOK: &str = "l2Book";
    pub const BBO: &str = "bbo";
    pub const ORDER_UPDATES: &str = "orderUpdates";
    pub const USER_EVENTS: &str = "userEvents";
    pub const USER_FILLS: &str = "userFills";
    pub const USER_FUNDINGS: &str = "userFundings";
    pub const USER_NON_FUNDING_LEDGER_UPDATES: &str = "userNonFundingLedgerUpdates";
    pub const POST: &str = "post";
    pub const PONG: &str = "pong";
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

// Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_variants() {
        // Arrange & Act & Assert
        assert_eq!(Network::Mainnet, Network::Mainnet);
        assert_eq!(Network::Testnet, Network::Testnet);
        assert_ne!(Network::Mainnet, Network::Testnet);
    }

    #[test]
    fn test_network_from_env_handles_default() {
        // Arrange & Act
        let network = Network::from_env();

        // Assert - Should be either Mainnet or Testnet
        assert!(matches!(network, Network::Mainnet | Network::Testnet));
    }

    #[test]
    fn test_ws_url() {
        // Arrange & Act & Assert
        assert_eq!(ws_url(Network::Mainnet), HYPERLIQUID_WS_URL);
        assert_eq!(ws_url(Network::Testnet), HYPERLIQUID_TESTNET_WS_URL);
    }

    #[test]
    fn test_info_url() {
        // Arrange & Act & Assert
        assert_eq!(info_url(Network::Mainnet), HYPERLIQUID_INFO_URL);
        assert_eq!(info_url(Network::Testnet), HYPERLIQUID_TESTNET_INFO_URL);
    }

    #[test]
    fn test_exchange_url() {
        // Arrange & Act & Assert
        assert_eq!(exchange_url(Network::Mainnet), HYPERLIQUID_EXCHANGE_URL);
        assert_eq!(
            exchange_url(Network::Testnet),
            HYPERLIQUID_TESTNET_EXCHANGE_URL
        );
    }

    #[test]
    fn test_constants_values() {
        // Arrange & Act & Assert
        assert_eq!(HEARTBEAT_INTERVAL, Duration::from_secs(30));
        assert_eq!(RECONNECT_BASE_BACKOFF, Duration::from_millis(250));
        assert_eq!(RECONNECT_MAX_BACKOFF, Duration::from_secs(30));
        assert_eq!(HTTP_TIMEOUT, Duration::from_secs(10));
        assert_eq!(INFLIGHT_MAX, 100);
        assert_eq!(QUEUE_MAX, 1000);
    }
}
