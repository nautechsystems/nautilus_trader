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

//! HTTP client implementation for Lighter REST API.

use std::sync::Arc;

use std::collections::HashMap;

use dashmap::DashMap;
use nautilus_model::instruments::InstrumentAny;
use nautilus_network::http::HttpClient as NetworkHttpClient;
use reqwest::Method;
use serde_json::Value;
use tokio::sync::RwLock;
use tracing::{debug, trace};

use crate::common::{
    credential::LighterCredentials,
    models::{LighterAccount, LighterMarket, LighterOrder, LighterTrade},
    parse::parse_instrument,
    urls::LighterUrls,
};

use super::error::{LighterHttpError, LighterHttpResult};

/// Inner implementation of the Lighter HTTP client.
#[derive(Debug, Clone)]
struct LighterHttpClientInner {
    /// Network HTTP client.
    client: NetworkHttpClient,
    /// URL manager.
    urls: LighterUrls,
    /// API credentials (optional for public endpoints).
    credentials: Option<LighterCredentials>,
    /// Instrument cache.
    instruments: Arc<DashMap<String, InstrumentAny>>,
    /// Current nonce for transactions.
    nonce: Arc<RwLock<u64>>,
}

impl LighterHttpClientInner {
    /// Creates a new inner HTTP client.
    fn new(
        client: NetworkHttpClient,
        urls: LighterUrls,
        credentials: Option<LighterCredentials>,
    ) -> Self {
        Self {
            client,
            urls,
            credentials,
            instruments: Arc::new(DashMap::new()),
            nonce: Arc::new(RwLock::new(0)),
        }
    }

    /// Fetches markets/instruments from the API.
    async fn request_markets(&self) -> LighterHttpResult<Vec<LighterMarket>> {
        let url = self.urls.markets();
        debug!("Fetching markets from: {}", url);

        let response = self
            .client
            .request(Method::GET, url, None, None, None, None)
            .await
            .map_err(|e| LighterHttpError::Other(e.to_string()))?;

        let body = String::from_utf8(response.body.to_vec())
            .map_err(|e| LighterHttpError::Other(e.to_string()))?;
        trace!("Markets response: {}", body);

        let markets: Vec<LighterMarket> = serde_json::from_str(&body)?;
        Ok(markets)
    }

    /// Fetches account information.
    async fn request_account(&self, account_id: Option<u64>) -> LighterHttpResult<LighterAccount> {
        let url = self.urls.account(account_id);
        debug!("Fetching account from: {}", url);

        let response = self
            .client
            .request(Method::GET, url, None, None, None, None)
            .await
            .map_err(|e| LighterHttpError::Other(e.to_string()))?;

        let body = String::from_utf8(response.body.to_vec())
            .map_err(|e| LighterHttpError::Other(e.to_string()))?;
        trace!("Account response: {}", body);

        let account: LighterAccount = serde_json::from_str(&body)?;
        Ok(account)
    }

    /// Fetches order book for a market.
    async fn request_order_book(&self, market_id: u64) -> LighterHttpResult<Value> {
        let url = self.urls.order_book(market_id);
        debug!("Fetching order book from: {}", url);

        let response = self
            .client
            .request(Method::GET, url, None, None, None, None)
            .await
            .map_err(|e| LighterHttpError::Other(e.to_string()))?;

        let body = String::from_utf8(response.body.to_vec())
            .map_err(|e| LighterHttpError::Other(e.to_string()))?;
        trace!("Order book response: {}", body);

        let data: Value = serde_json::from_str(&body)?;
        Ok(data)
    }

    /// Fetches recent trades for a market.
    async fn request_trades(&self, market_id: u64) -> LighterHttpResult<Vec<LighterTrade>> {
        let url = self.urls.trades(market_id);
        debug!("Fetching trades from: {}", url);

        let response = self
            .client
            .request(Method::GET, url, None, None, None, None)
            .await
            .map_err(|e| LighterHttpError::Other(e.to_string()))?;

        let body = String::from_utf8(response.body.to_vec())
            .map_err(|e| LighterHttpError::Other(e.to_string()))?;
        trace!("Trades response: {}", body);

        let trades: Vec<LighterTrade> = serde_json::from_str(&body)?;
        Ok(trades)
    }

    /// Fetches orders for an account.
    async fn request_orders(&self, account_id: Option<u64>) -> LighterHttpResult<Vec<LighterOrder>> {
        let url = self.urls.orders(account_id);
        debug!("Fetching orders from: {}", url);

        let response = self
            .client
            .request(Method::GET, url, None, None, None, None)
            .await
            .map_err(|e| LighterHttpError::Other(e.to_string()))?;

        let body = String::from_utf8(response.body.to_vec())
            .map_err(|e| LighterHttpError::Other(e.to_string()))?;
        trace!("Orders response: {}", body);

        let orders: Vec<LighterOrder> = serde_json::from_str(&body)?;
        Ok(orders)
    }

