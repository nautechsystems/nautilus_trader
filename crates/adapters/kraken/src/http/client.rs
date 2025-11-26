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

//! HTTP client for the Kraken REST API v2.

use std::{
    collections::HashMap,
    fmt::{Debug, Formatter},
    num::NonZeroU32,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicBool, Ordering},
    },
};

use dashmap::DashMap;
use nautilus_core::{
    consts::NAUTILUS_USER_AGENT, nanos::UnixNanos, time::get_atomic_clock_realtime,
};
use nautilus_model::{
    data::{Bar, BarType, TradeTick},
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
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

use super::{
    error::KrakenHttpError,
    models::{FuturesCandlesResponse, FuturesInstrumentsResponse, FuturesTickersResponse, *},
};
use crate::common::{
    credential::KrakenCredential,
    enums::{KrakenEnvironment, KrakenProductType},
    parse::{
        bar_type_to_futures_resolution, bar_type_to_spot_interval, parse_bar,
        parse_futures_instrument, parse_spot_instrument, parse_trade_tick_from_array,
    },
    urls::get_http_base_url,
};

pub static KRAKEN_REST_QUOTA: LazyLock<Quota> = LazyLock::new(|| {
    Quota::per_second(NonZeroU32::new(5).expect("Should be a valid non-zero u32"))
});

const KRAKEN_GLOBAL_RATE_KEY: &str = "kraken:global";

#[derive(Debug, Clone, serde::Deserialize)]
pub struct KrakenResponse<T> {
    pub error: Vec<String>,
    pub result: Option<T>,
}

pub struct KrakenRawHttpClient {
    base_url: String,
    client: HttpClient,
    credential: Option<KrakenCredential>,
    retry_manager: RetryManager<KrakenHttpError>,
    cancellation_token: CancellationToken,
}

impl Default for KrakenRawHttpClient {
    fn default() -> Self {
        Self::new(None, Some(60), None, None, None, None)
            .expect("Failed to create default KrakenRawHttpClient")
    }
}

impl Debug for KrakenRawHttpClient {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KrakenRawHttpClient")
            .field("base_url", &self.base_url)
            .field("has_credentials", &self.credential.is_some())
            .finish()
    }
}

impl KrakenRawHttpClient {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        base_url: Option<String>,
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

