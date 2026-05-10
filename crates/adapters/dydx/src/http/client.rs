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

//! Provides an ergonomic wrapper around the **dYdX v4 Indexer REST API** –
//! <https://docs.dydx.xyz/api_integration-indexer/indexer_api>.
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
//! ## Responsibilities
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
//! # Official Documentation
//!
//! | Endpoint          | Reference                                                                 |
//! |-------------------|---------------------------------------------------------------------------|
//! | Market data       | <https://docs.dydx.xyz/api_integration-indexer/indexer_api#markets>  |
//! | Account data      | <https://docs.dydx.xyz/api_integration-indexer/indexer_api#accounts> |
//! | Utility endpoints | <https://docs.dydx.xyz/api_integration-indexer/indexer_api#utility>  |

use std::{
    collections::HashMap,
    fmt::Debug,
    num::NonZeroU32,
    sync::{Arc, LazyLock},
};

use chrono::{DateTime, Utc};
use nautilus_core::{
    UnixNanos,
    consts::NAUTILUS_USER_AGENT,
    string::urlencoding,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{
        Bar, BarType, BookOrder, FundingRateUpdate, OrderBookDelta, OrderBookDeltas, TradeTick,
    },
    enums::{
        AggregationSource, BarAggregation, BookAction, OrderSide as NautilusOrderSide, PriceType,
        RecordFlag,
    },
    events::AccountState,
    identifiers::{AccountId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{Price, Quantity},
};
use nautilus_network::{
    http::{HttpClient, Method, USER_AGENT},
    ratelimiter::quota::Quota,
    retry::{RetryConfig, RetryManager},
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio_util::sync::CancellationToken;

use super::error::DydxHttpError;
use crate::{
    common::{
        consts::{DYDX_HTTP_URL, DYDX_TESTNET_HTTP_URL},
        enums::{DydxCandleResolution, DydxNetwork},
        instrument_cache::InstrumentCache,
        parse::extract_raw_symbol,
    },
    http::parse::{parse_account_state_from_http, parse_instrument_any},
};

/// Maximum number of candles returned per dYdX API request.
const DYDX_MAX_BARS_PER_REQUEST: u32 = 1_000;

/// Perpetual markets endpoint (shared between `get_markets` and `get_market`).
const ENDPOINT_PERPETUAL_MARKETS: &str = "/v4/perpetualMarkets";

fn bar_type_to_resolution(bar_type: &BarType) -> anyhow::Result<DydxCandleResolution> {
    if bar_type.aggregation_source() != AggregationSource::External {
        anyhow::bail!(
            "dYdX only supports EXTERNAL aggregation, was {:?}",
            bar_type.aggregation_source()
        );
    }

    let spec = bar_type.spec();
    if spec.price_type != PriceType::Last {
        anyhow::bail!(
            "dYdX only supports LAST price type, was {:?}",
            spec.price_type
        );
    }

    DydxCandleResolution::from_bar_spec(&spec)
}

/// Default dYdX Indexer REST API rate limit.
///
/// The dYdX Indexer API rate limits are generous for read-only operations:
/// - General: 100 requests per 10 seconds per IP
/// - We use a conservative 10 requests per second as the default quota.
pub static DYDX_REST_QUOTA: LazyLock<Quota> = LazyLock::new(|| {
    Quota::per_second(NonZeroU32::new(10).expect("non-zero")).expect("valid constant")
});

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
    network: DydxNetwork,
}

impl Default for DydxRawHttpClient {
    fn default() -> Self {
        Self::new(None, 60, None, DydxNetwork::Mainnet, None)
            .expect("Failed to create default DydxRawHttpClient")
    }
}

impl Debug for DydxRawHttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(DydxRawHttpClient))
            .field("base_url", &self.base_url)
            .field("network", &self.network)
            .finish_non_exhaustive()
    }
}

impl DydxRawHttpClient {
    /// Cancels all pending HTTP requests.
    pub fn cancel_all_requests(&self) {
        self.cancellation_token.cancel();
    }

