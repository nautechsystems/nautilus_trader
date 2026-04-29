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

//! Provides the HTTP client integration for the Ax REST API.

use std::{
    collections::HashMap,
    fmt::Debug,
    num::NonZeroU32,
    sync::{
        Arc, LazyLock, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

use chrono::{DateTime, Utc};
use nautilus_core::{
    AtomicMap, AtomicTime, UUID4, consts::NAUTILUS_USER_AGENT, nanos::UnixNanos,
    time::get_atomic_clock_realtime,
};
use nautilus_model::{
    data::{Bar, BookOrder, FundingRateUpdate, TradeTick},
    enums::{BookType, OrderSide, OrderType, TimeInForce},
    events::AccountState,
    identifiers::{AccountId, ClientOrderId, InstrumentId, VenueOrderId},
    instruments::{Instrument, any::InstrumentAny},
    orderbook::OrderBook,
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{Price, Quantity},
};
use nautilus_network::{
    http::HttpClient,
    ratelimiter::quota::Quota,
    retry::{RetryConfig, RetryManager},
};
use reqwest::{Method, header::USER_AGENT};
use rust_decimal::Decimal;
use serde::{Serialize, de::DeserializeOwned};
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{
    error::AxHttpError,
    models::{
        AuthenticateApiKeyRequest, AxAuthenticateResponse, AxBalancesResponse, AxBookResponse,
        AxCancelAllOrdersResponse, AxCancelOrderResponse, AxCandle, AxCandleResponse,
        AxCandlesResponse, AxFillsResponse, AxFundingRatesResponse,
        AxInitialMarginRequirementResponse, AxInstrument, AxInstrumentsResponse,
        AxOpenOrdersResponse, AxOrderStatusQueryResponse, AxOrdersResponse, AxPlaceOrderResponse,
        AxPositionsResponse, AxPreviewAggressiveLimitOrderResponse, AxReplaceOrderResponse,
        AxRiskSnapshotResponse, AxTicker, AxTickersResponse, AxTradesResponse,
        AxTransactionsResponse, AxWhoAmI, CancelAllOrdersRequest, CancelOrderRequest,
        PlaceOrderRequest, PreviewAggressiveLimitOrderRequest, ReplaceOrderRequest,
    },
    parse::{
        parse_account_state, parse_bar, parse_fill_report, parse_funding_rate,
        parse_order_status_report, parse_perp_instrument, parse_position_status_report,
        parse_trade_tick,
    },
    query::{
        GetBookParams, GetCandleParams, GetCandlesParams, GetFundingRatesParams,
        GetInstrumentParams, GetOrderStatusParams, GetOrdersParams, GetTickerParams,
        GetTradesParams, GetTransactionsParams,
    },
};
use crate::common::{
    consts::{AX_HTTP_URL, AX_ORDERS_URL},
    credential::Credential,
    enums::{AxCandleWidth, AxInstrumentState},
    parse::{cid_to_client_order_id, client_order_id_to_cid},
};

/// Default Ax REST API rate limit.
///
/// Conservative default of 10 requests per second.
pub static AX_REST_QUOTA: LazyLock<Quota> = LazyLock::new(|| {
    Quota::per_second(NonZeroU32::new(10).expect("non-zero")).expect("valid constant")
});

const AX_GLOBAL_RATE_KEY: &str = "architect:global";

/// Raw HTTP client for low-level AX Exchange API operations.
///
/// This client handles request/response operations with the AX Exchange API,
/// returning venue-specific response types. It does not parse to Nautilus domain types.
pub struct AxRawHttpClient {
    base_url: String,
    orders_base_url: String,
    client: HttpClient,
    credential: Option<Credential>,
    session_token: RwLock<Option<String>>,
    retry_manager: RetryManager<AxHttpError>,
    cancellation_token: RwLock<CancellationToken>,
}

impl Default for AxRawHttpClient {
    fn default() -> Self {
        Self::new(None, None, 60, 3, 1000, 10_000, None)
            .expect("Failed to create default AxRawHttpClient")
    }
}

impl Debug for AxRawHttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let has_session_token = self.session_token.read().is_ok_and(|guard| guard.is_some());
        f.debug_struct(stringify!(AxRawHttpClient))
            .field("base_url", &self.base_url)
            .field("orders_base_url", &self.orders_base_url)
            .field("has_credentials", &self.credential.is_some())
            .field("has_session_token", &has_session_token)
            .finish()
    }
}

impl AxRawHttpClient {
    /// Returns the base URL for this client.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Returns a masked version of the API key for logging purposes.
    #[must_use]
    pub fn api_key_masked(&self) -> String {
        self.credential
            .as_ref()
            .map_or_else(|| "None".to_string(), |c| c.masked_api_key())
    }

    /// Cancel all pending HTTP requests.
    ///
    /// # Panics
    ///
    /// Panics if the cancellation token lock is poisoned.
    pub fn cancel_all_requests(&self) {
        self.cancellation_token
            .read()
            .expect("Lock poisoned")
            .cancel();
    }

    /// Replaces the cancelled token so new requests can proceed after reconnect.
    ///
    /// # Panics
    ///
    /// Panics if the cancellation token lock is poisoned.
    pub fn reset_cancellation_token(&self) {
        *self.cancellation_token.write().expect("Lock poisoned") = CancellationToken::new();
    }

