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

//! HTTP client for the Kraken Futures REST API.

use std::{
    collections::HashMap,
    fmt::{Debug, Formatter},
    num::NonZeroU32,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use nautilus_core::{
    AtomicTime, UUID4, consts::NAUTILUS_USER_AGENT, nanos::UnixNanos,
    time::get_atomic_clock_realtime,
};
use nautilus_model::{
    data::{Bar, BarType, TradeTick},
    enums::{AccountType, CurrencyType, OrderSide, OrderType, TimeInForce},
    events::AccountState,
    identifiers::{AccountId, ClientOrderId, InstrumentId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, Money, Price, Quantity},
};
use nautilus_network::{
    http::{HttpClient, Method, USER_AGENT},
    ratelimiter::quota::Quota,
    retry::{RetryConfig, RetryManager},
};
use serde::de::DeserializeOwned;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{models::*, query::*};
use crate::{
    common::{
        consts::NAUTILUS_KRAKEN_BROKER_ID,
        credential::KrakenCredential,
        enums::{
            KrakenApiResult, KrakenEnvironment, KrakenFuturesOrderType, KrakenOrderSide,
            KrakenProductType, KrakenSendStatus,
        },
        parse::{
            bar_type_to_futures_resolution, parse_bar, parse_futures_fill_report,
            parse_futures_instrument, parse_futures_order_event_status_report,
            parse_futures_order_status_report, parse_futures_position_status_report,
            parse_futures_public_execution,
        },
        urls::get_kraken_http_base_url,
    },
    http::{error::KrakenHttpError, models::OhlcData},
};

/// Default Kraken Futures REST API rate limit (requests per second).
pub const KRAKEN_FUTURES_DEFAULT_RATE_LIMIT_PER_SECOND: u32 = 5;

const KRAKEN_GLOBAL_RATE_KEY: &str = "kraken:futures:global";

/// Maximum orders per batch cancel request for Kraken Futures API.
const BATCH_CANCEL_LIMIT: usize = 50;

/// Raw HTTP client for low-level Kraken Futures API operations.
///
/// This client handles request/response operations with the Kraken Futures API,
/// returning venue-specific response types. It does not parse to Nautilus domain types.
pub struct KrakenFuturesRawHttpClient {
    base_url: String,
    client: HttpClient,
    credential: Option<KrakenCredential>,
    retry_manager: RetryManager<KrakenHttpError>,
    cancellation_token: CancellationToken,
    clock: &'static AtomicTime,
    /// Mutex to serialize authenticated requests, ensuring nonces arrive at Kraken in order
    auth_mutex: tokio::sync::Mutex<()>,
}

impl Default for KrakenFuturesRawHttpClient {
    fn default() -> Self {
        Self::new(
            KrakenEnvironment::Mainnet,
            None,
            Some(60),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("Failed to create default KrakenFuturesRawHttpClient")
    }
}

impl Debug for KrakenFuturesRawHttpClient {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(KrakenFuturesRawHttpClient))
            .field("base_url", &self.base_url)
            .field("has_credentials", &self.credential.is_some())
            .finish()
    }
}

impl KrakenFuturesRawHttpClient {
    /// Creates a new [`KrakenFuturesRawHttpClient`].
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        environment: KrakenEnvironment,
        base_url_override: Option<String>,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        proxy_url: Option<String>,
        max_requests_per_second: Option<u32>,
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

        let rate_limit =
            max_requests_per_second.unwrap_or(KRAKEN_FUTURES_DEFAULT_RATE_LIMIT_PER_SECOND);