    /// Returns the cancellation token for this client.
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
        timeout_secs: u64,
        proxy_url: Option<String>,
        network: DydxNetwork,
        retry_config: Option<RetryConfig>,
    ) -> anyhow::Result<Self> {
        let base_url = match network {
            DydxNetwork::Testnet => base_url.unwrap_or_else(|| DYDX_TESTNET_HTTP_URL.to_string()),
            DydxNetwork::Mainnet => base_url.unwrap_or_else(|| DYDX_HTTP_URL.to_string()),
        };

        let retry_manager = RetryManager::new(retry_config.unwrap_or_default());

        let mut headers = HashMap::new();
        headers.insert(USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string());

        let client = HttpClient::new(
            headers,
            vec![], // No specific headers to extract from responses
            vec![], // No keyed quotas (we use a single global quota)
            Some(*DYDX_REST_QUOTA),
            Some(timeout_secs),
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
            network,
        })
    }

    /// Returns `true` if this client is configured for testnet.
    #[must_use]
    pub const fn is_testnet(&self) -> bool {
        matches!(self.network, DydxNetwork::Testnet)
    }

    /// Returns the base URL used by this client.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Sends a request to a dYdX Indexer API endpoint.
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
            } else if msg.contains("Timed out") {
                // Timeouts are transient -- map to HttpClientError so they are retried
                DydxHttpError::HttpClientError(msg)
            } else {
                DydxHttpError::ValidationError(msg)
            }
        };

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

        serde_json::from_slice(&response.body).map_err(|e| DydxHttpError::Deserialization {
            error: e.to_string(),
            body: String::from_utf8_lossy(&response.body).to_string(),
        })
    }

    /// Sends a POST request to a dYdX Indexer API endpoint.
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
            } else if msg.contains("Timed out") {
                // Timeouts are transient -- map to HttpClientError so they are retried
                DydxHttpError::HttpClientError(msg)
            } else {
                DydxHttpError::ValidationError(msg)
            }
        };

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

        serde_json::from_slice(&response.body).map_err(|e| DydxHttpError::Deserialization {
            error: e.to_string(),
            body: String::from_utf8_lossy(&response.body).to_string(),
        })
    }

    /// Fetch all perpetual markets from dYdX.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn get_markets(&self) -> Result<super::models::MarketsResponse, DydxHttpError> {
        self.send_request(Method::GET, ENDPOINT_PERPETUAL_MARKETS, None)
            .await
    }

    /// Fetch a single perpetual market by ticker.
    ///
    /// Uses the `market` query parameter for efficient single-market fetch.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn get_market(
        &self,
        ticker: &str,
    ) -> Result<super::models::MarketsResponse, DydxHttpError> {
        let query = format!("ticker={ticker}");
        self.send_request(Method::GET, ENDPOINT_PERPETUAL_MARKETS, Some(&query))
            .await
    }

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
        starting_before_or_at_height: Option<u64>,
    ) -> Result<super::models::TradesResponse, DydxHttpError> {
        let endpoint = format!("/v4/trades/perpetualMarket/{ticker}");
        let mut query_parts = Vec::new();

        if let Some(l) = limit {
            query_parts.push(format!("limit={l}"));
        }

        if let Some(height) = starting_before_or_at_height {
            query_parts.push(format!("createdBeforeOrAtHeight={height}"));
        }
        let query = if query_parts.is_empty() {
            None
        } else {
            Some(query_parts.join("&"))
        };
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
        let mut query_parts = vec![format!("resolution={resolution}")];

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
            query_parts.push("marketType=PERPETUAL".to_string());
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
            query_parts.push("marketType=PERPETUAL".to_string());
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

    /// Fetch historical funding rates for a market.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn get_historical_funding(
        &self,
        ticker: &str,
        limit: Option<u32>,
        effective_before_or_at_height: Option<u64>,
        effective_before_or_at: Option<DateTime<Utc>>,
    ) -> Result<super::models::HistoricalFundingResponse, DydxHttpError> {
        let endpoint = format!("/v4/historicalFunding/{ticker}");
        let mut query_parts = Vec::new();

        if let Some(l) = limit {
            query_parts.push(format!("limit={l}"));
        }

        if let Some(height) = effective_before_or_at_height {
            query_parts.push(format!("effectiveBeforeOrAtHeight={height}"));
        }

        if let Some(before) = effective_before_or_at {
            let before_str = before.to_rfc3339();
            query_parts.push(format!(
                "effectiveBeforeOrAt={}",
                urlencoding::encode(&before_str)
            ));
        }

        let query = if query_parts.is_empty() {
            None
        } else {
            Some(query_parts.join("&"))
        };
        self.send_request(Method::GET, &endpoint, query.as_deref())
            .await
    }

    /// Returns the current server time.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response parsing fails.
    pub async fn get_time(&self) -> Result<super::models::TimeResponse, DydxHttpError> {
        self.send_request(Method::GET, "/v4/time", None).await
    }

    /// Returns the current blockchain height.
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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.dydx", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.dydx")
)]
pub struct DydxHttpClient {
    /// Raw HTTP client wrapped in Arc for efficient cloning.
    pub(crate) inner: Arc<DydxRawHttpClient>,
    /// Shared instrument cache with multiple lookup indices.
    ///
    /// This cache is shared across HTTP client, WebSocket client, and execution client.
    /// It provides O(1) lookups by symbol, market ticker, or clob_pair_id.
    pub(crate) instrument_cache: Arc<InstrumentCache>,
    clock: &'static AtomicTime,
}

impl Clone for DydxHttpClient {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            instrument_cache: Arc::clone(&self.instrument_cache),
            clock: self.clock,
        }
    }
}

impl Default for DydxHttpClient {
    fn default() -> Self {
        Self::new(None, 60, None, DydxNetwork::Mainnet, None)
            .expect("Failed to create default DydxHttpClient")
    }
}

