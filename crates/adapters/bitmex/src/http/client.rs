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
    sync::{Arc, Mutex},
};

use ahash::AHashMap;
use chrono::{DateTime, Utc};
use nautilus_core::{
    MUTEX_POISONED, UnixNanos,
    consts::{NAUTILUS_TRADER, NAUTILUS_USER_AGENT},
    env::get_env_var,
    time::get_atomic_clock_realtime,
};
use nautilus_model::{
    data::{Bar, BarType, TradeTick},
    enums::{
        AggregationSource, BarAggregation, ContingencyType, OrderSide, OrderType, PriceType,
        TimeInForce, TriggerType,
    },
    events::AccountState,
    identifiers::{AccountId, ClientOrderId, InstrumentId, OrderListId, VenueOrderId},
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
        BitmexApiInfo, BitmexExecution, BitmexInstrument, BitmexMargin, BitmexOrder,
        BitmexPosition, BitmexTrade, BitmexTradeBin, BitmexWallet,
    },
    query::{
        DeleteAllOrdersParams, DeleteOrderParams, GetExecutionParams, GetExecutionParamsBuilder,
        GetOrderParams, GetPositionParams, GetPositionParamsBuilder, GetTradeBucketedParams,
        GetTradeBucketedParamsBuilder, GetTradeParams, GetTradeParamsBuilder, PostOrderParams,
        PostPositionLeverageParams, PutOrderParams,
    },
};
use crate::{
    common::{
        consts::{BITMEX_HTTP_TESTNET_URL, BITMEX_HTTP_URL},
        credential::Credential,
        enums::{BitmexContingencyType, BitmexOrderStatus, BitmexSide},
        parse::{parse_account_state, quantity_to_u32},
    },
    http::{
        parse::{
            parse_fill_report, parse_instrument_any, parse_order_status_report,
            parse_position_report, parse_trade, parse_trade_bin,
        },
        query::{DeleteAllOrdersParamsBuilder, GetOrderParamsBuilder, PutOrderParamsBuilder},
    },
    websocket::messages::BitmexMarginMsg,
};

/// Default BitMEX REST API rate limits.
///
/// BitMEX implements a dual-layer rate limiting system:
/// - Primary limit: 120 requests per minute for authenticated users (30 for unauthenticated).
/// - Secondary limit: 10 requests per second burst limit for specific endpoints.
const BITMEX_DEFAULT_RATE_LIMIT_PER_SECOND: u32 = 10;
const BITMEX_DEFAULT_RATE_LIMIT_PER_MINUTE_AUTHENTICATED: u32 = 120;
const BITMEX_DEFAULT_RATE_LIMIT_PER_MINUTE_UNAUTHENTICATED: u32 = 30;

const BITMEX_GLOBAL_RATE_KEY: &str = "bitmex:global";
const BITMEX_MINUTE_RATE_KEY: &str = "bitmex:minute";

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
    recv_window_ms: u64,
    retry_manager: RetryManager<BitmexHttpError>,
    cancellation_token: CancellationToken,
}

impl Default for BitmexHttpInnerClient {
    fn default() -> Self {
        Self::new(None, Some(60), None, None, None, None, None, None)
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
        recv_window_ms: Option<u64>,
        max_requests_per_second: Option<u32>,
        max_requests_per_minute: Option<u32>,
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

        let max_req_per_sec =
            max_requests_per_second.unwrap_or(BITMEX_DEFAULT_RATE_LIMIT_PER_SECOND);
        let max_req_per_min =
            max_requests_per_minute.unwrap_or(BITMEX_DEFAULT_RATE_LIMIT_PER_MINUTE_UNAUTHENTICATED);

        Ok(Self {
            base_url: base_url.unwrap_or(BITMEX_HTTP_URL.to_string()),
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                Self::rate_limiter_quotas(max_req_per_sec, max_req_per_min),
                Some(Self::default_quota(max_req_per_sec)),
                timeout_secs,
            ),
            credential: None,
            recv_window_ms: recv_window_ms.unwrap_or(10_000),
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
        recv_window_ms: Option<u64>,
        max_requests_per_second: Option<u32>,
        max_requests_per_minute: Option<u32>,
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

        let max_req_per_sec =
            max_requests_per_second.unwrap_or(BITMEX_DEFAULT_RATE_LIMIT_PER_SECOND);
        let max_req_per_min =
            max_requests_per_minute.unwrap_or(BITMEX_DEFAULT_RATE_LIMIT_PER_MINUTE_AUTHENTICATED);

        Ok(Self {
            base_url,
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                Self::rate_limiter_quotas(max_req_per_sec, max_req_per_min),
                Some(Self::default_quota(max_req_per_sec)),
                timeout_secs,
            ),
            credential: Some(Credential::new(api_key, api_secret)),
            recv_window_ms: recv_window_ms.unwrap_or(10_000),
            retry_manager,
            cancellation_token: CancellationToken::new(),
        })
    }

    fn default_headers() -> HashMap<String, String> {
        HashMap::from([(USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string())])
    }

