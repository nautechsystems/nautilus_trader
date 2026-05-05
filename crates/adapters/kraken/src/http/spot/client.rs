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

//! HTTP client for the Kraken Spot REST API.

use std::{
    collections::HashMap,
    fmt::Debug,
    num::NonZeroU32,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::AHashMap;
use anyhow::Context;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use nautilus_core::{
    AtomicMap, AtomicTime, UUID4, consts::NAUTILUS_USER_AGENT, datetime::NANOSECONDS_IN_SECOND,
    nanos::UnixNanos, time::get_atomic_clock_realtime,
};
use nautilus_model::{
    data::{Bar, BarType, BookOrder, TradeTick},
    enums::{
        AccountType, BookType, CurrencyType, MarketStatusAction, OrderSide, OrderType,
        PositionSideSpecified, TimeInForce, TriggerType,
    },
    events::AccountState,
    identifiers::{AccountId, ClientOrderId, InstrumentId, Symbol, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};
use nautilus_network::{
    http::{HttpClient, Method, USER_AGENT},
    ratelimiter::quota::Quota,
    retry::{RetryConfig, RetryManager},
};
use rust_decimal::Decimal;
use serde::de::DeserializeOwned;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{models::*, query::*};
use crate::{
    common::{
        consts::{
            KRAKEN_OFLAG_POST_ONLY, KRAKEN_OFLAG_QUOTE_QUANTITY, KRAKEN_VENUE,
            NAUTILUS_KRAKEN_BROKER_ID,
        },
        credential::KrakenCredential,
        enums::{
            KrakenAssetClass, KrakenEnvironment, KrakenOrderSide, KrakenOrderType,
            KrakenProductType,
        },
        parse::{
            bar_type_to_spot_interval, normalize_currency_code, normalize_spot_symbol, parse_bar,
            parse_fill_report, parse_order_status_report, parse_spot_instrument,
            parse_tokenized_instrument, parse_trade_tick_from_array, truncate_cl_ord_id,
        },
        urls::get_kraken_http_base_url,
    },
    http::error::{KrakenHttpError, kraken_http_should_retry},
};

/// Default Kraken Spot REST API rate limit (requests per second).
pub const KRAKEN_SPOT_DEFAULT_RATE_LIMIT_PER_SECOND: u32 = 5;

const KRAKEN_GLOBAL_RATE_KEY: &str = "kraken:spot:global";

/// Maximum orders per batch cancel request for Kraken Spot API.
const BATCH_CANCEL_LIMIT: usize = 50;

/// Maximum orders per batch submit request for Kraken Spot API.
const BATCH_SUBMIT_LIMIT: usize = 15;

/// Raw HTTP client for low-level Kraken Spot API operations.
///
/// This client handles request/response operations with the Kraken Spot API,
/// returning venue-specific response types. It does not parse to Nautilus domain types.
pub struct KrakenSpotRawHttpClient {
    base_url: String,
    client: HttpClient,
    credential: Option<KrakenCredential>,
    retry_manager: RetryManager<KrakenHttpError>,
    cancellation_token: CancellationToken,
    clock: &'static AtomicTime,
    /// Mutex to serialize authenticated requests, ensuring nonces arrive at Kraken in order
    auth_mutex: tokio::sync::Mutex<()>,
}

impl Default for KrakenSpotRawHttpClient {
    fn default() -> Self {
        Self::new(
            KrakenEnvironment::Live,
            None,
            60,
            None,
            None,
            None,
            None,
            KRAKEN_SPOT_DEFAULT_RATE_LIMIT_PER_SECOND,
        )
        .expect("Failed to create default KrakenSpotRawHttpClient")
    }
}

impl Debug for KrakenSpotRawHttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(KrakenSpotRawHttpClient))
            .field("base_url", &self.base_url)
            .field("has_credentials", &self.credential.is_some())
            .finish()
    }
}

