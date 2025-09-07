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

//! Provides the HTTP client integration for the [BitMEX](https://bitmex.com) REST API.
//!
//! This module defines and implements a [`BitmexHttpClient`] for
//! sending requests to various BitMEX endpoints. It handles request signing
//! (when credentials are provided), constructs valid HTTP requests
//! using the [`HttpClient`], and parses the responses back into structured data or a [`BitmexHttpError`].
//!
//! BitMEX API reference <https://www.bitmex.com/api/explorer/#/default>.

use std::{
    collections::HashMap,
    num::NonZeroU32,
    sync::{Arc, LazyLock, Mutex},
};

use ahash::AHashMap;
use chrono::Utc;
use nautilus_core::{
    UnixNanos,
    consts::{NAUTILUS_TRADER, NAUTILUS_USER_AGENT},
    env::get_env_var,
    time::get_atomic_clock_realtime,
};
use nautilus_model::{
    data::TradeTick,
    enums::{OrderSide, OrderType, TimeInForce},
    events::AccountState,
    identifiers::{AccountId, ClientOrderId, InstrumentId, VenueOrderId},
    instruments::{Instrument as InstrumentTrait, InstrumentAny},
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{Price, Quantity},
};
use nautilus_network::{
    http::HttpClient,
    ratelimiter::quota::Quota,
    retry::{RetryConfig, RetryManager},
};
use reqwest::{Method, StatusCode, header::USER_AGENT};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{
    error::{BitmexErrorResponse, BitmexHttpError},
    models::{
        BitmexExecution, BitmexInstrument, BitmexMargin, BitmexOrder, BitmexPosition, BitmexTrade,
        BitmexWallet,
    },
    query::{
        DeleteAllOrdersParams, DeleteOrderParams, GetExecutionParams, GetExecutionParamsBuilder,
        GetOrderParams, GetPositionParams, GetPositionParamsBuilder, GetTradeParams,
        GetTradeParamsBuilder, PostOrderBulkParams, PostOrderParams, PostPositionLeverageParams,
        PutOrderBulkParams, PutOrderParams,
    },
};
use crate::{
    common::{
        consts::{BITMEX_HTTP_TESTNET_URL, BITMEX_HTTP_URL},
        credential::Credential,
        enums::{BitmexOrderStatus, BitmexSide},
        parse::{parse_account_state, parse_instrument_id, quantity_to_u32},
    },
    http::{
        parse::{
            parse_fill_report, parse_instrument_any, parse_order_status_report,
            parse_position_report, parse_trade,
        },
        query::{DeleteAllOrdersParamsBuilder, GetOrderParamsBuilder, PutOrderParamsBuilder},
    },
    websocket::messages::BitmexMarginMsg,
};

/// Default BitMEX REST API rate limit.
///
/// BitMEX implements a dual-layer rate limiting system:
/// - Primary limit: 120 requests per minute for authenticated users (30 for unauthenticated).
/// - Secondary limit: 10 requests per second burst limit for specific endpoints.
///
/// We use 10 requests per second which respects the burst limit while the token bucket
/// mechanism naturally handles the average rate limit.
pub static BITMEX_REST_QUOTA: LazyLock<Quota> =
    LazyLock::new(|| Quota::per_second(NonZeroU32::new(10).expect("10 is a valid non-zero u32")));

/// Represents a BitMEX HTTP response.
#[derive(Debug, Serialize, Deserialize)]
pub struct BitmexResponse<T> {
    /// The typed data returned by the BitMEX endpoint.
    pub data: Vec<T>,
}

/// Provides a lower-level HTTP client for connecting to the [BitMEX](https://bitmex.com) REST API.
///
/// This client wraps the underlying [`HttpClient`] to handle functionality
/// specific to BitMEX, such as request signing (for authenticated endpoints),
/// forming request URLs, and deserializing responses into specific data models.
///
/// # Connection Management
///
/// The client uses HTTP keep-alive for connection pooling with a 90-second idle timeout,
/// which matches BitMEX's server-side keep-alive timeout. Connections are automatically
/// reused for subsequent requests to minimize latency.
///
/// # Rate Limiting
///
/// BitMEX enforces the following rate limits:
/// - 120 requests per minute for authenticated users (30 for unauthenticated).
/// - 10 requests per second burst limit for certain endpoints (order management).
///
/// The client automatically respects these limits through the configured quota.
#[derive(Debug, Clone)]
pub struct BitmexHttpInnerClient {
    base_url: String,
    client: HttpClient,
    credential: Option<Credential>,
    retry_manager: RetryManager<BitmexHttpError>,
    cancellation_token: CancellationToken,
}

impl Default for BitmexHttpInnerClient {
    fn default() -> Self {
        Self::new(None, Some(60), None, None, None)
            .expect("Failed to create default BitmexHttpInnerClient")
    }
}

impl BitmexHttpInnerClient {
    /// Cancel all pending HTTP requests.
    pub fn cancel_all_requests(&self) {
        self.cancellation_token.cancel();
    }

