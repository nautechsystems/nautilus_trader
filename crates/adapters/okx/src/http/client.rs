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

//! Provides an ergonomic wrapper around the **OKX v5 REST API** –
//! <https://www.okx.com/docs-v5/en/>.
//!
//! The core type exported by this module is [`OKXHttpClient`].  It offers an
//! interface to all exchange endpoints currently required by NautilusTrader.
//!
//! Key responsibilities handled internally:
//! • Request signing and header composition for private routes (HMAC-SHA256).
//! • Rate-limiting based on the public OKX specification.
//! • Zero-copy deserialization of large JSON payloads into domain models.
//! • Conversion of raw exchange errors into the rich [`OKXHttpError`] enum.
//!
//! # Quick links to official docs
//! | Domain                               | OKX reference                                          |
//! |--------------------------------------|--------------------------------------------------------|
//! | Market data                          | <https://www.okx.com/docs-v5/en/#rest-api-market-data> |
//! | Account & positions                  | <https://www.okx.com/docs-v5/en/#rest-api-account>     |
//! | Funding & asset balances             | <https://www.okx.com/docs-v5/en/#rest-api-funding>     |

use std::{
    collections::HashMap,
    fmt::Debug,
    num::NonZeroU32,
    str::FromStr,
    sync::{Arc, LazyLock, Mutex},
};

use ahash::{AHashMap, AHashSet};
use chrono::{DateTime, Utc};
use nautilus_core::{
    MUTEX_POISONED, UnixNanos, consts::NAUTILUS_USER_AGENT, env::get_or_env_var,
    time::get_atomic_clock_realtime,
};
use nautilus_model::{
    data::{Bar, BarType, IndexPriceUpdate, MarkPriceUpdate, TradeTick},
    enums::{AggregationSource, BarAggregation, OrderSide, OrderType, TriggerType},
    events::AccountState,
    identifiers::{AccountId, ClientOrderId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{Price, Quantity},
};
use nautilus_network::{
    http::HttpClient,
    ratelimiter::quota::Quota,
    retry::{RetryConfig, RetryManager},
};
use reqwest::{Method, StatusCode, header::USER_AGENT};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{
    error::OKXHttpError,
    models::{
        OKXAccount, OKXCancelAlgoOrderRequest, OKXCancelAlgoOrderResponse, OKXFeeRate,
        OKXIndexTicker, OKXMarkPrice, OKXOrderAlgo, OKXOrderHistory, OKXPlaceAlgoOrderRequest,
        OKXPlaceAlgoOrderResponse, OKXPosition, OKXPositionHistory, OKXPositionTier, OKXServerTime,
        OKXTransactionDetail,
    },
    query::{
        GetAlgoOrdersParams, GetAlgoOrdersParamsBuilder, GetCandlesticksParams,
        GetCandlesticksParamsBuilder, GetIndexTickerParams, GetIndexTickerParamsBuilder,
        GetInstrumentsParams, GetInstrumentsParamsBuilder, GetMarkPriceParams,
        GetMarkPriceParamsBuilder, GetOrderHistoryParams, GetOrderHistoryParamsBuilder,
        GetOrderListParams, GetOrderListParamsBuilder, GetPositionTiersParams,
        GetPositionsHistoryParams, GetPositionsParams, GetPositionsParamsBuilder,
        GetTradeFeeParams, GetTradesParams, GetTradesParamsBuilder, GetTransactionDetailsParams,
        GetTransactionDetailsParamsBuilder, SetPositionModeParams, SetPositionModeParamsBuilder,
    },
};
use crate::{
    common::{
        consts::{OKX_HTTP_URL, OKX_NAUTILUS_BROKER_ID, should_retry_error_code},
        credential::Credential,
        enums::{
            OKXAlgoOrderType, OKXInstrumentType, OKXOrderStatus, OKXPositionMode, OKXSide,
            OKXTradeMode, OKXTriggerType,
        },
        models::OKXInstrument,
        parse::{
            okx_instrument_type, parse_account_state, parse_candlestick, parse_fill_report,
            parse_index_price_update, parse_instrument_any, parse_mark_price_update,
            parse_order_status_report, parse_position_status_report, parse_trade_tick,
        },
    },
    http::{
        models::{OKXCandlestick, OKXTrade},
        query::{GetOrderParams, GetPendingOrdersParams},
    },
    websocket::{messages::OKXAlgoOrderMsg, parse::parse_algo_order_status_report},
};

const OKX_SUCCESS_CODE: &str = "0";

/// Default OKX REST API rate limit: 500 requests per 2 seconds.
///
/// - Sub-account order limit: 1000 requests per 2 seconds.
/// - Account balance: 10 requests per 2 seconds.
/// - Account instruments: 20 requests per 2 seconds.
///
/// We use a conservative 250 requests per second (500 per 2 seconds) as a general limit
/// that should accommodate most use cases while respecting OKX's documented limits.
pub static OKX_REST_QUOTA: LazyLock<Quota> =
    LazyLock::new(|| Quota::per_second(NonZeroU32::new(250).unwrap()));

const OKX_GLOBAL_RATE_KEY: &str = "okx:global";

/// Represents an OKX HTTP response.
#[derive(Debug, Serialize, Deserialize)]
pub struct OKXResponse<T> {
    /// The OKX response code, which is `"0"` for success.
    pub code: String,
    /// A message string which can be informational or describe an error cause.
    pub msg: String,
    /// The typed data returned by the OKX endpoint.
    pub data: Vec<T>,
}

/// Provides a HTTP client for connecting to the [OKX](https://okx.com) REST API.
///
/// This client wraps the underlying [`HttpClient`] to handle functionality
/// specific to OKX, such as request signing (for authenticated endpoints),
/// forming request URLs, and deserializing responses into specific data models.
pub struct OKXHttpInnerClient {
    base_url: String,
    client: HttpClient,
    credential: Option<Credential>,
    retry_manager: RetryManager<OKXHttpError>,
    cancellation_token: CancellationToken,
    is_demo: bool,
}

impl Default for OKXHttpInnerClient {
    fn default() -> Self {
        Self::new(None, Some(60), None, None, None, false)
            .expect("Failed to create default OKXHttpInnerClient")
    }
}

impl Debug for OKXHttpInnerClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let credential = self.credential.as_ref().map(|_| "<redacted>");
        f.debug_struct(stringify!(OKXHttpInnerClient))
            .field("base_url", &self.base_url)
            .field("credential", &credential)
            .finish_non_exhaustive()
    }
}

impl OKXHttpInnerClient {
    fn rate_limiter_quotas() -> Vec<(String, Quota)> {
        vec![
            (OKX_GLOBAL_RATE_KEY.to_string(), *OKX_REST_QUOTA),
            (
                "okx:/api/v5/account/balance".to_string(),
                Quota::per_second(NonZeroU32::new(5).unwrap()),
            ),
            (
                "okx:/api/v5/public/instruments".to_string(),
                Quota::per_second(NonZeroU32::new(10).unwrap()),
            ),
            (
                "okx:/api/v5/market/candles".to_string(),
                Quota::per_second(NonZeroU32::new(50).unwrap()),
            ),
            (
                "okx:/api/v5/market/history-candles".to_string(),
                Quota::per_second(NonZeroU32::new(20).unwrap()),
            ),
            (
                "okx:/api/v5/market/history-trades".to_string(),
                Quota::per_second(NonZeroU32::new(30).unwrap()),
            ),
            (
                "okx:/api/v5/trade/order".to_string(),
                Quota::per_second(NonZeroU32::new(30).unwrap()), // 60 requests / 2 seconds (per instrument)
            ),
            (
                "okx:/api/v5/trade/orders-pending".to_string(),
                Quota::per_second(NonZeroU32::new(20).unwrap()),
            ),
            (
                "okx:/api/v5/trade/orders-history".to_string(),
                Quota::per_second(NonZeroU32::new(20).unwrap()),
            ),
            (
                "okx:/api/v5/trade/fills".to_string(),
                Quota::per_second(NonZeroU32::new(30).unwrap()),
            ),
            (
                "okx:/api/v5/trade/order-algo".to_string(),
                Quota::per_second(NonZeroU32::new(10).unwrap()),
            ),
            (
                "okx:/api/v5/trade/cancel-algos".to_string(),
                Quota::per_second(NonZeroU32::new(10).unwrap()),
            ),
        ]
    }

    fn rate_limit_keys(endpoint: &str) -> Vec<Ustr> {
        let normalized = endpoint.split('?').next().unwrap_or(endpoint);
        let route = format!("okx:{normalized}");

        vec![Ustr::from(OKX_GLOBAL_RATE_KEY), Ustr::from(route.as_str())]
    }

    /// Cancel all pending HTTP requests.
    pub fn cancel_all_requests(&self) {
        self.cancellation_token.cancel();
    }

