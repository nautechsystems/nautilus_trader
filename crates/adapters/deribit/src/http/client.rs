// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

//! Deribit HTTP client implementation.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use ahash::AHashSet;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use nautilus_core::{datetime::nanos_to_millis, nanos::UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{
    data::{Bar, BarType, TradeTick},
    enums::{AggregationSource, BarAggregation},
    events::AccountState,
    identifiers::{AccountId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
};
use nautilus_network::{
    http::{HttpClient, Method},
    ratelimiter::quota::Quota,
    retry::{RetryConfig, RetryManager},
};
use serde::{Serialize, de::DeserializeOwned};
use strum::IntoEnumIterator;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{
    error::DeribitHttpError,
    models::{
        DeribitAccountSummariesResponse, DeribitCurrency, DeribitInstrument, DeribitJsonRpcRequest,
        DeribitJsonRpcResponse, DeribitPosition, DeribitUserTradesResponse,
    },
    query::{
        GetAccountSummariesParams, GetInstrumentParams, GetInstrumentsParams,
        GetOpenOrdersByInstrumentParams, GetOpenOrdersParams, GetOrderHistoryByCurrencyParams,
        GetOrderHistoryByInstrumentParams, GetOrderStateParams, GetPositionsParams,
        GetUserTradesByCurrencyAndTimeParams, GetUserTradesByInstrumentAndTimeParams,
    },
};
use crate::{
    common::{
        consts::{
            DERIBIT_ACCOUNT_RATE_KEY, DERIBIT_API_PATH, DERIBIT_GLOBAL_RATE_KEY,
            DERIBIT_HTTP_ACCOUNT_QUOTA, DERIBIT_HTTP_ORDER_QUOTA, DERIBIT_HTTP_REST_QUOTA,
            DERIBIT_ORDER_RATE_KEY, JSONRPC_VERSION, should_retry_error_code,
        },
        credential::Credential,
        parse::{
            extract_server_timestamp, parse_account_state, parse_bars,
            parse_deribit_instrument_any, parse_order_book, parse_trade_tick,
        },
        urls::get_http_base_url,
    },
    http::{
        models::{DeribitOrderBook, DeribitTradesResponse, DeribitTradingViewChartData},
        query::{
            GetLastTradesByInstrumentAndTimeParams, GetOrderBookParams,
            GetTradingViewChartDataParams,
        },
    },
    websocket::{
        messages::{DeribitOrderMsg, DeribitUserTradeMsg},
        parse::{parse_position_status_report, parse_user_order_msg, parse_user_trade_msg},
    },
};

/// Low-level Deribit HTTP client for raw API operations.
///
/// This client handles JSON-RPC 2.0 protocol, request signing, rate limiting,
/// and retry logic. It returns venue-specific response types.
#[derive(Debug)]
pub struct DeribitRawHttpClient {
    base_url: String,
    client: HttpClient,
    credential: Option<Credential>,
    retry_manager: RetryManager<DeribitHttpError>,
    cancellation_token: CancellationToken,
    request_id: AtomicU64,
}

