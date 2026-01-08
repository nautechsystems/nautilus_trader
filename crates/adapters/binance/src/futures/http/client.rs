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

//! Binance Futures HTTP client for USD-M and COIN-M markets.

use std::{collections::HashMap, num::NonZeroU32, sync::Arc, time::Duration};

use chrono::Utc;
use dashmap::DashMap;
use nautilus_core::{consts::NAUTILUS_USER_AGENT, nanos::UnixNanos};
use nautilus_model::instruments::any::InstrumentAny;
use nautilus_network::{
    http::{HttpClient, HttpResponse, Method},
    ratelimiter::quota::Quota,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use ustr::Ustr;

use super::{
    error::{BinanceFuturesHttpError, BinanceFuturesHttpResult},
    models::{
        BinanceBookTicker, BinanceFuturesCoinExchangeInfo, BinanceFuturesCoinSymbol,
        BinanceFuturesMarkPrice, BinanceFuturesTicker24hr, BinanceFuturesUsdExchangeInfo,
        BinanceFuturesUsdSymbol, BinanceOrderBook, BinancePriceTicker, BinanceServerTime,
    },
    query::{BinanceBookTickerParams, BinanceDepthParams, BinanceTicker24hrParams},
};
use crate::common::{
    consts::{
        BINANCE_DAPI_PATH, BINANCE_DAPI_RATE_LIMITS, BINANCE_FAPI_PATH, BINANCE_FAPI_RATE_LIMITS,
        BinanceRateLimitQuota,
    },
    credential::Credential,
    enums::{
        BinanceEnvironment, BinanceProductType, BinanceRateLimitInterval, BinanceRateLimitType,
    },
    models::BinanceErrorResponse,
    parse::{parse_coinm_instrument, parse_usdm_instrument},
    urls::get_http_base_url,
};

const BINANCE_GLOBAL_RATE_KEY: &str = "binance:global";
const BINANCE_ORDERS_RATE_KEY: &str = "binance:orders";

/// Raw HTTP client for Binance Futures REST API.
#[derive(Debug, Clone)]
pub struct BinanceRawFuturesHttpClient {
    client: HttpClient,
    base_url: String,
    api_path: &'static str,
    credential: Option<Credential>,
    recv_window: Option<u64>,
    order_rate_keys: Vec<String>,
}

impl BinanceRawFuturesHttpClient {
    /// Creates a new Binance raw futures HTTP client.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        product_type: BinanceProductType,
        environment: BinanceEnvironment,
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url_override: Option<String>,
        recv_window: Option<u64>,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
    ) -> BinanceFuturesHttpResult<Self> {
        let RateLimitConfig {
            default_quota,
            keyed_quotas,
            order_keys,
        } = Self::rate_limit_config(product_type);

        let credential = match (api_key, api_secret) {
            (Some(key), Some(secret)) => Some(Credential::new(key, secret)),
            (None, None) => None,
            _ => return Err(BinanceFuturesHttpError::MissingCredentials),
        };

        let base_url = base_url_override
            .unwrap_or_else(|| get_http_base_url(product_type, environment).to_string());

        let api_path = Self::resolve_api_path(product_type);
        let headers = Self::default_headers(&credential);

        let client = HttpClient::new(
            headers,
            vec!["X-MBX-APIKEY".to_string()],
            keyed_quotas,
            default_quota,
            timeout_secs,
            proxy_url,
        )?;

        Ok(Self {
            client,
            base_url,
            api_path,
            credential,
            recv_window,
            order_rate_keys: order_keys,
        })
    }

    /// Performs a GET request and deserializes the response body.
    pub async fn get<P, T>(
        &self,
        path: &str,
        params: Option<&P>,
        signed: bool,
        use_order_quota: bool,
    ) -> BinanceFuturesHttpResult<T>
    where
        P: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        self.request(Method::GET, path, params, signed, use_order_quota, None)
            .await
    }

    /// Performs a POST request with optional body and signed query.
    pub async fn post<P, T>(
        &self,
        path: &str,
        params: Option<&P>,
        body: Option<Vec<u8>>,
        signed: bool,
        use_order_quota: bool,
    ) -> BinanceFuturesHttpResult<T>
    where
        P: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        self.request(Method::POST, path, params, signed, use_order_quota, body)
            .await
    }

    /// Performs a PUT request with signed query.
    pub async fn request_put<P, T>(
        &self,
        path: &str,
        params: Option<&P>,
        signed: bool,
        use_order_quota: bool,
    ) -> BinanceFuturesHttpResult<T>
    where
        P: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        self.request(Method::PUT, path, params, signed, use_order_quota, None)
            .await
    }

    /// Performs a DELETE request with signed query.
    pub async fn request_delete<P, T>(
        &self,
        path: &str,
        params: Option<&P>,
        signed: bool,
        use_order_quota: bool,
    ) -> BinanceFuturesHttpResult<T>
    where
        P: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        self.request(Method::DELETE, path, params, signed, use_order_quota, None)
            .await
    }

    async fn request<P, T>(
        &self,
        method: Method,
        path: &str,
        params: Option<&P>,
        signed: bool,
        use_order_quota: bool,
        body: Option<Vec<u8>>,
    ) -> BinanceFuturesHttpResult<T>
    where
        P: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        let mut query = params
            .map(serde_urlencoded::to_string)
            .transpose()
            .map_err(|e| BinanceFuturesHttpError::ValidationError(e.to_string()))?
            .unwrap_or_default();

        let mut headers = HashMap::new();
        if signed {
            let cred = self
                .credential
                .as_ref()
                .ok_or(BinanceFuturesHttpError::MissingCredentials)?;

            if !query.is_empty() {
                query.push('&');
            }

            let timestamp = Utc::now().timestamp_millis();
            query.push_str(&format!("timestamp={timestamp}"));

            if let Some(recv_window) = self.recv_window {
                query.push_str(&format!("&recvWindow={recv_window}"));
            }

            let signature = cred.sign(&query);
            query.push_str(&format!("&signature={signature}"));
            headers.insert("X-MBX-APIKEY".to_string(), cred.api_key().to_string());
        }

        let url = self.build_url(path, &query);
        let keys = self.rate_limit_keys(use_order_quota);

        let response = self
            .client
            .request(
                method,
                url,
                None::<&HashMap<String, Vec<String>>>,
                Some(headers),
                body,
                None,
                Some(keys),
            )
            .await?;

        if !response.status.is_success() {
            return self.parse_error_response(response);
        }

        serde_json::from_slice::<T>(&response.body)
            .map_err(|e| BinanceFuturesHttpError::JsonError(e.to_string()))
    }

    fn build_url(&self, path: &str, query: &str) -> String {
        let normalized = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };
        let mut url = format!("{}{}{}", self.base_url, self.api_path, normalized);
        if !query.is_empty() {
            url.push('?');
            url.push_str(query);
        }
        url
    }

    fn rate_limit_keys(&self, use_orders: bool) -> Vec<String> {
        if use_orders {
            let mut keys = Vec::with_capacity(1 + self.order_rate_keys.len());
            keys.push(BINANCE_GLOBAL_RATE_KEY.to_string());
            keys.extend(self.order_rate_keys.iter().cloned());
            keys
        } else {
            vec![BINANCE_GLOBAL_RATE_KEY.to_string()]
        }
    }

    fn parse_error_response<T>(&self, response: HttpResponse) -> BinanceFuturesHttpResult<T> {
        let status = response.status.as_u16();
        let body = String::from_utf8_lossy(&response.body).to_string();

        if let Ok(err) = serde_json::from_str::<BinanceErrorResponse>(&body) {
            return Err(BinanceFuturesHttpError::BinanceError {
                code: err.code,
                message: err.msg,
            });
        }

        Err(BinanceFuturesHttpError::UnexpectedStatus { status, body })
    }

    fn default_headers(credential: &Option<Credential>) -> HashMap<String, String> {
        let mut headers = HashMap::new();
        headers.insert("User-Agent".to_string(), NAUTILUS_USER_AGENT.to_string());
        if let Some(cred) = credential {
            headers.insert("X-MBX-APIKEY".to_string(), cred.api_key().to_string());
        }
        headers
    }

    fn resolve_api_path(product_type: BinanceProductType) -> &'static str {
        match product_type {
            BinanceProductType::UsdM => BINANCE_FAPI_PATH,
            BinanceProductType::CoinM => BINANCE_DAPI_PATH,
            _ => BINANCE_FAPI_PATH, // Default to USD-M
        }
    }

    fn rate_limit_config(product_type: BinanceProductType) -> RateLimitConfig {
        let quotas = match product_type {
            BinanceProductType::UsdM => BINANCE_FAPI_RATE_LIMITS,
            BinanceProductType::CoinM => BINANCE_DAPI_RATE_LIMITS,
            _ => BINANCE_FAPI_RATE_LIMITS,
        };

        let mut keyed = Vec::new();
        let mut order_keys = Vec::new();
        let mut default = None;

        for quota in quotas {
            if let Some(q) = Self::quota_from(quota) {
                match quota.rate_limit_type {
                    BinanceRateLimitType::RequestWeight if default.is_none() => {
                        default = Some(q);
                    }
                    BinanceRateLimitType::Orders => {
                        let key = format!("{}:{:?}", BINANCE_ORDERS_RATE_KEY, quota.interval);
                        order_keys.push(key.clone());
                        keyed.push((key, q));
                    }
                    _ => {}
                }
            }
        }

        let default_quota =
            default.unwrap_or_else(|| Quota::per_second(NonZeroU32::new(10).unwrap()));

        keyed.push((BINANCE_GLOBAL_RATE_KEY.to_string(), default_quota));

        RateLimitConfig {
            default_quota: Some(default_quota),
            keyed_quotas: keyed,
            order_keys,
        }
    }

    fn quota_from(quota: &BinanceRateLimitQuota) -> Option<Quota> {
        let burst = NonZeroU32::new(quota.limit)?;
        match quota.interval {
            BinanceRateLimitInterval::Second => Some(Quota::per_second(burst)),
            BinanceRateLimitInterval::Minute => Some(Quota::per_minute(burst)),
            BinanceRateLimitInterval::Day => {
                Quota::with_period(Duration::from_secs(86_400)).map(|q| q.allow_burst(burst))
            }
        }
    }
}

