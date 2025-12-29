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

//! HTTP client for the Kraken Spot REST API.

use std::{
    collections::HashMap,
    fmt::{Debug, Formatter},
    num::NonZeroU32,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use indexmap::IndexMap;
use nautilus_core::{
    AtomicTime, UUID4, consts::NAUTILUS_USER_AGENT, datetime::NANOSECONDS_IN_SECOND,
    nanos::UnixNanos, time::get_atomic_clock_realtime,
};
use nautilus_model::{
    data::{Bar, BarType, TradeTick},
    enums::{AccountType, CurrencyType, OrderSide, OrderType, PositionSideSpecified, TimeInForce},
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
        enums::{KrakenEnvironment, KrakenOrderSide, KrakenOrderType, KrakenProductType},
        parse::{
            bar_type_to_spot_interval, normalize_currency_code, parse_bar, parse_fill_report,
            parse_order_status_report, parse_spot_instrument, parse_trade_tick_from_array,
        },
        urls::get_kraken_http_base_url,
    },
    http::error::KrakenHttpError,
};

/// Default Kraken Spot REST API rate limit (requests per second).
pub const KRAKEN_SPOT_DEFAULT_RATE_LIMIT_PER_SECOND: u32 = 5;

const KRAKEN_GLOBAL_RATE_KEY: &str = "kraken:spot:global";

/// Maximum orders per batch cancel request for Kraken Spot API.
const BATCH_CANCEL_LIMIT: usize = 50;

/// Computes the time-in-force and expiration time parameters for Kraken Spot orders.
///
/// Returns a tuple of (timeinforce, expiretm) for use in order requests.
/// For limit orders, handles GTC, IOC, and GTD. Market orders return (None, None).
fn compute_time_in_force(
    is_limit_order: bool,
    time_in_force: TimeInForce,
    expire_time: Option<UnixNanos>,
) -> anyhow::Result<(Option<String>, Option<String>)> {
    if is_limit_order {
        match time_in_force {
            TimeInForce::Gtc => Ok((None, None)), // Default, no parameter needed
            TimeInForce::Ioc => Ok((Some("IOC".to_string()), None)),
            TimeInForce::Fok => {
                anyhow::bail!("FOK time in force not supported by Kraken Spot API")
            }
            TimeInForce::Gtd => {
                let expire = expire_time.ok_or_else(|| {
                    anyhow::anyhow!("GTD time in force requires expire_time parameter")
                })?;
                // Convert nanoseconds to seconds for Kraken API
                let expire_secs = expire.as_u64() / NANOSECONDS_IN_SECOND;
                Ok((Some("GTD".to_string()), Some(expire_secs.to_string())))
            }
            _ => anyhow::bail!("Unsupported time in force: {time_in_force:?}"),
        }
    } else {
        // Market orders are inherently immediate, timeinforce not applicable
        Ok((None, None))
    }
}

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
            KrakenEnvironment::Mainnet,
            None,
            Some(60),
            None,
            None,
            None,
            None,
            None,
        )
        .expect("Failed to create default KrakenSpotRawHttpClient")
    }
}

impl Debug for KrakenSpotRawHttpClient {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(KrakenSpotRawHttpClient))
            .field("base_url", &self.base_url)
            .field("has_credentials", &self.credential.is_some())
            .finish()
    }
}