    /// Gets the next nonce for transaction signing.
    async fn get_next_nonce(&self) -> LighterHttpResult<u64> {
        if let Some(ref creds) = self.credentials {
            let url = self.urls.nonce(creds.api_key_index());
            debug!("Fetching nonce from: {}", url);

            let response = self
                .client
                .request(Method::GET, url, None, None, None, None)
                .await
                .map_err(|e| LighterHttpError::Other(e.to_string()))?;

            let body = String::from_utf8(response.body.to_vec())
                .map_err(|e| LighterHttpError::Other(e.to_string()))?;
            trace!("Nonce response: {}", body);

            let data: Value = serde_json::from_str(&body)?;
            let nonce = data["nonce"].as_u64().ok_or_else(|| {
                LighterHttpError::Other("Failed to parse nonce from response".to_string())
            })?;

            // Update local nonce
            let mut current_nonce = self.nonce.write().await;
            *current_nonce = nonce;

            Ok(nonce)
        } else {
            Err(LighterHttpError::Authentication(
                "No credentials configured".to_string(),
            ))
        }
    }

    /// Loads instruments into the cache.
    async fn load_instruments(&self) -> LighterHttpResult<Vec<InstrumentAny>> {
        let markets = self.request_markets().await?;
        let mut instruments = Vec::new();

        for market in markets {
            match parse_instrument(&market) {
                Ok(instrument) => {
                    self.instruments.insert(market.symbol.clone(), instrument.clone());
                    instruments.push(instrument);
                }
                Err(e) => {
                    debug!("Failed to parse instrument {}: {}", market.symbol, e);
                }
            }
        }

        debug!("Loaded {} instruments into cache", instruments.len());
        Ok(instruments)
    }
}

/// Lighter HTTP client for REST API access.
///
/// Follows the Arc<Inner> pattern for efficient cloning.
#[derive(Debug, Clone)]
pub struct LighterHttpClient {
    inner: Arc<LighterHttpClientInner>,
}

impl LighterHttpClient {
    /// Creates a new Lighter HTTP client.
    ///
    /// # Arguments
    ///
    /// * `base_http_url` - Base HTTP URL (None for default mainnet)
    /// * `base_ws_url` - Base WebSocket URL (None for default mainnet)
    /// * `is_testnet` - Whether to use testnet
    /// * `credentials` - API credentials (optional for public endpoints)
    #[must_use]
    pub fn new(
        base_http_url: Option<String>,
        base_ws_url: Option<String>,
        is_testnet: bool,
        credentials: Option<LighterCredentials>,
    ) -> Self {
        let urls = LighterUrls::new(base_http_url, base_ws_url, is_testnet);

        // Create HTTP client with default configuration
        let client = NetworkHttpClient::new(
            HashMap::new(), // headers
            Vec::new(),     // header_keys
            Vec::new(),     // keyed_quotas
            None,           // default_quota
            None,           // timeout_secs
            None,           // proxy_url
        ).expect("Failed to create HTTP client");

        Self {
            inner: Arc::new(LighterHttpClientInner::new(client, urls, credentials)),
        }
    }

    /// Fetches all markets/instruments.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn request_markets(&self) -> LighterHttpResult<Vec<LighterMarket>> {
        self.inner.request_markets().await
    }

    /// Fetches account information.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn request_account(&self, account_id: Option<u64>) -> LighterHttpResult<LighterAccount> {
        self.inner.request_account(account_id).await
    }

    /// Fetches order book for a market.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn request_order_book(&self, market_id: u64) -> LighterHttpResult<Value> {
        self.inner.request_order_book(market_id).await
    }

    /// Fetches recent trades for a market.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn request_trades(&self, market_id: u64) -> LighterHttpResult<Vec<LighterTrade>> {
        self.inner.request_trades(market_id).await
    }

    /// Fetches orders for an account.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn request_orders(&self, account_id: Option<u64>) -> LighterHttpResult<Vec<LighterOrder>> {
        self.inner.request_orders(account_id).await
    }

    /// Loads instruments into cache.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn load_instruments(&self) -> LighterHttpResult<Vec<InstrumentAny>> {
        self.inner.load_instruments().await
    }

    /// Gets the next nonce for transaction signing.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or credentials are missing.
    pub async fn get_next_nonce(&self) -> LighterHttpResult<u64> {
        self.inner.get_next_nonce().await
    }

    /// Returns a reference to the instrument cache.
    #[must_use]
    pub fn instruments(&self) -> &DashMap<String, InstrumentAny> {
        &self.inner.instruments
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = LighterHttpClient::new(None, None, false, None);
        assert_eq!(client.instruments().len(), 0);
    }
}
