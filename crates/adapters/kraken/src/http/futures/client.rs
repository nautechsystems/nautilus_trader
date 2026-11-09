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

//! HTTP client for the Kraken Futures REST API.

use std::{
    collections::HashMap,
    fmt::Debug,
    num::NonZeroU32,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::{AHashMap, AHashSet};
use indexmap::IndexMap;
use jiff::Timestamp;
use nautilus_common::cache::InstrumentLookupError;
use nautilus_core::{
    AtomicMap, AtomicTime, UUID4, nanos::UnixNanos, time::get_atomic_clock_realtime,
};
use nautilus_model::{
    data::{Bar, BarType, BookOrder, FundingRateUpdate, TradeTick},
    enums::{
        AccountType, BookType, CurrencyType, MarketStatusAction, OrderSide, OrderType, TimeInForce,
        TriggerType,
    },
    events::AccountState,
    identifiers::{AccountId, ClientOrderId, InstrumentId, Symbol, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};
use nautilus_network::{
    ConnectionUrl,
    http::{
        HttpClient, HttpRedirectPolicy, HttpResponse, Method, create_standard_nautilus_headers,
    },
    ratelimiter::quota::Quota,
    retry::{RetryConfig, RetryError, RetryManager},
};
use parking_lot::RwLock;
use rust_decimal::Decimal;
use serde::de::DeserializeOwned;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{models::*, query::*};
use crate::{
    common::{
        consts::{KRAKEN_VENUE, NAUTILUS_KRAKEN_BROKER_ID},
        credential::KrakenCredential,
        enums::{
            KrakenApiResult, KrakenEnvironment, KrakenFuturesOrderStatus, KrakenFuturesOrderType,
            KrakenFuturesQtyMode, KrakenOrderSide, KrakenProductType, KrakenSendStatus,
            KrakenTriggerSignal, product_type_from_symbol,
        },
        parse::{
            bar_type_to_futures_resolution, normalize_asset_key, parse_bar,
            parse_futures_fill_report, parse_futures_instrument,
            parse_futures_order_event_status_report, parse_futures_order_status_details_report,
            parse_futures_order_status_report, parse_futures_order_type_time_in_force,
            parse_futures_position_status_report, parse_futures_public_execution,
            parse_venue_order_id,
        },
        urls::get_kraken_http_base_url,
    },
    http::{
        apply_count_limit,
        coverage::ReportCoverage,
        error::{
            KrakenBatchOrderError, KrakenHttpError, KrakenModifyOrderError, KrakenSubmitOrderError,
            kraken_http_should_retry,
        },
        models::OhlcData,
    },
};

/// Default Kraken Futures REST API rate limit (requests per second).
pub const KRAKEN_FUTURES_DEFAULT_RATE_LIMIT_PER_SECOND: u32 = 5;

const KRAKEN_GLOBAL_RATE_KEY: &str = "kraken:futures:global";

/// Maximum orders per batch cancel request for Kraken Futures API.
const BATCH_CANCEL_LIMIT: usize = 50;

/// Maximum operations per batch order request for Kraken Futures API.
const BATCH_ORDER_LIMIT: usize = 10;

/// The largest order history page: the venue caps a larger `count` here silently and documents no
/// default for this endpoint. A page costs one token whatever its size.
const ORDER_HISTORY_PAGE_SIZE: usize = 1000;

/// The response header Kraken documents for the order history continuation token; the body's
/// `continuationToken` is read first and this header stands in when the body carries none.
const NEXT_CONTINUATION_TOKEN_HEADER: &str = "Next-Continuation-Token";

/// Raw HTTP client for low-level Kraken Futures API operations.
///
/// This client handles request/response operations with the Kraken Futures API,
/// returning venue-specific response types. It does not parse to Nautilus domain types.
pub struct KrakenFuturesRawHttpClient {
    base_url: ConnectionUrl,
    client: HttpClient,
    credential: Option<KrakenCredential>,
    retry_manager: RetryManager<KrakenHttpError>,
    cancellation_token: RwLock<CancellationToken>,
    clock: &'static AtomicTime,
    /// Mutex to serialize authenticated requests, ensuring nonces arrive at Kraken in order
    auth_mutex: tokio::sync::Mutex<()>,
}

impl Default for KrakenFuturesRawHttpClient {
    fn default() -> Self {
        Self::new(
            KrakenEnvironment::Live,
            None,
            60,
            None,
            None,
            None,
            None,
            KRAKEN_FUTURES_DEFAULT_RATE_LIMIT_PER_SECOND,
        )
        .expect("Failed to create default KrakenFuturesRawHttpClient")
    }
}

impl Debug for KrakenFuturesRawHttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(KrakenFuturesRawHttpClient))
            .field("base_url", &self.base_url)
            .field("has_credentials", &self.credential.is_some())
            .finish()
    }
}

impl KrakenFuturesRawHttpClient {
    /// Creates a new [`KrakenFuturesRawHttpClient`].
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        environment: KrakenEnvironment,
        base_url_override: Option<String>,
        timeout_secs: u64,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        proxy_url: Option<String>,
        max_requests_per_second: u32,
    ) -> anyhow::Result<Self> {
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
        let base_url = base_url_override.unwrap_or_else(|| {
            get_kraken_http_base_url(KrakenProductType::Futures, environment).to_string()
        });

        Ok(Self {
            base_url: base_url.into(),
            client: HttpClient::builder()
                .headers(Self::default_headers())
                .header_keys(vec![NEXT_CONTINUATION_TOKEN_HEADER.to_string()])
                .keyed_quotas(Self::rate_limiter_quotas(max_requests_per_second)?)
                .default_quota(Self::default_quota(max_requests_per_second)?)
                .timeout_secs(timeout_secs)
                .maybe_proxy_url(proxy_url)
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {e}"))?,
            credential: None,
            retry_manager,
            cancellation_token: RwLock::new(CancellationToken::new()),
            clock: get_atomic_clock_realtime(),
            auth_mutex: tokio::sync::Mutex::new(()),
        })
    }

    /// Creates a new [`KrakenFuturesRawHttpClient`] with credentials.
    #[expect(clippy::too_many_arguments)]
    pub fn with_credentials(
        api_key: String,
        api_secret: String,
        environment: KrakenEnvironment,
        base_url_override: Option<String>,
        timeout_secs: u64,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        proxy_url: Option<String>,
        max_requests_per_second: u32,
    ) -> anyhow::Result<Self> {
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
        let base_url = base_url_override.unwrap_or_else(|| {
            get_kraken_http_base_url(KrakenProductType::Futures, environment).to_string()
        });

        Ok(Self {
            base_url: base_url.into(),
            client: HttpClient::builder()
                .redirect_policy(HttpRedirectPolicy::Reject)
                .headers(Self::default_headers())
                .header_keys(vec![NEXT_CONTINUATION_TOKEN_HEADER.to_string()])
                .keyed_quotas(Self::rate_limiter_quotas(max_requests_per_second)?)
                .default_quota(Self::default_quota(max_requests_per_second)?)
                .timeout_secs(timeout_secs)
                .maybe_proxy_url(proxy_url)
                .build()
                .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {e}"))?,
            credential: Some(KrakenCredential::new(api_key, api_secret)),
            retry_manager,
            cancellation_token: RwLock::new(CancellationToken::new()),
            clock: get_atomic_clock_realtime(),
            auth_mutex: tokio::sync::Mutex::new(()),
        })
    }

    /// Generates a unique nonce for Kraken Futures API requests.
    ///
    /// Uses `AtomicTime` for strict monotonicity. The nanosecond timestamp
    /// guarantees uniqueness even for rapid consecutive calls.
    fn generate_nonce(&self) -> u64 {
        self.clock.get_time_ns().as_u64()
    }

    /// Returns the base URL for this client.
    pub fn base_url(&self) -> &str {
        self.base_url.expose()
    }

    /// Returns the credential for this client, if set.
    pub fn credential(&self) -> Option<&KrakenCredential> {
        self.credential.as_ref()
    }

    /// Cancels all pending HTTP requests.
    pub fn cancel_all_requests(&self) {
        self.cancellation_token.read().cancel();
    }

    /// Replaces the canceled token so requests can proceed after reconnect.
    pub fn reset_cancellation_token(&self) {
        *self.cancellation_token.write() = CancellationToken::new();
    }

    /// Returns a clone of the current cancellation token.
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation_token.read().clone()
    }

    fn default_headers() -> HashMap<String, String> {
        create_standard_nautilus_headers().into_iter().collect()
    }

    fn default_quota(max_requests_per_second: u32) -> anyhow::Result<Quota> {
        let burst = NonZeroU32::new(max_requests_per_second).unwrap_or(
            NonZeroU32::new(KRAKEN_FUTURES_DEFAULT_RATE_LIMIT_PER_SECOND).expect("non-zero"),
        );
        Quota::per_second(burst).ok_or_else(|| {
            anyhow::anyhow!(
                "Invalid max_requests_per_second: {max_requests_per_second} exceeds maximum"
            )
        })
    }

    fn rate_limiter_quotas(max_requests_per_second: u32) -> anyhow::Result<Vec<(String, Quota)>> {
        Ok(vec![(
            KRAKEN_GLOBAL_RATE_KEY.to_string(),
            Self::default_quota(max_requests_per_second)?,
        )])
    }

    fn rate_limit_keys(endpoint: &str) -> Vec<String> {
        let normalized = endpoint.split('?').next().unwrap_or(endpoint);
        let route = format!("kraken:futures:{normalized}");
        vec![KRAKEN_GLOBAL_RATE_KEY.to_string(), route]
    }

    async fn send_request<T: DeserializeOwned>(
        &self,
        method: Method,
        endpoint: &str,
        url: String,
        authenticate: bool,
    ) -> anyhow::Result<T, KrakenHttpError> {
        // Serialize authenticated requests to ensure nonces arrive at Kraken in order.
        // Without this, concurrent requests can race through the network and arrive
        // out-of-order, causing "Invalid nonce" errors.
        let _guard = if authenticate {
            Some(self.auth_mutex.lock().await)
        } else {
            None
        };

        let endpoint = endpoint.to_string();
        let method_clone = method.clone();
        let url_clone = url.clone();
        let credential = self.credential.clone();

        let operation = || {
            let url = url_clone.clone();
            let method = method_clone.clone();
            let endpoint = endpoint.clone();
            let credential = credential.clone();

            async move {
                let mut headers = Self::default_headers();

                if authenticate {
                    let cred = credential.as_ref().ok_or_else(|| {
                        KrakenHttpError::AuthenticationError(
                            "Missing credentials for authenticated request".to_string(),
                        )
                    })?;

                    let nonce = self.generate_nonce();

                    let signature = cred.sign_futures(&endpoint, "", nonce).map_err(|e| {
                        KrakenHttpError::AuthenticationError(format!("Failed to sign request: {e}"))
                    })?;

                    log::debug!(
                        "Kraken Futures auth: endpoint={endpoint}, nonce={nonce}, base_url={}",
                        self.base_url.origin()
                    );

                    headers.insert("APIKey".to_string(), cred.api_key().to_string());
                    headers.insert("Authent".to_string(), signature);
                    headers.insert("Nonce".to_string(), nonce.to_string());
                }

                let rate_limit_keys = Self::rate_limit_keys(&endpoint);

                let response = self
                    .client
                    .request(
                        method,
                        url,
                        None,
                        Some(headers),
                        None,
                        None,
                        Some(rate_limit_keys),
                    )
                    .await
                    .map_err(|e| {
                        KrakenHttpError::NetworkError(format!(
                            "Request to {} failed: {e}",
                            self.base_url.origin()
                        ))
                    })?;

                let status = response.status.as_u16();
                if status >= 400 {
                    let body = String::from_utf8_lossy(&response.body).to_string();
                    // Don't retry authentication errors
                    if status == 401 || status == 403 {
                        return Err(KrakenHttpError::AuthenticationError(format!(
                            "HTTP error {status}: {body}"
                        )));
                    }
                    return Err(KrakenHttpError::NetworkError(format!(
                        "HTTP error {status}: {body}"
                    )));
                }

                let response_text = String::from_utf8(response.body.to_vec()).map_err(|e| {
                    KrakenHttpError::ParseError(format!("Failed to parse response as UTF-8: {e}"))
                })?;

                serde_json::from_str(&response_text).map_err(|e| {
                    KrakenHttpError::ParseError(format!(
                        "Failed to deserialize futures response: {e}"
                    ))
                })
            }
        };

        let should_retry = kraken_http_should_retry;
        let create_error = |error: RetryError| KrakenHttpError::NetworkError(error.to_string());

        let cancellation_token = self.cancellation_token();

        self.retry_manager
            .invocation(&endpoint, operation, should_retry, create_error)
            .cancellation_token(&cancellation_token)
            .execute()
            .await
    }

    /// Sends authenticated GET request with query parameters included in signature.
    ///
    /// For Kraken Futures, GET requests with query params must include them in postData
    /// for signing: message = postData + nonce + endpoint
    async fn send_get_with_query<T: DeserializeOwned>(
        &self,
        endpoint: &str,
        url: String,
        query_string: &str,
    ) -> anyhow::Result<T, KrakenHttpError> {
        let response = self
            .send_get_with_query_raw(endpoint, url, query_string)
            .await?;

        Self::deserialize_body(&response)
    }

    /// Sends the authenticated GET of [`Self::send_get_with_query`] and hands back the whole
    /// response, for an endpoint that answers in its headers as well as its body.
    async fn send_get_with_query_raw(
        &self,
        endpoint: &str,
        url: String,
        query_string: &str,
    ) -> anyhow::Result<HttpResponse, KrakenHttpError> {
        let _guard = self.auth_mutex.lock().await;
        let cancellation_token = self.cancellation_token();

        if cancellation_token.is_cancelled() {
            return Err(KrakenHttpError::NetworkError(
                "Request cancelled".to_string(),
            ));
        }

        let credential = self.credential.as_ref().ok_or_else(|| {
            KrakenHttpError::AuthenticationError("Missing credentials".to_string())
        })?;

        let nonce = self.generate_nonce();

        // Query params go in postData for signing (not in endpoint)
        let signature = credential
            .sign_futures(endpoint, query_string, nonce)
            .map_err(|e| {
                KrakenHttpError::AuthenticationError(format!("Failed to sign request: {e}"))
            })?;

        log::debug!(
            "Kraken Futures GET with query: endpoint={endpoint}, query={query_string}, nonce={nonce}"
        );

        let mut headers = Self::default_headers();
        headers.insert("APIKey".to_string(), credential.api_key().to_string());
        headers.insert("Authent".to_string(), signature);
        headers.insert("Nonce".to_string(), nonce.to_string());

        let rate_limit_keys = Self::rate_limit_keys(endpoint);

        let response = self
            .client
            .request(
                Method::GET,
                url,
                None,
                Some(headers),
                None,
                None,
                Some(rate_limit_keys),
            )
            .await
            .map_err(|e| {
                KrakenHttpError::NetworkError(format!(
                    "Request to {} failed: {e}",
                    self.base_url.origin()
                ))
            })?;

        let status = response.status.as_u16();
        if status >= 400 {
            let body = String::from_utf8_lossy(&response.body).to_string();

            if status == 401 || status == 403 {
                return Err(KrakenHttpError::AuthenticationError(format!(
                    "HTTP error {status}: {body}"
                )));
            }
            return Err(KrakenHttpError::NetworkError(format!(
                "HTTP error {status}: {body}"
            )));
        }

        Ok(response)
    }

    fn deserialize_body<T: DeserializeOwned>(
        response: &HttpResponse,
    ) -> anyhow::Result<T, KrakenHttpError> {
        let response_text = std::str::from_utf8(&response.body).map_err(|e| {
            KrakenHttpError::ParseError(format!("Failed to parse response as UTF-8: {e}"))
        })?;

        serde_json::from_str(response_text).map_err(|e| {
            KrakenHttpError::ParseError(format!("Failed to deserialize futures response: {e}"))
        })
    }

    async fn send_request_with_body<T: DeserializeOwned>(
        &self,
        endpoint: &str,
        params: HashMap<String, String>,
    ) -> anyhow::Result<T, KrakenHttpError> {
        let post_data = serde_urlencoded::to_string(&params).map_err(|e| {
            KrakenHttpError::RequestNotStarted(format!("Failed to encode params: {e}"))
        })?;
        self.send_authenticated_post(endpoint, post_data).await
    }

    /// Sends a request with typed parameters (serializable struct).
    async fn send_request_with_params<P: serde::Serialize, T: DeserializeOwned>(
        &self,
        endpoint: &str,
        params: &P,
    ) -> anyhow::Result<T, KrakenHttpError> {
        let post_data = serde_urlencoded::to_string(params).map_err(|e| {
            KrakenHttpError::RequestNotStarted(format!("Failed to encode params: {e}"))
        })?;
        self.send_authenticated_post(endpoint, post_data).await
    }

    /// Core authenticated POST request - takes raw post_data string.
    async fn send_authenticated_post<T: DeserializeOwned>(
        &self,
        endpoint: &str,
        post_data: String,
    ) -> anyhow::Result<T, KrakenHttpError> {
        let cancellation_token = self.cancellation_token();
        if cancellation_token.is_cancelled() {
            return Err(KrakenHttpError::RequestNotStarted(
                "Request cancelled".to_string(),
            ));
        }

        // Serialize authenticated requests to ensure nonces arrive at Kraken in order
        let _guard = tokio::select! {
            biased;
            () = cancellation_token.cancelled() => {
                return Err(KrakenHttpError::RequestNotStarted(
                    "Request cancelled".to_string(),
                ));
            }
            guard = self.auth_mutex.lock() => guard,
        };

        let credential = self
            .credential
            .as_ref()
            .ok_or(KrakenHttpError::MissingCredentials)?;

        let nonce = self.generate_nonce();
        log::debug!("Generated nonce {nonce} for {endpoint}");

        let signature = credential
            .sign_futures(endpoint, &post_data, nonce)
            .map_err(|e| {
                KrakenHttpError::RequestNotStarted(format!("Failed to sign request: {e}"))
            })?;

        let url = format!("{}{endpoint}", self.base_url.expose());
        let mut headers = Self::default_headers();
        headers.insert(
            "Content-Type".to_string(),
            "application/x-www-form-urlencoded".to_string(),
        );
        headers.insert("APIKey".to_string(), credential.api_key().to_string());
        headers.insert("Authent".to_string(), signature);
        headers.insert("Nonce".to_string(), nonce.to_string());

        let rate_limit_keys = Self::rate_limit_keys(endpoint);

        let response = self
            .send_order_request(
                url,
                headers,
                post_data.into_bytes(),
                rate_limit_keys,
                &cancellation_token,
            )
            .await?;

        if response.status.as_u16() >= 400 {
            let status = response.status.as_u16();
            let body = String::from_utf8_lossy(&response.body).to_string();
            return Err(KrakenHttpError::NetworkError(format!(
                "HTTP error {status}: {body}"
            )));
        }

        let response_text = String::from_utf8(response.body.to_vec()).map_err(|e| {
            KrakenHttpError::ParseError(format!("Failed to parse response as UTF-8: {e}"))
        })?;

        serde_json::from_str(&response_text).map_err(|e| {
            log::warn!("Failed to parse response from {endpoint}: {response_text}");
            KrakenHttpError::ParseError(format!("Failed to deserialize response: {e}"))
        })
    }

    async fn send_order_request(
        &self,
        url: String,
        headers: HashMap<String, String>,
        body: Vec<u8>,
        rate_limit_keys: Vec<String>,
        cancellation_token: &CancellationToken,
    ) -> anyhow::Result<HttpResponse, KrakenHttpError> {
        if cancellation_token.is_cancelled() {
            return Err(KrakenHttpError::RequestNotStarted(
                "Request cancelled".to_string(),
            ));
        }

        let request_started = AtomicBool::new(false);
        let request = async {
            request_started.store(true, Ordering::Relaxed);
            self.client
                .request(
                    Method::POST,
                    url,
                    None,
                    Some(headers),
                    Some(body),
                    None,
                    Some(rate_limit_keys),
                )
                .await
        };
        tokio::pin!(request);

        tokio::select! {
            biased;
            () = cancellation_token.cancelled() => {
                if request_started.load(Ordering::Relaxed) {
                    Err(KrakenHttpError::NetworkError(
                        "Request cancelled after transport invocation".to_string(),
                    ))
                } else {
                    Err(KrakenHttpError::RequestNotStarted(
                        "Request cancelled".to_string(),
                    ))
                }
            }
            response = &mut request => response
                .map_err(|e| KrakenHttpError::NetworkError(
                    format!("Request to {} failed: {e}", self.base_url.origin())
                )),
        }
    }

    /// Requests tradable instruments from Kraken Futures.
    pub async fn get_instruments(
        &self,
    ) -> anyhow::Result<FuturesInstrumentsResponse, KrakenHttpError> {
        let endpoint = "/derivatives/api/v3/instruments";
        let url = format!("{}{endpoint}", self.base_url.expose());

        self.send_request(Method::GET, endpoint, url, false).await
    }

    /// Requests ticker information for all futures instruments.
    pub async fn get_tickers(&self) -> anyhow::Result<FuturesTickersResponse, KrakenHttpError> {
        let endpoint = "/derivatives/api/v3/tickers";
        let url = format!("{}{endpoint}", self.base_url.expose());

        self.send_request(Method::GET, endpoint, url, false).await
    }

    /// Requests order book depth for a futures symbol.
    pub async fn get_orderbook(
        &self,
        symbol: &str,
    ) -> anyhow::Result<FuturesOrderBookResponse, KrakenHttpError> {
        let endpoint = format!("/derivatives/api/v3/orderbook?symbol={symbol}");
        let url = format!("{}{endpoint}", self.base_url.expose());

        self.send_request(Method::GET, &endpoint, url, false).await
    }

    /// Requests historical funding rates for a futures symbol.
    pub async fn get_historical_funding_rates(
        &self,
        symbol: &str,
    ) -> anyhow::Result<FuturesHistoricalFundingRatesResponse, KrakenHttpError> {
        let endpoint = format!("/derivatives/api/v4/historicalfundingrates?symbol={symbol}");
        let url = format!("{}{endpoint}", self.base_url.expose());

        self.send_request(Method::GET, &endpoint, url, false).await
    }

    /// Requests OHLC candlestick data for a futures symbol.
    pub async fn get_ohlc(
        &self,
        tick_type: &str,
        symbol: &str,
        resolution: &str,
        from: Option<i64>,
        to: Option<i64>,
    ) -> anyhow::Result<FuturesCandlesResponse, KrakenHttpError> {
        let endpoint = format!("/api/charts/v1/{tick_type}/{symbol}/{resolution}");

        let mut url = format!("{}{endpoint}", self.base_url.expose());

        let mut query_params = Vec::new();

        if let Some(from_ts) = from {
            query_params.push(format!("from={from_ts}"));
        }

        if let Some(to_ts) = to {
            query_params.push(format!("to={to_ts}"));
        }

        if !query_params.is_empty() {
            url.push('?');
            url.push_str(&query_params.join("&"));
        }

        self.send_request(Method::GET, &endpoint, url, false).await
    }

    /// Gets public execution events (trades) for a futures symbol.
    pub async fn get_public_executions(
        &self,
        symbol: &str,
        since: Option<i64>,
        before: Option<i64>,
        sort: Option<&str>,
        continuation_token: Option<&str>,
    ) -> anyhow::Result<FuturesPublicExecutionsResponse, KrakenHttpError> {
        let endpoint = format!("/api/history/v3/market/{symbol}/executions");

        let mut url = format!("{}{endpoint}", self.base_url.expose());

        let mut query_params = Vec::new();

        if let Some(since_ts) = since {
            query_params.push(format!("since={since_ts}"));
        }

        if let Some(before_ts) = before {
            query_params.push(format!("before={before_ts}"));
        }

        if let Some(sort_order) = sort {
            query_params.push(format!("sort={sort_order}"));
        }

        if let Some(token) = continuation_token {
            query_params.push(format!("continuationToken={token}"));
        }

        if !query_params.is_empty() {
            url.push('?');
            url.push_str(&query_params.join("&"));
        }

        self.send_request(Method::GET, &endpoint, url, false).await
    }

    /// Requests all open orders (requires authentication).
    pub async fn get_open_orders(
        &self,
    ) -> anyhow::Result<FuturesOpenOrdersResponse, KrakenHttpError> {
        if self.credential.is_none() {
            return Err(KrakenHttpError::AuthenticationError(
                "API credentials required for futures open orders".to_string(),
            ));
        }

        let endpoint = "/derivatives/api/v3/openorders";
        let url = format!("{}{endpoint}", self.base_url.expose());

        self.send_request(Method::GET, endpoint, url, true).await
    }

    /// Requests historical order events (requires authentication).
    pub async fn get_order_events(
        &self,
        before: Option<i64>,
        since: Option<i64>,
        continuation_token: Option<&str>,
    ) -> anyhow::Result<FuturesOrderEventsResponse, KrakenHttpError> {
        if self.credential.is_none() {
            return Err(KrakenHttpError::AuthenticationError(
                "API credentials required for futures order events".to_string(),
            ));
        }

        let endpoint = "/api/history/v3/orders";
        let mut query_params = Vec::new();

        if let Some(before_ts) = before {
            query_params.push(format!("before={before_ts}"));
        }

        if let Some(since_ts) = since {
            query_params.push(format!("since={since_ts}"));
        }

        if let Some(token) = continuation_token {
            query_params.push(format!("continuation_token={token}"));
        }

        // A page that hands back a continuation token must hold the newest events, which the
        // single-order lookup relies on to tell how far back the page reaches, so the order is
        // pinned rather than left to the venue default
        query_params.push("sort=desc".to_string());
        query_params.push(format!("count={ORDER_HISTORY_PAGE_SIZE}"));

        // Build URL with query params
        let query_string = query_params.join("&");

        let url = if query_string.is_empty() {
            format!("{}{endpoint}", self.base_url.expose())
        } else {
            format!("{}{endpoint}?{query_string}", self.base_url.expose())
        };

        // For signing: query params go in postData, not endpoint
        // Kraken: message = postData + nonce + endpoint
        let response = self
            .send_get_with_query_raw(endpoint, url, &query_string)
            .await?;
        let page: FuturesOrderHistoryResponse = Self::deserialize_body(&response)?;
        let mut events = page.into_order_events()?;

        // Kraken documents the token in the response header and shows it in the body, so a page
        // that carries it in the header alone must not read as the last one.
        if events
            .continuation_token
            .as_deref()
            .is_none_or(str::is_empty)
        {
            events.continuation_token = response
                .headers
                .get(NEXT_CONTINUATION_TOKEN_HEADER)
                .filter(|token| !token.is_empty())
                .cloned();
        }

        Ok(events)
    }

    /// Requests the status of specific orders (requires authentication).
    ///
    /// The venue reports orders which are open or were filled/cancelled in
    /// the last 5 seconds, which covers the Maker Protection race where an
    /// order is absent from `/openorders` because its terminal transition is
    /// younger than the open-orders snapshot.
    pub async fn get_orders_status(
        &self,
        order_ids: &[String],
        cli_ord_ids: &[String],
    ) -> anyhow::Result<FuturesOrdersStatusResponse, KrakenHttpError> {
        if self.credential.is_none() {
            return Err(KrakenHttpError::AuthenticationError(
                "API credentials required for futures orders status".to_string(),
            ));
        }

        let pairs: Vec<(&str, &str)> = order_ids
            .iter()
            .map(|order_id| ("orderIds", order_id.as_str()))
            .chain(
                cli_ord_ids
                    .iter()
                    .map(|cli_ord_id| ("cliOrdIds", cli_ord_id.as_str())),
            )
            .collect();

        let post_data = serde_urlencoded::to_string(&pairs).map_err(|e| {
            KrakenHttpError::RequestNotStarted(format!("Failed to encode params: {e}"))
        })?;

        let endpoint = "/derivatives/api/v3/orders/status";
        self.send_authenticated_post(endpoint, post_data).await
    }

    /// Requests fill/trade history (requires authentication).
    pub async fn get_fills(
        &self,
        last_fill_time: Option<&str>,
    ) -> anyhow::Result<FuturesFillsResponse, KrakenHttpError> {
        if self.credential.is_none() {
            return Err(KrakenHttpError::AuthenticationError(
                "API credentials required for futures fills".to_string(),
            ));
        }

        let endpoint = "/derivatives/api/v3/fills";
        let query_string = last_fill_time
            .map(|t| format!("lastFillTime={t}"))
            .unwrap_or_default();

        let url = if query_string.is_empty() {
            format!("{}{endpoint}", self.base_url.expose())
        } else {
            format!("{}{endpoint}?{query_string}", self.base_url.expose())
        };

        // Query params go in postData for signing
        self.send_get_with_query(endpoint, url, &query_string).await
    }

    /// Requests open positions (requires authentication).
    pub async fn get_open_positions(
        &self,
    ) -> anyhow::Result<FuturesOpenPositionsResponse, KrakenHttpError> {
        if self.credential.is_none() {
            return Err(KrakenHttpError::AuthenticationError(
                "API credentials required for futures open positions".to_string(),
            ));
        }

        let endpoint = "/derivatives/api/v3/openpositions";
        let url = format!("{}{endpoint}", self.base_url.expose());

        self.send_request(Method::GET, endpoint, url, true).await
    }

    /// Requests all accounts (cash and margin) with balances and margin info.
    pub async fn get_accounts(&self) -> anyhow::Result<FuturesAccountsResponse, KrakenHttpError> {
        if self.credential.is_none() {
            return Err(KrakenHttpError::AuthenticationError(
                "API credentials required for futures accounts".to_string(),
            ));
        }

        let endpoint = "/derivatives/api/v3/accounts";
        let url = format!("{}{endpoint}", self.base_url.expose());

        self.send_request(Method::GET, endpoint, url, true).await
    }

    /// Submits a new order (requires authentication).
    pub async fn send_order(
        &self,
        params: HashMap<String, String>,
    ) -> anyhow::Result<FuturesSendOrderResponse, KrakenHttpError> {
        if self.credential.is_none() {
            return Err(KrakenHttpError::AuthenticationError(
                "API credentials required for sending orders".to_string(),
            ));
        }

        let endpoint = "/derivatives/api/v3/sendorder";
        self.send_request_with_body(endpoint, params).await
    }

    /// Submits a new order using typed parameters (requires authentication).
    pub async fn send_order_params(
        &self,
        params: &KrakenFuturesSendOrderParams,
    ) -> anyhow::Result<FuturesSendOrderResponse, KrakenHttpError> {
        if self.credential.is_none() {
            return Err(KrakenHttpError::MissingCredentials);
        }

        let endpoint = "/derivatives/api/v3/sendorder";
        self.send_request_with_params(endpoint, params).await
    }

    /// Cancels an open order (requires authentication).
    pub async fn cancel_order(
        &self,
        order_id: Option<String>,
        cli_ord_id: Option<String>,
    ) -> anyhow::Result<FuturesCancelOrderResponse, KrakenHttpError> {
        if self.credential.is_none() {
            return Err(KrakenHttpError::MissingCredentials);
        }

        let mut params = HashMap::new();

        if let Some(id) = order_id {
            params.insert("order_id".to_string(), id);
        }

        if let Some(id) = cli_ord_id {
            params.insert("cliOrdId".to_string(), id);
        }

        let endpoint = "/derivatives/api/v3/cancelorder";
        self.send_request_with_body(endpoint, params).await
    }

    /// Edits an existing order (requires authentication).
    pub async fn edit_order(
        &self,
        params: &KrakenFuturesEditOrderParams,
    ) -> anyhow::Result<FuturesEditOrderResponse, KrakenHttpError> {
        if self.credential.is_none() {
            return Err(KrakenHttpError::MissingCredentials);
        }

        let endpoint = "/derivatives/api/v3/editorder";
        self.send_request_with_params(endpoint, params).await
    }

    /// Submits multiple orders in a single batch request (requires authentication).
    pub async fn batch_order(
        &self,
        params: HashMap<String, String>,
    ) -> anyhow::Result<FuturesBatchOrderResponse, KrakenHttpError> {
        if self.credential.is_none() {
            return Err(KrakenHttpError::AuthenticationError(
                "API credentials required for batch orders".to_string(),
            ));
        }

        let endpoint = "/derivatives/api/v3/batchorder";
        self.send_request_with_body(endpoint, params).await
    }

    /// Cancels multiple orders in a single batch request (requires authentication).
    pub async fn cancel_orders_batch(
        &self,
        order_ids: Vec<String>,
    ) -> anyhow::Result<FuturesBatchCancelResponse, KrakenHttpError> {
        let batch_items: Vec<KrakenFuturesBatchCancelItem> = order_ids
            .into_iter()
            .map(KrakenFuturesBatchCancelItem::from_order_id)
            .collect();

        self.cancel_order_items_batch(batch_items).await
    }

    /// Cancels multiple order IDs or client order IDs in a single batch request
    /// (requires authentication).
    pub async fn cancel_order_items_batch(
        &self,
        batch_items: Vec<KrakenFuturesBatchCancelItem>,
    ) -> anyhow::Result<FuturesBatchCancelResponse, KrakenHttpError> {
        if self.credential.is_none() {
            return Err(KrakenHttpError::MissingCredentials);
        }

        let params = KrakenFuturesBatchOrderParams::new(batch_items);
        let post_data = params
            .to_body()
            .map_err(|e| KrakenHttpError::ParseError(format!("Failed to serialize batch: {e}")))?;

        let endpoint = "/derivatives/api/v3/batchorder";
        self.send_authenticated_post(endpoint, post_data).await
    }

    /// Submits multiple orders in a single batch request (requires authentication).
    pub async fn submit_orders_batch(
        &self,
        items: Vec<KrakenFuturesBatchSendItem>,
    ) -> anyhow::Result<FuturesBatchOrderResponse, KrakenHttpError> {
        if self.credential.is_none() {
            return Err(KrakenHttpError::MissingCredentials);
        }

        let params = KrakenFuturesBatchOrderParams::new(items);
        let post_data = params.to_body().map_err(|e| {
            KrakenHttpError::RequestNotStarted(format!("Failed to serialize batch: {e}"))
        })?;

        let endpoint = "/derivatives/api/v3/batchorder";
        self.send_authenticated_post(endpoint, post_data).await
    }

    /// Edits multiple orders in a single batch request (requires authentication).
    pub async fn edit_orders_batch(
        &self,
        items: Vec<KrakenFuturesBatchEditItem>,
    ) -> anyhow::Result<FuturesBatchOrderResponse, KrakenHttpError> {
        if self.credential.is_none() {
            return Err(KrakenHttpError::AuthenticationError(
                "API credentials required for batch orders".to_string(),
            ));
        }

        let params = KrakenFuturesBatchOrderParams::new(items);
        let post_data = params
            .to_body()
            .map_err(|e| KrakenHttpError::ParseError(format!("Failed to serialize batch: {e}")))?;

        let endpoint = "/derivatives/api/v3/batchorder";
        self.send_authenticated_post(endpoint, post_data).await
    }

    /// Cancels all open orders, optionally filtered by symbol (requires authentication).
    pub async fn cancel_all_orders(
        &self,
        symbol: Option<String>,
    ) -> anyhow::Result<FuturesCancelAllOrdersResponse, KrakenHttpError> {
        if self.credential.is_none() {
            return Err(KrakenHttpError::MissingCredentials);
        }

        let mut params = HashMap::new();

        if let Some(sym) = symbol {
            params.insert("symbol".to_string(), sym);
        }

        let endpoint = "/derivatives/api/v3/cancelallorders";
        self.send_request_with_body(endpoint, params).await
    }
}