impl KrakenSpotRawHttpClient {
    /// Creates a new [`KrakenSpotRawHttpClient`].
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
            get_kraken_http_base_url(KrakenProductType::Spot, environment).to_string()
        });

        Ok(Self {
            base_url,
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                Self::rate_limiter_quotas(max_requests_per_second)?,
                Some(Self::default_quota(max_requests_per_second)?),
                Some(timeout_secs),
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

    /// Creates a new [`KrakenSpotRawHttpClient`] with credentials.
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
            get_kraken_http_base_url(KrakenProductType::Spot, environment).to_string()
        });

        Ok(Self {
            base_url,
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                Self::rate_limiter_quotas(max_requests_per_second)?,
                Some(Self::default_quota(max_requests_per_second)?),
                Some(timeout_secs),
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

    /// Generates a unique nonce for Kraken Spot API requests.
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

    fn default_quota(max_requests_per_second: u32) -> anyhow::Result<Quota> {
        let burst = NonZeroU32::new(max_requests_per_second).unwrap_or(
            NonZeroU32::new(KRAKEN_SPOT_DEFAULT_RATE_LIMIT_PER_SECOND).expect("non-zero"),
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
        let route = format!("kraken:spot:{normalized}");
        vec![KRAKEN_GLOBAL_RATE_KEY.to_string(), route]
    }

    fn sign_spot(
        &self,
        path: &str,
        nonce: u64,
        params: &HashMap<String, String>,
    ) -> anyhow::Result<(HashMap<String, String>, String)> {
        let credential = self
            .credential
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing credentials"))?;

        let (signature, post_data) = credential.sign_spot(path, nonce, params)?;

        let mut headers = HashMap::new();
        headers.insert("API-Key".to_string(), credential.api_key().to_string());
        headers.insert("API-Sign".to_string(), signature);

        Ok((headers, post_data))
    }

    async fn send_request<T: DeserializeOwned>(
        &self,
        method: Method,
        endpoint: &str,
        body: Option<Vec<u8>>,
        authenticate: bool,
    ) -> anyhow::Result<KrakenResponse<T>, KrakenHttpError> {
        // Serialize authenticated requests to ensure nonces arrive at Kraken in order.
        // Without this, concurrent requests can race through the network and arrive
        // out-of-order, causing "Invalid nonce" errors.
        let _guard = if authenticate {
            Some(self.auth_mutex.lock().await)
        } else {
            None
        };

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
                let mut headers = Self::default_headers();

                let final_body = if authenticate {
                    let nonce = self.generate_nonce();
                    log::debug!("Generated nonce {nonce} for {endpoint}");

                    let params: HashMap<String, String> = if let Some(ref body_bytes) = body {
                        let body_str = std::str::from_utf8(body_bytes).map_err(|e| {
                            KrakenHttpError::ParseError(format!(
                                "Invalid UTF-8 in request body: {e}"
                            ))
                        })?;
                        serde_urlencoded::from_str(body_str).map_err(|e| {
                            KrakenHttpError::ParseError(format!(
                                "Failed to parse request params: {e}"
                            ))
                        })?
                    } else {
                        HashMap::new()
                    };

                    let (auth_headers, post_data) = self
                        .sign_spot(&endpoint, nonce, &params)
                        .map_err(|e| KrakenHttpError::NetworkError(e.to_string()))?;
                    headers.extend(auth_headers);
                    Some(post_data.into_bytes())
                } else {
                    body
                };

                if method == Method::POST {
                    headers.insert(
                        "Content-Type".to_string(),
                        "application/x-www-form-urlencoded".to_string(),
                    );
                }

                let rate_limit_keys = Self::rate_limit_keys(&endpoint);

                let response = self
                    .client
                    .request(
                        method,
                        url,
                        None,
                        Some(headers),
                        final_body,
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

                let kraken_response: KrakenResponse<T> = serde_json::from_str(&response_text)
                    .map_err(|e| {
                        KrakenHttpError::ParseError(format!("Failed to deserialize response: {e}"))
                    })?;

                if !kraken_response.error.is_empty() {
                    return Err(KrakenHttpError::ApiError(kraken_response.error));
                }

                Ok(kraken_response)
            }
        };

        let should_retry = kraken_http_should_retry;
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

    /// Requests the server time from Kraken.
    pub async fn get_server_time(&self) -> anyhow::Result<ServerTime, KrakenHttpError> {
        let response: KrakenResponse<ServerTime> = self
            .send_request(Method::GET, "/0/public/Time", None, false)
            .await?;

        response.result.ok_or_else(|| {
            KrakenHttpError::ParseError("Missing result in server time response".to_string())
        })
    }

    /// Requests the system status from Kraken.
    pub async fn get_system_status(&self) -> anyhow::Result<SystemStatus, KrakenHttpError> {
        let response: KrakenResponse<SystemStatus> = self
            .send_request(Method::GET, "/0/public/SystemStatus", None, false)
            .await?;

        response.result.ok_or_else(|| {
            KrakenHttpError::ParseError("Missing result in system status response".to_string())
        })
    }

    /// Requests tradable asset pairs from Kraken.
    ///
    /// When `aclass_base` is `None`, the Kraken API defaults to `"currency"` (crypto pairs).
    /// Pass `"tokenized_asset"` to fetch tokenized equities (xStocks).
    pub async fn get_asset_pairs(
        &self,
        pairs: Option<Vec<String>>,
        aclass_base: Option<&str>,
    ) -> anyhow::Result<AssetPairsResponse, KrakenHttpError> {
        let mut params = Vec::new();

        if let Some(pairs) = pairs {
            params.push(format!("pair={}", pairs.join(",")));
        }

        if let Some(aclass) = aclass_base {
            params.push(format!("aclass_base={aclass}"));
        }

        let endpoint = if params.is_empty() {
            "/0/public/AssetPairs".to_string()
        } else {
            format!("/0/public/AssetPairs?{}", params.join("&"))
        };

        let response: KrakenResponse<AssetPairsResponse> = self
            .send_request(Method::GET, &endpoint, None, false)
            .await?;

        response.result.ok_or_else(|| {
            KrakenHttpError::ParseError("Missing result in asset pairs response".to_string())
        })
    }

    /// Requests ticker information for asset pairs.
    pub async fn get_ticker(
        &self,
        pairs: Vec<String>,
        asset_class: Option<KrakenAssetClass>,
    ) -> anyhow::Result<TickerResponse, KrakenHttpError> {
        let mut endpoint = format!("/0/public/Ticker?pair={}", pairs.join(","));

        if let Some(aclass) = asset_class {
            endpoint.push_str(&format!("&asset_class={aclass}"));
        }

        let response: KrakenResponse<TickerResponse> = self
            .send_request(Method::GET, &endpoint, None, false)
            .await?;

        response.result.ok_or_else(|| {
            KrakenHttpError::ParseError("Missing result in ticker response".to_string())
        })
    }

    /// Requests OHLC candlestick data for an asset pair.
    pub async fn get_ohlc(
        &self,
        pair: &str,
        interval: Option<u32>,
        since: Option<i64>,
        asset_class: Option<KrakenAssetClass>,
    ) -> anyhow::Result<OhlcResponse, KrakenHttpError> {
        let mut endpoint = format!("/0/public/OHLC?pair={pair}");

        if let Some(aclass) = asset_class {
            endpoint.push_str(&format!("&asset_class={aclass}"));
        }

        if let Some(interval) = interval {
            endpoint.push_str(&format!("&interval={interval}"));
        }

        if let Some(since) = since {
            endpoint.push_str(&format!("&since={since}"));
        }

        let response: KrakenResponse<OhlcResponse> = self
            .send_request(Method::GET, &endpoint, None, false)
            .await?;

        response.result.ok_or_else(|| {
            KrakenHttpError::ParseError("Missing result in OHLC response".to_string())
        })
    }

    /// Requests order book depth for an asset pair.
    pub async fn get_book_depth(
        &self,
        pair: &str,
        count: Option<u32>,
        asset_class: Option<KrakenAssetClass>,
    ) -> anyhow::Result<OrderBookResponse, KrakenHttpError> {
        let mut endpoint = format!("/0/public/Depth?pair={pair}");

        if let Some(aclass) = asset_class {
            endpoint.push_str(&format!("&asset_class={aclass}"));
        }

        if let Some(count) = count {
            endpoint.push_str(&format!("&count={count}"));
        }

        let response: KrakenResponse<OrderBookResponse> = self
            .send_request(Method::GET, &endpoint, None, false)
            .await?;

        response.result.ok_or_else(|| {
            KrakenHttpError::ParseError("Missing result in book depth response".to_string())
        })
    }

    /// Requests recent trades for an asset pair.
    pub async fn get_trades(
        &self,
        pair: &str,
        since: Option<String>,
        asset_class: Option<KrakenAssetClass>,
    ) -> anyhow::Result<TradesResponse, KrakenHttpError> {
        let mut endpoint = format!("/0/public/Trades?pair={pair}");

        if let Some(aclass) = asset_class {
            endpoint.push_str(&format!("&asset_class={aclass}"));
        }

        if let Some(since) = since {
            endpoint.push_str(&format!("&since={since}"));
        }

        let response: KrakenResponse<TradesResponse> = self
            .send_request(Method::GET, &endpoint, None, false)
            .await?;

        response.result.ok_or_else(|| {
            KrakenHttpError::ParseError("Missing result in trades response".to_string())
        })
    }

    /// Requests an authentication token for WebSocket connections.
    pub async fn get_websockets_token(&self) -> anyhow::Result<WebSocketToken, KrakenHttpError> {
        if self.credential.is_none() {
            return Err(KrakenHttpError::AuthenticationError(
                "API credentials required for GetWebSocketsToken".to_string(),
            ));
        }

        let response: KrakenResponse<WebSocketToken> = self
            .send_request(Method::POST, "/0/private/GetWebSocketsToken", None, true)
            .await?;

        response.result.ok_or_else(|| {
            KrakenHttpError::ParseError("Missing result in websockets token response".to_string())
        })
    }

    /// Requests all open orders (requires authentication).
    pub async fn get_open_orders(
        &self,
        trades: Option<bool>,
        userref: Option<i64>,
    ) -> anyhow::Result<IndexMap<String, SpotOrder>, KrakenHttpError> {
        if self.credential.is_none() {
            return Err(KrakenHttpError::AuthenticationError(
                "API credentials required for OpenOrders".to_string(),
            ));
        }

        let mut params = vec![];

        if let Some(trades_flag) = trades {
            params.push(format!("trades={trades_flag}"));
        }

        if let Some(userref_val) = userref {
            params.push(format!("userref={userref_val}"));
        }

        let body = if params.is_empty() {
            None
        } else {
            Some(params.join("&").into_bytes())
        };

        let response: KrakenResponse<SpotOpenOrdersResult> = self
            .send_request(Method::POST, "/0/private/OpenOrders", body, true)
            .await?;

        let result = response.result.ok_or_else(|| {
            KrakenHttpError::ParseError("Missing result in open orders response".to_string())
        })?;

        Ok(result.open)
    }

    /// Requests closed orders history (requires authentication).
    pub async fn get_closed_orders(
        &self,
        trades: Option<bool>,
        userref: Option<i64>,
        start: Option<i64>,
        end: Option<i64>,
        ofs: Option<i32>,
        closetime: Option<String>,
    ) -> anyhow::Result<IndexMap<String, SpotOrder>, KrakenHttpError> {
        if self.credential.is_none() {
            return Err(KrakenHttpError::AuthenticationError(
                "API credentials required for ClosedOrders".to_string(),
            ));
        }

        let mut params = vec![];

        if let Some(trades_flag) = trades {
            params.push(format!("trades={trades_flag}"));
        }

        if let Some(userref_val) = userref {
            params.push(format!("userref={userref_val}"));
        }

        if let Some(start_val) = start {
            params.push(format!("start={start_val}"));
        }

        if let Some(end_val) = end {
            params.push(format!("end={end_val}"));
        }

        if let Some(ofs_val) = ofs {
            params.push(format!("ofs={ofs_val}"));
        }

        if let Some(closetime_val) = closetime {
            params.push(format!("closetime={closetime_val}"));
        }

        let body = if params.is_empty() {
            None
        } else {
            Some(params.join("&").into_bytes())
        };

        let response: KrakenResponse<SpotClosedOrdersResult> = self
            .send_request(Method::POST, "/0/private/ClosedOrders", body, true)
            .await?;

        let result = response.result.ok_or_else(|| {
            KrakenHttpError::ParseError("Missing result in closed orders response".to_string())
        })?;

        Ok(result.closed)
    }

    /// Requests trades history (requires authentication).
    pub async fn get_trades_history(
        &self,
        trade_type: Option<String>,
        trades: Option<bool>,
        start: Option<i64>,
        end: Option<i64>,
        ofs: Option<i32>,
    ) -> anyhow::Result<IndexMap<String, SpotTrade>, KrakenHttpError> {
        if self.credential.is_none() {
            return Err(KrakenHttpError::AuthenticationError(
                "API credentials required for TradesHistory".to_string(),
            ));
        }

        let mut params = vec![];

        if let Some(type_val) = trade_type {
            params.push(format!("type={type_val}"));
        }

        if let Some(trades_flag) = trades {
            params.push(format!("trades={trades_flag}"));
        }

        if let Some(start_val) = start {
            params.push(format!("start={start_val}"));
        }

        if let Some(end_val) = end {
            params.push(format!("end={end_val}"));
        }

        if let Some(ofs_val) = ofs {
            params.push(format!("ofs={ofs_val}"));
        }

        let body = if params.is_empty() {
            None
        } else {
            Some(params.join("&").into_bytes())
        };

        let response: KrakenResponse<SpotTradesHistoryResult> = self
            .send_request(Method::POST, "/0/private/TradesHistory", body, true)
            .await?;

        let result = response.result.ok_or_else(|| {
            KrakenHttpError::ParseError("Missing result in trades history response".to_string())
        })?;

        Ok(result.trades)
    }

    /// Submits a new order (requires authentication).
    pub async fn add_order(
        &self,
        params: &KrakenSpotAddOrderParams,
    ) -> anyhow::Result<SpotAddOrderResponse, KrakenHttpError> {
        if self.credential.is_none() {
            return Err(KrakenHttpError::AuthenticationError(
                "API credentials required for adding orders".to_string(),
            ));
        }

        let param_string = serde_urlencoded::to_string(params)
            .map_err(|e| KrakenHttpError::ParseError(format!("Failed to encode params: {e}")))?;
        let body = Some(param_string.into_bytes());

        let response: KrakenResponse<SpotAddOrderResponse> = self
            .send_request(Method::POST, "/0/private/AddOrder", body, true)
            .await?;

        response
            .result
            .ok_or_else(|| KrakenHttpError::ParseError("Missing result in response".to_string()))
    }

    /// Submits multiple orders in a single batch request (requires authentication).
    pub async fn add_order_batch(
        &self,
        params: &KrakenSpotAddOrderBatchParams,
    ) -> anyhow::Result<SpotAddOrderBatchResponse, KrakenHttpError> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            KrakenHttpError::AuthenticationError(
                "API credentials required for adding orders".to_string(),
            )
        })?;

        let _guard = self.auth_mutex.lock().await;

        let endpoint = "/0/private/AddOrderBatch";
        let nonce = self.generate_nonce();

        let mut json_body = serde_json::json!({
            "nonce": nonce.to_string(),
            "pair": params.pair,
            "orders": params.orders,
        });

        if let Some(aclass) = &params.asset_class {
            json_body["asset_class"] = serde_json::json!(aclass);
        }
        let json_str = serde_json::to_string(&json_body)
            .map_err(|e| KrakenHttpError::ParseError(format!("Failed to serialize: {e}")))?;

        let signature = credential
            .sign_spot_json(endpoint, nonce, &json_str)
            .map_err(|e| KrakenHttpError::AuthenticationError(format!("Failed to sign: {e}")))?;

        let mut headers = Self::default_headers();
        headers.insert("API-Key".to_string(), credential.api_key().to_string());
        headers.insert("API-Sign".to_string(), signature);
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        let url = format!("{}{endpoint}", self.base_url);
        let rate_limit_keys = Self::rate_limit_keys(endpoint);

        let response = self
            .client
            .request(
                Method::POST,
                url,
                None,
                Some(headers),
                Some(json_str.into_bytes()),
                None,
                Some(rate_limit_keys),
            )
            .await
            .map_err(|e| KrakenHttpError::NetworkError(e.to_string()))?;

        if !response.status.is_success() {
            return Err(KrakenHttpError::NetworkError(format!(
                "HTTP {:?} for {}",
                response.status, endpoint
            )));
        }

        let parsed: KrakenResponse<SpotAddOrderBatchResponse> =
            serde_json::from_slice(&response.body).map_err(|e| {
                KrakenHttpError::ParseError(format!("Failed to parse JSON response: {e}"))
            })?;

        if !parsed.error.is_empty() {
            return Err(KrakenHttpError::ApiError(parsed.error));
        }

        parsed
            .result
            .ok_or_else(|| KrakenHttpError::ParseError("Missing result in response".to_string()))
    }

    /// Cancels an open order (requires authentication).
    pub async fn cancel_order(
        &self,
        params: &KrakenSpotCancelOrderParams,
    ) -> anyhow::Result<SpotCancelOrderResponse, KrakenHttpError> {
        if self.credential.is_none() {
            return Err(KrakenHttpError::AuthenticationError(
                "API credentials required for canceling orders".to_string(),
            ));
        }

        let param_string = serde_urlencoded::to_string(params)
            .map_err(|e| KrakenHttpError::ParseError(format!("Failed to encode params: {e}")))?;

        let body = Some(param_string.into_bytes());

        let response: KrakenResponse<SpotCancelOrderResponse> = self
            .send_request(Method::POST, "/0/private/CancelOrder", body, true)
            .await?;

        response
            .result
            .ok_or_else(|| KrakenHttpError::ParseError("Missing result in response".to_string()))
    }

    /// Cancels multiple orders in a single batch request (max 50 orders).
    pub async fn cancel_order_batch(
        &self,
        params: &KrakenSpotCancelOrderBatchParams,
    ) -> anyhow::Result<SpotCancelOrderBatchResponse, KrakenHttpError> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            KrakenHttpError::AuthenticationError(
                "API credentials required for canceling orders".to_string(),
            )
        })?;

        // Serialize authenticated requests to ensure nonces arrive at Kraken in order
        let _guard = self.auth_mutex.lock().await;

        let endpoint = "/0/private/CancelOrderBatch";
        let nonce = self.generate_nonce();

        // CancelOrderBatch uses JSON body with nonce included
        let json_body = serde_json::json!({
            "nonce": nonce.to_string(),
            "orders": params.orders
        });
        let json_str = serde_json::to_string(&json_body)
            .map_err(|e| KrakenHttpError::ParseError(format!("Failed to serialize: {e}")))?;

        let signature = credential
            .sign_spot_json(endpoint, nonce, &json_str)
            .map_err(|e| KrakenHttpError::AuthenticationError(format!("Failed to sign: {e}")))?;

        let mut headers = Self::default_headers();
        headers.insert("API-Key".to_string(), credential.api_key().to_string());
        headers.insert("API-Sign".to_string(), signature);
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        let url = format!("{}{endpoint}", self.base_url);
        let rate_limit_keys = Self::rate_limit_keys(endpoint);

        let response = self
            .client
            .request(
                Method::POST,
                url,
                None,
                Some(headers),
                Some(json_str.into_bytes()),
                None,
                Some(rate_limit_keys),
            )
            .await
            .map_err(|e| KrakenHttpError::NetworkError(e.to_string()))?;

        if response.status.as_u16() >= 400 {
            let status = response.status.as_u16();
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

        let response_text = String::from_utf8(response.body.to_vec())
            .map_err(|e| KrakenHttpError::ParseError(format!("Invalid UTF-8: {e}")))?;

        let kraken_response: KrakenResponse<SpotCancelOrderBatchResponse> =
            serde_json::from_str(&response_text).map_err(|e| {
                KrakenHttpError::ParseError(format!("Failed to parse response: {e}"))
            })?;

        if !kraken_response.error.is_empty() {
            return Err(KrakenHttpError::ApiError(kraken_response.error));
        }

        kraken_response
            .result
            .ok_or_else(|| KrakenHttpError::ParseError("Missing result in response".to_string()))
    }

    /// Cancels all open orders (requires authentication).
    pub async fn cancel_all_orders(
        &self,
    ) -> anyhow::Result<SpotCancelOrderResponse, KrakenHttpError> {
        if self.credential.is_none() {
            return Err(KrakenHttpError::AuthenticationError(
                "API credentials required for canceling orders".to_string(),
            ));
        }

        let response: KrakenResponse<SpotCancelOrderResponse> = self
            .send_request(Method::POST, "/0/private/CancelAll", None, true)
            .await?;

        response
            .result
            .ok_or_else(|| KrakenHttpError::ParseError("Missing result in response".to_string()))
    }

    /// Edits an existing order (cancel and replace).
    pub async fn edit_order(
        &self,
        params: &KrakenSpotEditOrderParams,
    ) -> anyhow::Result<SpotEditOrderResponse, KrakenHttpError> {
        if self.credential.is_none() {
            return Err(KrakenHttpError::AuthenticationError(
                "API credentials required for editing orders".to_string(),
            ));
        }

        let param_string = serde_urlencoded::to_string(params)
            .map_err(|e| KrakenHttpError::ParseError(format!("Failed to encode params: {e}")))?;

        let body = Some(param_string.into_bytes());

        let response: KrakenResponse<SpotEditOrderResponse> = self
            .send_request(Method::POST, "/0/private/EditOrder", body, true)
            .await?;

        response
            .result
            .ok_or_else(|| KrakenHttpError::ParseError("Missing result in response".to_string()))
    }

    /// Amends an existing order in-place (no cancel/replace).
    pub async fn amend_order(
        &self,
        params: &KrakenSpotAmendOrderParams,
    ) -> anyhow::Result<SpotAmendOrderResponse, KrakenHttpError> {
        if self.credential.is_none() {
            return Err(KrakenHttpError::AuthenticationError(
                "API credentials required for amending orders".to_string(),
            ));
        }

        let param_string = serde_urlencoded::to_string(params)
            .map_err(|e| KrakenHttpError::ParseError(format!("Failed to encode params: {e}")))?;

        let body = Some(param_string.into_bytes());

        let response: KrakenResponse<SpotAmendOrderResponse> = self
            .send_request(Method::POST, "/0/private/AmendOrder", body, true)
            .await?;

        response
            .result
            .ok_or_else(|| KrakenHttpError::ParseError("Missing result in response".to_string()))
    }

    /// Requests account balances (requires authentication).
    pub async fn get_balance(&self) -> anyhow::Result<BalanceResponse, KrakenHttpError> {
        if self.credential.is_none() {
            return Err(KrakenHttpError::AuthenticationError(
                "API credentials required for Balance".to_string(),
            ));
        }

        let response: KrakenResponse<BalanceResponse> = self
            .send_request(Method::POST, "/0/private/Balance", None, true)
            .await?;

        response.result.ok_or_else(|| {
            KrakenHttpError::ParseError("Missing result in balance response".to_string())
        })
    }

    /// Requests margin account summary (requires authentication).
    ///
    /// Unlike `get_balance` which returns per-currency wallet amounts, this returns margin
    /// accounting metrics: used margin, free margin, equity, all denominated in `asset`
    /// (defaults to `"ZUSD"` when `None`). Only meaningful for spot margin accounts.
    pub async fn get_trade_balance(
        &self,
        asset: Option<&str>,
    ) -> anyhow::Result<TradeBalanceResponse, KrakenHttpError> {
        if self.credential.is_none() {
            return Err(KrakenHttpError::AuthenticationError(
                "API credentials required for TradeBalance".to_string(),
            ));
        }

        let params = asset.map(|a| SpotTradeBalanceParams {
            asset: Some(a.to_string()),
        });

        let body = params
            .as_ref()
            .and_then(|p| serde_urlencoded::to_string(p).ok())
            .map(|s| s.into_bytes());

        let response: KrakenResponse<TradeBalanceResponse> = self
            .send_request(Method::POST, "/0/private/TradeBalance", body, true)
            .await?;

        response.result.ok_or_else(|| {
            KrakenHttpError::ParseError("Missing result in TradeBalance response".to_string())
        })
    }

    /// Requests open spot margin positions (requires authentication).
    pub async fn get_open_positions(
        &self,
        params: &SpotOpenPositionsParams,
    ) -> anyhow::Result<SpotOpenPositionsResponse, KrakenHttpError> {
        if self.credential.is_none() {
            return Err(KrakenHttpError::AuthenticationError(
                "API credentials required for OpenPositions".to_string(),
            ));
        }

        let body = serde_urlencoded::to_string(params)
            .ok()
            .filter(|s| !s.is_empty())
            .map(|s| s.into_bytes());

        let response: KrakenResponse<SpotOpenPositionsResponse> = self
            .send_request(Method::POST, "/0/private/OpenPositions", body, true)
            .await?;

        response.result.ok_or_else(|| {
            KrakenHttpError::ParseError("Missing result in OpenPositions response".to_string())
        })
    }
}