    /// Get the cancellation token for this client.
    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.cancellation_token
    }
    /// Creates a new [`BitmexHttpInnerClient`] using the default BitMEX HTTP URL,
    /// optionally overridden with a custom base URL.
    ///
    /// This version of the client has **no credentials**, so it can only
    /// call publicly accessible endpoints.
    ///
    /// # Errors
    ///
    /// Returns an error if the retry manager cannot be created.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        base_url: Option<String>,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
    ) -> Result<Self, BitmexHttpError> {
        let retry_config = RetryConfig {
            max_retries: max_retries.unwrap_or(3),
            initial_delay_ms: retry_delay_ms.unwrap_or(1000),
            max_delay_ms: retry_delay_max_ms.unwrap_or(10_000),
            backoff_factor: 2.0,
            jitter_ms: 1000,
            operation_timeout_ms: Some(60_000),
            immediate_first: false,
            max_elapsed_ms: Some(180_000),
        };

        let retry_manager = RetryManager::new(retry_config).map_err(|e| {
            BitmexHttpError::NetworkError(format!("Failed to create retry manager: {e}"))
        })?;

        Ok(Self {
            base_url: base_url.unwrap_or(BITMEX_HTTP_URL.to_string()),
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                vec![],
                Some(*BITMEX_REST_QUOTA),
                timeout_secs,
            ),
            credential: None,
            retry_manager,
            cancellation_token: CancellationToken::new(),
        })
    }

    /// Creates a new [`BitmexHttpInnerClient`] configured with credentials
    /// for authenticated requests, optionally using a custom base URL.
    ///
    /// # Errors
    ///
    /// Returns an error if the retry manager cannot be created.
    #[allow(clippy::too_many_arguments)]
    pub fn with_credentials(
        api_key: String,
        api_secret: String,
        base_url: String,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
    ) -> Result<Self, BitmexHttpError> {
        let retry_config = RetryConfig {
            max_retries: max_retries.unwrap_or(3),
            initial_delay_ms: retry_delay_ms.unwrap_or(1000),
            max_delay_ms: retry_delay_max_ms.unwrap_or(10_000),
            backoff_factor: 2.0,
            jitter_ms: 1000,
            operation_timeout_ms: Some(60_000),
            immediate_first: false,
            max_elapsed_ms: Some(180_000),
        };

        let retry_manager = RetryManager::new(retry_config).map_err(|e| {
            BitmexHttpError::NetworkError(format!("Failed to create retry manager: {e}"))
        })?;

        Ok(Self {
            base_url,
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                vec![],
                Some(*BITMEX_REST_QUOTA),
                timeout_secs,
            ),
            credential: Some(Credential::new(api_key, api_secret)),
            retry_manager,
            cancellation_token: CancellationToken::new(),
        })
    }

    fn default_headers() -> HashMap<String, String> {
        HashMap::from([(USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string())])
    }

    fn sign_request(
        &self,
        method: &Method,
        endpoint: &str,
        body: Option<&[u8]>,
    ) -> Result<HashMap<String, String>, BitmexHttpError> {
        let credential = self
            .credential
            .as_ref()
            .ok_or(BitmexHttpError::MissingCredentials)?;

        let expires = Utc::now().timestamp() + 10;
        let body_str = body
            .and_then(|b| String::from_utf8(b.to_vec()).ok())
            .unwrap_or_default();

        let full_path = if endpoint.starts_with("/api/v1") {
            endpoint.to_string()
        } else {
            format!("/api/v1{endpoint}")
        };

        let signature = credential.sign(method.as_str(), &full_path, expires, &body_str);

        let mut headers = HashMap::new();
        headers.insert("api-expires".to_string(), expires.to_string());
        headers.insert("api-key".to_string(), credential.api_key.to_string());
        headers.insert("api-signature".to_string(), signature);

        Ok(headers)
    }

    async fn send_request<T: DeserializeOwned>(
        &self,
        method: Method,
        endpoint: &str,
        body: Option<Vec<u8>>,
        authenticate: bool,
    ) -> Result<T, BitmexHttpError> {
        let url = format!("{}{endpoint}", self.base_url);
        let method_clone = method.clone();
        let body_clone = body.clone();

        let operation = || {
            let url = url.clone();
            let method = method_clone.clone();
            let body = body_clone.clone();
            let endpoint = endpoint.to_string();

            async move {
                let headers = if authenticate {
                    Some(self.sign_request(&method, &endpoint, body.as_deref())?)
                } else {
                    None
                };

                let resp = self
                    .client
                    .request(method, url, headers, body, None, None)
                    .await?;

                if resp.status.is_success() {
                    serde_json::from_slice(&resp.body).map_err(Into::into)
                } else if let Ok(error_resp) =
                    serde_json::from_slice::<BitmexErrorResponse>(&resp.body)
                {
                    Err(error_resp.into())
                } else {
                    Err(BitmexHttpError::UnexpectedStatus {
                        status: StatusCode::from_u16(resp.status.as_u16())
                            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                        body: String::from_utf8_lossy(&resp.body).to_string(),
                    })
                }
            }
        };

        // Retry strategy based on BitMEX error responses and HTTP status codes:
        //
        // 1. Network errors: always retry (transient connection issues)
        // 2. HTTP 5xx/429: server errors and rate limiting should be retried
        // 3. BitMEX JSON errors with specific handling:
        //    - "RateLimitError": explicit rate limit error from BitMEX
        //    - "HTTPError": generic error name used by BitMEX for various issues
        //      Only retry if message contains "rate limit" to avoid retrying
        //      non-transient errors like authentication failures, validation errors,
        //      insufficient balance, etc. which also return as "HTTPError"
        //
        // Note: BitMEX returns many permanent errors as "HTTPError" (e.g., "Invalid orderQty",
        // "Account has insufficient Available Balance", "Invalid API Key") which should NOT
        // be retried. We only retry when the message explicitly mentions rate limiting.
        //
        // See tests in tests/http.rs for retry behavior validation
        let should_retry = |error: &BitmexHttpError| -> bool {
            match error {
                BitmexHttpError::NetworkError(_) => true,
                BitmexHttpError::UnexpectedStatus { status, .. } => {
                    status.as_u16() >= 500 || status.as_u16() == 429
                }
                BitmexHttpError::BitmexError {
                    error_name,
                    message,
                } => {
                    error_name == "RateLimitError"
                        || (error_name == "HTTPError"
                            && message.to_lowercase().contains("rate limit"))
                }
                _ => false,
            }
        };

        let create_error = |msg: String| -> BitmexHttpError {
            if msg == "canceled" {
                BitmexHttpError::NetworkError("Request canceled".to_string())
            } else {
                BitmexHttpError::NetworkError(msg)
            }
        };

        self.retry_manager
            .execute_with_retry_with_cancel(
                endpoint,
                operation,
                should_retry,
                create_error,
                &self.cancellation_token,
            )
            .await
    }

    /// Get all instruments.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails, the response cannot be parsed, or the API returns an error.
    pub async fn http_get_instruments(
        &self,
        active_only: bool,
    ) -> Result<Vec<BitmexInstrument>, BitmexHttpError> {
        let path = if active_only {
            "/instrument/active"
        } else {
            "/instrument"
        };
        self.send_request(Method::GET, path, None, false).await
    }

    /// Get instrument by symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails, the response cannot be parsed, or the API returns an error.
    pub async fn http_get_instrument(
        &self,
        symbol: &str,
    ) -> Result<BitmexInstrument, BitmexHttpError> {
        let path = &format!("/instrument?symbol={symbol}");
        self.send_request(Method::GET, path, None, false).await
    }

    /// Get user wallet information.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or the API returns an error.
    pub async fn http_get_wallet(&self) -> Result<BitmexWallet, BitmexHttpError> {
        let endpoint = "/user/wallet";
        self.send_request(Method::GET, endpoint, None, true).await
    }

    /// Get user margin information.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or the API returns an error.
    pub async fn http_get_margin(&self, currency: &str) -> Result<BitmexMargin, BitmexHttpError> {
        let path = format!("/user/margin?currency={currency}");
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Get historical trades.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the parameters cannot be serialized (should never happen with valid builder-generated params).
    pub async fn http_get_trades(
        &self,
        params: GetTradeParams,
    ) -> Result<Vec<BitmexTrade>, BitmexHttpError> {
        let query = serde_urlencoded::to_string(&params).map_err(|e| {
            BitmexHttpError::ValidationError(format!("Failed to serialize parameters: {e}"))
        })?;
        let path = format!("/trade?{query}");
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Get user orders.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the parameters cannot be serialized (should never happen with valid builder-generated params).
    pub async fn http_get_orders(
        &self,
        params: GetOrderParams,
    ) -> Result<Vec<BitmexOrder>, BitmexHttpError> {
        let query = serde_urlencoded::to_string(&params).map_err(|e| {
            BitmexHttpError::ValidationError(format!("Failed to serialize parameters: {e}"))
        })?;
        let path = format!("/order?{query}");
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Place a new order.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, order validation fails, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the parameters cannot be serialized (should never happen with valid builder-generated params).
    pub async fn http_place_order(
        &self,
        params: PostOrderParams,
    ) -> Result<Value, BitmexHttpError> {
        let query = serde_urlencoded::to_string(&params).map_err(|e| {
            BitmexHttpError::ValidationError(format!("Failed to serialize parameters: {e}"))
        })?;
        let path = format!("/order?{query}");
        self.send_request(Method::POST, &path, None, true).await
    }

    /// Cancel user orders.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, the order doesn't exist, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the parameters cannot be serialized (should never happen with valid builder-generated params).
    pub async fn http_cancel_orders(
        &self,
        params: DeleteOrderParams,
    ) -> Result<Value, BitmexHttpError> {
        let query = serde_urlencoded::to_string(&params).map_err(|e| {
            BitmexHttpError::ValidationError(format!("Failed to serialize parameters: {e}"))
        })?;
        let path = format!("/order?{query}");
        self.send_request(Method::DELETE, &path, None, true).await
    }

    /// Amend an existing order.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, the order doesn't exist, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the parameters cannot be serialized (should never happen with valid builder-generated params).
    pub async fn http_amend_order(&self, params: PutOrderParams) -> Result<Value, BitmexHttpError> {
        let query = serde_urlencoded::to_string(&params).map_err(|e| {
            BitmexHttpError::ValidationError(format!("Failed to serialize parameters: {e}"))
        })?;
        let path = format!("/order?{query}");
        self.send_request(Method::PUT, &path, None, true).await
    }

    /// Cancel all orders.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the parameters cannot be serialized (should never happen with valid builder-generated params).
    ///
    /// # References
    ///
    /// <https://www.bitmex.com/api/explorer/#!/Order/Order_cancelAll>
    pub async fn http_cancel_all_orders(
        &self,
        params: DeleteAllOrdersParams,
    ) -> Result<Value, BitmexHttpError> {
        let query = serde_urlencoded::to_string(&params).map_err(|e| {
            BitmexHttpError::ValidationError(format!("Failed to serialize parameters: {e}"))
        })?;
        let path = format!("/order/all?{query}");
        self.send_request(Method::DELETE, &path, None, true).await
    }

    /// Get user executions.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the parameters cannot be serialized (should never happen with valid builder-generated params).
    pub async fn http_get_executions(
        &self,
        params: GetExecutionParams,
    ) -> Result<Vec<BitmexExecution>, BitmexHttpError> {
        let query = serde_urlencoded::to_string(&params).map_err(|e| {
            BitmexHttpError::ValidationError(format!("Failed to serialize parameters: {e}"))
        })?;
        let path = format!("/execution/tradeHistory?{query}");
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Get user positions.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the parameters cannot be serialized (should never happen with valid builder-generated params).
    pub async fn http_get_positions(
        &self,
        params: GetPositionParams,
    ) -> Result<Vec<BitmexPosition>, BitmexHttpError> {
        let query = serde_urlencoded::to_string(&params).map_err(|e| {
            BitmexHttpError::ValidationError(format!("Failed to serialize parameters: {e}"))
        })?;
        let path = format!("/position?{query}");
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Place multiple orders in bulk.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, order validation fails, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the parameters cannot be serialized (should never happen with valid builder-generated params).
    pub async fn http_place_orders_bulk(
        &self,
        params: PostOrderBulkParams,
    ) -> Result<Vec<BitmexOrder>, BitmexHttpError> {
        let body = serde_json::to_vec(&params).map_err(|e| {
            BitmexHttpError::ValidationError(format!("Failed to serialize parameters: {e}"))
        })?;
        let path = "/order/bulk";
        self.send_request(Method::POST, path, Some(body), true)
            .await
    }

    /// Amend multiple orders in bulk.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, the orders don't exist, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the parameters cannot be serialized (should never happen with valid builder-generated params).
    pub async fn http_amend_orders_bulk(
        &self,
        params: PutOrderBulkParams,
    ) -> Result<Vec<BitmexOrder>, BitmexHttpError> {
        let body = serde_json::to_vec(&params).map_err(|e| {
            BitmexHttpError::ValidationError(format!("Failed to serialize parameters: {e}"))
        })?;
        let path = "/order/bulk";
        self.send_request(Method::PUT, path, Some(body), true).await
    }

    /// Update position leverage.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the parameters cannot be serialized (should never happen with valid builder-generated params).
    pub async fn http_update_position_leverage(
        &self,
        params: PostPositionLeverageParams,
    ) -> Result<BitmexPosition, BitmexHttpError> {
        let query = serde_urlencoded::to_string(&params).map_err(|e| {
            BitmexHttpError::ValidationError(format!("Failed to serialize parameters: {e}"))
        })?;
        let path = format!("/position/leverage?{query}");
        self.send_request(Method::POST, &path, None, true).await
    }
}

/// Provides a HTTP client for connecting to the [BitMEX](https://bitmex.com) REST API.
///
/// This is the high-level client that wraps the inner client and provides
/// Nautilus-specific functionality for trading operations.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct BitmexHttpClient {
    inner: Arc<BitmexHttpInnerClient>,
    instruments_cache: Arc<Mutex<AHashMap<Ustr, InstrumentAny>>>,
}

impl Default for BitmexHttpClient {
    fn default() -> Self {
        Self::new(None, None, None, false, Some(60), None, None, None)
            .expect("Failed to create default BitmexHttpClient")
    }
}

impl BitmexHttpClient {
    /// Creates a new [`BitmexHttpClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        base_url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        testnet: bool,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
    ) -> Result<Self, BitmexHttpError> {
        // Determine the base URL
        let url = base_url.unwrap_or_else(|| {
            if testnet {
                BITMEX_HTTP_TESTNET_URL.to_string()
            } else {
                BITMEX_HTTP_URL.to_string()
            }
        });

        let inner = match (api_key, api_secret) {
            (Some(key), Some(secret)) => BitmexHttpInnerClient::with_credentials(
                key,
                secret,
                url,
                timeout_secs,
                max_retries,
                retry_delay_ms,
                retry_delay_max_ms,
            )?,
            _ => BitmexHttpInnerClient::new(
                Some(url),
                timeout_secs,
                max_retries,
                retry_delay_ms,
                retry_delay_max_ms,
            )?,
        };

        Ok(Self {
            inner: Arc::new(inner),
            instruments_cache: Arc::new(Mutex::new(AHashMap::new())),
        })
    }

    /// Creates a new [`BitmexHttpClient`] instance using environment variables and
    /// the default BitMEX HTTP base URL.
    ///
    /// # Errors
    ///
    /// Returns an error if required environment variables are not set or invalid.
    pub fn from_env() -> anyhow::Result<Self> {
        Self::with_credentials(None, None, None, None, None, None, None)
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {e}"))
    }

    /// Creates a new [`BitmexHttpClient`] configured with credentials
    /// for authenticated requests.
    ///
    /// If `api_key` or `api_secret` are `None`, they will be sourced from the
    /// `BITMEX_API_KEY` and `BITMEX_API_SECRET` environment variables.
    ///
    /// # Errors
    ///
    /// Returns an error if one credential is provided without the other.
    #[allow(clippy::too_many_arguments)]
    pub fn with_credentials(
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url: Option<String>,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
    ) -> anyhow::Result<Self> {
        let api_key = api_key.or_else(|| get_env_var("BITMEX_API_KEY").ok());
        let api_secret = api_secret.or_else(|| get_env_var("BITMEX_API_SECRET").ok());

        // Determine testnet from URL if provided
        let testnet = base_url.as_ref().is_some_and(|url| url.contains("testnet"));

        // If we're trying to create an authenticated client, we need both key and secret
        if api_key.is_some() && api_secret.is_none() {
            anyhow::bail!("BITMEX_API_SECRET is required when BITMEX_API_KEY is provided");
        }
        if api_key.is_none() && api_secret.is_some() {
            anyhow::bail!("BITMEX_API_KEY is required when BITMEX_API_SECRET is provided");
        }

        Self::new(
            base_url,
            api_key,
            api_secret,
            testnet,
            timeout_secs,
            max_retries,
            retry_delay_ms,
            retry_delay_max_ms,
        )
        .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {e}"))
    }

    /// Returns the base url being used by the client.
    #[must_use]
    pub fn base_url(&self) -> &str {
        self.inner.base_url.as_str()
    }

    /// Returns the public API key being used by the client.
    #[must_use]
    pub fn api_key(&self) -> Option<&str> {
        self.inner.credential.as_ref().map(|c| c.api_key.as_str())
    }

    /// Generates a timestamp for initialization.
    fn generate_ts_init(&self) -> UnixNanos {
        get_atomic_clock_realtime().get_time_ns()
    }

    /// Cancel all pending HTTP requests.
    pub fn cancel_all_requests(&self) {
        self.inner.cancel_all_requests();
    }

    /// Get the cancellation token for this client.
    pub fn cancellation_token(&self) -> CancellationToken {
        self.inner.cancellation_token().clone()
    }

    /// Adds an instrument to the cache for precision lookups.
    ///
    /// # Panics
    ///
    /// Panics if the instruments cache mutex is poisoned.
    pub fn add_instrument(&mut self, instrument: InstrumentAny) {
        self.instruments_cache
            .lock()
            .unwrap()
            .insert(instrument.raw_symbol().inner(), instrument);
    }

    /// Request all available instruments and parse them into Nautilus types.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or parsing fails.
    pub async fn request_instrument(
        &self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<Option<InstrumentAny>> {
        let response = self
            .inner
            .http_get_instrument(instrument_id.symbol.as_str())
            .await?;

        let ts_init = self.generate_ts_init();

        Ok(parse_instrument_any(&response, ts_init))
    }

    /// Request all available instruments and parse them into Nautilus types.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or parsing fails.
    pub async fn request_instruments(
        &self,
        active_only: bool,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let instruments = self.inner.http_get_instruments(active_only).await?;
        let ts_init = self.generate_ts_init();

        let mut parsed_instruments = Vec::new();
        for inst in instruments {
            if let Some(instrument_any) = parse_instrument_any(&inst, ts_init) {
                parsed_instruments.push(instrument_any);
            }
        }

        Ok(parsed_instruments)
    }

    /// Get user wallet information.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the inner mutex is poisoned.
    pub async fn get_wallet(&self) -> Result<BitmexWallet, BitmexHttpError> {
        let inner = self.inner.clone();
        inner.http_get_wallet().await
    }

    /// Get user orders.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the inner mutex is poisoned.
    pub async fn get_orders(
        &self,
        params: GetOrderParams,
    ) -> Result<Vec<BitmexOrder>, BitmexHttpError> {
        let inner = self.inner.clone();
        inner.http_get_orders(params).await
    }

    /// Place a new order with raw API params.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, order validation fails, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the inner mutex is poisoned.
    pub async fn http_place_order(
        &self,
        params: PostOrderParams,
    ) -> Result<Value, BitmexHttpError> {
        let inner = self.inner.clone();
        inner.http_place_order(params).await
    }

    /// Cancel user orders with raw API params.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, the order doesn't exist, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the inner mutex is poisoned.
    pub async fn http_cancel_orders(
        &self,
        params: DeleteOrderParams,
    ) -> Result<Value, BitmexHttpError> {
        let inner = self.inner.clone();
        inner.http_cancel_orders(params).await
    }

    /// Amend an existing order with raw API params.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, the order doesn't exist, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the inner mutex is poisoned.
    pub async fn http_amend_order(&self, params: PutOrderParams) -> Result<Value, BitmexHttpError> {
        let inner = self.inner.clone();
        inner.http_amend_order(params).await
    }

    /// Cancel all orders with raw API params.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the inner mutex is poisoned.
    ///
    /// # References
    ///
    /// <https://www.bitmex.com/api/explorer/#!/Order/Order_cancelAll>
    pub async fn http_cancel_all_orders(
        &self,
        params: DeleteAllOrdersParams,
    ) -> Result<Value, BitmexHttpError> {
        let inner = self.inner.clone();
        inner.http_cancel_all_orders(params).await
    }

    /// Get price precision for a symbol from the instruments cache (if found).
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument is not found in the cache.
    ///
    /// # Panics
    ///
    /// Panics if the instruments cache mutex is poisoned.
    pub fn get_price_precision(&self, symbol: Ustr) -> anyhow::Result<u8> {
        let cache = self.instruments_cache.lock().unwrap();
        cache
            .get(&symbol)
            .map(|inst| inst.price_precision())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Instrument {symbol} not found in cache, ensure instruments loaded first"
                )
            })
    }

    /// Get user margin information.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or the API returns an error.
    pub async fn http_get_margin(&self, currency: &str) -> anyhow::Result<BitmexMargin> {
        self.inner
            .http_get_margin(currency)
            .await
            .map_err(|e| anyhow::anyhow!(e))
    }

    /// Request account state for the given account.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or no account state is returned.
    pub async fn request_account_state(
        &self,
        account_id: AccountId,
    ) -> anyhow::Result<AccountState> {
        // Get margin data for XBt (Bitcoin) by default
        let margin = self
            .inner
            .http_get_margin("XBt")
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let ts_init = nautilus_core::nanos::UnixNanos::from(
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default() as u64,
        );

        // Convert HTTP Margin to WebSocket MarginMsg for parsing
        let margin_msg = BitmexMarginMsg {
            account: margin.account,
            currency: margin.currency,
            risk_limit: margin.risk_limit,
            amount: margin.amount,
            prev_realised_pnl: margin.prev_realised_pnl,
            gross_comm: margin.gross_comm,
            gross_open_cost: margin.gross_open_cost,
            gross_open_premium: margin.gross_open_premium,
            gross_exec_cost: margin.gross_exec_cost,
            gross_mark_value: margin.gross_mark_value,
            risk_value: margin.risk_value,
            init_margin: margin.init_margin,
            maint_margin: margin.maint_margin,
            target_excess_margin: margin.target_excess_margin,
            realised_pnl: margin.realised_pnl,
            unrealised_pnl: margin.unrealised_pnl,
            wallet_balance: margin.wallet_balance,
            margin_balance: margin.margin_balance,
            margin_leverage: margin.margin_leverage,
            margin_used_pcnt: margin.margin_used_pcnt,
            excess_margin: margin.excess_margin,
            available_margin: margin.available_margin,
            withdrawable_margin: margin.withdrawable_margin,
            maker_fee_discount: None, // Not in HTTP response
            taker_fee_discount: None, // Not in HTTP response
            timestamp: margin.timestamp.unwrap_or_else(chrono::Utc::now),
            foreign_margin_balance: None,
            foreign_requirement: None,
        };

        parse_account_state(&margin_msg, account_id, ts_init)
    }

    /// Submit a new order.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, order validation fails,
    /// the order is rejected, or the API returns an error.
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: TimeInForce,
        price: Option<Price>,
        trigger_price: Option<Price>,
        display_qty: Option<Quantity>,
        post_only: bool,
        reduce_only: bool,
    ) -> anyhow::Result<OrderStatusReport> {
        use crate::common::enums::{
            BitmexExecInstruction, BitmexOrderType, BitmexSide, BitmexTimeInForce,
        };

        let mut params = super::query::PostOrderParamsBuilder::default();
        params.text(NAUTILUS_TRADER);
        params.symbol(instrument_id.symbol.as_str());
        params.cl_ord_id(client_order_id.as_str());

        let side = BitmexSide::try_from_order_side(order_side)?;
        params.side(side);

        let ord_type = BitmexOrderType::try_from_order_type(order_type)?;
        params.ord_type(ord_type);

        params.order_qty(quantity_to_u32(&quantity));

        let tif = BitmexTimeInForce::try_from_time_in_force(time_in_force)?;
        params.time_in_force(tif);

        if let Some(price) = price {
            params.price(price.as_f64());
        }

        if let Some(trigger_price) = trigger_price {
            params.stop_px(trigger_price.as_f64());
        }

        if let Some(display_qty) = display_qty {
            params.display_qty(quantity_to_u32(&display_qty));
        }

        let mut exec_inst = Vec::new();

        if post_only {
            exec_inst.push(BitmexExecInstruction::ParticipateDoNotInitiate);
        }

        if reduce_only {
            exec_inst.push(BitmexExecInstruction::ReduceOnly);
        }

        if !exec_inst.is_empty() {
            params.exec_inst(exec_inst);
        }

        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let response = self.inner.http_place_order(params).await?;

        let order: BitmexOrder = serde_json::from_value(response)?;

        if let Some(BitmexOrderStatus::Rejected) = order.ord_status {
            let reason = order
                .ord_rej_reason
                .map(|r| r.to_string())
                .unwrap_or_else(|| "No reason provided".to_string());
            return Err(anyhow::anyhow!("Order rejected: {reason}"));
        }

        let price_precision = self.get_price_precision(instrument_id.symbol.inner())?;
        let ts_init = self.generate_ts_init();

        parse_order_status_report(&order, instrument_id, price_precision, ts_init)
    }

    /// Cancel an order.
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - The order doesn't exist.
    /// - The API returns an error.
    pub async fn cancel_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> anyhow::Result<OrderStatusReport> {
        let mut params = super::query::DeleteOrderParamsBuilder::default();
        params.text(NAUTILUS_TRADER);

        if let Some(venue_order_id) = venue_order_id {
            params.order_id(vec![venue_order_id.as_str().to_string()]);
        } else if let Some(client_order_id) = client_order_id {
            params.cl_ord_id(vec![client_order_id.as_str().to_string()]);
        } else {
            return Err(anyhow::anyhow!(
                "Either client_order_id or venue_order_id must be provided"
            ));
        }

        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let response = self.inner.http_cancel_orders(params).await?;

        let orders: Vec<BitmexOrder> = serde_json::from_value(response)?;
        let order = orders
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No order returned in cancel response"))?;

        let price_precision = self.get_price_precision(instrument_id.symbol.inner())?;
        let ts_init = self.generate_ts_init();

        parse_order_status_report(&order, instrument_id, price_precision, ts_init)
    }

    /// Cancel multiple orders.
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - The order doesn't exist.
    /// - The API returns an error.
    pub async fn cancel_orders(
        &self,
        instrument_id: InstrumentId,
        client_order_ids: Option<Vec<ClientOrderId>>,
        venue_order_ids: Option<Vec<VenueOrderId>>,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let mut params = super::query::DeleteOrderParamsBuilder::default();
        params.text(NAUTILUS_TRADER);

        if let Some(client_order_ids) = client_order_ids {
            params.cl_ord_id(
                client_order_ids
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>(),
            );
        }

        if let Some(venue_order_ids) = venue_order_ids {
            params.order_id(
                venue_order_ids
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>(),
            );
        }

        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let response = self.inner.http_cancel_orders(params).await?;

        let orders: Vec<BitmexOrder> = serde_json::from_value(response)?;

        let ts_init = self.generate_ts_init();

        let mut reports = Vec::new();

        for order in orders {
            let price_precision = self.get_price_precision(instrument_id.symbol.inner())?;

            reports.push(parse_order_status_report(
                &order,
                instrument_id,
                price_precision,
                ts_init,
            )?);
        }

        Ok(reports)
    }

    /// Cancel all orders for an instrument and optionally an order side.
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - The order doesn't exist.
    /// - The API returns an error.
    pub async fn cancel_all_orders(
        &self,
        instrument_id: InstrumentId,
        order_side: Option<OrderSide>,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let mut params = DeleteAllOrdersParamsBuilder::default();
        params.text(NAUTILUS_TRADER);
        params.symbol(instrument_id.symbol.as_str());

        if let Some(side) = order_side {
            let side = BitmexSide::try_from_order_side(side)?;
            params.filter(serde_json::json!({
                "side": side
            }));
        }

        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let response = self.inner.http_cancel_all_orders(params).await?;

        let orders: Vec<BitmexOrder> = serde_json::from_value(response)?;

        let price_precision = self.get_price_precision(instrument_id.symbol.inner())?;
        let ts_init = self.generate_ts_init();

        let mut reports = Vec::new();

        for order in orders {
            reports.push(parse_order_status_report(
                &order,
                instrument_id,
                price_precision,
                ts_init,
            )?);
        }

        Ok(reports)
    }

    /// Modify an existing order.
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - The order doesn't exist.
    /// - The order is already closed.
    /// - The API returns an error.
    pub async fn modify_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
        quantity: Option<Quantity>,
        price: Option<Price>,
        trigger_price: Option<Price>,
    ) -> anyhow::Result<OrderStatusReport> {
        let mut params = PutOrderParamsBuilder::default();
        params.text(NAUTILUS_TRADER);

        // Set order ID - prefer venue_order_id if available
        if let Some(venue_order_id) = venue_order_id {
            params.order_id(venue_order_id.as_str());
        } else if let Some(client_order_id) = client_order_id {
            params.orig_cl_ord_id(client_order_id.as_str());
        } else {
            return Err(anyhow::anyhow!(
                "Either client_order_id or venue_order_id must be provided"
            ));
        }

        if let Some(quantity) = quantity {
            params.order_qty(quantity_to_u32(&quantity));
        }

        if let Some(price) = price {
            params.price(price.as_f64());
        }

        if let Some(trigger_price) = trigger_price {
            params.stop_px(trigger_price.as_f64());
        }

        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let response = self.inner.http_amend_order(params).await?;

        let order: BitmexOrder = serde_json::from_value(response)?;

        if let Some(BitmexOrderStatus::Rejected) = order.ord_status {
            let reason = order
                .ord_rej_reason
                .map(|r| r.to_string())
                .unwrap_or_else(|| "No reason provided".to_string());
            return Err(anyhow::anyhow!("Order modification rejected: {}", reason));
        }

        let price_precision = self.get_price_precision(instrument_id.symbol.inner())?;
        let ts_init = self.generate_ts_init();

        parse_order_status_report(&order, instrument_id, price_precision, ts_init)
    }

    /// Query a single order by client order ID or venue order ID.
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - The API returns an error.
    pub async fn query_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let mut params = GetOrderParamsBuilder::default();

        let filter_json = if let Some(client_order_id) = client_order_id {
            serde_json::json!({
                "clOrdID": client_order_id.to_string()
            })
        } else if let Some(venue_order_id) = venue_order_id {
            serde_json::json!({
                "orderID": venue_order_id.to_string()
            })
        } else {
            return Err(anyhow::anyhow!(
                "Either client_order_id or venue_order_id must be provided"
            ));
        };

        params.filter(filter_json);
        params.count(1); // Only need one order

        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let response = self.inner.http_get_orders(params).await?;

        if response.is_empty() {
            return Ok(None);
        }

        let order = &response[0];

        let price_precision = self.get_price_precision(instrument_id.symbol.inner())?;
        let ts_init = self.generate_ts_init();

        let report = parse_order_status_report(order, instrument_id, price_precision, ts_init)?;

        Ok(Some(report))
    }

    /// Request a single order status report.
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - The API returns an error.
    pub async fn request_order_status_report(
        &self,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> anyhow::Result<OrderStatusReport> {
        let mut params = GetOrderParamsBuilder::default();
        params.symbol(instrument_id.symbol.as_str());

        if let Some(venue_order_id) = venue_order_id {
            params.filter(serde_json::json!({
                "orderID": venue_order_id.as_str()
            }));
        } else if let Some(client_order_id) = client_order_id {
            params.filter(serde_json::json!({
                "clOrdID": client_order_id.as_str()
            }));
        }

        params.count(1i32);
        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let response = self.inner.http_get_orders(params).await?;

        let order = response
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("Order not found"))?;

        let price_precision = self.get_price_precision(instrument_id.symbol.inner())?;
        let ts_init = self.generate_ts_init();

        parse_order_status_report(&order, instrument_id, price_precision, ts_init)
    }

    /// Request multiple order status reports.
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - The API returns an error.
    pub async fn request_order_status_reports(
        &self,
        instrument_id: Option<InstrumentId>,
        open_only: bool,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let mut params = GetOrderParamsBuilder::default();

        if let Some(instrument_id) = &instrument_id {
            params.symbol(instrument_id.symbol.as_str());
        }

        if open_only {
            params.filter(serde_json::json!({
                "open": true
            }));
        }

        if let Some(limit) = limit {
            params.count(limit as i32);
        } else {
            params.count(500); // Default count to avoid empty query
        }

        params.reverse(true); // Get newest orders first

        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let response = self.inner.http_get_orders(params).await?;

        let ts_init = self.generate_ts_init();

        let mut reports = Vec::new();

        for order in response {
            // Skip orders without symbol (can happen with query responses)
            let Some(symbol) = order.symbol else {
                tracing::warn!("Order response missing symbol, skipping");
                continue;
            };

            let instrument_id = parse_instrument_id(symbol);
            let price_precision = self.get_price_precision(symbol)?;

            match parse_order_status_report(&order, instrument_id, price_precision, ts_init) {
                Ok(report) => reports.push(report),
                Err(e) => tracing::error!("Failed to parse order status report: {e}"),
            }
        }

        Ok(reports)
    }

    /// Request trades for the given instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or parsing fails.
    pub async fn request_trades(
        &self,
        instrument_id: InstrumentId,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<TradeTick>> {
        let mut params = GetTradeParamsBuilder::default();
        params.symbol(instrument_id.symbol.as_str());

        if let Some(limit) = limit {
            params.count(limit as i32);
        }
        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let response = self.inner.http_get_trades(params).await?;

        let ts_init = self.generate_ts_init();

        let mut parsed_trades = Vec::new();

        for trade in response {
            let price_precision = self.get_price_precision(trade.symbol)?;

            match parse_trade(trade, price_precision, ts_init) {
                Ok(trade) => parsed_trades.push(trade),
                Err(e) => tracing::error!("Failed to parse trade: {e}"),
            }
        }

        Ok(parsed_trades)
    }

    /// Request fill reports for the given instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or parsing fails.
    pub async fn request_fill_reports(
        &self,
        instrument_id: Option<InstrumentId>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<FillReport>> {
        let mut params = GetExecutionParamsBuilder::default();
        if let Some(instrument_id) = instrument_id {
            params.symbol(instrument_id.symbol.as_str());
        }
        if let Some(limit) = limit {
            params.count(limit as i32);
        } else {
            params.count(500); // Default count
        }
        params.reverse(true); // Get newest fills first

        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let response = self.inner.http_get_executions(params).await?;

        let ts_init = self.generate_ts_init();

        let mut reports = Vec::new();

        for exec in response {
            // Skip executions without symbol (e.g., CancelReject)
            let Some(symbol) = exec.symbol else {
                tracing::debug!("Skipping execution without symbol: {:?}", exec.exec_type);
                continue;
            };
            let price_precision = self.get_price_precision(symbol)?;

            match parse_fill_report(exec, price_precision, ts_init) {
                Ok(report) => reports.push(report),
                Err(e) => {
                    // Log at debug level for expected skip cases
                    let error_msg = e.to_string();
                    if error_msg.starts_with("Skipping non-trade execution")
                        || error_msg.starts_with("Skipping execution without order_id")
                    {
                        tracing::debug!("{e}");
                    } else {
                        tracing::error!("Failed to parse fill report: {e}");
                    }
                }
            }
        }

        Ok(reports)
    }

    /// Request position reports.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or parsing fails.
    pub async fn request_position_status_reports(
        &self,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let params = GetPositionParamsBuilder::default()
            .count(500) // Default count
            .build()
            .map_err(|e| anyhow::anyhow!(e))?;

        let response = self.inner.http_get_positions(params).await?;

        let ts_init = self.generate_ts_init();

        let mut reports = Vec::new();

        for pos in response {
            match parse_position_report(pos, ts_init) {
                Ok(report) => reports.push(report),
                Err(e) => tracing::error!("Failed to parse position report: {e}"),
            }
        }

        Ok(reports)
    }

    /// Submit multiple orders in bulk.
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - Order validation fails.
    /// - The API returns an error.
    pub async fn submit_orders_bulk(
        &self,
        orders: Vec<PostOrderParams>,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let params = PostOrderBulkParams { orders };

        let response = self.inner.http_place_orders_bulk(params).await?;

        let ts_init = self.generate_ts_init();
        let mut reports = Vec::new();

        for order in response {
            // Skip orders without symbol (can happen with query responses)
            let Some(symbol) = order.symbol else {
                tracing::warn!("Order response missing symbol, skipping");
                continue;
            };

            let instrument_id = parse_instrument_id(symbol);
            let price_precision = self.get_price_precision(symbol)?;

            match parse_order_status_report(&order, instrument_id, price_precision, ts_init) {
                Ok(report) => reports.push(report),
                Err(e) => tracing::error!("Failed to parse order status report: {e}"),
            }
        }

        Ok(reports)
    }

    /// Amend multiple orders in bulk.
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - An order doesn't exist.
    /// - An order is closed.
    /// - The API returns an error.
    pub async fn modify_orders_bulk(
        &self,
        orders: Vec<PutOrderParams>,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let params = PutOrderBulkParams { orders };

        let response = self.inner.http_amend_orders_bulk(params).await?;

        let ts_init = self.generate_ts_init();
        let mut reports = Vec::new();

        for order in response {
            // Skip orders without symbol (can happen with query responses)
            let Some(symbol) = order.symbol else {
                tracing::warn!("Order response missing symbol, skipping");
                continue;
            };

            let instrument_id = parse_instrument_id(symbol);
            let price_precision = self.get_price_precision(symbol)?;

            match parse_order_status_report(&order, instrument_id, price_precision, ts_init) {
                Ok(report) => reports.push(report),
                Err(e) => tracing::error!("Failed to parse order status report: {e}"),
            }
        }

        Ok(reports)
    }

    /// Update position leverage.
    ///
    /// # Errors
    ///
    /// - Credentials are missing.
    /// - The request fails.
    /// - The API returns an error.
    pub async fn update_position_leverage(
        &self,
        symbol: &str,
        leverage: f64,
    ) -> anyhow::Result<PositionStatusReport> {
        let params = PostPositionLeverageParams {
            symbol: symbol.to_string(),
            leverage,
            target_account_id: None,
        };

        let response = self.inner.http_update_position_leverage(params).await?;

        let ts_init = self.generate_ts_init();

        parse_position_report(response, ts_init)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::json;

    use super::*;

    #[rstest]
    fn test_sign_request_generates_correct_headers() {
        let client = BitmexHttpInnerClient::with_credentials(
            "test_api_key".to_string(),
            "test_api_secret".to_string(),
            "http://localhost:8080".to_string(),
            Some(60),
            None, // max_retries
            None, // retry_delay_ms
            None, // retry_delay_max_ms
        )
        .expect("Failed to create test client");

        let headers = client
            .sign_request(&Method::GET, "/api/v1/order", None)
            .unwrap();

        assert!(headers.contains_key("api-key"));
        assert!(headers.contains_key("api-signature"));
        assert!(headers.contains_key("api-expires"));
        assert_eq!(headers.get("api-key").unwrap(), "test_api_key");
    }

    #[rstest]
    fn test_sign_request_with_body() {
        let client = BitmexHttpInnerClient::with_credentials(
            "test_api_key".to_string(),
            "test_api_secret".to_string(),
            "http://localhost:8080".to_string(),
            Some(60),
            None, // max_retries
            None, // retry_delay_ms
            None, // retry_delay_max_ms
        )
        .expect("Failed to create test client");

        let body = json!({"symbol": "XBTUSD", "orderQty": 100});
        let body_bytes = serde_json::to_vec(&body).unwrap();

        let headers_without_body = client
            .sign_request(&Method::POST, "/api/v1/order", None)
            .unwrap();
        let headers_with_body = client
            .sign_request(&Method::POST, "/api/v1/order", Some(&body_bytes))
            .unwrap();

        // Signatures should be different when body is included
        assert_ne!(
            headers_without_body.get("api-signature").unwrap(),
            headers_with_body.get("api-signature").unwrap()
        );
    }
}
