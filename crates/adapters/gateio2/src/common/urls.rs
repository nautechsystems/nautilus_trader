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

//! Gate.io URL management for API endpoints.

use crate::common::consts::*;

/// Manages URLs for Gate.io API endpoints.
#[derive(Clone, Debug)]
pub struct GateioUrls {
    base_http: String,
    base_ws_spot: String,
    base_ws_futures: String,
    base_ws_options: String,
}

impl GateioUrls {
    /// Creates a new URL manager with the given base URLs.
    ///
    /// If any URL is `None`, the default mainnet URL is used.
    #[must_use]
    pub fn new(
        base_http: Option<String>,
        base_ws_spot: Option<String>,
        base_ws_futures: Option<String>,
        base_ws_options: Option<String>,
    ) -> Self {
        Self {
            base_http: base_http.unwrap_or_else(|| GATEIO_HTTP_BASE_URL.to_string()),
            base_ws_spot: base_ws_spot.unwrap_or_else(|| GATEIO_WS_SPOT_URL.to_string()),
            base_ws_futures: base_ws_futures
                .unwrap_or_else(|| GATEIO_WS_FUTURES_URL.to_string()),
            base_ws_options: base_ws_options
                .unwrap_or_else(|| GATEIO_WS_OPTIONS_URL.to_string()),
        }
    }

    /// Returns the base HTTP URL.
    #[must_use]
    pub fn base_http(&self) -> &str {
        &self.base_http
    }

    /// Returns the base WebSocket URL for spot.
    #[must_use]
    pub fn base_ws_spot(&self) -> &str {
        &self.base_ws_spot
    }

    /// Returns the base WebSocket URL for futures.
    #[must_use]
    pub fn base_ws_futures(&self) -> &str {
        &self.base_ws_futures
    }

    /// Returns the base WebSocket URL for options.
    #[must_use]
    pub fn base_ws_options(&self) -> &str {
        &self.base_ws_options
    }

    // ========== Spot Endpoints ==========

    /// Returns the spot currency pairs endpoint.
    #[must_use]
    pub fn spot_currency_pairs(&self) -> String {
        format!("{}/spot/currency_pairs", self.base_http)
    }

    /// Returns the spot accounts endpoint.
    #[must_use]
    pub fn spot_accounts(&self) -> String {
        format!("{}/spot/accounts", self.base_http)
    }

    /// Returns the spot order book endpoint for a given currency pair.
    #[must_use]
    pub fn spot_order_book(&self, currency_pair: &str) -> String {
        format!(
            "{}/spot/order_book?currency_pair={}",
            self.base_http, currency_pair
        )
    }

    /// Returns the spot trades endpoint for a given currency pair.
    #[must_use]
    pub fn spot_trades(&self, currency_pair: &str) -> String {
        format!(
            "{}/spot/trades?currency_pair={}",
            self.base_http, currency_pair
        )
    }

    /// Returns the spot orders endpoint.
    #[must_use]
    pub fn spot_orders(&self) -> String {
        format!("{}/spot/orders", self.base_http)
    }

    /// Returns the spot order endpoint for a given order ID and currency pair.
    #[must_use]
    pub fn spot_order(&self, order_id: &str, currency_pair: &str) -> String {
        format!(
            "{}/spot/orders/{}?currency_pair={}",
            self.base_http, order_id, currency_pair
        )
    }

    // ========== Futures Endpoints ==========

    /// Returns the futures contracts endpoint for a given settle currency.
    #[must_use]
    pub fn futures_contracts(&self, settle: &str) -> String {
        format!("{}/futures/{}/contracts", self.base_http, settle)
    }

    /// Returns the futures accounts endpoint for a given settle currency.
    #[must_use]
    pub fn futures_accounts(&self, settle: &str) -> String {
        format!("{}/futures/{}/accounts", self.base_http, settle)
    }