/// High-level HTTP client for the Kraken Spot REST API.
///
/// This client wraps the raw client and provides Nautilus domain types.
/// It maintains an instrument cache and uses it to parse venue responses
/// into Nautilus domain objects.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.kraken", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.kraken")
)]
pub struct KrakenSpotHttpClient {
    pub(crate) inner: Arc<KrakenSpotRawHttpClient>,
    pub(crate) instruments_cache: Arc<AtomicMap<Ustr, InstrumentAny>>,
    leverage_tiers_cache: LeverageTiersCache,
    clock: &'static AtomicTime,
    cache_initialized: Arc<AtomicBool>,
}

impl Clone for KrakenSpotHttpClient {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            instruments_cache: self.instruments_cache.clone(),
            leverage_tiers_cache: self.leverage_tiers_cache.clone(),
            cache_initialized: self.cache_initialized.clone(),
            clock: self.clock,
        }
    }
}

impl Default for KrakenSpotHttpClient {
    fn default() -> Self {
        Self::new(
            KrakenEnvironment::Live,
            None,
            60,
            None,
            None,
            None,
            None,
            KRAKEN_SPOT_DEFAULT_RATE_LIMIT_PER_SECOND,
        )
        .expect("Failed to create default KrakenSpotHttpClient")
    }
}

impl Debug for KrakenSpotHttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(KrakenSpotHttpClient))
            .field("inner", &self.inner)
            .finish()
    }
}

