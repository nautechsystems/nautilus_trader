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

//! Provides an ergonomic wrapper around the **dYdX v4 Indexer REST API** –
//! <https://docs.dydx.exchange/api_integration-indexer/indexer_api>.
//!
//! This module exports two complementary HTTP clients following the standardized
//! two-layer architecture pattern established in OKX, Bybit, and BitMEX adapters:
//!
//! - [`DydxRawHttpClient`]: Low-level HTTP methods matching dYdX Indexer API endpoints.
//! - [`DydxHttpClient`]: High-level methods using Nautilus domain types with instrument caching.
//!
//! ## Two-Layer Architecture
//!
//! The raw client handles HTTP communication, rate limiting, retries, and basic response parsing.
//! The domain client wraps the raw client in an `Arc`, maintains an instrument cache using `DashMap`,
//! and provides high-level methods that work with Nautilus domain types.
//!
//! ## Key Responsibilities
//!
//! - Rate-limiting based on the public dYdX specification.
//! - Zero-copy deserialization of large JSON payloads into domain models.
//! - Conversion of raw exchange errors into the rich [`DydxHttpError`] enum.
//! - Instrument caching with standard methods: `cache_instruments()`, `cache_instrument()`, `get_instrument()`.
//!
//! # Important Note
//!
//! The dYdX v4 Indexer REST API does **NOT** require authentication or request signing.
//! All endpoints are publicly accessible using only wallet addresses and subaccount numbers
//! as query parameters. Order submission and trading operations use gRPC with blockchain
//! transaction signing, not REST API.
//!
//! # Official documentation
//!
//! | Endpoint                             | Reference                                              |
//! |--------------------------------------|--------------------------------------------------------|
//! | Market data                          | <https://docs.dydx.exchange/api_integration-indexer/indexer_api#markets> |
//! | Account data                         | <https://docs.dydx.exchange/api_integration-indexer/indexer_api#accounts> |
//! | Utility endpoints                    | <https://docs.dydx.exchange/api_integration-indexer/indexer_api#utility> |

use std::{
    collections::HashMap,
    fmt::{Debug, Formatter},
    num::NonZeroU32,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicBool, Ordering},
    },
};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use nautilus_core::consts::NAUTILUS_USER_AGENT;
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{
    http::HttpClient,
    ratelimiter::quota::Quota,
    retry::{RetryConfig, RetryManager},
};
use reqwest::{Method, header::USER_AGENT};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::error::DydxHttpError;
use crate::common::{
    consts::{DYDX_HTTP_URL, DYDX_TESTNET_HTTP_URL},
    enums::DydxCandleResolution,
};

/// Default dYdX Indexer REST API rate limit.
///
/// The dYdX Indexer API rate limits are generous for read-only operations:
/// - General: 100 requests per 10 seconds per IP
/// - We use a conservative 10 requests per second as the default quota.
pub static DYDX_REST_QUOTA: LazyLock<Quota> =
    LazyLock::new(|| Quota::per_second(NonZeroU32::new(10).unwrap()));

/// Represents a dYdX HTTP response wrapper.
///
/// Most dYdX Indexer API endpoints return data directly without a wrapper,
/// but some endpoints may use this structure for consistency.
#[derive(Debug, Serialize, Deserialize)]
pub struct DydxResponse<T> {
    /// The typed data returned by the dYdX endpoint.
    pub data: T,
}

/// Provides a raw HTTP client for interacting with the [dYdX v4](https://dydx.exchange) Indexer REST API.
///
/// This client wraps the underlying [`HttpClient`] to handle functionality
/// specific to dYdX Indexer API, such as rate-limiting, forming request URLs,
/// and deserializing responses into dYdX specific data models.
///
/// **Note**: Unlike traditional centralized exchanges, the dYdX v4 Indexer REST API
/// does NOT require authentication, API keys, or request signing. All endpoints are
/// publicly accessible.
pub struct DydxRawHttpClient {
    base_url: String,
    client: HttpClient,
    retry_manager: RetryManager<DydxHttpError>,
    cancellation_token: CancellationToken,
    is_testnet: bool,
}

impl Default for DydxRawHttpClient {
    fn default() -> Self {
        Self::new(None, Some(60), None, false, None)
            .expect("Failed to create default DydxRawHttpClient")
    }
}