impl KrakenSpotRawHttpClient {
    /// Creates a new [`KrakenSpotRawHttpClient`].
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
            get_kraken_http_base_url(KrakenProductType::Spot, environment).to_string()
        });

        let rate_limit =
            max_requests_per_second.unwrap_or(KRAKEN_SPOT_DEFAULT_RATE_LIMIT_PER_SECOND);

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

    /// Creates a new [`KrakenSpotRawHttpClient`] with credentials.
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
            get_kraken_http_base_url(KrakenProductType::Spot, environment).to_string()
        });

        let rate_limit =
            max_requests_per_second.unwrap_or(KRAKEN_SPOT_DEFAULT_RATE_LIMIT_PER_SECOND);

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

    fn default_quota(max_requests_per_second: u32) -> Quota {
        Quota::per_second(
            NonZeroU32::new(max_requests_per_second).unwrap_or_else(|| {
                NonZeroU32::new(KRAKEN_SPOT_DEFAULT_RATE_LIMIT_PER_SECOND).unwrap()
            }),
        )
    }

    fn rate_limiter_quotas(max_requests_per_second: u32) -> Vec<(String, Quota)> {
        vec![(
            KRAKEN_GLOBAL_RATE_KEY.to_string(),
            Self::default_quota(max_requests_per_second),
        )]
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
                    tracing::debug!("Generated nonce {nonce} for {endpoint}");

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
                    return Err(KrakenHttpError::ApiError(kraken_response.error.clone()));
                }

                Ok(kraken_response)
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
    pub async fn get_asset_pairs(
        &self,
        pairs: Option<Vec<String>>,
    ) -> anyhow::Result<AssetPairsResponse, KrakenHttpError> {
        let endpoint = if let Some(pairs) = pairs {
            format!("/0/public/AssetPairs?pair={}", pairs.join(","))
        } else {
            "/0/public/AssetPairs".to_string()
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
    ) -> anyhow::Result<TickerResponse, KrakenHttpError> {
        let endpoint = format!("/0/public/Ticker?pair={}", pairs.join(","));

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
    ) -> anyhow::Result<OhlcResponse, KrakenHttpError> {
        let mut endpoint = format!("/0/public/OHLC?pair={pair}");

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
    ) -> anyhow::Result<OrderBookResponse, KrakenHttpError> {
        let mut endpoint = format!("/0/public/Depth?pair={pair}");

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
    ) -> anyhow::Result<TradesResponse, KrakenHttpError> {
        let mut endpoint = format!("/0/public/Trades?pair={pair}");

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
}

// =============================================================================
// Domain Client
// =============================================================================

/// High-level HTTP client for the Kraken Spot REST API.
///
/// This client wraps the raw client and provides Nautilus domain types.
/// It maintains an instrument cache and uses it to parse venue responses
/// into Nautilus domain objects.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.kraken")
)]
pub struct KrakenSpotHttpClient {
    pub(crate) inner: Arc<KrakenSpotRawHttpClient>,
    pub(crate) instruments_cache: Arc<DashMap<Ustr, InstrumentAny>>,
    cache_initialized: Arc<AtomicBool>,
    use_spot_position_reports: Arc<AtomicBool>,
    spot_positions_quote_currency: Arc<RwLock<Ustr>>,
}

impl Clone for KrakenSpotHttpClient {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            instruments_cache: self.instruments_cache.clone(),
            cache_initialized: self.cache_initialized.clone(),
            use_spot_position_reports: self.use_spot_position_reports.clone(),
            spot_positions_quote_currency: self.spot_positions_quote_currency.clone(),
        }
    }
}

impl Default for KrakenSpotHttpClient {
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
        .expect("Failed to create default KrakenSpotHttpClient")
    }
}

impl Debug for KrakenSpotHttpClient {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(KrakenSpotHttpClient))
            .field("inner", &self.inner)
            .finish()
    }
}