        Ok(Self {
            base_url,
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                Self::rate_limiter_quotas(rate_limit),
                Some(Self::default_quota(rate_limit)),
                timeout_secs,
                proxy_url,
            )
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {e}"))?,
            credential: None,
            retry_manager,
            cancellation_token: CancellationToken::new(),
            clock: get_atomic_clock_realtime(),
            auth_mutex: tokio::sync::Mutex::new(()),
        })
    }

    /// Creates a new [`KrakenFuturesRawHttpClient`] with credentials.
    #[allow(clippy::too_many_arguments)]
    pub fn with_credentials(
        api_key: String,
        api_secret: String,
        environment: KrakenEnvironment,
        base_url_override: Option<String>,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        proxy_url: Option<String>,
        max_requests_per_second: Option<u32>,
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

        let rate_limit =
            max_requests_per_second.unwrap_or(KRAKEN_FUTURES_DEFAULT_RATE_LIMIT_PER_SECOND);

        Ok(Self {
            base_url,
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                Self::rate_limiter_quotas(rate_limit),
                Some(Self::default_quota(rate_limit)),
                timeout_secs,
                proxy_url,
            )
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {e}"))?,
            credential: Some(KrakenCredential::new(api_key, api_secret)),
            retry_manager,
            cancellation_token: CancellationToken::new(),
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
        &self.base_url
    }

    /// Returns the credential for this client, if set.
    pub fn credential(&self) -> Option<&KrakenCredential> {
        self.credential.as_ref()
    }

    /// Cancels all pending HTTP requests.
    pub fn cancel_all_requests(&self) {
        self.cancellation_token.cancel();
    }

    /// Returns the cancellation token for this client.
    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.cancellation_token
    }

    fn default_headers() -> HashMap<String, String> {
        HashMap::from([(USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string())])
    }

    fn default_quota(max_requests_per_second: u32) -> Quota {
        Quota::per_second(NonZeroU32::new(max_requests_per_second).unwrap_or_else(|| {
            NonZeroU32::new(KRAKEN_FUTURES_DEFAULT_RATE_LIMIT_PER_SECOND).unwrap()
        }))
    }

    fn rate_limiter_quotas(max_requests_per_second: u32) -> Vec<(String, Quota)> {
        vec![(
            KRAKEN_GLOBAL_RATE_KEY.to_string(),
            Self::default_quota(max_requests_per_second),
        )]
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

                    let base_url = &self.base_url;
                    tracing::debug!(
                        "Kraken Futures auth: endpoint={endpoint}, nonce={nonce}, base_url={base_url}"
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
                    .map_err(|e| KrakenHttpError::NetworkError(e.to_string()))?;

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

        let should_retry =
            |error: &KrakenHttpError| -> bool { matches!(error, KrakenHttpError::NetworkError(_)) };
        let create_error = |msg: String| -> KrakenHttpError { KrakenHttpError::NetworkError(msg) };

        self.retry_manager
            .execute_with_retry_with_cancel(
                &endpoint,
                operation,
                should_retry,
                create_error,
                &self.cancellation_token,
            )
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
        let _guard = self.auth_mutex.lock().await;

        if self.cancellation_token.is_cancelled() {
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

        tracing::debug!(
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
            .map_err(|e| KrakenHttpError::NetworkError(e.to_string()))?;

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

        let response_text = String::from_utf8(response.body.to_vec()).map_err(|e| {
            KrakenHttpError::ParseError(format!("Failed to parse response as UTF-8: {e}"))
        })?;

        serde_json::from_str(&response_text).map_err(|e| {
            KrakenHttpError::ParseError(format!("Failed to deserialize futures response: {e}"))
        })
    }

    async fn send_request_with_body<T: DeserializeOwned>(
        &self,
        endpoint: &str,
        params: HashMap<String, String>,
    ) -> anyhow::Result<T, KrakenHttpError> {
        let post_data = serde_urlencoded::to_string(&params)
            .map_err(|e| KrakenHttpError::ParseError(format!("Failed to encode params: {e}")))?;
        self.send_authenticated_post(endpoint, post_data).await
    }

    /// Sends a request with typed parameters (serializable struct).
    async fn send_request_with_params<P: serde::Serialize, T: DeserializeOwned>(
        &self,
        endpoint: &str,
        params: &P,
    ) -> anyhow::Result<T, KrakenHttpError> {
        let post_data = serde_urlencoded::to_string(params)
            .map_err(|e| KrakenHttpError::ParseError(format!("Failed to encode params: {e}")))?;
        self.send_authenticated_post(endpoint, post_data).await
    }

    /// Core authenticated POST request - takes raw post_data string.
    async fn send_authenticated_post<T: DeserializeOwned>(
        &self,
        endpoint: &str,
        post_data: String,
    ) -> anyhow::Result<T, KrakenHttpError> {
        if self.cancellation_token.is_cancelled() {
            return Err(KrakenHttpError::NetworkError(
                "Request cancelled".to_string(),
            ));
        }

        // Serialize authenticated requests to ensure nonces arrive at Kraken in order
        let _guard = self.auth_mutex.lock().await;

        if self.cancellation_token.is_cancelled() {
            return Err(KrakenHttpError::NetworkError(
                "Request cancelled".to_string(),
            ));
        }

        let credential = self.credential.as_ref().ok_or_else(|| {
            KrakenHttpError::AuthenticationError("Missing credentials".to_string())
        })?;

        let nonce = self.generate_nonce();
        tracing::debug!("Generated nonce {nonce} for {endpoint}");

        let signature = credential
            .sign_futures(endpoint, &post_data, nonce)
            .map_err(|e| {
                KrakenHttpError::AuthenticationError(format!("Failed to sign request: {e}"))
            })?;

        let url = format!("{}{endpoint}", self.base_url);
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
            .client
            .request(
                Method::POST,
                url,
                None,
                Some(headers),
                Some(post_data.into_bytes()),
                None,
                Some(rate_limit_keys),
            )
            .await
            .map_err(|e| KrakenHttpError::NetworkError(e.to_string()))?;

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
            tracing::error!("Failed to parse response from {endpoint}: {response_text}");
            KrakenHttpError::ParseError(format!("Failed to deserialize response: {e}"))
        })
    }

    /// Requests tradable instruments from Kraken Futures.
    pub async fn get_instruments(
        &self,
    ) -> anyhow::Result<FuturesInstrumentsResponse, KrakenHttpError> {
        let endpoint = "/derivatives/api/v3/instruments";
        let url = format!("{}{endpoint}", self.base_url);

        self.send_request(Method::GET, endpoint, url, false).await
    }

    /// Requests ticker information for all futures instruments.
    pub async fn get_tickers(&self) -> anyhow::Result<FuturesTickersResponse, KrakenHttpError> {
        let endpoint = "/derivatives/api/v3/tickers";
        let url = format!("{}{endpoint}", self.base_url);

        self.send_request(Method::GET, endpoint, url, false).await
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

        let mut url = format!("{}{endpoint}", self.base_url);

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

        let mut url = format!("{}{endpoint}", self.base_url);

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
        let url = format!("{}{endpoint}", self.base_url);

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

        let endpoint = "/api/history/v2/orders";
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

        // Build URL with query params
        let query_string = query_params.join("&");
        let url = if query_string.is_empty() {
            format!("{}{endpoint}", self.base_url)
        } else {
            format!("{}{endpoint}?{query_string}", self.base_url)
        };

        // For signing: query params go in postData, not endpoint
        // Kraken: message = postData + nonce + endpoint
        self.send_get_with_query(endpoint, url, &query_string).await
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
            format!("{}{endpoint}", self.base_url)
        } else {
            format!("{}{endpoint}?{query_string}", self.base_url)
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
        let url = format!("{}{endpoint}", self.base_url);

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
        let url = format!("{}{endpoint}", self.base_url);

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
            return Err(KrakenHttpError::AuthenticationError(
                "API credentials required for sending orders".to_string(),
            ));
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
            return Err(KrakenHttpError::AuthenticationError(
                "API credentials required for canceling orders".to_string(),
            ));
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
            return Err(KrakenHttpError::AuthenticationError(
                "API credentials required for editing orders".to_string(),
            ));
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
        if self.credential.is_none() {
            return Err(KrakenHttpError::AuthenticationError(
                "API credentials required for batch orders".to_string(),
            ));
        }

        let batch_items: Vec<KrakenFuturesBatchCancelItem> = order_ids
            .into_iter()
            .map(KrakenFuturesBatchCancelItem::from_order_id)
            .collect();

        let params = KrakenFuturesBatchOrderParams::new(batch_items);
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
            return Err(KrakenHttpError::AuthenticationError(
                "API credentials required for canceling orders".to_string(),
            ));
        }

        let mut params = HashMap::new();
        if let Some(sym) = symbol {
            params.insert("symbol".to_string(), sym);
        }

        let endpoint = "/derivatives/api/v3/cancelallorders";
        self.send_request_with_body(endpoint, params).await
    }
}

