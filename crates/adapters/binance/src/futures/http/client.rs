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

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use nautilus_core::{consts::NAUTILUS_USER_AGENT, nanos::UnixNanos};
use nautilus_model::{
    data::{Bar, BarType, TradeTick},
    enums::{AggregationSource, AggressorSide, BarAggregation, OrderSide, OrderType, TimeInForce},
    events::AccountState,
    identifiers::{AccountId, ClientOrderId, InstrumentId, TradeId, VenueOrderId},
    instruments::any::InstrumentAny,
    reports::{FillReport, OrderStatusReport},
    types::{Price, Quantity},
};
use nautilus_network::{
    http::{HttpClient, HttpResponse, Method},
    ratelimiter::quota::Quota,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use ustr::Ustr;

use super::{
    error::{BinanceFuturesHttpError, BinanceFuturesHttpResult},
    models::{
        BatchOrderResult, BinanceBookTicker, BinanceCancelAllOrdersResponse, BinanceFundingRate,
        BinanceFuturesAccountInfo, BinanceFuturesAlgoOrder, BinanceFuturesAlgoOrderCancelResponse,
        BinanceFuturesCoinExchangeInfo, BinanceFuturesCoinSymbol, BinanceFuturesKline,
        BinanceFuturesMarkPrice, BinanceFuturesOrder, BinanceFuturesTicker24hr,
        BinanceFuturesTrade, BinanceFuturesUsdExchangeInfo, BinanceFuturesUsdSymbol,
        BinanceHedgeModeResponse, BinanceLeverageResponse, BinanceOpenInterest, BinanceOrderBook,
        BinancePositionRisk, BinancePriceTicker, BinanceServerTime, BinanceUserTrade,
        ListenKeyResponse,
    },
    query::{
        BatchCancelItem, BatchModifyItem, BatchOrderItem, BinanceAlgoOrderQueryParams,
        BinanceAllAlgoOrdersParams, BinanceAllOrdersParams, BinanceBookTickerParams,
        BinanceCancelAllAlgoOrdersParams, BinanceCancelAllOrdersParams, BinanceCancelOrderParams,
        BinanceDepthParams, BinanceFundingRateParams, BinanceKlinesParams, BinanceMarkPriceParams,
        BinanceModifyOrderParams, BinanceNewAlgoOrderParams, BinanceNewOrderParams,
        BinanceOpenAlgoOrdersParams, BinanceOpenInterestParams, BinanceOpenOrdersParams,
        BinanceOrderQueryParams, BinancePositionRiskParams, BinanceSetLeverageParams,
        BinanceSetMarginTypeParams, BinanceTicker24hrParams, BinanceTradesParams,
        BinanceUserTradesParams, ListenKeyParams,
    },
};
use crate::common::{
    consts::{
        BINANCE_DAPI_PATH, BINANCE_DAPI_RATE_LIMITS, BINANCE_FAPI_PATH, BINANCE_FAPI_RATE_LIMITS,
        BinanceRateLimitQuota,
    },
    credential::Credential,
    enums::{
        BinanceAlgoType, BinanceEnvironment, BinanceFuturesOrderType, BinancePositionSide,
        BinanceProductType, BinanceRateLimitInterval, BinanceRateLimitType, BinanceSide,
        BinanceTimeInForce,
    },
    models::BinanceErrorResponse,
    parse::{parse_coinm_instrument, parse_usdm_instrument},
    symbol::{format_binance_symbol, format_instrument_id},
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
    /// Returns a reference to the underlying HTTP client.
    #[must_use]
    pub fn http_client(&self) -> &HttpClient {
        &self.client
    }

    /// Creates a new Binance raw futures HTTP client.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are incomplete or the HTTP client fails to build.
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
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or response deserialization fails.
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
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or response deserialization fails.
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
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or response deserialization fails.
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
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or response deserialization fails.
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

    /// Performs a batch POST request with batchOrders parameter.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or JSON parsing fails.
    pub async fn batch_request<T: Serialize>(
        &self,
        path: &str,
        items: &[T],
        use_order_quota: bool,
    ) -> BinanceFuturesHttpResult<Vec<BatchOrderResult>> {
        self.batch_request_method(Method::POST, path, items, use_order_quota)
            .await
    }

    /// Performs a batch DELETE request with batchOrders parameter.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or JSON parsing fails.
    pub async fn batch_request_delete<T: Serialize>(
        &self,
        path: &str,
        items: &[T],
        use_order_quota: bool,
    ) -> BinanceFuturesHttpResult<Vec<BatchOrderResult>> {
        self.batch_request_method(Method::DELETE, path, items, use_order_quota)
            .await
    }

    /// Performs a batch PUT request with batchOrders parameter.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or JSON parsing fails.
    pub async fn batch_request_put<T: Serialize>(
        &self,
        path: &str,
        items: &[T],
        use_order_quota: bool,
    ) -> BinanceFuturesHttpResult<Vec<BatchOrderResult>> {
        self.batch_request_method(Method::PUT, path, items, use_order_quota)
            .await
    }

    async fn batch_request_method<T: Serialize>(
        &self,
        method: Method,
        path: &str,
        items: &[T],
        use_order_quota: bool,
    ) -> BinanceFuturesHttpResult<Vec<BatchOrderResult>> {
        let cred = self
            .credential
            .as_ref()
            .ok_or(BinanceFuturesHttpError::MissingCredentials)?;

        let batch_json = serde_json::to_string(items)
            .map_err(|e| BinanceFuturesHttpError::ValidationError(e.to_string()))?;

        let encoded_batch = Self::percent_encode(&batch_json);
        let timestamp = Utc::now().timestamp_millis();
        let mut query = format!("batchOrders={encoded_batch}&timestamp={timestamp}");

        if let Some(recv_window) = self.recv_window {
            query.push_str(&format!("&recvWindow={recv_window}"));
        }

        let signature = cred.sign(&query);
        query.push_str(&format!("&signature={signature}"));

        let url = self.build_url(path, &query);

        let mut headers = HashMap::new();
        headers.insert("X-MBX-APIKEY".to_string(), cred.api_key().to_string());

        let keys = self.rate_limit_keys(use_order_quota);

        let response = self
            .client
            .request(
                method,
                url,
                None::<&HashMap<String, Vec<String>>>,
                Some(headers),
                None,
                None,
                Some(keys),
            )
            .await?;

        if !response.status.is_success() {
            return self.parse_error_response(response);
        }

        serde_json::from_slice(&response.body)
            .map_err(|e| BinanceFuturesHttpError::JsonError(e.to_string()))
    }

    /// Percent-encodes a string for use in URL query parameters.
    fn percent_encode(input: &str) -> String {
        let mut result = String::with_capacity(input.len() * 3);
        for byte in input.bytes() {
            match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    result.push(byte as char);
                }
                _ => {
                    result.push('%');
                    result.push_str(&format!("{byte:02X}"));
                }
            }
        }
        result
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
        // Full API paths (e.g., /fapi/v2/account) bypass the default api_path
        let url_path = if path.starts_with("/fapi/") || path.starts_with("/dapi/") {
            path.to_string()
        } else if path.starts_with('/') {
            format!("{}{}", self.api_path, path)
        } else {
            format!("{}/{}", self.api_path, path)
        };

        let mut url = format!("{}{}", self.base_url, url_path);
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

    /// Fetches 24hr ticker statistics.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn ticker_24h(
        &self,
        params: &BinanceTicker24hrParams,
    ) -> BinanceFuturesHttpResult<Vec<BinanceFuturesTicker24hr>> {
        self.get("ticker/24hr", Some(params), false, false).await
    }

    /// Fetches best bid/ask prices.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn book_ticker(
        &self,
        params: &BinanceBookTickerParams,
    ) -> BinanceFuturesHttpResult<Vec<BinanceBookTicker>> {
        self.get("ticker/bookTicker", Some(params), false, false)
            .await
    }

    /// Fetches price ticker.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn price_ticker(
        &self,
        symbol: Option<&str>,
    ) -> BinanceFuturesHttpResult<Vec<BinancePriceTicker>> {
        #[derive(Serialize)]
        struct Params<'a> {
            #[serde(skip_serializing_if = "Option::is_none")]
            symbol: Option<&'a str>,
        }
        self.get("ticker/price", Some(&Params { symbol }), false, false)
            .await
    }

    /// Fetches order book depth.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn depth(
        &self,
        params: &BinanceDepthParams,
    ) -> BinanceFuturesHttpResult<BinanceOrderBook> {
        self.get("depth", Some(params), false, false).await
    }

    /// Fetches mark price and funding rate.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn mark_price(
        &self,
        params: &BinanceMarkPriceParams,
    ) -> BinanceFuturesHttpResult<Vec<BinanceFuturesMarkPrice>> {
        let response: MarkPriceResponse =
            self.get("premiumIndex", Some(params), false, false).await?;
        Ok(response.into())
    }

    /// Fetches funding rate history.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn funding_rate(
        &self,
        params: &BinanceFundingRateParams,
    ) -> BinanceFuturesHttpResult<Vec<BinanceFundingRate>> {
        self.get("fundingRate", Some(params), false, false).await
    }

    /// Fetches current open interest for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn open_interest(
        &self,
        params: &BinanceOpenInterestParams,
    ) -> BinanceFuturesHttpResult<BinanceOpenInterest> {
        self.get("openInterest", Some(params), false, false).await
    }

    /// Fetches recent public trades for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn trades(
        &self,
        params: &BinanceTradesParams,
    ) -> BinanceFuturesHttpResult<Vec<BinanceFuturesTrade>> {
        self.get("trades", Some(params), false, false).await
    }

    /// Fetches kline/candlestick data for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn klines(
        &self,
        params: &BinanceKlinesParams,
    ) -> BinanceFuturesHttpResult<Vec<BinanceFuturesKline>> {
        self.get("klines", Some(params), false, false).await
    }

    /// Sets leverage for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn set_leverage(
        &self,
        params: &BinanceSetLeverageParams,
    ) -> BinanceFuturesHttpResult<BinanceLeverageResponse> {
        self.post("leverage", Some(params), None, true, false).await
    }

    /// Sets margin type for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn set_margin_type(
        &self,
        params: &BinanceSetMarginTypeParams,
    ) -> BinanceFuturesHttpResult<serde_json::Value> {
        self.post("marginType", Some(params), None, true, false)
            .await
    }

    /// Queries hedge mode (dual side position) setting.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn query_hedge_mode(&self) -> BinanceFuturesHttpResult<BinanceHedgeModeResponse> {
        self.get::<(), _>("positionSide/dual", None, true, false)
            .await
    }

    /// Creates a listen key for user data stream.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn create_listen_key(&self) -> BinanceFuturesHttpResult<ListenKeyResponse> {
        self.post::<(), ListenKeyResponse>("listenKey", None, None, true, false)
            .await
    }

    /// Keeps alive an existing listen key.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn keepalive_listen_key(&self, listen_key: &str) -> BinanceFuturesHttpResult<()> {
        let params = ListenKeyParams {
            listen_key: listen_key.to_string(),
        };
        let _: serde_json::Value = self
            .request_put("listenKey", Some(&params), true, false)
            .await?;
        Ok(())
    }

    /// Closes an existing listen key.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn close_listen_key(&self, listen_key: &str) -> BinanceFuturesHttpResult<()> {
        let params = ListenKeyParams {
            listen_key: listen_key.to_string(),
        };
        let _: serde_json::Value = self
            .request_delete("listenKey", Some(&params), true, false)
            .await?;
        Ok(())
    }

    /// Fetches account information including balances and positions.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn query_account(&self) -> BinanceFuturesHttpResult<BinanceFuturesAccountInfo> {
        // USD-M uses /fapi/v2/account, COIN-M uses /dapi/v1/account
        let path = if self.api_path.starts_with("/fapi") {
            "/fapi/v2/account"
        } else {
            "/dapi/v1/account"
        };
        self.get::<(), _>(path, None, true, false).await
    }

    /// Fetches position risk information.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn query_positions(
        &self,
        params: &BinancePositionRiskParams,
    ) -> BinanceFuturesHttpResult<Vec<BinancePositionRisk>> {
        // USD-M uses /fapi/v2/positionRisk, COIN-M uses /dapi/v1/positionRisk
        let path = if self.api_path.starts_with("/fapi") {
            "/fapi/v2/positionRisk"
        } else {
            "/dapi/v1/positionRisk"
        };
        self.get(path, Some(params), true, false).await
    }

    /// Fetches user trades for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn query_user_trades(
        &self,
        params: &BinanceUserTradesParams,
    ) -> BinanceFuturesHttpResult<Vec<BinanceUserTrade>> {
        self.get("userTrades", Some(params), true, false).await
    }

    /// Queries a single order by order ID or client order ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn query_order(
        &self,
        params: &BinanceOrderQueryParams,
    ) -> BinanceFuturesHttpResult<BinanceFuturesOrder> {
        self.get("order", Some(params), true, false).await
    }

    /// Queries all open orders.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn query_open_orders(
        &self,
        params: &BinanceOpenOrdersParams,
    ) -> BinanceFuturesHttpResult<Vec<BinanceFuturesOrder>> {
        self.get("openOrders", Some(params), true, false).await
    }

    /// Queries all orders (including historical).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn query_all_orders(
        &self,
        params: &BinanceAllOrdersParams,
    ) -> BinanceFuturesHttpResult<Vec<BinanceFuturesOrder>> {
        self.get("allOrders", Some(params), true, false).await
    }

    /// Submits a new order.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn submit_order(
        &self,
        params: &BinanceNewOrderParams,
    ) -> BinanceFuturesHttpResult<BinanceFuturesOrder> {
        self.post("order", Some(params), None, true, true).await
    }

    /// Submits multiple orders in a single request (up to 5 orders).
    ///
    /// # Errors
    ///
    /// Returns an error if the batch exceeds 5 orders or the request fails.
    pub async fn submit_order_list(
        &self,
        orders: &[BatchOrderItem],
    ) -> BinanceFuturesHttpResult<Vec<BatchOrderResult>> {
        if orders.is_empty() {
            return Ok(Vec::new());
        }

        if orders.len() > 5 {
            return Err(BinanceFuturesHttpError::ValidationError(
                "Batch order limit is 5 orders maximum".to_string(),
            ));
        }

        self.batch_request("batchOrders", orders, true).await
    }

    /// Modifies an existing order (price and quantity only).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn modify_order(
        &self,
        params: &BinanceModifyOrderParams,
    ) -> BinanceFuturesHttpResult<BinanceFuturesOrder> {
        self.request_put("order", Some(params), true, true).await
    }

    /// Modifies multiple orders in a single request (up to 5 orders).
    ///
    /// # Errors
    ///
    /// Returns an error if the batch exceeds 5 orders or the request fails.
    pub async fn batch_modify_orders(
        &self,
        modifies: &[BatchModifyItem],
    ) -> BinanceFuturesHttpResult<Vec<BatchOrderResult>> {
        if modifies.is_empty() {
            return Ok(Vec::new());
        }

        if modifies.len() > 5 {
            return Err(BinanceFuturesHttpError::ValidationError(
                "Batch modify limit is 5 orders maximum".to_string(),
            ));
        }

        self.batch_request_put("batchOrders", modifies, true).await
    }

    /// Cancels an existing order.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn cancel_order(
        &self,
        params: &BinanceCancelOrderParams,
    ) -> BinanceFuturesHttpResult<BinanceFuturesOrder> {
        self.request_delete("order", Some(params), true, true).await
    }

    /// Cancels all open orders for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn cancel_all_orders(
        &self,
        params: &BinanceCancelAllOrdersParams,
    ) -> BinanceFuturesHttpResult<BinanceCancelAllOrdersResponse> {
        self.request_delete("allOpenOrders", Some(params), true, true)
            .await
    }

    /// Cancels multiple orders in a single request (up to 5 orders).
    ///
    /// # Errors
    ///
    /// Returns an error if the batch exceeds 5 orders or the request fails.
    pub async fn batch_cancel_orders(
        &self,
        cancels: &[BatchCancelItem],
    ) -> BinanceFuturesHttpResult<Vec<BatchOrderResult>> {
        if cancels.is_empty() {
            return Ok(Vec::new());
        }

        if cancels.len() > 5 {
            return Err(BinanceFuturesHttpError::ValidationError(
                "Batch cancel limit is 5 orders maximum".to_string(),
            ));
        }

        self.batch_request_delete("batchOrders", cancels, true)
            .await
    }

    /// Submits a new algo order (conditional order).
    ///
    /// Algo orders include STOP_MARKET, STOP (stop-limit), TAKE_PROFIT, TAKE_PROFIT_MARKET,
    /// and TRAILING_STOP_MARKET order types.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn submit_algo_order(
        &self,
        params: &BinanceNewAlgoOrderParams,
    ) -> BinanceFuturesHttpResult<BinanceFuturesAlgoOrder> {
        self.post("algoOrder", Some(params), None, true, true).await
    }

    /// Cancels an algo order.
    ///
    /// Must provide either `algo_id` or `client_algo_id`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn cancel_algo_order(
        &self,
        params: &BinanceAlgoOrderQueryParams,
    ) -> BinanceFuturesHttpResult<BinanceFuturesAlgoOrderCancelResponse> {
        self.request_delete("algoOrder", Some(params), true, true)
            .await
    }

    /// Queries a single algo order.
    ///
    /// Must provide either `algo_id` or `client_algo_id`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn query_algo_order(
        &self,
        params: &BinanceAlgoOrderQueryParams,
    ) -> BinanceFuturesHttpResult<BinanceFuturesAlgoOrder> {
        self.get("algoOrder", Some(params), true, false).await
    }

    /// Queries all open algo orders.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn query_open_algo_orders(
        &self,
        params: &BinanceOpenAlgoOrdersParams,
    ) -> BinanceFuturesHttpResult<Vec<BinanceFuturesAlgoOrder>> {
        self.get("openAlgoOrders", Some(params), true, false).await
    }

    /// Queries all algo orders including historical (7-day limit).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn query_all_algo_orders(
        &self,
        params: &BinanceAllAlgoOrdersParams,
    ) -> BinanceFuturesHttpResult<Vec<BinanceFuturesAlgoOrder>> {
        self.get("allAlgoOrders", Some(params), true, false).await
    }

    /// Cancels all open algo orders for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn cancel_all_algo_orders(
        &self,
        params: &BinanceCancelAllAlgoOrdersParams,
    ) -> BinanceFuturesHttpResult<BinanceCancelAllOrdersResponse> {
        self.request_delete("algoOpenOrders", Some(params), true, true)
            .await
    }
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