impl DeribitRawHttpClient {
    /// Creates a new [`DeribitRawHttpClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        base_url: Option<String>,
        is_testnet: bool,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        proxy_url: Option<String>,
    ) -> Result<Self, DeribitHttpError> {
        let base_url = base_url
            .unwrap_or_else(|| format!("{}{}", get_http_base_url(is_testnet), DERIBIT_API_PATH));
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

        let retry_manager = RetryManager::new(retry_config);

        Ok(Self {
            base_url,
            client: HttpClient::new(
                HashMap::new(),
                Vec::new(),
                Self::rate_limiter_quotas(),
                Some(*DERIBIT_HTTP_REST_QUOTA),
                timeout_secs,
                proxy_url,
            )
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {e}"))?,
            credential: None,
            retry_manager,
            cancellation_token: CancellationToken::new(),
            request_id: AtomicU64::new(1),
        })
    }

    /// Get the cancellation token for this client.
    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.cancellation_token
    }

    /// Returns whether this client is connected to testnet.
    #[must_use]
    pub fn is_testnet(&self) -> bool {
        self.base_url.contains("test")
    }

    /// Returns the rate limiter quotas for the HTTP client.
    ///
    /// Quotas are organized by:
    /// - Global: Overall rate limit for all requests
    /// - Orders: Matching engine operations (buy, sell, cancel, etc.)
    /// - Account: Account information endpoints
    fn rate_limiter_quotas() -> Vec<(String, Quota)> {
        vec![
            (
                DERIBIT_GLOBAL_RATE_KEY.to_string(),
                *DERIBIT_HTTP_REST_QUOTA,
            ),
            (
                DERIBIT_ORDER_RATE_KEY.to_string(),
                *DERIBIT_HTTP_ORDER_QUOTA,
            ),
            (
                DERIBIT_ACCOUNT_RATE_KEY.to_string(),
                *DERIBIT_HTTP_ACCOUNT_QUOTA,
            ),
        ]
    }

    /// Returns rate limit keys for a given RPC method.
    ///
    /// Maps Deribit JSON-RPC methods to appropriate rate limit buckets.
    fn rate_limit_keys(method: &str) -> Vec<String> {
        let mut keys = vec![DERIBIT_GLOBAL_RATE_KEY.to_string()];

        // Categorize by method type
        if Self::is_order_method(method) {
            keys.push(DERIBIT_ORDER_RATE_KEY.to_string());
        } else if Self::is_account_method(method) {
            keys.push(DERIBIT_ACCOUNT_RATE_KEY.to_string());
        }

        // Add method-specific key
        keys.push(format!("deribit:{method}"));

        keys
    }

    /// Returns true if the method is an order operation (matching engine).
    fn is_order_method(method: &str) -> bool {
        matches!(
            method,
            "private/buy"
                | "private/sell"
                | "private/edit"
                | "private/cancel"
                | "private/cancel_all"
                | "private/cancel_all_by_currency"
                | "private/cancel_all_by_instrument"
                | "private/cancel_by_label"
                | "private/close_position"
        )
    }

    /// Returns true if the method accesses account information.
    fn is_account_method(method: &str) -> bool {
        matches!(
            method,
            "private/get_account_summaries"
                | "private/get_account_summary"
                | "private/get_positions"
                | "private/get_position"
                | "private/get_open_orders_by_currency"
                | "private/get_open_orders_by_instrument"
                | "private/get_order_state"
                | "private/get_user_trades_by_currency"
                | "private/get_user_trades_by_instrument"
        )
    }

    /// Creates a new [`DeribitRawHttpClient`] with explicit credentials.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    #[allow(clippy::too_many_arguments)]
    pub fn with_credentials(
        api_key: String,
        api_secret: String,
        base_url: Option<String>,
        is_testnet: bool,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        proxy_url: Option<String>,
    ) -> Result<Self, DeribitHttpError> {
        let base_url = base_url
            .unwrap_or_else(|| format!("{}{}", get_http_base_url(is_testnet), DERIBIT_API_PATH));
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

        let retry_manager = RetryManager::new(retry_config);
        let credential = Credential::new(api_key, api_secret);

        Ok(Self {
            base_url,
            client: HttpClient::new(
                HashMap::new(),
                Vec::new(),
                Self::rate_limiter_quotas(),
                Some(*DERIBIT_HTTP_REST_QUOTA),
                timeout_secs,
                proxy_url,
            )
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {e}"))?,
            credential: Some(credential),
            retry_manager,
            cancellation_token: CancellationToken::new(),
            request_id: AtomicU64::new(1),
        })
    }

    /// Creates a new [`DeribitRawHttpClient`] with credentials from environment variables.
    ///
    /// If `api_key` or `api_secret` are not provided, they will be loaded from environment:
    /// - Mainnet: `DERIBIT_API_KEY`, `DERIBIT_API_SECRET`
    /// - Testnet: `DERIBIT_TESTNET_API_KEY`, `DERIBIT_TESTNET_API_SECRET`
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The HTTP client cannot be created
    /// - Credentials are not provided and environment variables are not set
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_env(
        api_key: Option<String>,
        api_secret: Option<String>,
        is_testnet: bool,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        proxy_url: Option<String>,
    ) -> Result<Self, DeribitHttpError> {
        // Determine environment variable names based on environment
        let (key_env, secret_env) = if is_testnet {
            ("DERIBIT_TESTNET_API_KEY", "DERIBIT_TESTNET_API_SECRET")
        } else {
            ("DERIBIT_API_KEY", "DERIBIT_API_SECRET")
        };

        // Resolve credentials from explicit params or environment
        let api_key = nautilus_core::env::get_or_env_var_opt(api_key, key_env);
        let api_secret = nautilus_core::env::get_or_env_var_opt(api_secret, secret_env);

        // If credentials were resolved, create authenticated client
        if let (Some(key), Some(secret)) = (api_key, api_secret) {
            Self::with_credentials(
                key,
                secret,
                None,
                is_testnet,
                timeout_secs,
                max_retries,
                retry_delay_ms,
                retry_delay_max_ms,
                proxy_url,
            )
        } else {
            // No credentials - create unauthenticated client
            Self::new(
                None,
                is_testnet,
                timeout_secs,
                max_retries,
                retry_delay_ms,
                retry_delay_max_ms,
                proxy_url,
            )
        }
    }

    /// Sends a JSON-RPC 2.0 request to the Deribit API.
    async fn send_request<T, P>(
        &self,
        method: &str,
        params: P,
        authenticate: bool,
    ) -> Result<DeribitJsonRpcResponse<T>, DeribitHttpError>
    where
        T: DeserializeOwned,
        P: Serialize,
    {
        // Create operation identifier combining URL and RPC method
        let operation_id = format!("{}#{}", self.base_url, method);
        let params_clone = serde_json::to_value(&params)?;

        let operation = || {
            let method = method.to_string();
            let params_clone = params_clone.clone();

            async move {
                // Build JSON-RPC request
                let id = self.request_id.fetch_add(1, Ordering::SeqCst);
                let request = DeribitJsonRpcRequest {
                    jsonrpc: JSONRPC_VERSION,
                    id,
                    method: method.clone(),
                    params: params_clone.clone(),
                };

                let body = serde_json::to_vec(&request)?;

                // Build headers
                let mut headers = HashMap::new();
                headers.insert("Content-Type".to_string(), "application/json".to_string());

                // Add authentication headers if required
                if authenticate {
                    let credentials = self
                        .credential
                        .as_ref()
                        .ok_or(DeribitHttpError::MissingCredentials)?;
                    let auth_headers = credentials.sign_auth_headers("POST", "/api/v2", &body)?;
                    headers.extend(auth_headers);
                }

                let rate_limit_keys = Self::rate_limit_keys(&method);
                let resp = self
                    .client
                    .request(
                        Method::POST,
                        self.base_url.clone(),
                        None,
                        Some(headers),
                        Some(body),
                        None,
                        Some(rate_limit_keys),
                    )
                    .await
                    .map_err(|e| DeribitHttpError::NetworkError(e.to_string()))?;

                // Parse JSON-RPC response
                // Note: Deribit may return JSON-RPC errors with non-2xx HTTP status (e.g., 400)
                // Always try to parse as JSON-RPC first, then fall back to HTTP error handling

                // Try to parse as JSON first
                let json_value: serde_json::Value = match serde_json::from_slice(&resp.body) {
                    Ok(json) => json,
                    Err(_) => {
                        // Not valid JSON - treat as HTTP error
                        let error_body = String::from_utf8_lossy(&resp.body);
                        log::error!(
                            "Non-JSON response: method={method}, status={}, body={error_body}",
                            resp.status.as_u16()
                        );
                        return Err(DeribitHttpError::UnexpectedStatus {
                            status: resp.status.as_u16(),
                            body: error_body.to_string(),
                        });
                    }
                };

                // Try to parse as JSON-RPC response
                let json_rpc_response: DeribitJsonRpcResponse<T> =
                    serde_json::from_value(json_value.clone()).map_err(|e| {
                        log::error!(
                            "Failed to deserialize Deribit JSON-RPC response: method={method}, status={}, error={e}",
                            resp.status.as_u16()
                        );
                        log::debug!(
                            "Response JSON (first 2000 chars): {}",
                            &json_value
                                .to_string()
                                .chars()
                                .take(2000)
                                .collect::<String>()
                        );
                        DeribitHttpError::JsonError(e.to_string())
                    })?;

                // Check if it's a success or error result
                if json_rpc_response.result.is_some() {
                    Ok(json_rpc_response)
                } else if let Some(error) = &json_rpc_response.error {
                    // JSON-RPC error (may come with any HTTP status)
                    log::warn!(
                        "Deribit RPC error response: method={method}, http_status={}, error_code={}, error_message={}, error_data={:?}",
                        resp.status.as_u16(),
                        error.code,
                        error.message,
                        error.data
                    );

                    // Map JSON-RPC error to appropriate error variant
                    Err(DeribitHttpError::from_jsonrpc_error(
                        error.code,
                        error.message.clone(),
                        error.data.clone(),
                    ))
                } else {
                    log::error!(
                        "Response contains neither result nor error field: method={method}, status={}, request_id={:?}",
                        resp.status.as_u16(),
                        json_rpc_response.id
                    );
                    Err(DeribitHttpError::JsonError(
                        "Response contains neither result nor error".to_string(),
                    ))
                }
            }
        };

        // Retry strategy based on Deribit error responses and HTTP status codes:
        //
        // 1. Network errors: always retry (transient connection issues)
        // 2. HTTP 5xx/429: server errors and rate limiting should be retried
        // 3. Deribit-specific retryable error codes (defined in common::consts)
        //
        // Note: Deribit returns many permanent errors which should NOT be retried
        // (e.g., "invalid_credentials", "not_enough_funds", "order_not_found")
        let should_retry = |error: &DeribitHttpError| -> bool {
            match error {
                DeribitHttpError::NetworkError(_) => true,
                DeribitHttpError::UnexpectedStatus { status, .. } => {
                    *status >= 500 || *status == 429
                }
                DeribitHttpError::DeribitError { error_code, .. } => {
                    should_retry_error_code(*error_code)
                }
                _ => false,
            }
        };

        let create_error = |msg: String| -> DeribitHttpError {
            if msg == "canceled" {
                DeribitHttpError::Canceled("Adapter disconnecting or shutting down".to_string())
            } else {
                DeribitHttpError::NetworkError(msg)
            }
        };

        self.retry_manager
            .execute_with_retry_with_cancel(
                &operation_id,
                operation,
                should_retry,
                create_error,
                &self.cancellation_token,
            )
            .await
    }

    /// Gets available trading instruments.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_instruments(
        &self,
        params: GetInstrumentsParams,
    ) -> Result<DeribitJsonRpcResponse<Vec<DeribitInstrument>>, DeribitHttpError> {
        self.send_request("public/get_instruments", params, false)
            .await
    }

    /// Gets details for a specific trading instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_instrument(
        &self,
        params: GetInstrumentParams,
    ) -> Result<DeribitJsonRpcResponse<DeribitInstrument>, DeribitHttpError> {
        self.send_request("public/get_instrument", params, false)
            .await
    }

    /// Gets recent trades for an instrument within a time range.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_last_trades_by_instrument_and_time(
        &self,
        params: GetLastTradesByInstrumentAndTimeParams,
    ) -> Result<DeribitJsonRpcResponse<DeribitTradesResponse>, DeribitHttpError> {
        self.send_request(
            "public/get_last_trades_by_instrument_and_time",
            params,
            false,
        )
        .await
    }

    /// Gets TradingView chart data (OHLCV) for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_tradingview_chart_data(
        &self,
        params: GetTradingViewChartDataParams,
    ) -> Result<DeribitJsonRpcResponse<DeribitTradingViewChartData>, DeribitHttpError> {
        self.send_request("public/get_tradingview_chart_data", params, false)
            .await
    }

    /// Gets account summaries for all currencies.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing ([`DeribitHttpError::MissingCredentials`])
    /// - Authentication fails (invalid signature, expired timestamp)
    /// - The request fails or the response cannot be parsed
    pub async fn get_account_summaries(
        &self,
        params: GetAccountSummariesParams,
    ) -> Result<DeribitJsonRpcResponse<DeribitAccountSummariesResponse>, DeribitHttpError> {
        self.send_request("private/get_account_summaries", params, true)
            .await
    }

    /// Gets order book for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_order_book(
        &self,
        params: GetOrderBookParams,
    ) -> Result<DeribitJsonRpcResponse<DeribitOrderBook>, DeribitHttpError> {
        self.send_request("public/get_order_book", params, false)
            .await
    }

    /// Gets a single order by its ID.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing ([`DeribitHttpError::MissingCredentials`])
    /// - Authentication fails (invalid signature, expired timestamp)
    /// - The request fails or the response cannot be parsed
    pub async fn get_order_state(
        &self,
        params: GetOrderStateParams,
    ) -> Result<DeribitJsonRpcResponse<DeribitOrderMsg>, DeribitHttpError> {
        self.send_request("private/get_order_state", params, true)
            .await
    }

    /// Gets all open orders across all currencies and instruments.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing ([`DeribitHttpError::MissingCredentials`])
    /// - Authentication fails (invalid signature, expired timestamp)
    /// - The request fails or the response cannot be parsed
    pub async fn get_open_orders(
        &self,
        params: GetOpenOrdersParams,
    ) -> Result<DeribitJsonRpcResponse<Vec<DeribitOrderMsg>>, DeribitHttpError> {
        self.send_request("private/get_open_orders", params, true)
            .await
    }

    /// Gets open orders for a specific instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing ([`DeribitHttpError::MissingCredentials`])
    /// - Authentication fails (invalid signature, expired timestamp)
    /// - The request fails or the response cannot be parsed
    pub async fn get_open_orders_by_instrument(
        &self,
        params: GetOpenOrdersByInstrumentParams,
    ) -> Result<DeribitJsonRpcResponse<Vec<DeribitOrderMsg>>, DeribitHttpError> {
        self.send_request("private/get_open_orders_by_instrument", params, true)
            .await
    }

    /// Gets historical orders for a specific instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing ([`DeribitHttpError::MissingCredentials`])
    /// - Authentication fails (invalid signature, expired timestamp)
    /// - The request fails or the response cannot be parsed
    pub async fn get_order_history_by_instrument(
        &self,
        params: GetOrderHistoryByInstrumentParams,
    ) -> Result<DeribitJsonRpcResponse<Vec<DeribitOrderMsg>>, DeribitHttpError> {
        self.send_request("private/get_order_history_by_instrument", params, true)
            .await
    }

    /// Gets historical orders for a specific currency.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing ([`DeribitHttpError::MissingCredentials`])
    /// - Authentication fails (invalid signature, expired timestamp)
    /// - The request fails or the response cannot be parsed
    pub async fn get_order_history_by_currency(
        &self,
        params: GetOrderHistoryByCurrencyParams,
    ) -> Result<DeribitJsonRpcResponse<Vec<DeribitOrderMsg>>, DeribitHttpError> {
        self.send_request("private/get_order_history_by_currency", params, true)
            .await
    }

    /// Gets user trades for a specific instrument within a time range.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing ([`DeribitHttpError::MissingCredentials`])
    /// - Authentication fails (invalid signature, expired timestamp)
    /// - The request fails or the response cannot be parsed
    pub async fn get_user_trades_by_instrument_and_time(
        &self,
        params: GetUserTradesByInstrumentAndTimeParams,
    ) -> Result<DeribitJsonRpcResponse<DeribitUserTradesResponse>, DeribitHttpError> {
        self.send_request(
            "private/get_user_trades_by_instrument_and_time",
            params,
            true,
        )
        .await
    }

    /// Gets user trades for a specific currency within a time range.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing ([`DeribitHttpError::MissingCredentials`])
    /// - Authentication fails (invalid signature, expired timestamp)
    /// - The request fails or the response cannot be parsed
    pub async fn get_user_trades_by_currency_and_time(
        &self,
        params: GetUserTradesByCurrencyAndTimeParams,
    ) -> Result<DeribitJsonRpcResponse<DeribitUserTradesResponse>, DeribitHttpError> {
        self.send_request("private/get_user_trades_by_currency_and_time", params, true)
            .await
    }

    /// Gets positions for a specific currency.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing ([`DeribitHttpError::MissingCredentials`])
    /// - Authentication fails (invalid signature, expired timestamp)
    /// - The request fails or the response cannot be parsed
    pub async fn get_positions(
        &self,
        params: GetPositionsParams,
    ) -> Result<DeribitJsonRpcResponse<Vec<DeribitPosition>>, DeribitHttpError> {
        self.send_request("private/get_positions", params, true)
            .await
    }
}

