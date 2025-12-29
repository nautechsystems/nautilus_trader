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

//! Binance HTTP client implementation.

use std::{collections::HashMap, num::NonZeroU32, sync::Arc, time::Duration};

use chrono::Utc;
use dashmap::DashMap;
use nautilus_core::consts::NAUTILUS_USER_AGENT;
use nautilus_network::{
    http::{HttpClient, HttpResponse, Method},
    ratelimiter::quota::Quota,
};
use serde::{Serialize, de::DeserializeOwned};
use ustr::Ustr;

use super::error::{BinanceHttpError, BinanceHttpResult};
use crate::{
    common::{
        consts::{
            BINANCE_DAPI_PATH, BINANCE_DAPI_RATE_LIMITS, BINANCE_EAPI_PATH,
            BINANCE_EAPI_RATE_LIMITS, BINANCE_FAPI_PATH, BINANCE_FAPI_RATE_LIMITS,
            BINANCE_SPOT_API_PATH, BINANCE_SPOT_RATE_LIMITS,
        },
        credential::Credential,
        enums::{BinanceEnvironment, BinanceProductType},
        models::BinanceErrorResponse,
        urls::get_http_base_url,
    },
    http::{
        models::{
            BinanceBookTicker, BinanceFuturesCoinExchangeInfo, BinanceFuturesCoinSymbol,
            BinanceFuturesTicker24hr, BinanceFuturesUsdExchangeInfo, BinanceFuturesUsdSymbol,
            BinanceOrderBook, BinancePriceTicker, BinanceServerTime, BinanceSpotExchangeInfo,
            BinanceSpotSymbol, BinanceSpotTicker24hr,
        },
        query::{
            BinanceBookTickerParams, BinanceDepthParams, BinancePriceTickerParams,
            BinanceSpotExchangeInfoParams, BinanceTicker24hrParams,
        },
    },
};

const BINANCE_GLOBAL_RATE_KEY: &str = "binance:global";
const BINANCE_ORDERS_RATE_KEY: &str = "binance:orders";

/// Lightweight raw HTTP client for Binance REST API access.
///
/// Handles:
/// - Base URL and API path resolution by product type/environment.
/// - Optional HMAC SHA256 signing for private endpoints.
/// - Rate limiting using documented quotas per product type.
/// - Basic error deserialization for Binance error payloads.
#[derive(Debug, Clone)]
pub struct BinanceRawHttpClient {
    client: HttpClient,
    base_url: String,
    api_path: &'static str,
    credential: Option<Credential>,
    recv_window: Option<u64>,
    order_rate_keys: Vec<String>,
}