impl BinanceFuturesInstrument {
    /// Returns the symbol name for the instrument.
    #[must_use]
    pub const fn symbol(&self) -> Ustr {
        match self {
            Self::UsdM(s) => s.symbol,
            Self::CoinM(s) => s.symbol,
        }
    }

    /// Returns the price precision for the instrument.
    #[must_use]
    pub const fn price_precision(&self) -> i32 {
        match self {
            Self::UsdM(s) => s.price_precision,
            Self::CoinM(s) => s.price_precision,
        }
    }

    /// Returns the quantity precision for the instrument.
    #[must_use]
    pub const fn quantity_precision(&self) -> i32 {
        match self {
            Self::UsdM(s) => s.quantity_precision,
            Self::CoinM(s) => s.quantity_precision,
        }
    }

    /// Returns the Nautilus-formatted instrument ID.
    #[must_use]
    pub fn id(&self) -> InstrumentId {
        match self {
            Self::UsdM(s) => format_instrument_id(&s.symbol, BinanceProductType::UsdM),
            Self::CoinM(s) => format_instrument_id(&s.symbol, BinanceProductType::CoinM),
        }
    }
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
    ///
    /// # Errors
    ///
    /// Returns an error if the product type is invalid or HTTP client creation fails.
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
                    "BinanceFuturesHttpClient requires UsdM or CoinM product type, was {product_type:?}"
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