/// High-level Deribit HTTP client with domain-level abstractions.
///
/// This client wraps the raw HTTP client and provides methods that use Nautilus
/// domain types. It maintains an instrument cache for efficient lookups.
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.deribit")
)]
pub struct DeribitHttpClient {
    pub(crate) inner: Arc<DeribitRawHttpClient>,
    pub(crate) instruments_cache: Arc<DashMap<Ustr, InstrumentAny>>,
    cache_initialized: AtomicBool,
}

impl Clone for DeribitHttpClient {
    fn clone(&self) -> Self {
        let cache_initialized = AtomicBool::new(false);

        let is_initialized = self.cache_initialized.load(Ordering::Acquire);
        if is_initialized {
            cache_initialized.store(true, Ordering::Release);
        }

        Self {
            inner: self.inner.clone(),
            instruments_cache: self.instruments_cache.clone(),
            cache_initialized,
        }
    }
}

impl DeribitHttpClient {
    /// Creates a new [`DeribitHttpClient`] with default configuration.
    ///
    /// # Parameters
    /// - `base_url`: Optional custom base URL (for testing)
    /// - `is_testnet`: Whether to use the testnet environment
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        base_url: Option<String>,
        is_testnet: bool,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        proxy_url: Option<String>,
    ) -> anyhow::Result<Self> {
        let raw_client = Arc::new(DeribitRawHttpClient::new(
            base_url,
            is_testnet,
            timeout_secs,
            max_retries,
            retry_delay_ms,
            retry_delay_max_ms,
            proxy_url,
        )?);

        Ok(Self {
            inner: raw_client,
            instruments_cache: Arc::new(DashMap::new()),
            cache_initialized: AtomicBool::new(false),
        })
    }

    /// Creates a new [`DeribitHttpClient`] with credentials from environment variables.
    ///
    /// If `api_key` or `api_secret` are not provided, they will be loaded from environment:
    /// - Mainnet: `DERIBIT_API_KEY`, `DERIBIT_API_SECRET`
    /// - Testnet: `DERIBIT_TESTNET_API_KEY`, `DERIBIT_TESTNET_API_SECRET`
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The HTTP client cannot be created
    /// - Credentials are not provided and environment variables are not set
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_env(
        api_key: Option<String>,
        api_secret: Option<String>,
        is_testnet: bool,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        proxy_url: Option<String>,
    ) -> anyhow::Result<Self> {
        let raw_client = Arc::new(DeribitRawHttpClient::new_with_env(
            api_key,
            api_secret,
            is_testnet,
            timeout_secs,
            max_retries,
            retry_delay_ms,
            retry_delay_max_ms,
            proxy_url,
        )?);

        Ok(Self {
            inner: raw_client,
            instruments_cache: Arc::new(DashMap::new()),
            cache_initialized: AtomicBool::new(false),
        })
    }

    /// Requests instruments for a specific currency.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or instruments cannot be parsed.
    pub async fn request_instruments(
        &self,
        currency: DeribitCurrency,
        kind: Option<super::models::DeribitInstrumentKind>,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        // Build parameters
        let params = if let Some(k) = kind {
            GetInstrumentsParams::with_kind(currency, k)
        } else {
            GetInstrumentsParams::new(currency)
        };

        // Call raw client
        let full_response = self.inner.get_instruments(params).await?;
        let result = full_response
            .result
            .ok_or_else(|| anyhow::anyhow!("No result in response"))?;
        let ts_event = extract_server_timestamp(full_response.us_out)?;
        let ts_init = self.generate_ts_init();

        // Parse each instrument
        let mut instruments = Vec::new();
        let mut skipped_count = 0;
        let mut error_count = 0;

        for raw_instrument in result {
            match parse_deribit_instrument_any(&raw_instrument, ts_init, ts_event) {
                Ok(Some(instrument)) => {
                    instruments.push(instrument);
                }
                Ok(None) => {
                    // Unsupported instrument type (e.g., combos)
                    skipped_count += 1;
                    log::debug!(
                        "Skipped unsupported instrument type: {} (kind: {:?})",
                        raw_instrument.instrument_name,
                        raw_instrument.kind
                    );
                }
                Err(e) => {
                    error_count += 1;
                    log::warn!(
                        "Failed to parse instrument {}: {}",
                        raw_instrument.instrument_name,
                        e
                    );
                }
            }
        }

        log::info!(
            "Parsed {} instruments ({} skipped, {} errors)",
            instruments.len(),
            skipped_count,
            error_count
        );

        Ok(instruments)
    }

    /// Requests a specific instrument by its Nautilus instrument ID.
    ///
    /// This is a high-level method that fetches the raw instrument data from Deribit
    /// and converts it to a Nautilus `InstrumentAny` type.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The instrument name format is invalid (error code `-32602`)
    /// - The instrument doesn't exist (error code `13020`)
    /// - Network or API errors occur
    pub async fn request_instrument(
        &self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<InstrumentAny> {
        let params = GetInstrumentParams {
            instrument_name: instrument_id.symbol.to_string(),
        };

        let full_response = self.inner.get_instrument(params).await?;
        let response = full_response
            .result
            .ok_or_else(|| anyhow::anyhow!("No result in response"))?;
        let ts_event = extract_server_timestamp(full_response.us_out)?;
        let ts_init = self.generate_ts_init();

        match parse_deribit_instrument_any(&response, ts_init, ts_event)? {
            Some(instrument) => Ok(instrument),
            None => anyhow::bail!(
                "Unsupported instrument type: {} (kind: {:?})",
                response.instrument_name,
                response.kind
            ),
        }
    }

    /// Requests historical trades for an instrument within a time range.
    ///
    /// Fetches trade ticks from Deribit and converts them to Nautilus [`TradeTick`] objects.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument to fetch trades for
    /// * `start` - Optional start time filter
    /// * `end` - Optional end time filter
    /// * `limit` - Optional limit on number of trades (max 1000)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The request fails
    /// - Trade parsing fails
    pub async fn request_trades(
        &self,
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<TradeTick>> {
        // Get instrument from cache to determine precisions
        let (price_precision, size_precision) =
            if let Some(instrument) = self.get_instrument(&instrument_id.symbol.inner()) {
                (instrument.price_precision(), instrument.size_precision())
            } else {
                log::warn!("Instrument {instrument_id} not in cache, skipping trades request");
                anyhow::bail!("Instrument {instrument_id} not in cache");
            };

        // Convert timestamps to milliseconds
        let now = Utc::now();
        let end_dt = end.unwrap_or(now);
        let start_dt = start.unwrap_or(end_dt - chrono::Duration::hours(1));

        if let (Some(s), Some(e)) = (start, end) {
            anyhow::ensure!(s < e, "Invalid time range: start={s:?} end={e:?}");
        }

        let start_timestamp = start_dt.timestamp_millis();
        let end_timestamp = end_dt.timestamp_millis();

        let params = GetLastTradesByInstrumentAndTimeParams::new(
            instrument_id.symbol.to_string(),
            start_timestamp,
            end_timestamp,
            limit,
            Some("asc".to_string()), // Sort ascending for historical data
        );

        let full_response = self
            .inner
            .get_last_trades_by_instrument_and_time(params)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let response_data = full_response
            .result
            .ok_or_else(|| anyhow::anyhow!("No result in response"))?;

        let ts_init = self.generate_ts_init();
        let mut trades = Vec::with_capacity(response_data.trades.len());

        for raw_trade in &response_data.trades {
            match parse_trade_tick(
                raw_trade,
                instrument_id,
                price_precision,
                size_precision,
                ts_init,
            ) {
                Ok(trade) => trades.push(trade),
                Err(e) => {
                    log::warn!(
                        "Failed to parse trade {} for {}: {}",
                        raw_trade.trade_id,
                        instrument_id,
                        e
                    );
                }
            }
        }

        Ok(trades)
    }

    /// Requests historical bars (OHLCV) for an instrument.
    ///
    /// Uses the `public/get_tradingview_chart_data` endpoint to fetch candlestick data.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Aggregation source is not EXTERNAL
    /// - Bar aggregation type is not supported by Deribit
    /// - The request fails or response cannot be parsed
    ///
    /// # Supported Resolutions
    ///
    /// Deribit supports: 1, 3, 5, 10, 15, 30, 60, 120, 180, 360, 720 minutes, and 1D (daily)
    pub async fn request_bars(
        &self,
        bar_type: BarType,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        _limit: Option<u32>,
    ) -> anyhow::Result<Vec<Bar>> {
        anyhow::ensure!(
            bar_type.aggregation_source() == AggregationSource::External,
            "Only EXTERNAL aggregation is supported"
        );

        let now = Utc::now();

        // Default to last hour if no start/end provided
        let end_dt = end.unwrap_or(now);
        let start_dt = start.unwrap_or(end_dt - chrono::Duration::hours(1));

        if let (Some(s), Some(e)) = (start, end) {
            anyhow::ensure!(s < e, "Invalid time range: start={s:?} end={e:?}");
        }

        // Convert BarType to Deribit resolution
        let spec = bar_type.spec();
        let step = spec.step.get();
        let resolution = match spec.aggregation {
            BarAggregation::Minute => format!("{step}"),
            BarAggregation::Hour => format!("{}", step * 60),
            BarAggregation::Day => "1D".to_string(),
            a => anyhow::bail!("Deribit does not support {a:?} aggregation"),
        };

        // Validate resolution is supported by Deribit
        let supported_resolutions = [
            "1", "3", "5", "10", "15", "30", "60", "120", "180", "360", "720", "1D",
        ];
        if !supported_resolutions.contains(&resolution.as_str()) {
            anyhow::bail!(
                "Deribit does not support resolution '{resolution}'. Supported: {supported_resolutions:?}"
            );
        }

        let instrument_name = bar_type.instrument_id().symbol.to_string();
        let start_timestamp = start_dt.timestamp_millis();
        let end_timestamp = end_dt.timestamp_millis();

        let params = GetTradingViewChartDataParams::new(
            instrument_name,
            start_timestamp,
            end_timestamp,
            resolution,
        );

        let full_response = self.inner.get_tradingview_chart_data(params).await?;
        let chart_data = full_response
            .result
            .ok_or_else(|| anyhow::anyhow!("No result in response"))?;

        if chart_data.status == "no_data" {
            log::debug!("No bar data returned for {bar_type}");
            return Ok(Vec::new());
        }

        // Get instrument from cache to determine precisions
        let instrument_id = bar_type.instrument_id();
        let (price_precision, size_precision) =
            if let Some(instrument) = self.get_instrument(&instrument_id.symbol.inner()) {
                (instrument.price_precision(), instrument.size_precision())
            } else {
                log::warn!("Instrument {instrument_id} not in cache, skipping bars request");
                anyhow::bail!("Instrument {instrument_id} not in cache");
            };

        let ts_init = self.generate_ts_init();
        let bars = parse_bars(
            &chart_data,
            bar_type,
            price_precision,
            size_precision,
            ts_init,
        )?;

        log::info!("Parsed {} bars for {}", bars.len(), bar_type);

        Ok(bars)
    }

    /// Requests a snapshot of the order book for an instrument.
    ///
    /// Fetches the order book from Deribit and converts it to a Nautilus [`OrderBook`].
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument to fetch the order book for
    /// * `depth` - Optional depth limit (valid values: 1, 5, 10, 20, 50, 100, 1000, 10000)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The request fails
    /// - Order book parsing fails
    pub async fn request_book_snapshot(
        &self,
        instrument_id: InstrumentId,
        depth: Option<u32>,
    ) -> anyhow::Result<OrderBook> {
        // Get instrument from cache to determine precisions
        let (price_precision, size_precision) =
            if let Some(instrument) = self.get_instrument(&instrument_id.symbol.inner()) {
                (instrument.price_precision(), instrument.size_precision())
            } else {
                // Default precisions if instrument not cached
                log::warn!("Instrument {instrument_id} not in cache, using default precisions");
                (8u8, 8u8)
            };

        let params = GetOrderBookParams::new(instrument_id.symbol.to_string(), depth);
        let full_response = self
            .inner
            .get_order_book(params)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let order_book_data = full_response
            .result
            .ok_or_else(|| anyhow::anyhow!("No result in response"))?;

        let ts_init = self.generate_ts_init();
        let book = parse_order_book(
            &order_book_data,
            instrument_id,
            price_precision,
            size_precision,
            ts_init,
        )?;

        log::info!(
            "Fetched order book for {} with {} bids and {} asks",
            instrument_id,
            order_book_data.bids.len(),
            order_book_data.asks.len()
        );

        Ok(book)
    }

    /// Requests account state for all currencies.
    ///
    /// Fetches account balance and margin information for all currencies from Deribit
    /// and converts it to Nautilus [`AccountState`] event.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The request fails
    /// - Currency conversion fails
    pub async fn request_account_state(
        &self,
        account_id: AccountId,
    ) -> anyhow::Result<AccountState> {
        let params = GetAccountSummariesParams::default();
        let full_response = self
            .inner
            .get_account_summaries(params)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        let response_data = full_response
            .result
            .ok_or_else(|| anyhow::anyhow!("No result in response"))?;
        let ts_init = self.generate_ts_init();
        let ts_event = extract_server_timestamp(full_response.us_out)?;

        parse_account_state(&response_data.summaries, account_id, ts_init, ts_event)
    }

    /// Generates a timestamp for initialization.
    fn generate_ts_init(&self) -> UnixNanos {
        get_atomic_clock_realtime().get_time_ns()
    }

    /// Caches instruments for later retrieval.
    pub fn cache_instruments(&self, instruments: Vec<InstrumentAny>) {
        for inst in instruments {
            self.instruments_cache
                .insert(inst.raw_symbol().inner(), inst);
        }
        self.cache_initialized.store(true, Ordering::Release);
    }

    /// Retrieves a cached instrument by symbol.
    #[must_use]
    pub fn get_instrument(&self, symbol: &Ustr) -> Option<InstrumentAny> {
        self.instruments_cache
            .get(symbol)
            .map(|entry| entry.value().clone())
    }

    /// Checks if the instrument cache has been initialized.
    #[must_use]
    pub fn is_cache_initialized(&self) -> bool {
        self.cache_initialized.load(Ordering::Acquire)
    }

    /// Returns whether this client is connected to testnet.
    #[must_use]
    pub fn is_testnet(&self) -> bool {
        self.inner.is_testnet()
    }

    /// Requests order status reports for reconciliation.
    ///
    /// Fetches order statuses from Deribit and converts them to Nautilus [`OrderStatusReport`].
    ///
    /// # Strategy
    /// - Uses `/private/get_open_orders` for all open orders (single efficient API call)
    /// - Uses `/private/get_open_orders_by_instrument` when specific instrument is provided
    /// - For historical orders (when `open_only=false`), iterates over currencies
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or parsing fails.
    pub async fn request_order_status_reports(
        &self,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        open_only: bool,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let ts_init = self.generate_ts_init();
        let mut reports = Vec::new();
        let mut seen_order_ids = AHashSet::new();

        let mut parse_and_add = |order: &DeribitOrderMsg| {
            let symbol = Ustr::from(&order.instrument_name);
            if let Some(instrument) = self.get_instrument(&symbol) {
                match parse_user_order_msg(order, &instrument, account_id, ts_init) {
                    Ok(report) => {
                        // Apply time range filter based on ts_last
                        let ts_last = report.ts_last;
                        let in_range = match (start, end) {
                            (Some(s), Some(e)) => ts_last >= s && ts_last <= e,
                            (Some(s), None) => ts_last >= s,
                            (None, Some(e)) => ts_last <= e,
                            (None, None) => true,
                        };
                        // Only deduplicate if in range (prevents dropping valid historical reports)
                        if in_range && seen_order_ids.insert(order.order_id.clone()) {
                            reports.push(report);
                        }
                    }
                    Err(e) => {
                        log::warn!(
                            "Failed to parse order {} for {}: {}",
                            order.order_id,
                            order.instrument_name,
                            e
                        );
                    }
                }
            } else {
                log::debug!(
                    "Skipping order {} - instrument {} not in cache",
                    order.order_id,
                    order.instrument_name
                );
            }
        };

        if let Some(instrument_id) = instrument_id {
            // Use instrument-specific endpoint (efficient)
            let instrument_name = instrument_id.symbol.to_string();

            // Get open orders for this instrument
            let open_params = GetOpenOrdersByInstrumentParams {
                instrument_name: instrument_name.clone(),
                r#type: None,
            };
            if let Some(orders) = self
                .inner
                .get_open_orders_by_instrument(open_params)
                .await?
                .result
            {
                for order in &orders {
                    parse_and_add(order);
                }
            }

            // Get historical orders if not open_only
            if !open_only {
                let history_params = GetOrderHistoryByInstrumentParams {
                    instrument_name,
                    count: Some(100),
                    offset: None,
                    include_old: Some(true),
                    include_unfilled: Some(true),
                };
                if let Some(orders) = self
                    .inner
                    .get_order_history_by_instrument(history_params)
                    .await?
                    .result
                {
                    for order in &orders {
                        parse_and_add(order);
                    }
                }
            }
        } else {
            // Use get_open_orders for ALL open orders - single API call!
            let open_params = GetOpenOrdersParams::default();
            if let Some(orders) = self.inner.get_open_orders(open_params).await?.result {
                for order in &orders {
                    parse_and_add(order);
                }
            }

            // For historical orders, iterate currencies (ANY may not be supported)
            if !open_only {
                for currency in DeribitCurrency::iter().filter(|c| *c != DeribitCurrency::ANY) {
                    let history_params = GetOrderHistoryByCurrencyParams {
                        currency,
                        kind: None,
                        count: Some(100),
                        include_unfilled: Some(true),
                    };
                    if let Some(orders) = self
                        .inner
                        .get_order_history_by_currency(history_params)
                        .await?
                        .result
                    {
                        for order in &orders {
                            parse_and_add(order);
                        }
                    }
                }
            }
        }

        log::debug!("Generated {} order status reports", reports.len());
        Ok(reports)
    }

    /// Requests fill reports for reconciliation.
    ///
    /// Fetches user trades from Deribit and converts them to Nautilus [`FillReport`].
    ///
    /// # Strategy
    /// - Uses `/private/get_user_trades_by_instrument_and_time` when instrument is provided
    /// - Otherwise iterates over currencies using `/private/get_user_trades_by_currency_and_time`
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or parsing fails.
    pub async fn request_fill_reports(
        &self,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<Vec<FillReport>> {
        let ts_init = self.generate_ts_init();
        let now_ms = Utc::now().timestamp_millis();

        // Convert UnixNanos to milliseconds for Deribit API
        let start_ms = start.map_or(0, |ns| nanos_to_millis(ns.as_u64()) as i64);
        let end_ms = end.map_or(now_ms, |ns| nanos_to_millis(ns.as_u64()) as i64);
        let mut reports = Vec::new();

        // Helper closure to parse trade and add to reports
        let mut parse_and_add = |trade: &DeribitUserTradeMsg| {
            let symbol = Ustr::from(&trade.instrument_name);
            if let Some(instrument) = self.get_instrument(&symbol) {
                match parse_user_trade_msg(trade, &instrument, account_id, ts_init) {
                    Ok(report) => reports.push(report),
                    Err(e) => {
                        log::warn!(
                            "Failed to parse trade {} for {}: {}",
                            trade.trade_id,
                            trade.instrument_name,
                            e
                        );
                    }
                }
            } else {
                log::debug!(
                    "Skipping trade {} - instrument {} not in cache",
                    trade.trade_id,
                    trade.instrument_name
                );
            }
        };

        if let Some(instrument_id) = instrument_id {
            // Use instrument-specific endpoint (1 API call)
            let params = GetUserTradesByInstrumentAndTimeParams {
                instrument_name: instrument_id.symbol.to_string(),
                start_timestamp: start_ms,
                end_timestamp: end_ms,
                count: Some(1000),
                sorting: None,
            };
            if let Some(response) = self
                .inner
                .get_user_trades_by_instrument_and_time(params)
                .await?
                .result
            {
                for trade in &response.trades {
                    parse_and_add(trade);
                }
            }
        } else {
            // Iterate currencies (ANY not supported for user trades endpoint)
            for currency in DeribitCurrency::iter().filter(|c| *c != DeribitCurrency::ANY) {
                let params = GetUserTradesByCurrencyAndTimeParams {
                    currency,
                    start_timestamp: start_ms,
                    end_timestamp: end_ms,
                    kind: None,
                    count: Some(1000),
                };
                if let Some(response) = self
                    .inner
                    .get_user_trades_by_currency_and_time(params)
                    .await?
                    .result
                {
                    for trade in &response.trades {
                        parse_and_add(trade);
                    }
                }
            }
        }

        log::debug!("Generated {} fill reports", reports.len());
        Ok(reports)
    }

    /// Requests position status reports for reconciliation.
    ///
    /// Fetches positions from Deribit and converts them to Nautilus [`PositionStatusReport`].
    ///
    /// # Strategy
    /// - Uses `currency=any` to fetch all positions in one call
    /// - Filters by instrument_id if provided
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or parsing fails.
    pub async fn request_position_status_reports(
        &self,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let ts_init = self.generate_ts_init();
        let mut reports = Vec::new();

        // Use ANY to get all positions across all currencies in one call
        let params = GetPositionsParams {
            currency: DeribitCurrency::ANY,
            kind: None,
        };
        if let Some(positions) = self.inner.get_positions(params).await?.result {
            for position in &positions {
                // Skip flat positions (size == 0)
                if position.size.is_zero() {
                    continue;
                }

                let symbol = Ustr::from(position.instrument_name.as_str());
                if let Some(instrument) = self.get_instrument(&symbol) {
                    let report =
                        parse_position_status_report(position, &instrument, account_id, ts_init);
                    reports.push(report);
                } else {
                    log::debug!(
                        "Skipping position - instrument {} not in cache",
                        position.instrument_name
                    );
                }
            }
        }

        // Filter by instrument if provided
        if let Some(instrument_id) = instrument_id {
            reports.retain(|r| r.instrument_id == instrument_id);
        }

        log::debug!("Generated {} position status reports", reports.len());
        Ok(reports)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::consts::{
        DERIBIT_ACCOUNT_RATE_KEY, DERIBIT_GLOBAL_RATE_KEY, DERIBIT_ORDER_RATE_KEY,
    };

    #[rstest]
    #[case("private/buy", true, false)]
    #[case("private/cancel", true, false)]
    #[case("private/get_account_summaries", false, true)]
    #[case("private/get_positions", false, true)]
    #[case("public/get_instruments", false, false)]
    fn test_method_classification(
        #[case] method: &str,
        #[case] is_order: bool,
        #[case] is_account: bool,
    ) {
        assert_eq!(DeribitRawHttpClient::is_order_method(method), is_order);
        assert_eq!(DeribitRawHttpClient::is_account_method(method), is_account);
    }

    #[rstest]
    #[case("private/buy", vec![DERIBIT_GLOBAL_RATE_KEY, DERIBIT_ORDER_RATE_KEY])]
    #[case("private/get_account_summaries", vec![DERIBIT_GLOBAL_RATE_KEY, DERIBIT_ACCOUNT_RATE_KEY])]
    #[case("public/get_instruments", vec![DERIBIT_GLOBAL_RATE_KEY])]
    fn test_rate_limit_keys(#[case] method: &str, #[case] expected_keys: Vec<&str>) {
        let keys = DeribitRawHttpClient::rate_limit_keys(method);

        for key in &expected_keys {
            assert!(keys.contains(&key.to_string()));
        }
        assert!(keys.contains(&format!("deribit:{method}")));
    }
}