    fn default_quota(max_requests_per_second: u32) -> Quota {
        Quota::per_second(
            NonZeroU32::new(max_requests_per_second)
                .unwrap_or_else(|| NonZeroU32::new(BITMEX_DEFAULT_RATE_LIMIT_PER_SECOND).unwrap()),
        )
    }

    fn rate_limiter_quotas(
        max_requests_per_second: u32,
        max_requests_per_minute: u32,
    ) -> Vec<(String, Quota)> {
        let per_sec_quota = Quota::per_second(
            NonZeroU32::new(max_requests_per_second)
                .unwrap_or_else(|| NonZeroU32::new(BITMEX_DEFAULT_RATE_LIMIT_PER_SECOND).unwrap()),
        );
        let per_min_quota =
            Quota::per_minute(NonZeroU32::new(max_requests_per_minute).unwrap_or_else(|| {
                NonZeroU32::new(BITMEX_DEFAULT_RATE_LIMIT_PER_MINUTE_AUTHENTICATED).unwrap()
            }));

        vec![
            (BITMEX_GLOBAL_RATE_KEY.to_string(), per_sec_quota),
            (BITMEX_MINUTE_RATE_KEY.to_string(), per_min_quota),
        ]
    }

    fn rate_limit_keys() -> Vec<Ustr> {
        vec![
            Ustr::from(BITMEX_GLOBAL_RATE_KEY),
            Ustr::from(BITMEX_MINUTE_RATE_KEY),
        ]
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

        let expires = Utc::now().timestamp() + (self.recv_window_ms / 1000) as i64;
        let body_str = body.and_then(|b| std::str::from_utf8(b).ok()).unwrap_or("");

        let full_path = if endpoint.starts_with("/api/v1") {
            endpoint.to_string()
        } else {
            format!("/api/v1{endpoint}")
        };

        let signature = credential.sign(method.as_str(), &full_path, expires, body_str);

        let mut headers = HashMap::new();
        headers.insert("api-expires".to_string(), expires.to_string());
        headers.insert("api-key".to_string(), credential.api_key.to_string());
        headers.insert("api-signature".to_string(), signature);

        // Add Content-Type header for form-encoded body
        if body.is_some()
            && (*method == Method::POST || *method == Method::PUT || *method == Method::DELETE)
        {
            headers.insert(
                "Content-Type".to_string(),
                "application/x-www-form-urlencoded".to_string(),
            );
        }

        Ok(headers)
    }