// =============================================================================
// Domain Client
// =============================================================================

/// High-level HTTP client for the Kraken Futures REST API.
///
/// This client wraps the raw client and provides Nautilus domain types.
/// It maintains an instrument cache and uses it to parse venue responses
/// into Nautilus domain objects.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.kraken")
)]
pub struct KrakenFuturesHttpClient {
    pub(crate) inner: Arc<KrakenFuturesRawHttpClient>,
    pub(crate) instruments_cache: Arc<DashMap<Ustr, InstrumentAny>>,
    cache_initialized: Arc<AtomicBool>,
}

impl Clone for KrakenFuturesHttpClient {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            instruments_cache: self.instruments_cache.clone(),
            cache_initialized: self.cache_initialized.clone(),
        }
    }
}

impl Default for KrakenFuturesHttpClient {
    fn default() -> Self {
        Self::new(
            KrakenEnvironment::Mainnet,
            None,
            Some(60),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("Failed to create default KrakenFuturesHttpClient")
    }
}

impl Debug for KrakenFuturesHttpClient {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(KrakenFuturesHttpClient))
            .field("inner", &self.inner)
            .finish()
    }
}

impl KrakenFuturesHttpClient {
    /// Creates a new [`KrakenFuturesHttpClient`].
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        environment: KrakenEnvironment,
        base_url_override: Option<String>,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        proxy_url: Option<String>,
        max_requests_per_second: Option<u32>,
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
            instruments_cache: Arc::new(DashMap::new()),
            cache_initialized: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Creates a new [`KrakenFuturesHttpClient`] with credentials.
    #[allow(clippy::too_many_arguments)]
    pub fn with_credentials(
        api_key: String,
        api_secret: String,
        environment: KrakenEnvironment,
        base_url_override: Option<String>,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        proxy_url: Option<String>,
        max_requests_per_second: Option<u32>,
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
            instruments_cache: Arc::new(DashMap::new()),
            cache_initialized: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Creates a new [`KrakenFuturesHttpClient`] loading credentials from environment variables.
    ///
    /// Looks for `KRAKEN_FUTURES_API_KEY` and `KRAKEN_FUTURES_API_SECRET` (mainnet)
    /// or `KRAKEN_FUTURES_DEMO_API_KEY` and `KRAKEN_FUTURES_DEMO_API_SECRET` (demo).
    ///
    /// Falls back to unauthenticated client if credentials are not set.
    #[allow(clippy::too_many_arguments)]
    pub fn from_env(
        environment: KrakenEnvironment,
        base_url_override: Option<String>,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        proxy_url: Option<String>,
        max_requests_per_second: Option<u32>,
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

    /// Returns the cancellation token for this client.
    pub fn cancellation_token(&self) -> &CancellationToken {
        self.inner.cancellation_token()
    }

    /// Caches an instrument for symbol lookup.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        self.instruments_cache
            .insert(instrument.symbol().inner(), instrument);
        self.cache_initialized.store(true, Ordering::Release);
    }

    /// Caches multiple instruments for symbol lookup.
    pub fn cache_instruments(&self, instruments: Vec<InstrumentAny>) {
        for instrument in instruments {
            self.instruments_cache
                .insert(instrument.symbol().inner(), instrument);
        }
        self.cache_initialized.store(true, Ordering::Release);
    }

    /// Gets an instrument from the cache by symbol.
    pub fn get_cached_instrument(&self, symbol: &Ustr) -> Option<InstrumentAny> {
        self.instruments_cache
            .get(symbol)
            .map(|entry| entry.value().clone())
    }

    fn get_instrument_by_raw_symbol(&self, raw_symbol: &str) -> Option<InstrumentAny> {
        self.instruments_cache
            .iter()
            .find(|entry| entry.value().raw_symbol().as_str() == raw_symbol)
            .map(|entry| entry.value().clone())
    }

    fn generate_ts_init(&self) -> UnixNanos {
        get_atomic_clock_realtime().get_time_ns()
    }

    /// Requests tradable instruments from Kraken Futures.
    pub async fn request_instruments(&self) -> anyhow::Result<Vec<InstrumentAny>, KrakenHttpError> {
        let ts_init = self.generate_ts_init();
        let response = self.inner.get_instruments().await?;

        let instruments: Vec<InstrumentAny> = response
            .instruments
            .iter()
            .filter_map(|fut_instrument| {
                match parse_futures_instrument(fut_instrument, ts_init, ts_init) {
                    Ok(instrument) => Some(instrument),
                    Err(e) => {
                        let symbol = &fut_instrument.symbol;
                        tracing::warn!("Failed to parse futures instrument {symbol}: {e}");
                        None
                    }
                }
            })
            .collect();

        Ok(instruments)
    }

    /// Requests the mark price for an instrument.
    pub async fn request_mark_price(
        &self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<f64, KrakenHttpError> {
        let instrument = self
            .get_cached_instrument(&instrument_id.symbol.inner())
            .ok_or_else(|| {
                KrakenHttpError::ParseError(format!(
                    "Instrument not found in cache: {instrument_id}"
                ))
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
    ) -> anyhow::Result<f64, KrakenHttpError> {
        let instrument = self
            .get_cached_instrument(&instrument_id.symbol.inner())
            .ok_or_else(|| {
                KrakenHttpError::ParseError(format!(
                    "Instrument not found in cache: {instrument_id}"
                ))
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
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u64>,
    ) -> anyhow::Result<Vec<TradeTick>, KrakenHttpError> {
        let instrument = self
            .get_cached_instrument(&instrument_id.symbol.inner())
            .ok_or_else(|| {
                KrakenHttpError::ParseError(format!(
                    "Instrument not found in cache: {instrument_id}"
                ))
            })?;

        let raw_symbol = instrument.raw_symbol().to_string();
        let ts_init = self.generate_ts_init();

        let since = start.map(|dt| dt.timestamp_millis());
        let before = end.map(|dt| dt.timestamp_millis());

        let response = self
            .inner
            .get_public_executions(&raw_symbol, since, before, Some("asc"), None)
            .await?;

        let mut trades = Vec::new();

        for element in &response.elements {
            let execution = &element.event.execution.execution;
            match parse_futures_public_execution(execution, &instrument, ts_init) {
                Ok(trade_tick) => {
                    trades.push(trade_tick);

                    if let Some(limit_count) = limit
                        && trades.len() >= limit_count as usize
                    {
                        return Ok(trades);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to parse futures trade tick: {e}");
                }
            }
        }

        Ok(trades)
    }

    pub async fn request_bars(
        &self,
        bar_type: BarType,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u64>,
    ) -> anyhow::Result<Vec<Bar>, KrakenHttpError> {
        let instrument_id = bar_type.instrument_id();
        let instrument = self
            .get_cached_instrument(&instrument_id.symbol.inner())
            .ok_or_else(|| {
                KrakenHttpError::ParseError(format!(
                    "Instrument not found in cache: {instrument_id}"
                ))
            })?;

        let raw_symbol = instrument.raw_symbol().to_string();
        let ts_init = self.generate_ts_init();
        let tick_type = "trade";
        let resolution = bar_type_to_futures_resolution(bar_type)
            .map_err(|e| KrakenHttpError::ParseError(e.to_string()))?;

        // Kraken Futures OHLC API expects Unix timestamp in seconds
        let from = start.map(|dt| dt.timestamp());
        let to = end.map(|dt| dt.timestamp());
        let end_ns = end.map(|dt| dt.timestamp_nanos_opt().unwrap_or(0) as u64);

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

                    if let Some(limit_count) = limit
                        && bars.len() >= limit_count as usize
                    {
                        return Ok(bars);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to parse futures bar: {e}");
                }
            }
        }

        Ok(bars)
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

        let mut balances: Vec<AccountBalance> = Vec::new();

        for account in accounts_response.accounts.values() {
            match account.account_type.as_str() {
                "multiCollateralMarginAccount" => {
                    for (currency_code, currency_info) in &account.currencies {
                        if currency_info.quantity == 0.0 {
                            continue;
                        }

                        let currency = Currency::new(
                            currency_code.as_str(),
                            8,
                            0,
                            currency_code.as_str(),
                            CurrencyType::Crypto,
                        );

                        let total_amount = currency_info.quantity;
                        let total = Money::new(total_amount, currency);

                        // Available can exceed quantity with positive PnL, cap to satisfy invariant
                        let available_amount = currency_info
                            .available
                            .unwrap_or(total_amount)
                            .min(total_amount);
                        let locked_amount = (total_amount - available_amount).max(0.0);
                        let locked = Money::new(locked_amount, currency);
                        // Compute free from total - locked to guarantee the invariant holds
                        let free = total - locked;

                        balances.push(AccountBalance::new(total, locked, free));
                    }

                    // Add USD balance from portfolio value for margin calculations.
                    // Multi-collateral accounts track margin in USD even though the
                    // actual collateral is held in various crypto currencies.
                    if let Some(portfolio_value) = account.portfolio_value
                        && portfolio_value > 0.0
                    {
                        let usd_currency = Currency::USD();
                        let total_usd = Money::new(portfolio_value, usd_currency);
                        let available_usd = account
                            .available_margin
                            .unwrap_or(portfolio_value)
                            .min(portfolio_value);
                        // Compute locked = total - available to guarantee the invariant holds
                        let locked_usd =
                            Money::new((portfolio_value - available_usd).max(0.0), usd_currency);
                        let free_usd = total_usd - locked_usd;

                        balances.push(AccountBalance::new(total_usd, locked_usd, free_usd));
                    }
                }
                "marginAccount" => {
                    for (currency_code, &amount) in &account.balances {
                        if amount == 0.0 {
                            continue;
                        }

                        let currency = Currency::new(
                            currency_code.as_str(),
                            8,
                            0,
                            currency_code.as_str(),
                            CurrencyType::Crypto,
                        );

                        let total = Money::new(amount, currency);

                        // Available can exceed balance with positive PnL, cap to satisfy invariant
                        let available = account
                            .auxiliary
                            .as_ref()
                            .and_then(|aux| aux.af)
                            .unwrap_or(amount)
                            .min(amount);
                        let locked = amount - available;

                        balances.push(AccountBalance::new(
                            total,
                            Money::new(locked, currency),
                            Money::new(available, currency),
                        ));
                    }
                }
                "cashAccount" => {
                    for (currency_code, &amount) in &account.balances {
                        if amount == 0.0 {
                            continue;
                        }

                        let currency = Currency::new(
                            currency_code.as_str(),
                            8,
                            0,
                            currency_code.as_str(),
                            CurrencyType::Crypto,
                        );

                        let total = Money::new(amount, currency);
                        let locked = Money::new(0.0, currency);

                        balances.push(AccountBalance::new(total, locked, total));
                    }
                }
                _ => {
                    let account_type = &account.account_type;
                    tracing::debug!("Unknown account type: {account_type}");
                }
            }
        }

        Ok(AccountState::new(
            account_id,
            AccountType::Margin,
            balances,
            vec![],
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
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        open_only: bool,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
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

        for order in &response.open_orders {
            if let Some(ref target_id) = instrument_id {
                let instrument = self.get_cached_instrument(&target_id.symbol.inner());
                if let Some(inst) = instrument
                    && inst.raw_symbol().as_str() != order.symbol
                {
                    continue;
                }
            }

            if let Some(instrument) = self.get_instrument_by_raw_symbol(&order.symbol) {
                match parse_futures_order_status_report(order, &instrument, account_id, ts_init) {
                    Ok(report) => all_reports.push(report),
                    Err(e) => {
                        let order_id = &order.order_id;
                        tracing::warn!("Failed to parse futures order {order_id}: {e}");
                    }
                }
            }
        }

        if !open_only {
            // Kraken Futures order events API expects Unix timestamp in milliseconds
            let start_ms = start.map(|dt| dt.timestamp_millis());
            let end_ms = end.map(|dt| dt.timestamp_millis());
            let response = self
                .inner
                .get_order_events(end_ms, start_ms, None)
                .await
                .map_err(|e| anyhow::anyhow!("get_order_events failed: {e}"))?;

            for event_wrapper in response.order_events {
                let event = &event_wrapper.order;
                if let Some(ref target_id) = instrument_id {
                    let instrument = self.get_cached_instrument(&target_id.symbol.inner());
                    if let Some(inst) = instrument
                        && inst.raw_symbol().as_str() != event.symbol
                    {
                        continue;
                    }
                }

                if let Some(instrument) = self.get_instrument_by_raw_symbol(&event.symbol) {
                    match parse_futures_order_event_status_report(
                        event,
                        &instrument,
                        account_id,
                        ts_init,
                    ) {
                        Ok(report) => all_reports.push(report),
                        Err(e) => {
                            let order_id = &event.order_id;
                            tracing::warn!("Failed to parse futures order event {order_id}: {e}");
                        }
                    }
                }
            }
        }

        Ok(all_reports)
    }

    pub async fn request_fill_reports(
        &self,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
    ) -> anyhow::Result<Vec<FillReport>> {
        let ts_init = self.generate_ts_init();
        let mut all_reports = Vec::new();

        let response = self.inner.get_fills(None).await?;
        if response.result != KrakenApiResult::Success {
            let error_msg = response
                .error
                .unwrap_or_else(|| "Unknown error".to_string());
            anyhow::bail!("Failed to get fills: {error_msg}");
        }

        let start_ms = start.map(|dt| dt.timestamp_millis());
        let end_ms = end.map(|dt| dt.timestamp_millis());

        for fill in response.fills {
            if let Some(start_threshold) = start_ms
                && let Ok(fill_ts) = DateTime::parse_from_rfc3339(&fill.fill_time)
            {
                let fill_ms = fill_ts.timestamp_millis();
                if fill_ms < start_threshold {
                    continue;
                }
            }
            if let Some(end_threshold) = end_ms
                && let Ok(fill_ts) = DateTime::parse_from_rfc3339(&fill.fill_time)
            {
                let fill_ms = fill_ts.timestamp_millis();
                if fill_ms > end_threshold {
                    continue;
                }
            }

            if let Some(ref target_id) = instrument_id {
                let instrument = self.get_cached_instrument(&target_id.symbol.inner());
                if let Some(inst) = instrument
                    && inst.raw_symbol().as_str() != fill.symbol
                {
                    continue;
                }
            }

            if let Some(instrument) = self.get_instrument_by_raw_symbol(&fill.symbol) {
                match parse_futures_fill_report(&fill, &instrument, account_id, ts_init) {
                    Ok(report) => all_reports.push(report),
                    Err(e) => {
                        let fill_id = &fill.fill_id;
                        tracing::warn!("Failed to parse futures fill {fill_id}: {e}");
                    }
                }
            }
        }

        Ok(all_reports)
    }

    pub async fn request_position_status_reports(
        &self,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let ts_init = self.generate_ts_init();
        let mut all_reports = Vec::new();

        let response = self.inner.get_open_positions().await?;
        if response.result != KrakenApiResult::Success {
            let error_msg = response
                .error
                .unwrap_or_else(|| "Unknown error".to_string());
            anyhow::bail!("Failed to get open positions: {error_msg}");
        }

        for position in response.open_positions {
            if let Some(ref target_id) = instrument_id {
                let instrument = self.get_cached_instrument(&target_id.symbol.inner());
                if let Some(inst) = instrument
                    && inst.raw_symbol().as_str() != position.symbol
                {
                    continue;
                }
            }

            if let Some(instrument) = self.get_instrument_by_raw_symbol(&position.symbol) {
                match parse_futures_position_status_report(
                    &position,
                    &instrument,
                    account_id,
                    ts_init,
                ) {
                    Ok(report) => all_reports.push(report),
                    Err(e) => {
                        let symbol = &position.symbol;
                        tracing::warn!("Failed to parse futures position {symbol}: {e}");
                    }
                }
            }
        }

        Ok(all_reports)
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
    #[allow(clippy::too_many_arguments)]
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
        reduce_only: bool,
        post_only: bool,
    ) -> anyhow::Result<OrderStatusReport> {
        let instrument = self
            .get_cached_instrument(&instrument_id.symbol.inner())
            .ok_or_else(|| anyhow::anyhow!("Instrument not found in cache: {instrument_id}"))?;

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
            OrderType::MarketIfTouched => KrakenFuturesOrderType::TakeProfit,
            _ => anyhow::bail!("Unsupported order type: {order_type:?}"),
        };

        let mut builder = KrakenFuturesSendOrderParamsBuilder::default();
        builder
            .cli_ord_id(client_order_id.to_string())
            .broker(NAUTILUS_KRAKEN_BROKER_ID)
            .symbol(raw_symbol)
            .side(KrakenOrderSide::from(order_side))
            .size(quantity.to_string())
            .order_type(kraken_order_type);

        // Handle prices based on order type
        match order_type {
            OrderType::StopMarket => {
                // Stop market orders need stop_price (trigger price)
                if let Some(trigger) = trigger_price {
                    builder.stop_price(trigger.to_string());
                }
            }
            OrderType::StopLimit => {
                // Stop limit orders need both stop_price and limit_price
                if let Some(trigger) = trigger_price {
                    builder.stop_price(trigger.to_string());
                }
                if let Some(limit) = price {
                    builder.limit_price(limit.to_string());
                }
            }
            OrderType::MarketIfTouched => {
                // Take-profit orders need stop_price (trigger price) and optionally limit_price
                if let Some(trigger) = trigger_price {
                    builder.stop_price(trigger.to_string());
                }
                if let Some(limit) = price {
                    builder.limit_price(limit.to_string());
                }
            }
            _ => {
                // Regular orders just use limit_price
                if let Some(limit) = price {
                    builder.limit_price(limit.to_string());
                }
            }
        }

        if reduce_only {
            builder.reduce_only(true);
        }

        let params = builder
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build order params: {e}"))?;

        let response = self.inner.send_order_params(&params).await?;

        if response.result != KrakenApiResult::Success {
            let error_msg = response
                .error
                .unwrap_or_else(|| "Unknown error".to_string());
            anyhow::bail!("Order submission failed: {error_msg}");
        }

        let send_status = response
            .send_status
            .ok_or_else(|| anyhow::anyhow!("No send_status in successful response"))?;

        let status = &send_status.status;

        // Check for post-only rejection (Kraken returns status="postWouldExecute")
        if status == "postWouldExecute" {
            let reason = send_status
                .order_events
                .as_ref()
                .and_then(|events| events.first())
                .and_then(|e| e.reason.clone())
                .unwrap_or_else(|| "Post-only order would have crossed".to_string());
            anyhow::bail!("POST_ONLY_REJECTED: {reason}");
        }

        let venue_order_id = send_status
            .order_id
            .ok_or_else(|| anyhow::anyhow!("No order_id in send_status: {status}"))?;

        let ts_init = self.generate_ts_init();

        let open_orders_response = self.inner.get_open_orders().await?;
        if let Some(order) = open_orders_response
            .open_orders
            .iter()
            .find(|o| o.order_id == venue_order_id)
        {
            return parse_futures_order_status_report(order, &instrument, account_id, ts_init);
        }

        // Order not in open orders - may have filled immediately (market order or aggressive limit)
        // Try to use order_events from send_status first
        if let Some(order_events) = &send_status.order_events
            && let Some(send_event) = order_events.first()
        {
            // Handle regular orders, trigger orders, and execution events
            let event = if let Some(order_data) = &send_event.order {
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
                }
            } else if let Some(trigger_data) = &send_event.order_trigger {
                FuturesOrderEvent {
                    order_id: trigger_data.uid.clone(),
                    cli_ord_id: trigger_data.client_id.clone(),
                    order_type: trigger_data.order_type,
                    symbol: trigger_data.symbol.clone(),
                    side: trigger_data.side,
                    quantity: trigger_data.quantity,
                    filled: 0.0,
                    limit_price: trigger_data.limit_price,
                    stop_price: Some(trigger_data.trigger_price),
                    timestamp: trigger_data.timestamp.clone(),
                    last_update_timestamp: trigger_data.last_update_timestamp.clone(),
                    reduce_only: trigger_data.reduce_only,
                }
            } else if let Some(prior_exec) = &send_event.order_prior_execution {
                // EXECUTION event - use orderPriorExecution data
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
                }
            } else {
                anyhow::bail!("No order, orderTrigger, or orderPriorExecution data in event");
            };
            return parse_futures_order_event_status_report(
                &event,
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

        parse_futures_order_event_status_report(
            &event_wrapper.order,
            &instrument,
            account_id,
            ts_init,
        )
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
    pub async fn modify_order(
        &self,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
        quantity: Option<Quantity>,
        price: Option<Price>,
        trigger_price: Option<Price>,
    ) -> anyhow::Result<VenueOrderId> {
        let _ = self
            .get_cached_instrument(&instrument_id.symbol.inner())
            .ok_or_else(|| anyhow::anyhow!("Instrument not found in cache: {instrument_id}"))?;

        let order_id = venue_order_id.as_ref().map(|id| id.to_string());
        let cli_ord_id = client_order_id.as_ref().map(|id| id.to_string());

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
        if let Some(qty) = quantity {
            builder.size(qty.to_string());
        }
        if let Some(p) = price {
            builder.limit_price(p.to_string());
        }
        if let Some(tp) = trigger_price {
            builder.stop_price(tp.to_string());
        }

        let params = builder
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build edit order params: {e}"))?;

        let response = self.inner.edit_order(&params).await?;

        if response.result != KrakenApiResult::Success {
            let status = &response.edit_status.status;
            anyhow::bail!("Order modification failed: {status}");
        }

        // Return the new order_id from the response, or fall back to the original
        let new_venue_order_id = response
            .edit_status
            .order_id
            .or(order_id)
            .ok_or_else(|| anyhow::anyhow!("No order ID in edit order response"))?;

        Ok(VenueOrderId::new(&new_venue_order_id))
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
            .ok_or_else(|| anyhow::anyhow!("Instrument not found in cache: {instrument_id}"))?;

        let order_id = venue_order_id.as_ref().map(|id| id.to_string());
        let cli_ord_id = client_order_id.as_ref().map(|id| id.to_string());

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
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

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
            KrakenEnvironment::Mainnet,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert!(client.credential.is_some());
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
            KrakenEnvironment::Mainnet,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .unwrap();
        assert!(client.instruments_cache.is_empty());
    }
}