    /// Get the cancellation token for this client.
    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.cancellation_token
    }

    /// Creates a new [`OKXHttpClient`] using the default OKX HTTP URL,
    /// optionally overridden with a custom base URL.
    ///
    /// This version of the client has **no credentials**, so it can only
    /// call publicly accessible endpoints.
    ///
    /// # Errors
    ///
    /// Returns an error if the retry manager cannot be created.
    pub fn new(
        base_url: Option<String>,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        is_demo: bool,
    ) -> Result<Self, OKXHttpError> {
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
            OKXHttpError::ValidationError(format!("Failed to create retry manager: {e}"))
        })?;

        Ok(Self {
            base_url: base_url.unwrap_or(OKX_HTTP_URL.to_string()),
            client: HttpClient::new(
                Self::default_headers(is_demo),
                vec![],
                Self::rate_limiter_quotas(),
                Some(*OKX_REST_QUOTA),
                timeout_secs,
            ),
            credential: None,
            retry_manager,
            cancellation_token: CancellationToken::new(),
            is_demo,
        })
    }

    /// Creates a new [`OKXHttpClient`] configured with credentials
    /// for authenticated requests, optionally using a custom base URL.
    ///
    /// # Errors
    ///
    /// Returns an error if the retry manager cannot be created.
    #[allow(clippy::too_many_arguments)]
    pub fn with_credentials(
        api_key: String,
        api_secret: String,
        api_passphrase: String,
        base_url: String,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        is_demo: bool,
    ) -> Result<Self, OKXHttpError> {
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
            OKXHttpError::ValidationError(format!("Failed to create retry manager: {e}"))
        })?;

        Ok(Self {
            base_url,
            client: HttpClient::new(
                Self::default_headers(is_demo),
                vec![],
                Self::rate_limiter_quotas(),
                Some(*OKX_REST_QUOTA),
                timeout_secs,
            ),
            credential: Some(Credential::new(api_key, api_secret, api_passphrase)),
            retry_manager,
            cancellation_token: CancellationToken::new(),
            is_demo,
        })
    }

    /// Builds the default headers to include with each request (e.g., `User-Agent`).
    fn default_headers(is_demo: bool) -> HashMap<String, String> {
        let mut headers =
            HashMap::from([(USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string())]);

        if is_demo {
            headers.insert("x-simulated-trading".to_string(), "1".to_string());
        }

        headers
    }

    /// Combine a base path with a `serde_urlencoded` query string if one exists.
    ///
    /// # Errors
    ///
    /// Returns an error if the query string serialization fails.
    fn build_path<S: Serialize>(base: &str, params: &S) -> Result<String, OKXHttpError> {
        let query = serde_urlencoded::to_string(params)
            .map_err(|e| OKXHttpError::JsonError(e.to_string()))?;
        if query.is_empty() {
            Ok(base.to_owned())
        } else {
            Ok(format!("{base}?{query}"))
        }
    }

    /// Signs an OKX request with timestamp, API key, passphrase, and signature.
    ///
    /// # Errors
    ///
    /// Returns [`OKXHttpError::MissingCredentials`] if no credentials are set
    /// but the request requires authentication.
    fn sign_request(
        &self,
        method: &Method,
        path: &str,
        body: Option<&[u8]>,
    ) -> Result<HashMap<String, String>, OKXHttpError> {
        let credential = match self.credential.as_ref() {
            Some(c) => c,
            None => return Err(OKXHttpError::MissingCredentials),
        };

        let api_key = credential.api_key.to_string();
        let api_passphrase = credential.api_passphrase.clone();

        // OKX requires milliseconds in the timestamp (ISO 8601 with milliseconds)
        let now = Utc::now();
        let millis = now.timestamp_subsec_millis();
        let timestamp = now.format("%Y-%m-%dT%H:%M:%S").to_string() + &format!(".{:03}Z", millis);
        let signature = credential.sign_bytes(&timestamp, method.as_str(), path, body);

        let mut headers = HashMap::new();
        headers.insert("OK-ACCESS-KEY".to_string(), api_key);
        headers.insert("OK-ACCESS-PASSPHRASE".to_string(), api_passphrase);
        headers.insert("OK-ACCESS-TIMESTAMP".to_string(), timestamp);
        headers.insert("OK-ACCESS-SIGN".to_string(), signature);

        Ok(headers)
    }

    /// Sends an HTTP request to OKX and parses the response into `Vec<T>`.
    ///
    /// Internally, this method handles:
    /// - Building the URL from `base_url` + `path`.
    /// - Optionally signing the request.
    /// - Deserializing JSON responses into typed models, or returning a [`OKXHttpError`].
    /// - Retrying with exponential backoff on transient errors.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The HTTP request fails.
    /// - Authentication is required but credentials are missing.
    /// - The response cannot be deserialized into the expected type.
    /// - The OKX API returns an error response.
    async fn send_request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
        authenticate: bool,
    ) -> Result<Vec<T>, OKXHttpError> {
        let url = format!("{}{path}", self.base_url);
        let endpoint = path.to_string();
        let method_clone = method.clone();
        let body_clone = body.clone();

        let operation = || {
            let url = url.clone();
            let method = method_clone.clone();
            let body = body_clone.clone();
            let endpoint = endpoint.clone();

            async move {
                let mut headers = if authenticate {
                    self.sign_request(&method, endpoint.as_str(), body.as_deref())?
                } else {
                    HashMap::new()
                };

                // Always set Content-Type header when body is present
                if body.is_some() {
                    headers.insert("Content-Type".to_string(), "application/json".to_string());
                }

                let rate_keys = Self::rate_limit_keys(endpoint.as_str());
                let resp = self
                    .client
                    .request_with_ustr_keys(
                        method.clone(),
                        url,
                        Some(headers),
                        body,
                        None,
                        Some(rate_keys),
                    )
                    .await?;

                tracing::trace!("Response: {resp:?}");

                if resp.status.is_success() {
                    let okx_response: OKXResponse<T> =
                        serde_json::from_slice(&resp.body).map_err(|e| {
                            tracing::error!("Failed to deserialize OKXResponse: {e}");
                            OKXHttpError::JsonError(e.to_string())
                        })?;

                    if okx_response.code != OKX_SUCCESS_CODE {
                        return Err(OKXHttpError::OkxError {
                            error_code: okx_response.code,
                            message: okx_response.msg,
                        });
                    }

                    Ok(okx_response.data)
                } else {
                    let error_body = String::from_utf8_lossy(&resp.body);
                    if resp.status.as_u16() == StatusCode::NOT_FOUND.as_u16() {
                        tracing::debug!("HTTP 404 with body: {error_body}");
                    } else {
                        tracing::error!(
                            "HTTP error {} with body: {error_body}",
                            resp.status.as_str()
                        );
                    }

                    if let Ok(parsed_error) = serde_json::from_slice::<OKXResponse<T>>(&resp.body) {
                        return Err(OKXHttpError::OkxError {
                            error_code: parsed_error.code,
                            message: parsed_error.msg,
                        });
                    }

                    Err(OKXHttpError::UnexpectedStatus {
                        status: StatusCode::from_u16(resp.status.as_u16()).unwrap(),
                        body: error_body.to_string(),
                    })
                }
            }
        };

        // Retry strategy based on OKX error responses and HTTP status codes:
        //
        // 1. Network errors: always retry (transient connection issues)
        // 2. HTTP 5xx/429: server errors and rate limiting should be retried
        // 3. OKX specific retryable error codes (defined in common::consts)
        //
        // Note: OKX returns many permanent errors which should NOT be retried
        // (e.g., "Invalid instrument", "Insufficient balance", "Invalid API Key")
        let should_retry = |error: &OKXHttpError| -> bool {
            match error {
                OKXHttpError::HttpClientError(_) => true,
                OKXHttpError::UnexpectedStatus { status, .. } => {
                    status.as_u16() >= 500 || status.as_u16() == 429
                }
                OKXHttpError::OkxError { error_code, .. } => should_retry_error_code(error_code),
                _ => false,
            }
        };

        let create_error = |msg: String| -> OKXHttpError {
            if msg == "canceled" {
                OKXHttpError::Canceled("Adapter disconnecting or shutting down".to_string())
            } else {
                OKXHttpError::ValidationError(msg)
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

    /// Sets the position mode for an account.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization of `params` fails, if the HTTP
    /// request fails, or if the response body cannot be deserialized.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#trading-account-rest-api-set-position-mode>
    pub async fn http_set_position_mode(
        &self,
        params: SetPositionModeParams,
    ) -> Result<Vec<serde_json::Value>, OKXHttpError> {
        let path = "/api/v5/account/set-position-mode";
        let body = serde_json::to_vec(&params)?;
        self.send_request(Method::POST, path, Some(body), true)
            .await
    }

    /// Requests position tiers information, maximum leverage depends on your borrowings and margin ratio.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails, authentication is rejected
    /// or the response cannot be deserialized.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#public-data-rest-api-get-position-tiers>
    pub async fn http_get_position_tiers(
        &self,
        params: GetPositionTiersParams,
    ) -> Result<Vec<OKXPositionTier>, OKXHttpError> {
        let path = Self::build_path("/api/v5/public/position-tiers", &params)?;
        self.send_request(Method::GET, &path, None, false).await
    }

    /// Requests a list of instruments with open contracts.
    ///
    /// # Errors
    ///
    /// Returns an error if JSON serialization of `params` fails, if the HTTP
    /// request fails, or if the response body cannot be deserialized.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#public-data-rest-api-get-instruments>
    pub async fn http_get_instruments(
        &self,
        params: GetInstrumentsParams,
    ) -> Result<Vec<OKXInstrument>, OKXHttpError> {
        let path = Self::build_path("/api/v5/public/instruments", &params)?;
        self.send_request(Method::GET, &path, None, false).await
    }

    /// Requests the current server time from OKX.
    ///
    /// Retrieves the OKX system time in Unix timestamp (milliseconds). This is useful for
    /// synchronizing local clocks with the exchange server and logging time drift.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or if the response body
    /// cannot be parsed into [`OKXServerTime`].
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#public-data-rest-api-get-system-time>
    pub async fn http_get_server_time(&self) -> Result<u64, OKXHttpError> {
        let response: Vec<OKXServerTime> = self
            .send_request(Method::GET, "/api/v5/public/time", None, false)
            .await?;
        response
            .first()
            .map(|t| t.ts)
            .ok_or_else(|| OKXHttpError::JsonError("Empty server time response".to_string()))
    }

    /// Requests a mark price.
    ///
    /// We set the mark price based on the SPOT index and at a reasonable basis to prevent individual
    /// users from manipulating the market and causing the contract price to fluctuate.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or if the response body
    /// cannot be parsed into [`OKXMarkPrice`].
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#public-data-rest-api-get-mark-price>
    pub async fn http_get_mark_price(
        &self,
        params: GetMarkPriceParams,
    ) -> Result<Vec<OKXMarkPrice>, OKXHttpError> {
        let path = Self::build_path("/api/v5/public/mark-price", &params)?;
        self.send_request(Method::GET, &path, None, false).await
    }

    /// Requests the latest index price.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#public-data-rest-api-get-index-tickers>
    pub async fn http_get_index_ticker(
        &self,
        params: GetIndexTickerParams,
    ) -> Result<Vec<OKXIndexTicker>, OKXHttpError> {
        let path = Self::build_path("/api/v5/market/index-tickers", &params)?;
        self.send_request(Method::GET, &path, None, false).await
    }

    /// Requests trades history.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-market-data-get-trades-history>
    pub async fn http_get_trades(
        &self,
        params: GetTradesParams,
    ) -> Result<Vec<OKXTrade>, OKXHttpError> {
        let path = Self::build_path("/api/v5/market/history-trades", &params)?;
        self.send_request(Method::GET, &path, None, false).await
    }

    /// Requests recent candlestick data.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-market-data-get-candlesticks>
    pub async fn http_get_candlesticks(
        &self,
        params: GetCandlesticksParams,
    ) -> Result<Vec<OKXCandlestick>, OKXHttpError> {
        let path = Self::build_path("/api/v5/market/candles", &params)?;
        self.send_request(Method::GET, &path, None, false).await
    }

    /// Requests historical candlestick data.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-market-data-get-candlesticks-history>
    pub async fn http_get_candlesticks_history(
        &self,
        params: GetCandlesticksParams,
    ) -> Result<Vec<OKXCandlestick>, OKXHttpError> {
        let path = Self::build_path("/api/v5/market/history-candles", &params)?;
        self.send_request(Method::GET, &path, None, false).await
    }

    /// Lists current open orders.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-trade-get-orders-pending>
    pub async fn http_get_pending_orders(
        &self,
        params: GetPendingOrdersParams,
    ) -> Result<Vec<OKXOrderHistory>, OKXHttpError> {
        let path = Self::build_path("/api/v5/trade/orders-pending", &params)?;
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Retrieves a single order’s details.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-trade-get-order>
    pub async fn http_get_order(
        &self,
        params: GetOrderParams,
    ) -> Result<Vec<OKXOrderHistory>, OKXHttpError> {
        let path = Self::build_path("/api/v5/trade/order", &params)?;
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Requests a list of assets (with non-zero balance), remaining balance, and available amount
    /// in the trading account.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#trading-account-rest-api-get-balance>
    pub async fn http_get_balance(&self) -> Result<Vec<OKXAccount>, OKXHttpError> {
        let path = "/api/v5/account/balance";
        self.send_request(Method::GET, path, None, true).await
    }

    /// Requests fee rates for the account.
    ///
    /// Returns fee rates for the specified instrument type and the user's VIP level.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#trading-account-rest-api-get-fee-rates>
    pub async fn http_get_trade_fee(
        &self,
        params: GetTradeFeeParams,
    ) -> Result<Vec<OKXFeeRate>, OKXHttpError> {
        let path = Self::build_path("/api/v5/account/trade-fee", &params)?;
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Requests historical order records.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-trade-get-orders-history>
    pub async fn http_get_order_history(
        &self,
        params: GetOrderHistoryParams,
    ) -> Result<Vec<OKXOrderHistory>, OKXHttpError> {
        let path = Self::build_path("/api/v5/trade/orders-history", &params)?;
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Requests order list (pending orders).
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-trade-get-order-list>
    pub async fn http_get_order_list(
        &self,
        params: GetOrderListParams,
    ) -> Result<Vec<OKXOrderHistory>, OKXHttpError> {
        let path = Self::build_path("/api/v5/trade/orders-pending", &params)?;
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Requests pending algo orders.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn http_get_order_algo_pending(
        &self,
        params: GetAlgoOrdersParams,
    ) -> Result<Vec<OKXOrderAlgo>, OKXHttpError> {
        let path = Self::build_path("/api/v5/trade/order-algo-pending", &params)?;
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Requests historical algo orders.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub async fn http_get_order_algo_history(
        &self,
        params: GetAlgoOrdersParams,
    ) -> Result<Vec<OKXOrderAlgo>, OKXHttpError> {
        let path = Self::build_path("/api/v5/trade/order-algo-history", &params)?;
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Requests information on your positions. When the account is in net mode, net positions will
    /// be displayed, and when the account is in long/short mode, long or short positions will be
    /// displayed. Returns in reverse chronological order using ctime.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#trading-account-rest-api-get-positions>
    pub async fn http_get_positions(
        &self,
        params: GetPositionsParams,
    ) -> Result<Vec<OKXPosition>, OKXHttpError> {
        let path = Self::build_path("/api/v5/account/positions", &params)?;
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Requests closed or historical position data.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#trading-account-rest-api-get-positions-history>
    pub async fn http_get_position_history(
        &self,
        params: GetPositionsHistoryParams,
    ) -> Result<Vec<OKXPositionHistory>, OKXHttpError> {
        let path = Self::build_path("/api/v5/account/positions-history", &params)?;
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Requests transaction details (fills) for the given parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-trade-get-transaction-details-last-3-days>
    pub async fn http_get_transaction_details(
        &self,
        params: GetTransactionDetailsParams,
    ) -> Result<Vec<OKXTransactionDetail>, OKXHttpError> {
        let path = Self::build_path("/api/v5/trade/fills", &params)?;
        self.send_request(Method::GET, &path, None, true).await
    }
}

/// Provides a higher-level HTTP client for the [OKX](https://okx.com) REST API.
///
/// This client wraps the underlying `OKXHttpInnerClient` to handle conversions
/// into the Nautilus domain model.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct OKXHttpClient {
    pub(crate) inner: Arc<OKXHttpInnerClient>,
    pub(crate) instruments_cache: Arc<Mutex<HashMap<Ustr, InstrumentAny>>>,
    cache_initialized: bool,
}

impl Default for OKXHttpClient {
    fn default() -> Self {
        Self::new(None, Some(60), None, None, None, false)
            .expect("Failed to create default OKXHttpClient")
    }
}

impl OKXHttpClient {
    /// Creates a new [`OKXHttpClient`] using the default OKX HTTP URL,
    /// optionally overridden with a custom base url.
    ///
    /// This version of the client has **no credentials**, so it can only
    /// call publicly accessible endpoints.
    ///
    /// # Errors
    ///
    /// Returns an error if the retry manager cannot be created.
    pub fn new(
        base_url: Option<String>,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        is_demo: bool,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            inner: Arc::new(OKXHttpInnerClient::new(
                base_url,
                timeout_secs,
                max_retries,
                retry_delay_ms,
                retry_delay_max_ms,
                is_demo,
            )?),
            instruments_cache: Arc::new(Mutex::new(HashMap::new())),
            cache_initialized: false,
        })
    }

    /// Creates a new authenticated [`OKXHttpClient`] using environment variables and
    /// the default OKX HTTP base url.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    pub fn from_env() -> anyhow::Result<Self> {
        Self::with_credentials(None, None, None, None, None, None, None, None, false)
    }

    /// Creates a new [`OKXHttpClient`] configured with credentials
    /// for authenticated requests, optionally using a custom base url.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    #[allow(clippy::too_many_arguments)]
    pub fn with_credentials(
        api_key: Option<String>,
        api_secret: Option<String>,
        api_passphrase: Option<String>,
        base_url: Option<String>,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        is_demo: bool,
    ) -> anyhow::Result<Self> {
        let api_key = get_or_env_var(api_key, "OKX_API_KEY")?;
        let api_secret = get_or_env_var(api_secret, "OKX_API_SECRET")?;
        let api_passphrase = get_or_env_var(api_passphrase, "OKX_API_PASSPHRASE")?;
        let base_url = base_url.unwrap_or(OKX_HTTP_URL.to_string());

        Ok(Self {
            inner: Arc::new(OKXHttpInnerClient::with_credentials(
                api_key,
                api_secret,
                api_passphrase,
                base_url,
                timeout_secs,
                max_retries,
                retry_delay_ms,
                retry_delay_max_ms,
                is_demo,
            )?),
            instruments_cache: Arc::new(Mutex::new(HashMap::new())),
            cache_initialized: false,
        })
    }

    /// Retrieves an instrument from the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument is not found in the cache.
    fn get_instrument_from_cache(&self, symbol: Ustr) -> anyhow::Result<InstrumentAny> {
        self.instruments_cache
            .lock()
            .expect("`instruments_cache` lock poisoned")
            .get(&symbol)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Instrument {symbol} not in cache"))
    }

    async fn instrument_or_fetch(&self, symbol: Ustr) -> anyhow::Result<InstrumentAny> {
        if let Ok(inst) = self.get_instrument_from_cache(symbol) {
            return Ok(inst);
        }

        for group in [
            OKXInstrumentType::Spot,
            OKXInstrumentType::Margin,
            OKXInstrumentType::Futures,
            OKXInstrumentType::Swap,
            OKXInstrumentType::Option,
        ] {
            if let Ok(instruments) = self.request_instruments(group, None).await {
                let mut guard = self.instruments_cache.lock().expect(MUTEX_POISONED);
                for inst in instruments {
                    guard.insert(inst.raw_symbol().inner(), inst);
                }
                drop(guard);

                if let Ok(inst) = self.get_instrument_from_cache(symbol) {
                    return Ok(inst);
                }
            }
        }

        anyhow::bail!("Instrument {symbol} not in cache and fetch failed");
    }

    /// Cancel all pending HTTP requests.
    pub fn cancel_all_requests(&self) {
        self.inner.cancel_all_requests();
    }

    /// Get the cancellation token for this client.
    pub fn cancellation_token(&self) -> &CancellationToken {
        self.inner.cancellation_token()
    }

    /// Returns the base url being used by the client.
    pub fn base_url(&self) -> &str {
        self.inner.base_url.as_str()
    }

    /// Returns the public API key being used by the client.
    pub fn api_key(&self) -> Option<&str> {
        self.inner.credential.as_ref().map(|c| c.api_key.as_str())
    }

    /// Returns whether the client is configured for demo trading.
    #[must_use]
    pub fn is_demo(&self) -> bool {
        self.inner.is_demo
    }

    /// Requests the current server time from OKX.
    ///
    /// Returns the OKX system time as a Unix timestamp in milliseconds.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or if the response cannot be parsed.
    pub async fn http_get_server_time(&self) -> Result<u64, OKXHttpError> {
        self.inner.http_get_server_time().await
    }

    /// Checks if the client is initialized.
    ///
    /// The client is considered initialized if any instruments have been cached from the venue.
    #[must_use]
    pub const fn is_initialized(&self) -> bool {
        self.cache_initialized
    }

    /// Generates a timestamp for initialization.
    fn generate_ts_init(&self) -> UnixNanos {
        get_atomic_clock_realtime().get_time_ns()
    }

    /// Returns the cached instrument symbols.
    #[must_use]
    /// Returns a snapshot of all instrument symbols currently held in the
    /// internal cache.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex guarding the instrument cache is poisoned
    /// (which would indicate a previous panic while the lock was held).
    pub fn get_cached_symbols(&self) -> Vec<String> {
        self.instruments_cache
            .lock()
            .unwrap()
            .keys()
            .map(std::string::ToString::to_string)
            .collect()
    }

    /// Adds the `instruments` to the clients instrument cache.
    ///
    /// Any existing instruments will be replaced.
    /// Inserts multiple instruments into the local cache.
    ///
    /// # Panics
    ///
    /// Panics if the instruments cache mutex is poisoned.
    pub fn add_instruments(&mut self, instruments: Vec<InstrumentAny>) {
        for inst in instruments {
            self.instruments_cache
                .lock()
                .unwrap()
                .insert(inst.raw_symbol().inner(), inst);
        }
        self.cache_initialized = true;
    }

    /// Adds the `instrument` to the clients instrument cache.
    ///
    /// Any existing instrument will be replaced.
    /// Inserts a single instrument into the local cache.
    ///
    /// # Panics
    ///
    /// Panics if the instruments cache mutex is poisoned.
    pub fn add_instrument(&mut self, instrument: InstrumentAny) {
        self.instruments_cache
            .lock()
            .unwrap()
            .insert(instrument.raw_symbol().inner(), instrument);
        self.cache_initialized = true;
    }

    /// Requests the account state for the `account_id` from OKX.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or no account state is returned.
    pub async fn request_account_state(
        &self,
        account_id: AccountId,
    ) -> anyhow::Result<AccountState> {
        let resp = self
            .inner
            .http_get_balance()
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let ts_init = self.generate_ts_init();
        let raw = resp
            .first()
            .ok_or_else(|| anyhow::anyhow!("No account state returned from OKX"))?;
        let account_state = parse_account_state(raw, account_id, ts_init)?;

        Ok(account_state)
    }

    /// Sets the position mode for the account.
    ///
    /// Defaults to NetMode if no position mode is provided.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the position mode cannot be set.
    ///
    /// # Note
    ///
    /// This endpoint only works for accounts with derivatives trading enabled.
    /// If the account only has spot trading, this will return an error.
    pub async fn set_position_mode(&self, position_mode: OKXPositionMode) -> anyhow::Result<()> {
        let mut params = SetPositionModeParamsBuilder::default();
        params.pos_mode(position_mode);
        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        match self.inner.http_set_position_mode(params).await {
            Ok(_) => Ok(()),
            Err(e) => {
                if let OKXHttpError::OkxError {
                    error_code,
                    message,
                } = &e
                    && error_code == "50115"
                {
                    tracing::warn!(
                        "Account does not support position mode setting (derivatives trading not enabled): {message}"
                    );
                    return Ok(()); // Gracefully handle this case
                }
                anyhow::bail!(e)
            }
        }
    }

    /// Requests all instruments for the `instrument_type` from OKX.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or instrument parsing fails.
    pub async fn request_instruments(
        &self,
        instrument_type: OKXInstrumentType,
        instrument_family: Option<String>,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let mut params = GetInstrumentsParamsBuilder::default();
        params.inst_type(instrument_type);

        if let Some(family) = instrument_family.clone() {
            params.inst_family(family);
        }

        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let resp = self
            .inner
            .http_get_instruments(params)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let fee_rate_opt = {
            let fee_params = GetTradeFeeParams {
                inst_type: instrument_type,
                uly: None,
                inst_family: instrument_family,
            };

            match self.inner.http_get_trade_fee(fee_params).await {
                Ok(rates) => rates.into_iter().next(),
                Err(OKXHttpError::MissingCredentials) => {
                    log::debug!("Missing credentials for fee rates, using None");
                    None
                }
                Err(e) => {
                    log::warn!("Failed to fetch fee rates for {instrument_type}: {e}");
                    None
                }
            }
        };

        let ts_init = self.generate_ts_init();

        let mut instruments: Vec<InstrumentAny> = Vec::new();
        for inst in &resp {
            // Determine which fee fields to use based on contract type
            let (maker_fee, taker_fee) = if let Some(ref fee_rate) = fee_rate_opt {
                let is_usdt_margined =
                    inst.ct_type == crate::common::enums::OKXContractType::Linear;
                let (maker_str, taker_str) = if is_usdt_margined {
                    (&fee_rate.maker_u, &fee_rate.taker_u)
                } else {
                    (&fee_rate.maker, &fee_rate.taker)
                };

                let maker = if !maker_str.is_empty() {
                    Decimal::from_str(maker_str).ok()
                } else {
                    None
                };
                let taker = if !taker_str.is_empty() {
                    Decimal::from_str(taker_str).ok()
                } else {
                    None
                };

                (maker, taker)
            } else {
                (None, None)
            };

            match parse_instrument_any(inst, None, None, maker_fee, taker_fee, ts_init) {
                Ok(Some(instrument_any)) => {
                    instruments.push(instrument_any);
                }
                Ok(None) => {
                    // Unsupported instrument type, skip silently
                }
                Err(e) => {
                    log::warn!("Failed to parse instrument {}: {e}", inst.inst_id);
                }
            }
        }

        Ok(instruments)
    }

    /// Requests the latest mark price for the `instrument_type` from OKX.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or no mark price is returned.
    pub async fn request_mark_price(
        &self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<MarkPriceUpdate> {
        let mut params = GetMarkPriceParamsBuilder::default();
        params.inst_id(instrument_id.symbol.inner());
        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let resp = self
            .inner
            .http_get_mark_price(params)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let raw = resp
            .first()
            .ok_or_else(|| anyhow::anyhow!("No mark price returned from OKX"))?;
        let inst = self
            .instrument_or_fetch(instrument_id.symbol.inner())
            .await?;
        let ts_init = self.generate_ts_init();

        let mark_price =
            parse_mark_price_update(raw, instrument_id, inst.price_precision(), ts_init)
                .map_err(|e| anyhow::anyhow!(e))?;
        Ok(mark_price)
    }

    /// Requests the latest index price for the `instrument_id` from OKX.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or no index price is returned.
    pub async fn request_index_price(
        &self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<IndexPriceUpdate> {
        let mut params = GetIndexTickerParamsBuilder::default();
        params.inst_id(instrument_id.symbol.inner());
        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let resp = self
            .inner
            .http_get_index_ticker(params)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let raw = resp
            .first()
            .ok_or_else(|| anyhow::anyhow!("No index price returned from OKX"))?;
        let inst = self
            .instrument_or_fetch(instrument_id.symbol.inner())
            .await?;
        let ts_init = self.generate_ts_init();

        let index_price =
            parse_index_price_update(raw, instrument_id, inst.price_precision(), ts_init)
                .map_err(|e| anyhow::anyhow!(e))?;
        Ok(index_price)
    }

    /// Requests trades for the `instrument_id` and `start` -> `end` time range.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or trade parsing fails.
    pub async fn request_trades(
        &self,
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<TradeTick>> {
        let mut params = GetTradesParamsBuilder::default();

        params.inst_id(instrument_id.symbol.inner());
        if let Some(s) = start {
            params.after(s.timestamp_millis().to_string());
        }
        if let Some(e) = end {
            params.before(e.timestamp_millis().to_string());
        }
        // OKX expects the optional `limit` parameter to be between 1 and 100 (default 100).
        // The request layer uses 0 to express "no explicit limit", so we omit the field in that case
        // and clamp any larger value to the documented maximum to avoid 51000 parameter errors.
        const OKX_TRADES_MAX_LIMIT: u32 = 100;
        if let Some(l) = limit
            && l > 0
        {
            params.limit(l.min(OKX_TRADES_MAX_LIMIT));
        }

        let params = params.build().map_err(anyhow::Error::new)?;

        // Fetch raw trades
        let raw_trades = self
            .inner
            .http_get_trades(params)
            .await
            .map_err(anyhow::Error::new)?;

        let ts_init = self.generate_ts_init();
        let inst = self
            .instrument_or_fetch(instrument_id.symbol.inner())
            .await?;

        let mut trades = Vec::with_capacity(raw_trades.len());
        for raw in raw_trades {
            match parse_trade_tick(
                &raw,
                instrument_id,
                inst.price_precision(),
                inst.size_precision(),
                ts_init,
            ) {
                Ok(trade) => trades.push(trade),
                Err(e) => tracing::error!("{e}"),
            }
        }

        Ok(trades)
    }

    /// Requests historical bars for the given bar type and time range.
    ///
    /// The aggregation source must be `EXTERNAL`. Time range validation ensures start < end.
    /// Returns bars sorted oldest to newest.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    ///
    /// # Endpoint Selection
    ///
    /// The OKX API has different endpoints with different limits:
    /// - Regular endpoint (`/api/v5/market/candles`): ≤ 300 rows/call, ≤ 40 req/2s
    ///   - Used when: start is None OR age ≤ 100 days
    /// - History endpoint (`/api/v5/market/history-candles`): ≤ 100 rows/call, ≤ 20 req/2s
    ///   - Used when: start is Some AND age > 100 days
    ///
    /// Age is calculated as `Utc::now() - start` at the time of the first request.
    ///
    /// # Supported Aggregations
    ///
    /// Maps to OKX bar query parameter:
    /// - `Second` → `{n}s`
    /// - `Minute` → `{n}m`
    /// - `Hour` → `{n}H`
    /// - `Day` → `{n}D`
    /// - `Week` → `{n}W`
    /// - `Month` → `{n}M`
    ///
    /// # Pagination
    ///
    /// - Uses `before` parameter for backwards pagination
    /// - Pages backwards from end time (or now) to start time
    /// - Stops when: limit reached, time window covered, or API returns empty
    /// - Rate limit safety: ≥ 50ms between requests
    ///
    /// # Panics
    ///
    /// May panic if internal data structures are in an unexpected state.
    ///
    /// # References
    ///
    /// - <https://tr.okx.com/docs-v5/en/#order-book-trading-market-data-get-candlesticks>
    /// - <https://tr.okx.com/docs-v5/en/#order-book-trading-market-data-get-candlesticks-history>
    pub async fn request_bars(
        &self,
        bar_type: BarType,
        start: Option<DateTime<Utc>>,
        mut end: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<Bar>> {
        const HISTORY_SPLIT_DAYS: i64 = 100;
        const MAX_PAGES_SOFT: usize = 500;

        let limit = if limit == Some(0) { None } else { limit };

        anyhow::ensure!(
            bar_type.aggregation_source() == AggregationSource::External,
            "Only EXTERNAL aggregation is supported"
        );
        if let (Some(s), Some(e)) = (start, end) {
            anyhow::ensure!(s < e, "Invalid time range: start={s:?} end={e:?}");
        }

        let now = Utc::now();
        if let Some(s) = start
            && s > now
        {
            return Ok(Vec::new());
        }
        if let Some(e) = end
            && e > now
        {
            end = Some(now);
        }

        let spec = bar_type.spec();
        let step = spec.step.get();
        let bar_param = match spec.aggregation {
            BarAggregation::Second => format!("{step}s"),
            BarAggregation::Minute => format!("{step}m"),
            BarAggregation::Hour => format!("{step}H"),
            BarAggregation::Day => format!("{step}D"),
            BarAggregation::Week => format!("{step}W"),
            BarAggregation::Month => format!("{step}M"),
            a => anyhow::bail!("OKX does not support {a:?} aggregation"),
        };

        let slot_ms: i64 = match spec.aggregation {
            BarAggregation::Second => (step as i64) * 1_000,
            BarAggregation::Minute => (step as i64) * 60_000,
            BarAggregation::Hour => (step as i64) * 3_600_000,
            BarAggregation::Day => (step as i64) * 86_400_000,
            BarAggregation::Week => (step as i64) * 7 * 86_400_000,
            BarAggregation::Month => (step as i64) * 30 * 86_400_000,
            _ => unreachable!("Unsupported aggregation should have been caught above"),
        };
        let slot_ns: i64 = slot_ms * 1_000_000;

        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        enum Mode {
            Latest,
            Backward,
            Range,
        }

        let mode = match (start, end) {
            (None, None) => Mode::Latest,
            (Some(_), None) => Mode::Backward, // Changed: when only start is provided, work backward from now
            (None, Some(_)) => Mode::Backward,
            (Some(_), Some(_)) => Mode::Range,
        };

        let start_ns = start.and_then(|s| s.timestamp_nanos_opt());
        let end_ns = end.and_then(|e| e.timestamp_nanos_opt());

        // Floor start and ceiling end to bar boundaries for cleaner API requests
        let start_ms = start.map(|s| {
            let ms = s.timestamp_millis();
            if slot_ms > 0 {
                (ms / slot_ms) * slot_ms // Floor to nearest bar boundary
            } else {
                ms
            }
        });
        let end_ms = end.map(|e| {
            let ms = e.timestamp_millis();
            if slot_ms > 0 {
                ((ms + slot_ms - 1) / slot_ms) * slot_ms // Ceiling to nearest bar boundary
            } else {
                ms
            }
        });
        let now_ms = now.timestamp_millis();

        let symbol = bar_type.instrument_id().symbol;
        let inst = self.instrument_or_fetch(symbol.inner()).await?;

        let mut out: Vec<Bar> = Vec::new();
        let mut pages = 0usize;

        // IMPORTANT: OKX API behavior:
        // - With 'after' parameter: returns bars with timestamp > after (going forward)
        // - With 'before' parameter: returns bars with timestamp < before (going backward)
        // For Range mode, we use 'before' starting from the end time to get bars before it
        let mut after_ms: Option<i64> = None;
        let mut before_ms: Option<i64> = match mode {
            Mode::Backward => end_ms.map(|v| v.saturating_sub(1)),
            Mode::Range => {
                // For Range, start from the end time (or current time if no end specified)
                // The API will return bars with timestamp < before_ms
                Some(end_ms.unwrap_or(now_ms))
            }
            Mode::Latest => None,
        };

        // For Range mode, we'll paginate backwards like Backward mode
        let mut forward_prepend_mode = matches!(mode, Mode::Range);

        // Adjust before_ms to ensure we get data from the API
        // OKX API might not have bars for the very recent past
        // This handles both explicit end=now and the actor layer setting end=now when it's None
        if matches!(mode, Mode::Backward | Mode::Range)
            && let Some(b) = before_ms
        {
            // OKX endpoints have different data availability windows:
            // - Regular endpoint: has most recent data but limited depth
            // - History endpoint: has deep history but lags behind current time
            // Use a small buffer to avoid the "dead zone"
            let buffer_ms = slot_ms.max(60_000); // At least 1 minute or 1 bar
            if b >= now_ms.saturating_sub(buffer_ms) {
                before_ms = Some(now_ms.saturating_sub(buffer_ms));
            }
        }

        let mut have_latest_first_page = false;
        let mut progressless_loops = 0u8;

        loop {
            if let Some(lim) = limit
                && lim > 0
                && out.len() >= lim as usize
            {
                break;
            }
            if pages >= MAX_PAGES_SOFT {
                break;
            }

            let pivot_ms = if let Some(a) = after_ms {
                a
            } else if let Some(b) = before_ms {
                b
            } else {
                now_ms
            };
            // Choose endpoint based on how old the data is:
            // - Use regular endpoint for recent data (< 1 hour old)
            // - Use history endpoint for older data (> 1 hour old)
            // This avoids the "gap" where history endpoint has no recent data
            // and regular endpoint has limited depth
            let age_ms = now_ms.saturating_sub(pivot_ms);
            let age_hours = age_ms / (60 * 60 * 1000);
            let using_history = age_hours > 1; // Use history if data is > 1 hour old

            let page_ceiling = if using_history { 100 } else { 300 };
            let remaining = limit
                .filter(|&l| l > 0) // Treat limit=0 as no limit
                .map_or(page_ceiling, |l| (l as usize).saturating_sub(out.len()));
            let page_cap = remaining.min(page_ceiling);

            let mut p = GetCandlesticksParamsBuilder::default();
            p.inst_id(symbol.as_str())
                .bar(&bar_param)
                .limit(page_cap as u32);

            // Track whether this planned request uses BEFORE or AFTER.
            let mut req_used_before = false;

            match mode {
                Mode::Latest => {
                    if have_latest_first_page && let Some(b) = before_ms {
                        p.before_ms(b);
                        req_used_before = true;
                    }
                }
                Mode::Backward => {
                    if let Some(b) = before_ms {
                        p.before_ms(b);
                        req_used_before = true;
                    }
                }
                Mode::Range => {
                    // For first request with regular endpoint, try without parameters
                    // to get the most recent bars, then filter
                    if pages == 0 && !using_history {
                        // Don't set any time parameters on first request
                        // This gets the most recent bars available
                    } else if forward_prepend_mode {
                        if let Some(b) = before_ms {
                            p.before_ms(b);
                            req_used_before = true;
                        }
                    } else if let Some(a) = after_ms {
                        p.after_ms(a);
                    }
                }
            }

            let params = p.build().map_err(anyhow::Error::new)?;

            let mut raw = if using_history {
                self.inner
                    .http_get_candlesticks_history(params.clone())
                    .await
                    .map_err(anyhow::Error::new)?
            } else {
                self.inner
                    .http_get_candlesticks(params.clone())
                    .await
                    .map_err(anyhow::Error::new)?
            };

            // --- Fallbacks on empty page ---
            if raw.is_empty() {
                // LATEST: retry same cursor via history, then step back a page-interval before giving up
                if matches!(mode, Mode::Latest)
                    && have_latest_first_page
                    && !using_history
                    && let Some(b) = before_ms
                {
                    let mut p2 = GetCandlesticksParamsBuilder::default();
                    p2.inst_id(symbol.as_str())
                        .bar(&bar_param)
                        .limit(page_cap as u32);
                    p2.before_ms(b);
                    let params2 = p2.build().map_err(anyhow::Error::new)?;
                    let raw2 = self
                        .inner
                        .http_get_candlesticks_history(params2)
                        .await
                        .map_err(anyhow::Error::new)?;
                    if !raw2.is_empty() {
                        raw = raw2;
                    } else {
                        // Step back one page interval and retry loop
                        let jump = (page_cap as i64).saturating_mul(slot_ms.max(1));
                        before_ms = Some(b.saturating_sub(jump));
                        progressless_loops = progressless_loops.saturating_add(1);
                        if progressless_loops >= 3 {
                            break;
                        }
                        continue;
                    }
                }

                // Range mode doesn't need special bootstrap - it uses the normal flow with before_ms set

                // If still empty: for Range after first page, try a single backstep window using BEFORE
                if raw.is_empty() && matches!(mode, Mode::Range) && pages > 0 {
                    let backstep_ms = (page_cap as i64).saturating_mul(slot_ms.max(1));
                    let pivot_back = after_ms.unwrap_or(now_ms).saturating_sub(backstep_ms);

                    let mut p2 = GetCandlesticksParamsBuilder::default();
                    p2.inst_id(symbol.as_str())
                        .bar(&bar_param)
                        .limit(page_cap as u32)
                        .before_ms(pivot_back);
                    let params2 = p2.build().map_err(anyhow::Error::new)?;
                    let raw2 = if (now_ms.saturating_sub(pivot_back)) / (24 * 60 * 60 * 1000)
                        > HISTORY_SPLIT_DAYS
                    {
                        self.inner.http_get_candlesticks_history(params2).await
                    } else {
                        self.inner.http_get_candlesticks(params2).await
                    }
                    .map_err(anyhow::Error::new)?;
                    if raw2.is_empty() {
                        break;
                    } else {
                        raw = raw2;
                        forward_prepend_mode = true;
                        req_used_before = true;
                    }
                }

                // First LATEST page empty: jump back >100d to force history, then continue loop
                if raw.is_empty()
                    && matches!(mode, Mode::Latest)
                    && !have_latest_first_page
                    && !using_history
                {
                    let jump_days_ms = (HISTORY_SPLIT_DAYS + 1) * 86_400_000;
                    before_ms = Some(now_ms.saturating_sub(jump_days_ms));
                    have_latest_first_page = true;
                    continue;
                }

                // Still empty for any other case? Just break.
                if raw.is_empty() {
                    break;
                }
            }
            // --- end fallbacks ---

            pages += 1;

            // Parse, oldest → newest
            let ts_init = self.generate_ts_init();
            let mut page: Vec<Bar> = Vec::with_capacity(raw.len());
            for r in &raw {
                page.push(parse_candlestick(
                    r,
                    bar_type,
                    inst.price_precision(),
                    inst.size_precision(),
                    ts_init,
                )?);
            }
            page.reverse();

            let page_oldest_ms = page.first().map(|b| b.ts_event.as_i64() / 1_000_000);
            let page_newest_ms = page.last().map(|b| b.ts_event.as_i64() / 1_000_000);

            // Range filter (inclusive)
            // For Range mode, if we have no bars yet and this is an early page,
            // be more tolerant with the start boundary to handle gaps in data
            let mut filtered: Vec<Bar> = if matches!(mode, Mode::Range)
                && out.is_empty()
                && pages < 2
            {
                // On first pages of Range mode with no data yet, include the most recent bar
                // even if it's slightly before our start time (within 2 bar periods)
                // BUT we want ALL bars in the page that are within our range
                let tolerance_ns = slot_ns * 2; // Allow up to 2 bar periods before start

                // Debug: log the page range
                if !page.is_empty() {
                    tracing::debug!(
                        "Range mode bootstrap page: {} bars from {} to {}, filtering with start={:?} end={:?}",
                        page.len(),
                        page.first().unwrap().ts_event.as_i64() / 1_000_000,
                        page.last().unwrap().ts_event.as_i64() / 1_000_000,
                        start_ms,
                        end_ms
                    );
                }

                let result: Vec<Bar> = page
                    .clone()
                    .into_iter()
                    .filter(|b| {
                        let ts = b.ts_event.as_i64();
                        // Accept bars from (start - tolerance) to end
                        let ok_after =
                            start_ns.is_none_or(|sns| ts >= sns.saturating_sub(tolerance_ns));
                        let ok_before = end_ns.is_none_or(|ens| ts <= ens);
                        ok_after && ok_before
                    })
                    .collect();

                result
            } else {
                // Normal filtering
                page.clone()
                    .into_iter()
                    .filter(|b| {
                        let ts = b.ts_event.as_i64();
                        let ok_after = start_ns.is_none_or(|sns| ts >= sns);
                        let ok_before = end_ns.is_none_or(|ens| ts <= ens);
                        ok_after && ok_before
                    })
                    .collect()
            };

            if !page.is_empty() && filtered.is_empty() {
                // For Range mode, if all bars are before our start time, there's no point continuing
                if matches!(mode, Mode::Range)
                    && !forward_prepend_mode
                    && let (Some(newest_ms), Some(start_ms)) = (page_newest_ms, start_ms)
                    && newest_ms < start_ms.saturating_sub(slot_ms * 2)
                {
                    // Bars are too old (more than 2 bar periods before start), stop
                    break;
                }
            }

            // Track contribution for progress guard
            let contribution;

            if out.is_empty() {
                contribution = filtered.len();
                out = filtered;
            } else {
                match mode {
                    Mode::Backward | Mode::Latest => {
                        if let Some(first) = out.first() {
                            filtered.retain(|b| b.ts_event < first.ts_event);
                        }
                        contribution = filtered.len();
                        if contribution != 0 {
                            let mut new_out = Vec::with_capacity(out.len() + filtered.len());
                            new_out.extend_from_slice(&filtered);
                            new_out.extend_from_slice(&out);
                            out = new_out;
                        }
                    }
                    Mode::Range => {
                        if forward_prepend_mode || req_used_before {
                            // We are backfilling older pages: prepend them.
                            if let Some(first) = out.first() {
                                filtered.retain(|b| b.ts_event < first.ts_event);
                            }
                            contribution = filtered.len();
                            if contribution != 0 {
                                let mut new_out = Vec::with_capacity(out.len() + filtered.len());
                                new_out.extend_from_slice(&filtered);
                                new_out.extend_from_slice(&out);
                                out = new_out;
                            }
                        } else {
                            // Normal forward: append newer pages.
                            if let Some(last) = out.last() {
                                filtered.retain(|b| b.ts_event > last.ts_event);
                            }
                            contribution = filtered.len();
                            out.extend(filtered);
                        }
                    }
                }
            }

            // Duplicate-window mitigation for Latest/Backward
            if contribution == 0
                && matches!(mode, Mode::Latest | Mode::Backward)
                && let Some(b) = before_ms
            {
                let jump = (page_cap as i64).saturating_mul(slot_ms.max(1));
                let new_b = b.saturating_sub(jump);
                if new_b != b {
                    before_ms = Some(new_b);
                }
            }

            if contribution == 0 {
                progressless_loops = progressless_loops.saturating_add(1);
                if progressless_loops >= 3 {
                    break;
                }
            } else {
                progressless_loops = 0;

                // Advance cursors only when we made progress
                match mode {
                    Mode::Latest | Mode::Backward => {
                        if let Some(oldest) = page_oldest_ms {
                            before_ms = Some(oldest.saturating_sub(1));
                            have_latest_first_page = true;
                        } else {
                            break;
                        }
                    }
                    Mode::Range => {
                        if forward_prepend_mode || req_used_before {
                            if let Some(oldest) = page_oldest_ms {
                                // Move back by at least one bar period to avoid getting the same data
                                let jump_back = slot_ms.max(60_000); // At least 1 minute
                                before_ms = Some(oldest.saturating_sub(jump_back));
                                after_ms = None;
                            } else {
                                break;
                            }
                        } else if let Some(newest) = page_newest_ms {
                            after_ms = Some(newest.saturating_add(1));
                            before_ms = None;
                        } else {
                            break;
                        }
                    }
                }
            }

            // Stop conditions
            if let Some(lim) = limit
                && lim > 0
                && out.len() >= lim as usize
            {
                break;
            }
            if let Some(ens) = end_ns
                && let Some(last) = out.last()
                && last.ts_event.as_i64() >= ens
            {
                break;
            }
            if let Some(sns) = start_ns
                && let Some(first) = out.first()
                && (matches!(mode, Mode::Backward) || forward_prepend_mode)
                && first.ts_event.as_i64() <= sns
            {
                // For Range mode, check if we have all bars up to the end time
                if matches!(mode, Mode::Range) {
                    // Don't stop if we haven't reached the end time yet
                    if let Some(ens) = end_ns
                        && let Some(last) = out.last()
                    {
                        let last_ts = last.ts_event.as_i64();
                        if last_ts < ens {
                            // We have bars before start but haven't reached end, need to continue forward
                            // Switch from backward to forward pagination
                            forward_prepend_mode = false;
                            after_ms = Some((last_ts / 1_000_000).saturating_add(1));
                            before_ms = None;
                            continue;
                        }
                    }
                }
                break;
            }

            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }

        // Final rescue for FORWARD/RANGE when nothing gathered
        if out.is_empty() && matches!(mode, Mode::Range) {
            let pivot = end_ms.unwrap_or(now_ms.saturating_sub(1));
            let hist = (now_ms.saturating_sub(pivot)) / (24 * 60 * 60 * 1000) > HISTORY_SPLIT_DAYS;
            let mut p = GetCandlesticksParamsBuilder::default();
            p.inst_id(symbol.as_str())
                .bar(&bar_param)
                .limit(300)
                .before_ms(pivot);
            let params = p.build().map_err(anyhow::Error::new)?;
            let raw = if hist {
                self.inner.http_get_candlesticks_history(params).await
            } else {
                self.inner.http_get_candlesticks(params).await
            }
            .map_err(anyhow::Error::new)?;
            if !raw.is_empty() {
                let ts_init = self.generate_ts_init();
                let mut page: Vec<Bar> = Vec::with_capacity(raw.len());
                for r in &raw {
                    page.push(parse_candlestick(
                        r,
                        bar_type,
                        inst.price_precision(),
                        inst.size_precision(),
                        ts_init,
                    )?);
                }
                page.reverse();
                out = page
                    .into_iter()
                    .filter(|b| {
                        let ts = b.ts_event.as_i64();
                        let ok_after = start_ns.is_none_or(|sns| ts >= sns);
                        let ok_before = end_ns.is_none_or(|ens| ts <= ens);
                        ok_after && ok_before
                    })
                    .collect();
            }
        }

        // Trim against end bound if needed (keep ≤ end)
        if let Some(ens) = end_ns {
            while out.last().is_some_and(|b| b.ts_event.as_i64() > ens) {
                out.pop();
            }
        }

        // Clamp first bar for Range when using forward pagination
        if matches!(mode, Mode::Range)
            && !forward_prepend_mode
            && let Some(sns) = start_ns
        {
            let lower = sns.saturating_sub(slot_ns);
            while out.first().is_some_and(|b| b.ts_event.as_i64() < lower) {
                out.remove(0);
            }
        }

        if let Some(lim) = limit
            && lim > 0
            && out.len() > lim as usize
        {
            out.truncate(lim as usize);
        }

        Ok(out)
    }

    /// Requests historical order status reports for the given parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    ///
    /// # References
    ///
    /// - <https://www.okx.com/docs-v5/en/#order-book-trading-trade-get-order-history-last-7-days>.
    /// - <https://www.okx.com/docs-v5/en/#order-book-trading-trade-get-order-history-last-3-months>.
    #[allow(clippy::too_many_arguments)]
    pub async fn request_order_status_reports(
        &self,
        account_id: AccountId,
        instrument_type: Option<OKXInstrumentType>,
        instrument_id: Option<InstrumentId>,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        open_only: bool,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let mut history_params = GetOrderHistoryParamsBuilder::default();

        let instrument_type = if let Some(instrument_type) = instrument_type {
            instrument_type
        } else {
            let instrument_id = instrument_id.ok_or_else(|| {
                anyhow::anyhow!("Instrument ID required if `instrument_type` not provided")
            })?;
            let instrument = self
                .instrument_or_fetch(instrument_id.symbol.inner())
                .await?;
            okx_instrument_type(&instrument)?
        };

        history_params.inst_type(instrument_type);

        if let Some(instrument_id) = instrument_id.as_ref() {
            history_params.inst_id(instrument_id.symbol.inner().to_string());
        }

        if let Some(limit) = limit {
            history_params.limit(limit);
        }

        let history_params = history_params.build().map_err(|e| anyhow::anyhow!(e))?;

        let mut pending_params = GetOrderListParamsBuilder::default();
        pending_params.inst_type(instrument_type);

        if let Some(instrument_id) = instrument_id.as_ref() {
            pending_params.inst_id(instrument_id.symbol.inner().to_string());
        }

        if let Some(limit) = limit {
            pending_params.limit(limit);
        }

        let pending_params = pending_params.build().map_err(|e| anyhow::anyhow!(e))?;

        let combined_resp = if open_only {
            // Only request pending/open orders
            self.inner
                .http_get_order_list(pending_params)
                .await
                .map_err(|e| anyhow::anyhow!(e))?
        } else {
            // Make both requests concurrently
            let (history_resp, pending_resp) = tokio::try_join!(
                self.inner.http_get_order_history(history_params),
                self.inner.http_get_order_list(pending_params)
            )
            .map_err(|e| anyhow::anyhow!(e))?;

            // Combine both responses
            let mut combined_resp = history_resp;
            combined_resp.extend(pending_resp);
            combined_resp
        };

        // Prepare time range filter
        let start_ns = start.map(UnixNanos::from);
        let end_ns = end.map(UnixNanos::from);

        let ts_init = self.generate_ts_init();
        let mut reports = Vec::with_capacity(combined_resp.len());

        // Use a seen filter in case pending orders are within the histories "2hr reserve window"
        let mut seen: AHashSet<String> = AHashSet::new();

        for order in combined_resp {
            let seen_key = if !order.cl_ord_id.is_empty() {
                order.cl_ord_id.as_str().to_string()
            } else if let Some(algo_cl_ord_id) = order
                .algo_cl_ord_id
                .as_ref()
                .filter(|value| !value.as_str().is_empty())
            {
                algo_cl_ord_id.as_str().to_string()
            } else if let Some(algo_id) = order
                .algo_id
                .as_ref()
                .filter(|value| !value.as_str().is_empty())
            {
                algo_id.as_str().to_string()
            } else {
                order.ord_id.as_str().to_string()
            };

            if !seen.insert(seen_key) {
                continue; // Reserved pending already reported
            }

            let inst = self.instrument_or_fetch(order.inst_id).await?;

            let report = parse_order_status_report(
                &order,
                account_id,
                inst.id(),
                inst.price_precision(),
                inst.size_precision(),
                ts_init,
            );

            if let Some(start_ns) = start_ns
                && report.ts_last < start_ns
            {
                continue;
            }
            if let Some(end_ns) = end_ns
                && report.ts_last > end_ns
            {
                continue;
            }

            reports.push(report);
        }

        Ok(reports)
    }

    /// Requests fill reports (transaction details) for the given parameters.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-trade-get-transaction-details-last-3-days>.
    pub async fn request_fill_reports(
        &self,
        account_id: AccountId,
        instrument_type: Option<OKXInstrumentType>,
        instrument_id: Option<InstrumentId>,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<FillReport>> {
        let mut params = GetTransactionDetailsParamsBuilder::default();

        let instrument_type = if let Some(instrument_type) = instrument_type {
            instrument_type
        } else {
            let instrument_id = instrument_id.ok_or_else(|| {
                anyhow::anyhow!("Instrument ID required if `instrument_type` not provided")
            })?;
            let instrument = self
                .instrument_or_fetch(instrument_id.symbol.inner())
                .await?;
            okx_instrument_type(&instrument)?
        };

        params.inst_type(instrument_type);

        if let Some(instrument_id) = instrument_id {
            let instrument = self
                .instrument_or_fetch(instrument_id.symbol.inner())
                .await?;
            let instrument_type = okx_instrument_type(&instrument)?;
            params.inst_type(instrument_type);
            params.inst_id(instrument_id.symbol.inner().to_string());
        }

        if let Some(limit) = limit {
            params.limit(limit);
        }

        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let resp = self
            .inner
            .http_get_transaction_details(params)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        // Prepare time range filter
        let start_ns = start.map(UnixNanos::from);
        let end_ns = end.map(UnixNanos::from);

        let ts_init = self.generate_ts_init();
        let mut reports = Vec::with_capacity(resp.len());

        for detail in resp {
            let inst = self.instrument_or_fetch(detail.inst_id).await?;

            let report = parse_fill_report(
                detail,
                account_id,
                inst.id(),
                inst.price_precision(),
                inst.size_precision(),
                ts_init,
            )?;

            if let Some(start_ns) = start_ns
                && report.ts_event < start_ns
            {
                continue;
            }

            if let Some(end_ns) = end_ns
                && report.ts_event > end_ns
            {
                continue;
            }

            reports.push(report);
        }

        Ok(reports)
    }

    /// Requests current position status reports for the given parameters.
    ///
    /// # Position Modes
    ///
    /// OKX supports two position modes, which affects how position data is returned:
    ///
    /// ## Net Mode (One-way)
    /// - `posSide` field will be `"net"`
    /// - `pos` field uses **signed quantities**:
    ///   - Positive value = Long position
    ///   - Negative value = Short position
    ///   - Zero = Flat/no position
    ///
    /// ## Long/Short Mode (Hedge/Dual-side)
    /// - `posSide` field will be `"long"` or `"short"`
    /// - `pos` field is **always positive** (use `posSide` to determine actual side)
    /// - Allows holding simultaneous long and short positions on the same instrument
    /// - Position IDs are suffixed with `-LONG` or `-SHORT` for uniqueness
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#trading-account-rest-api-get-positions>
    pub async fn request_position_status_reports(
        &self,
        account_id: AccountId,
        instrument_type: Option<OKXInstrumentType>,
        instrument_id: Option<InstrumentId>,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let mut params = GetPositionsParamsBuilder::default();

        let instrument_type = if let Some(instrument_type) = instrument_type {
            instrument_type
        } else {
            let instrument_id = instrument_id.ok_or_else(|| {
                anyhow::anyhow!("Instrument ID required if `instrument_type` not provided")
            })?;
            let instrument = self
                .instrument_or_fetch(instrument_id.symbol.inner())
                .await?;
            okx_instrument_type(&instrument)?
        };

        params.inst_type(instrument_type);

        instrument_id
            .as_ref()
            .map(|i| params.inst_id(i.symbol.inner()));

        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let resp = self
            .inner
            .http_get_positions(params)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let ts_init = self.generate_ts_init();
        let mut reports = Vec::with_capacity(resp.len());

        for position in resp {
            let inst = self.instrument_or_fetch(position.inst_id).await?;

            let report = parse_position_status_report(
                position,
                account_id,
                inst.id(),
                inst.size_precision(),
                ts_init,
            )?;
            reports.push(report);
        }

        Ok(reports)
    }

    /// Places an algo order via HTTP.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-algo-trading-post-place-algo-order>
    pub async fn place_algo_order(
        &self,
        request: OKXPlaceAlgoOrderRequest,
    ) -> Result<OKXPlaceAlgoOrderResponse, OKXHttpError> {
        let body =
            serde_json::to_vec(&request).map_err(|e| OKXHttpError::JsonError(e.to_string()))?;

        let resp: Vec<OKXPlaceAlgoOrderResponse> = self
            .inner
            .send_request(Method::POST, "/api/v5/trade/order-algo", Some(body), true)
            .await?;

        resp.into_iter()
            .next()
            .ok_or_else(|| OKXHttpError::ValidationError("Empty response".to_string()))
    }

    /// Cancels an algo order via HTTP.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-algo-trading-post-cancel-algo-order>
    pub async fn cancel_algo_order(
        &self,
        request: OKXCancelAlgoOrderRequest,
    ) -> Result<OKXCancelAlgoOrderResponse, OKXHttpError> {
        // OKX expects an array for cancel-algos endpoint
        // Serialize once to bytes to keep signing and sending identical
        let body =
            serde_json::to_vec(&[request]).map_err(|e| OKXHttpError::JsonError(e.to_string()))?;

        let resp: Vec<OKXCancelAlgoOrderResponse> = self
            .inner
            .send_request(Method::POST, "/api/v5/trade/cancel-algos", Some(body), true)
            .await?;

        resp.into_iter()
            .next()
            .ok_or_else(|| OKXHttpError::ValidationError("Empty response".to_string()))
    }

    /// Places an algo order using domain types.
    ///
    /// This is a convenience method that accepts Nautilus domain types
    /// and builds the appropriate OKX request structure internally.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    #[allow(clippy::too_many_arguments)]
    pub async fn place_algo_order_with_domain_types(
        &self,
        instrument_id: InstrumentId,
        td_mode: OKXTradeMode,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        trigger_price: Price,
        trigger_type: Option<TriggerType>,
        limit_price: Option<Price>,
        reduce_only: Option<bool>,
    ) -> Result<OKXPlaceAlgoOrderResponse, OKXHttpError> {
        if !matches!(order_side, OrderSide::Buy | OrderSide::Sell) {
            return Err(OKXHttpError::ValidationError(
                "Invalid order side".to_string(),
            ));
        }
        let okx_side: OKXSide = order_side.into();

        // Map trigger type to OKX format
        let trigger_px_type_enum = trigger_type.map_or(OKXTriggerType::Last, Into::into);

        // Determine order price based on order type
        let order_px = if matches!(order_type, OrderType::StopLimit | OrderType::LimitIfTouched) {
            limit_price.map(|p| p.to_string())
        } else {
            // Market orders use -1 to indicate market execution
            Some("-1".to_string())
        };

        let request = OKXPlaceAlgoOrderRequest {
            inst_id: instrument_id.symbol.as_str().to_string(),
            td_mode,
            side: okx_side,
            ord_type: OKXAlgoOrderType::Trigger, // All conditional orders use 'trigger' type
            sz: quantity.to_string(),
            algo_cl_ord_id: Some(client_order_id.as_str().to_string()),
            trigger_px: Some(trigger_price.to_string()),
            order_px,
            trigger_px_type: Some(trigger_px_type_enum),
            tgt_ccy: None,  // Let OKX determine based on instrument
            pos_side: None, // Use default position side
            close_position: None,
            tag: Some(OKX_NAUTILUS_BROKER_ID.to_string()),
            reduce_only,
        };

        self.place_algo_order(request).await
    }

    /// Cancels an algo order using domain types.
    ///
    /// This is a convenience method that accepts Nautilus domain types
    /// and builds the appropriate OKX request structure internally.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn cancel_algo_order_with_domain_types(
        &self,
        instrument_id: InstrumentId,
        algo_id: String,
    ) -> Result<OKXCancelAlgoOrderResponse, OKXHttpError> {
        let request = OKXCancelAlgoOrderRequest {
            inst_id: instrument_id.symbol.to_string(),
            algo_id: Some(algo_id),
            algo_cl_ord_id: None,
        };

        self.cancel_algo_order(request).await
    }

    /// Requests algo order status reports.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    #[allow(clippy::too_many_arguments)]
    pub async fn request_algo_order_status_reports(
        &self,
        account_id: AccountId,
        instrument_type: Option<OKXInstrumentType>,
        instrument_id: Option<InstrumentId>,
        algo_id: Option<String>,
        algo_client_order_id: Option<ClientOrderId>,
        state: Option<OKXOrderStatus>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let mut instruments_cache: AHashMap<Ustr, InstrumentAny> = AHashMap::new();

        let inst_type = if let Some(inst_type) = instrument_type {
            inst_type
        } else if let Some(inst_id) = instrument_id {
            let instrument = self.instrument_or_fetch(inst_id.symbol.inner()).await?;
            let inst_type = okx_instrument_type(&instrument)?;
            instruments_cache.insert(inst_id.symbol.inner(), instrument);
            inst_type
        } else {
            anyhow::bail!("instrument_type or instrument_id required for algo order query")
        };

        let mut params_builder = GetAlgoOrdersParamsBuilder::default();
        params_builder.inst_type(inst_type);
        if let Some(inst_id) = instrument_id {
            params_builder.inst_id(inst_id.symbol.inner().to_string());
        }
        if let Some(algo_id) = algo_id.as_ref() {
            params_builder.algo_id(algo_id.clone());
        }
        if let Some(client_order_id) = algo_client_order_id.as_ref() {
            params_builder.algo_cl_ord_id(client_order_id.as_str().to_string());
        }
        if let Some(state) = state {
            params_builder.state(state);
        }
        if let Some(limit) = limit {
            params_builder.limit(limit);
        }

        let params = params_builder
            .build()
            .map_err(|e| anyhow::anyhow!(format!("Failed to build algo order params: {e}")))?;

        let ts_init = self.generate_ts_init();
        let mut reports = Vec::new();
        let mut seen: AHashSet<(String, String)> = AHashSet::new();

        let pending = match self.inner.http_get_order_algo_pending(params.clone()).await {
            Ok(result) => result,
            Err(OKXHttpError::UnexpectedStatus { status, .. })
                if status == StatusCode::NOT_FOUND =>
            {
                Vec::new()
            }
            Err(e) => return Err(e.into()),
        };
        self.collect_algo_reports(
            account_id,
            &pending,
            &mut instruments_cache,
            ts_init,
            &mut seen,
            &mut reports,
        )
        .await?;

        let history = match self.inner.http_get_order_algo_history(params).await {
            Ok(result) => result,
            Err(OKXHttpError::UnexpectedStatus { status, .. })
                if status == StatusCode::NOT_FOUND =>
            {
                Vec::new()
            }
            Err(e) => return Err(e.into()),
        };
        self.collect_algo_reports(
            account_id,
            &history,
            &mut instruments_cache,
            ts_init,
            &mut seen,
            &mut reports,
        )
        .await?;

        Ok(reports)
    }

    /// Requests an algo order status report by client order identifier.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn request_algo_order_status_report(
        &self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        algo_client_order_id: ClientOrderId,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let reports = self
            .request_algo_order_status_reports(
                account_id,
                None,
                Some(instrument_id),
                None,
                Some(algo_client_order_id),
                None,
                Some(50_u32),
            )
            .await?;

        Ok(reports.into_iter().next())
    }
    async fn collect_algo_reports(
        &self,
        account_id: AccountId,
        orders: &[OKXOrderAlgo],
        instruments_cache: &mut AHashMap<Ustr, InstrumentAny>,
        ts_init: UnixNanos,
        seen: &mut AHashSet<(String, String)>,
        reports: &mut Vec<OrderStatusReport>,
    ) -> anyhow::Result<()> {
        for order in orders {
            let key = (order.algo_id.clone(), order.algo_cl_ord_id.clone());
            if !seen.insert(key) {
                continue;
            }

            let instrument = if let Some(instrument) = instruments_cache.get(&order.inst_id) {
                instrument.clone()
            } else {
                let instrument = self.instrument_or_fetch(order.inst_id).await?;
                instruments_cache.insert(order.inst_id, instrument.clone());
                instrument
            };

            let report = parse_http_algo_order(order, account_id, &instrument, ts_init)?;
            reports.push(report);
        }

        Ok(())
    }
}

fn parse_http_algo_order(
    order: &OKXOrderAlgo,
    account_id: AccountId,
    instrument: &InstrumentAny,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let ord_px = if order.ord_px.is_empty() {
        "-1".to_string()
    } else {
        order.ord_px.clone()
    };

    let reduce_only = if order.reduce_only.is_empty() {
        "false".to_string()
    } else {
        order.reduce_only.clone()
    };

    let msg = OKXAlgoOrderMsg {
        algo_id: order.algo_id.clone(),
        algo_cl_ord_id: order.algo_cl_ord_id.clone(),
        cl_ord_id: order.cl_ord_id.clone(),
        ord_id: order.ord_id.clone(),
        inst_id: order.inst_id,
        inst_type: order.inst_type,
        ord_type: order.ord_type,
        state: order.state,
        side: order.side,
        pos_side: order.pos_side,
        sz: order.sz.clone(),
        trigger_px: order.trigger_px.clone(),
        trigger_px_type: order.trigger_px_type.unwrap_or(OKXTriggerType::None),
        ord_px,
        td_mode: order.td_mode,
        lever: order.lever.clone(),
        reduce_only,
        actual_px: order.actual_px.clone(),
        actual_sz: order.actual_sz.clone(),
        notional_usd: order.notional_usd.clone(),
        c_time: order.c_time,
        u_time: order.u_time,
        trigger_time: order.trigger_time.clone(),
        tag: order.tag.clone(),
    };

    parse_algo_order_status_report(&msg, instrument, account_id, ts_init)
}