impl DydxHttpClient {
    /// Creates a new [`DydxHttpClient`] using the default dYdX Indexer HTTP URL,
    /// optionally overridden with a custom base URL.
    ///
    /// This constructor creates its own internal instrument cache. For shared caching
    /// across multiple clients, use [`new_with_cache`](Self::new_with_cache) instead.
    ///
    /// **Note**: No credentials are required as the dYdX Indexer API is publicly accessible.
    /// Order submission and trading operations use gRPC with blockchain transaction signing.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client or retry manager cannot be created.
    pub fn new(
        base_url: Option<String>,
        timeout_secs: u64,
        proxy_url: Option<String>,
        network: DydxNetwork,
        retry_config: Option<RetryConfig>,
    ) -> anyhow::Result<Self> {
        Self::new_with_cache(
            base_url,
            timeout_secs,
            proxy_url,
            network,
            retry_config,
            Arc::new(InstrumentCache::new()),
        )
    }

    /// Creates a new [`DydxHttpClient`] with a shared instrument cache.
    ///
    /// Use this constructor when sharing instrument data between HTTP client,
    /// WebSocket client, and execution client.
    ///
    /// # Arguments
    ///
    /// * `instrument_cache` - Shared instrument cache for lookups by symbol, ticker, or clob_pair_id
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client or retry manager cannot be created.
    pub fn new_with_cache(
        base_url: Option<String>,
        timeout_secs: u64,
        proxy_url: Option<String>,
        network: DydxNetwork,
        retry_config: Option<RetryConfig>,
        instrument_cache: Arc<InstrumentCache>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            inner: Arc::new(DydxRawHttpClient::new(
                base_url,
                timeout_secs,
                proxy_url,
                network,
                retry_config,
            )?),
            instrument_cache,
            clock: get_atomic_clock_realtime(),
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
        maker_fee: Option<Decimal>,
        taker_fee: Option<Decimal>,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let markets_response = self.inner.get_markets().await?;
        let ts_init = self.generate_ts_init();

        let mut instruments = Vec::new();
        let mut skipped_inactive = 0;

        for (ticker, market) in markets_response.markets {
            // Filter by symbol if specified
            if let Some(ref sym) = symbol
                && ticker != *sym
            {
                continue;
            }

            if !super::parse::is_market_active(&market.status) {
                log::debug!(
                    "Skipping inactive market {ticker} (status: {:?})",
                    market.status
                );
                skipped_inactive += 1;
                continue;
            }

            match super::parse::parse_instrument_any(&market, maker_fee, taker_fee, ts_init) {
                Ok(instrument) => {
                    instruments.push(instrument);
                }
                Err(e) => {
                    log::error!("Failed to parse instrument {ticker}: {e}");
                }
            }
        }

        if skipped_inactive > 0 {
            log::info!(
                "Parsed {} instruments, skipped {} inactive",
                instruments.len(),
                skipped_inactive
            );
        } else {
            log::debug!("Parsed {} instruments", instruments.len());
        }

        Ok(instruments)
    }

    /// Fetches instruments from the API and caches them.
    ///
    /// This is a convenience method that fetches instruments and populates both
    /// the symbol-based and CLOB pair ID-based caches.
    ///
    /// On success, existing caches are cleared and repopulated atomically.
    /// On failure, existing caches are preserved (no partial updates).
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails.
    pub async fn fetch_and_cache_instruments(&self) -> anyhow::Result<()> {
        // Fetch first - preserve existing cache on network failure
        let markets_response = self.inner.get_markets().await?;
        let ts_init = self.generate_ts_init();

        let mut parsed_instruments = Vec::new();
        let mut parsed_markets = Vec::new();
        let mut skipped_inactive = 0;

        for (ticker, market) in markets_response.markets {
            if !super::parse::is_market_active(&market.status) {
                log::debug!(
                    "Skipping inactive market {ticker} (status: {:?})",
                    market.status
                );
                skipped_inactive += 1;
                continue;
            }

            match super::parse::parse_instrument_any(&market, None, None, ts_init) {
                Ok(instrument) => {
                    parsed_instruments.push(instrument);
                    parsed_markets.push(market);
                }
                Err(e) => {
                    log::error!("Failed to parse instrument {ticker}: {e}");
                }
            }
        }

        // Only clear and repopulate cache after successful fetch and parse
        self.instrument_cache.clear();

        // Zip instruments with their market data for bulk insert
        let items: Vec<_> = parsed_instruments.into_iter().zip(parsed_markets).collect();

        if !items.is_empty() {
            self.instrument_cache.insert_many(items.clone());
        }

        let count = items.len();

        if skipped_inactive > 0 {
            log::info!("Cached {count} instruments, skipped {skipped_inactive} inactive");
        } else {
            log::info!("Cached {count} instruments");
        }

        Ok(())
    }

    /// Fetches a single instrument by ticker and caches it.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails.
    pub async fn fetch_and_cache_single_instrument(
        &self,
        ticker: &str,
    ) -> anyhow::Result<Option<InstrumentAny>> {
        let markets_response = self.inner.get_market(ticker).await?;
        let ts_init = self.generate_ts_init();

        // The API returns all markets if ticker not found, so check specifically
        if let Some(market) = markets_response.markets.get(ticker) {
            if !super::parse::is_market_active(&market.status) {
                log::debug!(
                    "Skipping inactive market {ticker} (status: {:?})",
                    market.status
                );
                return Ok(None);
            }

            let instrument = parse_instrument_any(market, None, None, ts_init)?;
            self.instrument_cache
                .insert(instrument.clone(), market.clone());

            log::info!("Fetched and cached new instrument: {ticker}");
            return Ok(Some(instrument));
        }

        Ok(None)
    }