struct RateLimitConfig {
    default_quota: Option<Quota>,
    keyed_quotas: Vec<(String, Quota)>,
    order_keys: Vec<String>,
}

/// In-memory cache entry for Binance Futures instruments.
#[derive(Clone, Debug)]
pub enum BinanceFuturesInstrument {
    /// USD-M futures symbol.
    UsdM(BinanceFuturesUsdSymbol),
    /// COIN-M futures symbol.
    CoinM(BinanceFuturesCoinSymbol),
}

/// Query parameters for mark price endpoints.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MarkPriceParams {
    /// Trading symbol (optional - if omitted, returns all symbols).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
}

/// Response wrapper for mark price endpoint.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum MarkPriceResponse {
    Single(BinanceFuturesMarkPrice),
    Multiple(Vec<BinanceFuturesMarkPrice>),
}

impl From<MarkPriceResponse> for Vec<BinanceFuturesMarkPrice> {
    fn from(response: MarkPriceResponse) -> Self {
        match response {
            MarkPriceResponse::Single(price) => vec![price],
            MarkPriceResponse::Multiple(prices) => prices,
        }
    }
}

/// Query parameters for funding rate history.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FundingRateParams {
    /// Trading symbol.
    pub symbol: String,
    /// Start time in milliseconds (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<i64>,
    /// End time in milliseconds (optional).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<i64>,
    /// Limit results (default 100, max 1000).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