impl Debug for DydxRawHttpClient {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(DydxRawHttpClient))
            .field("base_url", &self.base_url)
            .field("is_testnet", &self.is_testnet)
            .finish_non_exhaustive()
    }
}

impl DydxRawHttpClient {
    /// Cancel all pending HTTP requests.
    pub fn cancel_all_requests(&self) {
        self.cancellation_token.cancel();
    }

    /// Get the cancellation token for this client.
    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.cancellation_token
    }

    /// Creates a new [`DydxRawHttpClient`] using the default dYdX Indexer HTTP URL,
    /// optionally overridden with a custom base URL.
    ///
    /// **Note**: No credentials are required as the dYdX Indexer API is publicly accessible.
    ///
    /// # Errors
    ///
    /// Returns an error if the retry manager cannot be created.
    pub fn new(
        base_url: Option<String>,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
        is_testnet: bool,
        retry_config: Option<RetryConfig>,
    ) -> anyhow::Result<Self> {
        let base_url = if is_testnet {
            base_url.unwrap_or_else(|| DYDX_TESTNET_HTTP_URL.to_string())
        } else {
            base_url.unwrap_or_else(|| DYDX_HTTP_URL.to_string())
        };

        let retry_manager = RetryManager::new(retry_config.unwrap_or_default());

        // Build headers
        let mut headers = HashMap::new();
        headers.insert(USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string());

        let client = HttpClient::new(
            headers,
            vec![], // No specific headers to extract from responses
            vec![], // No keyed quotas (we use a single global quota)
            Some(*DYDX_REST_QUOTA),
            timeout_secs,
            proxy_url,
        )
        .map_err(|e| {
            DydxHttpError::ValidationError(format!("Failed to create HTTP client: {e}"))
        })?;

        Ok(Self {
            base_url,
            client,
            retry_manager,
            cancellation_token: CancellationToken::new(),
            is_testnet,
        })
    }

    /// Check if this client is configured for testnet.
    #[must_use]
    pub const fn is_testnet(&self) -> bool {
        self.is_testnet
    }

    /// Get the base URL being used by this client.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Send a request to a dYdX Indexer API endpoint.
    ///
    /// **Note**: dYdX Indexer API does not require authentication headers.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The HTTP request fails.
    /// - The response has a non-success HTTP status code.
    /// - The response body cannot be deserialized to type `T`.
    /// - The request is canceled.
    pub async fn send_request<T>(
        &self,
        method: Method,
        endpoint: &str,
        query_params: Option<&str>,
    ) -> Result<T, DydxHttpError>
    where
        T: DeserializeOwned,
    {
        let url = if let Some(params) = query_params {
            format!("{}{endpoint}?{params}", self.base_url)
        } else {
            format!("{}{endpoint}", self.base_url)
        };

        let operation = || async {
            let request = self
                .client
                .request_with_ustr_keys(
                    method.clone(),
                    url.clone(),
                    None, // No params
                    None, // No additional headers
                    None, // No body for GET requests
                    None, // Use default timeout
                    None, // No specific rate limit keys (using global quota)
                )
                .await
                .map_err(|e| DydxHttpError::HttpClientError(e.to_string()))?;

            // Check for HTTP errors
            if !request.status.is_success() {
                return Err(DydxHttpError::HttpStatus {
                    status: request.status.as_u16(),
                    message: String::from_utf8_lossy(&request.body).to_string(),
                });
            }

            Ok(request)
        };

        // Retry strategy for dYdX Indexer API:
        // 1. Network errors: always retry (transient connection issues)
        // 2. HTTP 429/5xx: rate limiting and server errors should be retried
        // 3. Client errors (4xx except 429): should NOT be retried
        let should_retry = |error: &DydxHttpError| -> bool {
            match error {
                DydxHttpError::HttpClientError(_) => true,
                DydxHttpError::HttpStatus { status, .. } => *status == 429 || *status >= 500,
                _ => false,
            }
        };

        let create_error = |msg: String| -> DydxHttpError {
            if msg == "canceled" {
                DydxHttpError::Canceled("Adapter disconnecting or shutting down".to_string())
            } else {
                DydxHttpError::ValidationError(msg)
            }
        };

        // Execute request with retry logic
        let response = self
            .retry_manager
            .execute_with_retry_with_cancel(
                endpoint,
                operation,
                should_retry,
                create_error,
                &self.cancellation_token,
            )
            .await?;

        // Deserialize response
        serde_json::from_slice(&response.body).map_err(|e| DydxHttpError::Deserialization {
            error: e.to_string(),
            body: String::from_utf8_lossy(&response.body).to_string(),
        })
    }

    /// Send a POST request to a dYdX Indexer API endpoint.
    ///
    /// Note: Most dYdX Indexer endpoints are GET-based. POST is rarely used.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The request body cannot be serialized to JSON.
    /// - The HTTP request fails.
    /// - The response has a non-success HTTP status code.
    /// - The response body cannot be deserialized to type `T`.
    /// - The request is canceled.
    pub async fn send_post_request<T, B>(
        &self,
        endpoint: &str,
        body: &B,
    ) -> Result<T, DydxHttpError>
    where
        T: DeserializeOwned,
        B: Serialize,
    {
        let url = format!("{}{endpoint}", self.base_url);

        let body_bytes = serde_json::to_vec(body).map_err(|e| DydxHttpError::Serialization {
            error: e.to_string(),
        })?;

        let operation = || async {
            let request = self
                .client
                .request_with_ustr_keys(
                    Method::POST,
                    url.clone(),
                    None, // No params
                    None, // No additional headers (content-type handled by body)
                    Some(body_bytes.clone()),
                    None, // Use default timeout
                    None, // No specific rate limit keys (using global quota)
                )
                .await
                .map_err(|e| DydxHttpError::HttpClientError(e.to_string()))?;

            // Check for HTTP errors
            if !request.status.is_success() {
                return Err(DydxHttpError::HttpStatus {
                    status: request.status.as_u16(),
                    message: String::from_utf8_lossy(&request.body).to_string(),
                });
            }

            Ok(request)
        };

        // Retry strategy (same as GET requests)
        let should_retry = |error: &DydxHttpError| -> bool {
            match error {
                DydxHttpError::HttpClientError(_) => true,
                DydxHttpError::HttpStatus { status, .. } => *status == 429 || *status >= 500,
                _ => false,
            }
        };

        let create_error = |msg: String| -> DydxHttpError {
            if msg == "canceled" {
                DydxHttpError::Canceled("Adapter disconnecting or shutting down".to_string())
            } else {
                DydxHttpError::ValidationError(msg)
            }
        };

        // Execute request with retry logic
        let response = self
            .retry_manager
            .execute_with_retry_with_cancel(
                endpoint,
                operation,
                should_retry,
                create_error,
                &self.cancellation_token,
            )
            .await?;

        // Deserialize response
        serde_json::from_slice(&response.body).map_err(|e| DydxHttpError::Deserialization {
            error: e.to_string(),
            body: String::from_utf8_lossy(&response.body).to_string(),
        })
    }

    // ========================================================================
    // Markets Endpoints
    // ========================================================================

    /// Fetch all perpetual markets from dYdX.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn get_markets(&self) -> Result<super::models::MarketsResponse, DydxHttpError> {
        self.send_request(Method::GET, "/v4/perpetualMarkets", None)
            .await
    }

    /// Fetch all instruments and parse them into Nautilus `InstrumentAny` types.
    ///
    /// This method fetches all perpetual markets from dYdX and converts them
    /// into Nautilus instrument definitions using the `parse_instrument_any` function.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The HTTP request fails.
    /// - The response cannot be parsed.
    /// - Any instrument parsing fails.
    ///
    pub async fn fetch_instruments(
        &self,
        maker_fee: Option<rust_decimal::Decimal>,
        taker_fee: Option<rust_decimal::Decimal>,
    ) -> Result<Vec<InstrumentAny>, DydxHttpError> {
        use nautilus_core::time::get_atomic_clock_realtime;

        let markets_response = self.get_markets().await?;
        let ts_init = get_atomic_clock_realtime().get_time_ns();

        let mut instruments = Vec::new();
        let mut skipped = 0;

        for (ticker, market) in markets_response.markets {
            match super::parse::parse_instrument_any(&market, maker_fee, taker_fee, ts_init) {
                Ok(instrument) => {
                    instruments.push(instrument);
                }
                Err(e) => {
                    tracing::warn!("Failed to parse instrument {ticker}: {e}");
                    skipped += 1;
                }
            }
        }

        if skipped > 0 {
            tracing::info!(
                "Parsed {} instruments, skipped {} (inactive or invalid)",
                instruments.len(),
                skipped
            );
        } else {
            tracing::info!("Parsed {} instruments", instruments.len());
        }

        Ok(instruments)
    }

    // ========================================================================
    // Account Endpoints
    // ========================================================================

    /// Fetch orderbook for a specific market.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn get_orderbook(
        &self,
        ticker: &str,
    ) -> Result<super::models::OrderbookResponse, DydxHttpError> {
        let endpoint = format!("/v4/orderbooks/perpetualMarket/{ticker}");
        self.send_request(Method::GET, &endpoint, None).await
    }

    /// Fetch recent trades for a market.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn get_trades(
        &self,
        ticker: &str,
        limit: Option<u32>,
    ) -> Result<super::models::TradesResponse, DydxHttpError> {
        let endpoint = format!("/v4/trades/perpetualMarket/{ticker}");
        let query = limit.map(|l| format!("limit={l}"));
        self.send_request(Method::GET, &endpoint, query.as_deref())
            .await
    }

    /// Fetch candles/klines for a market.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn get_candles(
        &self,
        ticker: &str,
        resolution: DydxCandleResolution,
        limit: Option<u32>,
        from_iso: Option<DateTime<Utc>>,
        to_iso: Option<DateTime<Utc>>,
    ) -> Result<super::models::CandlesResponse, DydxHttpError> {
        let endpoint = format!("/v4/candles/perpetualMarkets/{ticker}");
        let mut query_parts = vec![format!("resolution={}", resolution)];
        if let Some(l) = limit {
            query_parts.push(format!("limit={l}"));
        }
        if let Some(from) = from_iso {
            let from_str = from.to_rfc3339();
            query_parts.push(format!("fromISO={}", urlencoding::encode(&from_str)));
        }
        if let Some(to) = to_iso {
            let to_str = to.to_rfc3339();
            query_parts.push(format!("toISO={}", urlencoding::encode(&to_str)));
        }
        let query = query_parts.join("&");
        self.send_request(Method::GET, &endpoint, Some(&query))
            .await
    }

    // ========================================================================
    // Account Endpoints
    // ========================================================================

    /// Fetch subaccount information.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn get_subaccount(
        &self,
        address: &str,
        subaccount_number: u32,
    ) -> Result<super::models::SubaccountResponse, DydxHttpError> {
        let endpoint = format!("/v4/addresses/{address}/subaccountNumber/{subaccount_number}");
        self.send_request(Method::GET, &endpoint, None).await
    }

    /// Fetch fills for a subaccount.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn get_fills(
        &self,
        address: &str,
        subaccount_number: u32,
        market: Option<&str>,
        limit: Option<u32>,
    ) -> Result<super::models::FillsResponse, DydxHttpError> {
        let endpoint = "/v4/fills";
        let mut query_parts = vec![
            format!("address={address}"),
            format!("subaccountNumber={subaccount_number}"),
        ];
        if let Some(m) = market {
            query_parts.push(format!("market={m}"));
        }
        if let Some(l) = limit {
            query_parts.push(format!("limit={l}"));
        }
        let query = query_parts.join("&");
        self.send_request(Method::GET, endpoint, Some(&query)).await
    }

    /// Fetch orders for a subaccount.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn get_orders(
        &self,
        address: &str,
        subaccount_number: u32,
        market: Option<&str>,
        limit: Option<u32>,
    ) -> Result<super::models::OrdersResponse, DydxHttpError> {
        let endpoint = "/v4/orders";
        let mut query_parts = vec![
            format!("address={address}"),
            format!("subaccountNumber={subaccount_number}"),
        ];
        if let Some(m) = market {
            query_parts.push(format!("market={m}"));
        }
        if let Some(l) = limit {
            query_parts.push(format!("limit={l}"));
        }
        let query = query_parts.join("&");
        self.send_request(Method::GET, endpoint, Some(&query)).await
    }

    /// Fetch transfers for a subaccount.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn get_transfers(
        &self,
        address: &str,
        subaccount_number: u32,
        limit: Option<u32>,
    ) -> Result<super::models::TransfersResponse, DydxHttpError> {
        let endpoint = "/v4/transfers";
        let mut query_parts = vec![
            format!("address={address}"),
            format!("subaccountNumber={subaccount_number}"),
        ];
        if let Some(l) = limit {
            query_parts.push(format!("limit={l}"));
        }
        let query = query_parts.join("&");
        self.send_request(Method::GET, endpoint, Some(&query)).await
    }

    // ========================================================================
    // Utility Endpoints
    // ========================================================================

    /// Get current server time.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn get_time(&self) -> Result<super::models::TimeResponse, DydxHttpError> {
        self.send_request(Method::GET, "/v4/time", None).await
    }

    /// Get current blockchain height.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn get_height(&self) -> Result<super::models::HeightResponse, DydxHttpError> {
        self.send_request(Method::GET, "/v4/height", None).await
    }
}