        Ok(Self {
            base_url: base_url.unwrap_or_else(|| {
                get_http_base_url(KrakenProductType::Spot, KrakenEnvironment::Mainnet).to_string()
            }),
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                Self::rate_limiter_quotas(),
                Some(*KRAKEN_REST_QUOTA),
                timeout_secs,
                proxy_url,
            )
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {e}"))?,
            credential: None,
            retry_manager,
            cancellation_token: CancellationToken::new(),
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn with_credentials(
        api_key: String,
        api_secret: String,
        base_url: Option<String>,
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

        Ok(Self {
            base_url: base_url.unwrap_or_else(|| {
                get_http_base_url(KrakenProductType::Spot, KrakenEnvironment::Mainnet).to_string()
            }),
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                Self::rate_limiter_quotas(),
                Some(*KRAKEN_REST_QUOTA),
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
        vec![(KRAKEN_GLOBAL_RATE_KEY.to_string(), *KRAKEN_REST_QUOTA)]
    }

    fn rate_limit_keys(endpoint: &str) -> Vec<String> {
        let normalized = endpoint.split('?').next().unwrap_or(endpoint);
        let route = format!("kraken:{normalized}");
        vec![KRAKEN_GLOBAL_RATE_KEY.to_string(), route]
    }

    fn sign_request(
        &self,
        path: &str,
        nonce: u64,
        params: &HashMap<String, String>,
    ) -> anyhow::Result<(HashMap<String, String>, String)> {
        let credential = self
            .credential
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing credentials"))?;

        let (signature, post_data) = credential.sign_request(path, nonce, params)?;

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
                        .sign_request(&endpoint, nonce, &params)
                        .map_err(|e| KrakenHttpError::NetworkError(e.to_string()))?;
                    headers.extend(auth_headers);

                    // Use the exact post_data that was signed
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
                    let body = String::from_utf8_lossy(&response.body).to_string();
                    return Err(KrakenHttpError::NetworkError(format!(
                        "HTTP error {}: {body}",
                        response.status.as_u16()
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

    async fn send_futures_request<T: DeserializeOwned>(
        &self,
        method: Method,
        endpoint: &str,
        url: String,
    ) -> anyhow::Result<T, KrakenHttpError> {
        let endpoint = endpoint.to_string();
        let method_clone = method.clone();
        let url_clone = url.clone();

        let operation = || {
            let url = url_clone.clone();
            let method = method_clone.clone();
            let endpoint = endpoint.clone();

            async move {
                let headers = Self::default_headers();
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

                if response.status.as_u16() >= 400 {
                    let body = String::from_utf8_lossy(&response.body).to_string();
                    return Err(KrakenHttpError::NetworkError(format!(
                        "HTTP error {}: {body}",
                        response.status.as_u16()
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

    fn generate_ts_init(&self) -> UnixNanos {
        get_atomic_clock_realtime().get_time_ns()
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

    pub async fn get_instruments_futures(
        &self,
    ) -> anyhow::Result<FuturesInstrumentsResponse, KrakenHttpError> {
        let endpoint = "/derivatives/api/v3/instruments";
        let url = format!("{}{endpoint}", self.base_url);

        self.send_futures_request(Method::GET, endpoint, url).await
    }

    pub async fn get_tickers_futures(
        &self,
    ) -> anyhow::Result<FuturesTickersResponse, KrakenHttpError> {
        let endpoint = "/derivatives/api/v3/tickers";
        let url = format!("{}{endpoint}", self.base_url);

        self.send_futures_request(Method::GET, endpoint, url).await
    }

    pub async fn get_ohlc_futures(
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

        self.send_futures_request(Method::GET, &endpoint, url).await
    }
}

#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct KrakenHttpClient {
    pub(crate) inner: Arc<KrakenRawHttpClient>,
    pub(crate) instruments_cache: Arc<DashMap<Ustr, InstrumentAny>>,
    cache_initialized: Arc<AtomicBool>,
}

impl Clone for KrakenHttpClient {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            instruments_cache: self.instruments_cache.clone(),
            cache_initialized: self.cache_initialized.clone(),
        }
    }
}

impl Default for KrakenHttpClient {
    fn default() -> Self {
        Self::new(None, Some(60), None, None, None, None)
            .expect("Failed to create default KrakenHttpClient")
    }
}

impl Debug for KrakenHttpClient {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KrakenHttpClient")
            .field("inner", &self.inner)
            .finish()
    }
}

impl KrakenHttpClient {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        base_url: Option<String>,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        proxy_url: Option<String>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            inner: Arc::new(KrakenRawHttpClient::new(
                base_url,
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

    #[allow(clippy::too_many_arguments)]
    pub fn with_credentials(
        api_key: String,
        api_secret: String,
        base_url: Option<String>,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        proxy_url: Option<String>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            inner: Arc::new(KrakenRawHttpClient::with_credentials(
                api_key,
                api_secret,
                base_url,
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

    fn is_futures(&self) -> bool {
        self.inner.base_url().contains("futures")
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

    pub async fn get_websockets_token(&self) -> anyhow::Result<WebSocketToken, KrakenHttpError> {
        self.inner.get_websockets_token().await
    }

    pub async fn request_instruments(
        &self,
        pairs: Option<Vec<String>>,
    ) -> anyhow::Result<Vec<InstrumentAny>, KrakenHttpError> {
        let ts_init = self.inner.generate_ts_init();

        if self.is_futures() {
            let response = self.inner.get_instruments_futures().await?;

            let instruments: Vec<InstrumentAny> = response
                .instruments
                .iter()
                .filter_map(|fut_instrument| {
                    match parse_futures_instrument(fut_instrument, ts_init, ts_init) {
                        Ok(instrument) => Some(instrument),
                        Err(e) => {
                            tracing::warn!(
                                "Failed to parse futures instrument {}: {e}",
                                fut_instrument.symbol
                            );
                            None
                        }
                    }
                })
                .collect();

            return Ok(instruments);
        }

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

    pub async fn request_mark_price(
        &self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<f64, KrakenHttpError> {
        if !self.is_futures() {
            return Err(KrakenHttpError::ParseError(
                "Mark price is only available for futures instruments. Use a futures client (base URL must contain 'futures')".to_string(),
            ));
        }

        let instrument = self
            .get_cached_instrument(&instrument_id.symbol.inner())
            .ok_or_else(|| {
                KrakenHttpError::ParseError(format!(
                    "Instrument not found in cache: {}",
                    instrument_id
                ))
            })?;

        let raw_symbol = instrument.raw_symbol().to_string();
        let tickers = self.inner.get_tickers_futures().await?;

        tickers
            .tickers
            .iter()
            .find(|t| t.symbol == raw_symbol)
            .map(|t| t.mark_price)
            .ok_or_else(|| {
                KrakenHttpError::ParseError(format!("Symbol {} not found in tickers", raw_symbol))
            })
    }

    pub async fn request_index_price(
        &self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<f64, KrakenHttpError> {
        if !self.is_futures() {
            return Err(KrakenHttpError::ParseError(
                "Index price is only available for futures instruments. Use a futures client (base URL must contain 'futures')".to_string(),
            ));
        }

        let instrument = self
            .get_cached_instrument(&instrument_id.symbol.inner())
            .ok_or_else(|| {
                KrakenHttpError::ParseError(format!(
                    "Instrument not found in cache: {}",
                    instrument_id
                ))
            })?;

        let raw_symbol = instrument.raw_symbol().to_string();
        let tickers = self.inner.get_tickers_futures().await?;

        tickers
            .tickers
            .iter()
            .find(|t| t.symbol == raw_symbol)
            .map(|t| t.index_price)
            .ok_or_else(|| {
                KrakenHttpError::ParseError(format!("Symbol {} not found in tickers", raw_symbol))
            })
    }

    pub async fn request_trades(
        &self,
        instrument_id: InstrumentId,
        start: Option<u64>,
        end: Option<u64>,
        limit: Option<u64>,
    ) -> anyhow::Result<Vec<TradeTick>, KrakenHttpError> {
        if self.is_futures() {
            return Err(KrakenHttpError::ParseError(
                "Trade history is not yet implemented for futures instruments. Use a spot client instead.".to_string(),
            ));
        }

        let instrument = self
            .get_cached_instrument(&instrument_id.symbol.inner())
            .ok_or_else(|| {
                KrakenHttpError::ParseError(format!(
                    "Instrument not found in cache: {}",
                    instrument_id
                ))
            })?;

        let raw_symbol = instrument.raw_symbol().to_string();
        let since = start.map(|s| s.to_string());

        let ts_init = self.inner.generate_ts_init();
        let response = self.inner.get_trades(&raw_symbol, since).await?;

        let mut trades = Vec::new();

        // Get the first (and typically only) pair's trade data
        for (_pair_name, trade_arrays) in &response.data {
            for trade_array in trade_arrays {
                match parse_trade_tick_from_array(trade_array, &instrument, ts_init) {
                    Ok(trade_tick) => {
                        // Filter by end time if specified
                        if let Some(end_ns) = end
                            && trade_tick.ts_event.as_u64() > end_ns
                        {
                            continue;
                        }
                        trades.push(trade_tick);

                        // Check limit
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
        start: Option<u64>,
        end: Option<u64>,
        limit: Option<u64>,
    ) -> anyhow::Result<Vec<Bar>, KrakenHttpError> {
        self.request_bars_with_tick_type(bar_type, start, end, limit, None)
            .await
    }

    pub async fn request_bars_with_tick_type(
        &self,
        bar_type: BarType,
        start: Option<u64>,
        end: Option<u64>,
        limit: Option<u64>,
        tick_type: Option<&str>,
    ) -> anyhow::Result<Vec<Bar>, KrakenHttpError> {
        let instrument_id = bar_type.instrument_id();
        let instrument = self
            .get_cached_instrument(&instrument_id.symbol.inner())
            .ok_or_else(|| {
                KrakenHttpError::ParseError(format!(
                    "Instrument not found in cache: {}",
                    instrument_id
                ))
            })?;

        let raw_symbol = instrument.raw_symbol().to_string();
        let ts_init = self.inner.generate_ts_init();

        if self.is_futures() {
            let tick_type = tick_type.unwrap_or("trade");
            let resolution = bar_type_to_futures_resolution(bar_type)
                .map_err(|e| KrakenHttpError::ParseError(e.to_string()))?;

            // Kraken Futures API expects millisecond timestamps
            let from = start.map(|s| (s / 1_000_000) as i64);
            let to = end.map(|e| (e / 1_000_000) as i64);

            let response = self
                .inner
                .get_ohlc_futures(tick_type, &raw_symbol, resolution, from, to)
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
                        // Filter by end time if specified
                        if let Some(end_ns) = end
                            && bar.ts_event.as_u64() > end_ns
                        {
                            continue;
                        }
                        bars.push(bar);

                        // Check limit
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

            return Ok(bars);
        }

        let interval = Some(
            bar_type_to_spot_interval(bar_type)
                .map_err(|e| KrakenHttpError::ParseError(e.to_string()))?,
        );

        // Convert start time from nanoseconds to seconds
        let since = start.map(|s| (s / 1_000_000_000) as i64);

        let response = self.inner.get_ohlc(&raw_symbol, interval, since).await?;

        let mut bars = Vec::new();

        // Get the first (and typically only) pair's OHLC data
        for (_pair_name, ohlc_arrays) in &response.data {
            for ohlc_array in ohlc_arrays {
                // Convert array to OhlcData
                if ohlc_array.len() < 8 {
                    tracing::warn!("OHLC array too short: {}", ohlc_array.len());
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
                        // Filter by end time if specified
                        if let Some(end_ns) = end
                            && bar.ts_event.as_u64() > end_ns
                        {
                            continue;
                        }
                        bars.push(bar);

                        // Check limit
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
        let client = KrakenRawHttpClient::default();
        assert!(client.credential.is_none());
    }

    #[rstest]
    fn test_raw_client_with_credentials() {
        let client = KrakenRawHttpClient::with_credentials(
            "test_key".to_string(),
            "test_secret".to_string(),
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
        let client = KrakenHttpClient::default();
        assert!(client.instruments_cache.is_empty());
    }

    #[rstest]
    fn test_client_with_credentials() {
        let client = KrakenHttpClient::with_credentials(
            "test_key".to_string(),
            "test_secret".to_string(),
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