    /// Caches multiple instruments (symbol lookup only).
    ///
    /// Use `fetch_and_cache_instruments()` for full caching with market params.
    /// Any existing instruments with the same symbols will be replaced.
    pub fn cache_instruments(&self, instruments: Vec<InstrumentAny>) {
        self.instrument_cache.insert_instruments_only(instruments);
    }

    /// Caches a single instrument (symbol lookup only).
    ///
    /// Use `fetch_and_cache_instruments()` for full caching with market params.
    /// Any existing instrument with the same symbol will be replaced.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        self.instrument_cache.insert_instrument_only(instrument);
    }

    /// Gets an instrument from the cache by InstrumentId.
    #[must_use]
    pub fn get_instrument(&self, instrument_id: &InstrumentId) -> Option<InstrumentAny> {
        self.instrument_cache.get(instrument_id)
    }

    /// Gets an instrument by CLOB pair ID.
    ///
    /// Only works for instruments cached via `fetch_and_cache_instruments()`.
    #[must_use]
    pub fn get_instrument_by_clob_id(&self, clob_pair_id: u32) -> Option<InstrumentAny> {
        self.instrument_cache.get_by_clob_id(clob_pair_id)
    }

    /// Gets an instrument by market ticker (e.g., "BTC-USD").
    ///
    /// Only works for instruments cached via `fetch_and_cache_instruments()`.
    #[must_use]
    pub fn get_instrument_by_market(&self, ticker: &str) -> Option<InstrumentAny> {
        self.instrument_cache.get_by_market(ticker)
    }

    /// Gets market parameters for order submission from the cached market data.
    ///
    /// Returns the quantization parameters needed by OrderBuilder to construct
    /// properly formatted orders for the dYdX v4 protocol.
    ///
    /// # Errors
    ///
    /// Returns None if the instrument is not found in the market params cache.
    #[must_use]
    pub fn get_market_params(
        &self,
        instrument_id: &InstrumentId,
    ) -> Option<super::models::PerpetualMarket> {
        self.instrument_cache.get_market_params(instrument_id)
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
        starting_before_or_at_height: Option<u64>,
    ) -> anyhow::Result<super::models::TradesResponse> {
        self.inner
            .get_trades(symbol, limit, starting_before_or_at_height)
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

    /// Requests historical bars for an instrument with optional pagination.
    ///
    /// Fetches candle data from the dYdX Indexer API and converts to Nautilus
    /// `Bar` objects. Supports time-chunked pagination for large date ranges.
    ///
    /// The resolution is derived internally from `bar_type` (no need to pass
    /// `DydxCandleResolution`). Incomplete bars (where `ts_event >= now`) are
    /// filtered out.
    ///
    /// Results are returned in chronological order (oldest first).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The bar type uses unsupported aggregation/price type.
    /// - The HTTP request fails or response cannot be parsed.
    /// - The instrument is not found in the cache.
    pub async fn request_bars(
        &self,
        bar_type: BarType,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
        timestamp_on_close: bool,
    ) -> anyhow::Result<Vec<Bar>> {
        let resolution = bar_type_to_resolution(&bar_type)?;
        let instrument_id = bar_type.instrument_id();

        let instrument = self
            .get_instrument(&instrument_id)
            .ok_or_else(|| anyhow::anyhow!("Instrument not found in cache: {instrument_id}"))?;

        let ticker = extract_raw_symbol(instrument_id.symbol.as_str());
        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();
        let ts_init = self.generate_ts_init();

        let mut all_bars: Vec<Bar> = Vec::new();

        // Determine bar duration in seconds for pagination chunking
        let spec = bar_type.spec();
        let bar_secs: i64 = match spec.aggregation {
            BarAggregation::Minute => spec.step.get() as i64 * 60,
            BarAggregation::Hour => spec.step.get() as i64 * 3_600,
            BarAggregation::Day => spec.step.get() as i64 * 86_400,
            _ => anyhow::bail!("Unsupported aggregation: {:?}", spec.aggregation),
        };

        match (start, end) {
            // Time-chunked pagination for date ranges
            (Some(range_start), Some(range_end)) if range_end > range_start => {
                let overall_limit = limit.unwrap_or(u32::MAX);
                let mut remaining = overall_limit;
                let bars_per_call = DYDX_MAX_BARS_PER_REQUEST.min(remaining);
                let chunk_duration = chrono::Duration::seconds(bar_secs * bars_per_call as i64);
                let mut chunk_start = range_start;

                while chunk_start < range_end && remaining > 0 {
                    let chunk_end = (chunk_start + chunk_duration).min(range_end);
                    let per_call_limit = remaining.min(DYDX_MAX_BARS_PER_REQUEST);

                    let response = self
                        .inner
                        .get_candles(
                            ticker,
                            resolution,
                            Some(per_call_limit),
                            Some(chunk_start),
                            Some(chunk_end),
                        )
                        .await?;

                    let count = response.candles.len() as u32;
                    if count == 0 {
                        break;
                    }

                    for candle in &response.candles {
                        match super::parse::parse_bar(
                            candle,
                            bar_type,
                            price_precision,
                            size_precision,
                            timestamp_on_close,
                            ts_init,
                        ) {
                            Ok(bar) => all_bars.push(bar),
                            Err(e) => log::warn!("Failed to parse candle for {instrument_id}: {e}"),
                        }
                    }

                    if remaining <= count {
                        break;
                    }
                    remaining -= count;
                    chunk_start += chunk_duration;
                }
            }
            // Single request (no date range or invalid range)
            _ => {
                let req_limit = limit.unwrap_or(DYDX_MAX_BARS_PER_REQUEST);
                let response = self
                    .inner
                    .get_candles(ticker, resolution, Some(req_limit), None, None)
                    .await?;

                for candle in &response.candles {
                    match super::parse::parse_bar(
                        candle,
                        bar_type,
                        price_precision,
                        size_precision,
                        timestamp_on_close,
                        ts_init,
                    ) {
                        Ok(bar) => all_bars.push(bar),
                        Err(e) => log::warn!("Failed to parse candle for {instrument_id}: {e}"),
                    }
                }
            }
        }

        // Filter incomplete bars (ts_event >= current time)
        let current_time_ns = self.generate_ts_init();
        all_bars.retain(|bar| bar.ts_event < current_time_ns);

        Ok(all_bars)
    }

    /// Requests historical trade ticks for an instrument with optional pagination.
    ///
    /// Fetches trade data from the dYdX Indexer API and converts them to Nautilus
    /// `TradeTick` objects. Supports cursor-based pagination using block height
    /// and client-side time filtering (the dYdX API has no timestamp filter).
    ///
    /// Results are returned in chronological order (oldest first).
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails, response cannot be parsed,
    /// or the instrument is not found in the cache.
    ///
    /// # Panics
    ///
    /// This function will panic if the API returns a non-empty trades response
    /// but `last()` on the trades vector returns `None` (should never happen).
    pub async fn request_trade_ticks(
        &self,
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<TradeTick>> {
        const DYDX_MAX_TRADES_PER_REQUEST: u32 = 1_000;

        // Validation
        if let (Some(s), Some(e)) = (start, end) {
            anyhow::ensure!(s < e, "start ({s}) must be before end ({e})");
        }

        let instrument = self
            .get_instrument(&instrument_id)
            .ok_or_else(|| anyhow::anyhow!("Instrument not found in cache: {instrument_id}"))?;

        let ticker = extract_raw_symbol(instrument_id.symbol.as_str());
        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();
        let ts_init = self.generate_ts_init();

        // We always start pagination from the chain head (cursor = None). An earlier
        // version used `DEFAULT_BLOCK_TIME_SECS` with `get_height()` to skip directly
        // to an estimated target block, but any hardcoded block-time estimate that
        // underestimates the true average lands the cursor BEFORE the real `end`
        // block and silently drops the trades in the skipped window. Walking back
        // from head costs a few extra round-trips for stale `end` times but is
        // always correct. Per-call trades above `end` are filtered inside the loop.
        let overall_limit = limit.unwrap_or(u32::MAX);
        let mut remaining = overall_limit;
        let mut cursor_height: Option<u64> = None;
        let mut all_trades = Vec::new();
        // Global trade-id dedup across pages. Using a set prevents non-adjacent duplicates
        // from slipping past the legacy Vec::dedup_by adjacency check.
        let mut seen_trade_ids: ahash::AHashSet<String> = ahash::AHashSet::new();

        loop {
            let page_limit = remaining.min(DYDX_MAX_TRADES_PER_REQUEST);
            let response = self
                .inner
                .get_trades(ticker, Some(page_limit), cursor_height)
                .await?;

            let page_count = response.trades.len() as u32;
            if page_count == 0 {
                break;
            }

            // Trades come newest-first; oldest is last
            let oldest_trade = response.trades.last().unwrap();
            let oldest_height = oldest_trade.created_at_height;
            let oldest_created_at = oldest_trade.created_at;

            // Count how many unique (unseen) trades this page contributed
            let mut new_trades_this_page: usize = 0;
            let mut page_before_start = false;

            for trade in &response.trades {
                if !seen_trade_ids.insert(trade.id.clone()) {
                    // Already emitted; skip
                    continue;
                }

                if start.is_some_and(|s| trade.created_at < s) {
                    page_before_start = true;
                    continue;
                }

                if end.is_some_and(|e| trade.created_at > e) {
                    continue;
                }

                all_trades.push(super::parse::parse_trade_tick(
                    trade,
                    instrument_id,
                    price_precision,
                    size_precision,
                    ts_init,
                )?);
                new_trades_this_page += 1;
            }

            // If the oldest trade is before the start boundary we're done
            if let Some(s) = start
                && oldest_created_at < s
            {
                let _ = page_before_start;
                break;
            }

            // Advance the cursor by one block. `createdBeforeOrAtHeight` is an inclusive
            // upper bound, and the endpoint has no `after`/offset cursor, so keeping the
            // same height would re-request the same page. Any same-block trades that
            // overflowed the previous page are lost here; the dYdX venue tops out well
            // below `DYDX_MAX_TRADES_PER_REQUEST` trades per block in practice. The
            // `saturating_sub(1)` bottoms out at 0, which the `page_count == 0` guard at
            // the top of the loop handles.
            let next_cursor = Some(oldest_height.saturating_sub(1));

            // Terminal guard: if we're already at block 0 and this page produced nothing
            // new, there is nowhere further back to paginate.
            if oldest_height == 0 && new_trades_this_page == 0 {
                break;
            }
            cursor_height = next_cursor;

            remaining = remaining.saturating_sub(new_trades_this_page as u32);

            // Break on partial page (no more data) or limit reached
            if page_count < page_limit || remaining == 0 {
                break;
            }
        }

        // Reverse to chronological order (oldest first)
        all_trades.reverse();

        // Truncate to requested limit
        if let Some(lim) = limit {
            all_trades.truncate(lim as usize);
        }

        Ok(all_trades)
    }

    /// Requests historical funding rates for an instrument.
    ///
    /// Fetches funding rate data from the dYdX Indexer API's
    /// `/v4/historicalFunding/:ticker` endpoint and converts them to Nautilus
    /// `FundingRateUpdate` objects.
    ///
    /// Results are returned in chronological order (oldest first).
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or response cannot be parsed.
    pub async fn request_funding_rates(
        &self,
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<FundingRateUpdate>> {
        let ticker = extract_raw_symbol(instrument_id.symbol.as_str());
        let ts_init = self.generate_ts_init();

        let response = self
            .inner
            .get_historical_funding(ticker, limit, None, end)
            .await?;

        let mut rates = Vec::with_capacity(response.historical_funding.len());

        for entry in &response.historical_funding {
            // Filter by start time if specified
            if start.is_some_and(|s| entry.effective_at < s) {
                continue;
            }

            let ts_event =
                UnixNanos::from(entry.effective_at.timestamp_nanos_opt().ok_or_else(|| {
                    anyhow::anyhow!("Timestamp overflow for {}", entry.effective_at)
                })? as u64);

            rates.push(FundingRateUpdate::new(
                instrument_id,
                entry.rate,
                Some(60),
                None,
                ts_event,
                ts_init,
            ));
        }

        // dYdX returns newest first; reverse to chronological order
        rates.reverse();

        log::info!("Fetched {} funding rates for {instrument_id}", rates.len(),);

        Ok(rates)
    }

    /// Requests an order book snapshot for a symbol.
    ///
    /// Fetches order book data from the dYdX Indexer API and converts it to Nautilus
    /// `OrderBookDeltas`. The snapshot is represented as a sequence of deltas starting
    /// with a CLEAR action followed by ADD actions for each level.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails, response cannot be parsed,
    /// or the instrument is not found in the cache.
    pub async fn request_orderbook_snapshot(
        &self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<OrderBookDeltas> {
        let instrument = self
            .get_instrument(&instrument_id)
            .ok_or_else(|| anyhow::anyhow!("Instrument not found in cache: {instrument_id}"))?;

        let ticker = extract_raw_symbol(instrument_id.symbol.as_str());
        let response = self.inner.get_orderbook(ticker).await?;

        let ts_init = self.generate_ts_init();
        let snapshot_flag = RecordFlag::F_SNAPSHOT as u8;

        let mut deltas = Vec::with_capacity(1 + response.bids.len() + response.asks.len());

        // Empty book snapshot: Clear alone must carry F_SNAPSHOT | F_LAST
        if response.bids.is_empty() && response.asks.is_empty() {
            let mut clear_delta = OrderBookDelta::clear(instrument_id, 0, ts_init, ts_init);
            clear_delta.flags = snapshot_flag | RecordFlag::F_LAST as u8;
            deltas.push(clear_delta);
            return Ok(OrderBookDeltas::new(instrument_id, deltas));
        }

        let mut clear_delta = OrderBookDelta::clear(instrument_id, 0, ts_init, ts_init);
        clear_delta.flags = snapshot_flag;
        deltas.push(clear_delta);

        for (i, level) in response.bids.iter().enumerate() {
            let is_last = i == response.bids.len() - 1 && response.asks.is_empty();
            let flags = if is_last {
                snapshot_flag | RecordFlag::F_LAST as u8
            } else {
                snapshot_flag
            };

            let order = BookOrder::new(
                NautilusOrderSide::Buy,
                Price::from_decimal_dp(level.price, instrument.price_precision())?,
                Quantity::from_decimal_dp(level.size, instrument.size_precision())?,
                0,
            );

            deltas.push(OrderBookDelta::new(
                instrument_id,
                BookAction::Add,
                order,
                flags,
                0,
                ts_init,
                ts_init,
            ));
        }

        for (i, level) in response.asks.iter().enumerate() {
            let is_last = i == response.asks.len() - 1;
            let flags = if is_last {
                snapshot_flag | RecordFlag::F_LAST as u8
            } else {
                snapshot_flag
            };

            let order = BookOrder::new(
                NautilusOrderSide::Sell,
                Price::from_decimal_dp(level.price, instrument.price_precision())?,
                Quantity::from_decimal_dp(level.size, instrument.size_precision())?,
                0,
            );

            deltas.push(OrderBookDelta::new(
                instrument_id,
                BookAction::Add,
                order,
                flags,
                0,
                ts_init,
                ts_init,
            ));
        }

        Ok(OrderBookDeltas::new(instrument_id, deltas))
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

    /// Returns `true` if this client is configured for testnet.
    #[must_use]
    pub fn is_testnet(&self) -> bool {
        self.inner.is_testnet()
    }

    /// Returns the base URL used by this client.
    #[must_use]
    pub fn base_url(&self) -> &str {
        self.inner.base_url()
    }

    /// Returns `true` if the instrument cache has been initialized.
    #[must_use]
    pub fn is_cache_initialized(&self) -> bool {
        self.instrument_cache.is_initialized()
    }

    /// Returns the number of instruments currently cached.
    #[must_use]
    pub fn cached_instruments_count(&self) -> usize {
        self.instrument_cache.len()
    }

    /// Returns a reference to the shared instrument cache.
    ///
    /// The cache provides lookups by symbol, market ticker, and clob_pair_id.
    #[must_use]
    pub fn instrument_cache(&self) -> &Arc<InstrumentCache> {
        &self.instrument_cache
    }

    /// Returns all cached instruments.
    ///
    /// This is a convenience method that collects all instruments into a Vec.
    #[must_use]
    pub fn all_instruments(&self) -> Vec<InstrumentAny> {
        self.instrument_cache.all_instruments()
    }

    /// Returns all cached instrument IDs.
    #[must_use]
    pub fn all_instrument_ids(&self) -> Vec<InstrumentId> {
        self.instrument_cache.all_instrument_ids()
    }

    fn generate_ts_init(&self) -> UnixNanos {
        self.clock.get_time_ns()
    }

    /// Requests order status reports for a subaccount.
    ///
    /// Fetches orders from the dYdX Indexer API and converts them to Nautilus
    /// `OrderStatusReport` objects.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or parsing fails.
    pub async fn request_order_status_reports(
        &self,
        address: &str,
        subaccount_number: u32,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let ts_init = self.generate_ts_init();

        // Convert instrument_id to market filter
        let market = instrument_id.map(|id| {
            let symbol = id.symbol.to_string();
            // Remove -PERP suffix if present to get the dYdX market format (e.g., ETH-USD)
            symbol.trim_end_matches("-PERP").to_string()
        });

        let orders = self
            .inner
            .get_orders(address, subaccount_number, market.as_deref(), None)
            .await?;

        let mut reports = Vec::new();

        for order in orders {
            // Get instrument by clob_pair_id
            let instrument = match self.get_instrument_by_clob_id(order.clob_pair_id) {
                Some(inst) => inst,
                None => {
                    log::warn!(
                        "Skipping order {}: no cached instrument for clob_pair_id {}",
                        order.id,
                        order.clob_pair_id
                    );
                    continue;
                }
            };

            // Filter by instrument_id if specified
            if instrument_id.is_some_and(|filter_id| instrument.id() != filter_id) {
                continue;
            }

            match super::parse::parse_order_status_report(&order, &instrument, account_id, ts_init)
            {
                Ok(report) => reports.push(report),
                Err(e) => {
                    log::warn!("Failed to parse order {}: {e}", order.id);
                }
            }
        }

        Ok(reports)
    }

    /// Requests fill reports for a subaccount.
    ///
    /// Fetches fills from the dYdX Indexer API and converts them to Nautilus
    /// `FillReport` objects.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or parsing fails.
    pub async fn request_fill_reports(
        &self,
        address: &str,
        subaccount_number: u32,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
    ) -> anyhow::Result<Vec<FillReport>> {
        let ts_init = self.generate_ts_init();

        // Convert instrument_id to market filter
        let market = instrument_id.map(|id| {
            let symbol = id.symbol.to_string();
            symbol.trim_end_matches("-PERP").to_string()
        });

        let fills_response = self
            .inner
            .get_fills(address, subaccount_number, market.as_deref(), None)
            .await?;

        let mut reports = Vec::new();

        for fill in fills_response.fills {
            // Get instrument by market ticker (e.g., "BTC-USD")
            let instrument = match self.get_instrument_by_market(&fill.market) {
                Some(inst) => inst,
                None => {
                    log::warn!(
                        "Skipping fill {}: no cached instrument for market {}",
                        fill.id,
                        fill.market
                    );
                    continue;
                }
            };

            // Filter by instrument_id if specified
            if instrument_id.is_some_and(|filter_id| instrument.id() != filter_id) {
                continue;
            }

            match super::parse::parse_fill_report(&fill, &instrument, account_id, ts_init) {
                Ok(report) => reports.push(report),
                Err(e) => {
                    log::warn!("Failed to parse fill {}: {e}", fill.id);
                }
            }
        }

        Ok(reports)
    }

    /// Requests position status reports for a subaccount.
    ///
    /// Fetches positions from the dYdX Indexer API and converts them to Nautilus
    /// `PositionStatusReport` objects.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or parsing fails.
    pub async fn request_position_status_reports(
        &self,
        address: &str,
        subaccount_number: u32,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let ts_init = self.generate_ts_init();

        let subaccount_response = self
            .inner
            .get_subaccount(address, subaccount_number)
            .await?;

        let mut reports = Vec::new();

        for (market, position) in subaccount_response.subaccount.open_perpetual_positions {
            // Get instrument by market ticker (e.g., "BTC-USD")
            let instrument = match self.get_instrument_by_market(&market) {
                Some(inst) => inst,
                None => {
                    log::warn!("Skipping position: no cached instrument for market {market}");
                    continue;
                }
            };

            // Filter by instrument_id if specified
            if instrument_id.is_some_and(|filter_id| instrument.id() != filter_id) {
                continue;
            }

            match super::parse::parse_position_status_report(
                &position,
                &instrument,
                account_id,
                ts_init,
            ) {
                Ok(report) => reports.push(report),
                Err(e) => {
                    log::warn!("Failed to parse position for {market}: {e}");
                }
            }
        }

        Ok(reports)
    }

    /// Requests account state for a subaccount.
    ///
    /// Fetches the subaccount from the dYdX Indexer API and converts it to a Nautilus
    /// `AccountState` with balances and margin calculations.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or parsing fails.
    pub async fn request_account_state(
        &self,
        address: &str,
        subaccount_number: u32,
        account_id: AccountId,
    ) -> anyhow::Result<AccountState> {
        let ts_init = self.generate_ts_init();
        let subaccount_response = self
            .inner
            .get_subaccount(address, subaccount_number)
            .await?;

        // Build instruments map from cache
        let instruments: HashMap<InstrumentId, InstrumentAny> = self
            .instrument_cache
            .all_instruments()
            .into_iter()
            .map(|inst| (inst.id(), inst))
            .collect();

        // Use current oracle prices from instrument cache (updated via WS)
        let oracle_prices = self.instrument_cache.to_oracle_prices_map();

        parse_account_state_from_http(
            &subaccount_response.subaccount,
            account_id,
            &instruments,
            &oracle_prices,
            ts_init,
            ts_init,
        )
    }
}

#[cfg(test)]
mod tests {
    use axum::{Router, routing::get};
    use nautilus_model::identifiers::{Symbol, Venue};
    use rstest::rstest;

    use super::*;
    use crate::http::error;

    #[tokio::test]
    async fn test_raw_client_creation() {
        let client = DydxRawHttpClient::new(None, 30, None, DydxNetwork::Mainnet, None);
        assert!(client.is_ok());

        let client = client.unwrap();
        assert!(!client.is_testnet());
        assert_eq!(client.base_url(), DYDX_HTTP_URL);
    }

    #[tokio::test]
    async fn test_raw_client_testnet() {
        let client = DydxRawHttpClient::new(None, 30, None, DydxNetwork::Testnet, None);
        assert!(client.is_ok());

        let client = client.unwrap();
        assert!(client.is_testnet());
        assert_eq!(client.base_url(), DYDX_TESTNET_HTTP_URL);
    }

    #[tokio::test]
    async fn test_domain_client_creation() {
        let client = DydxHttpClient::new(None, 30, None, DydxNetwork::Mainnet, None);
        assert!(client.is_ok());

        let client = client.unwrap();
        assert!(!client.is_testnet());
        assert_eq!(client.base_url(), DYDX_HTTP_URL);
        assert!(!client.is_cache_initialized());
        assert_eq!(client.cached_instruments_count(), 0);
    }

    #[tokio::test]
    async fn test_domain_client_testnet() {
        let client = DydxHttpClient::new(None, 30, None, DydxNetwork::Testnet, None);
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
        let client = DydxHttpClient::new(None, 30, None, DydxNetwork::Mainnet, None).unwrap();

        // Clone before initialization
        let cloned = client.clone();
        assert!(!cloned.is_cache_initialized());

        client.instrument_cache.insert_instruments_only(vec![]);

        // Clone after initialization
        #[expect(clippy::redundant_clone)]
        let cloned_after = client.clone();
        assert!(cloned_after.is_cache_initialized());
    }

    #[rstest]
    fn test_domain_client_get_instrument_not_found() {
        let client = DydxHttpClient::default();
        let instrument_id = InstrumentId::new(Symbol::new("ETH-USD-PERP"), Venue::new("DYDX"));
        let result = client.get_instrument(&instrument_id);
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_http_timeout_respects_configuration_and_does_not_block() {
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

        let base_url = format!("http://{addr}");

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
        let client = DydxRawHttpClient::new(
            Some(base_url),
            60,
            None,
            DydxNetwork::Mainnet,
            Some(retry_config),
        )
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
