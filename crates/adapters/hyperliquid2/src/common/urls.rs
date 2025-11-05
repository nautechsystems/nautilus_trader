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

//! Hyperliquid URL management.

use super::consts::{
    HYPERLIQUID_EXCHANGE_PATH, HYPERLIQUID_HTTP_BASE_URL, HYPERLIQUID_HTTP_TESTNET_URL,
    HYPERLIQUID_INFO_PATH, HYPERLIQUID_WS_BASE_URL, HYPERLIQUID_WS_TESTNET_URL,
};

/// Manages Hyperliquid API URLs
#[derive(Debug, Clone)]
pub struct HyperliquidUrls {
    /// Base HTTP URL
    pub http_base: String,
    /// Base WebSocket URL
    pub ws_base: String,
    /// Info endpoint URL
    pub info: String,
    /// Exchange endpoint URL
    pub exchange: String,
}

impl HyperliquidUrls {
    /// Creates a new [`HyperliquidUrls`] instance
    ///
    /// # Parameters
    /// - `http_base`: Optional custom HTTP base URL (defaults to mainnet)
    /// - `ws_base`: Optional custom WebSocket base URL (defaults to mainnet)
    /// - `testnet`: Whether to use testnet URLs
    pub fn new(
        http_base: Option<String>,
        ws_base: Option<String>,
        testnet: bool,
    ) -> anyhow::Result<Self> {
        let default_http = if testnet {
            HYPERLIQUID_HTTP_TESTNET_URL
        } else {
            HYPERLIQUID_HTTP_BASE_URL
        };

        let default_ws = if testnet {
            HYPERLIQUID_WS_TESTNET_URL
        } else {
            HYPERLIQUID_WS_BASE_URL
        };

        let http_base = http_base.unwrap_or_else(|| default_http.to_string());
        let ws_base = ws_base.unwrap_or_else(|| default_ws.to_string());

        let info = format!("{}{}", http_base, HYPERLIQUID_INFO_PATH);
        let exchange = format!("{}{}", http_base, HYPERLIQUID_EXCHANGE_PATH);

        Ok(Self {
            http_base,
            ws_base,
            info,
            exchange,
        })
    }
}

impl Default for HyperliquidUrls {
    fn default() -> Self {
        Self::new(None, None, false).expect("Failed to create default HyperliquidUrls")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_urls() {
        let urls = HyperliquidUrls::default();
        assert_eq!(urls.http_base, HYPERLIQUID_HTTP_BASE_URL);
        assert_eq!(urls.ws_base, HYPERLIQUID_WS_BASE_URL);
        assert_eq!(urls.info, "https://api.hyperliquid.xyz/info");
        assert_eq!(urls.exchange, "https://api.hyperliquid.xyz/exchange");
    }

    #[test]
    fn test_testnet_urls() {
        let urls = HyperliquidUrls::new(None, None, true).unwrap();
        assert_eq!(urls.http_base, HYPERLIQUID_HTTP_TESTNET_URL);
        assert_eq!(urls.ws_base, HYPERLIQUID_WS_TESTNET_URL);
    }

    #[test]
    fn test_custom_urls() {
        let custom_http = "https://custom-api.example.com".to_string();
        let custom_ws = "wss://custom-ws.example.com".to_string();

        let urls = HyperliquidUrls::new(Some(custom_http.clone()), Some(custom_ws.clone()), false).unwrap();

        assert_eq!(urls.http_base, custom_http);
        assert_eq!(urls.ws_base, custom_ws);
        assert_eq!(urls.info, "https://custom-api.example.com/info");
        assert_eq!(urls.exchange, "https://custom-api.example.com/exchange");
    }
}