    /// Get a clone of the current cancellation token.
    ///
    /// # Panics
    ///
    /// Panics if the cancellation token lock is poisoned.
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation_token
            .read()
            .expect("Lock poisoned")
            .clone()
    }

    /// Creates a new [`AxRawHttpClient`] using the default Ax HTTP URL.
    ///
    /// # Errors
    ///
    /// Returns an error if the retry manager cannot be created.
    pub fn new(
        base_url: Option<String>,
        orders_base_url: Option<String>,
        timeout_secs: u64,
        max_retries: u32,
        retry_delay_ms: u64,
        retry_delay_max_ms: u64,
        proxy_url: Option<String>,
    ) -> Result<Self, AxHttpError> {
        let retry_config = RetryConfig {
            max_retries,
            initial_delay_ms: retry_delay_ms,
            max_delay_ms: retry_delay_max_ms,
            backoff_factor: 2.0,
            jitter_ms: 1000,
            operation_timeout_ms: Some(60_000),
            immediate_first: false,
            max_elapsed_ms: Some(180_000),
        };

        let retry_manager = RetryManager::new(retry_config);

        Ok(Self {
            base_url: base_url.unwrap_or_else(|| AX_HTTP_URL.to_string()),
            orders_base_url: orders_base_url.unwrap_or_else(|| AX_ORDERS_URL.to_string()),
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                Self::rate_limiter_quotas(),
                Some(*AX_REST_QUOTA),
                Some(timeout_secs),
                proxy_url,
            )
            .map_err(|e| AxHttpError::NetworkError(format!("Failed to create HTTP client: {e}")))?,
            credential: None,
            session_token: RwLock::new(None),
            retry_manager,
            cancellation_token: RwLock::new(CancellationToken::new()),
        })
    }

    /// Creates a new [`AxRawHttpClient`] configured with credentials.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    #[expect(clippy::too_many_arguments)]
    pub fn with_credentials(
        api_key: String,
        api_secret: String,
        base_url: Option<String>,
        orders_base_url: Option<String>,
        timeout_secs: u64,
        max_retries: u32,
        retry_delay_ms: u64,
        retry_delay_max_ms: u64,
        proxy_url: Option<String>,
    ) -> Result<Self, AxHttpError> {
        let retry_config = RetryConfig {
            max_retries,
            initial_delay_ms: retry_delay_ms,
            max_delay_ms: retry_delay_max_ms,
            backoff_factor: 2.0,
            jitter_ms: 1000,
            operation_timeout_ms: Some(60_000),
            immediate_first: false,
            max_elapsed_ms: Some(180_000),
        };

        let retry_manager = RetryManager::new(retry_config);

        Ok(Self {
            base_url: base_url.unwrap_or_else(|| AX_HTTP_URL.to_string()),
            orders_base_url: orders_base_url.unwrap_or_else(|| AX_ORDERS_URL.to_string()),
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                Self::rate_limiter_quotas(),
                Some(*AX_REST_QUOTA),
                Some(timeout_secs),
                proxy_url,
            )
            .map_err(|e| AxHttpError::NetworkError(format!("Failed to create HTTP client: {e}")))?,
            credential: Some(Credential::new(api_key, api_secret)),
            session_token: RwLock::new(None),
            retry_manager,
            cancellation_token: RwLock::new(CancellationToken::new()),
        })
    }

    /// Sets the session token for authenticated requests.
    ///
    /// The session token is obtained through the login flow and used for bearer token authentication.
    ///
    /// # Panics
    ///
    /// Panics if the internal lock is poisoned (indicates a panic in another thread).
    pub fn set_session_token(&self, token: String) {
        // Lock poisoning indicates a panic in another thread, which is fatal
        *self.session_token.write().expect("Lock poisoned") = Some(token);
    }

    fn default_headers() -> HashMap<String, String> {
        HashMap::from([
            (USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string()),
            ("Accept".to_string(), "application/json".to_string()),
        ])
    }

    fn rate_limiter_quotas() -> Vec<(String, Quota)> {
        vec![(AX_GLOBAL_RATE_KEY.to_string(), *AX_REST_QUOTA)]
    }

    fn rate_limit_keys(endpoint: &str) -> Vec<String> {
        let normalized = endpoint.split('?').next().unwrap_or(endpoint);
        let route = format!("architect:{normalized}");

        vec![AX_GLOBAL_RATE_KEY.to_string(), route]
    }

    fn auth_headers(&self) -> Result<HashMap<String, String>, AxHttpError> {
        // Lock poisoning indicates a panic in another thread, which is fatal
        let guard = self.session_token.read().expect("Lock poisoned");
        let session_token = guard.as_ref().ok_or(AxHttpError::MissingSessionToken)?;

        let mut headers = HashMap::new();
        headers.insert(
            "Authorization".to_string(),
            format!("Bearer {session_token}"),
        );

        Ok(headers)
    }

    async fn send_request<T: DeserializeOwned, P: Serialize>(
        &self,
        method: Method,
        endpoint: &str,
        params: Option<&P>,
        body: Option<Vec<u8>>,
        authenticate: bool,
    ) -> Result<T, AxHttpError> {
        self.send_request_to_url(&self.base_url, method, endpoint, params, body, authenticate)
            .await
    }

    async fn send_request_to_url<T: DeserializeOwned, P: Serialize>(
        &self,
        base_url: &str,
        method: Method,
        endpoint: &str,
        params: Option<&P>,
        body: Option<Vec<u8>>,
        authenticate: bool,
    ) -> Result<T, AxHttpError> {
        let endpoint = endpoint.to_string();
        let url = format!("{base_url}{endpoint}");

        let params_str = if method == Method::GET || method == Method::DELETE {
            params
                .map(serde_urlencoded::to_string)
                .transpose()
                .map_err(|e| AxHttpError::JsonError(format!("Failed to serialize params: {e}")))?
        } else {
            None
        };

        let operation = || {
            let url = url.clone();
            let method = method.clone();
            let endpoint = endpoint.clone();
            let params_str = params_str.clone();
            let body = body.clone();

            async move {
                let mut headers = Self::default_headers();

                if authenticate {
                    let auth_headers = self.auth_headers()?;
                    headers.extend(auth_headers);
                }

                if body.is_some() {
                    headers.insert("Content-Type".to_string(), "application/json".to_string());
                }

                let full_url = if let Some(ref query) = params_str {
                    if query.is_empty() {
                        url
                    } else {
                        format!("{url}?{query}")
                    }
                } else {
                    url
                };

                let rate_limit_keys = Self::rate_limit_keys(&endpoint);

                let response = self
                    .client
                    .request(
                        method,
                        full_url,
                        None,
                        Some(headers),
                        body,
                        None,
                        Some(rate_limit_keys),
                    )
                    .await?;

                let status = response.status;
                let response_body = String::from_utf8_lossy(&response.body).to_string();

                if !status.is_success() {
                    return Err(AxHttpError::UnexpectedStatus {
                        status: status.as_u16(),
                        body: response_body,
                    });
                }

                serde_json::from_str(&response_body).map_err(|e| {
                    AxHttpError::JsonError(format!(
                        "Failed to deserialize response: {e}\nBody: {response_body}"
                    ))
                })
            }
        };

        // Only retry idempotent methods to avoid duplicate orders/cancels
        let is_idempotent = matches!(method, Method::GET | Method::HEAD | Method::OPTIONS);
        let should_retry = |error: &AxHttpError| -> bool { is_idempotent && error.is_retryable() };

        let create_error = |msg: String| -> AxHttpError {
            if msg == "canceled" {
                AxHttpError::Canceled("Adapter disconnecting or shutting down".to_string())
            } else {
                AxHttpError::NetworkError(msg)
            }
        };

        let cancel_token = self
            .cancellation_token
            .read()
            .expect("Lock poisoned")
            .clone();

        self.retry_manager
            .execute_with_retry_with_cancel(
                endpoint.as_str(),
                operation,
                should_retry,
                create_error,
                &cancel_token,
            )
            .await
    }

    /// Fetches the current authenticated user information.
    ///
    /// # Endpoint
    /// `GET /whoami`
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_whoami(&self) -> Result<AxWhoAmI, AxHttpError> {
        self.send_request::<AxWhoAmI, ()>(Method::GET, "/whoami", None, None, true)
            .await
    }

    /// Fetches all available instruments.
    ///
    /// # Endpoint
    /// `GET /instruments`
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_instruments(&self) -> Result<AxInstrumentsResponse, AxHttpError> {
        self.send_request::<AxInstrumentsResponse, ()>(
            Method::GET,
            "/instruments",
            None,
            None,
            false,
        )
        .await
    }

    /// Fetches all account balances for the authenticated user.
    ///
    /// # Endpoint
    /// `GET /balances`
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_balances(&self) -> Result<AxBalancesResponse, AxHttpError> {
        self.send_request::<AxBalancesResponse, ()>(Method::GET, "/balances", None, None, true)
            .await
    }

    /// Fetches all open positions for the authenticated user.
    ///
    /// # Endpoint
    /// `GET /positions`
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_positions(&self) -> Result<AxPositionsResponse, AxHttpError> {
        self.send_request::<AxPositionsResponse, ()>(Method::GET, "/positions", None, None, true)
            .await
    }

    /// Fetches all tickers.
    ///
    /// # Endpoint
    /// `GET /tickers`
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_tickers(&self) -> Result<AxTickersResponse, AxHttpError> {
        self.send_request::<AxTickersResponse, ()>(Method::GET, "/tickers", None, None, true)
            .await
    }

    /// Fetches a single ticker by symbol.
    ///
    /// # Endpoint
    /// `GET /ticker?symbol=<symbol>`
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_ticker(&self, symbol: Ustr) -> Result<AxTicker, AxHttpError> {
        let params = GetTickerParams::new(symbol);
        self.send_request::<AxTicker, _>(Method::GET, "/ticker", Some(&params), None, true)
            .await
    }

    /// Fetches a single instrument by symbol.
    ///
    /// # Endpoint
    /// `GET /instrument?symbol=<symbol>`
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_instrument(&self, symbol: Ustr) -> Result<AxInstrument, AxHttpError> {
        let params = GetInstrumentParams::new(symbol);
        self.send_request::<AxInstrument, _>(Method::GET, "/instrument", Some(&params), None, false)
            .await
    }

    /// Authenticates using API key and secret to obtain a session token.
    ///
    /// # Endpoint
    /// `POST /authenticate`
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn authenticate(
        &self,
        api_key: &str,
        api_secret: &str,
        expiration_seconds: i32,
    ) -> Result<AxAuthenticateResponse, AxHttpError> {
        let request = AuthenticateApiKeyRequest::new(api_key, api_secret, expiration_seconds);

        let body = serde_json::to_vec(&request)
            .map_err(|e| AxHttpError::JsonError(format!("Failed to serialize request: {e}")))?;

        self.send_request::<AxAuthenticateResponse, ()>(
            Method::POST,
            "/authenticate",
            None,
            Some(body),
            false,
        )
        .await
    }

    /// Authenticates using stored credentials or environment variables.
    ///
    /// # Credential Resolution
    ///
    /// Credentials are resolved in the following order:
    /// 1. Stored credentials (from `with_credentials` constructor)
    /// 2. Environment variables (`AX_API_KEY` and `AX_API_SECRET`)
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No credentials are available from either source
    /// - The HTTP request fails
    /// - The credentials are invalid
    pub async fn authenticate_auto(
        &self,
        expiration_seconds: i32,
    ) -> Result<AxAuthenticateResponse, AxHttpError> {
        let (api_key, api_secret) = self
            .resolve_credentials()
            .ok_or(AxHttpError::MissingCredentials)?;

        self.authenticate(&api_key, &api_secret, expiration_seconds)
            .await
    }

    fn resolve_credentials(&self) -> Option<(String, String)> {
        if let Some(cred) = &self.credential {
            return Some((cred.api_key().to_string(), cred.api_secret().to_string()));
        }

        let cred = Credential::resolve(None, None)?;
        Some((cred.api_key().to_string(), cred.api_secret().to_string()))
    }

    /// Places a new order.
    ///
    /// # Endpoint
    /// `POST /place_order` (orders base URL)
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn place_order(
        &self,
        request: &PlaceOrderRequest,
    ) -> Result<AxPlaceOrderResponse, AxHttpError> {
        let body = serde_json::to_vec(request)
            .map_err(|e| AxHttpError::JsonError(format!("Failed to serialize request: {e}")))?;
        self.send_request_to_url::<AxPlaceOrderResponse, ()>(
            &self.orders_base_url,
            Method::POST,
            "/place_order",
            None,
            Some(body),
            true,
        )
        .await
    }

    /// Cancels an existing order.
    ///
    /// # Endpoint
    /// `POST /cancel_order` (orders base URL)
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn cancel_order(&self, order_id: &str) -> Result<AxCancelOrderResponse, AxHttpError> {
        let request = CancelOrderRequest::new(order_id);
        let body = serde_json::to_vec(&request)
            .map_err(|e| AxHttpError::JsonError(format!("Failed to serialize request: {e}")))?;
        self.send_request_to_url::<AxCancelOrderResponse, ()>(
            &self.orders_base_url,
            Method::POST,
            "/cancel_order",
            None,
            Some(body),
            true,
        )
        .await
    }

    /// Replaces (amends) an existing order.
    ///
    /// The exchange cancels the original order and creates a new one with the
    /// updated fields. Unspecified optional fields inherit from the original.
    ///
    /// # Endpoint
    /// `POST /replace_order` (orders base URL)
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn replace_order(
        &self,
        request: &ReplaceOrderRequest,
    ) -> Result<AxReplaceOrderResponse, AxHttpError> {
        let body = serde_json::to_vec(request)
            .map_err(|e| AxHttpError::JsonError(format!("Failed to serialize request: {e}")))?;
        self.send_request_to_url::<AxReplaceOrderResponse, ()>(
            &self.orders_base_url,
            Method::POST,
            "/replace_order",
            None,
            Some(body),
            true,
        )
        .await
    }

    /// Cancels all open orders, optionally filtered by symbol or venue.
    ///
    /// # Endpoint
    /// `POST /cancel_all_orders` (orders base URL)
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn cancel_all_orders(
        &self,
        request: &CancelAllOrdersRequest,
    ) -> Result<AxCancelAllOrdersResponse, AxHttpError> {
        let body = serde_json::to_vec(request)
            .map_err(|e| AxHttpError::JsonError(format!("Failed to serialize request: {e}")))?;
        self.send_request_to_url::<AxCancelAllOrdersResponse, ()>(
            &self.orders_base_url,
            Method::POST,
            "/cancel_all_orders",
            None,
            Some(body),
            true,
        )
        .await
    }

    /// Fetches all open orders.
    ///
    /// # Endpoint
    /// `GET /open_orders` (orders base URL)
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_open_orders(&self) -> Result<AxOpenOrdersResponse, AxHttpError> {
        self.send_request_to_url::<AxOpenOrdersResponse, ()>(
            &self.orders_base_url,
            Method::GET,
            "/open_orders",
            None,
            None,
            true,
        )
        .await
    }

    /// Fetches all fills/trades.
    ///
    /// # Endpoint
    /// `GET /fills`
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_fills(&self) -> Result<AxFillsResponse, AxHttpError> {
        self.send_request::<AxFillsResponse, ()>(Method::GET, "/fills", None, None, true)
            .await
    }

    /// Fetches historical candles.
    ///
    /// # Endpoint
    /// `GET /candles`
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_candles(
        &self,
        symbol: Ustr,
        start_timestamp_ns: i64,
        end_timestamp_ns: i64,
        candle_width: AxCandleWidth,
    ) -> Result<AxCandlesResponse, AxHttpError> {
        let params =
            GetCandlesParams::new(symbol, start_timestamp_ns, end_timestamp_ns, candle_width);
        self.send_request::<AxCandlesResponse, _>(
            Method::GET,
            "/candles",
            Some(&params),
            None,
            true,
        )
        .await
    }

    /// Fetches the current (incomplete) candle.
    ///
    /// # Endpoint
    /// `GET /candles/current`
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_current_candle(
        &self,
        symbol: Ustr,
        candle_width: AxCandleWidth,
    ) -> Result<AxCandle, AxHttpError> {
        let params = GetCandleParams::new(symbol, candle_width);
        let response = self
            .send_request::<AxCandleResponse, _>(
                Method::GET,
                "/candles/current",
                Some(&params),
                None,
                true,
            )
            .await?;
        Ok(response.candle)
    }

    /// Fetches the last completed candle.
    ///
    /// # Endpoint
    /// `GET /candles/last`
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_last_candle(
        &self,
        symbol: Ustr,
        candle_width: AxCandleWidth,
    ) -> Result<AxCandle, AxHttpError> {
        let params = GetCandleParams::new(symbol, candle_width);
        let response = self
            .send_request::<AxCandleResponse, _>(
                Method::GET,
                "/candles/last",
                Some(&params),
                None,
                true,
            )
            .await?;
        Ok(response.candle)
    }

    /// Fetches funding rates for a symbol.
    ///
    /// # Endpoint
    /// `GET /funding-rates`
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_funding_rates(
        &self,
        symbol: Ustr,
        start_timestamp_ns: i64,
        end_timestamp_ns: i64,
    ) -> Result<AxFundingRatesResponse, AxHttpError> {
        let params = GetFundingRatesParams::new(symbol, start_timestamp_ns, end_timestamp_ns);
        self.send_request::<AxFundingRatesResponse, _>(
            Method::GET,
            "/funding-rates",
            Some(&params),
            None,
            true,
        )
        .await
    }

    /// Fetches the current risk snapshot.
    ///
    /// # Endpoint
    /// `GET /risk-snapshot`
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_risk_snapshot(&self) -> Result<AxRiskSnapshotResponse, AxHttpError> {
        self.send_request::<AxRiskSnapshotResponse, ()>(
            Method::GET,
            "/risk-snapshot",
            None,
            None,
            true,
        )
        .await
    }

    /// Previews an aggressive limit order to get the "take through" price.
    ///
    /// This endpoint calculates the price needed to sweep the order book for a given
    /// quantity, which is used to simulate market orders on AX (which only supports
    /// limit orders natively).
    ///
    /// # Endpoint
    /// `POST /preview-aggressive-limit-order`
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn preview_aggressive_limit_order(
        &self,
        request: &PreviewAggressiveLimitOrderRequest,
    ) -> Result<AxPreviewAggressiveLimitOrderResponse, AxHttpError> {
        let body = serde_json::to_vec(request)
            .map_err(|e| AxHttpError::JsonError(format!("Failed to serialize request: {e}")))?;
        self.send_request::<AxPreviewAggressiveLimitOrderResponse, ()>(
            Method::POST,
            "/preview-aggressive-limit-order",
            None,
            Some(body),
            true,
        )
        .await
    }

    /// Fetches transactions filtered by type.
    ///
    /// # Endpoint
    /// `GET /transactions`
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_transactions(
        &self,
        transaction_types: Vec<String>,
    ) -> Result<AxTransactionsResponse, AxHttpError> {
        let params = GetTransactionsParams::new(transaction_types);
        self.send_request::<AxTransactionsResponse, _>(
            Method::GET,
            "/transactions",
            Some(&params),
            None,
            true,
        )
        .await
    }

    /// Fetches recent trades for a symbol.
    ///
    /// # Endpoint
    /// `GET /trades`
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_trades(
        &self,
        symbol: Ustr,
        limit: Option<i32>,
    ) -> Result<AxTradesResponse, AxHttpError> {
        let params = GetTradesParams::new(symbol, limit);
        self.send_request::<AxTradesResponse, _>(Method::GET, "/trades", Some(&params), None, true)
            .await
    }

    /// Fetches an order book snapshot for a symbol.
    ///
    /// # Endpoint
    /// `GET /book`
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_book(
        &self,
        symbol: Ustr,
        level: Option<i32>,
    ) -> Result<AxBookResponse, AxHttpError> {
        let params = GetBookParams::new(symbol, level);
        self.send_request::<AxBookResponse, _>(Method::GET, "/book", Some(&params), None, false)
            .await
    }

    /// Fetches the status of a single order by order ID.
    ///
    /// # Endpoint
    /// `GET /order-status` (orders base URL)
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_order_status_by_id(
        &self,
        order_id: &str,
    ) -> Result<AxOrderStatusQueryResponse, AxHttpError> {
        let params = GetOrderStatusParams::by_order_id(order_id);
        self.send_request_to_url::<AxOrderStatusQueryResponse, _>(
            &self.orders_base_url,
            Method::GET,
            "/order-status",
            Some(&params),
            None,
            true,
        )
        .await
    }

    /// Fetches the status of a single order by client order ID.
    ///
    /// # Endpoint
    /// `GET /order-status` (orders base URL)
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_order_status_by_cid(
        &self,
        client_order_id: u64,
    ) -> Result<AxOrderStatusQueryResponse, AxHttpError> {
        let params = GetOrderStatusParams::by_client_order_id(client_order_id);
        self.send_request_to_url::<AxOrderStatusQueryResponse, _>(
            &self.orders_base_url,
            Method::GET,
            "/order-status",
            Some(&params),
            None,
            true,
        )
        .await
    }

    /// Fetches historical orders with optional filters.
    ///
    /// # Endpoint
    /// `GET /orders` (orders base URL)
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_orders(
        &self,
        params: &GetOrdersParams,
    ) -> Result<AxOrdersResponse, AxHttpError> {
        self.send_request_to_url::<AxOrdersResponse, _>(
            &self.orders_base_url,
            Method::GET,
            "/orders",
            Some(params),
            None,
            true,
        )
        .await
    }

    /// Checks the initial margin requirement for a proposed order.
    ///
    /// # Endpoint
    /// `POST /initial-margin-requirement` (orders base URL)
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn check_initial_margin(
        &self,
        request: &PlaceOrderRequest,
    ) -> Result<AxInitialMarginRequirementResponse, AxHttpError> {
        let body = serde_json::to_vec(request)
            .map_err(|e| AxHttpError::JsonError(format!("Failed to serialize request: {e}")))?;
        self.send_request_to_url::<AxInitialMarginRequirementResponse, ()>(
            &self.orders_base_url,
            Method::POST,
            "/initial-margin-requirement",
            None,
            Some(body),
            true,
        )
        .await
    }
}