impl KrakenSpotHttpClient {
    /// Creates a new [`KrakenSpotHttpClient`].
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
            instruments_cache: Arc::new(DashMap::new()),
            cache_initialized: Arc::new(AtomicBool::new(false)),
            use_spot_position_reports: Arc::new(AtomicBool::new(false)),
            spot_positions_quote_currency: Arc::new(RwLock::new(Ustr::from("USDT"))),
        })
    }

    /// Creates a new [`KrakenSpotHttpClient`] with credentials.
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
            instruments_cache: Arc::new(DashMap::new()),
            cache_initialized: Arc::new(AtomicBool::new(false)),
            use_spot_position_reports: Arc::new(AtomicBool::new(false)),
            spot_positions_quote_currency: Arc::new(RwLock::new(Ustr::from("USDT"))),
        })
    }

    /// Creates a new [`KrakenSpotHttpClient`] loading credentials from environment variables.
    ///
    /// Looks for `KRAKEN_SPOT_API_KEY` and `KRAKEN_SPOT_API_SECRET`.
    ///
    /// Note: Kraken Spot does not have a testnet/demo environment.
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

    /// Sets whether to generate position reports from wallet balances for SPOT instruments.
    pub fn set_use_spot_position_reports(&self, value: bool) {
        self.use_spot_position_reports
            .store(value, Ordering::Relaxed);
    }

    /// Sets the quote currency filter for spot position reports.
    pub fn set_spot_positions_quote_currency(&self, currency: &str) {
        let mut guard = self.spot_positions_quote_currency.write().expect("lock");
        *guard = Ustr::from(currency);
    }

    /// Requests an authentication token for WebSocket connections.
    pub async fn get_websockets_token(&self) -> anyhow::Result<WebSocketToken, KrakenHttpError> {
        self.inner.get_websockets_token().await
    }

    /// Requests tradable instruments from Kraken.
    pub async fn request_instruments(
        &self,
        pairs: Option<Vec<String>>,
    ) -> anyhow::Result<Vec<InstrumentAny>, KrakenHttpError> {
        let ts_init = self.generate_ts_init();
        let asset_pairs = self.inner.get_asset_pairs(pairs).await?;

        let instruments: Vec<InstrumentAny> = asset_pairs
            .iter()
            .filter_map(|(pair_name, definition)| {
                match parse_spot_instrument(pair_name, definition, ts_init, ts_init) {
                    Ok(instrument) => Some(instrument),
                    Err(e) => {
                        tracing::warn!("Failed to parse instrument {pair_name}: {e}");
                        None
                    }
                }
            })
            .collect();

        Ok(instruments)
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
        let ts_init = self.generate_ts_init();

        // Kraken trades API expects nanoseconds since epoch as string
        let since = start.map(|dt| (dt.timestamp_nanos_opt().unwrap_or(0) as u64).to_string());
        let response = self.inner.get_trades(&raw_symbol, since).await?;

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
                        tracing::warn!("Failed to parse trade tick: {e}");
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
        let ts_init = self.generate_ts_init();

        let interval = Some(
            bar_type_to_spot_interval(bar_type)
                .map_err(|e| KrakenHttpError::ParseError(e.to_string()))?,
        );

        // Kraken OHLC API expects Unix timestamp in seconds
        let since = start.map(|dt| dt.timestamp());
        let end_ns = end.map(|dt| dt.timestamp_nanos_opt().unwrap_or(0) as u64);
        let response = self.inner.get_ohlc(&raw_symbol, interval, since).await?;

        let mut bars = Vec::new();

        for (_pair_name, ohlc_arrays) in &response.data {
            for ohlc_array in ohlc_arrays {
                if ohlc_array.len() < 8 {
                    let len = ohlc_array.len();
                    tracing::warn!("OHLC array too short: {len}");
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
                        tracing::warn!("Failed to parse bar: {e}");
                    }
                }
            }
        }

        Ok(bars)
    }

    /// Requests account state (balances) from Kraken.
    ///
    /// Returns an `AccountState` containing all currency balances.
    pub async fn request_account_state(
        &self,
        account_id: AccountId,
    ) -> anyhow::Result<AccountState> {
        let balances_raw = self.inner.get_balance().await?;
        let ts_init = self.generate_ts_init();

        let balances: Vec<AccountBalance> = balances_raw
            .iter()
            .filter_map(|(currency_code, amount_str)| {
                let amount = amount_str.parse::<f64>().ok()?;
                if amount == 0.0 {
                    return None;
                }

                // Kraken uses X-prefixed names for some currencies (e.g., XXBT for BTC)
                let normalized_code = currency_code
                    .strip_prefix("X")
                    .or_else(|| currency_code.strip_prefix("Z"))
                    .unwrap_or(currency_code);

                let currency = Currency::new(
                    normalized_code,
                    8, // Default precision
                    0,
                    "0",
                    CurrencyType::Crypto,
                );

                let total = Money::new(amount, currency);
                let locked = Money::new(0.0, currency);

                // Balance endpoint returns total only, so free = total (no locked info)
                Some(AccountBalance::new(total, locked, total))
            })
            .collect();

        Ok(AccountState::new(
            account_id,
            AccountType::Cash,
            balances,
            vec![], // No margins for spot
            true,   // reported
            UUID4::new(),
            ts_init,
            ts_init,
            None,
        ))
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
                        tracing::warn!("Failed to parse order {order_id}: {e}");
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
        const PAGE_SIZE: i32 = 50;

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
                            tracing::warn!("Failed to parse order {order_id}: {e}");
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
        let ts_init = self.generate_ts_init();
        let mut all_reports = Vec::new();

        // Kraken API expects Unix timestamps in seconds
        let start_ts = start.map(|dt| dt.timestamp());
        let end_ts = end.map(|dt| dt.timestamp());

        let mut offset = 0;
        const PAGE_SIZE: i32 = 50;

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
                            tracing::warn!("Failed to parse trade {trade_id}: {e}");
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
    /// Returns wallet balances as position reports if `use_spot_position_reports` is enabled.
    /// Otherwise returns an empty vector (spot traditionally has no "positions").
    pub async fn request_position_status_reports(
        &self,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        if self.use_spot_position_reports.load(Ordering::Relaxed) {
            self.generate_spot_position_reports_from_wallet(account_id, instrument_id)
                .await
        } else {
            Ok(Vec::new())
        }
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
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let balances_raw = self.inner.get_balance().await?;
        let ts_init = self.generate_ts_init();
        let mut wallet_by_coin: HashMap<Ustr, f64> = HashMap::new();

        for (currency_code, amount_str) in balances_raw.iter() {
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
            let quote_filter = *self.spot_positions_quote_currency.read().expect("lock");

            for entry in self.instruments_cache.iter() {
                let instrument = entry.value();

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

                tracing::debug!(
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
    #[allow(clippy::too_many_arguments)]
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
        reduce_only: bool,
        post_only: bool,
    ) -> anyhow::Result<VenueOrderId> {
        let instrument = self
            .get_cached_instrument(&instrument_id.symbol.inner())
            .ok_or_else(|| anyhow::anyhow!("Instrument not found in cache: {instrument_id}"))?;

        let raw_symbol = instrument.raw_symbol().inner();

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
            _ => anyhow::bail!("Unsupported order type: {order_type:?}"),
        };

        // Note: timeinforce is only valid for limit-type orders, not market orders
        let mut oflags = Vec::new();
        let is_limit_order = matches!(
            order_type,
            OrderType::Limit | OrderType::StopLimit | OrderType::LimitIfTouched
        );

        let (timeinforce, expiretm) =
            compute_time_in_force(is_limit_order, time_in_force, expire_time)?;

        if post_only {
            oflags.push("post");
        }

        if reduce_only {
            tracing::warn!("reduce_only is not supported by Kraken Spot API, ignoring");
        }

        let mut builder = KrakenSpotAddOrderParamsBuilder::default();
        builder
            .cl_ord_id(client_order_id.to_string())
            .broker(NAUTILUS_KRAKEN_BROKER_ID)
            .pair(raw_symbol)
            .side(kraken_side)
            .volume(quantity.to_string())
            .order_type(kraken_order_type);

        // For stop/conditional orders:
        // - price = trigger price (when the order activates)
        // - price2 = limit price (for stop-limit and take-profit-limit)
        // For regular limit orders:
        // - price = limit price
        let is_conditional = matches!(
            order_type,
            OrderType::StopMarket
                | OrderType::StopLimit
                | OrderType::MarketIfTouched
                | OrderType::LimitIfTouched
        );

        if is_conditional {
            if let Some(trigger) = trigger_price {
                builder.price(trigger.to_string());
            }
            if let Some(limit) = price {
                builder.price2(limit.to_string());
            }
        } else if let Some(limit) = price {
            builder.price(limit.to_string());
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

        let params = builder
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build order params: {e}"))?;

        let response = self.inner.add_order(&params).await?;

        let venue_order_id = response
            .txid
            .first()
            .ok_or_else(|| anyhow::anyhow!("No transaction ID in order response"))?;

        Ok(VenueOrderId::new(venue_order_id))
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
        let cl_ord_id = client_order_id.as_ref().map(|id| id.to_string());

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
        let cl_ord_id = client_order_id.as_ref().map(|id| id.to_string());

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
}

#[cfg(test)]
mod tests {
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
        let client = KrakenSpotHttpClient::default();
        assert!(client.instruments_cache.is_empty());
    }

    #[rstest]
    fn test_client_with_credentials() {
        let client = KrakenSpotHttpClient::with_credentials(
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
    #[case::fok_not_supported(TimeInForce::Fok, None, "FOK")]
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
}