impl BinanceRawHttpClient {
    /// Creates a new Binance raw HTTP client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying [`HttpClient`] fails to build.
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
    ) -> BinanceHttpResult<Self> {
        let RateLimitConfig {
            default_quota,
            keyed_quotas,
            order_keys,
        } = Self::rate_limit_config(product_type);

        let credential = match (api_key, api_secret) {
            (Some(key), Some(secret)) => Some(Credential::new(key, secret)),
            (None, None) => None,
            _ => return Err(BinanceHttpError::MissingCredentials),
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
    ///
    /// When `signed` is true, `timestamp`/`recvWindow` are appended and the signature is added.
    pub async fn get<P, T>(
        &self,
        path: &str,
        params: Option<&P>,
        signed: bool,
        use_order_quota: bool,
    ) -> BinanceHttpResult<T>
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
    ) -> BinanceHttpResult<T>
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
    ) -> BinanceHttpResult<T>
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
    ) -> BinanceHttpResult<T>
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
    ) -> BinanceHttpResult<T>
    where
        P: Serialize + ?Sized,
        T: DeserializeOwned,
    {
        let mut query = params
            .map(serde_urlencoded::to_string)
            .transpose()
            .map_err(|e| BinanceHttpError::ValidationError(e.to_string()))?
            .unwrap_or_default();

        let mut headers = HashMap::new();
        if signed {
            let cred = self
                .credential
                .as_ref()
                .ok_or(BinanceHttpError::MissingCredentials)?;

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
            .map_err(|e| BinanceHttpError::JsonError(e.to_string()))
    }

    #[cfg(not(test))]
    fn build_url(&self, path: &str, query: &str) -> String {
        Self::build_url_impl(&self.base_url, self.api_path, path, query)
    }

    #[cfg(test)]
    pub(crate) fn build_url(&self, path: &str, query: &str) -> String {
        Self::build_url_impl(&self.base_url, self.api_path, path, query)
    }

    fn build_url_impl(base_url: &str, api_path: &str, path: &str, query: &str) -> String {
        let mut url = format!("{}{}{}", base_url, api_path, Self::normalize_path(path));
        if !query.is_empty() {
            url.push('?');
            url.push_str(query);
        }
        url
    }

    pub(crate) fn normalize_path(path: &str) -> String {
        if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        }
    }

    #[cfg(not(test))]
    fn rate_limit_keys(&self, use_orders: bool) -> Vec<String> {
        Self::rate_limit_keys_impl(&self.order_rate_keys, use_orders)
    }

    #[cfg(test)]
    pub(crate) fn rate_limit_keys(&self, use_orders: bool) -> Vec<String> {
        Self::rate_limit_keys_impl(&self.order_rate_keys, use_orders)
    }

    fn rate_limit_keys_impl(order_rate_keys: &[String], use_orders: bool) -> Vec<String> {
        if use_orders {
            let mut keys = Vec::with_capacity(1 + order_rate_keys.len());
            keys.push(BINANCE_GLOBAL_RATE_KEY.to_string());
            keys.extend(order_rate_keys.iter().cloned());
            keys
        } else {
            vec![BINANCE_GLOBAL_RATE_KEY.to_string()]
        }
    }

    pub(crate) fn parse_error_response<T>(&self, response: HttpResponse) -> BinanceHttpResult<T> {
        let status = response.status.as_u16();
        let body = String::from_utf8_lossy(&response.body).to_string();

        if let Ok(err) = serde_json::from_str::<BinanceErrorResponse>(&body) {
            return Err(BinanceHttpError::BinanceError {
                code: err.code,
                message: err.msg,
            });
        }

        Err(BinanceHttpError::UnexpectedStatus { status, body })
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
            BinanceProductType::Spot | BinanceProductType::Margin => BINANCE_SPOT_API_PATH,
            BinanceProductType::UsdM => BINANCE_FAPI_PATH,
            BinanceProductType::CoinM => BINANCE_DAPI_PATH,
            BinanceProductType::Options => BINANCE_EAPI_PATH,
        }
    }

    pub(crate) fn rate_limit_config(product_type: BinanceProductType) -> RateLimitConfig {
        let quotas = match product_type {
            BinanceProductType::Spot | BinanceProductType::Margin => BINANCE_SPOT_RATE_LIMITS,
            BinanceProductType::UsdM => BINANCE_FAPI_RATE_LIMITS,
            BinanceProductType::CoinM => BINANCE_DAPI_RATE_LIMITS,
            BinanceProductType::Options => BINANCE_EAPI_RATE_LIMITS,
        };

        let mut keyed = Vec::new();
        let mut order_keys = Vec::new();
        let mut default = None;

        for quota in quotas {
            if let Some(q) = Self::quota_from(quota) {
                if quota.rate_limit_type == "REQUEST_WEIGHT" && default.is_none() {
                    default = Some(q);
                } else if quota.rate_limit_type == "ORDERS" {
                    let key = format!("{}:{}", BINANCE_ORDERS_RATE_KEY, quota.interval);
                    order_keys.push(key.clone());
                    keyed.push((key, q));
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

    fn quota_from(quota: &crate::common::consts::BinanceRateLimitQuota) -> Option<Quota> {
        let burst = NonZeroU32::new(quota.limit)?;
        match quota.interval {
            "SECOND" => Some(Quota::per_second(burst)),
            "MINUTE" => Some(Quota::per_minute(burst)),
            "DAY" => Quota::with_period(Duration::from_secs(86_400)).map(|q| q.allow_burst(burst)),
            _ => None,
        }
    }
}
pub(crate) struct RateLimitConfig {
    pub(crate) default_quota: Option<Quota>,
    pub(crate) keyed_quotas: Vec<(String, Quota)>,
    pub(crate) order_keys: Vec<String>,
}

/// In-memory cache entry for Binance instruments.
#[derive(Clone, Debug)]
#[allow(dead_code)]
pub enum BinanceInstrument {
    Spot(BinanceSpotSymbol),
    UsdM(BinanceFuturesUsdSymbol),
    CoinM(BinanceFuturesCoinSymbol),
}

/// Unified 24h ticker response across spot and futures products.
#[derive(Clone, Debug)]
pub enum BinanceTicker24hrEither {
    Spot(Vec<BinanceSpotTicker24hr>),
    Futures(Vec<BinanceFuturesTicker24hr>),
}

/// Higher-level HTTP client providing typed endpoints and instrument caching.
#[derive(Debug, Clone)]
pub struct BinanceHttpClient {
    raw: BinanceRawHttpClient,
    product_type: BinanceProductType,
    instruments: Arc<DashMap<Ustr, BinanceInstrument>>,
}

impl BinanceHttpClient {
    /// Creates a new [`BinanceHttpClient`] wrapping the raw client with an instrument cache.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be created or credentials are invalid.
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
    ) -> BinanceHttpResult<Self> {
        let raw = BinanceRawHttpClient::new(
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

    /// Returns a reference to the underlying raw client.
    #[must_use]
    pub const fn raw(&self) -> &BinanceRawHttpClient {
        &self.raw
    }

    /// Returns server time for the configured product type.
    pub async fn server_time(&self) -> BinanceHttpResult<BinanceServerTime> {
        self.raw
            .get::<_, BinanceServerTime>("time", None::<&()>, false, false)
            .await
    }

    /// Fetches exchange information and populates the instrument cache.
    pub async fn exchange_info(&self) -> BinanceHttpResult<()> {
        match self.product_type {
            BinanceProductType::Spot | BinanceProductType::Margin => {
                let info: BinanceSpotExchangeInfo = self
                    .raw
                    .get(
                        "exchangeInfo",
                        None::<&BinanceSpotExchangeInfoParams>,
                        false,
                        false,
                    )
                    .await?;
                for symbol in info.symbols {
                    self.instruments
                        .insert(symbol.symbol, BinanceInstrument::Spot(symbol));
                }
            }
            BinanceProductType::UsdM => {
                let info: BinanceFuturesUsdExchangeInfo = self
                    .raw
                    .get("exchangeInfo", None::<&()>, false, false)
                    .await?;
                for symbol in info.symbols {
                    self.instruments
                        .insert(symbol.symbol, BinanceInstrument::UsdM(symbol));
                }
            }
            BinanceProductType::CoinM => {
                let info: BinanceFuturesCoinExchangeInfo = self
                    .raw
                    .get("exchangeInfo", None::<&()>, false, false)
                    .await?;
                for symbol in info.symbols {
                    self.instruments
                        .insert(symbol.symbol, BinanceInstrument::CoinM(symbol));
                }
            }
            BinanceProductType::Options => {
                // Options exchange info follows similar pattern; keep placeholder for future coverage.
                return Err(BinanceHttpError::ValidationError(
                    "Options exchangeInfo not yet implemented".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Retrieves an instrument from cache, optionally fetching exchange info first.
    pub async fn get_instrument(
        &self,
        symbol: &str,
    ) -> BinanceHttpResult<Option<BinanceInstrument>> {
        let key = Ustr::from(symbol);

        if let Some(entry) = self.instruments.get(&key) {
            return Ok(Some(entry.value().clone()));
        }

        // Lazy load the cache.
        self.exchange_info().await?;
        Ok(self.instruments.get(&key).map(|e| e.value().clone()))
    }

    /// 24h ticker endpoint.
    pub async fn ticker_24h(
        &self,
        params: &BinanceTicker24hrParams,
    ) -> BinanceHttpResult<BinanceTicker24hrEither> {
        match self.product_type {
            BinanceProductType::Spot | BinanceProductType::Margin => {
                let data: Vec<BinanceSpotTicker24hr> = self
                    .raw
                    .get("ticker/24hr", Some(params), false, false)
                    .await?;
                Ok(BinanceTicker24hrEither::Spot(data))
            }
            _ => {
                let data: Vec<BinanceFuturesTicker24hr> = self
                    .raw
                    .get("ticker/24hr", Some(params), false, false)
                    .await?;
                Ok(BinanceTicker24hrEither::Futures(data))
            }
        }
    }

    /// Book ticker endpoint.
    pub async fn book_ticker(
        &self,
        params: &BinanceBookTickerParams,
    ) -> BinanceHttpResult<Vec<BinanceBookTicker>> {
        self.raw
            .get("ticker/bookTicker", Some(params), false, false)
            .await
    }

    /// Price ticker endpoint.
    pub async fn price_ticker(
        &self,
        params: &BinancePriceTickerParams,
    ) -> BinanceHttpResult<Vec<BinancePriceTicker>> {
        self.raw
            .get("ticker/price", Some(params), false, false)
            .await
    }

    /// Order book depth endpoint.
    pub async fn depth(&self, params: &BinanceDepthParams) -> BinanceHttpResult<BinanceOrderBook> {
        self.raw.get("depth", Some(params), false, false).await
    }
}

#[cfg(test)]
mod tests {
    use nautilus_network::http::{HttpStatus, StatusCode};
    use rstest::rstest;
    use tokio_util::bytes::Bytes;

    use super::*;

    // ------------------------------------------------------------------------------------------------
    // URL builder tests
    // ------------------------------------------------------------------------------------------------

    #[rstest]
    #[case("time", "/time")]
    #[case("/time", "/time")]
    #[case("ticker/24hr", "/ticker/24hr")]
    #[case("/ticker/24hr", "/ticker/24hr")]
    fn test_normalize_path(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(BinanceRawHttpClient::normalize_path(input), expected);
    }

    #[rstest]
    fn test_build_url_without_query() {
        let client = create_test_client(None);
        let url = client.build_url("time", "");

        assert_eq!(url, "https://api.binance.com/api/v3/time");
    }

    #[rstest]
    fn test_build_url_with_query() {
        let client = create_test_client(None);
        let url = client.build_url("depth", "symbol=BTCUSDT&limit=100");

        assert_eq!(
            url,
            "https://api.binance.com/api/v3/depth?symbol=BTCUSDT&limit=100"
        );
    }

    #[rstest]
    fn test_build_url_path_with_leading_slash() {
        let client = create_test_client(None);
        let url = client.build_url("/exchangeInfo", "");

        assert_eq!(url, "https://api.binance.com/api/v3/exchangeInfo");
    }

    // ------------------------------------------------------------------------------------------------
    // Error parsing tests
    // ------------------------------------------------------------------------------------------------

    #[rstest]
    fn test_parse_error_response_binance_error() {
        let client = create_test_client(None);
        let response = HttpResponse {
            status: HttpStatus::new(StatusCode::BAD_REQUEST),
            headers: HashMap::new(),
            body: Bytes::from(r#"{"code":-1121,"msg":"Invalid symbol."}"#),
        };

        let result: BinanceHttpResult<()> = client.parse_error_response(response);

        match result {
            Err(BinanceHttpError::BinanceError { code, message }) => {
                assert_eq!(code, -1121);
                assert_eq!(message, "Invalid symbol.");
            }
            other => panic!("Expected BinanceError, got {other:?}"),
        }
    }

    #[rstest]
    fn test_parse_error_response_unexpected_status_non_json() {
        let client = create_test_client(None);
        let response = HttpResponse {
            status: HttpStatus::new(StatusCode::INTERNAL_SERVER_ERROR),
            headers: HashMap::new(),
            body: Bytes::from("Internal Server Error"),
        };

        let result: BinanceHttpResult<()> = client.parse_error_response(response);

        match result {
            Err(BinanceHttpError::UnexpectedStatus { status, body }) => {
                assert_eq!(status, 500);
                assert_eq!(body, "Internal Server Error");
            }
            other => panic!("Expected UnexpectedStatus, got {other:?}"),
        }
    }

    #[rstest]
    fn test_parse_error_response_malformed_json() {
        let client = create_test_client(None);
        let response = HttpResponse {
            status: HttpStatus::new(StatusCode::BAD_REQUEST),
            headers: HashMap::new(),
            body: Bytes::from(r#"{"error": "not binance format"}"#),
        };

        let result: BinanceHttpResult<()> = client.parse_error_response(response);

        match result {
            Err(BinanceHttpError::UnexpectedStatus { status, body }) => {
                assert_eq!(status, 400);
                assert!(body.contains("not binance format"));
            }
            other => panic!("Expected UnexpectedStatus, got {other:?}"),
        }
    }

    // ------------------------------------------------------------------------------------------------
    // Rate limit wiring tests
    // ------------------------------------------------------------------------------------------------

    #[rstest]
    fn test_rate_limit_config_spot_has_request_weight_and_orders() {
        let config = BinanceRawHttpClient::rate_limit_config(BinanceProductType::Spot);

        assert!(config.default_quota.is_some());
        // Spot has 2 ORDERS quotas (SECOND and DAY)
        assert_eq!(config.order_keys.len(), 2);
        assert!(config.order_keys.iter().any(|k| k.contains("SECOND")));
        assert!(config.order_keys.iter().any(|k| k.contains("DAY")));
    }

    #[rstest]
    fn test_rate_limit_config_usdm_has_request_weight_and_orders() {
        let config = BinanceRawHttpClient::rate_limit_config(BinanceProductType::UsdM);

        assert!(config.default_quota.is_some());
        // USD-M has 2 ORDERS quotas (SECOND and MINUTE)
        assert_eq!(config.order_keys.len(), 2);
        assert!(config.order_keys.iter().any(|k| k.contains("SECOND")));
        assert!(config.order_keys.iter().any(|k| k.contains("MINUTE")));
    }

    #[rstest]
    fn test_rate_limit_config_coinm_has_request_weight_and_orders() {
        let config = BinanceRawHttpClient::rate_limit_config(BinanceProductType::CoinM);

        assert!(config.default_quota.is_some());
        // COIN-M has 2 ORDERS quotas (SECOND and MINUTE)
        assert_eq!(config.order_keys.len(), 2);
    }

    #[rstest]
    fn test_rate_limit_config_options_has_request_weight_and_orders() {
        let config = BinanceRawHttpClient::rate_limit_config(BinanceProductType::Options);

        assert!(config.default_quota.is_some());
        // Options has 2 ORDERS quotas (SECOND and MINUTE)
        assert_eq!(config.order_keys.len(), 2);
    }

    #[rstest]
    fn test_rate_limit_keys_without_orders() {
        let client = create_test_client(None);
        let keys = client.rate_limit_keys(false);

        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0], BINANCE_GLOBAL_RATE_KEY);
    }

    #[rstest]
    fn test_rate_limit_keys_with_orders() {
        let client = create_test_client(None);
        let keys = client.rate_limit_keys(true);

        // Should have global key + order keys (2 for Spot)
        assert!(keys.len() >= 2);
        assert!(keys.contains(&BINANCE_GLOBAL_RATE_KEY.to_string()));
        assert!(keys.iter().any(|k| k.starts_with(BINANCE_ORDERS_RATE_KEY)));
    }

    // ------------------------------------------------------------------------------------------------
    // Test helpers
    // ------------------------------------------------------------------------------------------------

    fn create_test_client(recv_window: Option<u64>) -> BinanceRawHttpClient {
        BinanceRawHttpClient::new(
            BinanceProductType::Spot,
            BinanceEnvironment::Mainnet,
            None,
            None,
            None,
            recv_window,
            None,
            None,
        )
        .expect("Failed to create test client")
    }
}