impl KrakenSpotHttpClient {
    /// Creates a new [`KrakenSpotHttpClient`].
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
            inner: Arc::new(KrakenSpotRawHttpClient::new(
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
            leverage_tiers_cache: Arc::new(AtomicMap::new()),
            cache_initialized: Arc::new(AtomicBool::new(false)),
            clock: get_atomic_clock_realtime(),
        })
    }

    /// Creates a new [`KrakenSpotHttpClient`] with credentials.
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
            inner: Arc::new(KrakenSpotRawHttpClient::with_credentials(
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
            leverage_tiers_cache: Arc::new(AtomicMap::new()),
            cache_initialized: Arc::new(AtomicBool::new(false)),
            clock: get_atomic_clock_realtime(),
        })
    }

    /// Creates a new [`KrakenSpotHttpClient`] loading credentials from environment variables.
    ///
    /// Looks for `KRAKEN_SPOT_API_KEY` and `KRAKEN_SPOT_API_SECRET`.
    ///
    /// Note: Kraken Spot does not have a testnet/demo environment.
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
        if let Some(credential) = KrakenCredential::from_env_spot() {
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

    fn generate_ts_init(&self) -> UnixNanos {
        self.clock.get_time_ns()
    }

    // Kraken requires `asset_class=tokenized_asset` on every request that references a tokenized pair.
    fn asset_class_for(instrument: &InstrumentAny) -> Option<KrakenAssetClass> {
        if matches!(instrument, InstrumentAny::TokenizedAsset(_)) {
            Some(KrakenAssetClass::TokenizedAsset)
        } else {
            None
        }
    }

    /// Requests an authentication token for WebSocket connections.
    pub async fn get_websockets_token(&self) -> anyhow::Result<WebSocketToken, KrakenHttpError> {
        self.inner.get_websockets_token().await
    }

    /// Requests tradable instruments from Kraken.
    ///
    /// When `pairs` is `None` (loading all), also fetches tokenized asset pairs
    /// (xStocks) and merges them with the default currency pairs.
    pub async fn request_instruments(
        &self,
        pairs: Option<Vec<String>>,
    ) -> anyhow::Result<Vec<InstrumentAny>, KrakenHttpError> {
        let ts_init = self.generate_ts_init();
        let asset_pairs = self.inner.get_asset_pairs(pairs.clone(), None).await?;

        let mut instruments: Vec<InstrumentAny> = asset_pairs
            .iter()
            .filter_map(|(pair_name, definition)| {
                match parse_spot_instrument(pair_name, definition, ts_init, ts_init) {
                    Ok(instrument) => Some((instrument, definition)),
                    Err(e) => {
                        log::warn!("Failed to parse instrument {pair_name}: {e}");
                        None
                    }
                }
            })
            .map(|(instrument, definition)| {
                let key = Ustr::from(instrument.raw_symbol().as_str());
                let tiers = (
                    definition.leverage_buy.clone(),
                    definition.leverage_sell.clone(),
                );
                self.leverage_tiers_cache.rcu(|m| {
                    m.insert(key, tiers.clone());
                });
                instrument
            })
            .collect();

        // Also fetch tokenized asset pairs (xStocks). When loading all pairs this
        // picks up tokenized equities; when loading specific pairs it covers the
        // case where the requested symbols are tokenized assets.
        {
            match self
                .inner
                .get_asset_pairs(pairs, Some("tokenized_asset"))
                .await
            {
                Ok(tokenized_pairs) => {
                    if !tokenized_pairs.is_empty() {
                        log::info!("Fetched {} tokenized asset pairs", tokenized_pairs.len());
                    }
                    let tokenized_instruments: Vec<InstrumentAny> =
                        tokenized_pairs
                            .iter()
                            .filter_map(|(pair_name, definition)| match parse_tokenized_instrument(
                                pair_name, definition, ts_init, ts_init,
                            ) {
                                Ok(instrument) => Some(instrument),
                                Err(e) => {
                                    log::warn!(
                                        "Failed to parse tokenized instrument {pair_name}: {e}"
                                    );
                                    None
                                }
                            })
                            .collect();
                    instruments.extend(tokenized_instruments);
                }
                Err(e) => {
                    log::warn!("Failed to fetch tokenized asset pairs: {e}");
                }
            }
        }

        Ok(instruments)
    }

    /// Requests the current market status for Kraken Spot instruments.
    ///
    /// Fetches both regular and tokenized asset pairs. The call returns an error if
    /// either fetch fails so callers can avoid emitting partial snapshots that would
    /// otherwise cause the missing tokenized symbols to be diffed as removed.
    pub async fn request_instrument_statuses(
        &self,
        pairs: Option<Vec<String>>,
    ) -> anyhow::Result<AHashMap<InstrumentId, MarketStatusAction>, KrakenHttpError> {
        let asset_pairs = self.inner.get_asset_pairs(pairs.clone(), None).await?;
        let mut statuses = collect_spot_statuses(&asset_pairs);

        let tokenized_pairs = self
            .inner
            .get_asset_pairs(pairs, Some("tokenized_asset"))
            .await?;
        statuses.extend(collect_spot_statuses(&tokenized_pairs));

        Ok(statuses)
    }

    /// Requests historical trades for an instrument.
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
                    "Instrument not found in cache: {instrument_id}",
                ))
            })?;

        let raw_symbol = instrument.raw_symbol().to_string();
        let asset_class = Self::asset_class_for(&instrument);
        let ts_init = self.generate_ts_init();

        // Kraken trades API expects nanoseconds since epoch as string
        let since = start.map(|dt| (dt.timestamp_nanos_opt().unwrap_or(0) as u64).to_string());
        let response = self
            .inner
            .get_trades(&raw_symbol, since, asset_class)
            .await?;

        let end_ns = end.map(|dt| dt.timestamp_nanos_opt().unwrap_or(0) as u64);
        let mut trades = Vec::new();

        for (_pair_name, trade_arrays) in &response.data {
            for trade_array in trade_arrays {
                match parse_trade_tick_from_array(trade_array, &instrument, ts_init) {
                    Ok(trade_tick) => {
                        if let Some(end_nanos) = end_ns
                            && trade_tick.ts_event.as_u64() > end_nanos
                        {
                            continue;
                        }
                        trades.push(trade_tick);

                        if let Some(limit_count) = limit
                            && trades.len() >= limit_count as usize
                        {
                            return Ok(trades);
                        }
                    }
                    Err(e) => {
                        log::warn!("Failed to parse trade tick: {e}");
                    }
                }
            }
        }

        Ok(trades)
    }

    /// Requests historical bars/OHLC data for an instrument.
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
        let asset_class = Self::asset_class_for(&instrument);
        let ts_init = self.generate_ts_init();

        let interval = Some(
            bar_type_to_spot_interval(bar_type)
                .map_err(|e| KrakenHttpError::ParseError(e.to_string()))?,
        );

        // Kraken OHLC API expects Unix timestamp in seconds
        let since = start.map(|dt| dt.timestamp());
        let end_ns = end.map(|dt| dt.timestamp_nanos_opt().unwrap_or(0) as u64);
        let response = self
            .inner
            .get_ohlc(&raw_symbol, interval, since, asset_class)
            .await?;

        let mut bars = Vec::new();

        for (_pair_name, ohlc_arrays) in &response.data {
            for ohlc_array in ohlc_arrays {
                if ohlc_array.len() < 8 {
                    let len = ohlc_array.len();
                    log::warn!("OHLC array too short: {len}");
                    continue;
                }

                let ohlc = OhlcData {
                    time: ohlc_array[0].as_i64().unwrap_or(0),
                    open: ohlc_array[1].as_str().unwrap_or("0").to_string(),
                    high: ohlc_array[2].as_str().unwrap_or("0").to_string(),
                    low: ohlc_array[3].as_str().unwrap_or("0").to_string(),
                    close: ohlc_array[4].as_str().unwrap_or("0").to_string(),
                    vwap: ohlc_array[5].as_str().unwrap_or("0").to_string(),
                    volume: ohlc_array[6].as_str().unwrap_or("0").to_string(),
                    count: ohlc_array[7].as_i64().unwrap_or(0),
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
                        log::warn!("Failed to parse bar: {e}");
                    }
                }
            }
        }

        Ok(bars)
    }

    /// Requests an order book snapshot for an instrument.
    pub async fn request_book_snapshot(
        &self,
        instrument_id: InstrumentId,
        depth: Option<u32>,
    ) -> anyhow::Result<OrderBook, KrakenHttpError> {
        let instrument = self
            .get_cached_instrument(&instrument_id.symbol.inner())
            .ok_or_else(|| {
                KrakenHttpError::ParseError(format!(
                    "Instrument not found in cache: {instrument_id}"
                ))
            })?;

        let raw_symbol = instrument.raw_symbol().to_string();
        let asset_class = Self::asset_class_for(&instrument);
        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();
        let ts_event = self.generate_ts_init();

        let response = self
            .inner
            .get_book_depth(&raw_symbol, depth, asset_class)
            .await?;

        let book_data = response.values().next().ok_or_else(|| {
            KrakenHttpError::ParseError(format!("No book data returned for {instrument_id}"))
        })?;

        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

        // Pass sequence=0 so the snapshot does not advance the book's high-water sequence,
        // the WS subscription owns sequencing once it starts streaming deltas.
        for (i, level) in book_data.bids.iter().enumerate() {
            let price_str = level.first().and_then(|v| v.as_str()).unwrap_or("0");
            let size_str = level.get(1).and_then(|v| v.as_str()).unwrap_or("0");
            let price = Price::new(price_str.parse::<f64>().unwrap_or(0.0), price_precision);
            let size = Quantity::new(size_str.parse::<f64>().unwrap_or(0.0), size_precision);
            let order = BookOrder::new(OrderSide::Buy, price, size, i as u64);
            book.add(order, 0, 0, ts_event);
        }

        let bids_len = book_data.bids.len();

        for (i, level) in book_data.asks.iter().enumerate() {
            let price_str = level.first().and_then(|v| v.as_str()).unwrap_or("0");
            let size_str = level.get(1).and_then(|v| v.as_str()).unwrap_or("0");
            let price = Price::new(price_str.parse::<f64>().unwrap_or(0.0), price_precision);
            let size = Quantity::new(size_str.parse::<f64>().unwrap_or(0.0), size_precision);
            let order = BookOrder::new(OrderSide::Sell, price, size, (bids_len + i) as u64);
            book.add(order, 0, 0, ts_event);
        }

        Ok(book)
    }

    /// Requests account state (balances) from Kraken.
    ///
    /// In cash mode returns wallet balances only.
    /// In margin mode additionally calls `TradeBalance` to build [`MarginBalance`] entries.
    /// `margin_balance_asset` selects the summary-display denomination for `TradeBalance`
    /// (e.g. `"ZUSD"`, `"ZGBP"`); `None` lets Kraken default to `ZUSD`.
    ///
    /// Callers that also need the `TradeBalance` metrics dictionary should use
    /// [`Self::request_account_state_with_metrics`] to avoid issuing two `TradeBalance`
    /// HTTP requests per account update.
    pub async fn request_account_state(
        &self,
        account_id: AccountId,
        account_type: AccountType,
        margin_balance_asset: Option<&str>,
    ) -> anyhow::Result<AccountState> {
        self.request_account_state_with_metrics(account_id, account_type, margin_balance_asset)
            .await
            .map(|(state, _)| state)
    }

    /// Requests the full margin account snapshot in a single round-trip.
    ///
    /// Returns the [`AccountState`] (including `MarginBalance` entries when
    /// `account_type` is `Margin`) and the `TradeBalance` metrics dictionary that
    /// callers attach to `AccountState.info`. In cash mode the metrics map is empty
    /// and `TradeBalance` is not called.
    ///
    /// In margin mode, replaces the raw `margin_balance_asset` wallet with a
    /// synthetic [`AccountBalance`] using `total = e` and `free = mf` from
    /// `TradeBalance`. Kraken reports these values across all collateral, which
    /// avoids clamping free margin to one wallet bucket in multi-asset accounts.
    ///
    /// The single shared fetch keeps Kraken rate-limit usage symmetric with `Balance`
    /// (one request per account update), instead of two as if `request_account_state`
    /// and `request_margin_metrics` were called in sequence.
    pub async fn request_account_state_with_metrics(
        &self,
        account_id: AccountId,
        account_type: AccountType,
        margin_balance_asset: Option<&str>,
    ) -> anyhow::Result<(AccountState, IndexMap<String, String>)> {
        let balances_raw = self.inner.get_balance().await?;
        let ts_init = self.generate_ts_init();

        let (margins, metrics, margin_entry, target_code) = if account_type == AccountType::Margin {
            let snapshot = self
                .fetch_trade_balance_snapshot(margin_balance_asset)
                .await?;
            let target_code = normalize_currency_code(margin_balance_asset.unwrap_or("ZUSD"));
            let currency = Currency::new(target_code, 8, 0, "0", CurrencyType::Crypto);
            let margin_entry = AccountBalance::from_total_and_free(
                snapshot.equity,
                snapshot.free_margin,
                currency,
            )
            .context("Failed to build synthetic margin AccountBalance from TradeBalance")?;

            (
                snapshot.margins,
                snapshot.metrics,
                Some(margin_entry),
                target_code,
            )
        } else {
            (Vec::new(), IndexMap::new(), None, "")
        };

        let skip_margin_wallet = margin_entry.is_some();

        let balances: Vec<AccountBalance> = balances_raw
            .iter()
            .filter_map(|(currency_code, amount_str)| {
                let amount = Decimal::from_str_exact(amount_str).ok()?;
                if amount.is_zero() {
                    return None;
                }

                let normalized_code = currency_code
                    .strip_prefix("X")
                    .or_else(|| currency_code.strip_prefix("Z"))
                    .unwrap_or(currency_code);

                if skip_margin_wallet && normalized_code == target_code {
                    return None;
                }

                let currency = Currency::new(normalized_code, 8, 0, "0", CurrencyType::Crypto);
                AccountBalance::from_total_and_locked(amount, Decimal::ZERO, currency).ok()
            })
            .chain(margin_entry)
            .collect();

        let state = AccountState::new(
            account_id,
            account_type,
            balances,
            margins,
            true,
            UUID4::new(),
            ts_init,
            ts_init,
            None,
        );

        Ok((state, metrics))
    }

    /// Fetches `TradeBalance` once and returns both the parsed [`MarginBalance`] entries
    /// and the metrics dictionary surfaced through `AccountState.info`.
    ///
    /// # Margin mapping rationale
    ///
    /// Kraken's `TradeBalance` returns a single used-margin value `m` ("margin amount
    /// of open positions") and does not split into separate initial- and maintenance-
    /// margin figures. Kraken's "maintenance margin" is a liquidation-level percentage
    /// threshold (around 80% margin level), not a collateral money figure.
    ///
    /// [`nautilus_model::accounts::MarginAccount::recalculate_balance`] sums
    /// `initial + maintenance` to compute `locked`, so duplicating `m` into both fields
    /// would double-lock equity and diverge from Kraken's reported `mf = e - m`.
    /// `m` is therefore mapped to `initial`, with `maintenance = Money::zero(currency)`.
    async fn fetch_trade_balance_snapshot(
        &self,
        asset: Option<&str>,
    ) -> anyhow::Result<TradeBalanceSnapshot> {
        let tb = self.inner.get_trade_balance(asset).await?;

        let used_margin = Decimal::from_str_exact(&tb.m)
            .with_context(|| format!("Failed to parse TradeBalance 'm' field {:?}", tb.m))?;
        let free_margin = Decimal::from_str_exact(&tb.mf)
            .with_context(|| format!("Failed to parse TradeBalance 'mf' field {:?}", tb.mf))?;
        let equity = Decimal::from_str_exact(&tb.e)
            .with_context(|| format!("Failed to parse TradeBalance 'e' field {:?}", tb.e))?;

        let margins = if used_margin.is_zero() {
            Vec::new()
        } else {
            let currency = trade_balance_currency(asset);
            let initial = Money::from_decimal(used_margin, currency)
                .context("Failed to build initial margin from TradeBalance 'm'")?;
            let maintenance = Money::zero(currency);
            vec![MarginBalance::new(initial, maintenance, None)]
        };

        let mut metrics = IndexMap::new();
        metrics.insert("equivalent_balance".to_string(), tb.eb);
        metrics.insert("trade_balance".to_string(), tb.tb);
        metrics.insert("used_margin".to_string(), tb.m);
        metrics.insert("unexecuted_value".to_string(), tb.uv);
        metrics.insert("unrealized_pnl".to_string(), tb.n);
        metrics.insert("cost_basis".to_string(), tb.c);
        metrics.insert("valuation".to_string(), tb.v);
        metrics.insert("equity".to_string(), tb.e);
        metrics.insert("free_margin".to_string(), tb.mf);
        if let Some(ml) = tb.ml {
            metrics.insert("margin_level".to_string(), ml);
        }
        metrics.insert(
            "asset".to_string(),
            normalize_currency_code(asset.unwrap_or("ZUSD")).to_string(),
        );

        Ok(TradeBalanceSnapshot {
            margins,
            metrics,
            free_margin,
            equity,
        })
    }

    /// Returns a flattened snapshot of Kraken's `TradeBalance` margin metrics.
    ///
    /// Caller is expected to invoke this only when operating in margin mode; consumers
    /// surface the values via `AccountState.info` (Python-side) for strategy access.
    /// Strings preserve venue precision exactly. Keys: `equivalent_balance`,
    /// `trade_balance`, `used_margin`, `unexecuted_value`, `unrealized_pnl`,
    /// `cost_basis`, `valuation`, `equity`, `free_margin`, `margin_level` (omitted
    /// when Kraken returns no value, i.e. no open positions), `asset`.
    ///
    /// When the metrics are needed alongside the [`AccountState`], prefer
    /// [`Self::request_account_state_with_metrics`] to share a single `TradeBalance`
    /// HTTP request between both.
    pub async fn request_margin_metrics(
        &self,
        asset: Option<&str>,
    ) -> anyhow::Result<IndexMap<String, String>> {
        self.fetch_trade_balance_snapshot(asset)
            .await
            .map(|snapshot| snapshot.metrics)
    }

    /// Requests order status reports from Kraken.
    pub async fn request_order_status_reports(
        &self,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        open_only: bool,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        const PAGE_SIZE: i32 = 50;

        let ts_init = self.generate_ts_init();
        let mut all_reports = Vec::new();

        let open_orders = self.inner.get_open_orders(Some(true), None).await?;

        for (order_id, order) in &open_orders {
            if let Some(ref target_id) = instrument_id {
                let instrument = self.get_cached_instrument(&target_id.symbol.inner());
                if let Some(inst) = instrument
                    && inst.raw_symbol().as_str() != order.descr.pair
                {
                    continue;
                }
            }

            if let Some(instrument) = self.get_instrument_by_raw_symbol(order.descr.pair.as_str()) {
                match parse_order_status_report(order_id, order, &instrument, account_id, ts_init) {
                    Ok(report) => all_reports.push(report),
                    Err(e) => {
                        log::warn!("Failed to parse order {order_id}: {e}");
                    }
                }
            }
        }

        if open_only {
            return Ok(all_reports);
        }

        // Kraken API expects Unix timestamps in seconds
        let start_ts = start.map(|dt| dt.timestamp());
        let end_ts = end.map(|dt| dt.timestamp());

        let mut offset = 0;

        loop {
            let closed_orders = self
                .inner
                .get_closed_orders(Some(true), None, start_ts, end_ts, Some(offset), None)
                .await?;

            if closed_orders.is_empty() {
                break;
            }

            for (order_id, order) in &closed_orders {
                if let Some(ref target_id) = instrument_id {
                    let instrument = self.get_cached_instrument(&target_id.symbol.inner());
                    if let Some(inst) = instrument
                        && inst.raw_symbol().as_str() != order.descr.pair
                    {
                        continue;
                    }
                }

                if let Some(instrument) =
                    self.get_instrument_by_raw_symbol(order.descr.pair.as_str())
                {
                    match parse_order_status_report(
                        order_id,
                        order,
                        &instrument,
                        account_id,
                        ts_init,
                    ) {
                        Ok(report) => all_reports.push(report),
                        Err(e) => {
                            log::warn!("Failed to parse order {order_id}: {e}");
                        }
                    }
                }
            }

            offset += PAGE_SIZE;
        }

        Ok(all_reports)
    }

    /// Requests fill/trade reports from Kraken.
    pub async fn request_fill_reports(
        &self,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
    ) -> anyhow::Result<Vec<FillReport>> {
        const PAGE_SIZE: i32 = 50;

        let ts_init = self.generate_ts_init();
        let mut all_reports = Vec::new();

        // Kraken API expects Unix timestamps in seconds
        let start_ts = start.map(|dt| dt.timestamp());
        let end_ts = end.map(|dt| dt.timestamp());

        let mut offset = 0;

        loop {
            let trades = self
                .inner
                .get_trades_history(None, Some(true), start_ts, end_ts, Some(offset))
                .await?;

            if trades.is_empty() {
                break;
            }

            for (trade_id, trade) in &trades {
                if let Some(ref target_id) = instrument_id {
                    let instrument = self.get_cached_instrument(&target_id.symbol.inner());
                    if let Some(inst) = instrument
                        && inst.raw_symbol().as_str() != trade.pair
                    {
                        continue;
                    }
                }

                if let Some(instrument) = self.get_instrument_by_raw_symbol(trade.pair.as_str()) {
                    match parse_fill_report(trade_id, trade, &instrument, account_id, ts_init) {
                        Ok(report) => all_reports.push(report),
                        Err(e) => {
                            log::warn!("Failed to parse trade {trade_id}: {e}");
                        }
                    }
                }
            }

            offset += PAGE_SIZE;
        }

        Ok(all_reports)
    }

    /// Requests position status reports for SPOT instruments.
    ///
    /// In margin mode: calls `OpenPositions` and returns reports for each open leveraged position.
    /// When `use_spot_position_reports` is enabled (cash mode): derives reports from wallet balances.
    /// Otherwise returns an empty vector.
    pub async fn request_position_status_reports(
        &self,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
        account_type: AccountType,
        use_spot_position_reports: bool,
        quote_currency: Ustr,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        if account_type == AccountType::Margin {
            self.generate_margin_position_reports(account_id, instrument_id)
                .await
        } else if use_spot_position_reports {
            self.generate_spot_position_reports_from_wallet(
                account_id,
                instrument_id,
                quote_currency,
            )
            .await
        } else {
            Ok(Vec::new())
        }
    }

    /// Generates position reports from Kraken `OpenPositions` (margin mode).
    async fn generate_margin_position_reports(
        &self,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let open_positions = self
            .inner
            .get_open_positions(&SpotOpenPositionsParams::default())
            .await?;

        let ts_init = self.generate_ts_init();

        // Aggregate individual lot entries by pair into a signed net quantity.
        // Kraken returns one entry per order lot (keyed by ordertxid); buy lots add to net
        // quantity and sell lots subtract. A single signed value per pair is correct for a
        // NETTING account and avoids emitting conflicting long+short reports for the same
        // instrument when opposing lots exist on the same pair. Aggregation uses `Decimal`
        // so opposing lots cancel exactly and partial-close noise does not leave residual
        // float dust in the reported quantity.
        let mut agg: IndexMap<String, (Decimal, InstrumentId)> = IndexMap::new();

        let target_pair: Option<Ustr> = match &instrument_id {
            Some(target_id) => match self.get_cached_instrument(&target_id.symbol.inner()) {
                Some(inst) => Some(Ustr::from(inst.raw_symbol().as_str())),
                None => return Ok(Vec::new()),
            },
            None => None,
        };

        for (_pos_id, pos) in open_positions.iter() {
            if let Some(pair) = target_pair
                && pair.as_str() != pos.pair
            {
                continue;
            }

            if let Some(status) = pos.posstatus.as_deref()
                && status != "open"
            {
                log::debug!(
                    "Skipping non-open OpenPositions entry for {}: posstatus={status}",
                    pos.pair,
                );
                continue;
            }

            let instrument = self
                .get_instrument_by_raw_symbol(pos.pair.as_str())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "OpenPositions: instrument not in cache for pair {}",
                        pos.pair
                    )
                })?;

            let vol = Decimal::from_str_exact(&pos.vol)
                .with_context(|| format!("OpenPositions: failed to parse vol for {}", pos.pair))?;
            let vol_closed = Decimal::from_str_exact(&pos.vol_closed).with_context(|| {
                format!("OpenPositions: failed to parse vol_closed for {}", pos.pair)
            })?;

            let lot_net = (vol - vol_closed).max(Decimal::ZERO);
            let signed_lot = match pos.side {
                KrakenOrderSide::Buy => lot_net,
                KrakenOrderSide::Sell => -lot_net,
            };

            let entry = agg
                .entry(pos.pair.clone())
                .or_insert((Decimal::ZERO, instrument.id()));
            entry.0 += signed_lot;
        }

        let mut reports = Vec::new();

        for (_, (signed_qty, inst_id)) in agg {
            let instrument = self
                .get_cached_instrument(&inst_id.symbol.inner())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "OpenPositions: instrument disappeared from cache for {inst_id}"
                    )
                })?;

            let side = if signed_qty.is_sign_positive() && !signed_qty.is_zero() {
                PositionSideSpecified::Long
            } else if signed_qty.is_sign_negative() && !signed_qty.is_zero() {
                PositionSideSpecified::Short
            } else {
                PositionSideSpecified::Flat
            };
            let quantity = Quantity::from_decimal_dp(signed_qty.abs(), instrument.size_precision())
                .map_err(|e| {
                    anyhow::anyhow!("OpenPositions: failed to build Quantity for {inst_id}: {e:?}")
                })?;
            let report = PositionStatusReport::new(
                account_id, inst_id, side, quantity, ts_init, ts_init, None, None, None,
            );
            reports.push(report);
        }

        // If a specific instrument was requested but no open position exists for it, emit
        // a FLAT report so the engine can reconcile a previously-open position to closed.
        // (Kraken omits fully-closed positions from OpenPositions entirely.)
        // Only emit for instruments known to this spot client; a missing cache entry means
        // the target belongs to a different product type (e.g. futures) and must not receive
        // a spurious FLAT from the spot reconciliation path.
        if let Some(target_id) = instrument_id {
            let already_reported = reports.iter().any(|r| r.instrument_id == target_id);

            if !already_reported
                && let Some(instrument) = self.get_cached_instrument(&target_id.symbol.inner())
            {
                let precision = instrument.size_precision();
                reports.push(PositionStatusReport::new(
                    account_id,
                    target_id,
                    PositionSideSpecified::Flat,
                    Quantity::zero(precision),
                    ts_init,
                    ts_init,
                    None,
                    None,
                    None,
                ));
            }
        }

        Ok(reports)
    }

    /// Generates SPOT position reports from wallet balances.
    ///
    /// Kraken spot balances are simple totals (no borrowing concept).
    /// Positive balances are reported as LONG positions.
    /// Zero balances are reported as FLAT.
    async fn generate_spot_position_reports_from_wallet(
        &self,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
        quote_currency: Ustr,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let balances_raw = self.inner.get_balance().await?;
        let ts_init = self.generate_ts_init();
        let mut wallet_by_coin: HashMap<Ustr, f64> = HashMap::new();

        for (currency_code, amount_str) in &balances_raw {
            let balance = match amount_str.parse::<f64>() {
                Ok(b) => b,
                Err(_) => continue,
            };

            if balance == 0.0 {
                continue;
            }

            wallet_by_coin.insert(Ustr::from(normalize_currency_code(currency_code)), balance);
        }

        let mut reports = Vec::new();

        if let Some(instrument_id) = instrument_id {
            if let Some(instrument) = self.get_cached_instrument(&instrument_id.symbol.inner()) {
                let base_currency = match instrument.base_currency() {
                    Some(currency) => currency,
                    None => return Ok(reports),
                };

                let coin = Ustr::from(normalize_currency_code(base_currency.code.as_str()));
                let wallet_balance = wallet_by_coin.get(&coin).copied().unwrap_or(0.0);

                let side = if wallet_balance > 0.0 {
                    PositionSideSpecified::Long
                } else {
                    PositionSideSpecified::Flat
                };

                let abs_balance = wallet_balance.abs();
                let quantity = Quantity::new(abs_balance, instrument.size_precision());

                let report = PositionStatusReport::new(
                    account_id,
                    instrument_id,
                    side,
                    quantity,
                    ts_init,
                    ts_init,
                    None,
                    None,
                    None,
                );

                reports.push(report);
            }
        } else {
            let quote_filter = quote_currency;

            let instruments_guard = self.instruments_cache.load();
            for instrument in instruments_guard.values() {
                let quote_currency = match instrument.quote_currency() {
                    currency if currency.code == quote_filter => currency,
                    _ => continue,
                };

                let base_currency = match instrument.base_currency() {
                    Some(currency) => currency,
                    None => continue,
                };

                let coin = Ustr::from(normalize_currency_code(base_currency.code.as_str()));
                let wallet_balance = wallet_by_coin.get(&coin).copied().unwrap_or(0.0);

                if wallet_balance == 0.0 {
                    continue;
                }

                let side = PositionSideSpecified::Long;
                let quantity = Quantity::new(wallet_balance, instrument.size_precision());

                if quantity.is_zero() {
                    continue;
                }

                log::debug!(
                    "Spot position: {} {} (quote: {})",
                    quantity,
                    base_currency.code,
                    quote_currency.code
                );

                let report = PositionStatusReport::new(
                    account_id,
                    instrument.id(),
                    side,
                    quantity,
                    ts_init,
                    ts_init,
                    None,
                    None,
                    None,
                );

                reports.push(report);
            }
        }

        Ok(reports)
    }

    /// Submits a new order to the Kraken Spot exchange.
    ///
    /// Returns the venue order ID on success. WebSocket handles all execution events.
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
        _account_id: AccountId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: TimeInForce,
        expire_time: Option<UnixNanos>,
        price: Option<Price>,
        trigger_price: Option<Price>,
        trigger_type: Option<TriggerType>,
        trailing_offset: Option<Decimal>,
        limit_offset: Option<Decimal>,
        reduce_only: bool,
        post_only: bool,
        quote_quantity: bool,
        display_qty: Option<Quantity>,
        leverage: Option<u16>,
        account_type: AccountType,
    ) -> anyhow::Result<VenueOrderId> {
        let params = self.build_add_order_params(
            instrument_id,
            client_order_id,
            order_side,
            order_type,
            quantity,
            time_in_force,
            expire_time,
            price,
            trigger_price,
            trigger_type,
            trailing_offset,
            limit_offset,
            reduce_only,
            post_only,
            quote_quantity,
            display_qty,
            leverage,
            account_type,
        )?;
        let response = self.inner.add_order(&params).await?;

        let venue_order_id = response
            .txid
            .first()
            .ok_or_else(|| anyhow::anyhow!("No transaction ID in order response"))?;

        Ok(VenueOrderId::new(venue_order_id))
    }

    /// Submits multiple orders to the Kraken Spot exchange.
    ///
    /// Automatically groups orders by pair and chunks batch requests at the venue
    /// limit. Single-order groups fall back to `AddOrder`.
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
            Option<UnixNanos>,
            Option<Price>,
            Option<Price>,
            Option<TriggerType>,
            Option<Decimal>,
            Option<Decimal>,
            bool,
            bool,
            bool,
            Option<Quantity>,
            Option<u16>,
        )>,
        account_type: AccountType,
    ) -> anyhow::Result<Vec<String>> {
        let count = orders.len();
        if count == 0 {
            return Ok(Vec::new());
        }

        let mut all_statuses: Vec<Option<String>> = vec![None; count];
        let mut grouped: AHashMap<Ustr, Vec<(usize, KrakenSpotAddOrderParams)>> = AHashMap::new();

        for (
            idx,
            (
                instrument_id,
                client_order_id,
                order_side,
                order_type,
                quantity,
                time_in_force,
                expire_time,
                price,
                trigger_price,
                trigger_type,
                trailing_offset,
                limit_offset,
                reduce_only,
                post_only,
                quote_quantity,
                display_qty,
                leverage,
            ),
        ) in orders.into_iter().enumerate()
        {
            match self.build_add_order_params(
                instrument_id,
                client_order_id,
                order_side,
                order_type,
                quantity,
                time_in_force,
                expire_time,
                price,
                trigger_price,
                trigger_type,
                trailing_offset,
                limit_offset,
                reduce_only,
                post_only,
                quote_quantity,
                display_qty,
                leverage,
                account_type,
            ) {
                Ok(params) => {
                    grouped.entry(params.pair).or_default().push((idx, params));
                }
                Err(e) => {
                    all_statuses[idx] = Some(format!("validation_error: {e}"));
                }
            }
        }

        let mut grouped_batches: Vec<_> = grouped.into_values().collect();
        grouped_batches.sort_by_key(|group| group.first().map_or(usize::MAX, |(idx, _)| *idx));

        for grouped_orders in grouped_batches {
            for chunk in grouped_orders.chunks(BATCH_SUBMIT_LIMIT) {
                if chunk.len() == 1 {
                    let (idx, params) = &chunk[0];
                    match self.inner.add_order(params).await {
                        Ok(response) => {
                            let status = if response.txid.is_empty() {
                                "Unknown error".to_string()
                            } else {
                                "placed".to_string()
                            };
                            all_statuses[*idx] = Some(status);
                        }
                        Err(e) => {
                            all_statuses[*idx] = Some(format!("batch_error: {e}"));
                        }
                    }
                    continue;
                }

                let batch_params = KrakenSpotAddOrderBatchParams {
                    pair: chunk[0].1.pair,
                    orders: chunk
                        .iter()
                        .map(|(_, params)| params.clone().into())
                        .collect(),
                    asset_class: chunk[0].1.asset_class,
                };

                match self.inner.add_order_batch(&batch_params).await {
                    Ok(response) => {
                        for (offset, (idx, _)) in chunk.iter().enumerate() {
                            let status = response.orders.get(offset).map_or_else(
                                || "Unknown error".to_string(),
                                |order| {
                                    if order.txid.is_some() {
                                        "placed".to_string()
                                    } else {
                                        order
                                            .error
                                            .clone()
                                            .unwrap_or_else(|| "Unknown error".to_string())
                                    }
                                },
                            );
                            all_statuses[*idx] = Some(status);
                        }
                    }
                    Err(e) => {
                        for (idx, _) in chunk {
                            all_statuses[*idx] = Some(format!("batch_error: {e}"));
                        }
                    }
                }
            }
        }

        Ok(all_statuses
            .into_iter()
            .map(|status| status.unwrap_or_else(|| "Unknown error".to_string()))
            .collect())
    }

    /// Modifies an existing order on the Kraken Spot exchange using atomic amend.
    ///
    /// Uses the AmendOrder endpoint which modifies the order in-place,
    /// keeping the same order ID and queue position.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Neither `client_order_id` nor `venue_order_id` is provided.
    /// - The instrument is not found in cache.
    /// - The request fails.
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

        let txid = venue_order_id.as_ref().map(|id| id.to_string());
        let cl_ord_id = client_order_id.as_ref().map(truncate_cl_ord_id);

        if txid.is_none() && cl_ord_id.is_none() {
            anyhow::bail!("Either client_order_id or venue_order_id must be provided");
        }

        let mut builder = KrakenSpotAmendOrderParamsBuilder::default();

        // Prefer txid (venue_order_id) over cl_ord_id
        if let Some(ref id) = txid {
            builder.txid(id.clone());
        } else if let Some(ref id) = cl_ord_id {
            builder.cl_ord_id(id.clone());
        }

        if let Some(qty) = quantity {
            builder.order_qty(qty.to_string());
        }

        if let Some(p) = price {
            builder.limit_price(p.to_string());
        }

        if let Some(tp) = trigger_price {
            builder.trigger_price(tp.to_string());
        }

        let params = builder
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build amend order params: {e}"))?;

        let _response = self.inner.amend_order(&params).await?;

        // AmendOrder modifies in-place, so the order keeps its original ID
        let order_id = venue_order_id
            .ok_or_else(|| anyhow::anyhow!("venue_order_id required for amend response"))?;

        Ok(order_id)
    }

    /// Cancels an order on the Kraken Spot exchange.
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

        let txid = venue_order_id.as_ref().map(|id| id.to_string());
        let cl_ord_id = client_order_id.as_ref().map(truncate_cl_ord_id);

        if txid.is_none() && cl_ord_id.is_none() {
            anyhow::bail!("Either client_order_id or venue_order_id must be provided");
        }

        // Prefer txid (venue identifier) since Kraken always knows it.
        // cl_ord_id may not be known to Kraken for reconciled orders.
        let mut builder = KrakenSpotCancelOrderParamsBuilder::default();

        if let Some(ref id) = txid {
            builder.txid(id.clone());
        } else if let Some(ref id) = cl_ord_id {
            builder.cl_ord_id(id.clone());
        }
        let params = builder
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build cancel params: {e}"))?;

        self.inner.cancel_order(&params).await?;

        Ok(())
    }

    /// Cancels multiple orders on the Kraken Spot exchange (batched, max 50 per request).
    pub async fn cancel_orders_batch(
        &self,
        venue_order_ids: Vec<VenueOrderId>,
    ) -> anyhow::Result<i32> {
        if venue_order_ids.is_empty() {
            return Ok(0);
        }

        let mut total_cancelled = 0;

        for chunk in venue_order_ids.chunks(BATCH_CANCEL_LIMIT) {
            let orders: Vec<String> = chunk.iter().map(|id| id.to_string()).collect();
            let params = KrakenSpotCancelOrderBatchParams { orders };

            let response = self.inner.cancel_order_batch(&params).await?;
            total_cancelled += response.count;
        }

        Ok(total_cancelled)
    }

    #[expect(clippy::too_many_arguments)]
    fn build_add_order_params(
        &self,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: TimeInForce,
        expire_time: Option<UnixNanos>,
        price: Option<Price>,
        trigger_price: Option<Price>,
        trigger_type: Option<TriggerType>,
        trailing_offset: Option<Decimal>,
        limit_offset: Option<Decimal>,
        reduce_only: bool,
        post_only: bool,
        quote_quantity: bool,
        display_qty: Option<Quantity>,
        leverage: Option<u16>,
        account_type: AccountType,
    ) -> anyhow::Result<KrakenSpotAddOrderParams> {
        let instrument = self
            .get_cached_instrument(&instrument_id.symbol.inner())
            .ok_or_else(|| anyhow::anyhow!("Instrument not found in cache: {instrument_id}"))?;

        let raw_symbol = instrument.raw_symbol().inner();
        let asset_class = Self::asset_class_for(&instrument);

        let kraken_side = match order_side {
            OrderSide::Buy => KrakenOrderSide::Buy,
            OrderSide::Sell => KrakenOrderSide::Sell,
            _ => anyhow::bail!("Invalid order side: {order_side:?}"),
        };

        let kraken_order_type = match order_type {
            OrderType::Market => KrakenOrderType::Market,
            OrderType::Limit => KrakenOrderType::Limit,
            OrderType::StopMarket => KrakenOrderType::StopLoss,
            OrderType::StopLimit => KrakenOrderType::StopLossLimit,
            OrderType::MarketIfTouched => KrakenOrderType::TakeProfit,
            OrderType::LimitIfTouched => KrakenOrderType::TakeProfitLimit,
            OrderType::TrailingStopMarket => KrakenOrderType::TrailingStop,
            OrderType::TrailingStopLimit => KrakenOrderType::TrailingStopLimit,
            _ => anyhow::bail!("Unsupported order type: {order_type:?}"),
        };

        let mut oflags = Vec::new();
        let is_limit_order = matches!(
            order_type,
            OrderType::Limit
                | OrderType::StopLimit
                | OrderType::LimitIfTouched
                | OrderType::TrailingStopLimit
        );

        if time_in_force == TimeInForce::Fok && order_type != OrderType::Limit {
            anyhow::bail!("FOK time in force only supported for LIMIT orders on Kraken Spot");
        }

        let (timeinforce, expiretm) =
            compute_time_in_force(is_limit_order, time_in_force, expire_time)?;

        if post_only {
            oflags.push(KRAKEN_OFLAG_POST_ONLY);
        }

        if quote_quantity {
            oflags.push(KRAKEN_OFLAG_QUOTE_QUANTITY);
        }

        let mut builder = KrakenSpotAddOrderParamsBuilder::default();
        builder
            .cl_ord_id(truncate_cl_ord_id(&client_order_id))
            .broker(NAUTILUS_KRAKEN_BROKER_ID)
            .pair(raw_symbol)
            .side(kraken_side)
            .volume(quantity.to_string())
            .order_type(kraken_order_type);

        let is_conditional = matches!(
            order_type,
            OrderType::StopMarket
                | OrderType::StopLimit
                | OrderType::MarketIfTouched
                | OrderType::LimitIfTouched
                | OrderType::TrailingStopMarket
                | OrderType::TrailingStopLimit
        );

        let is_trailing = matches!(
            order_type,
            OrderType::TrailingStopMarket | OrderType::TrailingStopLimit
        );

        if is_trailing {
            if trigger_price.is_some() {
                anyhow::bail!(
                    "Kraken Spot trailing stops do not support activation trigger prices"
                );
            }

            if let Some(offset) = trailing_offset {
                builder.price(offset.to_string());
            }

            if let Some(offset) = limit_offset {
                builder.price2(offset.to_string());
            }
        } else if is_conditional {
            if let Some(trigger) = trigger_price {
                builder.price(trigger.to_string());
            }

            if let Some(limit) = price {
                builder.price2(limit.to_string());
            }
        } else if let Some(limit) = price {
            builder.price(limit.to_string());
        }

        if is_conditional {
            match trigger_type {
                Some(TriggerType::IndexPrice) => {
                    builder.trigger("index".to_string());
                }
                Some(TriggerType::LastPrice | TriggerType::Default) | None => {}
                Some(other) => {
                    anyhow::bail!(
                        "Unsupported trigger type for Kraken Spot: {other:?} (only LastPrice and IndexPrice supported)"
                    );
                }
            }
        }

        if !oflags.is_empty() {
            builder.oflags(oflags.join(","));
        }

        if let Some(tif) = timeinforce {
            builder.timeinforce(tif);
        }

        if let Some(expire) = expiretm {
            builder.expiretm(expire);
        }

        if let Some(dq) = display_qty {
            builder.displayvol(dq.to_string());
        }

        if let Some(ac) = asset_class {
            builder.asset_class(ac);
        }

        if leverage.is_some() && account_type != AccountType::Margin {
            anyhow::bail!("leverage requires spot_account_type=Margin (current: Cash)");
        }

        if let Some(n) = leverage {
            let tiers = self.leverage_tiers_cache.get_cloned(&raw_symbol);
            let (buy_tiers, sell_tiers) = tiers.ok_or_else(|| {
                anyhow::anyhow!(
                    "Leverage tiers not loaded for {raw_symbol}; cannot validate leverage {n}:1 (instruments must be initialized before submitting margin orders)"
                )
            })?;
            let valid_tiers = match order_side {
                OrderSide::Buy => buy_tiers,
                _ => sell_tiers,
            };
            let side_label = match order_side {
                OrderSide::Buy => "buy",
                _ => "sell",
            };

            if valid_tiers.is_empty() {
                anyhow::bail!("Leverage not supported for {raw_symbol} on {side_label} side");
            }

            if !valid_tiers.contains(&(n as i32)) {
                anyhow::bail!(
                    "Leverage {n}:1 not supported for {raw_symbol} on {side_label} side (valid: {valid_tiers:?})"
                );
            }
            builder.leverage(format!("{n}:1"));
        }

        if reduce_only {
            if account_type != AccountType::Margin {
                anyhow::bail!("reduce_only requires spot_account_type=Margin (current: Cash)");
            }
            builder.reduce_only(true);
        }

        builder
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build order params: {e}"))
    }
}