/// Query parameters for open interest endpoints.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenInterestParams {
    /// Trading symbol.
    pub symbol: String,
}

/// Open interest response.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceOpenInterest {
    /// Trading symbol.
    pub symbol: String,
    /// Total open interest.
    pub open_interest: String,
    /// Response timestamp.
    pub time: i64,
}

/// Funding rate history entry.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinanceFundingRate {
    /// Trading symbol.
    pub symbol: String,
    /// Funding rate value.
    pub funding_rate: String,
    /// Funding time in milliseconds.
    pub funding_time: i64,
    /// Mark price at funding time.
    #[serde(default)]
    pub mark_price: Option<String>,
}

/// Listen key response from user data stream endpoints.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListenKeyResponse {
    /// The listen key for WebSocket user data stream.
    pub listen_key: String,
}

/// Listen key request parameters.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ListenKeyParams {
    listen_key: String,
}

/// Binance Futures HTTP client for USD-M and COIN-M perpetuals.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.binance")
)]
pub struct BinanceFuturesHttpClient {
    raw: BinanceRawFuturesHttpClient,
    product_type: BinanceProductType,
    instruments: Arc<DashMap<Ustr, BinanceFuturesInstrument>>,
}

impl BinanceFuturesHttpClient {
    /// Creates a new [`BinanceFuturesHttpClient`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        product_type: BinanceProductType,
        environment: BinanceEnvironment,
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url_override: Option<String>,
        recv_window: Option<u64>,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
    ) -> BinanceFuturesHttpResult<Self> {
        match product_type {
            BinanceProductType::UsdM | BinanceProductType::CoinM => {}
            _ => {
                return Err(BinanceFuturesHttpError::ValidationError(format!(
                    "BinanceFuturesHttpClient requires UsdM or CoinM product type, got {product_type:?}"
                )));
            }
        }

        let raw = BinanceRawFuturesHttpClient::new(
            product_type,
            environment,
            api_key,
            api_secret,
            base_url_override,
            recv_window,
            timeout_secs,
            proxy_url,
        )?;

        Ok(Self {
            raw,
            product_type,
            instruments: Arc::new(DashMap::new()),
        })
    }

    /// Returns the product type (UsdM or CoinM).
    #[must_use]
    pub const fn product_type(&self) -> BinanceProductType {
        self.product_type
    }

    /// Returns a reference to the underlying raw HTTP client.
    #[must_use]
    pub const fn raw(&self) -> &BinanceRawFuturesHttpClient {
        &self.raw
    }

    /// Returns a reference to the instruments cache.
    #[must_use]
    pub fn instruments_cache(&self) -> &DashMap<Ustr, BinanceFuturesInstrument> {
        &self.instruments
    }

    /// Returns server time.
    pub async fn server_time(&self) -> BinanceFuturesHttpResult<BinanceServerTime> {
        self.raw
            .get::<_, BinanceServerTime>("time", None::<&()>, false, false)
            .await
    }

    /// Fetches exchange information and populates the instrument cache.
    pub async fn exchange_info(&self) -> BinanceFuturesHttpResult<()> {
        match self.product_type {
            BinanceProductType::UsdM => {
                let info: BinanceFuturesUsdExchangeInfo = self
                    .raw
                    .get("exchangeInfo", None::<&()>, false, false)
                    .await?;
                for symbol in info.symbols {
                    self.instruments
                        .insert(symbol.symbol, BinanceFuturesInstrument::UsdM(symbol));
                }
            }
            BinanceProductType::CoinM => {
                let info: BinanceFuturesCoinExchangeInfo = self
                    .raw
                    .get("exchangeInfo", None::<&()>, false, false)
                    .await?;
                for symbol in info.symbols {
                    self.instruments
                        .insert(symbol.symbol, BinanceFuturesInstrument::CoinM(symbol));
                }
            }
            _ => {
                return Err(BinanceFuturesHttpError::ValidationError(
                    "Invalid product type for futures".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Fetches exchange information and returns parsed Nautilus instruments.
    pub async fn request_instruments(&self) -> BinanceFuturesHttpResult<Vec<InstrumentAny>> {
        let ts_init = UnixNanos::default();

        let instruments = match self.product_type {
            BinanceProductType::UsdM => {
                let info: BinanceFuturesUsdExchangeInfo = self
                    .raw
                    .get("exchangeInfo", None::<&()>, false, false)
                    .await?;

                let mut instruments = Vec::with_capacity(info.symbols.len());
                for symbol in &info.symbols {
                    match parse_usdm_instrument(symbol, ts_init, ts_init) {
                        Ok(instrument) => instruments.push(instrument),
                        Err(e) => {
                            log::debug!(
                                "Skipping symbol during instrument parsing: symbol={}, error={e}",
                                symbol.symbol
                            );
                        }
                    }
                }

                log::info!(
                    "Loaded USD-M perpetual instruments: count={}",
                    instruments.len()
                );
                instruments
            }
            BinanceProductType::CoinM => {
                let info: BinanceFuturesCoinExchangeInfo = self
                    .raw
                    .get("exchangeInfo", None::<&()>, false, false)
                    .await?;

                let mut instruments = Vec::with_capacity(info.symbols.len());
                for symbol in &info.symbols {
                    match parse_coinm_instrument(symbol, ts_init, ts_init) {
                        Ok(instrument) => instruments.push(instrument),
                        Err(e) => {
                            log::debug!(
                                "Skipping symbol during instrument parsing: symbol={}, error={e}",
                                symbol.symbol
                            );
                        }
                    }
                }

                log::info!(
                    "Loaded COIN-M perpetual instruments: count={}",
                    instruments.len()
                );
                instruments
            }
            _ => {
                return Err(BinanceFuturesHttpError::ValidationError(
                    "Invalid product type for futures".to_string(),
                ));
            }
        };

        Ok(instruments)
    }

    /// Fetches 24hr ticker statistics.
    pub async fn ticker_24h(
        &self,
        params: &BinanceTicker24hrParams,
    ) -> BinanceFuturesHttpResult<Vec<BinanceFuturesTicker24hr>> {
        self.raw
            .get("ticker/24hr", Some(params), false, false)
            .await
    }

    /// Fetches best bid/ask prices.
    pub async fn book_ticker(
        &self,
        params: &BinanceBookTickerParams,
    ) -> BinanceFuturesHttpResult<Vec<BinanceBookTicker>> {
        self.raw
            .get("ticker/bookTicker", Some(params), false, false)
            .await
    }

    /// Fetches price ticker.
    pub async fn price_ticker(
        &self,
        symbol: Option<&str>,
    ) -> BinanceFuturesHttpResult<Vec<BinancePriceTicker>> {
        #[derive(Serialize)]
        struct Params<'a> {
            #[serde(skip_serializing_if = "Option::is_none")]
            symbol: Option<&'a str>,
        }
        self.raw
            .get("ticker/price", Some(&Params { symbol }), false, false)
            .await
    }

    /// Fetches order book depth.
    pub async fn depth(
        &self,
        params: &BinanceDepthParams,
    ) -> BinanceFuturesHttpResult<BinanceOrderBook> {
        self.raw.get("depth", Some(params), false, false).await
    }

    /// Fetches mark price and funding rate.
    pub async fn mark_price(
        &self,
        params: &MarkPriceParams,
    ) -> BinanceFuturesHttpResult<Vec<BinanceFuturesMarkPrice>> {
        let response: MarkPriceResponse = self
            .raw
            .get("premiumIndex", Some(params), false, false)
            .await?;
        Ok(response.into())
    }

    /// Fetches funding rate history.
    pub async fn funding_rate(
        &self,
        params: &FundingRateParams,
    ) -> BinanceFuturesHttpResult<Vec<BinanceFundingRate>> {
        self.raw
            .get("fundingRate", Some(params), false, false)
            .await
    }

    /// Fetches current open interest for a symbol.
    pub async fn open_interest(
        &self,
        params: &OpenInterestParams,
    ) -> BinanceFuturesHttpResult<BinanceOpenInterest> {
        self.raw
            .get("openInterest", Some(params), false, false)
            .await
    }

    /// Creates a listen key for user data stream.
    pub async fn create_listen_key(&self) -> BinanceFuturesHttpResult<ListenKeyResponse> {
        self.raw
            .post::<(), ListenKeyResponse>("listenKey", None, None, true, false)
            .await
    }

    /// Keeps alive an existing listen key.
    pub async fn keepalive_listen_key(&self, listen_key: &str) -> BinanceFuturesHttpResult<()> {
        let params = ListenKeyParams {
            listen_key: listen_key.to_string(),
        };
        let _: serde_json::Value = self
            .raw
            .request_put("listenKey", Some(&params), true, false)
            .await?;
        Ok(())
    }

    /// Closes an existing listen key.
    pub async fn close_listen_key(&self, listen_key: &str) -> BinanceFuturesHttpResult<()> {
        let params = ListenKeyParams {
            listen_key: listen_key.to_string(),
        };
        let _: serde_json::Value = self
            .raw
            .request_delete("listenKey", Some(&params), true, false)
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use nautilus_network::http::{HttpStatus, StatusCode};
    use rstest::rstest;
    use tokio_util::bytes::Bytes;

    use super::*;

    #[rstest]
    fn test_rate_limit_config_usdm_has_request_weight_and_orders() {
        let config = BinanceRawFuturesHttpClient::rate_limit_config(BinanceProductType::UsdM);

        assert!(config.default_quota.is_some());
        assert_eq!(config.order_keys.len(), 2);
        assert!(config.order_keys.iter().any(|k| k.contains("Second")));
        assert!(config.order_keys.iter().any(|k| k.contains("Minute")));
    }

    #[rstest]
    fn test_rate_limit_config_coinm_has_request_weight_and_orders() {
        let config = BinanceRawFuturesHttpClient::rate_limit_config(BinanceProductType::CoinM);

        assert!(config.default_quota.is_some());
        assert_eq!(config.order_keys.len(), 2);
    }

    #[rstest]
    fn test_create_client_rejects_spot_product_type() {
        let result = BinanceFuturesHttpClient::new(
            BinanceProductType::Spot,
            BinanceEnvironment::Mainnet,
            None,
            None,
            None,
            None,
            None,
            None,
        );

        assert!(result.is_err());
    }

    fn create_test_raw_client() -> BinanceRawFuturesHttpClient {
        BinanceRawFuturesHttpClient::new(
            BinanceProductType::UsdM,
            BinanceEnvironment::Mainnet,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("Failed to create test client")
    }

    #[rstest]
    fn test_parse_error_response_binance_error() {
        let client = create_test_raw_client();
        let response = HttpResponse {
            status: HttpStatus::new(StatusCode::BAD_REQUEST),
            headers: HashMap::new(),
            body: Bytes::from(r#"{"code":-1121,"msg":"Invalid symbol."}"#),
        };

        let result: BinanceFuturesHttpResult<()> = client.parse_error_response(response);

        match result {
            Err(BinanceFuturesHttpError::BinanceError { code, message }) => {
                assert_eq!(code, -1121);
                assert_eq!(message, "Invalid symbol.");
            }
            other => panic!("Expected BinanceError, got {other:?}"),
        }
    }
}