/// High-level HTTP client for the Ax REST API.
///
/// This client wraps the underlying [`AxRawHttpClient`] to provide a convenient
/// interface for Python bindings and instrument caching.
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.architect",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.architect_ax")
)]
pub struct AxHttpClient {
    pub(crate) inner: Arc<AxRawHttpClient>,
    pub(crate) instruments_cache: Arc<AtomicMap<Ustr, InstrumentAny>>,
    clock: &'static AtomicTime,
    cache_initialized: Arc<AtomicBool>,
}

impl Clone for AxHttpClient {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            instruments_cache: self.instruments_cache.clone(),
            cache_initialized: self.cache_initialized.clone(),
            clock: self.clock,
        }
    }
}

impl Default for AxHttpClient {
    fn default() -> Self {
        Self::new(None, None, 60, 3, 1000, 10_000, None)
            .expect("Failed to create default AxHttpClient")
    }
}

impl AxHttpClient {
    /// Creates a new [`AxHttpClient`] using the default Ax HTTP URL.
    ///
    /// # Errors
    ///
    /// Returns an error if the retry manager cannot be created.
    pub fn new(
        base_url: Option<String>,
        orders_base_url: Option<String>,
        timeout_secs: u64,
        max_retries: u32,
        retry_delay_ms: u64,
        retry_delay_max_ms: u64,
        proxy_url: Option<String>,
    ) -> Result<Self, AxHttpError> {
        Ok(Self {
            inner: Arc::new(AxRawHttpClient::new(
                base_url,
                orders_base_url,
                timeout_secs,
                max_retries,
                retry_delay_ms,
                retry_delay_max_ms,
                proxy_url,
            )?),
            instruments_cache: Arc::new(AtomicMap::new()),
            cache_initialized: Arc::new(AtomicBool::new(false)),
            clock: get_atomic_clock_realtime(),
        })
    }