pub(crate) type FuturesBatchOrder = (
    InstrumentId,
    ClientOrderId,
    OrderSide,
    OrderType,
    Quantity,
    TimeInForce,
    Option<Price>,
    Option<Price>,
    Option<TriggerType>,
    bool,
    bool,
);

pub(crate) struct FuturesBatchSubmitItem {
    pub result: KrakenApiResult,
    pub status: FuturesSendStatus,
}

/// High-level HTTP client for the Kraken Futures REST API.
///
/// This client wraps the raw client and provides Nautilus domain types.
/// It maintains an instrument cache and uses it to parse venue responses
/// into Nautilus domain objects.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.kraken", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.kraken")
)]
pub struct KrakenFuturesHttpClient {
    pub(crate) inner: Arc<KrakenFuturesRawHttpClient>,
    pub(crate) instruments_cache: Arc<AtomicMap<Ustr, InstrumentAny>>,
    clock: &'static AtomicTime,
    cache_initialized: Arc<AtomicBool>,
}

impl Clone for KrakenFuturesHttpClient {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            instruments_cache: self.instruments_cache.clone(),
            cache_initialized: self.cache_initialized.clone(),
            clock: self.clock,
        }
    }
}

impl Default for KrakenFuturesHttpClient {
    fn default() -> Self {
        Self::new(
            KrakenEnvironment::Live,
            None,
            60,
            None,
            None,
            None,
            None,
            KRAKEN_FUTURES_DEFAULT_RATE_LIMIT_PER_SECOND,
        )
        .expect("Failed to create default KrakenFuturesHttpClient")
    }
}

impl Debug for KrakenFuturesHttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(KrakenFuturesHttpClient))
            .field("inner", &self.inner)
            .finish()
    }
}