    async fn send_request<T: DeserializeOwned>(
        &self,
        method: Method,
        endpoint: &str,
        body: Option<Vec<u8>>,
        authenticate: bool,
    ) -> Result<T, BitmexHttpError> {
        let endpoint = endpoint.to_string();
        let url = format!("{}{endpoint}", self.base_url);
        let method_clone = method.clone();
        let body_clone = body.clone();

        let operation = || {
            let url = url.clone();
            let method = method_clone.clone();
            let body = body_clone.clone();
            let endpoint = endpoint.clone();

            async move {
                let headers = if authenticate {
                    Some(self.sign_request(&method, endpoint.as_str(), body.as_deref())?)
                } else {
                    None
                };

                let rate_keys = Self::rate_limit_keys();
                let resp = self
                    .client
                    .request_with_ustr_keys(method, url, headers, body, None, Some(rate_keys))
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
                BitmexHttpError::Canceled("Adapter disconnecting or shutting down".to_string())
            } else {
                BitmexHttpError::NetworkError(msg)
            }
        };

        self.retry_manager
            .execute_with_retry_with_cancel(
                endpoint.as_str(),
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

    /// Requests the current server time from BitMEX.
    ///
    /// Retrieves the BitMEX API info including the system time in Unix timestamp (milliseconds).
    /// This is useful for synchronizing local clocks with the exchange server and logging time drift.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or if the response body
    /// cannot be parsed into [`BitmexApiInfo`].
    pub async fn http_get_server_time(&self) -> Result<u64, BitmexHttpError> {
        let response: BitmexApiInfo = self.send_request(Method::GET, "", None, false).await?;
        Ok(response.timestamp)
    }

    /// Get the instrument definition for the specified symbol.
    ///
    /// BitMEX responds to `/instrument?symbol=...` with an array, even when
    /// a single symbol is requested. This helper returns the first element of
    /// that array and yields `Ok(None)` when the venue returns an empty list
    /// (e.g. unknown symbol).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the payload cannot be deserialized.
    pub async fn http_get_instrument(
        &self,
        symbol: &str,
    ) -> Result<Option<BitmexInstrument>, BitmexHttpError> {
        let path = &format!("/instrument?symbol={symbol}");
        let instruments: Vec<BitmexInstrument> =
            self.send_request(Method::GET, path, None, false).await?;

        Ok(instruments.into_iter().next())
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

    /// Get bucketed (aggregated) trade data.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or the API returns an error.
    pub async fn http_get_trade_bucketed(
        &self,
        params: GetTradeBucketedParams,
    ) -> Result<Vec<BitmexTradeBin>, BitmexHttpError> {
        let query = serde_urlencoded::to_string(&params).map_err(|e| {
            BitmexHttpError::ValidationError(format!("Failed to serialize parameters: {e}"))
        })?;
        let path = format!("/trade/bucketed?{query}");
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
        // BitMEX spec requires form-encoded body for POST /order
        let body = serde_urlencoded::to_string(&params)
            .map_err(|e| {
                BitmexHttpError::ValidationError(format!("Failed to serialize parameters: {e}"))
            })?
            .into_bytes();
        let path = "/order";
        self.send_request(Method::POST, path, Some(body), true)
            .await
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
        // BitMEX spec requires form-encoded body for DELETE /order
        let body = serde_urlencoded::to_string(&params)
            .map_err(|e| {
                BitmexHttpError::ValidationError(format!("Failed to serialize parameters: {e}"))
            })?
            .into_bytes();
        let path = "/order";
        self.send_request(Method::DELETE, path, Some(body), true)
            .await
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
        // BitMEX spec requires form-encoded body for PUT /order
        let body = serde_urlencoded::to_string(&params)
            .map_err(|e| {
                BitmexHttpError::ValidationError(format!("Failed to serialize parameters: {e}"))
            })?
            .into_bytes();
        let path = "/order";
        self.send_request(Method::PUT, path, Some(body), true).await
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
        // BitMEX spec requires form-encoded body for POST endpoints
        let body = serde_urlencoded::to_string(&params)
            .map_err(|e| {
                BitmexHttpError::ValidationError(format!("Failed to serialize parameters: {e}"))
            })?
            .into_bytes();
        let path = "/position/leverage";
        self.send_request(Method::POST, path, Some(body), true)
            .await
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
        Self::new(
            None,
            None,
            None,
            false,
            Some(60),
            None,
            None,
            None,
            None,
            None,
            None,
        )
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
        recv_window_ms: Option<u64>,
        max_requests_per_second: Option<u32>,
        max_requests_per_minute: Option<u32>,
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
                recv_window_ms,
                max_requests_per_second,
                max_requests_per_minute,
            )?,
            _ => BitmexHttpInnerClient::new(
                Some(url),
                timeout_secs,
                max_retries,
                retry_delay_ms,
                retry_delay_max_ms,
                recv_window_ms,
                max_requests_per_second,
                max_requests_per_minute,
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
        Self::with_credentials(None, None, None, None, None, None, None, None, None, None)
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
        recv_window_ms: Option<u64>,
        max_requests_per_second: Option<u32>,
        max_requests_per_minute: Option<u32>,
    ) -> anyhow::Result<Self> {
        // Determine testnet from URL first to select correct environment variables
        let testnet = base_url.as_ref().is_some_and(|url| url.contains("testnet"));

        // Choose environment variables based on testnet flag
        let (key_var, secret_var) = if testnet {
            ("BITMEX_TESTNET_API_KEY", "BITMEX_TESTNET_API_SECRET")
        } else {
            ("BITMEX_API_KEY", "BITMEX_API_SECRET")
        };

        let api_key = api_key.or_else(|| get_env_var(key_var).ok());
        let api_secret = api_secret.or_else(|| get_env_var(secret_var).ok());

        // If we're trying to create an authenticated client, we need both key and secret
        if api_key.is_some() && api_secret.is_none() {
            anyhow::bail!("{secret_var} is required when {key_var} is provided");
        }
        if api_key.is_none() && api_secret.is_some() {
            anyhow::bail!("{key_var} is required when {secret_var} is provided");
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
            recv_window_ms,
            max_requests_per_second,
            max_requests_per_minute,
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

    /// Requests the current server time from BitMEX.
    ///
    /// Returns the BitMEX system time as a Unix timestamp in milliseconds.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or if the response cannot be parsed.
    pub async fn http_get_server_time(&self) -> Result<u64, BitmexHttpError> {
        self.inner.http_get_server_time().await
    }

    /// Generates a timestamp for initialization.
    fn generate_ts_init(&self) -> UnixNanos {
        get_atomic_clock_realtime().get_time_ns()
    }

    /// Check if the order has a contingency type that requires linking.
    fn is_contingent_order(contingency_type: ContingencyType) -> bool {
        matches!(
            contingency_type,
            ContingencyType::Oco | ContingencyType::Oto | ContingencyType::Ouo
        )
    }

    /// Check if the order is a parent in contingency relationships.
    fn is_parent_contingency(contingency_type: ContingencyType) -> bool {
        matches!(
            contingency_type,
            ContingencyType::Oco | ContingencyType::Oto
        )
    }

    /// Populate missing `linked_order_ids` for contingency orders by grouping on `order_list_id`.
    fn populate_linked_order_ids(reports: &mut [OrderStatusReport]) {
        let mut order_list_groups: HashMap<OrderListId, Vec<ClientOrderId>> = HashMap::new();
        let mut order_list_parents: HashMap<OrderListId, ClientOrderId> = HashMap::new();
        let mut prefix_groups: HashMap<String, Vec<ClientOrderId>> = HashMap::new();
        let mut prefix_parents: HashMap<String, ClientOrderId> = HashMap::new();

        for report in reports.iter() {
            let Some(client_order_id) = report.client_order_id else {
                continue;
            };

            if let Some(order_list_id) = report.order_list_id {
                order_list_groups
                    .entry(order_list_id)
                    .or_default()
                    .push(client_order_id);

                if Self::is_parent_contingency(report.contingency_type) {
                    order_list_parents
                        .entry(order_list_id)
                        .or_insert(client_order_id);
                }
            }

            if let Some((base, _)) = client_order_id.as_str().rsplit_once('-')
                && Self::is_contingent_order(report.contingency_type)
            {
                prefix_groups
                    .entry(base.to_owned())
                    .or_default()
                    .push(client_order_id);

                if Self::is_parent_contingency(report.contingency_type) {
                    prefix_parents
                        .entry(base.to_owned())
                        .or_insert(client_order_id);
                }
            }
        }

        for report in reports.iter_mut() {
            let Some(client_order_id) = report.client_order_id else {
                continue;
            };

            if report.linked_order_ids.is_some() {
                continue;
            }

            // Only process contingent orders
            if !Self::is_contingent_order(report.contingency_type) {
                continue;
            }

            if let Some(order_list_id) = report.order_list_id
                && let Some(group) = order_list_groups.get(&order_list_id)
            {
                let mut linked: Vec<ClientOrderId> = group
                    .iter()
                    .copied()
                    .filter(|candidate| candidate != &client_order_id)
                    .collect();

                if !linked.is_empty() {
                    if let Some(parent_id) = order_list_parents.get(&order_list_id) {
                        if client_order_id != *parent_id {
                            linked.sort_by_key(
                                |candidate| {
                                    if candidate == parent_id { 0 } else { 1 }
                                },
                            );
                            report.parent_order_id = Some(*parent_id);
                        } else {
                            report.parent_order_id = None;
                        }
                    } else {
                        report.parent_order_id = None;
                    }

                    tracing::trace!(
                        client_order_id = ?client_order_id,
                        order_list_id = ?order_list_id,
                        contingency_type = ?report.contingency_type,
                        linked_order_ids = ?linked,
                        "BitMEX linked ids sourced from order list id",
                    );
                    report.linked_order_ids = Some(linked);
                    continue;
                }

                tracing::trace!(
                    client_order_id = ?client_order_id,
                    order_list_id = ?order_list_id,
                    contingency_type = ?report.contingency_type,
                    order_list_group = ?group,
                    "BitMEX order list id group had no peers",
                );
                report.parent_order_id = None;
            } else if report.order_list_id.is_none() {
                report.parent_order_id = None;
            }

            if let Some((base, _)) = client_order_id.as_str().rsplit_once('-')
                && let Some(group) = prefix_groups.get(base)
            {
                let mut linked: Vec<ClientOrderId> = group
                    .iter()
                    .copied()
                    .filter(|candidate| candidate != &client_order_id)
                    .collect();

                if !linked.is_empty() {
                    if let Some(parent_id) = prefix_parents.get(base) {
                        if client_order_id != *parent_id {
                            linked.sort_by_key(
                                |candidate| {
                                    if candidate == parent_id { 0 } else { 1 }
                                },
                            );
                            report.parent_order_id = Some(*parent_id);
                        } else {
                            report.parent_order_id = None;
                        }
                    } else {
                        report.parent_order_id = None;
                    }

                    tracing::trace!(
                        client_order_id = ?client_order_id,
                        contingency_type = ?report.contingency_type,
                        base = base,
                        linked_order_ids = ?linked,
                        "BitMEX linked ids constructed from client order id prefix",
                    );
                    report.linked_order_ids = Some(linked);
                    continue;
                }

                tracing::trace!(
                    client_order_id = ?client_order_id,
                    contingency_type = ?report.contingency_type,
                    base = base,
                    prefix_group = ?group,
                    "BitMEX client order id prefix group had no peers",
                );
                report.parent_order_id = None;
            } else if client_order_id.as_str().contains('-') {
                report.parent_order_id = None;
            }

            if Self::is_contingent_order(report.contingency_type) {
                tracing::warn!(
                    client_order_id = ?report.client_order_id,
                    order_list_id = ?report.order_list_id,
                    contingency_type = ?report.contingency_type,
                    "BitMEX order status report missing linked ids after grouping",
                );
                report.contingency_type = ContingencyType::NoContingency;
                report.parent_order_id = None;
            }

            report.linked_order_ids = None;
        }
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
    pub fn add_instrument(&self, instrument: InstrumentAny) {
        self.instruments_cache
            .lock()
            .unwrap()
            .insert(instrument.raw_symbol().inner(), instrument);
    }

    /// Request a single instrument and parse it into a Nautilus type.
    ///
    /// # Errors
    ///
    /// Returns `Ok(Some(..))` when the venue returns a definition that parses
    /// successfully, `Ok(None)` when the instrument is unknown or the payload
    /// cannot be converted into a Nautilus `Instrument`.
    pub async fn request_instrument(
        &self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<Option<InstrumentAny>> {
        let response = self
            .inner
            .http_get_instrument(instrument_id.symbol.as_str())
            .await?;

        let instrument = match response {
            Some(instrument) => instrument,
            None => return Ok(None),
        };

        let ts_init = self.generate_ts_init();

        Ok(parse_instrument_any(&instrument, ts_init))
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
        let mut failed_count = 0;
        let total_count = instruments.len();

        for inst in instruments {
            if let Some(instrument_any) = parse_instrument_any(&inst, ts_init) {
                parsed_instruments.push(instrument_any);
            } else {
                failed_count += 1;
                tracing::error!(
                    "Failed to parse instrument: symbol={}, type={:?}, state={:?} - instrument will not be cached",
                    inst.symbol,
                    inst.instrument_type,
                    inst.state
                );
            }
        }

        if failed_count > 0 {
            tracing::error!(
                "Instrument parse failures: {} failed out of {} total ({}  successfully parsed)",
                failed_count,
                total_count,
                parsed_instruments.len()
            );
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
    fn instrument_from_cache(&self, symbol: Ustr) -> anyhow::Result<InstrumentAny> {
        let cache = self.instruments_cache.lock().expect(MUTEX_POISONED);
        cache.get(&symbol).cloned().ok_or_else(|| {
            anyhow::anyhow!(
                "Instrument {symbol} not found in cache, ensure instruments loaded first"
            )
        })
    }

    /// Returns the cached price precision for the given symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument was never cached (for example, if
    /// instruments were not loaded prior to use).
    pub fn get_price_precision(&self, symbol: Ustr) -> anyhow::Result<u8> {
        self.instrument_from_cache(symbol)
            .map(|instrument| instrument.price_precision())
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
        trigger_type: Option<TriggerType>,
        display_qty: Option<Quantity>,
        post_only: bool,
        reduce_only: bool,
        order_list_id: Option<OrderListId>,
        contingency_type: Option<ContingencyType>,
    ) -> anyhow::Result<OrderStatusReport> {
        use crate::common::enums::{
            BitmexExecInstruction, BitmexOrderType, BitmexSide, BitmexTimeInForce,
        };

        let instrument = self.instrument_from_cache(instrument_id.symbol.inner())?;

        let mut params = super::query::PostOrderParamsBuilder::default();
        params.text(NAUTILUS_TRADER);
        params.symbol(instrument_id.symbol.as_str());
        params.cl_ord_id(client_order_id.as_str());

        let side = BitmexSide::try_from_order_side(order_side)?;
        params.side(side);

        let ord_type = BitmexOrderType::try_from_order_type(order_type)?;
        params.ord_type(ord_type);

        params.order_qty(quantity_to_u32(&quantity, &instrument));

        let tif = BitmexTimeInForce::try_from_time_in_force(time_in_force)?;
        params.time_in_force(tif);

        if let Some(price) = price {
            params.price(price.as_f64());
        }

        if let Some(trigger_price) = trigger_price {
            params.stop_px(trigger_price.as_f64());
        }

        if let Some(display_qty) = display_qty {
            params.display_qty(quantity_to_u32(&display_qty, &instrument));
        }

        if let Some(order_list_id) = order_list_id {
            params.cl_ord_link_id(order_list_id.as_str());
        }

        let mut exec_inst = Vec::new();

        if post_only {
            exec_inst.push(BitmexExecInstruction::ParticipateDoNotInitiate);
        }

        if reduce_only {
            exec_inst.push(BitmexExecInstruction::ReduceOnly);
        }

        if trigger_price.is_some()
            && let Some(trigger_type) = trigger_type
        {
            match trigger_type {
                TriggerType::LastPrice => exec_inst.push(BitmexExecInstruction::LastPrice),
                TriggerType::MarkPrice => exec_inst.push(BitmexExecInstruction::MarkPrice),
                TriggerType::IndexPrice => exec_inst.push(BitmexExecInstruction::IndexPrice),
                _ => {} // Use BitMEX default (LastPrice) for other trigger types
            }
        }

        if !exec_inst.is_empty() {
            params.exec_inst(exec_inst);
        }

        if let Some(contingency_type) = contingency_type {
            let bitmex_contingency = BitmexContingencyType::try_from(contingency_type)?;
            params.contingency_type(bitmex_contingency);
        }

        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let response = self.inner.http_place_order(params).await?;

        let order: BitmexOrder = serde_json::from_value(response)?;

        if let Some(BitmexOrderStatus::Rejected) = order.ord_status {
            let reason = order
                .ord_rej_reason
                .map_or_else(|| "No reason provided".to_string(), |r| r.to_string());
            anyhow::bail!("Order rejected: {reason}");
        }

        let instrument = self.instrument_from_cache(instrument_id.symbol.inner())?;
        let ts_init = self.generate_ts_init();

        parse_order_status_report(&order, &instrument, ts_init)
    }

    /// Cancel an order.
    ///
    /// # Errors
    ///
    /// Returns an error if:
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
            anyhow::bail!("Either client_order_id or venue_order_id must be provided");
        }

        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let response = self.inner.http_cancel_orders(params).await?;

        let orders: Vec<BitmexOrder> = serde_json::from_value(response)?;
        let order = orders
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No order returned in cancel response"))?;

        let instrument = self.instrument_from_cache(instrument_id.symbol.inner())?;
        let ts_init = self.generate_ts_init();

        parse_order_status_report(&order, &instrument, ts_init)
    }

    /// Cancel multiple orders.
    ///
    /// # Errors
    ///
    /// Returns an error if:
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

        // BitMEX API requires either client order IDs or venue order IDs, not both
        // Prioritize venue order IDs if both are provided
        if let Some(venue_order_ids) = venue_order_ids {
            if venue_order_ids.is_empty() {
                anyhow::bail!("venue_order_ids cannot be empty");
            }
            params.order_id(
                venue_order_ids
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>(),
            );
        } else if let Some(client_order_ids) = client_order_ids {
            if client_order_ids.is_empty() {
                anyhow::bail!("client_order_ids cannot be empty");
            }
            params.cl_ord_id(
                client_order_ids
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>(),
            );
        } else {
            anyhow::bail!("Either client_order_ids or venue_order_ids must be provided");
        }

        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let response = self.inner.http_cancel_orders(params).await?;

        let orders: Vec<BitmexOrder> = serde_json::from_value(response)?;

        let ts_init = self.generate_ts_init();
        let instrument = self.instrument_from_cache(instrument_id.symbol.inner())?;

        let mut reports = Vec::new();

        for order in orders {
            reports.push(parse_order_status_report(&order, &instrument, ts_init)?);
        }

        Self::populate_linked_order_ids(&mut reports);

        Ok(reports)
    }

    /// Cancel all orders for an instrument and optionally an order side.
    ///
    /// # Errors
    ///
    /// Returns an error if:
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

        let instrument = self.instrument_from_cache(instrument_id.symbol.inner())?;
        let ts_init = self.generate_ts_init();

        let mut reports = Vec::new();

        for order in orders {
            reports.push(parse_order_status_report(&order, &instrument, ts_init)?);
        }

        Self::populate_linked_order_ids(&mut reports);

        Ok(reports)
    }

    /// Modify an existing order.
    ///
    /// # Errors
    ///
    /// Returns an error if:
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
            anyhow::bail!("Either client_order_id or venue_order_id must be provided");
        }

        if let Some(quantity) = quantity {
            let instrument = self.instrument_from_cache(instrument_id.symbol.inner())?;
            params.order_qty(quantity_to_u32(&quantity, &instrument));
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
                .map_or_else(|| "No reason provided".to_string(), |r| r.to_string());
            anyhow::bail!("Order modification rejected: {reason}");
        }

        let instrument = self.instrument_from_cache(instrument_id.symbol.inner())?;
        let ts_init = self.generate_ts_init();

        parse_order_status_report(&order, &instrument, ts_init)
    }

    /// Query a single order by client order ID or venue order ID.
    ///
    /// # Errors
    ///
    /// Returns an error if:
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
            anyhow::bail!("Either client_order_id or venue_order_id must be provided");
        };

        params.filter(filter_json);
        params.count(1); // Only need one order

        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let response = self.inner.http_get_orders(params).await?;

        if response.is_empty() {
            return Ok(None);
        }

        let order = &response[0];

        let instrument = self.instrument_from_cache(instrument_id.symbol.inner())?;
        let ts_init = self.generate_ts_init();

        let report = parse_order_status_report(order, &instrument, ts_init)?;

        Ok(Some(report))
    }

    /// Request a single order status report.
    ///
    /// # Errors
    ///
    /// Returns an error if:
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

        let instrument = self.instrument_from_cache(instrument_id.symbol.inner())?;
        let ts_init = self.generate_ts_init();

        parse_order_status_report(&order, &instrument, ts_init)
    }

