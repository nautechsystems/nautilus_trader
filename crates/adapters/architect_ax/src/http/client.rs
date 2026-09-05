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
    collections::{HashMap, HashSet},
    fmt::Debug,
    num::NonZeroU32,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::Context;
use arc_swap::ArcSwapOption;
use jiff::{Timestamp, civil::Date};
use nautilus_core::{
    AtomicMap, AtomicTime, UUID4, consts::NAUTILUS_USER_AGENT, nanos::UnixNanos,
    string::secret::SecretString, time::get_atomic_clock_realtime,
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
    retry::{RetryConfig, RetryError, RetryManager},
};
use parking_lot::RwLock;
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
        AxCandlesResponse, AxFillsResponse, AxFundingRatesResponse, AxFundingSlotsResponse,
        AxInitialMarginRequirementResponse, AxInstrument, AxInstrumentsResponse,
        AxOpenOrdersResponse, AxOrderStatusQueryResponse, AxOrdersResponse, AxPlaceOrderResponse,
        AxPositionsResponse, AxPreviewAggressiveLimitOrderResponse, AxReplaceOrderResponse,
        AxRiskSnapshotResponse, AxTicker, AxTickerResponse, AxTickersResponse, AxTradesResponse,
        AxTransactionsResponse, AxWhoAmI, CancelAllOrdersRequest, CancelOrderRequest,
        PlaceOrderRequest, PreviewAggressiveLimitOrderRequest, ReplaceOrderRequest,
    },
    parse::{
        parse_account_state, parse_bar, parse_fill_report, parse_funding_rate, parse_instrument,
        parse_order_detail_status_report, parse_order_status_report, parse_position_status_report,
        parse_trade_tick,
    },
    query::{
        GetBookParams, GetCandleParams, GetCandlesParams, GetFillsParams, GetFundingRatesParams,
        GetFundingSlotsParams, GetInstrumentParams, GetOpenOrdersParams, GetOrderStatusParams,
        GetOrdersParams, GetTickerParams, GetTickersParams, GetTradesParams, GetTransactionsParams,
    },
};
use crate::common::{
    consts::{AX_FILLS_MAX_LOOKBACK_DAYS, AX_HTTP_URL, AX_ORDERS_URL},
    credential::Credential,
    enums::{AxCandleWidth, AxInstrumentState},
    parse::{ax_timestamp_stn_to_unix_nanos, cid_to_client_order_id, client_order_id_to_cid},
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
    session_token: RwLock<Option<SecretString>>,
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
        let has_session_token = self.session_token.read().is_some();
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
    pub fn cancel_all_requests(&self) {
        self.cancellation_token.read().cancel();
    }

    /// Replaces the cancelled token so new requests can proceed after reconnect.
    pub fn reset_cancellation_token(&self) {
        *self.cancellation_token.write() = CancellationToken::new();
    }

    /// Get a clone of the current cancellation token.
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation_token.read().clone()
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
            client: HttpClient::builder()
                .headers(Self::default_headers())
                .keyed_quotas(Self::rate_limiter_quotas())
                .default_quota(*AX_REST_QUOTA)
                .timeout_secs(timeout_secs)
                .maybe_proxy_url(proxy_url)
                .build()
                .map_err(|e| {
                    AxHttpError::NetworkError(format!("Failed to create HTTP client: {e}"))
                })?,
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
            client: HttpClient::builder()
                .headers(Self::default_headers())
                .keyed_quotas(Self::rate_limiter_quotas())
                .default_quota(*AX_REST_QUOTA)
                .timeout_secs(timeout_secs)
                .maybe_proxy_url(proxy_url)
                .build()
                .map_err(|e| {
                    AxHttpError::NetworkError(format!("Failed to create HTTP client: {e}"))
                })?,
            credential: Some(Credential::new(api_key, api_secret)),
            session_token: RwLock::new(None),
            retry_manager,
            cancellation_token: RwLock::new(CancellationToken::new()),
        })
    }

    /// Sets the session token for authenticated requests.
    ///
    /// The session token is obtained through the login flow and used for bearer token authentication.
    pub fn set_session_token(&self, token: SecretString) {
        *self.session_token.write() = Some(token);
    }

    pub(crate) fn has_session_token(&self) -> bool {
        self.session_token.read().is_some()
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
        let guard = self.session_token.read();
        let session_token = guard.as_ref().ok_or(AxHttpError::MissingSessionToken)?;

        let mut headers = HashMap::new();
        headers.insert(
            "Authorization".to_string(),
            format!("Bearer {}", session_token.expose_secret()),
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

        let create_error = |error: RetryError| -> AxHttpError {
            match error {
                RetryError::Canceled => {
                    AxHttpError::Canceled("Adapter disconnecting or shutting down".to_string())
                }
                error => AxHttpError::NetworkError(error.to_string()),
            }
        };

        let cancel_token = self.cancellation_token.read().clone();

        self.retry_manager
            .invocation(endpoint.as_str(), operation, should_retry, create_error)
            .cancellation_token(&cancel_token)
            .execute()
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

    /// Fetches tickers with optional pagination and sorting.
    ///
    /// # Endpoint
    /// `GET /tickers`
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_tickers_with_params(
        &self,
        params: &GetTickersParams,
    ) -> Result<AxTickersResponse, AxHttpError> {
        self.send_request::<AxTickersResponse, _>(Method::GET, "/tickers", Some(params), None, true)
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
        self.send_request::<AxTickerResponse, _>(Method::GET, "/ticker", Some(&params), None, true)
            .await
            .map(|response| response.ticker)
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

        self.authenticate(
            api_key.expose_secret(),
            api_secret.expose_secret(),
            expiration_seconds,
        )
        .await
    }

    fn resolve_credentials(&self) -> Option<(SecretString, SecretString)> {
        if let Some(cred) = &self.credential {
            return Some((
                SecretString::from(cred.api_key()),
                SecretString::from(cred.api_secret()),
            ));
        }

        let cred = Credential::resolve(None, None)?;
        Some((
            SecretString::from(cred.api_key()),
            SecretString::from(cred.api_secret()),
        ))
    }

    /// Places a new order.
    ///
    /// # Endpoint
    /// `POST /place-order` (orders base URL)
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
            "/place-order",
            None,
            Some(body),
            true,
        )
        .await
    }

    /// Cancels an existing order.
    ///
    /// # Endpoint
    /// `POST /cancel-order` (orders base URL)
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
            "/cancel-order",
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
    /// `POST /replace-order` (orders base URL)
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
            "/replace-order",
            None,
            Some(body),
            true,
        )
        .await
    }

    /// Cancels all open orders, optionally filtered by account or symbol.
    ///
    /// # Endpoint
    /// `POST /cancel-all-orders` (orders base URL)
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
            "/cancel-all-orders",
            None,
            Some(body),
            true,
        )
        .await
    }

    /// Fetches all open orders.
    ///
    /// # Endpoint
    /// `GET /open-orders` (orders base URL)
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_open_orders(&self) -> Result<AxOpenOrdersResponse, AxHttpError> {
        self.get_open_orders_page(&GetOpenOrdersParams::new()).await
    }

    /// Fetches one page of open orders.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_open_orders_page(
        &self,
        params: &GetOpenOrdersParams,
    ) -> Result<AxOpenOrdersResponse, AxHttpError> {
        self.send_request_to_url::<AxOpenOrdersResponse, _>(
            &self.orders_base_url,
            Method::GET,
            "/open-orders",
            Some(params),
            None,
            true,
        )
        .await
    }

    /// Fetches the default page of fills/trades.
    ///
    /// # Endpoint
    /// `GET /fills`
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_fills(
        &self,
        start_timestamp_ns: i64,
        end_timestamp_ns: i64,
    ) -> Result<AxFillsResponse, AxHttpError> {
        let params = GetFillsParams::new(start_timestamp_ns, end_timestamp_ns);
        self.get_fills_page(&params).await
    }

    /// Fetches one page of fills/trades.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_fills_page(
        &self,
        params: &GetFillsParams,
    ) -> Result<AxFillsResponse, AxHttpError> {
        self.send_request::<AxFillsResponse, _>(Method::GET, "/fills", Some(params), None, true)
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

    /// Fetches the default page of funding rates for a symbol.
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
        self.get_funding_rates_page(&params).await
    }

    /// Fetches one page of funding rates for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_funding_rates_page(
        &self,
        params: &GetFundingRatesParams,
    ) -> Result<AxFundingRatesResponse, AxHttpError> {
        self.send_request::<AxFundingRatesResponse, _>(
            Method::GET,
            "/funding-rates",
            Some(params),
            None,
            true,
        )
        .await
    }

    /// Fetches the funding-slot schedule for a symbol on a trading day.
    ///
    /// # Endpoint
    /// `GET /funding-slots`
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_funding_slots(
        &self,
        params: &GetFundingSlotsParams,
    ) -> Result<AxFundingSlotsResponse, AxHttpError> {
        self.send_request::<AxFundingSlotsResponse, _>(
            Method::GET,
            "/funding-slots",
            Some(params),
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

    /// Fetches the default page of transactions filtered by type.
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
        start_timestamp_ns: i64,
        end_timestamp_ns: i64,
    ) -> Result<AxTransactionsResponse, AxHttpError> {
        let params =
            GetTransactionsParams::new(transaction_types, start_timestamp_ns, end_timestamp_ns);
        self.get_transactions_page(&params).await
    }

    /// Fetches one page of transactions.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    pub async fn get_transactions_page(
        &self,
        params: &GetTransactionsParams,
    ) -> Result<AxTransactionsResponse, AxHttpError> {
        self.send_request::<AxTransactionsResponse, _>(
            Method::GET,
            "/transactions",
            Some(params),
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
        // The AX sandbox requires authentication for `/book` despite the public schema
        self.send_request::<AxBookResponse, _>(Method::GET, "/book", Some(&params), None, true)
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
    pyo3::pyclass(module = "nautilus_trader.adapters.architect_ax", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.architect_ax")
)]
pub struct AxHttpClient {
    pub(crate) inner: Arc<AxRawHttpClient>,
    pub(crate) instruments_cache: Arc<AtomicMap<Ustr, InstrumentAny>>,
    clock: &'static AtomicTime,
    cache_initialized: Arc<AtomicBool>,
    account_fees: Arc<ArcSwapOption<(Decimal, Decimal)>>,
}

impl Clone for AxHttpClient {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            instruments_cache: self.instruments_cache.clone(),
            cache_initialized: self.cache_initialized.clone(),
            clock: self.clock,
            account_fees: self.account_fees.clone(),
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
            account_fees: Arc::new(ArcSwapOption::empty()),
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
            account_fees: Arc::new(ArcSwapOption::empty()),
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
    pub fn set_session_token(&self, token: SecretString) {
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
    ) -> Result<SecretString, AxHttpError> {
        let resp = self
            .inner
            .authenticate(api_key, api_secret, expiration_seconds)
            .await?;
        let token = resp.into_token();
        self.inner.set_session_token(token.clone());
        Ok(token)
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
    pub async fn authenticate_auto(
        &self,
        expiration_seconds: i32,
    ) -> Result<SecretString, AxHttpError> {
        let resp = self.inner.authenticate_auto(expiration_seconds).await?;
        let token = resp.into_token();
        self.inner.set_session_token(token.clone());
        Ok(token)
    }

    /// Gets an instrument from the cache by symbol.
    pub fn get_instrument(&self, symbol: &Ustr) -> Option<InstrumentAny> {
        self.instruments_cache.get_cloned(symbol)
    }

    /// Resolves the maker and taker fee rates for the account behind the current credentials.
    ///
    /// AX reports fee rates per account rather than per user, and returns the accounts the
    /// credentials can act on. The first entry is used, which is the account AX resolves when a
    /// request carries no explicit selector. The rates are retained so later instrument requests,
    /// including the periodic refresh, keep reporting them.
    ///
    /// Requires an authenticated client.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails, the response carries no accounts, or the selected
    /// account supplies no fee rates. An absent rate is not treated as zero, because a zero rate
    /// is itself valid and a silent zero would outlive the response that caused it.
    pub async fn request_account_fees(&self) -> anyhow::Result<(Decimal, Decimal)> {
        let whoami = self
            .inner
            .get_whoami()
            .await
            .map_err(|e| anyhow::anyhow!(e))
            .context("failed to request AX whoami")?;

        let Some(account) = whoami.accounts.first() else {
            anyhow::bail!("AX whoami returned no accounts to resolve fees from");
        };

        if whoami.accounts.len() > 1 {
            log::warn!(
                "AX credentials cover {} accounts, using fee rates from {}",
                whoami.accounts.len(),
                account.id,
            );
        }

        let (Some(maker_fee), Some(taker_fee)) = (account.maker_fee, account.taker_fee) else {
            anyhow::bail!("AX whoami account {} supplied no fee rates", account.id);
        };

        let fees = (maker_fee, taker_fee);
        self.account_fees.store(Some(Arc::new(fees)));

        Ok(fees)
    }

    /// Requests all instruments from Ax.
    ///
    /// Fee rates fall back to the rates last resolved from `GET /whoami`, and to zero when no
    /// rates have been resolved.
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

        let (maker_fee, taker_fee) = self.resolve_fees(maker_fee, taker_fee);
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

            match parse_instrument(inst, maker_fee, taker_fee, ts_init, ts_init) {
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
    /// Fee rates fall back to the rates last resolved from `GET /whoami`, and to zero when no
    /// rates have been resolved.
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

        let (maker_fee, taker_fee) = self.resolve_fees(maker_fee, taker_fee);
        let ts_init = self.generate_ts_init();

        parse_instrument(&resp, maker_fee, taker_fee, ts_init, ts_init)
    }

    fn resolve_fees(
        &self,
        maker_fee: Option<Decimal>,
        taker_fee: Option<Decimal>,
    ) -> (Decimal, Decimal) {
        let resolved = self.account_fees.load();

        let Some(&(resolved_maker, resolved_taker)) = resolved.as_deref() else {
            // Either rate missing becomes zero, so warn on a partial argument too
            if (maker_fee.is_none() || taker_fee.is_none()) && self.inner.has_session_token() {
                log::warn!(
                    "Building instruments with zero fees: authenticated but account fee rates \
                     were never resolved"
                );
            }

            return (
                maker_fee.unwrap_or(Decimal::ZERO),
                taker_fee.unwrap_or(Decimal::ZERO),
            );
        };

        (
            maker_fee.unwrap_or(resolved_maker),
            taker_fee.unwrap_or(resolved_taker),
        )
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
        let ts_event = ax_timestamp_stn_to_unix_nanos(resp.book.ts, resp.book.tn)?;

        for (i, level) in resp.book.b.iter().enumerate() {
            if depth.is_some_and(|d| i >= d) {
                break;
            }
            let price = Price::from_decimal_dp(level.p, price_precision).with_context(|| {
                format!(
                    "Failed to convert AX book bid price {} for {symbol}",
                    level.p
                )
            })?;
            let size = Quantity::new(level.q as f64, size_precision);
            let order = BookOrder::new(OrderSide::Buy, price, size, i as u64);
            book.add(order, 0, i as u64, ts_event);
        }

        let bids_len = resp.book.b.len();
        for (i, level) in resp.book.a.iter().enumerate() {
            if depth.is_some_and(|d| i >= d) {
                break;
            }
            let price = Price::from_decimal_dp(level.p, price_precision).with_context(|| {
                format!(
                    "Failed to convert AX book ask price {} for {symbol}",
                    level.p
                )
            })?;
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
        start: Option<Timestamp>,
        end: Option<Timestamp>,
        width: AxCandleWidth,
    ) -> anyhow::Result<Vec<Bar>> {
        let instrument = self
            .get_instrument(&symbol)
            .ok_or_else(|| anyhow::anyhow!("Instrument {symbol} not found in cache"))?;

        let start_ns = start
            .and_then(|dt| i64::try_from(dt.as_nanosecond()).ok())
            .unwrap_or(0);
        let end_ns = end
            .and_then(|dt| i64::try_from(dt.as_nanosecond()).ok())
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
    /// Traverses the provider's cursor chain. This is a best-effort historical
    /// read, not an atomic snapshot if AX corrects rows during the traversal.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails.
    pub async fn request_funding_rates(
        &self,
        instrument_id: InstrumentId,
        start: Option<Timestamp>,
        end: Option<Timestamp>,
    ) -> Result<Vec<FundingRateUpdate>, AxHttpError> {
        const PAGE_SIZE: i32 = 100;

        let symbol = instrument_id.symbol.inner();
        let start_ns = start
            .and_then(|dt| i64::try_from(dt.as_nanosecond()).ok())
            .unwrap_or(0);
        let end_ns = end
            .and_then(|dt| i64::try_from(dt.as_nanosecond()).ok())
            .unwrap_or_else(|| self.generate_ts_init().as_i64());
        let mut params = GetFundingRatesParams::new(symbol, start_ns, end_ns);
        params.limit = Some(PAGE_SIZE);
        params.sort_ts = Some("desc".to_string());

        let mut funding_rates = Vec::new();
        let mut seen_rows = HashSet::new();
        let mut seen_cursors = HashSet::new();
        let mut expected_total = None;

        loop {
            let response = self.inner.get_funding_rates_page(&params).await?;
            let page_len = response.funding_rates.len();

            if page_len > PAGE_SIZE as usize {
                return Err(format!(
                    "AX funding-rates page length {page_len} exceeds requested limit {PAGE_SIZE}"
                )
                .into());
            }

            if let Some(limit) = response.limit {
                if !(0..=PAGE_SIZE).contains(&limit) {
                    return Err(format!(
                        "AX funding-rates applied limit must be between 0 and {PAGE_SIZE}, was {limit}"
                    )
                    .into());
                }

                if page_len > limit as usize {
                    return Err(format!(
                        "AX funding-rates page length {page_len} exceeds applied limit {limit}"
                    )
                    .into());
                }
            }

            if let Some(total_count) = response.total_count {
                if total_count < 0 {
                    return Err(format!(
                        "AX funding-rates total_count must be non-negative, was {total_count}"
                    )
                    .into());
                }

                if let Some(expected) = expected_total {
                    if total_count != expected {
                        return Err(format!(
                            "AX funding-rates total_count changed during pagination: expected {expected}, was {total_count}"
                        )
                        .into());
                    }
                } else {
                    expected_total = Some(total_count);
                }
            }

            for rate in response.funding_rates {
                let identity = (
                    rate.symbol,
                    rate.timestamp_ns,
                    rate.funding_rate,
                    rate.funding_amount,
                    rate.benchmark_price,
                    rate.settlement_price,
                );

                if !seen_rows.insert(identity) {
                    return Err(format!(
                        "AX funding-rates pagination returned an exact duplicate row for {} at {}",
                        rate.symbol, rate.timestamp_ns
                    )
                    .into());
                }
                funding_rates.push(rate);
            }

            if let Some(total_count) = expected_total
                && funding_rates.len() as i64 > total_count
            {
                return Err(format!(
                    "AX funding-rates pagination returned more unique rows ({}) than total_count {total_count}",
                    funding_rates.len()
                )
                .into());
            }

            match response.next_cursor {
                Some(next_cursor) => {
                    if next_cursor.is_empty() {
                        return Err("AX funding-rates returned an empty next_cursor"
                            .to_string()
                            .into());
                    }

                    if page_len == 0 {
                        return Err("AX funding-rates returned an empty page with a next_cursor"
                            .to_string()
                            .into());
                    }

                    if !seen_cursors.insert(next_cursor.clone()) {
                        return Err(format!(
                            "AX funding-rates pagination repeated cursor {next_cursor:?}"
                        )
                        .into());
                    }
                    params.cursor = Some(next_cursor);
                }
                None => break,
            }
        }

        if let Some(total_count) = expected_total
            && funding_rates.len() as i64 != total_count
        {
            return Err(format!(
                "AX funding-rates pagination returned {} unique rows, expected {total_count}",
                funding_rates.len()
            )
            .into());
        }

        let ts_init = self.generate_ts_init();
        let updates = funding_rates
            .iter()
            .map(|r| parse_funding_rate(r, instrument_id, ts_init))
            .collect::<anyhow::Result<Vec<_>>>()
            .map_err(|e| AxHttpError::from(e.to_string()))?;

        Ok(updates)
    }

    /// Requests the funding-slot schedule for a symbol on a trading day.
    ///
    /// AX returns a single response covering the whole trading day, so there
    /// is no pagination. The schedule has no Nautilus domain equivalent, so
    /// the venue response is returned verbatim.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn request_funding_slots(
        &self,
        instrument_id: InstrumentId,
        date: Option<Date>,
    ) -> Result<AxFundingSlotsResponse, AxHttpError> {
        let symbol = instrument_id.symbol.inner();
        let mut params = GetFundingSlotsParams::new(symbol);

        if let Some(date) = date {
            params.date = Some(date.strftime("%Y-%m-%d").to_string());
        }

        self.inner.get_funding_slots(&params).await
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
        order_side: Option<OrderSide>,
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
    /// Missing instruments are requested from Ax and cached before parsing order details.
    ///
    /// The `cid_resolver` parameter is an optional function that resolves a `cid` (u64)
    /// to a `ClientOrderId`. This is needed for correlating orders submitted via WebSocket.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The HTTP request fails.
    /// - An order's instrument cannot be fetched or parsed.
    ///
    /// # Notes
    ///
    /// Order parsing failures are skipped with a warning.
    pub async fn request_order_status_reports<F>(
        &self,
        account_id: AccountId,
        cid_resolver: Option<F>,
    ) -> anyhow::Result<Vec<OrderStatusReport>>
    where
        F: Fn(u64) -> Option<ClientOrderId>,
    {
        const PAGE_SIZE: i32 = 100;

        let mut orders = Vec::new();
        let mut seen_order_ids = HashSet::new();
        let mut offset = 0_i64;
        let mut expected_total = None;

        loop {
            let request_offset = i32::try_from(offset)
                .context("AX open-orders offset exceeds the documented int32 range")?;
            let params = GetOpenOrdersParams {
                account_id: None,
                limit: Some(PAGE_SIZE),
                offset: Some(request_offset),
                sort_ts: Some("desc".to_string()),
            };
            let response = self
                .inner
                .get_open_orders_page(&params)
                .await
                .map_err(|e| anyhow::anyhow!(e))?;

            anyhow::ensure!(
                response.total_count >= 0,
                "AX open-orders total_count must be non-negative, was {}",
                response.total_count
            );
            anyhow::ensure!(
                response.limit >= 0 && response.limit <= PAGE_SIZE,
                "AX open-orders applied limit must be between 0 and {PAGE_SIZE}, was {}",
                response.limit
            );
            anyhow::ensure!(
                i64::from(response.offset) == offset,
                "AX open-orders response offset mismatch: requested {offset}, was {}",
                response.offset
            );

            let total_count = *expected_total.get_or_insert(response.total_count);
            anyhow::ensure!(
                response.total_count == total_count,
                "AX open-orders total_count changed during pagination: expected {total_count}, was {}",
                response.total_count
            );

            let page_len = i64::try_from(response.orders.len())
                .context("AX open-orders page length exceeds i64")?;
            anyhow::ensure!(
                page_len <= i64::from(response.limit),
                "AX open-orders page length {page_len} exceeds applied limit {}",
                response.limit
            );
            let next_offset = offset
                .checked_add(page_len)
                .context("AX open-orders offset overflow")?;
            anyhow::ensure!(
                next_offset <= total_count,
                "AX open-orders page exceeds total_count: next offset {next_offset}, total {total_count}"
            );

            if total_count == 0 {
                anyhow::ensure!(
                    response.orders.is_empty(),
                    "AX open-orders returned rows with total_count zero"
                );
                break;
            }

            anyhow::ensure!(
                !response.orders.is_empty(),
                "AX open-orders returned an empty page before offset {offset} reached total {total_count}"
            );

            for order in response.orders {
                anyhow::ensure!(
                    seen_order_ids.insert(order.oid.clone()),
                    "AX open-orders pagination returned duplicate order ID {}",
                    order.oid
                );
                orders.push(order);
            }

            if next_offset == total_count {
                break;
            }

            offset = next_offset;
        }

        anyhow::ensure!(
            i64::try_from(orders.len()).context("AX open-orders result length exceeds i64")?
                == expected_total.unwrap_or_default(),
            "AX open-orders pagination did not return the advertised number of unique orders"
        );

        let ts_init = self.generate_ts_init();
        let mut reports = Vec::with_capacity(orders.len());

        for order in &orders {
            let instrument = self.resolve_report_instrument(order.s).await?;

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

    /// Requests historical orders from Ax and parses them to Nautilus
    /// [`OrderStatusReport`].
    ///
    /// Missing instruments are requested from Ax and cached before parsing order details.
    ///
    /// The `cid_resolver` parameter is an optional function that resolves a `cid` (u64)
    /// to a `ClientOrderId`. This is needed for correlating orders submitted via WebSocket.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The HTTP request or pagination contract fails.
    /// - An order's instrument cannot be fetched or parsed.
    ///
    /// # Notes
    ///
    /// Order parsing failures are skipped with a warning.
    pub async fn request_historical_order_status_reports<F>(
        &self,
        account_id: AccountId,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
        cid_resolver: Option<F>,
    ) -> anyhow::Result<Vec<OrderStatusReport>>
    where
        F: Fn(u64) -> Option<ClientOrderId>,
    {
        const PAGE_SIZE: i32 = 100;

        let mut params = GetOrdersParams {
            start_timestamp_ns: start.map(|timestamp| timestamp.as_i64()),
            end_timestamp_ns: end.map(|timestamp| timestamp.as_i64()),
            limit: Some(PAGE_SIZE),
            ..Default::default()
        };
        let mut orders = Vec::new();
        let mut seen_cursors = HashSet::new();
        let mut seen_order_ids = HashSet::new();

        loop {
            let response = self
                .inner
                .get_orders(&params)
                .await
                .map_err(|e| anyhow::anyhow!(e))?;

            for order in response.orders {
                anyhow::ensure!(
                    seen_order_ids.insert(order.oid.clone()),
                    "AX orders pagination returned duplicate order ID {}",
                    order.oid
                );
                orders.push(order);
            }

            match response.next_cursor {
                Some(next_cursor) => {
                    anyhow::ensure!(
                        seen_cursors.insert(next_cursor.clone()),
                        "AX orders pagination repeated cursor {next_cursor:?}"
                    );
                    params.cursor = Some(next_cursor);
                }
                None => break,
            }
        }

        let ts_init = self.generate_ts_init();
        let mut reports = Vec::with_capacity(orders.len());

        for order in &orders {
            let instrument = self.resolve_report_instrument(order.s).await?;

            match parse_order_detail_status_report(
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
    /// Missing instruments are requested from Ax and cached before parsing fill details.
    /// Traverses the provider's cursor chain. This is a best-effort historical
    /// read, not an atomic snapshot if AX corrects rows during the traversal.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The HTTP request fails.
    /// - A fill's instrument cannot be fetched or parsed.
    /// - Fill parsing fails.
    pub async fn request_fill_reports(
        &self,
        account_id: AccountId,
        start: Option<UnixNanos>,
        end: Option<UnixNanos>,
    ) -> anyhow::Result<Vec<FillReport>> {
        const PAGE_SIZE: i32 = 100;

        // The AX `/fills` endpoint requires a bounded time range and caps the span at 7 days
        let max_span_ns = AX_FILLS_MAX_LOOKBACK_DAYS * 24 * 60 * 60 * 1_000_000_000;
        let end_ns = end.map_or_else(|| self.generate_ts_init().as_i64(), |e| e.as_i64());
        let floor_ns = end_ns - max_span_ns;
        let start_ns = start.map_or(floor_ns, |s| s.as_i64().max(floor_ns));
        let mut params = GetFillsParams::new(start_ns, end_ns);
        params.limit = Some(PAGE_SIZE);
        params.sort_ts = Some("desc".to_string());

        let mut fills = Vec::new();
        let mut seen_trade_ids = HashSet::new();
        let mut seen_cursors = HashSet::new();
        let mut expected_total = None;

        loop {
            let response = self
                .inner
                .get_fills_page(&params)
                .await
                .map_err(|e| anyhow::anyhow!(e))?;
            let page_len = response.fills.len();

            anyhow::ensure!(
                page_len <= PAGE_SIZE as usize,
                "AX fills page length {page_len} exceeds requested limit {PAGE_SIZE}"
            );

            if let Some(limit) = response.limit {
                anyhow::ensure!(
                    (0..=PAGE_SIZE).contains(&limit),
                    "AX fills applied limit must be between 0 and {PAGE_SIZE}, was {limit}"
                );
                anyhow::ensure!(
                    page_len <= limit as usize,
                    "AX fills page length {page_len} exceeds applied limit {limit}"
                );
            }

            if let Some(total_count) = response.total_count {
                anyhow::ensure!(
                    total_count >= 0,
                    "AX fills total_count must be non-negative, was {total_count}"
                );

                if let Some(expected) = expected_total {
                    anyhow::ensure!(
                        total_count == expected,
                        "AX fills total_count changed during pagination: expected {expected}, was {total_count}"
                    );
                } else {
                    expected_total = Some(total_count);
                }
            }

            for fill in response.fills {
                anyhow::ensure!(
                    seen_trade_ids.insert(fill.trade_id.clone()),
                    "AX fills pagination returned duplicate trade ID {}",
                    fill.trade_id
                );
                fills.push(fill);
            }

            if let Some(total_count) = expected_total {
                anyhow::ensure!(
                    fills.len() as i64 <= total_count,
                    "AX fills pagination returned more unique rows ({}) than total_count {total_count}",
                    fills.len()
                );
            }

            match response.next_cursor {
                Some(next_cursor) => {
                    anyhow::ensure!(
                        !next_cursor.is_empty(),
                        "AX fills returned an empty next_cursor"
                    );
                    anyhow::ensure!(
                        page_len > 0,
                        "AX fills returned an empty page with a next_cursor"
                    );
                    anyhow::ensure!(
                        seen_cursors.insert(next_cursor.clone()),
                        "AX fills pagination repeated cursor {next_cursor:?}"
                    );
                    params.cursor = Some(next_cursor);
                }
                None => break,
            }
        }

        if let Some(total_count) = expected_total {
            anyhow::ensure!(
                fills.len() as i64 == total_count,
                "AX fills pagination returned {} unique rows, expected {total_count}",
                fills.len()
            );
        }

        let ts_init = self.generate_ts_init();
        let mut reports = Vec::with_capacity(fills.len());

        for fill in &fills {
            let instrument = self.resolve_report_instrument(fill.symbol).await?;
            let report = parse_fill_report(fill, account_id, &instrument, ts_init)
                .with_context(|| format!("Failed to parse AX fill {}", fill.trade_id))?;
            reports.push(report);
        }

        Ok(reports)
    }

    /// Requests positions from Ax and parses them to Nautilus [`PositionStatusReport`].
    ///
    /// Missing instruments are requested from Ax and cached before parsing position details.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The HTTP request fails.
    /// - A position's instrument cannot be fetched or parsed.
    ///
    /// # Notes
    ///
    /// Position parsing failures are skipped with a warning.
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

            let instrument = self.resolve_report_instrument(position.symbol).await?;

            match parse_position_status_report(position, account_id, &instrument, ts_init) {
                Ok(report) => reports.push(report),
                Err(e) => {
                    log::warn!("Failed to parse position for {}: {e}", position.symbol);
                }
            }
        }

        Ok(reports)
    }

    async fn resolve_report_instrument(&self, symbol: Ustr) -> anyhow::Result<InstrumentAny> {
        if let Some(instrument) = self.get_instrument(&symbol) {
            return Ok(instrument);
        }

        let instrument = self
            .request_instrument(symbol, None, None)
            .await
            .map_err(|e| {
                anyhow::anyhow!("Failed to resolve AX instrument {symbol} via GET /instrument: {e}")
            })?;
        self.cache_instrument(instrument.clone());
        Ok(instrument)
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