fn collect_spot_statuses(
    asset_pairs: &AssetPairsResponse,
) -> AHashMap<InstrumentId, MarketStatusAction> {
    asset_pairs
        .iter()
        .map(|(_, definition)| {
            let symbol_str = definition.wsname.as_ref().unwrap_or(&definition.altname);
            let normalized_symbol = normalize_spot_symbol(symbol_str.as_str());
            let instrument_id = InstrumentId::new(Symbol::new(&normalized_symbol), *KRAKEN_VENUE);
            let action = definition
                .status
                .map_or(MarketStatusAction::Trading, MarketStatusAction::from);

            (instrument_id, action)
        })
        .collect()
}

/// Maps raw symbol (altname, e.g. "XBTUSD") to leverage tiers.
type LeverageTiersCache = Arc<AtomicMap<Ustr, (Vec<i32>, Vec<i32>)>>;

struct TradeBalanceSnapshot {
    margins: Vec<MarginBalance>,
    metrics: IndexMap<String, String>,
    free_margin: Decimal,
    equity: Decimal,
}

/// Resolves the Nautilus [`Currency`] used to denominate `TradeBalance` margin metrics.
///
/// Kraken's `TradeBalance` defaults to `ZUSD` when no asset is supplied. This strips
/// Kraken's legacy `X`/`Z` prefixes and falls back to a 2dp fiat currency for unknown
/// codes so unusual collateral assets still produce a tagged `MarginBalance`.
fn trade_balance_currency(asset: Option<&str>) -> Currency {
    let raw = asset.unwrap_or("ZUSD");
    let normalized = normalize_currency_code(raw);
    Currency::try_from_str(normalized)
        .unwrap_or_else(|| Currency::new(normalized, 2, 0, normalized, CurrencyType::Fiat))
}