    /// Returns a clone of the instruments cache Arc.
    #[must_use]
    pub fn instruments_cache(&self) -> Arc<DashMap<Ustr, BinanceFuturesInstrument>> {
        Arc::clone(&self.instruments)
    }

    /// Returns server time.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn server_time(&self) -> BinanceFuturesHttpResult<BinanceServerTime> {
        self.raw
            .get::<_, BinanceServerTime>("time", None::<&()>, false, false)
            .await
    }

    /// Sets leverage for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn set_leverage(
        &self,
        params: &BinanceSetLeverageParams,
    ) -> BinanceFuturesHttpResult<BinanceLeverageResponse> {
        self.raw.set_leverage(params).await
    }

    /// Sets margin type for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn set_margin_type(
        &self,
        params: &BinanceSetMarginTypeParams,
    ) -> BinanceFuturesHttpResult<serde_json::Value> {
        self.raw.set_margin_type(params).await
    }

    /// Queries hedge mode (dual side position) setting.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn query_hedge_mode(&self) -> BinanceFuturesHttpResult<BinanceHedgeModeResponse> {
        self.raw.query_hedge_mode().await
    }

    /// Creates a listen key for user data stream.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn create_listen_key(&self) -> BinanceFuturesHttpResult<ListenKeyResponse> {
        self.raw.create_listen_key().await
    }

    /// Keeps alive an existing listen key.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn keepalive_listen_key(&self, listen_key: &str) -> BinanceFuturesHttpResult<()> {
        self.raw.keepalive_listen_key(listen_key).await
    }

    /// Closes an existing listen key.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn close_listen_key(&self, listen_key: &str) -> BinanceFuturesHttpResult<()> {
        self.raw.close_listen_key(listen_key).await
    }

    /// Fetches exchange information and populates the instrument cache.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the product type is invalid.
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
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the product type is invalid.
    pub async fn request_instruments(&self) -> BinanceFuturesHttpResult<Vec<InstrumentAny>> {
        let ts_init = UnixNanos::default();

        let instruments = match self.product_type {
            BinanceProductType::UsdM => {
                let info: BinanceFuturesUsdExchangeInfo = self
                    .raw
                    .get("exchangeInfo", None::<&()>, false, false)
                    .await?;

                let mut instruments = Vec::with_capacity(info.symbols.len());

                for symbol in info.symbols {
                    // Cache symbol for precision lookups
                    self.instruments.insert(
                        symbol.symbol,
                        BinanceFuturesInstrument::UsdM(symbol.clone()),
                    );

                    match parse_usdm_instrument(&symbol, ts_init, ts_init) {
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
                for symbol in info.symbols {
                    // Cache symbol for precision lookups
                    self.instruments.insert(
                        symbol.symbol,
                        BinanceFuturesInstrument::CoinM(symbol.clone()),
                    );

                    match parse_coinm_instrument(&symbol, ts_init, ts_init) {
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
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn ticker_24h(
        &self,
        params: &BinanceTicker24hrParams,
    ) -> BinanceFuturesHttpResult<Vec<BinanceFuturesTicker24hr>> {
        self.raw.ticker_24h(params).await
    }

    /// Fetches best bid/ask prices.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn book_ticker(
        &self,
        params: &BinanceBookTickerParams,
    ) -> BinanceFuturesHttpResult<Vec<BinanceBookTicker>> {
        self.raw.book_ticker(params).await
    }

    /// Fetches price ticker.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn price_ticker(
        &self,
        symbol: Option<&str>,
    ) -> BinanceFuturesHttpResult<Vec<BinancePriceTicker>> {
        self.raw.price_ticker(symbol).await
    }

    /// Fetches order book depth.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn depth(
        &self,
        params: &BinanceDepthParams,
    ) -> BinanceFuturesHttpResult<BinanceOrderBook> {
        self.raw.depth(params).await
    }

    /// Fetches mark price and funding rate.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn mark_price(
        &self,
        params: &BinanceMarkPriceParams,
    ) -> BinanceFuturesHttpResult<Vec<BinanceFuturesMarkPrice>> {
        self.raw.mark_price(params).await
    }

    /// Fetches funding rate history.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn funding_rate(
        &self,
        params: &BinanceFundingRateParams,
    ) -> BinanceFuturesHttpResult<Vec<BinanceFundingRate>> {
        self.raw.funding_rate(params).await
    }

    /// Fetches current open interest for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn open_interest(
        &self,
        params: &BinanceOpenInterestParams,
    ) -> BinanceFuturesHttpResult<BinanceOpenInterest> {
        self.raw.open_interest(params).await
    }

    /// Queries a single order by order ID or client order ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn query_order(
        &self,
        params: &BinanceOrderQueryParams,
    ) -> BinanceFuturesHttpResult<BinanceFuturesOrder> {
        self.raw.query_order(params).await
    }

    /// Queries all open orders.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn query_open_orders(
        &self,
        params: &BinanceOpenOrdersParams,
    ) -> BinanceFuturesHttpResult<Vec<BinanceFuturesOrder>> {
        self.raw.query_open_orders(params).await
    }

    /// Queries all orders (including historical).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn query_all_orders(
        &self,
        params: &BinanceAllOrdersParams,
    ) -> BinanceFuturesHttpResult<Vec<BinanceFuturesOrder>> {
        self.raw.query_all_orders(params).await
    }

    /// Fetches account information including balances and positions.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn query_account(&self) -> BinanceFuturesHttpResult<BinanceFuturesAccountInfo> {
        self.raw.query_account().await
    }

    /// Fetches position risk information.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn query_positions(
        &self,
        params: &BinancePositionRiskParams,
    ) -> BinanceFuturesHttpResult<Vec<BinancePositionRisk>> {
        self.raw.query_positions(params).await
    }

    /// Fetches user trades for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn query_user_trades(
        &self,
        params: &BinanceUserTradesParams,
    ) -> BinanceFuturesHttpResult<Vec<BinanceUserTrade>> {
        self.raw.query_user_trades(params).await
    }

    /// Submits a new order.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The instrument is not cached.
    /// - The order type or time-in-force is unsupported.
    /// - Stop orders are submitted without a trigger price.
    /// - The request fails.
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
        position_side: Option<BinancePositionSide>,
    ) -> anyhow::Result<OrderStatusReport> {
        let symbol = format_binance_symbol(&instrument_id);
        let size_precision = self.get_size_precision(&symbol)?;

        let binance_side = BinanceSide::try_from(order_side)?;
        let binance_order_type = order_type_to_binance_futures(order_type)?;
        let binance_tif = BinanceTimeInForce::try_from(time_in_force)?;

        let requires_trigger_price = matches!(
            order_type,
            OrderType::StopMarket
                | OrderType::StopLimit
                | OrderType::TrailingStopMarket
                | OrderType::MarketIfTouched
                | OrderType::LimitIfTouched
        );
        if requires_trigger_price && trigger_price.is_none() {
            anyhow::bail!("Order type {order_type:?} requires a trigger price");
        }

        // MARKET and STOP_MARKET orders don't accept timeInForce
        let requires_time_in_force = matches!(
            order_type,
            OrderType::Limit | OrderType::StopLimit | OrderType::LimitIfTouched
        );

        let qty_str = quantity.to_string();
        let price_str = price.map(|p| p.to_string());
        let stop_price_str = trigger_price.map(|p| p.to_string());
        let client_id_str = client_order_id.to_string();

        let params = BinanceNewOrderParams {
            symbol,
            side: binance_side,
            order_type: binance_order_type,
            time_in_force: if requires_time_in_force {
                Some(binance_tif)
            } else {
                None
            },
            quantity: Some(qty_str),
            price: price_str,
            new_client_order_id: Some(client_id_str),
            stop_price: stop_price_str,
            reduce_only: if reduce_only { Some(true) } else { None },
            position_side,
            close_position: None,
            activation_price: None,
            callback_rate: None,
            working_type: None,
            price_protect: None,
            new_order_resp_type: None,
            good_till_date: None,
            recv_window: None,
            price_match: None,
            self_trade_prevention_mode: None,
        };

        let order = self.raw.submit_order(&params).await?;
        order.to_order_status_report(account_id, instrument_id, size_precision)
    }

    /// Submits an algo order (conditional order) to the Binance Algo Service.
    ///
    /// As of 2025-12-09, Binance migrated conditional order types to the Algo Service API.
    /// This method handles StopMarket, StopLimit, MarketIfTouched, LimitIfTouched,
    /// and TrailingStopMarket orders.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The order type requires a trigger price but none is provided.
    /// - The instrument is not cached.
    /// - The request fails.
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_algo_order(
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
        position_side: Option<BinancePositionSide>,
    ) -> anyhow::Result<OrderStatusReport> {
        let symbol = format_binance_symbol(&instrument_id);
        let size_precision = self.get_size_precision(&symbol)?;

        let binance_side = BinanceSide::try_from(order_side)?;
        let binance_order_type = order_type_to_binance_futures(order_type)?;
        let binance_tif = BinanceTimeInForce::try_from(time_in_force)?;

        anyhow::ensure!(
            trigger_price.is_some(),
            "Algo order type {order_type:?} requires a trigger price"
        );

        // Limit orders require time in force
        let requires_time_in_force =
            matches!(order_type, OrderType::StopLimit | OrderType::LimitIfTouched);

        let qty_str = quantity.to_string();
        let price_str = price.map(|p| p.to_string());
        let trigger_price_str = trigger_price.map(|p| p.to_string());
        let client_id_str = client_order_id.to_string();

        let params = BinanceNewAlgoOrderParams {
            symbol,
            side: binance_side,
            order_type: binance_order_type,
            algo_type: BinanceAlgoType::Conditional,
            position_side,
            quantity: Some(qty_str),
            price: price_str,
            trigger_price: trigger_price_str,
            time_in_force: if requires_time_in_force {
                Some(binance_tif)
            } else {
                None
            },
            working_type: None,
            close_position: None,
            price_protect: None,
            reduce_only: if reduce_only { Some(true) } else { None },
            activation_price: None,
            callback_rate: None,
            client_algo_id: Some(client_id_str),
            good_till_date: None,
            recv_window: None,
        };

        let order = self.raw.submit_algo_order(&params).await?;
        order.to_order_status_report(account_id, instrument_id, size_precision)
    }

    /// Submits multiple orders in a single request (up to 5 orders).
    ///
    /// Each order in the batch is processed independently. The response contains
    /// the result for each order, which can be either a success or an error.
    ///
    /// # Errors
    ///
    /// Returns an error if the batch exceeds 5 orders or the request fails.
    pub async fn submit_order_list(
        &self,
        orders: &[BatchOrderItem],
    ) -> BinanceFuturesHttpResult<Vec<BatchOrderResult>> {
        self.raw.submit_order_list(orders).await
    }

    /// Modifies an existing order (price and quantity only).
    ///
    /// Either `venue_order_id` or `client_order_id` must be provided.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Neither venue_order_id nor client_order_id is provided.
    /// - The instrument is not cached.
    /// - The request fails.
    #[allow(clippy::too_many_arguments)]
    pub async fn modify_order(
        &self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        venue_order_id: Option<VenueOrderId>,
        client_order_id: Option<ClientOrderId>,
        order_side: OrderSide,
        quantity: Quantity,
        price: Price,
    ) -> anyhow::Result<OrderStatusReport> {
        anyhow::ensure!(
            venue_order_id.is_some() || client_order_id.is_some(),
            "Either venue_order_id or client_order_id must be provided"
        );

        let symbol = format_binance_symbol(&instrument_id);
        let size_precision = self.get_size_precision(&symbol)?;

        let binance_side = BinanceSide::try_from(order_side)?;

        let order_id = venue_order_id
            .map(|id| id.inner().parse::<i64>())
            .transpose()
            .map_err(|_| anyhow::anyhow!("Invalid venue order ID"))?;

        let params = BinanceModifyOrderParams {
            symbol,
            order_id,
            orig_client_order_id: client_order_id.map(|id| id.to_string()),
            side: binance_side,
            quantity: quantity.to_string(),
            price: price.to_string(),
            recv_window: None,
        };

        let order = self.raw.modify_order(&params).await?;
        order.to_order_status_report(account_id, instrument_id, size_precision)
    }

    /// Modifies multiple orders in a single request (up to 5 orders).
    ///
    /// Each modify in the batch is processed independently. The response contains
    /// the result for each modify, which can be either a success or an error.
    ///
    /// # Errors
    ///
    /// Returns an error if the batch exceeds 5 orders or the request fails.
    pub async fn batch_modify_orders(
        &self,
        modifies: &[BatchModifyItem],
    ) -> BinanceFuturesHttpResult<Vec<BatchOrderResult>> {
        self.raw.batch_modify_orders(modifies).await
    }

    /// Cancels an order by venue order ID or client order ID.
    ///
    /// Either `venue_order_id` or `client_order_id` must be provided.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Neither venue_order_id nor client_order_id is provided.
    /// - The request fails.
    pub async fn cancel_order(
        &self,
        instrument_id: InstrumentId,
        venue_order_id: Option<VenueOrderId>,
        client_order_id: Option<ClientOrderId>,
    ) -> anyhow::Result<VenueOrderId> {
        anyhow::ensure!(
            venue_order_id.is_some() || client_order_id.is_some(),
            "Either venue_order_id or client_order_id must be provided"
        );

        let symbol = format_binance_symbol(&instrument_id);

        let order_id = venue_order_id
            .map(|id| id.inner().parse::<i64>())
            .transpose()
            .map_err(|_| anyhow::anyhow!("Invalid venue order ID"))?;

        let params = BinanceCancelOrderParams {
            symbol,
            order_id,
            orig_client_order_id: client_order_id.map(|id| id.to_string()),
            recv_window: None,
        };

        let order = self.raw.cancel_order(&params).await?;
        Ok(VenueOrderId::new(order.order_id.to_string()))
    }

    /// Cancels an algo order (conditional order) via the Binance Algo Service.
    ///
    /// Use the `client_algo_id` which corresponds to the `client_order_id` used
    /// when submitting the algo order.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn cancel_algo_order(&self, client_order_id: ClientOrderId) -> anyhow::Result<()> {
        let params = BinanceAlgoOrderQueryParams {
            algo_id: None,
            client_algo_id: Some(client_order_id.to_string()),
            recv_window: None,
        };

        let response = self.raw.cancel_algo_order(&params).await?;
        if response.code == 200 {
            Ok(())
        } else {
            anyhow::bail!(
                "Cancel algo order failed: code={}, msg={}",
                response.code,
                response.msg
            )
        }
    }

    /// Cancels all open orders for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn cancel_all_orders(
        &self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<Vec<VenueOrderId>> {
        let symbol = format_binance_symbol(&instrument_id);

        let params = BinanceCancelAllOrdersParams {
            symbol,
            recv_window: None,
        };

        let response = self.raw.cancel_all_orders(&params).await?;
        if response.code == 200 {
            Ok(vec![])
        } else {
            anyhow::bail!("Cancel all orders failed: {}", response.msg);
        }
    }

    /// Cancels all open algo orders for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn cancel_all_algo_orders(&self, instrument_id: InstrumentId) -> anyhow::Result<()> {
        let symbol = format_binance_symbol(&instrument_id);

        let params = BinanceCancelAllAlgoOrdersParams {
            symbol,
            recv_window: None,
        };

        let response = self.raw.cancel_all_algo_orders(&params).await?;
        if response.code == 200 {
            Ok(())
        } else {
            anyhow::bail!("Cancel all algo orders failed: {}", response.msg);
        }
    }

    /// Cancels multiple orders in a single request (up to 5 orders).
    ///
    /// Each cancel in the batch is processed independently. The response contains
    /// the result for each cancel, which can be either a success or an error.
    ///
    /// # Errors
    ///
    /// Returns an error if the batch exceeds 5 orders or the request fails.
    pub async fn batch_cancel_orders(
        &self,
        cancels: &[BatchCancelItem],
    ) -> BinanceFuturesHttpResult<Vec<BatchOrderResult>> {
        self.raw.batch_cancel_orders(cancels).await
    }

    /// Queries open algo orders (conditional orders).
    ///
    /// Returns all open algo orders, optionally filtered by symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn query_open_algo_orders(
        &self,
        instrument_id: Option<InstrumentId>,
    ) -> BinanceFuturesHttpResult<Vec<BinanceFuturesAlgoOrder>> {
        let symbol = instrument_id.map(|id| format_binance_symbol(&id));

        let params = BinanceOpenAlgoOrdersParams {
            symbol,
            recv_window: None,
        };

        self.raw.query_open_algo_orders(&params).await
    }

    /// Queries a single algo order by client_order_id.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn query_algo_order(
        &self,
        client_order_id: ClientOrderId,
    ) -> BinanceFuturesHttpResult<BinanceFuturesAlgoOrder> {
        let params = BinanceAlgoOrderQueryParams {
            algo_id: None,
            client_algo_id: Some(client_order_id.to_string()),
            recv_window: None,
        };

        self.raw.query_algo_order(&params).await
    }

    /// Returns the size precision for an instrument from the cache.
    fn get_size_precision(&self, symbol: &str) -> anyhow::Result<u8> {
        let instrument = self
            .instruments
            .get(&Ustr::from(symbol))
            .ok_or_else(|| anyhow::anyhow!("Instrument not found in cache: {symbol}"))?;

        let precision = match instrument.value() {
            BinanceFuturesInstrument::UsdM(s) => s.quantity_precision,
            BinanceFuturesInstrument::CoinM(s) => s.quantity_precision,
        };

        Ok(precision as u8)
    }

    /// Returns the price precision for an instrument from the cache.
    fn get_price_precision(&self, symbol: &str) -> anyhow::Result<u8> {
        let instrument = self
            .instruments
            .get(&Ustr::from(symbol))
            .ok_or_else(|| anyhow::anyhow!("Instrument not found in cache: {symbol}"))?;

        let precision = match instrument.value() {
            BinanceFuturesInstrument::UsdM(s) => s.price_precision,
            BinanceFuturesInstrument::CoinM(s) => s.price_precision,
        };

        Ok(precision as u8)
    }

    /// Requests the current account state.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or parsing fails.
    pub async fn request_account_state(
        &self,
        account_id: AccountId,
    ) -> anyhow::Result<AccountState> {
        let ts_init = UnixNanos::default();
        let account_info = self.raw.query_account().await?;
        account_info.to_account_state(account_id, ts_init)
    }

    /// Requests a single order status report.
    ///
    /// Either `venue_order_id` or `client_order_id` must be provided.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or parsing fails.
    pub async fn request_order_status_report(
        &self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        venue_order_id: Option<VenueOrderId>,
        client_order_id: Option<ClientOrderId>,
    ) -> anyhow::Result<OrderStatusReport> {
        anyhow::ensure!(
            venue_order_id.is_some() || client_order_id.is_some(),
            "Either venue_order_id or client_order_id must be provided"
        );

        let symbol = format_binance_symbol(&instrument_id);
        let size_precision = self.get_size_precision(&symbol)?;

        let order_id = venue_order_id
            .map(|id| id.inner().parse::<i64>())
            .transpose()
            .map_err(|_| anyhow::anyhow!("Invalid venue order ID"))?;

        let orig_client_order_id = client_order_id.map(|id| id.to_string());

        let params = BinanceOrderQueryParams {
            symbol,
            order_id,
            orig_client_order_id,
            recv_window: None,
        };

        let order = self.raw.query_order(&params).await?;
        order.to_order_status_report(account_id, instrument_id, size_precision)
    }

    /// Requests order status reports for open orders.
    ///
    /// If `instrument_id` is None, returns all open orders.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or parsing fails.
    pub async fn request_order_status_reports(
        &self,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
        open_only: bool,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let symbol = instrument_id.map(|id| format_binance_symbol(&id));

        let orders = if open_only {
            let params = BinanceOpenOrdersParams {
                symbol: symbol.clone(),
                recv_window: None,
            };
            self.raw.query_open_orders(&params).await?
        } else {
            // For historical orders, symbol is required
            let symbol = symbol.ok_or_else(|| {
                anyhow::anyhow!("instrument_id is required for historical orders")
            })?;
            let params = BinanceAllOrdersParams {
                symbol,
                order_id: None,
                start_time: None,
                end_time: None,
                limit: None,
                recv_window: None,
            };
            self.raw.query_all_orders(&params).await?
        };

        let mut reports = Vec::with_capacity(orders.len());

        for order in orders {
            let order_instrument_id = instrument_id.unwrap_or_else(|| {
                // Build instrument ID from order symbol
                let suffix = self.product_type.suffix();
                InstrumentId::from(format!("{}{}.BINANCE", order.symbol, suffix))
            });

            let size_precision = self.get_size_precision(&order.symbol).unwrap_or(8); // Default precision if not in cache

            match order.to_order_status_report(account_id, order_instrument_id, size_precision) {
                Ok(report) => reports.push(report),
                Err(e) => {
                    log::warn!("Failed to parse order status report: {e}");
                }
            }
        }

        Ok(reports)
    }

    /// Requests fill reports for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or parsing fails.
    pub async fn request_fill_reports(
        &self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        venue_order_id: Option<VenueOrderId>,
        start: Option<i64>,
        end: Option<i64>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<FillReport>> {
        let symbol = format_binance_symbol(&instrument_id);
        let size_precision = self.get_size_precision(&symbol)?;
        let price_precision = self.get_price_precision(&symbol)?;

        let order_id = venue_order_id
            .map(|id| id.inner().parse::<i64>())
            .transpose()
            .map_err(|_| anyhow::anyhow!("Invalid venue order ID"))?;

        let params = BinanceUserTradesParams {
            symbol,
            order_id,
            start_time: start,
            end_time: end,
            from_id: None,
            limit,
            recv_window: None,
        };

        let trades = self.raw.query_user_trades(&params).await?;

        let mut reports = Vec::with_capacity(trades.len());

        for trade in trades {
            match trade.to_fill_report(account_id, instrument_id, price_precision, size_precision) {
                Ok(report) => reports.push(report),
                Err(e) => {
                    log::warn!("Failed to parse fill report: {e}");
                }
            }
        }

        Ok(reports)
    }

    /// Requests recent public trades for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails, instrument is not cached, or parsing fails.
    pub async fn request_trades(
        &self,
        instrument_id: InstrumentId,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<TradeTick>> {
        let symbol = format_binance_symbol(&instrument_id);
        let size_precision = self.get_size_precision(&symbol)?;
        let price_precision = self.get_price_precision(&symbol)?;

        let params = BinanceTradesParams { symbol, limit };

        let trades = self.raw.trades(&params).await?;
        let ts_init = UnixNanos::default();

        let mut result = Vec::with_capacity(trades.len());
        for trade in trades {
            let price: f64 = trade.price.parse().unwrap_or(0.0);
            let size: f64 = trade.qty.parse().unwrap_or(0.0);
            let ts_event = UnixNanos::from((trade.time * 1_000_000) as u64);

            let aggressor_side = if trade.is_buyer_maker {
                AggressorSide::Seller
            } else {
                AggressorSide::Buyer
            };

            let tick = TradeTick::new(
                instrument_id,
                Price::new(price, price_precision),
                Quantity::new(size, size_precision),
                aggressor_side,
                TradeId::new(trade.id.to_string()),
                ts_event,
                ts_init,
            );
            result.push(tick);
        }

        Ok(result)
    }

    /// Requests bar (kline/candlestick) data for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the bar type is not supported, instrument is not cached,
    /// or the request fails.
    pub async fn request_bars(
        &self,
        bar_type: BarType,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<Bar>> {
        anyhow::ensure!(
            bar_type.aggregation_source() == AggregationSource::External,
            "Only EXTERNAL aggregation is supported"
        );

        let spec = bar_type.spec();
        let step = spec.step.get();
        let interval = match spec.aggregation {
            BarAggregation::Second => {
                anyhow::bail!("Binance Futures does not support second-level kline intervals")
            }
            BarAggregation::Minute => format!("{step}m"),
            BarAggregation::Hour => format!("{step}h"),
            BarAggregation::Day => format!("{step}d"),
            BarAggregation::Week => format!("{step}w"),
            BarAggregation::Month => format!("{step}M"),
            a => anyhow::bail!("Binance Futures does not support {a:?} aggregation"),
        };

        let symbol = format_binance_symbol(&bar_type.instrument_id());
        let price_precision = self.get_price_precision(&symbol)?;
        let size_precision = self.get_size_precision(&symbol)?;

        let params = BinanceKlinesParams {
            symbol,
            interval,
            start_time: start.map(|dt| dt.timestamp_millis()),
            end_time: end.map(|dt| dt.timestamp_millis()),
            limit,
        };

        let klines = self.raw.klines(&params).await?;
        let ts_init = UnixNanos::default();

        let mut result = Vec::with_capacity(klines.len());
        for kline in klines {
            let open: f64 = kline.open.parse().unwrap_or(0.0);
            let high: f64 = kline.high.parse().unwrap_or(0.0);
            let low: f64 = kline.low.parse().unwrap_or(0.0);
            let close: f64 = kline.close.parse().unwrap_or(0.0);
            let volume: f64 = kline.volume.parse().unwrap_or(0.0);

            // close_time is end of interval, add 1ms for next bar's open
            let ts_event = UnixNanos::from((kline.close_time * 1_000_000) as u64);

            let bar = Bar::new(
                bar_type,
                Price::new(open, price_precision),
                Price::new(high, price_precision),
                Price::new(low, price_precision),
                Price::new(close, price_precision),
                Quantity::new(volume, size_precision),
                ts_event,
                ts_init,
            );
            result.push(bar);
        }

        Ok(result)
    }
}

/// Checks if an order type requires the Binance Algo Service API.
///
/// As of 2025-12-09, Binance migrated conditional order types to the Algo Service API.
/// The traditional `/fapi/v1/order` endpoint returns error `-4120` for these types.
#[must_use]
pub fn is_algo_order_type(order_type: OrderType) -> bool {
    matches!(
        order_type,
        OrderType::StopMarket
            | OrderType::StopLimit
            | OrderType::MarketIfTouched
            | OrderType::LimitIfTouched
            | OrderType::TrailingStopMarket
    )
}

/// Converts a Nautilus order type to a Binance Futures order type.
fn order_type_to_binance_futures(order_type: OrderType) -> anyhow::Result<BinanceFuturesOrderType> {
    match order_type {
        OrderType::Market => Ok(BinanceFuturesOrderType::Market),
        OrderType::Limit => Ok(BinanceFuturesOrderType::Limit),
        OrderType::StopMarket => Ok(BinanceFuturesOrderType::StopMarket),
        OrderType::StopLimit => Ok(BinanceFuturesOrderType::Stop),
        OrderType::MarketIfTouched => Ok(BinanceFuturesOrderType::TakeProfitMarket),
        OrderType::LimitIfTouched => Ok(BinanceFuturesOrderType::TakeProfit),
        OrderType::TrailingStopMarket => Ok(BinanceFuturesOrderType::TrailingStopMarket),
        _ => anyhow::bail!("Unsupported order type for Binance Futures: {order_type:?}"),
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
            other => panic!("Expected BinanceError, was {other:?}"),
        }
    }
}