/// Provides a higher-level HTTP client for the [dYdX v4](https://dydx.exchange) Indexer REST API.
///
/// This client wraps the underlying `DydxRawHttpClient` to handle conversions
/// into the Nautilus domain model, following the two-layer pattern established
/// in OKX, Bybit, and BitMEX adapters.
///
/// **Architecture:**
/// - **Raw client** (`DydxRawHttpClient`): Low-level HTTP methods matching dYdX Indexer API endpoints.
/// - **Domain client** (`DydxHttpClient`): High-level methods using Nautilus domain types.
///
/// The domain client:
/// - Wraps the raw client in an `Arc` for efficient cloning (required for Python bindings).
/// - Maintains an instrument cache using `DashMap` for thread-safe concurrent access.
/// - Provides standard cache methods: `cache_instruments()`, `cache_instrument()`, `get_instrument()`.
/// - Tracks cache initialization state for optimizations.
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct DydxHttpClient {
    /// Raw HTTP client wrapped in Arc for efficient cloning.
    pub(crate) inner: Arc<DydxRawHttpClient>,
    /// Instrument cache shared across the adapter using DashMap for thread-safe access.
    pub(crate) instruments_cache: Arc<DashMap<Ustr, InstrumentAny>>,
    /// Cached mapping from CLOB pair ID → InstrumentId for efficient lookups.
    ///
    /// This is populated from HTTP PerpetualMarket metadata (`clob_pair_id`) alongside
    /// instrument creation to avoid re-deriving IDs from symbols or other heuristics.
    pub(crate) clob_pair_id_to_instrument: Arc<DashMap<u32, InstrumentId>>,
    /// Tracks whether the instrument cache has been initialized.
    cache_initialized: AtomicBool,
}

