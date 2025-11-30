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
        Arc, LazyLock,
        atomic::{AtomicBool, Ordering},
    },
};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use indexmap::IndexMap;
use nautilus_core::{
    consts::NAUTILUS_USER_AGENT, nanos::UnixNanos, time::get_atomic_clock_realtime,
};
use nautilus_model::{
    data::{Bar, BarType, TradeTick},
    enums::{OrderSide, OrderType, TimeInForce},
    identifiers::{AccountId, ClientOrderId, InstrumentId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    reports::{FillReport, OrderStatusReport},
    types::{Price, Quantity},
};
use nautilus_network::{
    http::HttpClient,
    ratelimiter::quota::Quota,
    retry::{RetryConfig, RetryManager},
};
use reqwest::{Method, header::USER_AGENT};
use serde::de::DeserializeOwned;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::models::*;
use crate::{
    common::{
        consts::NAUTILUS_KRAKEN_BROKER_ID,
        credential::KrakenCredential,
        enums::{KrakenEnvironment, KrakenProductType},
        parse::{
            bar_type_to_spot_interval, parse_bar, parse_fill_report, parse_order_status_report,
            parse_spot_instrument, parse_trade_tick_from_array,
        },
        urls::get_kraken_http_base_url,
    },
    http::error::KrakenHttpError,
};

/// Default Kraken REST API rate limit.
pub static KRAKEN_SPOT_REST_QUOTA: LazyLock<Quota> = LazyLock::new(|| {
    Quota::per_second(NonZeroU32::new(5).expect("Should be a valid non-zero u32"))
});

const KRAKEN_GLOBAL_RATE_KEY: &str = "kraken:spot:global";

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
        )
        .expect("Failed to create default KrakenSpotRawHttpClient")
    }
}

impl Debug for KrakenSpotRawHttpClient {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KrakenSpotRawHttpClient")
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
                Self::rate_limiter_quotas(),
                Some(*KRAKEN_SPOT_REST_QUOTA),
                timeout_secs,
                proxy_url,
            )
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {e}"))?,
            credential: None,
            retry_manager,
            cancellation_token: CancellationToken::new(),
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
                Self::rate_limiter_quotas(),
                Some(*KRAKEN_SPOT_REST_QUOTA),
                timeout_secs,
                proxy_url,
            )
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {e}"))?,
            credential: Some(KrakenCredential::new(api_key, api_secret)),
            retry_manager,
            cancellation_token: CancellationToken::new(),
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub fn credential(&self) -> Option<&KrakenCredential> {
        self.credential.as_ref()
    }

    pub fn cancel_all_requests(&self) {
        self.cancellation_token.cancel();
    }

    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.cancellation_token
    }

    fn default_headers() -> HashMap<String, String> {
        HashMap::from([(USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string())])
    }

    fn rate_limiter_quotas() -> Vec<(String, Quota)> {
        vec![(KRAKEN_GLOBAL_RATE_KEY.to_string(), *KRAKEN_SPOT_REST_QUOTA)]
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
                    let nonce = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .expect("Time went backwards")
                        .as_millis() as u64;

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

    pub async fn get_server_time(&self) -> anyhow::Result<ServerTime, KrakenHttpError> {
        let response: KrakenResponse<ServerTime> = self
            .send_request(Method::GET, "/0/public/Time", None, false)
            .await?;

        response.result.ok_or_else(|| {
            KrakenHttpError::ParseError("Missing result in server time response".to_string())
        })
    }

    pub async fn get_system_status(&self) -> anyhow::Result<SystemStatus, KrakenHttpError> {
        let response: KrakenResponse<SystemStatus> = self
            .send_request(Method::GET, "/0/public/SystemStatus", None, false)
            .await?;

        response.result.ok_or_else(|| {
            KrakenHttpError::ParseError("Missing result in system status response".to_string())
        })
    }

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

    pub async fn add_order(
        &self,
        params: HashMap<String, String>,
    ) -> anyhow::Result<SpotAddOrderResponse, KrakenHttpError> {
        if self.credential.is_none() {
            return Err(KrakenHttpError::AuthenticationError(
                "API credentials required for adding orders".to_string(),
            ));
        }

        let param_string = serde_urlencoded::to_string(&params)
            .map_err(|e| KrakenHttpError::ParseError(format!("Failed to encode params: {e}")))?;
        let body = Some(param_string.into_bytes());

        let response: KrakenResponse<SpotAddOrderResponse> = self
            .send_request(Method::POST, "/0/private/AddOrder", body, true)
            .await?;

        response
            .result
            .ok_or_else(|| KrakenHttpError::ParseError("Missing result in response".to_string()))
    }

    pub async fn cancel_order(
        &self,
        txid: Option<String>,
        cl_ord_id: Option<String>,
    ) -> anyhow::Result<SpotCancelOrderResponse, KrakenHttpError> {
        if self.credential.is_none() {
            return Err(KrakenHttpError::AuthenticationError(
                "API credentials required for canceling orders".to_string(),
            ));
        }

        let mut params = HashMap::new();
        if let Some(id) = txid {
            params.insert("txid".to_string(), id);
        }
        if let Some(id) = cl_ord_id {
            params.insert("cl_ord_id".to_string(), id);
        }

        let param_string = serde_urlencoded::to_string(&params)
            .map_err(|e| KrakenHttpError::ParseError(format!("Failed to encode params: {e}")))?;
        let body = Some(param_string.into_bytes());

        let response: KrakenResponse<SpotCancelOrderResponse> = self
            .send_request(Method::POST, "/0/private/CancelOrder", body, true)
            .await?;

        response
            .result
            .ok_or_else(|| KrakenHttpError::ParseError("Missing result in response".to_string()))
    }

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

    pub async fn edit_order(
        &self,
        params: HashMap<String, String>,
    ) -> anyhow::Result<SpotEditOrderResponse, KrakenHttpError> {
        if self.credential.is_none() {
            return Err(KrakenHttpError::AuthenticationError(
                "API credentials required for editing orders".to_string(),
            ));
        }

        let param_string = serde_urlencoded::to_string(&params)
            .map_err(|e| KrakenHttpError::ParseError(format!("Failed to encode params: {e}")))?;
        let body = Some(param_string.into_bytes());

        let response: KrakenResponse<SpotEditOrderResponse> = self
            .send_request(Method::POST, "/0/private/EditOrder", body, true)
            .await?;

        response
            .result
            .ok_or_else(|| KrakenHttpError::ParseError("Missing result in response".to_string()))
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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct KrakenSpotHttpClient {
    pub(crate) inner: Arc<KrakenSpotRawHttpClient>,
    pub(crate) instruments_cache: Arc<DashMap<Ustr, InstrumentAny>>,
    cache_initialized: Arc<AtomicBool>,
}

impl Clone for KrakenSpotHttpClient {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            instruments_cache: self.instruments_cache.clone(),
            cache_initialized: self.cache_initialized.clone(),
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
        )
        .expect("Failed to create default KrakenSpotHttpClient")
    }
}