    /// Creates a new [`AxHttpClient`] configured with credentials.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    #[expect(clippy::too_many_arguments)]
    pub fn with_credentials(
        api_key: String,
        api_secret: String,
        base_url: Option<String>,
        orders_base_url: Option<String>,
        timeout_secs: u64,
        max_retries: u32,
        retry_delay_ms: u64,
        retry_delay_max_ms: u64,
        proxy_url: Option<String>,
    ) -> Result<Self, AxHttpError> {
        Ok(Self {
            inner: Arc::new(AxRawHttpClient::with_credentials(
                api_key,
                api_secret,
                base_url,
                orders_base_url,
                timeout_secs,
                max_retries,
                retry_delay_ms,
                retry_delay_max_ms,
                proxy_url,
            )?),
            instruments_cache: Arc::new(AtomicMap::new()),
            cache_initialized: Arc::new(AtomicBool::new(false)),
            clock: get_atomic_clock_realtime(),
        })
    }

    /// Returns the base URL for this client.
    #[must_use]
    pub fn base_url(&self) -> &str {
        self.inner.base_url()
    }

    /// Returns a masked version of the API key for logging purposes.
    #[must_use]
    pub fn api_key_masked(&self) -> String {
        self.inner.api_key_masked()
    }

    /// Cancel all pending HTTP requests.
    pub fn cancel_all_requests(&self) {
        self.inner.cancel_all_requests();
    }