impl Clone for DydxHttpClient {
    fn clone(&self) -> Self {
        let cache_initialized = AtomicBool::new(false);
        let is_initialized = self.cache_initialized.load(Ordering::Acquire);
        if is_initialized {
            cache_initialized.store(true, Ordering::Release);
        }

        Self {
            inner: self.inner.clone(),
            instruments_cache: self.instruments_cache.clone(),
            clob_pair_id_to_instrument: self.clob_pair_id_to_instrument.clone(),
            cache_initialized,
        }
    }
}

impl Default for DydxHttpClient {
    fn default() -> Self {
        Self::new(None, Some(60), None, false, None)
            .expect("Failed to create default DydxHttpClient")
    }
}

impl DydxHttpClient {
    /// Creates a new [`DydxHttpClient`] using the default dYdX Indexer HTTP URL,
    /// optionally overridden with a custom base URL.
    ///
    /// **Note**: No credentials are required as the dYdX Indexer API is publicly accessible.
    /// Order submission and trading operations use gRPC with blockchain transaction signing.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client or retry manager cannot be created.
    pub fn new(
        base_url: Option<String>,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
        is_testnet: bool,
        retry_config: Option<RetryConfig>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            inner: Arc::new(DydxRawHttpClient::new(
                base_url,
                timeout_secs,
                proxy_url,
                is_testnet,
                retry_config,
            )?),
            instruments_cache: Arc::new(DashMap::new()),
            clob_pair_id_to_instrument: Arc::new(DashMap::new()),
            cache_initialized: AtomicBool::new(false),
        })
    }

    /// Requests instruments from the dYdX Indexer API and returns Nautilus domain types.
    ///
    /// This method does NOT automatically cache results. Use `fetch_and_cache_instruments()`
    /// for automatic caching, or call `cache_instruments()` manually with the results.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request or parsing fails.
    /// Individual instrument parsing errors are logged as warnings.
    pub async fn request_instruments(
        &self,
        symbol: Option<String>,
        maker_fee: Option<rust_decimal::Decimal>,
        taker_fee: Option<rust_decimal::Decimal>,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        use nautilus_core::time::get_atomic_clock_realtime;

        let markets_response = self.inner.get_markets().await?;
        let ts_init = get_atomic_clock_realtime().get_time_ns();

        let mut instruments = Vec::new();
        let mut skipped = 0;

        for (ticker, market) in markets_response.markets {
            // Filter by symbol if specified
            if let Some(ref sym) = symbol
                && ticker != *sym
            {
                continue;
            }

            // Parse using http/parse.rs
            match super::parse::parse_instrument_any(&market, maker_fee, taker_fee, ts_init) {
                Ok(instrument) => {
                    instruments.push(instrument);
                }
                Err(e) => {
                    tracing::warn!("Failed to parse instrument {ticker}: {e}");
                    skipped += 1;
                }
            }
        }

        if skipped > 0 {
            tracing::info!(
                "Parsed {} instruments, skipped {} (inactive or invalid)",
                instruments.len(),
                skipped
            );
        } else {
            tracing::debug!("Parsed {} instruments", instruments.len());
        }

        Ok(instruments)
    }

    /// Fetches instruments from the API and caches them.
    ///
    /// This is a convenience method that fetches instruments and populates both
    /// the symbol-based and CLOB pair ID-based caches.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request or parsing fails.
    pub async fn fetch_and_cache_instruments(&self) -> anyhow::Result<()> {
        use nautilus_core::time::get_atomic_clock_realtime;

        self.instruments_cache.clear();
        self.clob_pair_id_to_instrument.clear();

        let markets_response = self.inner.get_markets().await?;
        let ts_init = get_atomic_clock_realtime().get_time_ns();

        let mut instruments = Vec::new();
        let mut skipped = 0;

        for (ticker, market) in markets_response.markets {
            // Parse using http/parse.rs
            match super::parse::parse_instrument_any(&market, None, None, ts_init) {
                Ok(instrument) => {
                    let instrument_id = instrument.id();
                    let symbol = instrument_id.symbol.inner();
                    self.instruments_cache.insert(symbol, instrument.clone());

                    // Also cache by clob_pair_id for efficient WebSocket lookups
                    self.clob_pair_id_to_instrument
                        .insert(market.clob_pair_id, instrument_id);

                    instruments.push(instrument);
                }
                Err(e) => {
                    tracing::warn!("Failed to parse instrument {ticker}: {e}");
                    skipped += 1;
                }
            }
        }

        if !instruments.is_empty() {
            self.cache_initialized.store(true, Ordering::Release);
        }

        if skipped > 0 {
            tracing::info!(
                "Cached {} instruments, skipped {} (inactive or invalid)",
                instruments.len(),
                skipped
            );
        } else {
            tracing::info!("Cached {} instruments", instruments.len());
        }

        Ok(())
    }

    /// Caches multiple instruments.
    ///
    /// Any existing instruments with the same symbols will be replaced.
    pub fn cache_instruments(&self, instruments: Vec<InstrumentAny>) {
        for inst in instruments {
            let symbol = inst.id().symbol.inner();
            self.instruments_cache.insert(symbol, inst);
        }
        self.cache_initialized.store(true, Ordering::Release);
    }

    /// Caches a single instrument.
    ///
    /// Any existing instrument with the same symbol will be replaced.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        let symbol = instrument.id().symbol.inner();
        self.instruments_cache.insert(symbol, instrument);
        self.cache_initialized.store(true, Ordering::Release);
    }

    /// Gets an instrument from the cache by symbol.
    #[must_use]
    pub fn get_instrument(&self, symbol: &Ustr) -> Option<InstrumentAny> {
        self.instruments_cache
            .get(symbol)
            .map(|entry| entry.clone())
    }

    /// Gets an instrument by CLOB pair ID.
    ///
    /// This uses the internal clob_pair_id mapping populated during `fetch_and_cache_instruments()`.
    #[must_use]
    pub fn get_instrument_by_clob_id(&self, clob_pair_id: u32) -> Option<InstrumentAny> {
        // First get the InstrumentId from clob_pair_id mapping
        let instrument_id = self
            .clob_pair_id_to_instrument
            .get(&clob_pair_id)
            .map(|entry| *entry)?;

        // Then look up the full instrument by symbol
        self.get_instrument(&instrument_id.symbol.inner())
    }

    /// Requests historical trades for a symbol.
    ///
    /// Fetches trade data from the dYdX Indexer API's `/v4/trades/perpetualMarket/:ticker` endpoint.
    /// Results are ordered by creation time descending (newest first).
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response cannot be parsed.
    pub async fn request_trades(
        &self,
        symbol: &str,
        limit: Option<u32>,
    ) -> anyhow::Result<super::models::TradesResponse> {
        self.inner
            .get_trades(symbol, limit)
            .await
            .map_err(Into::into)
    }

    /// Requests historical candles for a symbol.
    ///
    /// Fetches candle data from the dYdX Indexer API's `/v4/candles/perpetualMarkets/:ticker` endpoint.
    /// Results are ordered by start time ascending (oldest first).
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response cannot be parsed.
    pub async fn request_candles(
        &self,
        symbol: &str,
        resolution: DydxCandleResolution,
        limit: Option<u32>,
        from_iso: Option<DateTime<Utc>>,
        to_iso: Option<DateTime<Utc>>,
    ) -> anyhow::Result<super::models::CandlesResponse> {
        self.inner
            .get_candles(symbol, resolution, limit, from_iso, to_iso)
            .await
            .map_err(Into::into)
    }

    /// Helper to get instrument or fetch if not cached.
    ///
    /// This is a convenience method that first checks the cache, and if the
    /// instrument is not found, fetches it from the API. This is useful for
    /// ensuring an instrument is available without explicitly managing the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The HTTP request fails.
    /// - The instrument is not found on the exchange.
    ///
    #[allow(dead_code)]
    async fn instrument_or_fetch(&self, symbol: Ustr) -> anyhow::Result<InstrumentAny> {
        if let Some(instrument) = self.get_instrument(&symbol) {
            return Ok(instrument);
        }

        // Fetch from API
        let instruments = self
            .request_instruments(Some(symbol.to_string()), None, None)
            .await?;

        instruments
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("Instrument not found: {symbol}"))
    }

    /// Exposes raw HTTP client for testing and advanced use cases.
    ///
    /// This provides access to the underlying [`DydxRawHttpClient`] for cases
    /// where low-level API access is needed. Most users should use the domain
    /// client methods instead.
    #[must_use]
    pub fn raw_client(&self) -> &Arc<DydxRawHttpClient> {
        &self.inner
    }

    /// Check if this client is configured for testnet.
    #[must_use]
    pub fn is_testnet(&self) -> bool {
        self.inner.is_testnet()
    }

    /// Get the base URL being used by this client.
    #[must_use]
    pub fn base_url(&self) -> &str {
        self.inner.base_url()
    }

    /// Check if the instrument cache has been initialized.
    #[must_use]
    pub fn is_cache_initialized(&self) -> bool {
        self.cache_initialized.load(Ordering::Acquire)
    }

    /// Get the number of instruments currently cached.
    #[must_use]
    pub fn cached_instruments_count(&self) -> usize {
        self.instruments_cache.len()
    }

    /// Returns a reference to the instruments cache.
    #[must_use]
    pub fn instruments(&self) -> &Arc<DashMap<Ustr, InstrumentAny>> {
        &self.instruments_cache
    }

    /// Get the mapping from CLOB pair ID to `InstrumentId`.
    ///
    /// This map is populated when instruments are fetched via `request_instruments` /
    /// `cache_instruments()` using the Indexer `PerpetualMarket.clob_pair_id` field.
    #[must_use]
    pub fn clob_pair_id_mapping(&self) -> &Arc<DashMap<u32, InstrumentId>> {
        &self.clob_pair_id_to_instrument
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use rstest::rstest;

    use super::*;
    use crate::http::error;

    // ========================================================================
    // Raw Client Tests
    // ========================================================================

    #[tokio::test]
    async fn test_raw_client_creation() {
        let client = DydxRawHttpClient::new(None, Some(30), None, false, None);
        assert!(client.is_ok());

        let client = client.unwrap();
        assert!(!client.is_testnet());
        assert_eq!(client.base_url(), DYDX_HTTP_URL);
    }

    #[tokio::test]
    async fn test_raw_client_testnet() {
        let client = DydxRawHttpClient::new(None, Some(30), None, true, None);
        assert!(client.is_ok());

        let client = client.unwrap();
        assert!(client.is_testnet());
        assert_eq!(client.base_url(), DYDX_TESTNET_HTTP_URL);
    }

    // ========================================================================
    // Domain Client Tests
    // ========================================================================

    #[tokio::test]
    async fn test_domain_client_creation() {
        let client = DydxHttpClient::new(None, Some(30), None, false, None);
        assert!(client.is_ok());

        let client = client.unwrap();
        assert!(!client.is_testnet());
        assert_eq!(client.base_url(), DYDX_HTTP_URL);
        assert!(!client.is_cache_initialized());
        assert_eq!(client.cached_instruments_count(), 0);
    }

    #[tokio::test]
    async fn test_domain_client_testnet() {
        let client = DydxHttpClient::new(None, Some(30), None, true, None);
        assert!(client.is_ok());

        let client = client.unwrap();
        assert!(client.is_testnet());
        assert_eq!(client.base_url(), DYDX_TESTNET_HTTP_URL);
    }

    #[tokio::test]
    async fn test_domain_client_default() {
        let client = DydxHttpClient::default();
        assert!(!client.is_testnet());
        assert_eq!(client.base_url(), DYDX_HTTP_URL);
        assert!(!client.is_cache_initialized());
    }

    #[tokio::test]
    async fn test_domain_client_clone() {
        let client = DydxHttpClient::new(None, Some(30), None, false, None).unwrap();

        // Clone before initialization
        let cloned = client.clone();
        assert!(!cloned.is_cache_initialized());

        // Simulate cache initialization
        client.cache_initialized.store(true, Ordering::Release);

        // Clone after initialization
        #[allow(clippy::redundant_clone)]
        let cloned_after = client.clone();
        assert!(cloned_after.is_cache_initialized());
    }

    #[rstest]
    fn test_domain_client_cache_instrument() {
        use nautilus_model::{
            identifiers::{InstrumentId, Symbol},
            instruments::CryptoPerpetual,
            types::{Currency, Price, Quantity},
        };

        let client = DydxHttpClient::default();
        assert_eq!(client.cached_instruments_count(), 0);

        // Create a test instrument
        let instrument_id =
            InstrumentId::new(Symbol::from("BTC-USD"), *crate::common::consts::DYDX_VENUE);
        let price = Price::from("1.0");
        let size = Quantity::from("0.001");
        let instrument = CryptoPerpetual::new(
            instrument_id,
            Symbol::from("BTC-USD"),
            Currency::BTC(),
            Currency::USD(),
            Currency::USD(),
            false,
            price.precision,
            size.precision,
            price,
            size,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        // Cache the instrument
        client.cache_instrument(InstrumentAny::CryptoPerpetual(instrument));
        assert_eq!(client.cached_instruments_count(), 1);
        assert!(client.is_cache_initialized());

        // Retrieve it
        let btc_usd = Ustr::from("BTC-USD");
        let cached = client.get_instrument(&btc_usd);
        assert!(cached.is_some());
    }

    #[rstest]
    fn test_domain_client_get_instrument_not_found() {
        let client = DydxHttpClient::default();
        let eth_usd = Ustr::from("ETH-USD");
        let result = client.get_instrument(&eth_usd);
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_http_timeout_respects_configuration_and_does_not_block() {
        use axum::{Router, routing::get};
        use tokio::net::TcpListener;

        async fn slow_handler() -> &'static str {
            // Sleep longer than the configured HTTP timeout.
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
            "ok"
        }

        let router = Router::new().route("/v4/slow", get(slow_handler));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            axum::serve(listener, router.into_make_service())
                .await
                .unwrap();
        });

        let base_url = format!("http://{}", addr);

        // Configure a small operation timeout and no retries so the request
        // fails quickly even though the handler sleeps for 5 seconds.
        let retry_config = RetryConfig {
            max_retries: 0,
            initial_delay_ms: 0,
            max_delay_ms: 0,
            backoff_factor: 1.0,
            jitter_ms: 0,
            operation_timeout_ms: Some(500),
            immediate_first: true,
            max_elapsed_ms: Some(1_000),
        };

        // Keep HTTP client timeout at a typical value; rely on RetryManager
        // operation timeout to enforce non-blocking behavior.
        let client =
            DydxRawHttpClient::new(Some(base_url), Some(60), None, false, Some(retry_config))
                .unwrap();

        let start = std::time::Instant::now();
        let result: Result<serde_json::Value, error::DydxHttpError> =
            client.send_request(Method::GET, "/v4/slow", None).await;
        let elapsed = start.elapsed();

        // Request should fail (timeout or client error), but without blocking the thread
        // for the full handler duration.
        assert!(result.is_err());
        assert!(elapsed < std::time::Duration::from_secs(3));
    }
}