fn compute_time_in_force(
    is_limit_order: bool,
    time_in_force: TimeInForce,
    expire_time: Option<UnixNanos>,
) -> anyhow::Result<(Option<String>, Option<String>)> {
    if !is_limit_order {
        return Ok((None, None));
    }

    match time_in_force {
        TimeInForce::Gtc => Ok((None, None)),
        TimeInForce::Ioc => Ok((Some("IOC".to_string()), None)),
        TimeInForce::Fok => Ok((Some("FOK".to_string()), None)),
        TimeInForce::Gtd => {
            let expire = expire_time.ok_or_else(|| {
                anyhow::anyhow!("GTD time in force requires expire_time parameter")
            })?;
            let expire_secs = expire.as_u64() / NANOSECONDS_IN_SECOND;
            Ok((Some("GTD".to_string()), Some(expire_secs.to_string())))
        }
        _ => anyhow::bail!("Unsupported time in force: {time_in_force:?}"),
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::instruments::CurrencyPair;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_raw_client_creation() {
        let client = KrakenSpotRawHttpClient::default();
        assert!(client.credential.is_none());
    }

    #[rstest]
    fn test_raw_client_with_credentials() {
        let client = KrakenSpotRawHttpClient::with_credentials(
            "test_key".to_string(),
            "test_secret".to_string(),
            KrakenEnvironment::Live,
            None,
            60,
            None,
            None,
            None,
            None,
            KRAKEN_SPOT_DEFAULT_RATE_LIMIT_PER_SECOND,
        )
        .unwrap();
        assert!(client.credential.is_some());
    }

    #[rstest]
    fn test_client_creation() {
        let client = KrakenSpotHttpClient::default();
        assert!(client.instruments_cache.is_empty());
    }

    #[rstest]
    fn test_client_with_credentials() {
        let client = KrakenSpotHttpClient::with_credentials(
            "test_key".to_string(),
            "test_secret".to_string(),
            KrakenEnvironment::Live,
            None,
            60,
            None,
            None,
            None,
            None,
            KRAKEN_SPOT_DEFAULT_RATE_LIMIT_PER_SECOND,
        )
        .unwrap();
        assert!(client.instruments_cache.is_empty());
    }

    #[rstest]
    fn test_nonce_generation_strictly_increasing() {
        let client = KrakenSpotRawHttpClient::default();

        let nonce1 = client.generate_nonce();
        let nonce2 = client.generate_nonce();
        let nonce3 = client.generate_nonce();

        assert!(
            nonce2 > nonce1,
            "nonce2 ({nonce2}) should be > nonce1 ({nonce1})"
        );
        assert!(
            nonce3 > nonce2,
            "nonce3 ({nonce3}) should be > nonce2 ({nonce2})"
        );
    }

    #[rstest]
    fn test_nonce_is_nanosecond_timestamp() {
        let client = KrakenSpotRawHttpClient::default();

        let nonce = client.generate_nonce();

        // Nonce should be a nanosecond timestamp (roughly 1.7e18 for Dec 2025)
        // Verify it's in a reasonable range (> 1.5e18, which is ~2017)
        assert!(
            nonce > 1_500_000_000_000_000_000,
            "Nonce should be nanosecond timestamp"
        );
    }

    #[rstest]
    #[case::gtc_limit(true, TimeInForce::Gtc, None, None, None)]
    #[case::ioc_limit(true, TimeInForce::Ioc, None, Some("IOC"), None)]
    #[case::fok_limit(true, TimeInForce::Fok, None, Some("FOK"), None)]
    #[case::gtd_limit_with_expire(
        true,
        TimeInForce::Gtd,
        Some(1_704_067_200_000_000_000u64),
        Some("GTD"),
        Some("1704067200")
    )]
    #[case::gtc_market(false, TimeInForce::Gtc, None, None, None)]
    #[case::ioc_market(false, TimeInForce::Ioc, None, None, None)]
    fn test_compute_time_in_force_success(
        #[case] is_limit: bool,
        #[case] tif: TimeInForce,
        #[case] expire_nanos: Option<u64>,
        #[case] expected_tif: Option<&str>,
        #[case] expected_expire: Option<&str>,
    ) {
        let expire_time = expire_nanos.map(UnixNanos::from);
        let result = compute_time_in_force(is_limit, tif, expire_time).unwrap();
        assert_eq!(result.0, expected_tif.map(String::from));
        assert_eq!(result.1, expected_expire.map(String::from));
    }

    #[rstest]
    #[case::gtd_missing_expire(TimeInForce::Gtd, None, "expire_time")]
    fn test_compute_time_in_force_errors(
        #[case] tif: TimeInForce,
        #[case] expire_nanos: Option<u64>,
        #[case] expected_error: &str,
    ) {
        let expire_time = expire_nanos.map(UnixNanos::from);
        let result = compute_time_in_force(true, tif, expire_time);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains(expected_error));
    }

    #[rstest]
    fn test_build_add_order_params_sets_index_trigger_for_conditional_orders() {
        let client = KrakenSpotHttpClient::default();
        let instrument_id = cache_test_spot_instrument(&client);

        let params = client
            .build_add_order_params(
                instrument_id,
                ClientOrderId::new("spot-trigger-index"),
                OrderSide::Buy,
                OrderType::StopMarket,
                Quantity::from("0.01"),
                TimeInForce::Gtc,
                None,
                None,
                Some(Price::from("50000")),
                Some(TriggerType::IndexPrice),
                None,
                None,
                false,
                false,
                false,
                None,
                None,
                AccountType::Cash,
            )
            .unwrap();

        assert_eq!(params.trigger, Some("index".to_string()));
        assert_eq!(params.price, Some("50000".to_string()));
    }

    #[rstest]
    fn test_build_add_order_params_sets_trailing_offsets() {
        let client = KrakenSpotHttpClient::default();
        let instrument_id = cache_test_spot_instrument(&client);

        let params = client
            .build_add_order_params(
                instrument_id,
                ClientOrderId::new("spot-trailing"),
                OrderSide::Sell,
                OrderType::TrailingStopLimit,
                Quantity::from("0.01"),
                TimeInForce::Gtc,
                None,
                Some(Price::from("49900")),
                None,
                Some(TriggerType::LastPrice),
                Some(Decimal::from(50)),
                Some(Decimal::from(25)),
                false,
                false,
                false,
                Some(Quantity::from("0.005")),
                None,
                AccountType::Cash,
            )
            .unwrap();

        assert_eq!(params.price, Some("50".to_string()));
        assert_eq!(params.price2, Some("25".to_string()));
        assert_eq!(params.trigger, None);
        assert_eq!(params.displayvol, Some("0.005".to_string()));
    }

    #[rstest]
    fn test_build_add_order_params_rejects_unsupported_trigger_type() {
        let client = KrakenSpotHttpClient::default();
        let instrument_id = cache_test_spot_instrument(&client);

        let error = client
            .build_add_order_params(
                instrument_id,
                ClientOrderId::new("spot-trigger-invalid"),
                OrderSide::Buy,
                OrderType::StopMarket,
                Quantity::from("0.01"),
                TimeInForce::Gtc,
                None,
                None,
                Some(Price::from("50000")),
                Some(TriggerType::MarkPrice),
                None,
                None,
                false,
                false,
                false,
                None,
                None,
                AccountType::Cash,
            )
            .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("Unsupported trigger type for Kraken Spot")
        );
    }

    fn cache_test_spot_instrument(client: &KrakenSpotHttpClient) -> InstrumentId {
        let instrument_id = InstrumentId::from("XBT/USD.KRAKEN");

        client.cache_instrument(InstrumentAny::CurrencyPair(CurrencyPair::new(
            instrument_id,
            Symbol::new("XBTUSD"),
            Currency::BTC(),
            Currency::USD(),
            1,
            8,
            Price::from("0.1"),
            Quantity::from("0.00000001"),
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
            None,
            0.into(),
            0.into(),
        )));

        instrument_id
    }

    fn cache_test_spot_instrument_with_leverage(
        client: &KrakenSpotHttpClient,
        leverage_buy: &[i32],
        leverage_sell: &[i32],
    ) -> InstrumentId {
        let instrument_id = cache_test_spot_instrument(client);
        let raw_symbol = Ustr::from("XBTUSD");
        client.leverage_tiers_cache.rcu(|m| {
            m.insert(raw_symbol, (leverage_buy.to_vec(), leverage_sell.to_vec()));
        });
        instrument_id
    }

    #[rstest]
    fn test_build_add_order_params_leverage_serialised_as_ratio() {
        let client = KrakenSpotHttpClient::default();
        let instrument_id =
            cache_test_spot_instrument_with_leverage(&client, &[2, 3, 5], &[2, 3, 5]);

        let params = client
            .build_add_order_params(
                instrument_id,
                ClientOrderId::new("spot-margin-buy"),
                OrderSide::Buy,
                OrderType::Limit,
                Quantity::from("0.01"),
                TimeInForce::Gtc,
                None,
                Some(Price::from("50000")),
                None,
                None,
                None,
                None,
                false,
                false,
                false,
                None,
                Some(3),
                AccountType::Margin,
            )
            .unwrap();

        assert_eq!(params.leverage, Some("3:1".to_string()));
    }

    #[rstest]
    fn test_build_add_order_params_invalid_leverage_rejected() {
        let client = KrakenSpotHttpClient::default();
        let instrument_id =
            cache_test_spot_instrument_with_leverage(&client, &[2, 3, 5], &[2, 3, 5]);

        let err = client
            .build_add_order_params(
                instrument_id,
                ClientOrderId::new("spot-margin-bad"),
                OrderSide::Buy,
                OrderType::Limit,
                Quantity::from("0.01"),
                TimeInForce::Gtc,
                None,
                Some(Price::from("50000")),
                None,
                None,
                None,
                None,
                false,
                false,
                false,
                None,
                Some(7),
                AccountType::Margin,
            )
            .unwrap_err();

        assert!(
            err.to_string().contains("not supported"),
            "Expected tier-validation error: {err}"
        );
    }

    #[rstest]
    fn test_build_add_order_params_no_leverage_is_cash() {
        let client = KrakenSpotHttpClient::default();
        let instrument_id =
            cache_test_spot_instrument_with_leverage(&client, &[2, 3, 5], &[2, 3, 5]);

        let params = client
            .build_add_order_params(
                instrument_id,
                ClientOrderId::new("spot-cash"),
                OrderSide::Buy,
                OrderType::Limit,
                Quantity::from("0.01"),
                TimeInForce::Gtc,
                None,
                Some(Price::from("50000")),
                None,
                None,
                None,
                None,
                false,
                false,
                false,
                None,
                None,
                AccountType::Cash,
            )
            .unwrap();

        assert_eq!(
            params.leverage, None,
            "Cash order should not have leverage field"
        );
    }

    #[rstest]
    fn test_build_add_order_params_rejects_per_order_leverage_in_cash_mode() {
        let client = KrakenSpotHttpClient::default();
        let instrument_id = cache_test_spot_instrument(&client);

        let err = client
            .build_add_order_params(
                instrument_id,
                ClientOrderId::new("cash-with-leverage"),
                OrderSide::Buy,
                OrderType::Limit,
                Quantity::from("0.01"),
                TimeInForce::Gtc,
                None,
                Some(Price::from("50000")),
                None,
                None,
                None,
                None,
                false,
                false,
                false,
                None,
                Some(3),
                AccountType::Cash,
            )
            .unwrap_err();

        assert!(
            err.to_string().contains("Margin"),
            "Expected Margin mode rejection: {err}"
        );
    }

    #[rstest]
    fn test_build_add_order_params_reduce_only_forwarded_in_margin_mode() {
        let client = KrakenSpotHttpClient::default();
        let instrument_id = cache_test_spot_instrument(&client);

        let params = client
            .build_add_order_params(
                instrument_id,
                ClientOrderId::new("margin-reduce-only"),
                OrderSide::Sell,
                OrderType::Limit,
                Quantity::from("0.01"),
                TimeInForce::Gtc,
                None,
                Some(Price::from("50000")),
                None,
                None,
                None,
                None,
                true,
                false,
                false,
                None,
                None,
                AccountType::Margin,
            )
            .unwrap();

        assert_eq!(params.reduce_only, Some(true));
    }

    #[rstest]
    fn test_build_add_order_params_rejects_reduce_only_in_cash_mode() {
        let client = KrakenSpotHttpClient::default();
        let instrument_id = cache_test_spot_instrument(&client);

        let err = client
            .build_add_order_params(
                instrument_id,
                ClientOrderId::new("cash-reduce-only"),
                OrderSide::Sell,
                OrderType::Limit,
                Quantity::from("0.01"),
                TimeInForce::Gtc,
                None,
                Some(Price::from("50000")),
                None,
                None,
                None,
                None,
                true,
                false,
                false,
                None,
                None,
                AccountType::Cash,
            )
            .unwrap_err();

        assert!(
            err.to_string().contains("reduce_only requires"),
            "expected reduce_only Margin rejection: {err}"
        );
    }

    #[rstest]
    fn test_build_add_order_params_rejects_leverage_when_tiers_not_loaded() {
        let client = KrakenSpotHttpClient::default();
        let instrument_id = cache_test_spot_instrument(&client);

        let err = client
            .build_add_order_params(
                instrument_id,
                ClientOrderId::new("missing-tiers"),
                OrderSide::Buy,
                OrderType::Limit,
                Quantity::from("0.01"),
                TimeInForce::Gtc,
                None,
                Some(Price::from("50000")),
                None,
                None,
                None,
                None,
                false,
                false,
                false,
                None,
                Some(3),
                AccountType::Margin,
            )
            .unwrap_err();

        assert!(
            err.to_string().contains("Leverage tiers not loaded"),
            "expected cache-miss rejection: {err}"
        );
    }
}