    /// Replaces the cancelled token so new requests can proceed after reconnect.
    pub fn reset_cancellation_token(&self) {
        self.inner.reset_cancellation_token();
    }

    /// Sets the session token for authenticated requests.
    ///
    /// The session token is obtained through the login flow and used for bearer token authentication.
    pub fn set_session_token(&self, token: String) {
        self.inner.set_session_token(token);
    }

    /// Generates a timestamp for initialization.
    fn generate_ts_init(&self) -> UnixNanos {
        self.clock.get_time_ns()
    }

    /// Checks if the client is initialized.
    ///
    /// The client is considered initialized if any instruments have been cached from the venue.
    #[must_use]
    pub fn is_initialized(&self) -> bool {
        self.cache_initialized.load(Ordering::Acquire)
    }

    /// Returns a snapshot of all instrument symbols currently held in the internal cache.
    #[must_use]
    pub fn get_cached_symbols(&self) -> Vec<String> {
        self.instruments_cache
            .load()
            .keys()
            .map(|k| k.to_string())
            .collect()
    }

    /// Caches multiple instruments.
    ///
    /// Any existing instruments with the same symbols will be replaced.
    pub fn cache_instruments(&self, instruments: &[InstrumentAny]) {
        self.instruments_cache.rcu(|m| {
            for inst in instruments {
                m.insert(inst.raw_symbol().inner(), inst.clone());
            }
        });
        self.cache_initialized.store(true, Ordering::Release);
    }