impl Debug for KrakenSpotHttpClient {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KrakenSpotHttpClient")
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
            )?),
            instruments_cache: Arc::new(DashMap::new()),
            cache_initialized: Arc::new(AtomicBool::new(false)),
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
            )?),
            instruments_cache: Arc::new(DashMap::new()),
            cache_initialized: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Creates a new [`KrakenSpotHttpClient`] loading credentials from environment variables.
    ///
    /// Looks for `KRAKEN_SPOT_API_KEY` and `KRAKEN_SPOT_API_SECRET` (mainnet)
    /// or `KRAKEN_SPOT_TESTNET_API_KEY` and `KRAKEN_SPOT_TESTNET_API_SECRET` (testnet).
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
    ) -> anyhow::Result<Self> {
        let testnet = environment == KrakenEnvironment::Testnet;

        if let Some(credential) = KrakenCredential::from_env_spot(testnet) {
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
            )
        }
    }

    pub fn cancel_all_requests(&self) {
        self.inner.cancel_all_requests();
    }

    pub fn cancellation_token(&self) -> &CancellationToken {
        self.inner.cancellation_token()
    }

    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        self.instruments_cache
            .insert(instrument.symbol().inner(), instrument);
        self.cache_initialized.store(true, Ordering::Release);
    }

    pub fn cache_instruments(&self, instruments: Vec<InstrumentAny>) {
        for instrument in instruments {
            self.instruments_cache
                .insert(instrument.symbol().inner(), instrument);
        }
        self.cache_initialized.store(true, Ordering::Release);
    }

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

    pub async fn get_websockets_token(&self) -> anyhow::Result<WebSocketToken, KrakenHttpError> {
        self.inner.get_websockets_token().await
    }

    // High-Level Methods (return Nautilus domain types)

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

    /// Submit a new order to the Kraken Spot exchange.
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
        reduce_only: bool,
        post_only: bool,
    ) -> anyhow::Result<OrderStatusReport> {
        let instrument = self
            .get_cached_instrument(&instrument_id.symbol.inner())
            .ok_or_else(|| anyhow::anyhow!("Instrument not found in cache: {instrument_id}"))?;

        let raw_symbol = instrument.raw_symbol().to_string();

        let kraken_side = match order_side {
            OrderSide::Buy => "buy",
            OrderSide::Sell => "sell",
            _ => anyhow::bail!("Invalid order side: {order_side:?}"),
        };

        let kraken_order_type = match order_type {
            OrderType::Market => "market",
            OrderType::Limit => "limit",
            OrderType::StopMarket => "stop-loss",
            OrderType::StopLimit => "stop-loss-limit",
            OrderType::MarketIfTouched => "take-profit",
            OrderType::LimitIfTouched => "take-profit-limit",
            _ => anyhow::bail!("Unsupported order type: {order_type:?}"),
        };

        let mut params = HashMap::new();
        params.insert("pair".to_string(), raw_symbol);
        params.insert("type".to_string(), kraken_side.to_string());
        params.insert("ordertype".to_string(), kraken_order_type.to_string());
        params.insert("volume".to_string(), quantity.to_string());
        params.insert("cl_ord_id".to_string(), client_order_id.to_string());

        // Add broker ID for partner attribution
        params.insert("broker".to_string(), NAUTILUS_KRAKEN_BROKER_ID.to_string());

        if let Some(price) = price {
            params.insert("price".to_string(), price.to_string());
        }

        // Build oflags based on time in force and order options
        let mut oflags = Vec::new();

        match time_in_force {
            TimeInForce::Gtc => {} // Default, no flag needed
            TimeInForce::Ioc => {
                oflags.push("ioc");
            }
            TimeInForce::Fok => {
                anyhow::bail!("FOK time in force not supported by Kraken Spot API");
            }
            TimeInForce::Gtd => {
                anyhow::bail!("GTD time in force requires expire_time parameter");
            }
            _ => anyhow::bail!("Unsupported time in force: {time_in_force:?}"),
        }

        if post_only {
            oflags.push("post");
        }

        if reduce_only {
            // Kraken Spot doesn't support reduce_only, ignore silently
            tracing::warn!("reduce_only is not supported by Kraken Spot API, ignoring");
        }

        if !oflags.is_empty() {
            params.insert("oflags".to_string(), oflags.join(","));
        }

        let response = self.inner.add_order(params).await?;

        let venue_order_id = response
            .txid
            .first()
            .ok_or_else(|| anyhow::anyhow!("No transaction ID in order response"))?;

        // Query the order to get full status
        let orders = self.inner.get_open_orders(Some(true), None).await?;

        let order = orders
            .get(venue_order_id)
            .ok_or_else(|| anyhow::anyhow!("Order not found after submission: {venue_order_id}"))?;

        let ts_init = self.generate_ts_init();
        parse_order_status_report(venue_order_id, order, &instrument, account_id, ts_init)
    }

    /// Cancel an order on the Kraken Spot exchange.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing.
    /// - Neither client_order_id nor venue_order_id is provided.
    /// - The order is not found.
    /// - The request fails.
    pub async fn cancel_order(
        &self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> anyhow::Result<OrderStatusReport> {
        let instrument = self
            .get_cached_instrument(&instrument_id.symbol.inner())
            .ok_or_else(|| anyhow::anyhow!("Instrument not found in cache: {instrument_id}"))?;

        let txid = venue_order_id.map(|id| id.to_string());
        let cl_ord_id = client_order_id.map(|id| id.to_string());

        if txid.is_none() && cl_ord_id.is_none() {
            anyhow::bail!("Either client_order_id or venue_order_id must be provided");
        }

        let _response = self.inner.cancel_order(txid.clone(), cl_ord_id).await?;

        // Query the order to get final status
        let order_id = txid.ok_or_else(|| {
            anyhow::anyhow!("venue_order_id required to query order status after cancellation")
        })?;

        // Check closed orders for the canceled order
        let closed_orders = self
            .inner
            .get_closed_orders(Some(true), None, None, None, None, None)
            .await?;

        let order = closed_orders
            .get(&order_id)
            .ok_or_else(|| anyhow::anyhow!("Order not found after cancellation: {order_id}"))?;

        let ts_init = self.generate_ts_init();
        parse_order_status_report(&order_id, order, &instrument, account_id, ts_init)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

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
        )
        .unwrap();
        assert!(client.instruments_cache.is_empty());
    }
}