impl KrakenFuturesHttpClient {
    /// Creates a new [`KrakenFuturesHttpClient`].
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        environment: KrakenEnvironment,
        base_url_override: Option<String>,
        timeout_secs: u64,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        proxy_url: Option<String>,
        max_requests_per_second: u32,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            inner: Arc::new(KrakenFuturesRawHttpClient::new(
                environment,
                base_url_override,
                timeout_secs,
                max_retries,
                retry_delay_ms,
                retry_delay_max_ms,
                proxy_url,
                max_requests_per_second,
            )?),
            instruments_cache: Arc::new(AtomicMap::new()),
            cache_initialized: Arc::new(AtomicBool::new(false)),
            clock: get_atomic_clock_realtime(),
        })
    }

    /// Creates a new [`KrakenFuturesHttpClient`] with credentials.
    #[expect(clippy::too_many_arguments)]
    pub fn with_credentials(
        api_key: String,
        api_secret: String,
        environment: KrakenEnvironment,
        base_url_override: Option<String>,
        timeout_secs: u64,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        proxy_url: Option<String>,
        max_requests_per_second: u32,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            inner: Arc::new(KrakenFuturesRawHttpClient::with_credentials(
                api_key,
                api_secret,
                environment,
                base_url_override,
                timeout_secs,
                max_retries,
                retry_delay_ms,
                retry_delay_max_ms,
                proxy_url,
                max_requests_per_second,
            )?),
            instruments_cache: Arc::new(AtomicMap::new()),
            cache_initialized: Arc::new(AtomicBool::new(false)),
            clock: get_atomic_clock_realtime(),
        })
    }

    /// Creates a new [`KrakenFuturesHttpClient`] loading credentials from environment variables.
    ///
    /// Looks for `KRAKEN_FUTURES_API_KEY` and `KRAKEN_FUTURES_API_SECRET` (live)
    /// or `KRAKEN_FUTURES_DEMO_API_KEY` and `KRAKEN_FUTURES_DEMO_API_SECRET` (demo).
    ///
    /// Falls back to unauthenticated client if credentials are not set.
    #[expect(clippy::too_many_arguments)]
    pub fn from_env(
        environment: KrakenEnvironment,
        base_url_override: Option<String>,
        timeout_secs: u64,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        proxy_url: Option<String>,
        max_requests_per_second: u32,
    ) -> anyhow::Result<Self> {
        let demo = environment == KrakenEnvironment::Demo;

        if let Some(credential) = KrakenCredential::from_env_futures(demo) {
            let (api_key, api_secret) = credential.into_parts();
            Self::with_credentials(
                api_key,
                api_secret,
                environment,
                base_url_override,
                timeout_secs,
                max_retries,
                retry_delay_ms,
                retry_delay_max_ms,
                proxy_url,
                max_requests_per_second,
            )
        } else {
            Self::new(
                environment,
                base_url_override,
                timeout_secs,
                max_retries,
                retry_delay_ms,
                retry_delay_max_ms,
                proxy_url,
                max_requests_per_second,
            )
        }
    }

    /// Cancels all pending HTTP requests.
    pub fn cancel_all_requests(&self) {
        self.inner.cancel_all_requests();
    }

    /// Replaces the canceled token so requests can proceed after reconnect.
    pub fn reset_cancellation_token(&self) {
        self.inner.reset_cancellation_token();
    }

    /// Returns a clone of the current cancellation token.
    pub fn cancellation_token(&self) -> CancellationToken {
        self.inner.cancellation_token()
    }

    /// Caches an instrument for symbol lookup.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        self.instruments_cache
            .insert(instrument.symbol().inner(), instrument);
        self.cache_initialized.store(true, Ordering::Release);
    }

    /// Caches multiple instruments for symbol lookup.
    pub fn cache_instruments(&self, instruments: &[InstrumentAny]) {
        self.instruments_cache.rcu(|m| {
            for instrument in instruments {
                m.insert(instrument.symbol().inner(), instrument.clone());
            }
        });
        self.cache_initialized.store(true, Ordering::Release);
    }

    /// Gets an instrument from the cache by symbol.
    pub fn get_cached_instrument(&self, symbol: &Ustr) -> Option<InstrumentAny> {
        self.instruments_cache.get_cloned(symbol)
    }

    fn get_instrument_by_raw_symbol(&self, raw_symbol: &str) -> Option<InstrumentAny> {
        self.instruments_cache
            .load()
            .values()
            .find(|inst| inst.raw_symbol().as_str() == raw_symbol)
            .cloned()
    }

    /// Resolves the contract a history row names, which the venue may spell in another case than
    /// the listing (the documented example is `pi_xbtusd`), so the exact spelling is tried first
    /// and a case-insensitive match second. A spelling matching two contracts resolves to neither.
    fn get_instrument_by_history_tradeable(&self, tradeable: &str) -> Option<InstrumentAny> {
        self.get_instrument_by_raw_symbol(tradeable).or_else(|| {
            let instruments = self.instruments_cache.load();
            let mut matches = instruments
                .values()
                .filter(|inst| inst.raw_symbol().as_str().eq_ignore_ascii_case(tradeable));
            let instrument = matches.next()?;

            matches.next().is_none().then(|| instrument.clone())
        })
    }

    fn generate_ts_init(&self) -> UnixNanos {
        self.clock.get_time_ns()
    }

    /// Requests the complete tradable instrument catalog from Kraken Futures.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying request fails or any instrument definition cannot be
    /// parsed. An instrument parse failure returns [`KrakenHttpError::ParseError`] without a
    /// partial catalog.
    pub async fn request_instruments(&self) -> anyhow::Result<Vec<InstrumentAny>, KrakenHttpError> {
        let ts_init = self.generate_ts_init();
        let response = self.inner.get_instruments().await?;

        response
            .instruments
            .iter()
            .map(|fut_instrument| {
                parse_futures_instrument(fut_instrument, ts_init, ts_init)
                    .map_err(|e| KrakenHttpError::ParseError(e.to_string()))
            })
            .collect()
    }

    /// Requests the current market status for Kraken Futures instruments.
    pub async fn request_instrument_statuses(
        &self,
    ) -> anyhow::Result<AHashMap<InstrumentId, MarketStatusAction>, KrakenHttpError> {
        let response = self.inner.get_instruments().await?;

        Ok(response
            .instruments
            .iter()
            .map(|instrument| {
                let instrument_id =
                    InstrumentId::new(Symbol::new(&instrument.symbol), *KRAKEN_VENUE);
                let action = if instrument.tradeable {
                    MarketStatusAction::Trading
                } else {
                    MarketStatusAction::NotAvailableForTrading
                };

                (instrument_id, action)
            })
            .collect())
    }

    /// Requests the mark price for an instrument.
    pub async fn request_mark_price(
        &self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<Decimal, KrakenHttpError> {
        let instrument = self
            .get_cached_instrument(&instrument_id.symbol.inner())
            .ok_or_else(|| {
                KrakenHttpError::ParseError(
                    InstrumentLookupError::not_found(instrument_id).to_string(),
                )
            })?;

        let raw_symbol = instrument.raw_symbol().to_string();
        let tickers = self.inner.get_tickers().await?;

        tickers
            .tickers
            .iter()
            .find(|t| t.symbol == raw_symbol)
            .ok_or_else(|| {
                KrakenHttpError::ParseError(format!("Symbol {raw_symbol} not found in tickers"))
            })
            .and_then(|t| {
                t.mark_price.ok_or_else(|| {
                    KrakenHttpError::ParseError(format!(
                        "Mark price not available for {raw_symbol} (may not be available in testnet)"
                    ))
                })
            })
    }

    pub async fn request_index_price(
        &self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<Decimal, KrakenHttpError> {
        let instrument = self
            .get_cached_instrument(&instrument_id.symbol.inner())
            .ok_or_else(|| {
                KrakenHttpError::ParseError(
                    InstrumentLookupError::not_found(instrument_id).to_string(),
                )
            })?;

        let raw_symbol = instrument.raw_symbol().to_string();
        let tickers = self.inner.get_tickers().await?;

        tickers
            .tickers
            .iter()
            .find(|t| t.symbol == raw_symbol)
            .ok_or_else(|| {
                KrakenHttpError::ParseError(format!("Symbol {raw_symbol} not found in tickers"))
            })
            .and_then(|t| {
                t.index_price.ok_or_else(|| {
                    KrakenHttpError::ParseError(format!(
                        "Index price not available for {raw_symbol} (may not be available in testnet)"
                    ))
                })
            })
    }

    pub async fn request_trades(
        &self,
        instrument_id: InstrumentId,
        start: Option<Timestamp>,
        end: Option<Timestamp>,
        limit: Option<u64>,
    ) -> anyhow::Result<Vec<TradeTick>, KrakenHttpError> {
        let instrument = self
            .get_cached_instrument(&instrument_id.symbol.inner())
            .ok_or_else(|| {
                KrakenHttpError::ParseError(
                    InstrumentLookupError::not_found(instrument_id).to_string(),
                )
            })?;

        let raw_symbol = instrument.raw_symbol().to_string();
        let ts_init = self.generate_ts_init();

        let since = start.map(|dt| dt.as_millisecond());
        let before = end.map(|dt| dt.as_millisecond());

        // Executions are oldest-anchored for `sort=asc`; count-only fetches the
        // newest page with `sort=desc` (reversed to ascending below)
        let sort = if start.is_some() { "asc" } else { "desc" };

        let response = self
            .inner
            .get_public_executions(&raw_symbol, since, before, Some(sort), None)
            .await?;

        let mut trades = Vec::new();

        for element in &response.elements {
            let execution = &element.event.execution.execution;
            match parse_futures_public_execution(execution, &instrument, ts_init) {
                Ok(trade_tick) => trades.push(trade_tick),
                Err(e) => {
                    log::warn!("Failed to parse futures trade tick: {e}");
                }
            }
        }

        if start.is_none() {
            trades.reverse();
        }

        apply_count_limit(&mut trades, start, limit);

        Ok(trades)
    }

    pub async fn request_bars(
        &self,
        bar_type: BarType,
        start: Option<Timestamp>,
        end: Option<Timestamp>,
        limit: Option<u64>,
    ) -> anyhow::Result<Vec<Bar>, KrakenHttpError> {
        let instrument_id = bar_type.instrument_id();
        let instrument = self
            .get_cached_instrument(&instrument_id.symbol.inner())
            .ok_or_else(|| {
                KrakenHttpError::ParseError(
                    InstrumentLookupError::not_found(instrument_id).to_string(),
                )
            })?;

        let raw_symbol = instrument.raw_symbol().to_string();
        let ts_init = self.generate_ts_init();
        let tick_type = "trade";
        let resolution = bar_type_to_futures_resolution(bar_type)
            .map_err(|e| KrakenHttpError::ParseError(e.to_string()))?;

        // Kraken Futures OHLC API expects Unix timestamp in seconds
        let from = start.map(|dt| dt.as_second());
        let to = end.map(|dt| dt.as_second());
        let end_ns = end.map(|dt| u64::try_from(dt.as_nanosecond()).unwrap_or(0));

        let response = self
            .inner
            .get_ohlc(tick_type, &raw_symbol, resolution, from, to)
            .await?;

        let mut bars = Vec::new();

        for candle in response.candles {
            let ohlc = OhlcData {
                time: candle.time / 1000,
                open: candle.open,
                high: candle.high,
                low: candle.low,
                close: candle.close,
                vwap: "0".to_string(),
                volume: candle.volume,
                count: 0,
            };

            match parse_bar(&ohlc, &instrument, bar_type, ts_init) {
                Ok(bar) => {
                    if let Some(end_nanos) = end_ns
                        && bar.ts_event.as_u64() > end_nanos
                    {
                        continue;
                    }
                    bars.push(bar);
                }
                Err(e) => {
                    log::warn!("Failed to parse futures bar: {e}");
                }
            }
        }

        // Kraken returns the page oldest-first; keep the most recent `limit`
        // bars for count-only requests rather than the oldest (issue #4254).
        apply_count_limit(&mut bars, start, limit);

        Ok(bars)
    }

    /// Requests an order book snapshot for a futures instrument.
    pub async fn request_book_snapshot(
        &self,
        instrument_id: InstrumentId,
        depth: Option<u32>,
    ) -> anyhow::Result<OrderBook, KrakenHttpError> {
        let instrument = self
            .get_cached_instrument(&instrument_id.symbol.inner())
            .ok_or_else(|| {
                KrakenHttpError::ParseError(
                    InstrumentLookupError::not_found(instrument_id).to_string(),
                )
            })?;

        let raw_symbol = instrument.raw_symbol().to_string();
        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();
        let ts_event = self.generate_ts_init();

        let response = self.inner.get_orderbook(&raw_symbol).await?;
        let book_data = &response.order_book;

        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

        let bid_limit = depth.map_or(book_data.bids.len(), |d| {
            (d as usize).min(book_data.bids.len())
        });
        let ask_limit = depth.map_or(book_data.asks.len(), |d| {
            (d as usize).min(book_data.asks.len())
        });

        // Pass sequence=0 so the snapshot does not advance the book's high-water sequence,
        // the WS subscription owns sequencing once it starts streaming deltas.
        for (i, level) in book_data.bids.iter().take(bid_limit).enumerate() {
            let price = Price::from_decimal_dp(level.price, price_precision)
                .map_err(|e| KrakenHttpError::ParseError(e.to_string()))?;
            let size = Quantity::from_decimal_dp(level.qty, size_precision)
                .map_err(|e| KrakenHttpError::ParseError(e.to_string()))?;
            let order = BookOrder::new(OrderSide::Buy, price, size, i as u64);
            book.add(order, 0, 0, ts_event);
        }

        for (i, level) in book_data.asks.iter().take(ask_limit).enumerate() {
            let price = Price::from_decimal_dp(level.price, price_precision)
                .map_err(|e| KrakenHttpError::ParseError(e.to_string()))?;
            let size = Quantity::from_decimal_dp(level.qty, size_precision)
                .map_err(|e| KrakenHttpError::ParseError(e.to_string()))?;
            let order = BookOrder::new(OrderSide::Sell, price, size, (bid_limit + i) as u64);
            book.add(order, 0, 0, ts_event);
        }

        Ok(book)
    }

    /// Requests historical funding rates for a futures instrument.
    ///
    /// Kraken returns all available rates; client-side filtering applies
    /// the `start`, `end`, and `limit` constraints from the caller.
    pub async fn request_funding_rates(
        &self,
        instrument_id: InstrumentId,
        start: Option<Timestamp>,
        end: Option<Timestamp>,
        limit: Option<usize>,
    ) -> anyhow::Result<Vec<FundingRateUpdate>, KrakenHttpError> {
        let instrument = self
            .get_cached_instrument(&instrument_id.symbol.inner())
            .ok_or_else(|| {
                KrakenHttpError::ParseError(
                    InstrumentLookupError::not_found(instrument_id).to_string(),
                )
            })?;

        let raw_symbol = instrument.raw_symbol().to_string();
        let ts_init = self.generate_ts_init();
        let start_ns = start.map(|dt| u64::try_from(dt.as_nanosecond()).unwrap_or(0));
        let end_ns = end.map(|dt| u64::try_from(dt.as_nanosecond()).unwrap_or(0));

        let response = self.inner.get_historical_funding_rates(&raw_symbol).await?;

        let mut rates = Vec::new();

        for entry in &response.rates {
            let ts_event = entry.timestamp.parse::<Timestamp>().map_or(ts_init, |dt| {
                UnixNanos::from(u64::try_from(dt.as_nanosecond()).unwrap_or(0))
            });

            if let Some(s) = start_ns
                && ts_event.as_u64() < s
            {
                continue;
            }

            if let Some(e) = end_ns
                && ts_event.as_u64() > e
            {
                continue;
            }

            rates.push(FundingRateUpdate::new(
                instrument_id,
                entry.relative_funding_rate,
                None,
                None,
                ts_event,
                ts_init,
            ));

            if let Some(lim) = limit
                && rates.len() >= lim
            {
                break;
            }
        }

        // Kraken returns newest-first; reverse to ascending chronological order
        rates.reverse();

        Ok(rates)
    }

    /// Requests account state from the Kraken Futures exchange.
    ///
    /// This queries the accounts endpoint and converts the response into a
    /// Nautilus `AccountState` event containing balances and margin info.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - Response parsing fails.
    pub async fn request_account_state(
        &self,
        account_id: AccountId,
    ) -> anyhow::Result<AccountState> {
        let accounts_response = self.inner.get_accounts().await?;

        if accounts_response.result != KrakenApiResult::Success {
            let error_msg = accounts_response
                .error
                .unwrap_or_else(|| "Unknown error".to_string());
            anyhow::bail!("Failed to get futures accounts: {error_msg}");
        }

        let ts_init = self.generate_ts_init();
        let (balances, margins) = parse_account_entries(&accounts_response.accounts);

        Ok(AccountState::new(
            account_id,
            AccountType::Margin,
            balances,
            margins,
            true,
            UUID4::new(),
            ts_init,
            ts_init,
            None,
        ))
    }

    pub async fn request_order_status_reports(
        &self,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
        start: Option<Timestamp>,
        end: Option<Timestamp>,
        open_only: bool,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        self.request_order_status_reports_checked(account_id, instrument_id, start, end, open_only)
            .await
            .map(|(reports, _)| reports)
    }

    /// Requests order status reports, also reporting what the read covered.
    ///
    /// An in-scope open order whose instrument cannot be resolved, or that cannot be parsed, fails
    /// the read, except an untouched reduce-only trigger listed without an unfilled size that no
    /// open position sizes, which is skipped, and an order a read scoped to one instrument cannot
    /// resolve, which may be on another instrument and is skipped. The coverage records each such
    /// skip, each history row in scope that cannot be parsed, resolved or represented, and a
    /// continuation token, which cuts the read short at the page's oldest event: the read takes one
    /// page, since every Kraken `/history` endpoint draws on one pool of 100 tokens, replenished at
    /// 100 every 10 minutes, at a token per page. A read scoped to an unheld futures instrument
    /// reads nothing and rules out no order. `ExecutionMassStatus::set_report_window` records
    /// whether it is complete for bounded history, and the single-order lookup whether it rules out
    /// an order it did not return.
    ///
    /// A skipped history row is out of scope only when the open-order snapshot reports its order,
    /// or when a scoped read finds it names both an order and another contract; a row that
    /// establishes neither stays in scope. An undocumented kind is always in scope. A row in scope
    /// at least as new as the state an order would be reported from withholds that order, so an
    /// unreadable newest row cannot fall back to an older state, whether that state is open or
    /// closed. A row that names no order can withhold nothing, and leaves the read incomplete.
    pub(crate) async fn request_order_status_reports_checked(
        &self,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
        start: Option<Timestamp>,
        end: Option<Timestamp>,
        open_only: bool,
    ) -> anyhow::Result<(Vec<OrderStatusReport>, ReportCoverage)> {
        let mut coverage = ReportCoverage::default();

        // A scoped read for an instrument this client does not hold can match nothing, so
        // return before the request rather than reporting every instrument's rows. A spot
        // instrument names none of this client's orders, so that read is complete; a futures one
        // the listing lacks read nothing, so it rules out none of its orders.
        if let Some(ref target_id) = instrument_id
            && self
                .get_cached_instrument(&target_id.symbol.inner())
                .is_none()
        {
            if product_type_from_symbol(target_id.symbol.as_str()) == KrakenProductType::Futures {
                coverage.truncate(None);
            }
            return Ok((Vec::new(), coverage));
        }
        let ts_init = self.generate_ts_init();
        let mut all_reports = Vec::new();

        let response = self
            .inner
            .get_open_orders()
            .await
            .map_err(|e| anyhow::anyhow!("get_open_orders failed: {e}"))?;

        if response.result != KrakenApiResult::Success {
            let error_msg = response
                .error
                .unwrap_or_else(|| "Unknown error".to_string());
            anyhow::bail!("Failed to get open orders: {error_msg}");
        }

        let position_sizes = if response
            .open_orders
            .iter()
            .any(|order| order.unfilled_size.is_none())
        {
            match self.inner.get_open_positions().await {
                Ok(response) if response.result == KrakenApiResult::Success => Ok(response
                    .open_positions
                    .into_iter()
                    .map(|position| (position.symbol, position.size))
                    .collect::<AHashMap<_, _>>()),
                Ok(response) => {
                    let reason = response
                        .error
                        .unwrap_or_else(|| "Unknown error".to_string());
                    log::warn!("Failed to get open positions for order quantities: {reason}");
                    Err(reason)
                }
                Err(e) => {
                    log::warn!("Failed to get open positions for order quantities: {e}");
                    Err(e.to_string())
                }
            }
        } else {
            Ok(AHashMap::new())
        };
        let mut skipped_open_order_ids = AHashSet::new();

        for order in &response.open_orders {
            // Resolve the row and compare instrument ids, so a scoped read cannot match on a
            // spelling and cannot fall through to every instrument when the id is not held.
            let resolved = self.get_instrument_by_raw_symbol(&order.symbol);
            if let Some(ref target_id) = instrument_id {
                match &resolved {
                    Some(inst) if inst.id() != *target_id => continue,
                    // Unresolved, the row may be the scoped instrument's own
                    None => {
                        coverage.skip(&order.order_id);
                        continue;
                    }
                    Some(_) => {}
                }
            }

            if let Some(instrument) = resolved {
                let sized_by_position = order.unfilled_size.is_none()
                    && matches!(
                        order.order_type,
                        KrakenFuturesOrderType::Stop
                            | KrakenFuturesOrderType::StopLower
                            | KrakenFuturesOrderType::StopLoss
                            | KrakenFuturesOrderType::TakeProfit
                    )
                    && order.status == KrakenFuturesOrderStatus::Untouched
                    && order.reduce_only == Some(true);
                let position_size = match &position_sizes {
                    Ok(sizes) if sized_by_position => sizes.get(&order.symbol).copied(),
                    _ => None,
                };

                // Such a trigger has no size of its own until it fires. With no open position on
                // its contract it has none to report, and failing would fail every unscoped
                // futures order read while it rests, so it is skipped, its history rows with it;
                // a failed positions read leaves its size unknown instead, which fails the read.
                if sized_by_position && position_size.is_none() {
                    if let Err(reason) = &position_sizes {
                        anyhow::bail!(
                            "OpenOrders: futures order {} lists no unfilled size and the open \
                             positions read failed: {reason}",
                            order.order_id
                        );
                    }
                    log::warn!(
                        "Skipping futures order {} on {}: the venue lists no unfilled size and no \
                         open position sizes it; the set is incomplete",
                        order.order_id,
                        order.symbol
                    );
                    coverage.skip(&order.order_id);

                    // No history row can carry an invalid ID, and `skip` has marked the set
                    // unidentified, so an invalid ID needs no entry here.
                    if let Ok(venue_order_id) = VenueOrderId::new_checked(order.order_id.as_str()) {
                        skipped_open_order_ids.insert(venue_order_id);
                    }
                    continue;
                }

                // An in-scope open order the client cannot parse fails the read: dropped, it reads
                // to reconciliation as an order the venue does not have.
                let report = parse_futures_order_status_report(
                    order,
                    &instrument,
                    account_id,
                    position_size,
                    ts_init,
                )
                .map_err(|e| {
                    anyhow::anyhow!(
                        "OpenOrders: failed to parse futures order {}: {e}",
                        order.order_id
                    )
                })?;
                all_reports.push(report);
            } else {
                // An in-scope open order the client cannot resolve fails the read, as the spot
                // client's does: dropped, it reads to reconciliation as an order the venue never had.
                anyhow::bail!(
                    "OpenOrders: instrument not in cache for futures symbol {}",
                    order.symbol
                );
            }
        }

        if !open_only {
            // Kraken Futures order events API expects Unix timestamp in milliseconds
            let start_ms = start.map(|dt| dt.as_millisecond());
            let end_ms = end.map(|dt| dt.as_millisecond());
            // Typed, so the mass status can tell a refusal from a fault; the message repeats the
            // cause for callers that print only the outermost context.
            let response = self
                .inner
                .get_order_events(end_ms, start_ms, None)
                .await
                .map_err(|e| {
                    let message = format!("get_order_events failed: {e}");
                    anyhow::Error::new(e).context(message)
                })?;

            // A page that hands back a continuation token leaves events of the window unread.
            if response
                .continuation_token
                .as_deref()
                .is_some_and(|token| !token.is_empty())
            {
                log::warn!(
                    "Order history window since={start_ms:?} before={end_ms:?} holds more events than one page; the events beyond it are not read, marking the set incomplete"
                );
                // The page holds the newest events, so it covers every event from its oldest one
                // on; a page with no usable event time covers nothing it can place.
                coverage.truncate(
                    response
                        .oldest_event_ms
                        .and_then(UnixNanos::from_millis_checked),
                );
            }

            // The history lists every lifecycle event, so an order appears once per event. Each
            // report reconciles against the same cached state, so the read hands back one report
            // per order: the open-order snapshot when the venue still lists it, else the latest
            // history state. An open order the snapshot skips is not reported from its history.
            let open_order_ids: AHashSet<VenueOrderId> = all_reports
                .iter()
                .map(|r| r.venue_order_id)
                .chain(skipped_open_order_ids.iter().copied())
                .collect();
            let mut latest: IndexMap<VenueOrderId, (OrderStatusReport, i64)> = IndexMap::new();
            let mut unread_ms: AHashMap<VenueOrderId, i64> = AHashMap::new();
            // Kraken documents the `IoC` and `Post` types on history rows but not on the open-order
            // snapshot, so a snapshot report takes its time in force and post-only from the newest
            // readable row of its order, the first listed on a tie. Kraken's edit takes no order
            // type, and a stop reports `GTC` before and after it triggers, so an older row states
            // the time in force an unread newer one would; at worst it misses a post-only order a
            // trigger fired, which the snapshot misses too.
            let mut open_time_in_force: AHashMap<VenueOrderId, (i64, TimeInForce, bool)> =
                AHashMap::new();
            let mut skipped_in_scope = 0usize;

            for row in &response.skipped_rows {
                let snapshot_covers = row
                    .order_id
                    .as_deref()
                    .is_some_and(|order_id| open_order_ids.contains(&VenueOrderId::new(order_id)));
                let on_another_contract = row.order_id.is_some()
                    && instrument_id.as_ref().is_some_and(|target_id| {
                        row.tradeable
                            .as_deref()
                            .and_then(|tradeable| {
                                self.get_instrument_by_history_tradeable(tradeable)
                            })
                            .is_some_and(|instrument| instrument.id() != *target_id)
                    });

                if !row.undocumented && (snapshot_covers || on_another_contract) {
                    continue;
                }
                skipped_in_scope += 1;

                if row.undocumented {
                    coverage.skip_unidentified();
                }

                match row.order_id.as_deref() {
                    Some(order_id) => {
                        record_unread_row(&mut coverage, &mut unread_ms, order_id, row.event_ms);
                    }
                    None => coverage.skip_unidentified(),
                }
            }

            if skipped_in_scope > 0 {
                log::warn!(
                    "Order history page had {skipped_in_scope} row(s) in scope the adapter could not represent; marking the set incomplete",
                );
            }

            for event_wrapper in response.order_events {
                let event = &event_wrapper.order;
                let venue_order_id = VenueOrderId::new(&event.order_id);

                if open_order_ids.contains(&venue_order_id) {
                    match open_time_in_force.get(&venue_order_id) {
                        Some((newest_ms, _, _)) if *newest_ms >= event_wrapper.event_ms => {}
                        _ => {
                            open_time_in_force.insert(
                                venue_order_id,
                                (event_wrapper.event_ms, event.time_in_force, event.post_only),
                            );
                        }
                    }
                    continue;
                }

                // Resolve the row and compare instrument ids, so a scoped read cannot match on a
                // spelling and cannot fall through to every instrument when the id is not held.
                let resolved = self.get_instrument_by_history_tradeable(&event.symbol);
                if let Some(ref target_id) = instrument_id {
                    match &resolved {
                        Some(inst) if inst.id() != *target_id => continue,
                        // Unresolved, the row may be the scoped instrument's own
                        None => {
                            record_unread_row(
                                &mut coverage,
                                &mut unread_ms,
                                &event.order_id,
                                event_wrapper.event_ms,
                            );
                            continue;
                        }
                        Some(_) => {}
                    }
                }

                if let Some(instrument) = resolved {
                    match parse_futures_order_event_status_report(
                        event,
                        Some(event_wrapper.event_type),
                        &instrument,
                        account_id,
                        ts_init,
                    ) {
                        Ok(report) => match latest.get(&report.venue_order_id) {
                            Some((existing, _)) if !supersedes(&report, existing) => {}
                            _ => {
                                latest.insert(
                                    report.venue_order_id,
                                    (report, event_wrapper.state_ms),
                                );
                            }
                        },
                        Err(e) => {
                            let order_id = &event.order_id;
                            log::warn!("Failed to parse futures order event {order_id}: {e}");
                            record_unread_row(
                                &mut coverage,
                                &mut unread_ms,
                                order_id,
                                event_wrapper.event_ms,
                            );
                        }
                    }
                } else {
                    log::warn!(
                        "Instrument not in cache for futures symbol {}, skipping order event",
                        event.symbol
                    );
                    record_unread_row(
                        &mut coverage,
                        &mut unread_ms,
                        &event.order_id,
                        event_wrapper.event_ms,
                    );
                }
            }

            for report in &mut all_reports {
                if let Some(&(_, time_in_force, post_only)) =
                    open_time_in_force.get(&report.venue_order_id)
                {
                    report.time_in_force = time_in_force;
                    report.post_only = post_only;
                }
            }

            for (report, state_ms) in latest.into_values() {
                if unread_ms
                    .get(&report.venue_order_id)
                    .is_some_and(|newest_unread_ms| *newest_unread_ms >= state_ms)
                {
                    log::warn!(
                        "Withholding futures order {} from the order read: a history row that could not be read is not older than its latest readable state",
                        report.venue_order_id
                    );
                    coverage.withhold(report);
                } else {
                    all_reports.push(report);
                }
            }
        }

        Ok((all_reports, coverage))
    }

    /// Requests order status reports from the venue's `/orders/status`
    /// 5-second window for the given venue order IDs and client order IDs.
    ///
    /// Orders the venue no longer reports within that window are simply absent
    /// from the result, which callers treat as absence of recent evidence.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying request fails or the venue rejects
    /// it.
    pub async fn request_orders_status_reports(
        &self,
        account_id: AccountId,
        order_ids: &[String],
        cli_ord_ids: &[String],
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let ts_init = self.generate_ts_init();

        let response = self
            .inner
            .get_orders_status(order_ids, cli_ord_ids)
            .await
            .map_err(|e| anyhow::anyhow!("get_orders_status failed: {e}"))?;

        if response.result != KrakenApiResult::Success {
            let error_msg = response
                .error
                .unwrap_or_else(|| "Unknown error".to_string());
            anyhow::bail!("Failed to get orders status: {error_msg}");
        }

        let mut reports = Vec::with_capacity(response.orders.len());
        for details in &response.orders {
            let Some(instrument) = self.get_instrument_by_raw_symbol(&details.order.symbol) else {
                anyhow::bail!(
                    "No cached instrument for symbol: {}, order {} cannot be reported; \
                     treating the lookup as failed rather than the order as absent",
                    details.order.symbol,
                    details.order.order_id,
                );
            };

            match parse_futures_order_status_details_report(
                details,
                &instrument,
                account_id,
                ts_init,
            ) {
                Ok(report) => reports.push(report),
                Err(e) => anyhow::bail!(
                    "Failed to parse futures order status {}: {e}; treating the lookup as \
                     failed rather than the order as absent",
                    details.order.order_id,
                ),
            }
        }

        Ok(reports)
    }

    pub async fn request_fill_reports(
        &self,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
        start: Option<Timestamp>,
        end: Option<Timestamp>,
    ) -> anyhow::Result<Vec<FillReport>> {
        self.request_fill_reports_checked(account_id, instrument_id, start, end)
            .await
            .map(|(reports, _)| reports)
    }

    /// Requests fill reports, also reporting whether the set is complete.
    ///
    /// The flag is `false` when an in-scope fill cannot be resolved or parsed, which
    /// `ExecutionMassStatus::set_report_window` records for bounded history.
    pub(crate) async fn request_fill_reports_checked(
        &self,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
        start: Option<Timestamp>,
        end: Option<Timestamp>,
    ) -> anyhow::Result<(Vec<FillReport>, bool)> {
        let mut complete = true;
        let ts_init = self.generate_ts_init();
        let mut all_reports = Vec::new();

        // As above: a scoped read for an instrument this client does not hold matches nothing.
        if let Some(ref target_id) = instrument_id
            && self
                .get_cached_instrument(&target_id.symbol.inner())
                .is_none()
        {
            return Ok((all_reports, complete));
        }

        let response = self.inner.get_fills(None).await?;
        if response.result != KrakenApiResult::Success {
            let error_msg = response
                .error
                .unwrap_or_else(|| "Unknown error".to_string());
            anyhow::bail!("Failed to get fills: {error_msg}");
        }

        let start_ms = start.map(|dt| dt.as_millisecond());
        let end_ms = end.map(|dt| dt.as_millisecond());

        for fill in response.fills {
            if let Some(start_threshold) = start_ms
                && let Ok(fill_ts) = fill.fill_time.parse::<Timestamp>()
            {
                let fill_ms = fill_ts.as_millisecond();
                if fill_ms < start_threshold {
                    continue;
                }
            }

            if let Some(end_threshold) = end_ms
                && let Ok(fill_ts) = fill.fill_time.parse::<Timestamp>()
            {
                let fill_ms = fill_ts.as_millisecond();
                if fill_ms > end_threshold {
                    continue;
                }
            }

            // Resolve the row and compare instrument ids, so a scoped read cannot match on a
            // spelling and cannot fall through to every instrument when the id is not held.
            let resolved = self.get_instrument_by_raw_symbol(&fill.symbol);
            if let Some(ref target_id) = instrument_id
                && resolved.as_ref().is_none_or(|inst| inst.id() != *target_id)
            {
                continue;
            }

            if let Some(instrument) = resolved {
                match parse_futures_fill_report(&fill, &instrument, account_id, ts_init) {
                    Ok(report) => all_reports.push(report),
                    Err(e) => {
                        let fill_id = &fill.fill_id;
                        log::warn!("Failed to parse futures fill {fill_id}: {e}");
                        complete = false;
                    }
                }
            } else {
                log::warn!(
                    "Instrument not in cache for futures symbol {}, skipping fill",
                    fill.symbol
                );
                complete = false;
            }
        }

        Ok((all_reports, complete))
    }

    pub async fn request_position_status_reports(
        &self,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let ts_init = self.generate_ts_init();
        let mut all_reports = Vec::new();

        // As above: a scoped read for an instrument this client does not hold matches nothing.
        if let Some(ref target_id) = instrument_id
            && self
                .get_cached_instrument(&target_id.symbol.inner())
                .is_none()
        {
            return Ok(all_reports);
        }

        let response = self.inner.get_open_positions().await?;
        if response.result != KrakenApiResult::Success {
            let error_msg = response
                .error
                .unwrap_or_else(|| "Unknown error".to_string());
            anyhow::bail!("Failed to get open positions: {error_msg}");
        }

        for position in response.open_positions {
            // Resolve the row and compare instrument ids, so a scoped read cannot match on a
            // spelling and cannot fall through to every instrument when the id is not held.
            let resolved = self.get_instrument_by_raw_symbol(&position.symbol);
            if let Some(ref target_id) = instrument_id
                && resolved.as_ref().is_none_or(|inst| inst.id() != *target_id)
            {
                continue;
            }

            // An in-scope position the client cannot resolve or parse fails the read: dropped,
            // it reads to reconciliation as flat.
            let Some(instrument) = resolved else {
                anyhow::bail!(
                    "OpenPositions: instrument not in cache for futures symbol {}",
                    position.symbol
                );
            };

            let report =
                parse_futures_position_status_report(&position, &instrument, account_id, ts_init)
                    .map_err(|e| {
                    anyhow::anyhow!(
                        "OpenPositions: failed to parse futures position {}: {e}",
                        position.symbol
                    )
                })?;
            all_reports.push(report);
        }

        Ok(all_reports)
    }

    #[expect(clippy::too_many_arguments)]
    fn build_send_order_params(
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
        reduce_only: bool,
        post_only: bool,
    ) -> anyhow::Result<KrakenFuturesSendOrderParams> {
        let instrument = self
            .get_cached_instrument(&instrument_id.symbol.inner())
            .ok_or_else(|| InstrumentLookupError::not_found(instrument_id))?;

        let raw_symbol = instrument.raw_symbol().inner();

        // Map order type and time-in-force to Kraken order type
        // Kraken Futures encodes TIF in the orderType field:
        // - lmt = limit (GTC)
        // - ioc = immediate-or-cancel
        // - post = post-only (maker only)
        // - mkt = market
        let kraken_order_type = match order_type {
            OrderType::Market => KrakenFuturesOrderType::Market,
            OrderType::Limit => {
                if post_only {
                    KrakenFuturesOrderType::Post
                } else {
                    match time_in_force {
                        TimeInForce::Ioc => KrakenFuturesOrderType::Ioc,
                        TimeInForce::Fok => {
                            anyhow::bail!("FOK not supported by Kraken Futures, use IOC instead")
                        }
                        TimeInForce::Gtd => {
                            anyhow::bail!("GTD not supported by Kraken Futures, use GTC instead")
                        }
                        _ => KrakenFuturesOrderType::Limit, // GTC is default
                    }
                }
            }
            OrderType::StopMarket | OrderType::StopLimit => KrakenFuturesOrderType::Stop,
            OrderType::MarketIfTouched | OrderType::LimitIfTouched => {
                KrakenFuturesOrderType::TakeProfit
            }
            _ => anyhow::bail!("Unsupported order type: {order_type:?}"),
        };

        let kraken_side = KrakenOrderSide::from(order_side);

        let mut builder = KrakenFuturesSendOrderParamsBuilder::default();
        builder
            .cli_ord_id(client_order_id.to_string())
            .broker(NAUTILUS_KRAKEN_BROKER_ID)
            .symbol(raw_symbol)
            .side(kraken_side)
            .size(quantity.to_string())
            .order_type(kraken_order_type);

        if matches!(
            order_type,
            OrderType::StopMarket
                | OrderType::StopLimit
                | OrderType::MarketIfTouched
                | OrderType::LimitIfTouched
        ) && let Some(signal) = map_futures_trigger_signal(trigger_type)?
        {
            builder.trigger_signal(signal);
        }

        match order_type {
            OrderType::StopMarket => {
                if let Some(trigger) = trigger_price {
                    builder.stop_price(trigger.to_string());
                }
            }
            OrderType::StopLimit => {
                if let Some(trigger) = trigger_price {
                    builder.stop_price(trigger.to_string());
                }

                if let Some(limit) = price {
                    builder.limit_price(limit.to_string());
                }
            }
            OrderType::MarketIfTouched | OrderType::LimitIfTouched => {
                if let Some(trigger) = trigger_price {
                    builder.stop_price(trigger.to_string());
                }

                if let Some(limit) = price {
                    builder.limit_price(limit.to_string());
                }
            }
            _ => {
                if let Some(limit) = price {
                    builder.limit_price(limit.to_string());
                }
            }
        }

        if reduce_only {
            builder.reduce_only(true);
        }

        builder
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build order params: {e}"))
    }

    /// Submits a new order to the Kraken Futures exchange.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing.
    /// - The instrument is not found in cache.
    /// - The order type or time in force is not supported.
    /// - The request fails.
    /// - The order is rejected.
    #[expect(clippy::too_many_arguments)]
    pub async fn submit_order(
        &self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: TimeInForce,
        price: Option<Price>,
        trigger_price: Option<Price>,
        trigger_type: Option<TriggerType>,
        reduce_only: bool,
        post_only: bool,
    ) -> anyhow::Result<OrderStatusReport> {
        let instrument = self
            .get_cached_instrument(&instrument_id.symbol.inner())
            .ok_or_else(|| InstrumentLookupError::not_found(instrument_id))?;

        let params = self.build_send_order_params(
            instrument_id,
            client_order_id,
            order_side,
            order_type,
            quantity,
            time_in_force,
            price,
            trigger_price,
            trigger_type,
            reduce_only,
            post_only,
        )?;

        let response = self.inner.send_order_params(&params).await?;

        if response.result != KrakenApiResult::Success {
            return Err(KrakenSubmitOrderError::Rejected {
                reason: response
                    .error
                    .unwrap_or_else(|| "Unknown error".to_string()),
            }
            .into());
        }

        let send_status = response
            .send_status
            .ok_or(KrakenSubmitOrderError::MissingStatus)?;

        match send_status.status.as_str() {
            "placed" | "filled" => {}
            "postWouldExecute" => {
                let reason = send_status
                    .order_events
                    .as_ref()
                    .and_then(|events| events.first())
                    .and_then(|event| event.reason.clone())
                    .unwrap_or_else(|| "Post-only order would have crossed".to_string());
                return Err(KrakenSubmitOrderError::Rejected {
                    reason: format!("POST_ONLY_REJECTED: {reason}"),
                }
                .into());
            }
            status if is_futures_submit_rejection(status) => {
                return Err(KrakenSubmitOrderError::Rejected {
                    reason: status.to_string(),
                }
                .into());
            }
            status => {
                return Err(KrakenSubmitOrderError::UnknownStatus {
                    status: status.to_string(),
                }
                .into());
            }
        }

        let venue_order_id =
            send_status
                .order_id
                .clone()
                .ok_or_else(|| KrakenSubmitOrderError::MissingOrderId {
                    detail: format!("send status was {}", send_status.status),
                })?;

        let report: anyhow::Result<OrderStatusReport> = async {
            let ts_init = self.generate_ts_init();

            let open_orders_response = self.inner.get_open_orders().await?;
            if let Some(order) = open_orders_response
                .open_orders
                .iter()
                .find(|o| o.order_id == venue_order_id)
            {
                let mut report = parse_futures_order_status_report(
                    order,
                    &instrument,
                    account_id,
                    Some(quantity.as_decimal()),
                    ts_init,
                )?;

                // Kraken documents the `ioc` and `post` types in the send status's order events but
                // not in the open-order read, so the first event's order type states the time in
                // force and post-only.
                let send_order_type = send_status
                    .order_events
                    .as_ref()
                    .and_then(|events| events.first())
                    .and_then(|event| {
                        event
                            .order
                            .as_ref()
                            .map(|order| order.order_type)
                            .or_else(|| event.order_trigger.as_ref().map(|t| t.order_type))
                            .or_else(|| event.order_prior_execution.as_ref().map(|p| p.order_type))
                    });

                if let Some(order_type) = send_order_type {
                    let (time_in_force, post_only) =
                        parse_futures_order_type_time_in_force(order_type);
                    report.time_in_force = time_in_force;
                    report.post_only = post_only;
                }
                return Ok(report);
            }

            // Order not in open orders - may have filled immediately (market order or aggressive limit)
            // Try to use order_events from send_status first
            if let Some(order_events) = &send_status.order_events
                && let Some(send_event) = order_events.first()
            {
                // Handle regular orders, trigger orders, and execution events
                let event = if let Some(order_data) = &send_event.order {
                    let (time_in_force, post_only) =
                        parse_futures_order_type_time_in_force(order_data.order_type);
                    FuturesOrderEvent {
                        order_id: order_data.order_id.clone(),
                        cli_ord_id: order_data.cli_ord_id.clone(),
                        order_type: order_data.order_type,
                        symbol: order_data.symbol.clone(),
                        side: order_data.side,
                        quantity: order_data.quantity,
                        filled: order_data.filled,
                        limit_price: order_data.limit_price,
                        stop_price: order_data.stop_price,
                        timestamp: order_data.timestamp.clone(),
                        last_update_timestamp: order_data.last_update_timestamp.clone(),
                        reduce_only: order_data.reduce_only,
                        time_in_force,
                        post_only,
                    }
                } else if let Some(trigger_data) = &send_event.order_trigger {
                    let (time_in_force, post_only) =
                        parse_futures_order_type_time_in_force(trigger_data.order_type);
                    FuturesOrderEvent {
                        order_id: trigger_data.uid.clone(),
                        cli_ord_id: trigger_data.client_id.clone(),
                        order_type: trigger_data.order_type,
                        symbol: trigger_data.symbol.clone(),
                        side: trigger_data.side,
                        quantity: trigger_data.quantity,
                        filled: Decimal::ZERO,
                        limit_price: trigger_data.limit_price,
                        stop_price: Some(trigger_data.trigger_price),
                        timestamp: trigger_data.timestamp.clone(),
                        last_update_timestamp: trigger_data.last_update_timestamp.clone(),
                        reduce_only: trigger_data.reduce_only,
                        time_in_force,
                        post_only,
                    }
                } else if let Some(prior_exec) = &send_event.order_prior_execution {
                    // EXECUTION event - use orderPriorExecution data
                    let (time_in_force, post_only) =
                        parse_futures_order_type_time_in_force(prior_exec.order_type);
                    FuturesOrderEvent {
                        order_id: prior_exec.order_id.clone(),
                        cli_ord_id: prior_exec.cli_ord_id.clone(),
                        order_type: prior_exec.order_type,
                        symbol: prior_exec.symbol.clone(),
                        side: prior_exec.side,
                        quantity: prior_exec.quantity,
                        filled: send_event.amount.unwrap_or(prior_exec.quantity), // Use execution amount
                        limit_price: prior_exec.limit_price,
                        stop_price: prior_exec.stop_price,
                        timestamp: prior_exec.timestamp.clone(),
                        last_update_timestamp: prior_exec.last_update_timestamp.clone(),
                        reduce_only: prior_exec.reduce_only,
                        time_in_force,
                        post_only,
                    }
                } else {
                    anyhow::bail!("No order, orderTrigger, or orderPriorExecution data in event");
                };
                return parse_futures_order_event_status_report(
                    &event,
                    Some(send_event.event_type),
                    &instrument,
                    account_id,
                    ts_init,
                );
            }

            // Fall back to querying order events
            let events_response = self.inner.get_order_events(None, None, None).await?;
            let event_wrapper = events_response
                .order_events
                .iter()
                .find(|e| e.order.order_id == venue_order_id)
                .ok_or_else(|| {
                    anyhow::anyhow!("Order not found in open orders or events: {venue_order_id}")
                })?;

            if events_response
                .skipped_rows
                .iter()
                .any(|row| row.may_supersede(&venue_order_id, event_wrapper.state_ms))
            {
                anyhow::bail!(
                    "Order {venue_order_id}: a history row that could not be read may hold a newer state than the one found"
                );
            }

            parse_futures_order_event_status_report(
                &event_wrapper.order,
                Some(event_wrapper.event_type),
                &instrument,
                account_id,
                ts_init,
            )
        }
        .await;

        report.map_err(|source| KrakenSubmitOrderError::PostSubmitLookup { source }.into())
    }

    /// Modifies an existing order on the Kraken Futures exchange.
    ///
    /// Returns the new venue order ID assigned to the modified order.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Neither `client_order_id` nor `venue_order_id` is provided.
    /// - The instrument is not found in cache.
    /// - The request fails.
    /// - The edit fails on the exchange.
    /// - The response carries an invalid order ID.
    pub async fn modify_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
        quantity: Option<Quantity>,
        price: Option<Price>,
        trigger_price: Option<Price>,
    ) -> anyhow::Result<VenueOrderId> {
        let params = self.build_edit_order_params(
            instrument_id,
            client_order_id,
            venue_order_id,
            quantity,
            price,
            trigger_price,
        )?;
        let original_order_id = params.order_id.clone();

        let response = self.inner.edit_order(&params).await?;
        let status = response.edit_status.status.as_str();

        if response.result != KrakenApiResult::Success {
            return Err(KrakenModifyOrderError::Rejected {
                reason: status.to_string(),
            }
            .into());
        }

        match status {
            "edited" => {}
            status if is_futures_modify_rejection(status) => {
                return Err(KrakenModifyOrderError::Rejected {
                    reason: status.to_string(),
                }
                .into());
            }
            status => {
                return Err(KrakenModifyOrderError::UnknownStatus {
                    status: status.to_string(),
                }
                .into());
            }
        }

        // Return the new order_id from the response, or fall back to the original
        let new_venue_order_id = response
            .edit_status
            .order_id
            .or(response.edit_status.order_id_snake_case)
            .or(original_order_id)
            .ok_or(KrakenModifyOrderError::MissingOrderId)?;

        parse_venue_order_id(&new_venue_order_id, "futures edit response").map_err(|e| {
            KrakenModifyOrderError::InvalidOrderId {
                detail: format!("{e:#}"),
            }
            .into()
        })
    }

    /// Cancels an order on the Kraken Futures exchange.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing.
    /// - Neither client_order_id nor venue_order_id is provided.
    /// - The request fails.
    /// - The order cancellation is rejected.
    pub async fn cancel_order(
        &self,
        _account_id: AccountId,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> anyhow::Result<()> {
        let _ = self
            .get_cached_instrument(&instrument_id.symbol.inner())
            .ok_or_else(|| InstrumentLookupError::not_found(instrument_id))?;

        let (order_id, cli_ord_id) =
            order_identifier(client_order_id.as_ref(), venue_order_id.as_ref());

        if order_id.is_none() && cli_ord_id.is_none() {
            anyhow::bail!("Either client_order_id or venue_order_id must be provided");
        }

        let response = self.inner.cancel_order(order_id, cli_ord_id).await?;

        if response.result != KrakenApiResult::Success {
            let status = &response.cancel_status.status;
            anyhow::bail!("Order cancellation failed: {status}");
        }

        Ok(())
    }

    /// Cancels multiple orders on the Kraken Futures exchange.
    ///
    /// Automatically chunks requests into batches of 50 orders.
    ///
    /// # Parameters
    /// - `venue_order_ids` - List of venue order IDs to cancel.
    ///
    /// # Returns
    /// The total number of successfully cancelled orders.
    pub async fn cancel_orders_batch(
        &self,
        venue_order_ids: Vec<VenueOrderId>,
    ) -> anyhow::Result<usize> {
        if venue_order_ids.is_empty() {
            return Ok(0);
        }

        let mut total_cancelled = 0;

        for chunk in venue_order_ids.chunks(BATCH_CANCEL_LIMIT) {
            let order_ids: Vec<String> = chunk.iter().map(|id| id.to_string()).collect();
            let response = self.inner.cancel_orders_batch(order_ids).await?;

            if response.result != KrakenApiResult::Success {
                let error_msg = response.error.as_deref().unwrap_or("Unknown error");
                anyhow::bail!("Batch cancel failed: {error_msg}");
            }

            let success_count = response
                .batch_status
                .iter()
                .filter(|s| {
                    s.status == Some(KrakenSendStatus::Cancelled)
                        || s.cancel_status
                            .as_ref()
                            .is_some_and(|cs| cs.status == KrakenSendStatus::Cancelled)
                })
                .count();

            total_cancelled += success_count;
        }

        Ok(total_cancelled)
    }

    /// Submits multiple orders in a single batch request.
    ///
    /// Builds batch send items from order parameters, chunks at the batch limit,
    /// and returns per-item send statuses.
    ///
    /// # Errors
    ///
    /// Returns an error if the batch request fails at the API level.
    #[expect(clippy::type_complexity)]
    pub async fn submit_orders_batch(
        &self,
        orders: Vec<(
            InstrumentId,
            ClientOrderId,
            OrderSide,
            OrderType,
            Quantity,
            TimeInForce,
            Option<Price>,
            Option<Price>,
            Option<TriggerType>,
            bool,
            bool,
        )>,
    ) -> anyhow::Result<Vec<FuturesSendStatus>> {
        Ok(self
            .send_order_batches(orders)
            .await
            .into_iter()
            .map(|result| match result {
                Ok(item) if item.result == KrakenApiResult::Success => item.status,
                Ok(mut item) => {
                    item.status.status = format!("api_error: {}", item.status.status);
                    item.status
                }
                Err(e) => FuturesSendStatus {
                    order_id: None,
                    order_tag: None,
                    status: if matches!(
                        e.downcast_ref::<KrakenBatchOrderError>(),
                        Some(KrakenBatchOrderError::Validation { .. })
                    ) {
                        format!("validation_error: {e}")
                    } else {
                        format!("batch_error: {e}")
                    },
                    order_events: None,
                    cli_ord_id: None,
                    received_time: None,
                },
            })
            .collect())
    }

    pub(crate) async fn send_order_batches(
        &self,
        orders: Vec<FuturesBatchOrder>,
    ) -> Vec<anyhow::Result<FuturesBatchSubmitItem>> {
        let count = orders.len();
        if count == 0 {
            return Vec::new();
        }

        let mut results: Vec<Option<anyhow::Result<FuturesBatchSubmitItem>>> =
            (0..count).map(|_| None).collect();
        let mut valid_items = Vec::with_capacity(count);

        for (
            idx,
            (
                instrument_id,
                client_order_id,
                order_side,
                order_type,
                quantity,
                time_in_force,
                price,
                trigger_price,
                trigger_type,
                reduce_only,
                post_only,
            ),
        ) in orders.into_iter().enumerate()
        {
            match self.build_send_order_params(
                instrument_id,
                client_order_id,
                order_side,
                order_type,
                quantity,
                time_in_force,
                price,
                trigger_price,
                trigger_type,
                reduce_only,
                post_only,
            ) {
                Ok(params) => {
                    valid_items.push((
                        idx,
                        KrakenFuturesBatchSendItem::from_params(params, idx.to_string()),
                    ));
                }
                Err(e) => {
                    results[idx] = Some(Err(KrakenBatchOrderError::Validation {
                        reason: e.to_string(),
                    }
                    .into()));
                }
            }
        }

        if valid_items.is_empty() {
            return results.into_iter().flatten().collect();
        }

        let chunks: Vec<_> = valid_items.chunks(BATCH_ORDER_LIMIT).collect();
        for (chunk_index, chunk) in chunks.iter().enumerate() {
            let items = chunk.iter().map(|(_, item)| item.clone()).collect();
            match self.inner.submit_orders_batch(items).await {
                Ok(response) => {
                    let mut by_tag: HashMap<String, Option<FuturesSendStatus>> = HashMap::new();
                    let response_result = response.result;

                    for status in response.batch_status {
                        if let Some(tag) = status.order_tag.clone() {
                            by_tag
                                .entry(tag)
                                .and_modify(|entry| *entry = None)
                                .or_insert(Some(status));
                        }
                    }

                    for (idx, item) in *chunk {
                        let result = match by_tag.remove(&item.order_tag) {
                            Some(Some(status)) => Ok(FuturesBatchSubmitItem {
                                result: response_result,
                                status,
                            }),
                            Some(None) => Err(KrakenBatchOrderError::DuplicateResponse {
                                key: format!("order_tag {}", item.order_tag),
                            }
                            .into()),
                            None => Err(KrakenBatchOrderError::MissingResponse {
                                key: format!("order_tag {}", item.order_tag),
                            }
                            .into()),
                        };
                        results[*idx] = Some(result);
                    }
                }
                Err(e) => {
                    for (idx, _) in *chunk {
                        results[*idx] = Some(Err(anyhow::Error::new(e.clone())));
                    }

                    for later_chunk in &chunks[chunk_index + 1..] {
                        for (idx, _) in *later_chunk {
                            results[*idx] = Some(Err(KrakenBatchOrderError::NotAttempted.into()));
                        }
                    }
                    break;
                }
            }
        }

        results
            .into_iter()
            .map(|result| result.unwrap_or_else(|| Err(KrakenBatchOrderError::NotAttempted.into())))
            .collect()
    }

    /// Modifies multiple orders in a single batch request.
    #[expect(clippy::type_complexity)]
    pub async fn edit_orders_batch(
        &self,
        orders: Vec<(
            InstrumentId,
            Option<ClientOrderId>,
            Option<VenueOrderId>,
            Option<Quantity>,
            Option<Price>,
            Option<Price>,
        )>,
    ) -> anyhow::Result<Vec<String>> {
        let count = orders.len();
        if count == 0 {
            return Ok(Vec::new());
        }

        let mut all_statuses: Vec<Option<String>> = vec![None; count];
        let mut valid_items = Vec::with_capacity(count);
        let mut valid_indices = Vec::with_capacity(count);

        for (
            idx,
            (instrument_id, client_order_id, venue_order_id, quantity, price, trigger_price),
        ) in orders.into_iter().enumerate()
        {
            match self.build_edit_order_params(
                instrument_id,
                client_order_id,
                venue_order_id,
                quantity,
                price,
                trigger_price,
            ) {
                Ok(params) => {
                    valid_items.push(KrakenFuturesBatchEditItem::from_params(
                        params,
                        idx.to_string(),
                    ));
                    valid_indices.push(idx);
                }
                Err(e) => {
                    all_statuses[idx] = Some(format!("validation_error: {e}"));
                }
            }
        }

        if valid_items.is_empty() {
            return Ok(all_statuses.into_iter().flatten().collect());
        }

        let mut batch_statuses: Vec<String> = Vec::with_capacity(valid_items.len());

        for chunk in valid_items.chunks(BATCH_ORDER_LIMIT) {
            match self.inner.edit_orders_batch(chunk.to_vec()).await {
                Ok(response) => {
                    if response.result == KrakenApiResult::Success {
                        batch_statuses.extend(response.batch_status.into_iter().map(|s| s.status));
                    } else {
                        let error_msg = response
                            .batch_status
                            .first()
                            .map_or("Unknown error", |s| s.status.as_str());

                        for _ in 0..chunk.len() {
                            batch_statuses.push(format!("api_error: {error_msg}"));
                        }
                    }
                }
                Err(e) => {
                    let remaining = valid_items.len() - batch_statuses.len();
                    for _ in 0..remaining {
                        batch_statuses.push(format!("batch_error: {e}"));
                    }
                    break;
                }
            }
        }

        for (batch_idx, &original_idx) in valid_indices.iter().enumerate() {
            if let Some(status) = batch_statuses.get(batch_idx) {
                all_statuses[original_idx] = Some(status.clone());
            }
        }

        Ok(all_statuses.into_iter().flatten().collect())
    }

    fn build_edit_order_params(
        &self,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
        quantity: Option<Quantity>,
        price: Option<Price>,
        trigger_price: Option<Price>,
    ) -> anyhow::Result<KrakenFuturesEditOrderParams> {
        let _ = self
            .get_cached_instrument(&instrument_id.symbol.inner())
            .ok_or_else(|| InstrumentLookupError::not_found(instrument_id))?;

        let (order_id, cli_ord_id) =
            order_identifier(client_order_id.as_ref(), venue_order_id.as_ref());

        if order_id.is_none() && cli_ord_id.is_none() {
            anyhow::bail!("Either client_order_id or venue_order_id must be provided");
        }

        let mut builder = KrakenFuturesEditOrderParamsBuilder::default();

        if let Some(ref id) = order_id {
            builder.order_id(id.clone());
        }

        if let Some(ref id) = cli_ord_id {
            builder.cli_ord_id(id.clone());
        }

        // A Nautilus quantity is the order's total, fills included, while the venue reads a bare
        // size as the open size
        if let Some(qty) = quantity {
            builder.size(qty.to_string());
            builder.qty_mode(KrakenFuturesQtyMode::Absolute);
        }

        if let Some(p) = price {
            builder.limit_price(p.to_string());
        }

        if let Some(tp) = trigger_price {
            builder.stop_price(tp.to_string());
        }

        builder
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build edit order params: {e}"))
    }
}

/// The identifiers a cancel or edit names an order by, as `(order_id, cliOrdId)`: its venue order
/// ID when known, else its full client order ID. An order sent with a shortened client order ID is
/// held under that form, so its full ID beside the venue order ID could name no order.
pub(crate) fn order_identifier(
    client_order_id: Option<&ClientOrderId>,
    venue_order_id: Option<&VenueOrderId>,
) -> (Option<String>, Option<String>) {
    match venue_order_id {
        Some(venue_order_id) => (Some(venue_order_id.to_string()), None),
        None => (None, client_order_id.map(ToString::to_string)),
    }
}

pub(crate) fn is_futures_submit_rejection(status: &str) -> bool {
    matches!(
        status.parse::<KrakenSendStatus>(),
        Ok(KrakenSendStatus::InsufficientAvailableFunds
            | KrakenSendStatus::InvalidOrderType
            | KrakenSendStatus::InvalidSize
            | KrakenSendStatus::WouldCauseLiquidation
            | KrakenSendStatus::PostWouldExecute
            | KrakenSendStatus::IocWouldNotExecute
            | KrakenSendStatus::ReduceOnlyWouldIncreasePosition
            | KrakenSendStatus::ClientOrderIdAlreadyExist
            | KrakenSendStatus::ClientOrderIdTooBig
            | KrakenSendStatus::ClientOrderIdTooLong
            | KrakenSendStatus::ClientOrderIdInvalid)
    )
}

fn is_futures_modify_rejection(status: &str) -> bool {
    status.parse::<KrakenSendStatus>().is_ok_and(|status| {
        status == KrakenSendStatus::NotFound || is_futures_submit_rejection(status.as_ref())
    })
}

fn map_futures_trigger_signal(
    trigger_type: Option<TriggerType>,
) -> anyhow::Result<Option<KrakenTriggerSignal>> {
    match trigger_type {
        None => Ok(None),
        Some(TriggerType::Default | TriggerType::LastPrice) => Ok(Some(KrakenTriggerSignal::Last)),
        Some(TriggerType::MarkPrice) => Ok(Some(KrakenTriggerSignal::Mark)),
        Some(TriggerType::IndexPrice) => Ok(Some(KrakenTriggerSignal::Index)),
        Some(other) => anyhow::bail!(
            "Unsupported trigger type for Kraken Futures: {other:?} (only LastPrice, MarkPrice, and IndexPrice supported)"
        ),
    }
}

/// Accumulated `(total, locked)` balance amounts per standard code.
type AmountsByCode = AHashMap<Ustr, (Decimal, Decimal)>;

/// Combines the per-wallet balances and margins of a Kraken Futures accounts response.
///
/// The venue keys the same asset differently per wallet, `xbt` in the cash and single-collateral
/// wallets against `XBT` in the flex wallet, and every spelling maps to one standard code. The
/// accounts map is unordered, so balances accumulate per code and are emitted once in code order.
/// Pushing them into a flat vector instead would leave the surviving entry to iteration order,
/// since the account keys its balances by currency.
///
/// A wallet the venue lists at zero contributes a zero, so an asset drawn down to nothing is
/// reported at zero rather than left at its previous value: the engine only ever inserts
/// balances, and nothing downstream can clear one.
///
/// Margins are summed per currency the same way. A single-collateral wallet's requirement is in
/// its `currency` and a flex wallet's in USD, and `MarginAccount` keys account-wide margins by
/// currency, so two entries under one code would collapse by iteration order.
fn parse_account_entries(
    accounts: &AHashMap<String, FuturesAccount>,
) -> (Vec<AccountBalance>, Vec<MarginBalance>) {
    let mut balances: AmountsByCode = AHashMap::new();
    let mut margins: AmountsByCode = AHashMap::new();

    for account in accounts.values() {
        match account.account_type {
            KrakenFuturesAccountType::MultiCollateralMarginAccount => {
                parse_multi_collateral_balances(account, &mut balances);
                parse_multi_collateral_margins(account, &mut margins);
            }
            KrakenFuturesAccountType::MarginAccount => {
                let currency = margin_account_currency(account);

                if currency.is_none() {
                    // Labeling the requirement, or attributing the available funds, with a
                    // guessed currency would misstate them.
                    log::warn!(
                        "Single-collateral wallet currency unresolved (currency {:?}): its margin requirement is skipped and none of its assets is reported as locked; balances {:?}",
                        account.currency,
                        account.balances.keys().collect::<Vec<_>>()
                    );
                }

                parse_margin_account_balances(account, currency.as_deref(), &mut balances);
                parse_margin_account_margins(account, currency.as_deref(), &mut margins);
            }
            KrakenFuturesAccountType::CashAccount => {
                parse_cash_account_balances(account, &mut balances);
            }
            KrakenFuturesAccountType::Unknown => {
                log::debug!("Unknown account type: {:?}", account.account_type);
            }
        }
    }

    (emit_balances(&balances), emit_margins(&margins))
}

/// Resolves the currency a futures balance or margin requirement is emitted in.
///
/// Every balance and margin keeps an eight-decimal currency, so wallet amounts are never rounded;
/// resolving USD to the registered two-decimal currency would round cash and single-collateral
/// USD balances, and the flex USD requirement, to cents.
fn futures_balance_currency(code: &str) -> anyhow::Result<Currency> {
    Currency::new_checked(code, 8, 0, code, CurrencyType::Crypto)
        .map_err(|e| anyhow::anyhow!("Invalid currency code {code:?}: {e}"))
}

/// Returns the codes of `amounts` in a stable order.
fn sorted_codes(amounts: &AmountsByCode) -> Vec<Ustr> {
    let mut codes: Vec<Ustr> = amounts.keys().copied().collect();
    codes.sort_unstable();
    codes
}

fn emit_balances(amounts: &AmountsByCode) -> Vec<AccountBalance> {
    let mut balances = Vec::with_capacity(amounts.len());

    for code in sorted_codes(amounts) {
        let (total, locked) = amounts[&code];
        match futures_balance_currency(code.as_str())
            .and_then(|currency| aggregate_balance(total, locked, currency))
        {
            Ok(balance) => balances.push(balance),
            Err(e) => log::warn!("Skipping {code} balance: {e}"),
        }
    }

    balances
}

/// Builds the balance for amounts already bounded per wallet and summed.
///
/// `AccountBalance::from_total_and_locked` would clamp the aggregate's locked amount into
/// `[0, total]` a second time, which hides a shortfall: a margin wallet at -1 with -2 available
/// reserves 1, and combined with 1.5 in cash the sums are 0.5 total and 1 locked, so free is -0.5.
/// Clamping would report 0.5 locked and 0 free. The sums are therefore emitted as they are, with
/// free derived in fixed point so `total == locked + free` holds exactly.
fn aggregate_balance(
    total: Decimal,
    locked: Decimal,
    currency: Currency,
) -> anyhow::Result<AccountBalance> {
    let total = Money::from_decimal(total, currency)?;
    let locked = Money::from_decimal(locked, currency)?;
    let free = total
        .checked_sub(locked)
        .ok_or_else(|| anyhow::anyhow!("Derived `free` for {currency} exceeds Money bounds"))?;

    AccountBalance::new_checked(total, locked, free).map_err(Into::into)
}

/// Adds one wallet's `total` and `locked` to the entry for `code`.
///
/// The wallet's `locked` is bounded first, exactly as `AccountBalance::from_total_and_locked`
/// bounds a single balance: clamped into `[0, total]` for a non-negative total, and passed
/// through unchanged for a negative one. Summing the raw figure instead would let one wallet
/// cancel another's reservation. A flex wallet holding 1 BTC with 1.6 BTC-equivalent available
/// margin reports a raw locked of -0.6, which would erase the 0.6 a single-collateral wallet
/// holding 1 BTC with 0.4 available has genuinely reserved.
fn accumulate_balance(amounts: &mut AmountsByCode, code: &str, total: Decimal, locked: Decimal) {
    let locked = if total.is_sign_negative() {
        locked
    } else {
        locked.clamp(Decimal::ZERO, total)
    };

    let entry = amounts
        .entry(Ustr::from(code))
        .or_insert((Decimal::ZERO, Decimal::ZERO));
    entry.0 += total;
    entry.1 += locked;
}

fn parse_multi_collateral_balances(account: &FuturesAccount, balances: &mut AmountsByCode) {
    // `portfolioValue` is the USD valuation of the whole flex collateral set, so adding the USD
    // collateral entry on top of it would count the same funds twice.
    let emits_portfolio_value = account
        .portfolio_value
        .is_some_and(|value| value > Decimal::ZERO);

    for (currency_code, currency_info) in &account.currencies {
        // The venue keys these by its own spelling and casing, so map to the standard code.
        let code = normalize_asset_key(currency_code.as_str());

        if emits_portfolio_value && code == "USD" {
            continue;
        }

        let total_amount = currency_info.quantity;
        let available_amount = currency_info.available.unwrap_or(total_amount);
        let locked_amount = total_amount - available_amount;

        accumulate_balance(balances, &code, total_amount, locked_amount);
    }

    // Multi-collateral accounts track margin in USD even though the
    // actual collateral is held in various crypto currencies.
    if let Some(portfolio_value) = account.portfolio_value
        && portfolio_value > Decimal::ZERO
    {
        let available_usd = account.available_margin.unwrap_or(portfolio_value);
        let locked_usd = portfolio_value - available_usd;

        accumulate_balance(balances, "USD", portfolio_value, locked_usd);
    }
}

fn parse_multi_collateral_margins(account: &FuturesAccount, margins: &mut AmountsByCode) {
    // The flex wallet reports its requirement in USD in `initialMargin` and `maintenanceMargin`;
    // its schema carries no `marginRequirements`.
    let initial_margin = account.initial_margin.unwrap_or(Decimal::ZERO);
    let maintenance = account.maintenance_margin.unwrap_or(Decimal::ZERO);

    // The same gate as the single-collateral wallet: a requirement counts when either figure is
    // positive.
    if initial_margin > Decimal::ZERO || maintenance > Decimal::ZERO {
        accumulate_margin(margins, "USD", initial_margin, maintenance);
    }
}

/// `wallet_currency` is the wallet's resolved `currency`, or `None` when it cannot be resolved.
fn parse_margin_account_balances(
    account: &FuturesAccount,
    wallet_currency: Option<&str>,
    balances: &mut AmountsByCode,
) {
    // `auxiliary.af` is the wallet's available funds in its `currency`, so it bounds that asset
    // alone; another asset the wallet holds has nothing reserved against it. Locked is derived so
    // that free equals the venue's available funds, which already net unrealized PnL. Without a
    // resolved currency the figure cannot be attributed, and no asset is reported as locked.
    let available = account.auxiliary.as_ref().and_then(|aux| aux.af);

    for (currency_code, &amount) in &account.balances {
        // The venue keys this map by contract symbol as well as by currency, for example
        // `FI_XBTUSD_171215`, and those entries are positions rather than balances. No Kraken
        // asset code contains an underscore.
        if currency_code.contains('_') {
            continue;
        }

        // The venue keys these by its own spelling and casing, so map to the standard code.
        let code = normalize_asset_key(currency_code.as_str());

        let locked = match available {
            Some(af) if wallet_currency == Some(code.as_str()) => amount - af,
            _ => Decimal::ZERO,
        };

        accumulate_balance(balances, &code, amount, locked);
    }
}

/// Returns the currency a single-collateral wallet's figures are denominated in.
///
/// The wallet schema requires `currency` and states that `auxiliary` and `marginRequirements` are
/// in it. Guessing it from the asset keys in `balances` could label a requirement with dust held in
/// another asset, so a wallet without a usable field resolves to `None`.
fn margin_account_currency(account: &FuturesAccount) -> Option<String> {
    account
        .currency
        .as_deref()
        .map(str::trim)
        .filter(|code| !code.is_empty())
        .map(normalize_asset_key)
}

/// `wallet_currency` is the wallet's resolved `currency`; a wallet without one contributes no
/// margin entry, since labeling the requirement with a guess would misstate it.
fn parse_margin_account_margins(
    account: &FuturesAccount,
    wallet_currency: Option<&str>,
    margins: &mut AmountsByCode,
) {
    if let (Some(mr), Some(code)) = (account.margin_requirements.as_ref(), wallet_currency) {
        let im = mr.im.unwrap_or(Decimal::ZERO);
        let mm = mr.mm.unwrap_or(Decimal::ZERO);

        if im > Decimal::ZERO || mm > Decimal::ZERO {
            accumulate_margin(margins, code, im, mm);
        }
    }
}

/// Adds one wallet's requirement to the entry for `code`.
fn accumulate_margin(
    margins: &mut AmountsByCode,
    code: &str,
    initial: Decimal,
    maintenance: Decimal,
) {
    let entry = margins
        .entry(Ustr::from(code))
        .or_insert((Decimal::ZERO, Decimal::ZERO));
    entry.0 += initial;
    entry.1 += maintenance;
}

fn emit_margins(amounts: &AmountsByCode) -> Vec<MarginBalance> {
    let mut margins = Vec::with_capacity(amounts.len());

    for code in sorted_codes(amounts) {
        let (initial, maintenance) = amounts[&code];
        let margin = futures_balance_currency(code.as_str()).and_then(|currency| {
            let initial = Money::from_decimal(initial, currency)?;
            let maintenance = Money::from_decimal(maintenance, currency)?;
            Ok(MarginBalance::new(initial, maintenance, None))
        });

        match margin {
            Ok(margin) => margins.push(margin),
            Err(e) => log::warn!("Skipping {code} margin: {e}"),
        }
    }

    margins
}

fn parse_cash_account_balances(account: &FuturesAccount, balances: &mut AmountsByCode) {
    for (currency_code, &amount) in &account.balances {
        // The venue keys these by its own spelling and casing, so map to the standard code.
        let code = normalize_asset_key(currency_code.as_str());

        accumulate_balance(balances, &code, amount, Decimal::ZERO);
    }
}

/// Records a history row in scope that the read could not use, keeping the newest such row per
/// order so the fold can withhold an order it would otherwise report from an older state.
fn record_unread_row(
    coverage: &mut ReportCoverage,
    unread_ms: &mut AHashMap<VenueOrderId, i64>,
    order_id: &str,
    event_ms: i64,
) {
    coverage.skip(order_id);

    let newest = unread_ms
        .entry(VenueOrderId::new(order_id))
        .or_insert(event_ms);
    *newest = (*newest).max(event_ms);
}

/// Whether `candidate` describes a later state of the same order than `existing`.
///
/// A closed order does not reopen, so a closed state beats an open one whatever their stamps: an
/// open state stamped later, such as a refused edit logged after a cancel, describes the order as
/// it stood before it closed. Between two states of the same kind the later `ts_last` wins, and on
/// a tie, which the venue's millisecond stamps allow, the larger filled quantity.
pub(crate) fn supersedes(candidate: &OrderStatusReport, existing: &OrderStatusReport) -> bool {
    match (
        candidate.order_status.is_closed(),
        existing.order_status.is_closed(),
    ) {
        (true, false) => true,
        (false, true) => false,
        _ if candidate.ts_last != existing.ts_last => candidate.ts_last > existing.ts_last,
        _ => candidate.filled_qty > existing.filled_qty,
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use ahash::AHashMap;
    use nautilus_model::{enums::OrderStatus, instruments::CryptoPerpetual};
    use nautilus_testkit::http::assert_http_redirect_rejected;
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    #[tokio::test]
    async fn test_authenticated_client_rejects_redirects() {
        let client = KrakenFuturesRawHttpClient::with_credentials(
            "key".into(),
            "secret".into(),
            KrakenEnvironment::Live,
            None,
            3,
            Some(0),
            None,
            None,
            None,
            10,
        )
        .unwrap()
        .client;
        assert_http_redirect_rejected(|url| async move {
            client
                .get(url, None, None, Some(3), None)
                .await
                .unwrap()
                .status
                .as_u16()
        })
        .await;
    }

    #[rstest]
    fn test_raw_client_creation() {
        let client = KrakenFuturesRawHttpClient::default();
        assert!(client.credential.is_none());
        assert!(client.base_url().contains("futures"));
    }

    #[rstest]
    fn test_raw_client_with_credentials() {
        let client = KrakenFuturesRawHttpClient::with_credentials(
            "test_key".to_string(),
            "test_secret".to_string(),
            KrakenEnvironment::Live,
            None,
            60,
            None,
            None,
            None,
            None,
            KRAKEN_FUTURES_DEFAULT_RATE_LIMIT_PER_SECOND,
        )
        .unwrap();
        assert!(client.credential.is_some());
    }

    #[rstest]
    #[tokio::test]
    async fn test_order_request_cancellation_before_transport() {
        let client = Arc::new(KrakenFuturesRawHttpClient::default());
        let guard = client.auth_mutex.lock().await;
        let waiting_client = Arc::clone(&client);
        let waiting = tokio::spawn(async move {
            waiting_client
                .send_authenticated_post::<serde_json::Value>(
                    "/derivatives/api/v3/sendorder",
                    "orderType=lmt".to_string(),
                )
                .await
        });

        tokio::task::yield_now().await;
        client.cancel_all_requests();
        let waiting_result = tokio::time::timeout(Duration::from_secs(1), waiting)
            .await
            .expect("auth lock wait should stop on cancellation")
            .expect("auth request task should complete");
        drop(guard);
        client.reset_cancellation_token();
        let reset_token = client.cancellation_token();

        assert!(matches!(
            waiting_result,
            Err(KrakenHttpError::RequestNotStarted(ref message))
                if message == "Request cancelled"
        ));
        assert!(!reset_token.is_cancelled());
    }

    #[rstest]
    fn test_client_creation() {
        let client = KrakenFuturesHttpClient::default();
        assert!(client.instruments_cache.is_empty());
    }

    #[rstest]
    fn test_client_with_credentials() {
        let client = KrakenFuturesHttpClient::with_credentials(
            "test_key".to_string(),
            "test_secret".to_string(),
            KrakenEnvironment::Live,
            None,
            60,
            None,
            None,
            None,
            None,
            KRAKEN_FUTURES_DEFAULT_RATE_LIMIT_PER_SECOND,
        )
        .unwrap();
        assert!(client.instruments_cache.is_empty());
    }

    fn entries_for(
        accounts: &[(&str, FuturesAccount)],
    ) -> (Vec<AccountBalance>, Vec<MarginBalance>) {
        let map: AHashMap<String, FuturesAccount> = accounts
            .iter()
            .map(|(name, account)| ((*name).to_string(), account.clone()))
            .collect();

        parse_account_entries(&map)
    }

    fn flex_wallet(
        currencies: &[(&str, Decimal)],
        portfolio_value: Option<Decimal>,
    ) -> FuturesAccount {
        FuturesAccount {
            account_type: KrakenFuturesAccountType::MultiCollateralMarginAccount,
            currency: None,
            balances: AHashMap::new(),
            currencies: currencies
                .iter()
                .map(|(code, quantity)| {
                    (
                        (*code).to_string(),
                        FuturesFlexCurrency {
                            quantity: *quantity,
                            value: None,
                            collateral: None,
                            available: Some(*quantity),
                        },
                    )
                })
                .collect(),
            auxiliary: None,
            margin_requirements: None,
            portfolio_value,
            available_margin: portfolio_value,
            initial_margin: None,
            maintenance_margin: None,
            pnl: None,
        }
    }

    fn cash_wallet(balances: &[(&str, Decimal)]) -> FuturesAccount {
        FuturesAccount {
            account_type: KrakenFuturesAccountType::CashAccount,
            currency: None,
            balances: balances
                .iter()
                .map(|(code, amount)| ((*code).to_string(), *amount))
                .collect(),
            currencies: AHashMap::new(),
            auxiliary: None,
            margin_requirements: None,
            portfolio_value: None,
            available_margin: None,
            initial_margin: None,
            maintenance_margin: None,
            pnl: None,
        }
    }

    /// One asset held in two wallets must produce exactly one balance, holding both amounts.
    ///
    /// The cash wallet keys it `xbt` and the flex wallet `XBT`; both map to `BTC`. Emitting two
    /// entries would leave the survivor to hash-map order, since the account keys balances by
    /// currency.
    #[rstest]
    fn test_parse_account_entries_combines_one_asset_across_wallets() {
        let (balances, _) = entries_for(&[
            ("cash", cash_wallet(&[("xbt", dec!(1.5))])),
            ("flex", flex_wallet(&[("XBT", dec!(0.25))], None)),
        ]);

        let btc: Vec<_> = balances
            .iter()
            .filter(|b| b.currency.code.as_str() == "BTC")
            .collect();
        assert_eq!(btc.len(), 1, "expected one BTC balance: {balances:?}");
        assert_eq!(btc[0].total.as_decimal(), dec!(1.75));
    }

    /// Entry order must not depend on hash-map iteration order.
    #[rstest]
    fn test_parse_account_entries_emits_balances_in_code_order() {
        let (balances, _) = entries_for(&[(
            "cash",
            cash_wallet(&[("xrp", dec!(10)), ("xbt", dec!(1)), ("eth", dec!(2))]),
        )]);

        let codes: Vec<&str> = balances.iter().map(|b| b.currency.code.as_str()).collect();
        assert_eq!(codes, vec!["BTC", "ETH", "XRP"]);
    }

    /// `portfolioValue` already values the flex collateral set, so its USD entry wins alone.
    #[rstest]
    fn test_parse_account_entries_does_not_double_count_flex_usd() {
        let (balances, _) = entries_for(&[(
            "flex",
            flex_wallet(&[("USD", dec!(5000))], Some(dec!(34995.52))),
        )]);

        let usd: Vec<_> = balances
            .iter()
            .filter(|b| b.currency.code.as_str() == "USD")
            .collect();
        assert_eq!(usd.len(), 1, "expected one USD balance: {balances:?}");
        assert_eq!(usd[0].total.as_decimal(), dec!(34995.52));
    }

    /// Each wallet's `locked` is bounded before the sum, so one wallet cannot cancel another's.
    ///
    /// A single-collateral wallet with 1 BTC and 0.4 available has reserved 0.6. A flex wallet
    /// with 1 BTC and 1.6 BTC-equivalent available margin reports a raw locked of -0.6, which
    /// summed unbounded would erase that reservation.
    #[rstest]
    fn test_parse_account_entries_bounds_locked_per_wallet() {
        let single = FuturesAccount {
            account_type: KrakenFuturesAccountType::MarginAccount,
            currency: Some("xbt".to_string()),
            balances: [("xbt".to_string(), dec!(1))].into_iter().collect(),
            currencies: AHashMap::new(),
            auxiliary: Some(FuturesAuxiliary {
                usd: None,
                pv: None,
                pnl: None,
                af: Some(dec!(0.4)),
                funding: None,
            }),
            margin_requirements: None,
            portfolio_value: None,
            available_margin: None,
            initial_margin: None,
            maintenance_margin: None,
            pnl: None,
        };

        let mut flex = flex_wallet(&[], None);
        flex.currencies.insert(
            "XBT".to_string(),
            FuturesFlexCurrency {
                quantity: dec!(1),
                value: None,
                collateral: None,
                available: Some(dec!(1.6)),
            },
        );

        let (balances, _) = entries_for(&[("fi_xbtusd", single), ("flex", flex)]);

        let btc = balances
            .iter()
            .find(|b| b.currency.code.as_str() == "BTC")
            .expect("one BTC balance");
        assert_eq!(btc.total.as_decimal(), dec!(2));
        assert_eq!(
            btc.locked.as_decimal(),
            dec!(0.6),
            "the flex wallet's negative raw locked must not cancel the reservation"
        );
        assert_eq!(btc.free.as_decimal(), dec!(1.4));
    }

    /// Summed wallet-local amounts are emitted as summed, so a shortfall stays visible.
    ///
    /// A margin wallet at -1 BTC with -2 BTC available reserves 1 BTC. Combined with 1.5 BTC in
    /// cash the sums are 0.5 total and 1 locked, leaving free at -0.5. Clamping the aggregate
    /// would report 0.5 locked and 0 free, dropping half the reservation.
    #[rstest]
    fn test_parse_account_entries_preserves_summed_amounts_across_mixed_sign_wallets() {
        let margin = FuturesAccount {
            account_type: KrakenFuturesAccountType::MarginAccount,
            currency: Some("xbt".to_string()),
            balances: [("xbt".to_string(), dec!(-1))].into_iter().collect(),
            currencies: AHashMap::new(),
            auxiliary: Some(FuturesAuxiliary {
                usd: None,
                pv: None,
                pnl: None,
                af: Some(dec!(-2)),
                funding: None,
            }),
            margin_requirements: None,
            portfolio_value: None,
            available_margin: None,
            initial_margin: None,
            maintenance_margin: None,
            pnl: None,
        };

        let (balances, _) = entries_for(&[
            ("fi_xbtusd", margin),
            ("cash", cash_wallet(&[("xbt", dec!(1.5))])),
        ]);

        let btc = balances
            .iter()
            .find(|b| b.currency.code.as_str() == "BTC")
            .expect("one BTC balance");
        assert_eq!(btc.total.as_decimal(), dec!(0.5));
        assert_eq!(
            btc.locked.as_decimal(),
            dec!(1),
            "the margin wallet's reservation must survive the aggregate"
        );
        assert_eq!(
            btc.free.as_decimal(),
            dec!(-0.5),
            "the shortfall must stay visible rather than clamp to zero"
        );
    }

    /// A negative total keeps its reported `locked`, as a single balance always has.
    #[rstest]
    fn test_parse_account_entries_passes_a_negative_total_through() {
        let account = FuturesAccount {
            account_type: KrakenFuturesAccountType::MarginAccount,
            currency: Some("xbt".to_string()),
            balances: [("xbt".to_string(), dec!(-1))].into_iter().collect(),
            currencies: AHashMap::new(),
            auxiliary: Some(FuturesAuxiliary {
                usd: None,
                pv: None,
                pnl: None,
                af: Some(dec!(-0.4)),
                funding: None,
            }),
            margin_requirements: None,
            portfolio_value: None,
            available_margin: None,
            initial_margin: None,
            maintenance_margin: None,
            pnl: None,
        };

        let (balances, _) = entries_for(&[("fi_xbtusd", account)]);

        assert_eq!(balances.len(), 1);
        assert_eq!(balances[0].total.as_decimal(), dec!(-1));
        assert_eq!(balances[0].locked.as_decimal(), dec!(-0.6));
        assert_eq!(balances[0].free.as_decimal(), dec!(-0.4));
    }

    /// A USD wallet balance keeps eight decimals rather than rounding to cents.
    #[rstest]
    fn test_parse_account_entries_keeps_usd_balance_at_eight_decimals() {
        let (balances, _) = entries_for(&[("cash", cash_wallet(&[("usd", dec!(1234.56789012))]))]);

        let usd = balances
            .iter()
            .find(|b| b.currency.code.as_str() == "USD")
            .expect("one USD balance");
        assert_eq!(usd.currency.precision, 8);
        assert_eq!(usd.total.as_decimal(), dec!(1234.56789012));
    }

    /// Margins are keyed by currency: a single-collateral wallet's in its `currency`, the flex
    /// wallet's in USD, so requirements in different currencies stay apart.
    #[rstest]
    fn test_parse_account_entries_keeps_margins_per_currency() {
        let mut flex = flex_wallet(&[], Some(dec!(10000)));
        flex.initial_margin = Some(dec!(500));
        flex.maintenance_margin = Some(dec!(250));

        let single = FuturesAccount {
            account_type: KrakenFuturesAccountType::MarginAccount,
            currency: Some("xbt".to_string()),
            balances: [("xbt".to_string(), dec!(2))].into_iter().collect(),
            currencies: AHashMap::new(),
            auxiliary: None,
            margin_requirements: Some(FuturesMarginRequirements {
                im: Some(dec!(0.1)),
                mm: Some(dec!(0.05)),
                lt: None,
                tt: None,
            }),
            portfolio_value: None,
            available_margin: None,
            initial_margin: None,
            maintenance_margin: None,
            pnl: None,
        };

        let (_, margins) = entries_for(&[("flex", flex), ("fi_xbtusd", single)]);

        assert_eq!(
            margins.len(),
            2,
            "one margin entry per currency: {margins:?}"
        );
        let mut entries: Vec<(String, Decimal)> = margins
            .iter()
            .map(|m| (m.currency.code.to_string(), m.initial.as_decimal()))
            .collect();
        entries.sort();
        assert_eq!(
            entries,
            vec![
                ("BTC".to_string(), dec!(0.1)),
                ("USD".to_string(), dec!(500))
            ],
            "each currency keeps its own entry"
        );
    }

    /// The wallet's available funds bound its own currency alone; another asset it holds has
    /// nothing reserved against it.
    #[rstest]
    fn test_parse_margin_account_balances_bound_locked_to_the_wallet_currency() {
        let wallet = FuturesAccount {
            account_type: KrakenFuturesAccountType::MarginAccount,
            currency: Some("xbt".to_string()),
            balances: [("xbt".to_string(), dec!(0.3)), ("xrp".to_string(), dec!(5))]
                .into_iter()
                .collect(),
            currencies: AHashMap::new(),
            auxiliary: Some(FuturesAuxiliary {
                usd: None,
                pv: None,
                pnl: None,
                af: Some(dec!(0.1)),
                funding: None,
            }),
            margin_requirements: None,
            portfolio_value: None,
            available_margin: None,
            initial_margin: None,
            maintenance_margin: None,
            pnl: None,
        };

        let (balances, _) = entries_for(&[("fi_xbtusd", wallet)]);

        let by_code = |code: &str| {
            balances
                .iter()
                .find(|b| b.currency.code.as_str() == code)
                .unwrap_or_else(|| panic!("{code} balance"))
        };
        assert_eq!(by_code("BTC").locked.as_decimal(), dec!(0.2));
        assert_eq!(by_code("XRP").locked.as_decimal(), dec!(0));
        assert_eq!(by_code("XRP").free.as_decimal(), dec!(5));
    }

    /// Contract-symbol keys in `balances` are positions, not balances, and are skipped.
    #[rstest]
    fn test_parse_margin_account_balances_skip_contract_symbol_keys() {
        let wallet = FuturesAccount {
            account_type: KrakenFuturesAccountType::MarginAccount,
            currency: Some("xbt".to_string()),
            balances: [
                ("xbt".to_string(), dec!(0.5)),
                ("FI_XBTUSD_171215".to_string(), dec!(50000)),
                ("FI_XBTUSD_180615".to_string(), dec!(-15000)),
            ]
            .into_iter()
            .collect(),
            currencies: AHashMap::new(),
            auxiliary: None,
            margin_requirements: None,
            portfolio_value: None,
            available_margin: None,
            initial_margin: None,
            maintenance_margin: None,
            pnl: None,
        };

        let (balances, _) = entries_for(&[("fi_xbtusd", wallet)]);

        assert_eq!(balances.len(), 1, "{balances:?}");
        assert_eq!(balances[0].currency.code.as_str(), "BTC");
    }

    /// A flex wallet reports its requirement in USD in the top-level `initialMargin` and
    /// `maintenanceMargin` fields, and both figures reach the account-wide entry.
    ///
    /// Kraken's documented `flex` wallet shape, with the two requirement figures set.
    #[rstest]
    fn test_parse_multi_collateral_margins_read_the_documented_fields() {
        let json = r#"{
            "type": "multiCollateralMarginAccount",
            "currencies": {
                "XBT": {"quantity": 0.1185308247, "value": 4998.721054420551, "collateral": 4886.49976674881, "available": 0.1185308247},
                "USD": {"quantity": 5000, "value": 5000, "collateral": 5000, "available": 5000}
            },
            "balanceValue": 9998.72,
            "portfolioValue": 9998.72,
            "collateralValue": 9886.5,
            "initialMargin": 500,
            "initialMarginWithOrders": 500,
            "maintenanceMargin": 250,
            "pnl": 0,
            "unrealizedFunding": 0,
            "totalUnrealized": 0,
            "totalUnrealizedAsMargin": 0,
            "marginEquity": 9886.5,
            "availableMargin": 9386.5
        }"#;
        let account: FuturesAccount = serde_json::from_str(json).unwrap();

        let (_, margins) = entries_for(&[("flex", account)]);

        assert_eq!(margins.len(), 1, "{margins:?}");
        assert_eq!(margins[0].currency.code.as_str(), "USD");
        assert_eq!(margins[0].initial.as_decimal(), dec!(500));
        assert_eq!(margins[0].maintenance.as_decimal(), dec!(250));
        assert_eq!(margins[0].instrument_id, None);
    }

    /// A flex wallet with no requirement contributes no entry; one with a requirement contributes
    /// an account-wide USD entry, so the maintenance figure alone is enough as on the
    /// single-collateral wallet.
    #[rstest]
    #[case::none(None, None, 0)]
    #[case::initial_only(Some(dec!(100)), None, 1)]
    #[case::maintenance_only(None, Some(dec!(40)), 1)]
    fn test_parse_multi_collateral_margins_follow_the_requirement_gate(
        #[case] initial_margin: Option<Decimal>,
        #[case] maintenance: Option<Decimal>,
        #[case] expected: usize,
    ) {
        let mut flex = flex_wallet(&[("USD", dec!(1000))], Some(dec!(1000)));
        flex.initial_margin = initial_margin;
        flex.maintenance_margin = maintenance;

        let (_, margins) = entries_for(&[("flex", flex)]);

        assert_eq!(margins.len(), expected, "{margins:?}");
        for margin in &margins {
            assert_eq!(margin.currency.code.as_str(), "USD");
            assert_eq!(
                margin.instrument_id, None,
                "the requirement is account-wide"
            );
        }
    }

    /// A single-collateral wallet's requirement is denominated in the wallet's `currency`.
    ///
    /// Kraken's documented `fi_xbtusd` wallet: `currency: xbt`, a funded `xbt` key, an `xrp: 0` key
    /// and two contract-symbol keys. The requirement is reported in BTC at eight decimals.
    #[rstest]
    fn test_parse_margin_account_margins_use_the_wallet_currency() {
        let json = r#"{
            "auxiliary": {"af": 100.73891563, "funding": 100.73891563, "pnl": 12.42134766, "pv": 153.73891563, "usd": 0},
            "balances": {"FI_XBTUSD_171215": "50000", "FI_XBTUSD_180615": "-15000", "xbt": "141.31756797", "xrp": "0"},
            "currency": "xbt",
            "marginRequirements": {"im": 52.8, "lt": 39.6, "mm": 23.76, "tt": 15.84},
            "triggerEstimates": {"im": 3110, "lt": 2890, "mm": 3000, "tt": 2830},
            "type": "marginAccount"
        }"#;
        let account: FuturesAccount = serde_json::from_str(json).unwrap();

        let (balances, margins) = entries_for(&[("fi_xbtusd", account)]);

        assert_eq!(margins.len(), 1, "{margins:?}");
        assert_eq!(margins[0].currency.code.as_str(), "BTC");
        assert_eq!(margins[0].currency.precision, 8);
        assert_eq!(margins[0].initial.as_decimal(), dec!(52.8));
        assert_eq!(margins[0].maintenance.as_decimal(), dec!(23.76));
        let btc = balances
            .iter()
            .find(|b| b.currency.code.as_str() == "BTC")
            .expect("BTC balance");
        assert_eq!(btc.total.as_decimal(), dec!(141.31756797));
    }

    /// A requirement whose currency cannot be resolved is skipped rather than labeled by a guess.
    ///
    /// Without the `currency` field, or with an empty one, the funded `xbt` key beside dust in
    /// `xrp` is not evidence of the denomination, and an empty code must not reach the currency
    /// constructor.
    #[rstest]
    #[case::absent(None)]
    #[case::empty(Some(""))]
    #[case::blank(Some("  "))]
    fn test_parse_margin_account_margins_skip_a_wallet_whose_currency_is_unresolved(
        #[case] currency: Option<&str>,
    ) {
        let mut wallet = cash_wallet(&[("xbt", dec!(1.5)), ("xrp", Decimal::ZERO)]);
        wallet.account_type = KrakenFuturesAccountType::MarginAccount;
        wallet.currency = currency.map(str::to_string);
        wallet.margin_requirements = Some(FuturesMarginRequirements {
            im: Some(dec!(0.05)),
            mm: Some(dec!(0.025)),
            lt: None,
            tt: None,
        });

        let (_, margins) = entries_for(&[("fi_xbtusd", wallet)]);

        assert!(margins.is_empty(), "no guessed denomination: {margins:?}");
    }

    /// Wallets sharing a currency sum into one margin entry.
    ///
    /// `MarginAccount` keys account-wide margins by currency, so two entries under one code would
    /// survive by iteration order instead.
    #[rstest]
    fn test_parse_account_entries_sums_margins_of_wallets_sharing_a_currency() {
        let requirement = |im: Decimal, mm: Decimal| {
            Some(FuturesMarginRequirements {
                im: Some(im),
                mm: Some(mm),
                lt: None,
                tt: None,
            })
        };
        let mut first = cash_wallet(&[("xbt", dec!(2))]);
        first.account_type = KrakenFuturesAccountType::MarginAccount;
        first.currency = Some("xbt".to_string());
        first.margin_requirements = requirement(dec!(0.1), dec!(0.05));
        let mut second = cash_wallet(&[("xbt", dec!(1))]);
        second.account_type = KrakenFuturesAccountType::MarginAccount;
        second.currency = Some("XBT".to_string());
        second.margin_requirements = requirement(dec!(0.02), dec!(0.01));

        let (_, margins) = entries_for(&[("fi_a", first), ("fi_b", second)]);

        assert_eq!(margins.len(), 1, "one entry per currency: {margins:?}");
        assert_eq!(margins[0].currency.code.as_str(), "BTC");
        assert_eq!(margins[0].initial.as_decimal(), dec!(0.12));
        assert_eq!(margins[0].maintenance.as_decimal(), dec!(0.06));
    }

    /// An asset key the currency constructor rejects is skipped, not a panic.
    #[rstest]
    fn test_parse_account_entries_skips_an_empty_asset_key() {
        let (balances, _) =
            entries_for(&[("cash", cash_wallet(&[("", dec!(1)), ("xbt", dec!(2))]))]);

        assert_eq!(balances.len(), 1, "{balances:?}");
        assert_eq!(balances[0].currency.code.as_str(), "BTC");
    }

    #[rstest]
    fn test_parse_margin_account_margins() {
        let account = FuturesAccount {
            account_type: KrakenFuturesAccountType::MarginAccount,
            currency: Some("xbt".to_string()),
            balances: [("xbt".to_string(), dec!(2))].into_iter().collect(),
            currencies: AHashMap::new(),
            auxiliary: None,
            margin_requirements: Some(FuturesMarginRequirements {
                im: Some(dec!(100)),
                mm: Some(dec!(50)),
                lt: None,
                tt: None,
            }),
            portfolio_value: None,
            available_margin: None,
            initial_margin: None,
            maintenance_margin: None,
            pnl: None,
        };

        let (_, margins) = entries_for(&[("wallet", account)]);

        assert_eq!(margins.len(), 1);
        let margin = &margins[0];
        assert_eq!(margin.currency.code.as_str(), "BTC");
        assert_eq!(margin.initial.as_decimal(), dec!(100));
        assert_eq!(margin.maintenance.as_decimal(), dec!(50));
    }

    #[rstest]
    fn test_parse_margin_account_margins_no_requirements() {
        let account = FuturesAccount {
            account_type: KrakenFuturesAccountType::MarginAccount,
            currency: None,
            balances: AHashMap::new(),
            currencies: AHashMap::new(),
            auxiliary: None,
            margin_requirements: None,
            portfolio_value: None,
            available_margin: None,
            initial_margin: None,
            maintenance_margin: None,
            pnl: None,
        };

        let (_, margins) = entries_for(&[("wallet", account)]);

        assert_eq!(margins.len(), 0);
    }

    #[rstest]
    fn test_parse_multi_collateral_balances() {
        let mut currencies = AHashMap::new();
        currencies.insert(
            "BTC".to_string(),
            FuturesFlexCurrency {
                quantity: dec!(1.5),
                value: None,
                collateral: None,
                available: Some(dec!(1.2)),
            },
        );

        let account = FuturesAccount {
            account_type: KrakenFuturesAccountType::MultiCollateralMarginAccount,
            currency: None,
            balances: AHashMap::new(),
            currencies,
            auxiliary: None,
            margin_requirements: None,
            portfolio_value: Some(dec!(50000)),
            available_margin: Some(dec!(45000)),
            initial_margin: None,
            maintenance_margin: None,
            pnl: None,
        };

        let (balances, _) = entries_for(&[("wallet", account)]);

        // BTC balance + USD portfolio balance
        assert_eq!(balances.len(), 2);
    }

    #[rstest]
    fn test_parse_margin_account_balances_preserves_exact_values() {
        let mut bals = AHashMap::new();
        bals.insert("XBT".to_string(), dec!(10.00000003));

        let account = FuturesAccount {
            account_type: KrakenFuturesAccountType::MarginAccount,
            currency: Some("xbt".to_string()),
            balances: bals,
            currencies: AHashMap::new(),
            auxiliary: Some(FuturesAuxiliary {
                usd: None,
                pv: None,
                pnl: None,
                af: Some(dec!(0.00000004)),
                funding: None,
            }),
            margin_requirements: None,
            portfolio_value: None,
            available_margin: None,
            initial_margin: None,
            maintenance_margin: None,
            pnl: None,
        };

        let (balances, _) = entries_for(&[("wallet", account)]);

        assert_eq!(balances.len(), 1);
        let balance = &balances[0];
        assert_eq!(balance.total.as_decimal(), dec!(10.00000003));
        assert_eq!(balance.locked.as_decimal(), dec!(9.99999999));
        assert_eq!(balance.free.as_decimal(), dec!(0.00000004));
        assert_eq!(balance.total, balance.locked + balance.free);
    }

    #[rstest]
    fn test_parse_cash_account_balances() {
        let mut bals = AHashMap::new();
        bals.insert("ETH".to_string(), dec!(10));
        bals.insert("BTC".to_string(), Decimal::ZERO);

        let account = FuturesAccount {
            account_type: KrakenFuturesAccountType::CashAccount,
            currency: None,
            balances: bals,
            currencies: AHashMap::new(),
            auxiliary: None,
            margin_requirements: None,
            portfolio_value: None,
            available_margin: None,
            initial_margin: None,
            maintenance_margin: None,
            pnl: None,
        };

        let (balances, _) = entries_for(&[("wallet", account)]);

        assert_eq!(
            balances.len(),
            2,
            "a wallet listed at zero is reported: {balances:?}"
        );
        let eth = balances
            .iter()
            .find(|b| b.currency.code.as_str() == "ETH")
            .expect("ETH balance");
        assert_eq!(eth.total.as_decimal(), dec!(10));
        assert_eq!(eth.locked.as_decimal(), Decimal::ZERO);
        let btc = balances
            .iter()
            .find(|b| b.currency.code.as_str() == "BTC")
            .expect("BTC balance");
        assert_eq!(btc.total.as_decimal(), Decimal::ZERO);
        assert_eq!(btc.free.as_decimal(), Decimal::ZERO);
    }

    /// A flex collateral currency at zero quantity is reported at zero.
    #[rstest]
    fn test_parse_multi_collateral_balances_reports_zero_quantity() {
        let (balances, _) = entries_for(&[("flex", flex_wallet(&[("XBT", Decimal::ZERO)], None))]);

        assert_eq!(balances.len(), 1, "{balances:?}");
        assert_eq!(balances[0].currency.code.as_str(), "BTC");
        assert_eq!(balances[0].total.as_decimal(), Decimal::ZERO);
        assert_eq!(balances[0].free.as_decimal(), Decimal::ZERO);
    }

    /// A single-collateral wallet drawn down to zero is reported at zero.
    #[rstest]
    fn test_parse_margin_account_balances_reports_zero_wallet() {
        let mut wallet = cash_wallet(&[("xbt", Decimal::ZERO)]);
        wallet.account_type = KrakenFuturesAccountType::MarginAccount;

        let (balances, _) = entries_for(&[("fi_xbtusd", wallet)]);

        assert_eq!(balances.len(), 1, "{balances:?}");
        assert_eq!(balances[0].currency.code.as_str(), "BTC");
        assert_eq!(balances[0].total.as_decimal(), Decimal::ZERO);
        assert_eq!(balances[0].free.as_decimal(), Decimal::ZERO);
    }

    /// An empty wallet joins the per-currency sum, so it cannot displace a funded wallet.
    #[rstest]
    fn test_parse_account_entries_zero_wallet_does_not_displace_a_funded_one() {
        let mut margin = cash_wallet(&[("xbt", dec!(1.5))]);
        margin.account_type = KrakenFuturesAccountType::MarginAccount;

        let (balances, _) = entries_for(&[
            ("cash", cash_wallet(&[("xbt", Decimal::ZERO)])),
            ("fi_xbtusd", margin),
            ("flex", flex_wallet(&[("XBT", Decimal::ZERO)], None)),
        ]);

        assert_eq!(balances.len(), 1, "one BTC balance: {balances:?}");
        assert_eq!(balances[0].currency.code.as_str(), "BTC");
        assert_eq!(balances[0].total.as_decimal(), dec!(1.5));
        assert_eq!(balances[0].free.as_decimal(), dec!(1.5));
    }

    /// An asset every wallet lists at zero is reported once, at zero.
    #[rstest]
    fn test_parse_account_entries_reports_an_asset_all_wallets_hold_at_zero() {
        let mut margin = cash_wallet(&[("xbt", Decimal::ZERO)]);
        margin.account_type = KrakenFuturesAccountType::MarginAccount;

        let (balances, _) = entries_for(&[
            ("cash", cash_wallet(&[("xbt", Decimal::ZERO)])),
            ("fi_xbtusd", margin),
            ("flex", flex_wallet(&[("XBT", Decimal::ZERO)], None)),
        ]);

        assert_eq!(balances.len(), 1, "one BTC balance: {balances:?}");
        assert_eq!(balances[0].currency.code.as_str(), "BTC");
        assert_eq!(balances[0].total.as_decimal(), Decimal::ZERO);
        assert_eq!(balances[0].locked.as_decimal(), Decimal::ZERO);
        assert_eq!(balances[0].free.as_decimal(), Decimal::ZERO);
    }

    #[rstest]
    #[case(None, None)]
    #[case(Some(TriggerType::Default), Some(KrakenTriggerSignal::Last))]
    #[case(Some(TriggerType::LastPrice), Some(KrakenTriggerSignal::Last))]
    #[case(Some(TriggerType::MarkPrice), Some(KrakenTriggerSignal::Mark))]
    #[case(Some(TriggerType::IndexPrice), Some(KrakenTriggerSignal::Index))]
    fn test_build_send_order_params_maps_supported_trigger_signals(
        #[case] trigger_type: Option<TriggerType>,
        #[case] expected_signal: Option<KrakenTriggerSignal>,
    ) {
        let client = KrakenFuturesHttpClient::default();
        let instrument_id = cache_test_futures_instrument(&client);

        let params = client
            .build_send_order_params(
                instrument_id,
                ClientOrderId::new("futures-trigger"),
                OrderSide::Buy,
                OrderType::StopMarket,
                Quantity::from("1"),
                TimeInForce::Gtc,
                None,
                Some(Price::from("45000")),
                trigger_type,
                false,
                false,
            )
            .unwrap();

        assert_eq!(params.trigger_signal, expected_signal);
    }

    #[rstest]
    fn test_build_send_order_params_rejects_unsupported_trigger_signal() {
        let client = KrakenFuturesHttpClient::default();
        let instrument_id = cache_test_futures_instrument(&client);

        let error = client
            .build_send_order_params(
                instrument_id,
                ClientOrderId::new("futures-trigger-invalid"),
                OrderSide::Buy,
                OrderType::StopMarket,
                Quantity::from("1"),
                TimeInForce::Gtc,
                None,
                Some(Price::from("45000")),
                Some(TriggerType::BidAsk),
                false,
                false,
            )
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("Unsupported trigger type for Kraken Futures")
        );
    }

    /// A history page of two rows on `PF_XBTUSD`; the first row's order type is `first_order_type`
    /// and the page hands back `continuation_token` when one is given.
    fn history_page(first_order_type: &str, continuation_token: Option<&str>) -> String {
        let token = continuation_token
            .map(|token| format!(r#","continuationToken":"{token}""#))
            .unwrap_or_default();
        format!(
            r#"{{"elements":[{{"uid":"e1","timestamp":1680876930250,"event":{{"OrderPlaced":{{"order":{{"uid":"H-SKIP-1","tradeable":"PF_XBTUSD","direction":"Sell","quantity":"3","filled":"1","limitPrice":"70000","orderType":"{first_order_type}","clientId":"cl-1","reduceOnly":false,"timestamp":1680876930250,"lastUpdateTimestamp":1680876930250}}}}}}}},{{"uid":"e2","timestamp":1680877245500,"event":{{"OrderPlaced":{{"order":{{"uid":"H-KEEP-2","tradeable":"PF_XBTUSD","direction":"Buy","quantity":"2","filled":"0.5","limitPrice":"69500","orderType":"Limit","clientId":"cl-2","reduceOnly":false,"timestamp":1680877245500,"lastUpdateTimestamp":1680877245500}}}}}}}}]{token}}}"#
        )
    }

    /// A client against a mock venue with no open orders whose order history serves `history`,
    /// with `header_token` in the `Next-Continuation-Token` header when one is given.
    async fn history_test_client(
        history: Arc<RwLock<String>>,
        header_token: Option<&'static str>,
    ) -> KrakenFuturesHttpClient {
        history_test_client_with_open_orders(
            r#"{"result":"success","openOrders":[]}"#.to_string(),
            history,
            header_token,
        )
        .await
    }

    /// As [`history_test_client`], for a venue serving `open_orders` as its open-orders response.
    async fn history_test_client_with_open_orders(
        open_orders: String,
        history: Arc<RwLock<String>>,
        header_token: Option<&'static str>,
    ) -> KrakenFuturesHttpClient {
        use axum::{
            Router, body::Body, extract::State, http::header, response::Response, routing::any,
        };

        let app = Router::new()
            .route(
                "/derivatives/api/v3/openorders",
                any(move || {
                    let open_orders = open_orders.clone();
                    async move { ([(header::CONTENT_TYPE, "application/json")], open_orders) }
                }),
            )
            .route(
                "/api/history/v3/orders",
                any(
                    move |State(history): State<Arc<RwLock<String>>>| async move {
                        let mut response =
                            Response::builder().header(header::CONTENT_TYPE, "application/json");

                        if let Some(token) = header_token {
                            response = response.header(NEXT_CONTINUATION_TOKEN_HEADER, token);
                        }
                        response.body(Body::from(history.read().clone())).unwrap()
                    },
                ),
            )
            .with_state(history);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        KrakenFuturesHttpClient::with_credentials(
            "test".to_string(),
            "test".to_string(),
            KrakenEnvironment::Live,
            Some(format!("http://{addr}")),
            10,
            None,
            None,
            None,
            None,
            10,
        )
        .unwrap()
    }

    /// A page that hands back a continuation token, in the body or only in the
    /// `Next-Continuation-Token` header, leaves events of the window unread, so the read reports
    /// the page's rows and marks the set incomplete; without a token the same page reads complete.
    #[rstest]
    #[case::body_token(Some("c2ltYjE3OA=="), None, false)]
    #[case::header_token(None, Some("c2ltYjE3OA=="), false)]
    #[case::no_token(None, None, true)]
    #[tokio::test]
    async fn test_request_order_status_reports_marks_a_page_with_a_continuation_token_incomplete(
        #[case] body_token: Option<&str>,
        #[case] header_token: Option<&'static str>,
        #[case] expected_complete: bool,
    ) {
        let history = Arc::new(RwLock::new(history_page("Limit", body_token)));
        let client = history_test_client(history, header_token).await;
        cache_test_futures_instrument(&client);

        let (reports, coverage) = client
            .request_order_status_reports_checked(
                AccountId::from("KRAKEN-001"),
                None,
                None,
                None,
                false,
            )
            .await
            .unwrap();

        assert_eq!(coverage.is_complete(), expected_complete);
        assert_eq!(
            reports
                .iter()
                .map(|report| report.venue_order_id.as_str())
                .collect::<Vec<_>>(),
            vec!["H-SKIP-1", "H-KEEP-2"],
            "the page's rows are reported either way"
        );
    }

    /// An edit that resizes the order sends its size as the order's total, fills included, since
    /// the venue reads a bare size as the open size, on the single and the batch edit alike; a
    /// price-only edit sends no size and no mode.
    #[rstest]
    #[case::resize(Some("5"), Some("ABSOLUTE"))]
    #[case::price_only(None, None)]
    #[tokio::test]
    async fn test_build_edit_order_params_sends_a_size_as_the_total(
        #[case] size: Option<&str>,
        #[case] expected_qty_mode: Option<&str>,
    ) {
        let client = history_test_client(Arc::new(RwLock::new(String::new())), None).await;
        let instrument_id = cache_test_futures_instrument(&client);

        let params = client
            .build_edit_order_params(
                instrument_id,
                None,
                Some(VenueOrderId::from("V-EDIT-1")),
                size.map(Quantity::from),
                Some(Price::from("70000")),
                None,
            )
            .unwrap();

        let form: Vec<(String, String)> =
            serde_urlencoded::from_str(&serde_urlencoded::to_string(&params).unwrap()).unwrap();
        let form_field = |name: &str| {
            form.iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
        };
        assert_eq!(form_field("size"), size, "{form:?}");
        assert_eq!(form_field("qtyMode"), expected_qty_mode, "{form:?}");

        let item =
            serde_json::to_value(KrakenFuturesBatchEditItem::from_params(params, "tag-1")).unwrap();
        let item_field = |name: &str| item.get(name).and_then(|value| value.as_str());
        assert_eq!(item_field("size"), size, "{item}");
        assert_eq!(item_field("qtyMode"), expected_qty_mode, "{item}");
    }

    /// A closed order does not reopen: a refused edit logged after the cancel carries the order as
    /// it stood before the cancel, so the order is reported canceled although the refusal is
    /// stamped later.
    #[rstest]
    #[tokio::test]
    async fn test_request_order_status_reports_keep_a_closed_order_closed_after_a_refused_edit() {
        let order = |last_update_ms: u64| {
            format!(
                r#"{{"uid":"H-CLOSED","tradeable":"PF_XBTUSD","direction":"Buy","quantity":"1","filled":"0","limitPrice":"70000","orderType":"Limit","clientId":"","reduceOnly":false,"timestamp":1680876930250,"lastUpdateTimestamp":{last_update_ms}}}"#
            )
        };
        // Newest first, as the venue sorts: the refused edit at T2, the cancel at T1, the
        // placement at T0.
        let page = format!(
            r#"{{"elements":[{{"uid":"e3","timestamp":1680877400000,"event":{{"OrderEditRejected":{{"oldOrder":{},"reason":"order_for_edit_not_found"}}}}}},{{"uid":"e2","timestamp":1680877300000,"event":{{"OrderCancelled":{{"order":{},"reason":"cancelled_by_user"}}}}}},{{"uid":"e1","timestamp":1680876930250,"event":{{"OrderPlaced":{{"order":{},"reason":"new_user_order"}}}}}}]}}"#,
            order(1680876930250),
            order(1680877300000),
            order(1680876930250),
        );
        let client = history_test_client(Arc::new(RwLock::new(page)), None).await;
        cache_test_futures_instrument(&client);

        let (reports, coverage) = client
            .request_order_status_reports_checked(
                AccountId::from("KRAKEN-001"),
                None,
                None,
                None,
                false,
            )
            .await
            .unwrap();

        assert!(coverage.is_complete());
        assert_eq!(reports.len(), 1, "{reports:?}");
        assert_eq!(reports[0].venue_order_id, VenueOrderId::from("H-CLOSED"));
        assert_eq!(reports[0].order_status, OrderStatus::Canceled);
    }

    /// A history row the projection cannot represent marks the checked read incomplete while
    /// the rows it can represent are still reported.
    #[rstest]
    #[tokio::test]
    async fn test_request_order_status_reports_marks_the_set_incomplete_for_a_skipped_history_row()
    {
        let history = Arc::new(RwLock::new(history_page("Limit", None)));
        let client = history_test_client(history.clone(), None).await;
        let instrument_id = cache_test_futures_instrument(&client);
        let account_id = AccountId::from("KRAKEN-001");

        // Control: the same page with a decodable type reads complete, so the flag below is
        // driven by the undecodable type rather than by the page itself.
        let (control, coverage) = client
            .request_order_status_reports_checked(account_id, None, None, None, false)
            .await
            .unwrap();
        assert!(
            coverage.is_complete(),
            "the control must be complete, or the skip proves nothing"
        );
        assert_eq!(control.len(), 2);

        *history.write() = history_page("Unknown", None);
        let (reports, coverage) = client
            .request_order_status_reports_checked(account_id, None, None, None, false)
            .await
            .unwrap();

        assert!(
            !coverage.is_complete(),
            "a skipped history row must leave the set incomplete"
        );
        assert_eq!(reports.len(), 1);
        let report = &reports[0];
        assert_eq!(report.venue_order_id, VenueOrderId::from("H-KEEP-2"));
        assert_eq!(report.instrument_id, instrument_id);
        assert_eq!(report.account_id, account_id);
        assert_eq!(report.client_order_id, Some(ClientOrderId::from("cl-2")));
        assert_eq!(report.order_side, Some(OrderSide::Buy));
        assert_eq!(report.order_type, OrderType::Limit);
        assert_eq!(report.quantity, Quantity::from("2"));
        assert_eq!(report.filled_qty, Quantity::from("0.5"));
        assert_eq!(report.price, Some(Price::from("69500")));
    }

    /// The order history request asks for the newest events first rather than relying on the
    /// venue default, since a page with a continuation token is read as reaching back to its
    /// oldest event, and for the largest page, since a read takes only one.
    #[rstest]
    #[tokio::test]
    async fn test_get_order_events_asks_for_a_full_page_of_the_newest_events() {
        use axum::{Router, extract::RawQuery, routing::any};

        let query: Arc<RwLock<Option<String>>> = Arc::new(RwLock::new(None));
        let recorded = query.clone();
        let app = Router::new().route(
            "/api/history/v3/orders",
            any(move |RawQuery(raw): RawQuery| {
                let recorded = recorded.clone();
                async move {
                    *recorded.write() = raw;
                    r#"{"elements":[]}"#
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = KrakenFuturesHttpClient::with_credentials(
            "test".to_string(),
            "test".to_string(),
            KrakenEnvironment::Live,
            Some(format!("http://{addr}")),
            10,
            None,
            None,
            None,
            None,
            10,
        )
        .unwrap();

        client
            .inner
            .get_order_events(None, None, None)
            .await
            .unwrap();

        let query = query.read().clone().unwrap_or_default();
        assert!(
            query.split('&').any(|param| param == "sort=desc"),
            "{query}"
        );
        assert!(
            query.split('&').any(|param| param == "count=1000"),
            "{query}"
        );
    }

    const HISTORY_T0: i64 = 1_680_877_000_000;
    const HISTORY_T1: i64 = HISTORY_T0 + 1_000;
    const HISTORY_T2: i64 = HISTORY_T0 + 2_000;
    const HISTORY_T3: i64 = HISTORY_T0 + 3_000;

    /// A history order of `uid` on `tradeable` with its own stamp at `own_ms`, with `fields`
    /// standing in for the quantity and order type.
    fn history_order(uid: &str, tradeable: &str, own_ms: i64, fields: &str) -> String {
        format!(
            r#"{{"uid":"{uid}","tradeable":"{tradeable}","direction":"Buy","filled":"0","limitPrice":"70000","clientId":"","reduceOnly":false,"timestamp":{HISTORY_T0},"lastUpdateTimestamp":{own_ms},{fields}}}"#
        )
    }

    /// An element of `kind` at `event_ms` carrying `order` where the kind documents it.
    fn history_element(kind: &str, event_ms: i64, order: &str) -> String {
        let position = match kind {
            "OrderUpdated" => "newOrder",
            "OrderEditRejected" => "oldOrder",
            _ => "order",
        };
        format!(
            r#"{{"uid":"e-{event_ms}","timestamp":{event_ms},"event":{{"{kind}":{{"{position}":{order}}}}}}}"#
        )
    }

    /// A readable row of `uid` on the test instrument.
    fn readable_row(kind: &str, uid: &str, event_ms: i64, own_ms: i64) -> String {
        let order = history_order(
            uid,
            "PF_XBTUSD",
            own_ms,
            r#""quantity":"3","orderType":"Limit""#,
        );
        history_element(kind, event_ms, &order)
    }

    /// A row whose quantity does not decode, so the projection never sees its state.
    fn unreadable_row(uid: &str, event_ms: i64) -> String {
        let order = history_order(
            uid,
            "PF_XBTUSD",
            event_ms,
            r#""quantity":"oops","orderType":"Limit""#,
        );
        history_element("OrderCancelled", event_ms, &order)
    }

    /// A row that decodes but fails to parse into a report, its quantity negative.
    fn unparsable_row(uid: &str, event_ms: i64) -> String {
        let order = history_order(
            uid,
            "PF_XBTUSD",
            event_ms,
            r#""quantity":"-1","orderType":"Limit""#,
        );
        history_element("OrderCancelled", event_ms, &order)
    }

    /// A row the projection cannot represent, its type reported as `Unknown`.
    fn unrepresentable_row(uid: &str, event_ms: i64) -> String {
        let order = history_order(
            uid,
            "PF_XBTUSD",
            event_ms,
            r#""quantity":"3","orderType":"Unknown""#,
        );
        history_element("OrderCancelled", event_ms, &order)
    }

    /// A row that decodes, on a contract the client cannot resolve.
    fn unresolved_row(uid: &str, event_ms: i64) -> String {
        let order = history_order(
            uid,
            "PF_NOPE",
            event_ms,
            r#""quantity":"3","orderType":"Limit""#,
        );
        history_element("OrderCancelled", event_ms, &order)
    }

    /// A row of a kind the documentation does not list.
    fn undocumented_row(uid: &str, event_ms: i64) -> String {
        let order = history_order(
            uid,
            "PF_XBTUSD",
            event_ms,
            r#""quantity":"3","orderType":"Limit""#,
        );
        history_element("OrderSomethingNew", event_ms, &order)
    }

    /// An undecodable row naming only what it is given.
    fn partial_row(uid: Option<&str>, tradeable: Option<&str>, event_ms: i64) -> String {
        let uid = uid
            .map(|uid| format!(r#""uid":"{uid}","#))
            .unwrap_or_default();
        let tradeable = tradeable
            .map(|tradeable| format!(r#""tradeable":"{tradeable}","#))
            .unwrap_or_default();
        history_element(
            "OrderCancelled",
            event_ms,
            &format!(r#"{{{uid}{tradeable}"quantity":"oops"}}"#),
        )
    }

    fn history_json(rows: &[String]) -> String {
        format!(r#"{{"elements":[{}]}}"#, rows.join(","))
    }

    fn cache_futures_contract(client: &KrakenFuturesHttpClient, raw_symbol: &str) {
        client.cache_instrument(InstrumentAny::CryptoPerpetual(
            CryptoPerpetual::builder()
                .instrument_id(InstrumentId::from(format!("{raw_symbol}.KRAKEN").as_str()))
                .raw_symbol(Symbol::new(raw_symbol))
                .base_currency(Currency::BTC())
                .quote_currency(Currency::USD())
                .settlement_currency(Currency::USD())
                .is_inverse(false)
                .price_precision(0)
                .size_precision(4)
                .price_increment(Price::from("1"))
                .size_increment(Quantity::from("0.0001"))
                .ts_event(0.into())
                .ts_init(0.into())
                .build()
                .unwrap(),
        ));
    }

    /// An open-orders response with one resting buy per `(order_id, symbol)`.
    fn open_orders_json(rows: &[(&str, &str)]) -> String {
        let orders: Vec<String> = rows
            .iter()
            .map(|(order_id, symbol)| {
                format!(
                    r#"{{"order_id":"{order_id}","symbol":"{symbol}","side":"buy","orderType":"lmt","limitPrice":70000,"unfilledSize":3,"receivedTime":"2023-04-07T14:15:30.250Z","status":"untouched","filledSize":0,"reduceOnly":false,"lastUpdateTime":"2023-04-07T14:15:30.250Z"}}"#
                )
            })
            .collect();
        format!(
            r#"{{"result":"success","openOrders":[{}]}}"#,
            orders.join(",")
        )
    }

    /// An order read of `rows` against a venue listing `open_orders`, with `PF_XBTUSD` and
    /// `PF_ETHUSD` in the cache.
    async fn read_history(
        open_orders: &[(&str, &str)],
        rows: &[String],
        scope: Option<&str>,
    ) -> (Vec<OrderStatusReport>, ReportCoverage) {
        let client = history_test_client_with_open_orders(
            open_orders_json(open_orders),
            Arc::new(RwLock::new(history_json(rows))),
            None,
        )
        .await;
        cache_futures_contract(&client, "PF_XBTUSD");
        cache_futures_contract(&client, "PF_ETHUSD");

        client
            .request_order_status_reports_checked(
                AccountId::from("KRAKEN-001"),
                scope.map(|symbol| InstrumentId::from(format!("{symbol}.KRAKEN").as_str())),
                None,
                None,
                false,
            )
            .await
            .unwrap()
    }

    fn report_status(reports: &[OrderStatusReport], uid: &str) -> Option<OrderStatus> {
        reports
            .iter()
            .find(|report| report.venue_order_id.as_str() == uid)
            .map(|report| report.order_status)
    }

    fn rules_out(coverage: &ReportCoverage, uid: &str) -> bool {
        coverage.rules_out(Some(VenueOrderId::from(uid)), None)
    }

    /// An order is reported only from a state no unread row of the order is as new as, measured
    /// against the state the fold chose, and a state other than a cancel or reject stamped before
    /// its event counts from that earlier stamp. Withheld or not, the read is incomplete, the order
    /// is not ruled out, and a sibling on the page is still reported and another order still ruled
    /// out.
    #[rstest]
    #[case::newer(
        vec![unreadable_row("H-O", HISTORY_T2), readable_row("OrderPlaced", "H-O", HISTORY_T1, HISTORY_T1)],
        None
    )]
    #[case::tied(
        vec![unreadable_row("H-O", HISTORY_T1), readable_row("OrderPlaced", "H-O", HISTORY_T1, HISTORY_T1)],
        None
    )]
    #[case::no_usable_stamp(
        vec![readable_row("OrderPlaced", "H-O", HISTORY_T1, HISTORY_T1), unreadable_row("H-O", -1)],
        None
    )]
    #[case::only_row(vec![unreadable_row("H-O", HISTORY_T1)], None)]
    #[case::unparsable_newer(
        vec![unparsable_row("H-O", HISTORY_T2), readable_row("OrderPlaced", "H-O", HISTORY_T1, HISTORY_T1)],
        None
    )]
    #[case::unrepresentable_newer(
        vec![unrepresentable_row("H-O", HISTORY_T2), readable_row("OrderPlaced", "H-O", HISTORY_T1, HISTORY_T1)],
        None
    )]
    #[case::unresolved_newer(
        vec![unresolved_row("H-O", HISTORY_T2), readable_row("OrderPlaced", "H-O", HISTORY_T1, HISTORY_T1)],
        None
    )]
    #[case::closed_older(
        vec![unreadable_row("H-O", HISTORY_T2), readable_row("OrderCancelled", "H-O", HISTORY_T1, HISTORY_T0)],
        None
    )]
    #[case::between_closed_and_later_open(
        vec![
            readable_row("OrderEditRejected", "H-O", HISTORY_T3, HISTORY_T0),
            unreadable_row("H-O", HISTORY_T2),
            readable_row("OrderCancelled", "H-O", HISTORY_T1, HISTORY_T1),
        ],
        None
    )]
    #[case::unreadable_cancel_before_refused_edit(
        vec![readable_row("OrderEditRejected", "H-O", HISTORY_T3, HISTORY_T1), unreadable_row("H-O", HISTORY_T2)],
        None
    )]
    #[case::update_stamped_before_the_unread_row(
        vec![readable_row("OrderUpdated", "H-O", HISTORY_T3, HISTORY_T1), unreadable_row("H-O", HISTORY_T2)],
        None
    )]
    #[case::older(
        vec![readable_row("OrderUpdated", "H-O", HISTORY_T2, HISTORY_T2), unreadable_row("H-O", HISTORY_T1)],
        Some(OrderStatus::Accepted)
    )]
    #[case::older_than_closed(
        vec![readable_row("OrderCancelled", "H-O", HISTORY_T2, HISTORY_T0), unreadable_row("H-O", HISTORY_T1)],
        Some(OrderStatus::Canceled)
    )]
    #[case::older_than_rejected(
        vec![readable_row("OrderRejected", "H-O", HISTORY_T2, HISTORY_T0), unreadable_row("H-O", HISTORY_T1)],
        Some(OrderStatus::Rejected)
    )]
    #[case::before_closed(
        vec![
            readable_row("OrderEditRejected", "H-O", HISTORY_T3, HISTORY_T0),
            readable_row("OrderCancelled", "H-O", HISTORY_T1, HISTORY_T1),
            unreadable_row("H-O", HISTORY_T0),
        ],
        Some(OrderStatus::Canceled)
    )]
    #[case::before_refused_edit_state(
        vec![readable_row("OrderEditRejected", "H-O", HISTORY_T3, HISTORY_T1), unreadable_row("H-O", HISTORY_T0)],
        Some(OrderStatus::Accepted)
    )]
    #[tokio::test]
    async fn test_request_order_status_reports_withhold_an_order_from_an_older_state(
        #[case] mut rows: Vec<String>,
        #[case] expected: Option<OrderStatus>,
    ) {
        rows.push(readable_row(
            "OrderPlaced",
            "H-SIBLING",
            HISTORY_T1,
            HISTORY_T1,
        ));

        let (reports, coverage) = read_history(&[], &rows, None).await;

        assert_eq!(report_status(&reports, "H-O"), expected, "{reports:?}");
        assert_eq!(
            report_status(&reports, "H-SIBLING"),
            Some(OrderStatus::Accepted)
        );
        assert!(!coverage.is_complete());
        assert!(!rules_out(&coverage, "H-O"));
        assert!(rules_out(&coverage, "H-OTHER"));
    }

    /// A scoped read cannot place a row whose contract does not resolve, so it may be the order's
    /// own and withholds the order.
    #[rstest]
    #[tokio::test]
    async fn test_request_order_status_reports_withhold_on_a_scoped_read_for_an_unresolved_row() {
        let rows = [
            unresolved_row("H-O", HISTORY_T2),
            readable_row("OrderPlaced", "H-O", HISTORY_T1, HISTORY_T1),
            readable_row("OrderPlaced", "H-SIBLING", HISTORY_T1, HISTORY_T1),
        ];

        let (reports, coverage) = read_history(&[], &rows, Some("PF_XBTUSD")).await;

        assert_eq!(report_status(&reports, "H-O"), None, "{reports:?}");
        assert!(report_status(&reports, "H-SIBLING").is_some());
        assert!(!coverage.is_complete());
        assert!(!rules_out(&coverage, "H-O"));
    }

    /// An undocumented kind's shape is only a guess, so the order it appears to name is withheld
    /// but no other order is ruled out, and a sibling is still reported.
    #[rstest]
    #[tokio::test]
    async fn test_request_order_status_reports_withhold_an_order_an_undocumented_row_names() {
        let rows = [
            undocumented_row("H-O", HISTORY_T2),
            readable_row("OrderPlaced", "H-O", HISTORY_T1, HISTORY_T1),
            readable_row("OrderPlaced", "H-SIBLING", HISTORY_T1, HISTORY_T1),
        ];

        let (reports, coverage) = read_history(&[], &rows, None).await;

        assert_eq!(report_status(&reports, "H-O"), None, "{reports:?}");
        assert!(report_status(&reports, "H-SIBLING").is_some());
        assert!(!coverage.is_complete());
        assert!(!rules_out(&coverage, "H-OTHER"));
    }

    /// A row that names no order may be any order's, so the read rules out none, but it can
    /// withhold none either: the page's orders are still reported.
    #[rstest]
    #[tokio::test]
    async fn test_request_order_status_reports_keep_siblings_beside_an_unidentified_row() {
        let rows = [
            partial_row(None, Some("PF_XBTUSD"), HISTORY_T2),
            readable_row("OrderPlaced", "H-SIBLING", HISTORY_T1, HISTORY_T1),
        ];

        let (reports, coverage) = read_history(&[], &rows, None).await;

        assert!(report_status(&reports, "H-SIBLING").is_some());
        assert!(!coverage.is_complete());
        assert!(!rules_out(&coverage, "H-OTHER"));
    }

    /// A decoded row whose order ID cannot be an ID names no order: the read survives it, rules
    /// out none and keeps the page's other rows, even with the row on another contract than a
    /// scoped read's target.
    #[rstest]
    #[case::empty("", "PF_XBTUSD", None)]
    #[case::whitespace(" ", "PF_XBTUSD", None)]
    #[case::non_ascii("\u{e9}", "PF_XBTUSD", None)]
    #[case::scoped_other_contract("", "PF_ETHUSD", Some("PF_XBTUSD"))]
    #[tokio::test]
    async fn test_request_order_status_reports_treat_an_invalid_order_id_as_unidentified(
        #[case] uid: &str,
        #[case] tradeable: &str,
        #[case] scope: Option<&str>,
    ) {
        let order = history_order(
            uid,
            tradeable,
            HISTORY_T2,
            r#""quantity":"3","orderType":"Limit""#,
        );
        let rows = [
            history_element("OrderPlaced", HISTORY_T2, &order),
            readable_row("OrderPlaced", "H-SIBLING", HISTORY_T1, HISTORY_T1),
        ];

        let (reports, coverage) = read_history(&[], &rows, scope).await;

        assert_eq!(reports.len(), 1, "{reports:?}");
        assert!(report_status(&reports, "H-SIBLING").is_some());
        assert!(!coverage.is_complete());
        assert!(!rules_out(&coverage, "H-OTHER"));
    }

    /// With no open-order snapshot covering it, a skipped row stays out of the coverage only when
    /// it is provably outside the read: it names an order and a contract the cache resolves to
    /// another instrument than the read's target. A row that establishes less, or a read with no
    /// target, counts it.
    #[rstest]
    #[case::scoped_target(Some("PF_XBTUSD"), Some("H-O"), Some("PF_XBTUSD"), false, false, true)]
    #[case::scoped_target_other_case(
        Some("PF_XBTUSD"),
        Some("H-O"),
        Some("pf_xbtusd"),
        false,
        false,
        true
    )]
    #[case::scoped_unresolved(Some("PF_XBTUSD"), Some("H-O"), Some("PF_NOPE"), false, false, true)]
    #[case::scoped_no_contract(Some("PF_XBTUSD"), Some("H-O"), None, false, false, true)]
    #[case::scoped_other_contract(
        Some("PF_XBTUSD"),
        Some("H-O"),
        Some("PF_ETHUSD"),
        true,
        true,
        true
    )]
    #[case::scoped_other_contract_other_case(
        Some("PF_XBTUSD"),
        Some("H-O"),
        Some("pf_ethusd"),
        true,
        true,
        true
    )]
    #[case::scoped_other_contract_no_order(
        Some("PF_XBTUSD"),
        None,
        Some("PF_ETHUSD"),
        false,
        false,
        false
    )]
    #[case::scoped_neither(Some("PF_XBTUSD"), None, None, false, false, false)]
    #[case::unscoped_named(None, Some("H-O"), Some("PF_ETHUSD"), false, false, true)]
    #[case::unscoped_no_order(None, None, Some("PF_ETHUSD"), false, false, false)]
    #[tokio::test]
    async fn test_request_order_status_reports_count_a_skipped_row_unless_provably_out_of_scope(
        #[case] scope: Option<&str>,
        #[case] uid: Option<&str>,
        #[case] tradeable: Option<&str>,
        #[case] expected_complete: bool,
        #[case] expected_rules_out_order: bool,
        #[case] expected_rules_out_other: bool,
    ) {
        let rows = [
            partial_row(uid, tradeable, HISTORY_T2),
            readable_row("OrderPlaced", "H-SIBLING", HISTORY_T1, HISTORY_T1),
        ];

        let (reports, coverage) = read_history(&[], &rows, scope).await;

        assert!(report_status(&reports, "H-SIBLING").is_some());
        assert_eq!(coverage.is_complete(), expected_complete);
        assert_eq!(rules_out(&coverage, "H-O"), expected_rules_out_order);
        assert_eq!(rules_out(&coverage, "H-OTHER"), expected_rules_out_other);
    }

    /// A row of an order the open-order snapshot reports does not count, however it failed, since
    /// the snapshot is the evidence for that order.
    #[rstest]
    #[case::unreadable(unreadable_row("H-OPEN", HISTORY_T2))]
    #[case::unreadable_no_contract(partial_row(Some("H-OPEN"), None, HISTORY_T2))]
    #[case::unparsable(unparsable_row("H-OPEN", HISTORY_T2))]
    #[case::unrepresentable(unrepresentable_row("H-OPEN", HISTORY_T2))]
    #[case::unresolved(unresolved_row("H-OPEN", HISTORY_T2))]
    #[tokio::test]
    async fn test_request_order_status_reports_ignore_a_skipped_row_the_snapshot_covers(
        #[case] row: String,
    ) {
        let (reports, coverage) = read_history(&[("H-OPEN", "PF_XBTUSD")], &[row], None).await;

        assert_eq!(reports.len(), 1, "{reports:?}");
        assert_eq!(reports[0].venue_order_id, VenueOrderId::from("H-OPEN"));
        assert!(coverage.is_complete());
    }

    /// A snapshot row a scoped read passes over reports nothing, so it covers nothing: a row of
    /// that order that does not name its contract still counts.
    #[rstest]
    #[tokio::test]
    async fn test_request_order_status_reports_count_a_row_of_an_order_listed_out_of_scope() {
        let rows = [partial_row(Some("H-OPEN"), None, HISTORY_T2)];

        let (reports, coverage) =
            read_history(&[("H-OPEN", "PF_ETHUSD")], &rows, Some("PF_XBTUSD")).await;

        assert!(reports.is_empty(), "{reports:?}");
        assert!(!coverage.is_complete());
        assert!(!rules_out(&coverage, "H-OPEN"));
    }

    /// An undocumented kind proves nothing about its order, so the snapshot does not excuse it.
    #[rstest]
    #[tokio::test]
    async fn test_request_order_status_reports_count_an_undocumented_row_the_snapshot_lists() {
        let rows = [undocumented_row("H-OPEN", HISTORY_T2)];

        let (reports, coverage) = read_history(&[("H-OPEN", "PF_XBTUSD")], &rows, None).await;

        assert_eq!(reports.len(), 1, "{reports:?}");
        assert!(!coverage.is_complete());
        assert!(!rules_out(&coverage, "H-OTHER"));
    }

    /// An undocumented kind proves nothing about its contract either, so a scoped read counts it
    /// even when the order it appears to name is on another contract.
    #[rstest]
    #[tokio::test]
    async fn test_request_order_status_reports_count_an_undocumented_row_of_another_contract() {
        let order = history_order(
            "H-O",
            "PF_ETHUSD",
            HISTORY_T2,
            r#""quantity":"3","orderType":"Limit""#,
        );
        let rows = [history_element("OrderSomethingNew", HISTORY_T2, &order)];

        let (_, coverage) = read_history(&[], &rows, Some("PF_XBTUSD")).await;

        assert!(!coverage.is_complete());
        assert!(!rules_out(&coverage, "H-OTHER"));
    }

    /// A continuation token still cuts the read short at the oldest event beside a skipped row.
    #[rstest]
    #[tokio::test]
    async fn test_request_order_status_reports_keep_the_cut_beside_a_skipped_row() {
        let page = format!(
            r#"{{"elements":[{},{}],"continuationToken":"c2ltYjE3OA=="}}"#,
            unreadable_row("H-O", HISTORY_T2),
            readable_row("OrderPlaced", "H-SIBLING", HISTORY_T1, HISTORY_T1),
        );
        let client = history_test_client(Arc::new(RwLock::new(page)), None).await;
        cache_test_futures_instrument(&client);

        let (reports, coverage) = client
            .request_order_status_reports_checked(
                AccountId::from("KRAKEN-001"),
                None,
                None,
                None,
                false,
            )
            .await
            .unwrap();

        assert_eq!(reports.len(), 1, "{reports:?}");
        assert!(!coverage.is_complete());
        let after_the_cut = UnixNanos::from((HISTORY_T1 as u64 + 120_000) * 1_000_000);
        let before_the_cut = UnixNanos::from((HISTORY_T1 as u64 - 120_000) * 1_000_000);
        assert!(coverage.rules_out(Some(VenueOrderId::from("H-OTHER")), Some(after_the_cut)));
        assert!(!coverage.rules_out(Some(VenueOrderId::from("H-OTHER")), Some(before_the_cut)));
    }

    const DOCUMENTED_EDIT_RESPONSE: &str =
        include_str!("../../../test_data/http_edit_order_futures.json");
    const DOCUMENTED_EDIT_ORDER_ID: &str = "022774bc-2c4a-4f26-9317-436c8d85746d";

    /// Kraken's documented edit response with its `orderId` replaced or removed, and the send
    /// response's `order_id` key added.
    fn edit_response(
        order_id: Option<serde_json::Value>,
        order_id_snake_case: Option<serde_json::Value>,
    ) -> String {
        let mut response: serde_json::Value =
            serde_json::from_str(DOCUMENTED_EDIT_RESPONSE).unwrap();
        let edit_status = response["editStatus"].as_object_mut().unwrap();
        match order_id {
            Some(order_id) => edit_status.insert("orderId".to_string(), order_id),
            None => edit_status.remove("orderId"),
        };

        if let Some(order_id) = order_id_snake_case {
            edit_status.insert("order_id".to_string(), order_id);
        }
        response.to_string()
    }

    /// Sends an edit naming the order by `venue_order_id`, or by client order ID alone, to a venue
    /// that answers with `body`.
    async fn modify_order_answered_with(
        body: String,
        venue_order_id: Option<&str>,
    ) -> anyhow::Result<VenueOrderId> {
        use axum::{Router, http::header, routing::any};

        let app = Router::new().route(
            "/derivatives/api/v3/editorder",
            any(move || {
                let body = body.clone();
                async move { ([(header::CONTENT_TYPE, "application/json")], body) }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = KrakenFuturesHttpClient::with_credentials(
            "test".to_string(),
            "test".to_string(),
            KrakenEnvironment::Live,
            Some(format!("http://{addr}")),
            10,
            None,
            None,
            None,
            None,
            10,
        )
        .unwrap();
        let instrument_id = cache_test_futures_instrument(&client);
        let client_order_id = venue_order_id
            .is_none()
            .then(|| ClientOrderId::from("O-EDIT"));

        client
            .modify_order(
                instrument_id,
                client_order_id,
                venue_order_id.map(VenueOrderId::from),
                Some(Quantity::from("1")),
                None,
                None,
            )
            .await
    }

    /// Kraken's documented response names the order whichever identifier the request used.
    #[rstest]
    #[case::by_venue_id(Some(DOCUMENTED_EDIT_ORDER_ID))]
    #[case::by_client_id(None)]
    #[tokio::test]
    async fn test_modify_order_returns_the_documented_order_id(
        #[case] request_venue_order_id: Option<&str>,
    ) {
        let venue_order_id = modify_order_answered_with(
            DOCUMENTED_EDIT_RESPONSE.to_string(),
            request_venue_order_id,
        )
        .await
        .unwrap();

        assert_eq!(venue_order_id, VenueOrderId::from(DOCUMENTED_EDIT_ORDER_ID));
    }

    /// The send response's key is read after the documented one, and a response naming no order
    /// falls back to the request's venue order ID.
    #[rstest]
    #[case::snake_case_key(None, Some(serde_json::json!("V-SNAKE")), None, "V-SNAKE")]
    #[case::both_keys(
        Some(serde_json::json!("V-DOC")),
        Some(serde_json::json!("V-SNAKE")),
        None,
        "V-DOC"
    )]
    #[case::absent(None, None, Some("V-EDIT"), "V-EDIT")]
    #[case::null(Some(serde_json::Value::Null), None, Some("V-EDIT"), "V-EDIT")]
    #[case::response_over_request(Some(serde_json::json!("V-DOC")), None, Some("V-EDIT"), "V-DOC")]
    #[tokio::test]
    async fn test_modify_order_resolves_the_order_id_by_key_then_request(
        #[case] order_id: Option<serde_json::Value>,
        #[case] order_id_snake_case: Option<serde_json::Value>,
        #[case] request_venue_order_id: Option<&str>,
        #[case] expected: &str,
    ) {
        let venue_order_id = modify_order_answered_with(
            edit_response(order_id, order_id_snake_case),
            request_venue_order_id,
        )
        .await
        .unwrap();

        assert_eq!(venue_order_id, VenueOrderId::from(expected));
    }

    /// An edit the venue applied, answered with an order ID the model cannot hold, returns the typed
    /// error: an untyped one would read as a command never sent. An invalid ID differs from an
    /// absent one, which falls back to the request's venue order ID.
    #[rstest]
    #[case::by_venue_id(Some(serde_json::json!("")), None, Some("V-EDIT"))]
    #[case::by_client_id(Some(serde_json::json!("")), None, None)]
    #[case::snake_case_key(None, Some(serde_json::json!("")), Some("V-EDIT"))]
    #[tokio::test]
    async fn test_modify_order_returns_a_typed_error_for_an_invalid_order_id(
        #[case] order_id: Option<serde_json::Value>,
        #[case] order_id_snake_case: Option<serde_json::Value>,
        #[case] request_venue_order_id: Option<&str>,
    ) {
        let error = modify_order_answered_with(
            edit_response(order_id, order_id_snake_case),
            request_venue_order_id,
        )
        .await
        .unwrap_err();

        assert!(
            matches!(
                error.downcast_ref::<KrakenModifyOrderError>(),
                Some(KrakenModifyOrderError::InvalidOrderId { detail })
                    if detail.starts_with("Invalid order_id '' in Kraken futures edit response")
            ),
            "{error:#}"
        );
    }

    /// A response naming no order leaves nothing to name it on a request by client order ID alone.
    #[rstest]
    #[case::absent(None)]
    #[case::null(Some(serde_json::Value::Null))]
    #[tokio::test]
    async fn test_modify_order_by_client_id_fails_without_an_order_id(
        #[case] order_id: Option<serde_json::Value>,
    ) {
        let error = modify_order_answered_with(edit_response(order_id, None), None)
            .await
            .unwrap_err();

        assert!(
            matches!(
                error.downcast_ref::<KrakenModifyOrderError>(),
                Some(KrakenModifyOrderError::MissingOrderId)
            ),
            "{error:#}"
        );
    }

    /// A contract the listing spells two ways in different cases cannot be told apart by a
    /// case-insensitive match, so such a spelling resolves to neither, while an exact spelling
    /// and a unique case-insensitive one still resolve.
    #[rstest]
    #[tokio::test]
    async fn test_history_tradeable_leaves_an_ambiguous_spelling_unresolved() {
        let client = history_test_client(Arc::new(RwLock::new(String::new())), None).await;
        cache_futures_contract(&client, "PF_AAPLxUSD");
        cache_futures_contract(&client, "PF_AAPLXUSD");
        cache_futures_contract(&client, "PF_XBTUSD");

        let resolve = |tradeable: &str| {
            client
                .get_instrument_by_history_tradeable(tradeable)
                .map(|instrument| instrument.raw_symbol().to_string())
        };

        assert_eq!(resolve("pf_aaplxusd"), None);
        assert_eq!(resolve("PF_AAPLxUSD"), Some("PF_AAPLxUSD".to_string()));
        assert_eq!(resolve("pf_xbtusd"), Some("PF_XBTUSD".to_string()));
    }

    /// The post-submit lookup answers from history only when no unread row may hold a newer state
    /// of the order: a row of the order, a row naming none or an undocumented row that is as new
    /// fails the lookup, while an older row or one naming another order leaves the answer standing.
    /// A refused edit's state, stamped before its event, counts from that stamp.
    #[rstest]
    #[case::order_newer(partial_row(Some("H-O"), None, HISTORY_T2), HISTORY_T1, false)]
    #[case::order_tied(partial_row(Some("H-O"), None, HISTORY_T1), HISTORY_T1, false)]
    #[case::order_older(partial_row(Some("H-O"), None, HISTORY_T0), HISTORY_T1, true)]
    #[case::unnamed_newer(partial_row(None, None, HISTORY_T2), HISTORY_T1, false)]
    #[case::unnamed_older(partial_row(None, None, HISTORY_T0), HISTORY_T1, true)]
    #[case::sibling_newer(partial_row(Some("H-SIBLING"), None, HISTORY_T2), HISTORY_T1, true)]
    #[case::undocumented_sibling_newer(
        undocumented_row("H-SIBLING", HISTORY_T2),
        HISTORY_T1,
        false
    )]
    #[case::invalid_id_newer(unrepresentable_row("", HISTORY_T2), HISTORY_T1, false)]
    #[case::invalid_id_on_a_readable_row_newer(
        readable_row("OrderPlaced", "", HISTORY_T2, HISTORY_T2),
        HISTORY_T1,
        false
    )]
    #[case::before_the_event_after_the_state(
        partial_row(Some("H-O"), None, HISTORY_T2),
        HISTORY_T3,
        false
    )]
    #[tokio::test]
    async fn test_submit_order_history_fallback_defers_to_an_unread_newer_row(
        #[case] row: String,
        #[case] found_event_ms: i64,
        #[case] expected_ok: bool,
    ) {
        use axum::{Router, http::header, routing::any};

        let page = history_json(&[
            row,
            readable_row("OrderEditRejected", "H-O", found_event_ms, HISTORY_T1),
        ]);
        let app = Router::new()
            .route(
                "/derivatives/api/v3/sendorder",
                any(|| async {
                    (
                        [(header::CONTENT_TYPE, "application/json")],
                        r#"{"result":"success","sendStatus":{"status":"placed","order_id":"H-O"}}"#,
                    )
                }),
            )
            .route(
                "/derivatives/api/v3/openorders",
                any(|| async {
                    (
                        [(header::CONTENT_TYPE, "application/json")],
                        r#"{"result":"success","openOrders":[]}"#,
                    )
                }),
            )
            .route(
                "/api/history/v3/orders",
                any(move || {
                    let page = page.clone();
                    async move { ([(header::CONTENT_TYPE, "application/json")], page) }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = KrakenFuturesHttpClient::with_credentials(
            "test".to_string(),
            "test".to_string(),
            KrakenEnvironment::Live,
            Some(format!("http://{addr}")),
            10,
            None,
            None,
            None,
            None,
            10,
        )
        .unwrap();
        cache_test_futures_instrument(&client);

        let result = client
            .submit_order(
                AccountId::from("KRAKEN-001"),
                InstrumentId::from("PF_XBTUSD.KRAKEN"),
                ClientOrderId::from("cl-fallback"),
                OrderSide::Buy,
                OrderType::Limit,
                Quantity::from("3"),
                TimeInForce::Gtc,
                Some(Price::from("70000")),
                None,
                None,
                false,
                false,
            )
            .await;

        assert_eq!(result.is_ok(), expected_ok, "{result:?}");
    }

    /// A history row's report carries the time in force and post-only its order type states.
    #[rstest]
    #[case::ioc("IoC", TimeInForce::Ioc, false)]
    #[case::fill_or_kill("FillOrKill", TimeInForce::Fok, false)]
    #[case::post("Post", TimeInForce::Gtc, true)]
    #[case::limit("Limit", TimeInForce::Gtc, false)]
    #[tokio::test]
    async fn test_request_order_status_reports_carry_the_time_in_force_of_a_history_row(
        #[case] order_type: &str,
        #[case] expected_time_in_force: TimeInForce,
        #[case] expected_post_only: bool,
    ) {
        let order = history_order(
            "H-O",
            "PF_XBTUSD",
            HISTORY_T1,
            &format!(r#""quantity":"3","orderType":"{order_type}""#),
        );
        let rows = [history_element("OrderPlaced", HISTORY_T1, &order)];

        let (reports, _) = read_history(&[], &rows, None).await;

        assert_eq!(reports.len(), 1, "{reports:?}");
        assert_eq!(reports[0].time_in_force, expected_time_in_force);
        assert_eq!(reports[0].post_only, expected_post_only);
    }

    /// The venue refuses a client order ID it already holds, or one too long or otherwise invalid,
    /// so these statuses reject the submission rather than leave it ambiguous.
    #[rstest]
    #[case::already_exists("clientOrderIdAlreadyExist")]
    #[case::too_big("clientOrderIdTooBig")]
    #[case::too_long("clientOrderIdTooLong")]
    #[case::invalid("clientOrderIdInvalid")]
    fn test_client_order_id_send_statuses_reject_the_submission(#[case] status: &str) {
        assert!(is_futures_submit_rejection(status));
    }

    const DOCUMENTED_SEND_PLACED: &str =
        include_str!("../../../test_data/http_send_order_futures_placed.json");
    const DOCUMENTED_SEND_EXECUTED: &str =
        include_str!("../../../test_data/http_send_order_futures_executed.json");
    const SEND_TRIGGER: &str =
        include_str!("../../../test_data/http_send_order_futures_unknown_trigger.json");

    /// `response` with the type of its first event's `payload` replaced by `order_type` and the
    /// send status naming that payload's order, returned with the order's ID and contract.
    fn send_response(response: &str, payload: &str, order_type: &str) -> (String, String, String) {
        let mut response: serde_json::Value = serde_json::from_str(response).unwrap();
        let order = &mut response["sendStatus"]["orderEvents"][0][payload];
        order["type"] = serde_json::json!(order_type);
        let order_id = order
            .get("orderId")
            .or_else(|| order.get("uid"))
            .and_then(|id| id.as_str())
            .unwrap()
            .to_string();
        let symbol = order["symbol"].as_str().unwrap().to_string();
        response["sendStatus"]["order_id"] = serde_json::json!(order_id);
        (response.to_string(), order_id, symbol)
    }

    /// The report of a buy limit submitted on `symbol` to a venue answering the send with `send`
    /// and the open-order read with `open_orders`.
    async fn submit_report(send: String, open_orders: String, symbol: &str) -> OrderStatusReport {
        use axum::{Router, http::header, routing::any};

        let app = Router::new()
            .route(
                "/derivatives/api/v3/sendorder",
                any(move || {
                    let send = send.clone();
                    async move { ([(header::CONTENT_TYPE, "application/json")], send) }
                }),
            )
            .route(
                "/derivatives/api/v3/openorders",
                any(move || {
                    let open_orders = open_orders.clone();
                    async move { ([(header::CONTENT_TYPE, "application/json")], open_orders) }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = KrakenFuturesHttpClient::with_credentials(
            "test".to_string(),
            "test".to_string(),
            KrakenEnvironment::Live,
            Some(format!("http://{addr}")),
            10,
            None,
            None,
            None,
            None,
            10,
        )
        .unwrap();
        cache_futures_contract(&client, symbol);

        client
            .submit_order(
                AccountId::from("KRAKEN-001"),
                InstrumentId::from(format!("{symbol}.KRAKEN").as_str()),
                ClientOrderId::from("cl-send-status"),
                OrderSide::Buy,
                OrderType::Limit,
                Quantity::from("1"),
                TimeInForce::Gtc,
                Some(Price::from("9400")),
                None,
                None,
                false,
                false,
            )
            .await
            .unwrap()
    }

    /// An open-orders response listing `order_id` on `symbol` with the order type `listed_as`.
    fn listed_open_orders(order_id: &str, symbol: &str, listed_as: &str) -> String {
        open_orders_json(&[(order_id, symbol)]).replace(
            r#""orderType":"lmt""#,
            &format!(r#""orderType":"{listed_as}""#),
        )
    }

    /// A submit reports the time in force and post-only its send status's order type states,
    /// whichever payload carries the order, whether or not the open-order read lists the order.
    #[rstest]
    #[case::order_ioc(DOCUMENTED_SEND_PLACED, "order", "ioc", None, TimeInForce::Ioc, false)]
    #[case::order_post(DOCUMENTED_SEND_PLACED, "order", "post", None, TimeInForce::Gtc, true)]
    #[case::order_lmt(DOCUMENTED_SEND_PLACED, "order", "lmt", None, TimeInForce::Gtc, false)]
    #[case::order_stp(DOCUMENTED_SEND_PLACED, "order", "stp", None, TimeInForce::Gtc, false)]
    #[case::trigger_ioc(SEND_TRIGGER, "orderTrigger", "ioc", None, TimeInForce::Ioc, false)]
    #[case::trigger_post(SEND_TRIGGER, "orderTrigger", "post", None, TimeInForce::Gtc, true)]
    #[case::trigger_lmt(SEND_TRIGGER, "orderTrigger", "lmt", None, TimeInForce::Gtc, false)]
    #[case::trigger_stp(SEND_TRIGGER, "orderTrigger", "stp", None, TimeInForce::Gtc, false)]
    #[case::prior_execution_ioc(
        DOCUMENTED_SEND_EXECUTED,
        "orderPriorExecution",
        "ioc",
        None,
        TimeInForce::Ioc,
        false
    )]
    #[case::prior_execution_lmt(
        DOCUMENTED_SEND_EXECUTED,
        "orderPriorExecution",
        "lmt",
        None,
        TimeInForce::Gtc,
        false
    )]
    #[case::prior_execution_stp(
        DOCUMENTED_SEND_EXECUTED,
        "orderPriorExecution",
        "stp",
        None,
        TimeInForce::Gtc,
        false
    )]
    #[case::listed_post(
        DOCUMENTED_SEND_PLACED,
        "order",
        "post",
        Some("lmt"),
        TimeInForce::Gtc,
        true
    )]
    #[case::listed_lmt(
        DOCUMENTED_SEND_PLACED,
        "order",
        "lmt",
        Some("lmt"),
        TimeInForce::Gtc,
        false
    )]
    #[case::listed_trigger_stp(
        SEND_TRIGGER,
        "orderTrigger",
        "stp",
        Some("stop"),
        TimeInForce::Gtc,
        false
    )]
    #[case::listed_prior_execution_lmt(
        DOCUMENTED_SEND_EXECUTED,
        "orderPriorExecution",
        "lmt",
        Some("lmt"),
        TimeInForce::Gtc,
        false
    )]
    #[case::listed_trigger_post(
        SEND_TRIGGER,
        "orderTrigger",
        "post",
        Some("stop"),
        TimeInForce::Gtc,
        true
    )]
    #[tokio::test]
    async fn test_submit_order_reads_the_time_in_force_of_the_send_status_order_type(
        #[case] response: &str,
        #[case] payload: &str,
        #[case] order_type: &str,
        #[case] listed_as: Option<&str>,
        #[case] expected_time_in_force: TimeInForce,
        #[case] expected_post_only: bool,
    ) {
        let (send, order_id, symbol) = send_response(response, payload, order_type);
        let open_orders = match listed_as {
            Some(listed_as) => listed_open_orders(&order_id, &symbol, listed_as),
            None => r#"{"result":"success","openOrders":[]}"#.to_string(),
        };

        let report = submit_report(send, open_orders, &symbol).await;

        assert_eq!(report.venue_order_id.as_str(), order_id);
        if listed_as.is_some() {
            assert_eq!(report.quantity, Quantity::from("3"));
        }
        assert_eq!(report.time_in_force, expected_time_in_force);
        assert_eq!(report.post_only, expected_post_only);
    }

    /// A row of `kind` for `uid` on the test instrument at `event_ms`, of `order_type`, its quantity
    /// 5 where the open-order snapshot lists 3.
    fn typed_row(kind: &str, uid: &str, order_type: &str, event_ms: i64) -> String {
        history_element(
            kind,
            event_ms,
            &history_order(
                uid,
                "PF_XBTUSD",
                event_ms,
                &format!(r#""quantity":"5","orderType":"{order_type}""#),
            ),
        )
    }

    /// A report built from the open-order snapshot keeps the snapshot's state but takes its time in
    /// force and post-only from the newest readable history row of its order on the page, the first
    /// listed on a tie; without such a row, or on an open-only read, it keeps the snapshot's own.
    #[rstest]
    #[case::post(
        "lmt",
        vec![typed_row("OrderPlaced", "H-OPEN", "Post", HISTORY_T1)],
        false,
        TimeInForce::Gtc,
        true
    )]
    #[case::ioc(
        "lmt",
        vec![typed_row("OrderPlaced", "H-OPEN", "IoC", HISTORY_T1)],
        false,
        TimeInForce::Ioc,
        false
    )]
    #[case::fill_or_kill(
        "lmt",
        vec![typed_row("OrderPlaced", "H-OPEN", "FillOrKill", HISTORY_T1)],
        false,
        TimeInForce::Fok,
        false
    )]
    #[case::limit(
        "lmt",
        vec![typed_row("OrderPlaced", "H-OPEN", "Limit", HISTORY_T1)],
        false,
        TimeInForce::Gtc,
        false
    )]
    #[case::history_over_snapshot(
        "post",
        vec![typed_row("OrderPlaced", "H-OPEN", "Limit", HISTORY_T1)],
        false,
        TimeInForce::Gtc,
        false
    )]
    #[case::several_rows(
        "lmt",
        vec![
            typed_row("OrderUpdated", "H-OPEN", "Post", HISTORY_T2),
            typed_row("OrderPlaced", "H-OPEN", "Post", HISTORY_T1),
        ],
        false,
        TimeInForce::Gtc,
        true
    )]
    #[case::newest_listed_first(
        "lmt",
        vec![
            typed_row("OrderUpdated", "H-OPEN", "Post", HISTORY_T2),
            typed_row("OrderPlaced", "H-OPEN", "Limit", HISTORY_T1),
        ],
        false,
        TimeInForce::Gtc,
        true
    )]
    #[case::newest_listed_last(
        "lmt",
        vec![
            typed_row("OrderPlaced", "H-OPEN", "Limit", HISTORY_T1),
            typed_row("OrderUpdated", "H-OPEN", "Post", HISTORY_T2),
        ],
        false,
        TimeInForce::Gtc,
        true
    )]
    #[case::newest_is_limit(
        "lmt",
        vec![
            typed_row("OrderUpdated", "H-OPEN", "Limit", HISTORY_T2),
            typed_row("OrderPlaced", "H-OPEN", "Post", HISTORY_T1),
        ],
        false,
        TimeInForce::Gtc,
        false
    )]
    #[case::tied(
        "lmt",
        vec![
            typed_row("OrderPlaced", "H-OPEN", "Post", HISTORY_T1),
            typed_row("OrderUpdated", "H-OPEN", "Limit", HISTORY_T1),
        ],
        false,
        TimeInForce::Gtc,
        true
    )]
    #[case::unreadable_newer_row(
        "lmt",
        vec![
            unreadable_row("H-OPEN", HISTORY_T2),
            typed_row("OrderPlaced", "H-OPEN", "Post", HISTORY_T1),
        ],
        false,
        TimeInForce::Gtc,
        true
    )]
    #[case::closing_row(
        "lmt",
        vec![typed_row("OrderCancelled", "H-OPEN", "Post", HISTORY_T2)],
        false,
        TimeInForce::Gtc,
        true
    )]
    #[case::unresolved_contract(
        "lmt",
        vec![history_element(
            "OrderPlaced",
            HISTORY_T1,
            &history_order("H-OPEN", "PF_NOPE", HISTORY_T1, r#""quantity":"5","orderType":"Post""#),
        )],
        false,
        TimeInForce::Gtc,
        true
    )]
    #[case::no_row(
        "lmt",
        vec![typed_row("OrderPlaced", "H-SIBLING", "Post", HISTORY_T1)],
        false,
        TimeInForce::Gtc,
        false
    )]
    #[case::no_row_keeps_snapshot("post", vec![], false, TimeInForce::Gtc, true)]
    #[case::open_only(
        "lmt",
        vec![typed_row("OrderPlaced", "H-OPEN", "Post", HISTORY_T1)],
        true,
        TimeInForce::Gtc,
        false
    )]
    #[tokio::test]
    async fn test_request_order_status_reports_take_an_open_order_time_in_force_from_its_history(
        #[case] snapshot_type: &str,
        #[case] rows: Vec<String>,
        #[case] open_only: bool,
        #[case] expected_time_in_force: TimeInForce,
        #[case] expected_post_only: bool,
    ) {
        let open_orders = open_orders_json(&[("H-OPEN", "PF_XBTUSD")]).replace(
            r#""orderType":"lmt""#,
            &format!(r#""orderType":"{snapshot_type}""#),
        );
        let client = history_test_client_with_open_orders(
            open_orders,
            Arc::new(RwLock::new(history_json(&rows))),
            None,
        )
        .await;
        cache_futures_contract(&client, "PF_XBTUSD");

        let (reports, coverage) = client
            .request_order_status_reports_checked(
                AccountId::from("KRAKEN-001"),
                None,
                None,
                None,
                open_only,
            )
            .await
            .unwrap();

        let open_reports: Vec<_> = reports
            .iter()
            .filter(|report| report.venue_order_id.as_str() == "H-OPEN")
            .collect();
        assert_eq!(open_reports.len(), 1, "{reports:?}");
        let report = open_reports[0];
        assert_eq!(report.order_status, OrderStatus::Accepted);
        assert_eq!(report.quantity, Quantity::from("3"));
        assert_eq!(report.time_in_force, expected_time_in_force);
        assert_eq!(report.post_only, expected_post_only);
        assert!(coverage.is_complete());
    }

    /// A page cut short by a continuation token still states the type of each order it holds.
    #[rstest]
    #[tokio::test]
    async fn test_request_order_status_reports_take_the_time_in_force_from_a_cut_page() {
        let page = format!(
            r#"{{"elements":[{}],"continuationToken":"c2ltYjE3OA=="}}"#,
            typed_row("OrderPlaced", "H-OPEN", "Post", HISTORY_T1),
        );
        let client = history_test_client_with_open_orders(
            open_orders_json(&[("H-OPEN", "PF_XBTUSD")]),
            Arc::new(RwLock::new(page)),
            None,
        )
        .await;
        cache_futures_contract(&client, "PF_XBTUSD");

        let (reports, coverage) = client
            .request_order_status_reports_checked(
                AccountId::from("KRAKEN-001"),
                None,
                None,
                None,
                false,
            )
            .await
            .unwrap();

        assert_eq!(reports.len(), 1, "{reports:?}");
        assert_eq!(reports[0].time_in_force, TimeInForce::Gtc);
        assert!(reports[0].post_only);
        assert!(!coverage.is_complete());
    }

    /// An open order takes its time in force and post-only from its own rows, never from another
    /// order's, and a closed order keeps the values of its own state, on a scoped read as on an
    /// unscoped one.
    #[rstest]
    #[case::unscoped(None)]
    #[case::scoped(Some("PF_XBTUSD"))]
    #[tokio::test]
    async fn test_request_order_status_reports_take_each_open_order_time_in_force_from_its_rows(
        #[case] scope: Option<&str>,
    ) {
        let rows = [
            typed_row("OrderCancelled", "H-DONE", "IoC", HISTORY_T3),
            typed_row("OrderUpdated", "H-OTHER", "Limit", HISTORY_T2),
            typed_row("OrderPlaced", "H-OPEN", "Post", HISTORY_T1),
        ];

        let (reports, coverage) = read_history(
            &[("H-OPEN", "PF_XBTUSD"), ("H-OTHER", "PF_XBTUSD")],
            &rows,
            scope,
        )
        .await;

        let values: Vec<_> = reports
            .iter()
            .map(|report| {
                (
                    report.venue_order_id.as_str(),
                    report.time_in_force,
                    report.post_only,
                )
            })
            .collect();
        assert_eq!(values.len(), 3, "{reports:?}");
        assert!(
            values.contains(&("H-OPEN", TimeInForce::Gtc, true)),
            "{values:?}"
        );
        assert!(
            values.contains(&("H-OTHER", TimeInForce::Gtc, false)),
            "{values:?}"
        );
        assert!(
            values.contains(&("H-DONE", TimeInForce::Ioc, false)),
            "{values:?}"
        );
        assert!(coverage.is_complete());
    }

    fn cache_test_futures_instrument(client: &KrakenFuturesHttpClient) -> InstrumentId {
        let instrument_id = InstrumentId::from("PF_XBTUSD.KRAKEN");

        client.cache_instrument(InstrumentAny::CryptoPerpetual(
            CryptoPerpetual::builder()
                .instrument_id(instrument_id)
                .raw_symbol(Symbol::new("PF_XBTUSD"))
                .base_currency(Currency::BTC())
                .quote_currency(Currency::USD())
                .settlement_currency(Currency::USD())
                .is_inverse(false)
                .price_precision(0)
                .size_precision(4)
                .price_increment(Price::from("1"))
                .size_increment(Quantity::from("0.0001"))
                .ts_event(0.into())
                .ts_init(0.into())
                .build()
                .unwrap(),
        ));

        instrument_id
    }
}