    /// Returns the futures order book endpoint.
    #[must_use]
    pub fn futures_order_book(&self, settle: &str, contract: &str) -> String {
        format!(
            "{}/futures/{}/order_book?contract={}",
            self.base_http, settle, contract
        )
    }

    /// Returns the futures trades endpoint.
    #[must_use]
    pub fn futures_trades(&self, settle: &str, contract: &str) -> String {
        format!(
            "{}/futures/{}/trades?contract={}",
            self.base_http, settle, contract
        )
    }

    /// Returns the futures orders endpoint.
    #[must_use]
    pub fn futures_orders(&self, settle: &str) -> String {
        format!("{}/futures/{}/orders", self.base_http, settle)
    }

    /// Returns the futures positions endpoint.
    #[must_use]
    pub fn futures_positions(&self, settle: &str) -> String {
        format!("{}/futures/{}/positions", self.base_http, settle)
    }

    // ========== Wallet Endpoints ==========

    /// Returns the wallet deposits endpoint.
    #[must_use]
    pub fn wallet_deposits(&self) -> String {
        format!("{}/wallet/deposits", self.base_http)
    }

    /// Returns the wallet withdrawals endpoint.
    #[must_use]
    pub fn wallet_withdrawals(&self) -> String {
        format!("{}/wallet/withdrawals", self.base_http)
    }

    /// Returns the wallet total balance endpoint.
    #[must_use]
    pub fn wallet_total_balance(&self) -> String {
        format!("{}/wallet/total_balance", self.base_http)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_urls() {
        let urls = GateioUrls::new(None, None, None, None);
        assert_eq!(urls.base_http(), GATEIO_HTTP_BASE_URL);
        assert_eq!(urls.base_ws_spot(), GATEIO_WS_SPOT_URL);
        assert_eq!(urls.base_ws_futures(), GATEIO_WS_FUTURES_URL);
        assert_eq!(urls.base_ws_options(), GATEIO_WS_OPTIONS_URL);
    }

    #[test]
    fn test_custom_urls() {
        let custom_http = "https://custom.gateio.api".to_string();
        let custom_ws_spot = "wss://custom.gateio.ws/spot".to_string();
        let urls = GateioUrls::new(
            Some(custom_http.clone()),
            Some(custom_ws_spot.clone()),
            None,
            None,
        );
        assert_eq!(urls.base_http(), custom_http);
        assert_eq!(urls.base_ws_spot(), custom_ws_spot);
    }

    #[test]
    fn test_spot_endpoints() {
        let urls = GateioUrls::new(None, None, None, None);

        assert!(urls.spot_currency_pairs().contains("spot/currency_pairs"));
        assert!(urls.spot_accounts().contains("spot/accounts"));

        let orderbook = urls.spot_order_book("BTC_USDT");
        assert!(orderbook.contains("spot/order_book"));
        assert!(orderbook.contains("BTC_USDT"));

        let trades = urls.spot_trades("BTC_USDT");
        assert!(trades.contains("spot/trades"));
        assert!(trades.contains("BTC_USDT"));
    }

    #[test]
    fn test_futures_endpoints() {
        let urls = GateioUrls::new(None, None, None, None);

        let contracts = urls.futures_contracts("usdt");
        assert!(contracts.contains("futures/usdt/contracts"));

        let orderbook = urls.futures_order_book("usdt", "BTC_USDT");
        assert!(orderbook.contains("futures/usdt/order_book"));
        assert!(orderbook.contains("BTC_USDT"));

        let positions = urls.futures_positions("usdt");
        assert!(positions.contains("futures/usdt/positions"));
    }

    #[test]
    fn test_wallet_endpoints() {
        let urls = GateioUrls::new(None, None, None, None);

        assert!(urls.wallet_deposits().contains("wallet/deposits"));
        assert!(urls.wallet_withdrawals().contains("wallet/withdrawals"));
        assert!(urls
            .wallet_total_balance()
            .contains("wallet/total_balance"));
    }
}