    /// Request multiple order status reports.
    ///
    /// # Errors
    ///
    /// Returns an error if:
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

            let instrument = self.instrument_from_cache(symbol)?;

            match parse_order_status_report(&order, &instrument, ts_init) {
                Ok(report) => reports.push(report),
                Err(e) => tracing::error!("Failed to parse order status report: {e}"),
            }
        }

        Self::populate_linked_order_ids(&mut reports);

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
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<TradeTick>> {
        let mut params = GetTradeParamsBuilder::default();
        params.symbol(instrument_id.symbol.as_str());

        if let Some(start) = start {
            params.start_time(start);
        }

        if let Some(end) = end {
            params.end_time(end);
        }

        if let (Some(start), Some(end)) = (start, end) {
            anyhow::ensure!(
                start < end,
                "Invalid time range: start={start:?} end={end:?}",
            );
        }

        if let Some(limit) = limit {
            let clamped_limit = limit.min(1000);
            if limit > 1000 {
                tracing::warn!(
                    limit,
                    clamped_limit,
                    "BitMEX trade request limit exceeds venue maximum; clamping",
                );
            }
            params.count(i32::try_from(clamped_limit).unwrap_or(1000));
        }
        params.reverse(false);
        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let response = self.inner.http_get_trades(params).await?;

        let ts_init = self.generate_ts_init();

        let mut parsed_trades = Vec::new();

        for trade in response {
            if let Some(start) = start
                && trade.timestamp < start
            {
                continue;
            }

            if let Some(end) = end
                && trade.timestamp > end
            {
                continue;
            }

            let price_precision = self.get_price_precision(trade.symbol)?;

            match parse_trade(trade, price_precision, ts_init) {
                Ok(trade) => parsed_trades.push(trade),
                Err(e) => tracing::error!("Failed to parse trade: {e}"),
            }
        }

        Ok(parsed_trades)
    }

    /// Request bars for the given bar type.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails, parsing fails, or the bar specification is
    /// unsupported by BitMEX.
    pub async fn request_bars(
        &self,
        mut bar_type: BarType,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
        partial: bool,
    ) -> anyhow::Result<Vec<Bar>> {
        bar_type = bar_type.standard();

        anyhow::ensure!(
            bar_type.aggregation_source() == AggregationSource::External,
            "Only EXTERNAL aggregation bars are supported"
        );
        anyhow::ensure!(
            bar_type.spec().price_type == PriceType::Last,
            "Only LAST price type bars are supported"
        );
        if let (Some(start), Some(end)) = (start, end) {
            anyhow::ensure!(
                start < end,
                "Invalid time range: start={start:?} end={end:?}"
            );
        }

        let spec = bar_type.spec();
        let bin_size = match (spec.aggregation, spec.step.get()) {
            (BarAggregation::Minute, 1) => "1m",
            (BarAggregation::Minute, 5) => "5m",
            (BarAggregation::Hour, 1) => "1h",
            (BarAggregation::Day, 1) => "1d",
            _ => anyhow::bail!(
                "BitMEX does not support {}-{:?}-{:?} bars",
                spec.step.get(),
                spec.aggregation,
                spec.price_type,
            ),
        };

        let instrument_id = bar_type.instrument_id();
        let instrument = self.instrument_from_cache(instrument_id.symbol.inner())?;

        let mut params = GetTradeBucketedParamsBuilder::default();
        params.symbol(instrument_id.symbol.as_str());
        params.bin_size(bin_size);
        if partial {
            params.partial(true);
        }
        if let Some(start) = start {
            params.start_time(start);
        }
        if let Some(end) = end {
            params.end_time(end);
        }
        if let Some(limit) = limit {
            let clamped_limit = limit.min(1000);
            if limit > 1000 {
                tracing::warn!(
                    limit,
                    clamped_limit,
                    "BitMEX bar request limit exceeds venue maximum; clamping",
                );
            }
            params.count(i32::try_from(clamped_limit).unwrap_or(1000));
        }
        params.reverse(false);
        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let response = self.inner.http_get_trade_bucketed(params).await?;
        let ts_init = self.generate_ts_init();
        let mut bars = Vec::new();

        for bin in response {
            if let Some(start) = start
                && bin.timestamp < start
            {
                continue;
            }
            if let Some(end) = end
                && bin.timestamp > end
            {
                continue;
            }
            if bin.symbol != instrument_id.symbol.inner() {
                tracing::warn!(
                    symbol = %bin.symbol,
                    expected = %instrument_id.symbol,
                    "Skipping trade bin for unexpected symbol",
                );
                continue;
            }

            match parse_trade_bin(bin, &instrument, &bar_type, ts_init) {
                Ok(bar) => bars.push(bar),
                Err(e) => tracing::warn!("Failed to parse trade bin: {e}"),
            }
        }

        Ok(bars)
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
            let symbol_str = symbol.to_string();

            let instrument = match self.instrument_from_cache(symbol) {
                Ok(instrument) => instrument,
                Err(e) => {
                    tracing::error!(symbol = %symbol_str, "Instrument not found in cache for execution parsing: {e}");
                    continue;
                }
            };

            match parse_fill_report(exec, &instrument, ts_init) {
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
            let symbol = Ustr::from(pos.symbol.as_str());
            let instrument = match self.instrument_from_cache(symbol) {
                Ok(instrument) => instrument,
                Err(e) => {
                    tracing::error!(
                        symbol = pos.symbol.as_str(),
                        "Instrument not found in cache for position parsing: {e}"
                    );
                    continue;
                }
            };

            match parse_position_report(pos, &instrument, ts_init) {
                Ok(report) => reports.push(report),
                Err(e) => tracing::error!("Failed to parse position report: {e}"),
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

        let instrument = self.instrument_from_cache(Ustr::from(symbol))?;
        let ts_init = self.generate_ts_init();

        parse_position_report(response, &instrument, ts_init)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use nautilus_core::UUID4;
    use nautilus_model::enums::OrderStatus;
    use rstest::rstest;
    use serde_json::json;

    use super::*;

    fn build_report(
        client_order_id: &str,
        venue_order_id: &str,
        contingency_type: ContingencyType,
        order_list_id: Option<&str>,
    ) -> OrderStatusReport {
        let mut report = OrderStatusReport::new(
            AccountId::from("BITMEX-1"),
            InstrumentId::from("XBTUSD.BITMEX"),
            Some(ClientOrderId::from(client_order_id)),
            VenueOrderId::from(venue_order_id),
            OrderSide::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            Quantity::new(100.0, 0),
            Quantity::default(),
            UnixNanos::from(1_u64),
            UnixNanos::from(1_u64),
            UnixNanos::from(1_u64),
            Some(UUID4::new()),
        );

        if let Some(id) = order_list_id {
            report = report.with_order_list_id(OrderListId::from(id));
        }

        report.with_contingency_type(contingency_type)
    }

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
            None, // recv_window_ms
            None, // max_requests_per_second
            None, // max_requests_per_minute
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
            None, // recv_window_ms
            None, // max_requests_per_second
            None, // max_requests_per_minute
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

    #[rstest]
    fn test_sign_request_uses_custom_recv_window() {
        let client_default = BitmexHttpInnerClient::with_credentials(
            "test_api_key".to_string(),
            "test_api_secret".to_string(),
            "http://localhost:8080".to_string(),
            Some(60),
            None,
            None,
            None,
            None, // Use default recv_window_ms (10000ms = 10s)
            None, // max_requests_per_second
            None, // max_requests_per_minute
        )
        .expect("Failed to create test client");

        let client_custom = BitmexHttpInnerClient::with_credentials(
            "test_api_key".to_string(),
            "test_api_secret".to_string(),
            "http://localhost:8080".to_string(),
            Some(60),
            None,
            None,
            None,
            Some(30_000), // 30 seconds
            None,         // max_requests_per_second
            None,         // max_requests_per_minute
        )
        .expect("Failed to create test client");

        let headers_default = client_default
            .sign_request(&Method::GET, "/api/v1/order", None)
            .unwrap();
        let headers_custom = client_custom
            .sign_request(&Method::GET, "/api/v1/order", None)
            .unwrap();

        // Parse expires timestamps
        let expires_default: i64 = headers_default.get("api-expires").unwrap().parse().unwrap();
        let expires_custom: i64 = headers_custom.get("api-expires").unwrap().parse().unwrap();

        // Verify both are valid future timestamps
        let now = Utc::now().timestamp();
        assert!(expires_default > now);
        assert!(expires_custom > now);

        // Custom window should be greater than default
        assert!(expires_custom > expires_default);

        // The difference should be approximately 20 seconds (30s - 10s)
        // Allow wider tolerance for delays between calls on slow CI runners
        let diff = expires_custom - expires_default;
        assert!((18..=25).contains(&diff));
    }

    #[rstest]
    fn test_populate_linked_order_ids_from_order_list() {
        let base = "O-20250922-002219-001-000";
        let entry = format!("{base}-1");
        let stop = format!("{base}-2");
        let take = format!("{base}-3");

        let mut reports = vec![
            build_report(&entry, "V-1", ContingencyType::Oto, Some("OL-1")),
            build_report(&stop, "V-2", ContingencyType::Ouo, Some("OL-1")),
            build_report(&take, "V-3", ContingencyType::Ouo, Some("OL-1")),
        ];

        BitmexHttpClient::populate_linked_order_ids(&mut reports);

        assert_eq!(
            reports[0].linked_order_ids,
            Some(vec![
                ClientOrderId::from(stop.as_str()),
                ClientOrderId::from(take.as_str()),
            ]),
        );
        assert_eq!(
            reports[1].linked_order_ids,
            Some(vec![
                ClientOrderId::from(entry.as_str()),
                ClientOrderId::from(take.as_str()),
            ]),
        );
        assert_eq!(
            reports[2].linked_order_ids,
            Some(vec![
                ClientOrderId::from(entry.as_str()),
                ClientOrderId::from(stop.as_str()),
            ]),
        );
    }

    #[rstest]
    fn test_populate_linked_order_ids_from_id_prefix() {
        let base = "O-20250922-002220-001-000";
        let entry = format!("{base}-1");
        let stop = format!("{base}-2");
        let take = format!("{base}-3");

        let mut reports = vec![
            build_report(&entry, "V-1", ContingencyType::Oto, None),
            build_report(&stop, "V-2", ContingencyType::Ouo, None),
            build_report(&take, "V-3", ContingencyType::Ouo, None),
        ];

        BitmexHttpClient::populate_linked_order_ids(&mut reports);

        assert_eq!(
            reports[0].linked_order_ids,
            Some(vec![
                ClientOrderId::from(stop.as_str()),
                ClientOrderId::from(take.as_str()),
            ]),
        );
        assert_eq!(
            reports[1].linked_order_ids,
            Some(vec![
                ClientOrderId::from(entry.as_str()),
                ClientOrderId::from(take.as_str()),
            ]),
        );
        assert_eq!(
            reports[2].linked_order_ids,
            Some(vec![
                ClientOrderId::from(entry.as_str()),
                ClientOrderId::from(stop.as_str()),
            ]),
        );
    }

    #[rstest]
    fn test_populate_linked_order_ids_respects_non_contingent_orders() {
        let base = "O-20250922-002221-001-000";
        let entry = format!("{base}-1");
        let passive = format!("{base}-2");

        let mut reports = vec![
            build_report(&entry, "V-1", ContingencyType::NoContingency, None),
            build_report(&passive, "V-2", ContingencyType::Ouo, None),
        ];

        BitmexHttpClient::populate_linked_order_ids(&mut reports);

        // Non-contingent orders should not be linked
        assert!(reports[0].linked_order_ids.is_none());

        // A contingent order with no other contingent peers should have contingency reset
        assert!(reports[1].linked_order_ids.is_none());
        assert_eq!(reports[1].contingency_type, ContingencyType::NoContingency);
    }
}
