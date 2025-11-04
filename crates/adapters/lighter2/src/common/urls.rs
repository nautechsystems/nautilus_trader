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

//! URL management for Lighter API endpoints.

use super::consts::{
    LIGHTER_MAINNET_HTTP_URL, LIGHTER_MAINNET_WS_URL, LIGHTER_TESTNET_HTTP_URL,
    LIGHTER_TESTNET_WS_URL,
};

/// URL manager for Lighter API endpoints.
#[derive(Debug, Clone)]
pub struct LighterUrls {
    base_http: String,
    base_ws: String,
}

impl LighterUrls {
    /// Creates a new URL manager.
    ///
    /// # Arguments
    ///
    /// * `base_http` - Base HTTP URL (None for default mainnet)
    /// * `base_ws` - Base WebSocket URL (None for default mainnet)
    /// * `is_testnet` - Whether to use testnet URLs
    #[must_use]
    pub fn new(base_http: Option<String>, base_ws: Option<String>, is_testnet: bool) -> Self {
        let base_http = base_http.unwrap_or_else(|| {
            if is_testnet {
                LIGHTER_TESTNET_HTTP_URL.to_string()
            } else {
                LIGHTER_MAINNET_HTTP_URL.to_string()
            }
        });

        let base_ws = base_ws.unwrap_or_else(|| {
            if is_testnet {
                LIGHTER_TESTNET_WS_URL.to_string()
            } else {
                LIGHTER_MAINNET_WS_URL.to_string()
            }
        });

        Self { base_http, base_ws }
    }

    /// Returns the base HTTP URL.
    #[must_use]
    pub fn base_http(&self) -> &str {
        &self.base_http
    }

    /// Returns the base WebSocket URL.
    #[must_use]
    pub fn base_ws(&self) -> &str {
        &self.base_ws
    }

    /// Returns the account endpoint URL.
    #[must_use]
    pub fn account(&self, account_id: Option<u64>) -> String {
        match account_id {
            Some(id) => format!("{}/api/account?by=index&value={}", self.base_http, id),
            None => format!("{}/api/account", self.base_http),
        }
    }

    /// Returns the markets endpoint URL.
    #[must_use]
    pub fn markets(&self) -> String {
        format!("{}/api/markets", self.base_http)
    }

    /// Returns the order book endpoint URL.
    #[must_use]
    pub fn order_book(&self, market_id: u64) -> String {
        format!("{}/api/orderbook?market_id={}", self.base_http, market_id)
    }

    /// Returns the trades endpoint URL.
    #[must_use]
    pub fn trades(&self, market_id: u64) -> String {
        format!("{}/api/trades?market_id={}", self.base_http, market_id)
    }

    /// Returns the orders endpoint URL.
    #[must_use]
    pub fn orders(&self, account_id: Option<u64>) -> String {
        match account_id {
            Some(id) => format!("{}/api/orders?account_id={}", self.base_http, id),
            None => format!("{}/api/orders", self.base_http),
        }
    }

    /// Returns the transaction nonce endpoint URL.
    #[must_use]
    pub fn nonce(&self, api_key_index: u8) -> String {
        format!("{}/api/transaction/nonce?api_key_index={}", self.base_http, api_key_index)
    }

    /// Returns the transaction send endpoint URL.
    #[must_use]
    pub fn send_transaction(&self) -> String {
        format!("{}/api/transaction/send", self.base_http)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mainnet_urls() {
        let urls = LighterUrls::new(None, None, false);
        assert_eq!(urls.base_http(), LIGHTER_MAINNET_HTTP_URL);
        assert_eq!(urls.base_ws(), LIGHTER_MAINNET_WS_URL);
    }

    #[test]
    fn test_testnet_urls() {
        let urls = LighterUrls::new(None, None, true);
        assert_eq!(urls.base_http(), LIGHTER_TESTNET_HTTP_URL);
        assert_eq!(urls.base_ws(), LIGHTER_TESTNET_WS_URL);
    }
}
