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

//! Hyperliquid HTTP client implementation.

use crate::common::{
    HyperliquidCredentials, HyperliquidHttpError, HyperliquidMetaInfo, HyperliquidUrls,
    HyperliquidL2Book, HyperliquidTrade, HyperliquidUserState, HyperliquidOpenOrder,
    HyperliquidUserFill, HyperliquidOrderRequest, HyperliquidOrderResponse, parse_instrument,
};
use nautilus_model::instruments::{Instrument, InstrumentAny};
use nautilus_network::http::HttpClient;
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc};
use tokio::sync::RwLock;
use tracing::{debug, error, trace};

/// Inner state for Hyperliquid HTTP client
pub struct Hyperliquid2HttpClientInner {
    /// HTTP client
    pub client: HttpClient,
    /// URL configuration
    pub urls: HyperliquidUrls,
    /// Credentials (optional for public endpoints)
    pub credentials: Option<HyperliquidCredentials>,
    /// Cached instruments
    pub instruments: RwLock<HashMap<String, InstrumentAny>>,
}

/// Hyperliquid HTTP client for REST API interactions
#[derive(Clone)]
pub struct Hyperliquid2HttpClient {
    inner: Arc<Hyperliquid2HttpClientInner>,
}

impl Hyperliquid2HttpClient {
    /// Creates a new [`Hyperliquid2HttpClient`] instance
    ///
    /// # Parameters
    /// - `private_key`: Optional Ethereum private key for authenticated requests
    /// - `http_base`: Optional custom HTTP base URL
    /// - `testnet`: Whether to use testnet (default: false)
    pub fn new(
        private_key: Option<String>,
        http_base: Option<String>,
        testnet: bool,
    ) -> anyhow::Result<Self> {
        let credentials = private_key
            .as_ref()
            .map(|key| HyperliquidCredentials::new(key))
            .transpose()?;

        let urls = HyperliquidUrls::new(http_base, None, testnet)?;

        // Create HTTP client with default configuration
        let client = HttpClient::new(
            HashMap::new(),
            Vec::new(),
            Vec::new(),
            None,
            None,
            None,
        )?;

        let inner = Hyperliquid2HttpClientInner {
            client,
            urls,
            credentials,
            instruments: RwLock::new(HashMap::new()),
        };

        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    /// Makes a POST request to the info endpoint
    async fn request_info(&self, payload: Value) -> Result<Value, HyperliquidHttpError> {
        trace!("Info request: {}", serde_json::to_string(&payload)?);

        let payload_str = serde_json::to_string(&payload)?;

        let response = self
            .inner
            .client
            .request(
                reqwest::Method::POST,
                self.inner.urls.info.clone(),
                None,
                Some(payload_str.into_bytes()),
                None,
                None,
            )
            .await
            .map_err(|e| HyperliquidHttpError::HttpRequest(e.to_string()))?;

        let response_text = String::from_utf8_lossy(&response.body).to_string();
        trace!("Info response: {}", response_text);

        let response_json: Value = serde_json::from_str(&response_text)?;
        Ok(response_json)
    }

    /// Makes an authenticated POST request to the exchange endpoint
    async fn request_exchange(&self, action: Value) -> Result<Value, HyperliquidHttpError> {
        let credentials = self
            .inner
            .credentials
            .as_ref()
            .ok_or_else(|| {
                HyperliquidHttpError::Authentication("No credentials provided".to_string())
            })?;

        let nonce = HyperliquidCredentials::generate_nonce();
        let (signature, _) = credentials.sign_l1_action(&action, nonce).await?;

        let payload = json!({
            "action": action,
            "nonce": nonce,
            "signature": signature,
        });

        trace!("Exchange request: {}", serde_json::to_string(&payload)?);

        let payload_str = serde_json::to_string(&payload)?;

        let response = self
            .inner
            .client
            .request(
                reqwest::Method::POST,
                self.inner.urls.exchange.clone(),
                None,
                Some(payload_str.into_bytes()),
                None,
                None,
            )
            .await
            .map_err(|e| HyperliquidHttpError::HttpRequest(e.to_string()))?;

        let response_text = String::from_utf8_lossy(&response.body).to_string();
        trace!("Exchange response: {}", response_text);

        let response_json: Value = serde_json::from_str(&response_text)?;
        Ok(response_json)
    }

    // ======================== Public API Methods ========================

    /// Fetches meta information (universe of assets)
    pub async fn request_meta_info(&self) -> Result<HyperliquidMetaInfo, HyperliquidHttpError> {
        let payload = json!({
            "type": "meta"
        });

        let response = self.request_info(payload).await?;
        let meta_info: HyperliquidMetaInfo = serde_json::from_value(response)?;
        Ok(meta_info)
    }

    /// Fetches all mids (mid prices for all assets)
    pub async fn request_all_mids(&self) -> Result<HashMap<String, String>, HyperliquidHttpError> {
        let payload = json!({
            "type": "allMids"
        });

        let response = self.request_info(payload).await?;
        let mids: HashMap<String, String> = serde_json::from_value(response)?;
        Ok(mids)
    }

    /// Fetches L2 order book for a specific coin
    pub async fn request_l2_book(&self, coin: &str) -> Result<HyperliquidL2Book, HyperliquidHttpError> {
        let payload = json!({
            "type": "l2Book",
            "coin": coin
        });

        let response = self.request_info(payload).await?;
        let book: HyperliquidL2Book = serde_json::from_value(response)?;
        Ok(book)
    }

    /// Fetches recent trades for a specific coin
    pub async fn request_trades(&self, coin: &str) -> Result<Vec<HyperliquidTrade>, HyperliquidHttpError> {
        let payload = json!({
            "type": "recentTrades",
            "coin": coin
        });

        let response = self.request_info(payload).await?;
        let trades: Vec<HyperliquidTrade> = serde_json::from_value(response)?;
        Ok(trades)
    }

    // ======================== Private API Methods ========================

    /// Fetches user state (positions, balances)
    pub async fn request_user_state(&self, user: &str) -> Result<HyperliquidUserState, HyperliquidHttpError> {
        let payload = json!({
            "type": "clearinghouseState",
            "user": user
        });

        let response = self.request_info(payload).await?;
        let state: HyperliquidUserState = serde_json::from_value(response)?;
        Ok(state)
    }

    /// Fetches open orders for a user
    pub async fn request_open_orders(&self, user: &str) -> Result<Vec<HyperliquidOpenOrder>, HyperliquidHttpError> {
        let payload = json!({
            "type": "openOrders",
            "user": user
        });

        let response = self.request_info(payload).await?;
        let orders: Vec<HyperliquidOpenOrder> = serde_json::from_value(response)?;
        Ok(orders)
    }

    /// Fetches user fills (trade history)
    pub async fn request_user_fills(&self, user: &str) -> Result<Vec<HyperliquidUserFill>, HyperliquidHttpError> {
        let payload = json!({
            "type": "userFills",
            "user": user
        });

        let response = self.request_info(payload).await?;
        let fills: Vec<HyperliquidUserFill> = serde_json::from_value(response)?;
        Ok(fills)
    }

    // ======================== Trading Methods ========================

    /// Places an order
    pub async fn place_order(&self, order: HyperliquidOrderRequest) -> Result<HyperliquidOrderResponse, HyperliquidHttpError> {
        let action = json!({
            "type": "order",
            "orders": [order],
            "grouping": "na"
        });

        let response = self.request_exchange(action).await?;
        let order_response: HyperliquidOrderResponse = serde_json::from_value(response)?;
        Ok(order_response)
    }

    /// Cancels an order
    pub async fn cancel_order(&self, asset: u32, oid: u64) -> Result<Value, HyperliquidHttpError> {
        let action = json!({
            "type": "cancel",
            "cancels": [{
                "a": asset,
                "o": oid
            }]
        });

        self.request_exchange(action).await
    }

    /// Cancels all orders for an asset
    pub async fn cancel_all_orders(&self, asset: Option<u32>) -> Result<Value, HyperliquidHttpError> {
        let action = if let Some(asset) = asset {
            json!({
                "type": "cancelByCloid",
                "asset": asset
            })
        } else {
            json!({
                "type": "cancelByCloid"
            })
        };

        self.request_exchange(action).await
    }

    // ======================== Instrument Loading ========================

    /// Loads all instruments from the exchange
    pub async fn load_instruments(&self) -> Result<Vec<InstrumentAny>, HyperliquidHttpError> {
        debug!("Loading instruments from Hyperliquid");

        let meta_info = self.request_meta_info().await?;
        let mut instruments = Vec::new();
        let mut instruments_map = self.inner.instruments.write().await;

        for asset in &meta_info.universe {
            match parse_instrument(asset) {
                Ok(instrument) => {
                    let symbol = instrument.id().symbol.to_string();
                    instruments_map.insert(symbol, instrument.clone());
                    instruments.push(instrument);
                }
                Err(e) => {
                    error!("Failed to parse instrument {}: {}", asset.name, e);
                }
            }
        }

        debug!("Loaded {} instruments", instruments.len());
        Ok(instruments)
    }

    /// Returns cached instruments
    pub async fn instruments(&self) -> Vec<InstrumentAny> {
        let instruments_map = self.inner.instruments.read().await;
        instruments_map.values().cloned().collect()
    }

    /// Returns a specific instrument by symbol
    pub async fn instrument(&self, symbol: &str) -> Option<InstrumentAny> {
        let instruments_map = self.inner.instruments.read().await;
        instruments_map.get(symbol).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = Hyperliquid2HttpClient::new(None, None, false);
        assert!(client.is_ok());
    }

    #[test]
    fn test_client_with_credentials() {
        let private_key = "0x1234567890123456789012345678901234567890123456789012345678901234";
        let client = Hyperliquid2HttpClient::new(Some(private_key.to_string()), None, false);
        assert!(client.is_ok());
    }

    #[tokio::test]
    async fn test_instruments_cache() {
        let client = Hyperliquid2HttpClient::new(None, None, false).unwrap();
        let instruments = client.instruments().await;
        assert_eq!(instruments.len(), 0); // Initially empty
    }
}
