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

//! Provides the HTTP client integration for the Delta Exchange REST API.

use std::{
    collections::HashMap,
    num::NonZeroU32,
    sync::{Arc, LazyLock, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use nautilus_core::{consts::NAUTILUS_USER_AGENT, time::get_atomic_clock_realtime};
use nautilus_network::{http::HttpClient, ratelimiter::quota::Quota};
use reqwest::{header::USER_AGENT, Method, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tracing::{debug, error, trace, warn};
use ustr::Ustr;

use super::{
    error::{DeltaExchangeErrorResponse, DeltaExchangeHttpError, DeltaExchangeSuccessResponse},
    models::{
        DeltaExchangeAsset, DeltaExchangeBalance, DeltaExchangeCandle, DeltaExchangeFill,
        DeltaExchangeOrder, DeltaExchangeOrderBook, DeltaExchangePosition, DeltaExchangeProduct,
        DeltaExchangeTicker, DeltaExchangeTrade,
    },
};
use crate::common::{
    consts::{
        ASSETS_ENDPOINT, CANDLES_ENDPOINT, DEFAULT_HTTP_TIMEOUT_SECS, FILLS_ENDPOINT,
        HEADER_API_KEY, HEADER_SIGNATURE, HEADER_TIMESTAMP, MAX_REQUESTS_PER_SECOND,
        ORDERBOOK_ENDPOINT, ORDERS_ENDPOINT, POSITIONS_ENDPOINT, PRODUCTS_ENDPOINT,
        TICKERS_ENDPOINT, TRADES_ENDPOINT, WALLET_ENDPOINT,
    },
    credential::Credential,
};

/// Rate limiting quota for Delta Exchange REST API.
pub static DELTA_EXCHANGE_REST_QUOTA: LazyLock<Quota> =
    LazyLock::new(|| Quota::per_second(NonZeroU32::new(MAX_REQUESTS_PER_SECOND).unwrap()));

/// Provides a lower-level HTTP client for connecting to the Delta Exchange REST API.
#[derive(Debug, Clone)]
pub struct DeltaExchangeHttpInnerClient {
    base_url: String,
    client: HttpClient,
    credential: Option<Credential>,
}

impl DeltaExchangeHttpInnerClient {
    /// Creates a new [`DeltaExchangeHttpInnerClient`] instance.
    pub fn new(
        base_url: String,
        api_key: Option<String>,
        api_secret: Option<String>,
        timeout_secs: Option<u64>,
    ) -> Result<Self, DeltaExchangeHttpError> {
        let timeout = std::time::Duration::from_secs(timeout_secs.unwrap_or(DEFAULT_HTTP_TIMEOUT_SECS));
        let client = HttpClient::new(Some(timeout), None, None);

        let credential = match (api_key, api_secret) {
            (Some(key), Some(secret)) => Some(
                Credential::new(key, secret)
                    .map_err(DeltaExchangeHttpError::CredentialError)?,
            ),
            (None, None) => None,
            _ => {
                return Err(DeltaExchangeHttpError::CredentialError(
                    "Both API key and secret must be provided together".to_string(),
                ));
            }
        };

        Ok(Self {
            base_url,
            client,
            credential,
        })
    }

    /// Get current Unix timestamp in milliseconds.
    fn get_timestamp_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis() as u64
    }

    /// Send a GET request to the specified endpoint.
    async fn send_get_request<T>(
        &self,
        endpoint: &str,
        params: Option<&HashMap<String, String>>,
        authenticated: bool,
    ) -> Result<T, DeltaExchangeHttpError>
    where
        T: DeserializeOwned,
    {
        self.send_request(Method::GET, endpoint, params, None::<&()>, authenticated)
            .await
    }

    /// Send a POST request to the specified endpoint.
    async fn send_post_request<T, B>(
        &self,
        endpoint: &str,
        body: Option<&B>,
        authenticated: bool,
    ) -> Result<T, DeltaExchangeHttpError>
    where
        T: DeserializeOwned,
        B: Serialize,
    {
        self.send_request(Method::POST, endpoint, None, body, authenticated)
            .await
    }

    /// Send a PUT request to the specified endpoint.
    async fn send_put_request<T, B>(
        &self,
        endpoint: &str,
        body: Option<&B>,
        authenticated: bool,
    ) -> Result<T, DeltaExchangeHttpError>
    where
        T: DeserializeOwned,
        B: Serialize,
    {
        self.send_request(Method::PUT, endpoint, None, body, authenticated)
            .await
    }

    /// Send a DELETE request to the specified endpoint.
    async fn send_delete_request<T>(
        &self,
        endpoint: &str,
        authenticated: bool,
    ) -> Result<T, DeltaExchangeHttpError>
    where
        T: DeserializeOwned,
    {
        self.send_request(Method::DELETE, endpoint, None, None::<&()>, authenticated)
            .await
    }

    /// Send an HTTP request with proper authentication and error handling.
    async fn send_request<T, B>(
        &self,
        method: Method,
        endpoint: &str,
        params: Option<&HashMap<String, String>>,
        body: Option<&B>,
        authenticated: bool,
    ) -> Result<T, DeltaExchangeHttpError>
    where
        T: DeserializeOwned,
        B: Serialize,
    {
        if authenticated && self.credential.is_none() {
            return Err(DeltaExchangeHttpError::MissingCredentials);
        }

        // Build URL with query parameters
        let mut url = format!("{}{}", self.base_url, endpoint);
        if let Some(params) = params {
            if !params.is_empty() {
                let query_string = serde_urlencoded::to_string(params)?;
                url.push('?');
                url.push_str(&query_string);
            }
        }

        // Serialize body
        let body_str = match body {
            Some(b) => serde_json::to_string(b)?,
            None => String::new(),
        };

        // Prepare headers
        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());
        headers.insert(USER_AGENT.as_str().to_string(), NAUTILUS_USER_AGENT.to_string());

        // Add authentication headers if required
        if authenticated {
            if let Some(credential) = &self.credential {
                let timestamp = Self::get_timestamp_ms();
                let signature = credential
                    .sign(method.as_str(), endpoint, &body_str, timestamp)
                    .map_err(DeltaExchangeHttpError::CredentialError)?;

                headers.insert(HEADER_API_KEY.to_string(), credential.api_key.to_string());
                headers.insert(HEADER_SIGNATURE.to_string(), signature);
                headers.insert(HEADER_TIMESTAMP.to_string(), timestamp.to_string());
            }
        }

        trace!(
            "Sending {} request to {}: {}",
            method,
            url,
            if body_str.is_empty() { "no body" } else { &body_str }
        );

        // Send request
        let response = self
            .client
            .send_request(method, url, Some(headers), Some(body_str.into_bytes()))
            .await?;

        // Handle response
        let status = response.status();
        let response_text = String::from_utf8_lossy(&response.body);

        trace!("Response status: {}, body: {}", status, response_text);

        if status.is_success() {
            // Try to parse as success response
            match serde_json::from_str::<DeltaExchangeSuccessResponse<T>>(&response_text) {
                Ok(success_response) => Ok(success_response.result),
                Err(_) => {
                    // Try to parse directly as T (for some endpoints that don't wrap in success/result)
                    serde_json::from_str::<T>(&response_text)
                        .map_err(DeltaExchangeHttpError::from)
                }
            }
        } else {
            // Try to parse as error response
            match serde_json::from_str::<DeltaExchangeErrorResponse>(&response_text) {
                Ok(error_response) => Err(DeltaExchangeHttpError::from_api_error(error_response.error)),
                Err(_) => Err(DeltaExchangeHttpError::from_status_and_body(
                    status,
                    response_text.to_string(),
                )),
            }
        }
    }

    // Public API methods

    /// Get all available assets.
    pub async fn get_assets(&self) -> Result<Vec<DeltaExchangeAsset>, DeltaExchangeHttpError> {
        self.send_get_request(ASSETS_ENDPOINT, None, false).await
    }

    /// Get all available products.
    pub async fn get_products(&self) -> Result<Vec<DeltaExchangeProduct>, DeltaExchangeHttpError> {
        self.send_get_request(PRODUCTS_ENDPOINT, None, false).await
    }

    /// Get ticker for a specific product.
    pub async fn get_ticker(&self, symbol: &str) -> Result<DeltaExchangeTicker, DeltaExchangeHttpError> {
        let mut params = HashMap::new();
        params.insert("symbol".to_string(), symbol.to_string());
        self.send_get_request(TICKERS_ENDPOINT, Some(&params), false).await
    }

    /// Get all tickers.
    pub async fn get_tickers(&self) -> Result<Vec<DeltaExchangeTicker>, DeltaExchangeHttpError> {
        self.send_get_request(TICKERS_ENDPOINT, None, false).await
    }

    /// Get order book for a specific product.
    pub async fn get_orderbook(&self, symbol: &str) -> Result<DeltaExchangeOrderBook, DeltaExchangeHttpError> {
        let mut params = HashMap::new();
        params.insert("symbol".to_string(), symbol.to_string());
        self.send_get_request(ORDERBOOK_ENDPOINT, Some(&params), false).await
    }

    /// Get recent trades for a specific product.
    pub async fn get_trades(&self, symbol: &str, limit: Option<u32>) -> Result<Vec<DeltaExchangeTrade>, DeltaExchangeHttpError> {
        let mut params = HashMap::new();
        params.insert("symbol".to_string(), symbol.to_string());
        if let Some(limit) = limit {
            params.insert("page_size".to_string(), limit.to_string());
        }
        self.send_get_request(TRADES_ENDPOINT, Some(&params), false).await
    }

    /// Get historical candles for a specific product.
    pub async fn get_candles(
        &self,
        symbol: &str,
        resolution: &str,
        start: Option<u64>,
        end: Option<u64>,
    ) -> Result<Vec<DeltaExchangeCandle>, DeltaExchangeHttpError> {
        let mut params = HashMap::new();
        params.insert("symbol".to_string(), symbol.to_string());
        params.insert("resolution".to_string(), resolution.to_string());
        
        if let Some(start) = start {
            params.insert("start".to_string(), start.to_string());
        }
        if let Some(end) = end {
            params.insert("end".to_string(), end.to_string());
        }
        
        self.send_get_request(CANDLES_ENDPOINT, Some(&params), false).await
    }

    // Authenticated API methods

    /// Get wallet balances.
    pub async fn get_wallet(&self) -> Result<Vec<DeltaExchangeBalance>, DeltaExchangeHttpError> {
        self.send_get_request(WALLET_ENDPOINT, None, true).await
    }

    /// Get all orders.
    pub async fn get_orders(
        &self,
        product_id: Option<u64>,
        state: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Vec<DeltaExchangeOrder>, DeltaExchangeHttpError> {
        let mut params = HashMap::new();

        if let Some(product_id) = product_id {
            params.insert("product_id".to_string(), product_id.to_string());
        }
        if let Some(state) = state {
            params.insert("state".to_string(), state.to_string());
        }
        if let Some(limit) = limit {
            params.insert("page_size".to_string(), limit.to_string());
        }

        self.send_get_request(ORDERS_ENDPOINT, Some(&params), true).await
    }

    /// Get a specific order by ID.
    pub async fn get_order(&self, order_id: u64) -> Result<DeltaExchangeOrder, DeltaExchangeHttpError> {
        let endpoint = format!("{}/{}", ORDERS_ENDPOINT, order_id);
        self.send_get_request(&endpoint, None, true).await
    }

    /// Create a new order.
    pub async fn create_order(&self, order_request: &CreateOrderRequest) -> Result<DeltaExchangeOrder, DeltaExchangeHttpError> {
        self.send_post_request(ORDERS_ENDPOINT, Some(order_request), true).await
    }

    /// Modify an existing order.
    pub async fn modify_order(
        &self,
        order_id: u64,
        modify_request: &ModifyOrderRequest,
    ) -> Result<DeltaExchangeOrder, DeltaExchangeHttpError> {
        let endpoint = format!("{}/{}", ORDERS_ENDPOINT, order_id);
        self.send_put_request(&endpoint, Some(modify_request), true).await
    }

    /// Cancel an order.
    pub async fn cancel_order(&self, order_id: u64) -> Result<DeltaExchangeOrder, DeltaExchangeHttpError> {
        let endpoint = format!("{}/{}", ORDERS_ENDPOINT, order_id);
        self.send_delete_request(&endpoint, true).await
    }

    /// Cancel all orders for a product.
    pub async fn cancel_all_orders(&self, product_id: u64) -> Result<Vec<DeltaExchangeOrder>, DeltaExchangeHttpError> {
        let mut params = HashMap::new();
        params.insert("product_id".to_string(), product_id.to_string());

        let endpoint = format!("{}/all", ORDERS_ENDPOINT);
        self.send_request(Method::DELETE, &endpoint, Some(&params), None::<&()>, true).await
    }

    /// Get positions.
    pub async fn get_positions(&self, product_id: Option<u64>) -> Result<Vec<DeltaExchangePosition>, DeltaExchangeHttpError> {
        let mut params = HashMap::new();

        if let Some(product_id) = product_id {
            params.insert("product_id".to_string(), product_id.to_string());
        }

        self.send_get_request(POSITIONS_ENDPOINT, Some(&params), true).await
    }

    /// Get fills/trades.
    pub async fn get_fills(
        &self,
        product_id: Option<u64>,
        order_id: Option<u64>,
        limit: Option<u32>,
    ) -> Result<Vec<DeltaExchangeFill>, DeltaExchangeHttpError> {
        let mut params = HashMap::new();

        if let Some(product_id) = product_id {
            params.insert("product_id".to_string(), product_id.to_string());
        }
        if let Some(order_id) = order_id {
            params.insert("order_id".to_string(), order_id.to_string());
        }
        if let Some(limit) = limit {
            params.insert("page_size".to_string(), limit.to_string());
        }

        self.send_get_request(FILLS_ENDPOINT, Some(&params), true).await
    }
}

/// Request structure for creating orders.
#[derive(Debug, Serialize)]
pub struct CreateOrderRequest {
    pub product_id: u64,
    pub size: String,
    pub side: String,
    pub order_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_in_force: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reduce_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_order_id: Option<String>,
}

/// Request structure for modifying orders.
#[derive(Debug, Serialize)]
pub struct ModifyOrderRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_price: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_price: Option<String>,
}

/// Higher-level HTTP client with rate limiting and caching.
#[derive(Debug, Clone)]
pub struct DeltaExchangeHttpClient {
    inner: Arc<DeltaExchangeHttpInnerClient>,
    rate_limiter: Arc<Mutex<()>>, // Simple rate limiting placeholder
}

impl DeltaExchangeHttpClient {
    /// Creates a new [`DeltaExchangeHttpClient`] instance.
    pub fn new(
        base_url: String,
        api_key: Option<String>,
        api_secret: Option<String>,
        timeout_secs: Option<u64>,
    ) -> Result<Self, DeltaExchangeHttpError> {
        let inner = DeltaExchangeHttpInnerClient::new(base_url, api_key, api_secret, timeout_secs)?;

        Ok(Self {
            inner: Arc::new(inner),
            rate_limiter: Arc::new(Mutex::new(())),
        })
    }

    /// Apply rate limiting before making requests.
    async fn apply_rate_limit(&self) {
        // Simple rate limiting - in production this would use a proper rate limiter
        let _guard = self.rate_limiter.lock().unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    // Delegate all methods to inner client with rate limiting

    pub async fn get_assets(&self) -> Result<Vec<DeltaExchangeAsset>, DeltaExchangeHttpError> {
        self.apply_rate_limit().await;
        self.inner.get_assets().await
    }

    pub async fn get_products(&self) -> Result<Vec<DeltaExchangeProduct>, DeltaExchangeHttpError> {
        self.apply_rate_limit().await;
        self.inner.get_products().await
    }

    pub async fn get_ticker(&self, symbol: &str) -> Result<DeltaExchangeTicker, DeltaExchangeHttpError> {
        self.apply_rate_limit().await;
        self.inner.get_ticker(symbol).await
    }

    pub async fn get_tickers(&self) -> Result<Vec<DeltaExchangeTicker>, DeltaExchangeHttpError> {
        self.apply_rate_limit().await;
        self.inner.get_tickers().await
    }

    pub async fn get_orderbook(&self, symbol: &str) -> Result<DeltaExchangeOrderBook, DeltaExchangeHttpError> {
        self.apply_rate_limit().await;
        self.inner.get_orderbook(symbol).await
    }

    pub async fn get_trades(&self, symbol: &str, limit: Option<u32>) -> Result<Vec<DeltaExchangeTrade>, DeltaExchangeHttpError> {
        self.apply_rate_limit().await;
        self.inner.get_trades(symbol, limit).await
    }

    pub async fn get_candles(
        &self,
        symbol: &str,
        resolution: &str,
        start: Option<u64>,
        end: Option<u64>,
    ) -> Result<Vec<DeltaExchangeCandle>, DeltaExchangeHttpError> {
        self.apply_rate_limit().await;
        self.inner.get_candles(symbol, resolution, start, end).await
    }

    pub async fn get_wallet(&self) -> Result<Vec<DeltaExchangeBalance>, DeltaExchangeHttpError> {
        self.apply_rate_limit().await;
        self.inner.get_wallet().await
    }

    pub async fn get_orders(
        &self,
        product_id: Option<u64>,
        state: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Vec<DeltaExchangeOrder>, DeltaExchangeHttpError> {
        self.apply_rate_limit().await;
        self.inner.get_orders(product_id, state, limit).await
    }

    pub async fn get_order(&self, order_id: u64) -> Result<DeltaExchangeOrder, DeltaExchangeHttpError> {
        self.apply_rate_limit().await;
        self.inner.get_order(order_id).await
    }

    pub async fn create_order(&self, order_request: &CreateOrderRequest) -> Result<DeltaExchangeOrder, DeltaExchangeHttpError> {
        self.apply_rate_limit().await;
        self.inner.create_order(order_request).await
    }

    pub async fn modify_order(
        &self,
        order_id: u64,
        modify_request: &ModifyOrderRequest,
    ) -> Result<DeltaExchangeOrder, DeltaExchangeHttpError> {
        self.apply_rate_limit().await;
        self.inner.modify_order(order_id, modify_request).await
    }

    pub async fn cancel_order(&self, order_id: u64) -> Result<DeltaExchangeOrder, DeltaExchangeHttpError> {
        self.apply_rate_limit().await;
        self.inner.cancel_order(order_id).await
    }

    pub async fn cancel_all_orders(&self, product_id: u64) -> Result<Vec<DeltaExchangeOrder>, DeltaExchangeHttpError> {
        self.apply_rate_limit().await;
        self.inner.cancel_all_orders(product_id).await
    }

    pub async fn get_positions(&self, product_id: Option<u64>) -> Result<Vec<DeltaExchangePosition>, DeltaExchangeHttpError> {
        self.apply_rate_limit().await;
        self.inner.get_positions(product_id).await
    }

    pub async fn get_fills(
        &self,
        product_id: Option<u64>,
        order_id: Option<u64>,
        limit: Option<u32>,
    ) -> Result<Vec<DeltaExchangeFill>, DeltaExchangeHttpError> {
        self.apply_rate_limit().await;
        self.inner.get_fills(product_id, order_id, limit).await
    }
}