    /// Caches a single instrument.
    ///
    /// Any existing instrument with the same symbol will be replaced.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        self.instruments_cache
            .insert(instrument.raw_symbol().inner(), instrument);
        self.cache_initialized.store(true, Ordering::Release);
    }

    /// Authenticates with Ax using API credentials.
    ///
    /// On success, the session token is automatically stored for subsequent authenticated requests.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or credentials are invalid.
    pub async fn authenticate(
        &self,
        api_key: &str,
        api_secret: &str,
        expiration_seconds: i32,
    ) -> Result<String, AxHttpError> {
        let resp = self
            .inner
            .authenticate(api_key, api_secret, expiration_seconds)
            .await?;
        self.inner.set_session_token(resp.token.clone());
        Ok(resp.token)
    }

    /// Authenticates using stored credentials or environment variables.
    ///
    /// # Credential Resolution
    ///
    /// Credentials are resolved in the following order:
    /// 1. Stored credentials (from `with_credentials` constructor)
    /// 2. Environment variables (`AX_API_KEY` and `AX_API_SECRET`)
    ///
    /// On success, the session token is automatically stored for subsequent authenticated requests.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No credentials are available from either source
    /// - The HTTP request fails
    /// - The credentials are invalid
    pub async fn authenticate_auto(&self, expiration_seconds: i32) -> Result<String, AxHttpError> {
        let resp = self.inner.authenticate_auto(expiration_seconds).await?;
        self.inner.set_session_token(resp.token.clone());
        Ok(resp.token)
    }

    /// Gets an instrument from the cache by symbol.
    pub fn get_instrument(&self, symbol: &Ustr) -> Option<InstrumentAny> {
        self.instruments_cache.get_cloned(symbol)
    }

    /// Requests all instruments from Ax.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or instrument parsing fails.
    pub async fn request_instruments(
        &self,
        maker_fee: Option<Decimal>,
        taker_fee: Option<Decimal>,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let resp = self
            .inner
            .get_instruments()
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let maker_fee = maker_fee.unwrap_or(Decimal::ZERO);
        let taker_fee = taker_fee.unwrap_or(Decimal::ZERO);
        let ts_init = self.generate_ts_init();

        let mut instruments: Vec<InstrumentAny> = Vec::new();
        for inst in &resp.instruments {
            if inst.state == AxInstrumentState::Delisted {
                log::debug!("Skipping delisted instrument: {}", inst.symbol);
                continue;
            }

            // Skip test instruments (not real tradable products)
            if inst.symbol.as_str().starts_with("TEST") {
                log::debug!("Skipping test instrument: {}", inst.symbol);
                continue;
            }

            match parse_perp_instrument(inst, maker_fee, taker_fee, ts_init, ts_init) {
                Ok(instrument) => instruments.push(instrument),
                Err(e) => {
                    log::warn!("Failed to parse instrument {}: {e}", inst.symbol);
                }
            }
        }

        Ok(instruments)
    }

    /// Requests a single instrument from Ax by symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or instrument parsing fails.
    pub async fn request_instrument(
        &self,
        symbol: Ustr,
        maker_fee: Option<Decimal>,
        taker_fee: Option<Decimal>,
    ) -> anyhow::Result<InstrumentAny> {
        let resp = self
            .inner
            .get_instrument(symbol)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let maker_fee = maker_fee.unwrap_or(Decimal::ZERO);
        let taker_fee = taker_fee.unwrap_or(Decimal::ZERO);
        let ts_init = self.generate_ts_init();

        parse_perp_instrument(&resp, maker_fee, taker_fee, ts_init, ts_init)
    }

    /// Requests an order book snapshot from Ax and builds a Nautilus [`OrderBook`].
    ///
    /// Requires the instrument to be cached.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The instrument is not found in the cache.
    /// - The HTTP request fails.
    pub async fn request_book_snapshot(
        &self,
        symbol: Ustr,
        depth: Option<usize>,
    ) -> anyhow::Result<OrderBook> {
        let instrument = self
            .get_instrument(&symbol)
            .ok_or_else(|| anyhow::anyhow!("Instrument {symbol} not found in cache"))?;

        let resp = self
            .inner
            .get_book(symbol, Some(2))
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let instrument_id = instrument.id();
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();
        let ts_event = UnixNanos::from(resp.book.ts as u64 * 1_000_000_000 + resp.book.tn as u64);

        for (i, level) in resp.book.b.iter().enumerate() {
            if depth.is_some_and(|d| i >= d) {
                break;
            }
            let price = Price::from_decimal_dp(level.p, price_precision)
                .unwrap_or_else(|_| Price::from(level.p.to_string().as_str()));
            let size = Quantity::new(level.q as f64, size_precision);
            let order = BookOrder::new(OrderSide::Buy, price, size, i as u64);
            book.add(order, 0, i as u64, ts_event);
        }

        let bids_len = resp.book.b.len();
        for (i, level) in resp.book.a.iter().enumerate() {
            if depth.is_some_and(|d| i >= d) {
                break;
            }
            let price = Price::from_decimal_dp(level.p, price_precision)
                .unwrap_or_else(|_| Price::from(level.p.to_string().as_str()));
            let size = Quantity::new(level.q as f64, size_precision);
            let order = BookOrder::new(OrderSide::Sell, price, size, (bids_len + i) as u64);
            book.add(order, 0, (bids_len + i) as u64, ts_event);
        }

        Ok(book)
    }

    /// Requests recent trades from Ax and parses them to Nautilus [`TradeTick`].
    ///
    /// The AX trades endpoint does not accept time range parameters, so
    /// `start` and `end` are applied as client-side filters after fetching.
    ///
    /// Requires the instrument to be cached.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The instrument is not found in the cache.
    /// - The HTTP request fails.
    /// - Trade parsing fails.
    pub async fn request_trade_ticks(
        &self,
        symbol: Ustr,
        limit: Option<i32>,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<Vec<TradeTick>> {
        let instrument = self
            .get_instrument(&symbol)
            .ok_or_else(|| anyhow::anyhow!("Instrument {symbol} not found in cache"))?;

        let resp = self
            .inner
            .get_trades(symbol, limit)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let ts_init = self.generate_ts_init();
        let mut ticks = Vec::with_capacity(resp.trades.len());

        for trade in &resp.trades {
            match parse_trade_tick(trade, &instrument, ts_init) {
                Ok(tick) => {
                    if start.is_some_and(|s| tick.ts_event < s) {
                        continue;
                    }

                    if end.is_some_and(|e| tick.ts_event > e) {
                        continue;
                    }
                    ticks.push(tick);
                }
                Err(e) => {
                    log::warn!("Failed to parse trade for {symbol}: {e}");
                }
            }
        }

        Ok(ticks)
    }

    /// Requests historical bars from Ax and parses them to Nautilus Bar types.
    ///
    /// Requires the instrument to be cached (call `request_instruments` first).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The instrument is not found in the cache.
    /// - The HTTP request fails.
    /// - Bar parsing fails.
    pub async fn request_bars(
        &self,
        symbol: Ustr,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        width: AxCandleWidth,
    ) -> anyhow::Result<Vec<Bar>> {
        let instrument = self
            .get_instrument(&symbol)
            .ok_or_else(|| anyhow::anyhow!("Instrument {symbol} not found in cache"))?;

        let start_ns = start.and_then(|dt| dt.timestamp_nanos_opt()).unwrap_or(0);
        let end_ns = end
            .and_then(|dt| dt.timestamp_nanos_opt())
            .unwrap_or_else(|| self.generate_ts_init().as_i64());
        let resp = self
            .inner
            .get_candles(symbol, start_ns, end_ns, width)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let ts_init = self.generate_ts_init();
        let mut bars = Vec::with_capacity(resp.candles.len());

        for candle in &resp.candles {
            match parse_bar(candle, &instrument, ts_init) {
                Ok(bar) => bars.push(bar),
                Err(e) => {
                    log::warn!("Failed to parse bar for {symbol}: {e}");
                }
            }
        }

        Ok(bars)
    }

    /// Requests funding rates from Ax and parses them to Nautilus types.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails.
    pub async fn request_funding_rates(
        &self,
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
    ) -> Result<Vec<FundingRateUpdate>, AxHttpError> {
        let symbol = instrument_id.symbol.inner();
        let start_ns = start.and_then(|dt| dt.timestamp_nanos_opt()).unwrap_or(0);
        let end_ns = end
            .and_then(|dt| dt.timestamp_nanos_opt())
            .unwrap_or_else(|| self.generate_ts_init().as_i64());
        let response = self
            .inner
            .get_funding_rates(symbol, start_ns, end_ns)
            .await?;

        let ts_init = self.generate_ts_init();
        let funding_rates = response
            .funding_rates
            .iter()
            .map(|r| parse_funding_rate(r, instrument_id, ts_init))
            .collect::<anyhow::Result<Vec<_>>>()
            .map_err(|e| AxHttpError::from(e.to_string()))?;

        Ok(funding_rates)
    }

    /// Requests account state from Ax and parses to a Nautilus [`AccountState`].
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or parsing fails.
    pub async fn request_account_state(
        &self,
        account_id: AccountId,
    ) -> anyhow::Result<AccountState> {
        let response = self
            .inner
            .get_balances()
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let ts_init = self.generate_ts_init();
        parse_account_state(&response, account_id, ts_init, ts_init)
    }

    /// Checks the initial margin requirement for a proposed order.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails.
    pub async fn check_initial_margin(
        &self,
        request: &PlaceOrderRequest,
    ) -> anyhow::Result<Decimal> {
        let resp = self
            .inner
            .check_initial_margin(request)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        Ok(resp.im)
    }

    /// Queries a single order by venue order ID or client order ID using the
    /// dedicated `/order-status` endpoint, which works for any order state.
    ///
    /// The caller must supply `order_side`, `order_type`, and `time_in_force`
    /// because the endpoint does not return these fields.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Neither `venue_order_id` nor `client_order_id` is provided.
    /// - The HTTP request fails.
    #[expect(clippy::too_many_arguments)]
    pub async fn request_order_status(
        &self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
        order_side: OrderSide,
        order_type: OrderType,
        time_in_force: TimeInForce,
    ) -> anyhow::Result<OrderStatusReport> {
        let resp = if let Some(ref voi) = venue_order_id {
            self.inner.get_order_status_by_id(voi.as_str()).await
        } else if let Some(ref coid) = client_order_id {
            let cid = client_order_id_to_cid(coid);
            self.inner.get_order_status_by_cid(cid).await
        } else {
            anyhow::bail!("Either venue_order_id or client_order_id must be provided")
        }
        .map_err(|e| anyhow::anyhow!(e))?;

        let detail = resp.status;
        let size_precision = self
            .get_instrument(&detail.symbol)
            .map_or(0, |i| i.size_precision());

        let voi = VenueOrderId::new(&detail.order_id);
        let order_status = detail.state.into();
        let filled = detail.filled_quantity.unwrap_or(0);
        let remaining = detail.remaining_quantity.unwrap_or(0);
        let quantity = Quantity::new((filled + remaining) as f64, size_precision);
        let filled_qty = Quantity::new(filled as f64, size_precision);
        let ts_init = self.generate_ts_init();

        let resolved_coid = client_order_id.or_else(|| detail.clord_id.map(cid_to_client_order_id));

        Ok(OrderStatusReport::new(
            account_id,
            instrument_id,
            resolved_coid,
            voi,
            order_side,
            order_type,
            time_in_force,
            order_status,
            quantity,
            filled_qty,
            ts_init,
            ts_init,
            ts_init,
            Some(UUID4::new()),
        ))
    }

    /// Requests open orders from Ax and parses them to Nautilus [`OrderStatusReport`].
    ///
    /// Requires instruments to be cached for parsing order details.
    ///
    /// The `cid_resolver` parameter is an optional function that resolves a `cid` (u64)
    /// to a `ClientOrderId`. This is needed for correlating orders submitted via WebSocket.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The HTTP request fails.
    /// - An order's instrument is not found in the cache.
    /// - Order parsing fails.
    pub async fn request_order_status_reports<F>(
        &self,
        account_id: AccountId,
        cid_resolver: Option<F>,
    ) -> anyhow::Result<Vec<OrderStatusReport>>
    where
        F: Fn(u64) -> Option<ClientOrderId>,
    {
        let response = self
            .inner
            .get_open_orders()
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let ts_init = self.generate_ts_init();
        let mut reports = Vec::with_capacity(response.orders.len());

        for order in &response.orders {
            let instrument = self
                .get_instrument(&order.s)
                .ok_or_else(|| anyhow::anyhow!("Instrument {} not found in cache", order.s))?;

            match parse_order_status_report(
                order,
                account_id,
                &instrument,
                ts_init,
                cid_resolver.as_ref(),
            ) {
                Ok(report) => reports.push(report),
                Err(e) => {
                    log::warn!("Failed to parse order {}: {e}", order.oid);
                }
            }
        }

        Ok(reports)
    }

    /// Requests fills from Ax and parses them to Nautilus [`FillReport`].
    ///
    /// Requires instruments to be cached for parsing fill details.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The HTTP request fails.
    /// - A fill's instrument is not found in the cache.
    /// - Fill parsing fails.
    pub async fn request_fill_reports(
        &self,
        account_id: AccountId,
    ) -> anyhow::Result<Vec<FillReport>> {
        let response = self
            .inner
            .get_fills()
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let ts_init = self.generate_ts_init();
        let mut reports = Vec::with_capacity(response.fills.len());

        for fill in &response.fills {
            let instrument = self
                .get_instrument(&fill.symbol)
                .ok_or_else(|| anyhow::anyhow!("Instrument {} not found in cache", fill.symbol))?;

            match parse_fill_report(fill, account_id, &instrument, ts_init) {
                Ok(report) => reports.push(report),
                Err(e) => {
                    log::warn!("Failed to parse fill {}: {e}", fill.trade_id);
                }
            }
        }

        Ok(reports)
    }

    /// Requests positions from Ax and parses them to Nautilus [`PositionStatusReport`].
    ///
    /// Requires instruments to be cached for parsing position details.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The HTTP request fails.
    /// - A position's instrument is not found in the cache.
    /// - Position parsing fails.
    pub async fn request_position_reports(
        &self,
        account_id: AccountId,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let response = self
            .inner
            .get_positions()
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let ts_init = self.generate_ts_init();
        let mut reports = Vec::with_capacity(response.positions.len());

        for position in &response.positions {
            // Skip flat positions (zero quantity)
            if position.signed_quantity == 0 {
                continue;
            }

            let instrument = self.get_instrument(&position.symbol).ok_or_else(|| {
                anyhow::anyhow!("Instrument {} not found in cache", position.symbol)
            })?;

            match parse_position_status_report(position, account_id, &instrument, ts_init) {
                Ok(report) => reports.push(report),
                Err(e) => {
                    log::warn!("Failed to parse position for {}: {e}", position.symbol);
                }
            }
        }

        Ok(reports)
    }

    /// Cancels all open orders for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn cancel_all_orders(&self, instrument_id: InstrumentId) -> Result<(), AxHttpError> {
        let request = CancelAllOrdersRequest::new().with_symbol(instrument_id.symbol.inner());
        self.inner.cancel_all_orders(&request).await?;
        Ok(())
    }
}
