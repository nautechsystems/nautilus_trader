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

//! Binance Spot HTTP client with SBE encoding.
//!
//! This client communicates with Binance Spot REST API using SBE (Simple Binary
//! Encoding) for all request/response payloads, providing microsecond timestamp
//! precision and reduced latency compared to JSON.
//!
//! ## Architecture
//!
//! Two-layer client pattern:
//! - [`BinanceRawSpotHttpClient`]: Low-level API methods returning raw bytes.
//! - [`BinanceSpotHttpClient`]: High-level methods with SBE decoding.
//!
//! ## SBE Headers
//!
//! All requests include:
//! - `Accept: application/sbe`
//! - `X-MBX-SBE: 3:2` (schema ID:version)

use std::{collections::HashMap, fmt::Debug, num::NonZeroU32, sync::Arc};

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use nautilus_core::{
    consts::NAUTILUS_USER_AGENT, nanos::UnixNanos, time::get_atomic_clock_realtime,
};
use nautilus_model::{
    data::{Bar, BarType, TradeTick},
    enums::{AggregationSource, BarAggregation, OrderSide, OrderType, TimeInForce},
    events::AccountState,
    identifiers::{AccountId, ClientOrderId, InstrumentId, VenueOrderId},
    instruments::{Instrument, any::InstrumentAny},
    reports::{FillReport, OrderStatusReport},
    types::{Price, Quantity},
};
use nautilus_network::{
    http::{HttpClient, HttpResponse, Method},
    ratelimiter::quota::Quota,
};
use serde::Serialize;
use ustr::Ustr;

use super::{
    error::{BinanceSpotHttpError, BinanceSpotHttpResult},
    models::{
        AvgPrice, BatchCancelResult, BatchOrderResult, BinanceAccountInfo, BinanceAccountTrade,
        BinanceCancelOrderResponse, BinanceDepth, BinanceKlines, BinanceNewOrderResponse,
        BinanceOrderResponse, BinanceTrades, BookTicker, ListenKeyResponse, Ticker24hr,
        TickerPrice, TradeFee,
    },
    parse,
    query::{
        AccountInfoParams, AccountTradesParams, AllOrdersParams, AvgPriceParams, BatchCancelItem,
        BatchOrderItem, CancelOpenOrdersParams, CancelOrderParams, CancelReplaceOrderParams,
        DepthParams, KlinesParams, ListenKeyParams, NewOrderParams, OpenOrdersParams,
        QueryOrderParams, TickerParams, TradeFeeParams, TradesParams,
    },
};
use crate::{
    common::{
        consts::{BINANCE_SPOT_RATE_LIMITS, BinanceRateLimitQuota},
        credential::Credential,
        enums::{
            BinanceEnvironment, BinanceProductType, BinanceRateLimitInterval, BinanceRateLimitType,
            BinanceSide, BinanceTimeInForce,
        },
        models::BinanceErrorResponse,
        parse::{
            get_currency, parse_fill_report_sbe, parse_klines_to_bars,
            parse_new_order_response_sbe, parse_order_status_report_sbe, parse_spot_instrument_sbe,
            parse_spot_trades_sbe,
        },
        sbe::spot::{
            ReadBuf, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION,
            error_response_codec::{self, ErrorResponseDecoder},
            message_header_codec::MessageHeaderDecoder,
        },
        urls::get_http_base_url,
    },
    spot::enums::{
        BinanceCancelReplaceMode, BinanceOrderResponseType, BinanceSpotOrderType,
        order_type_to_binance_spot,
    },
};

/// SBE schema header value for Spot API.
pub const SBE_SCHEMA_HEADER: &str = "3:2";

/// Binance Spot API path.
const SPOT_API_PATH: &str = "/api/v3";

/// Global rate limit key.
const BINANCE_GLOBAL_RATE_KEY: &str = "binance:spot:global";

/// Orders rate limit key prefix.
const BINANCE_ORDERS_RATE_KEY: &str = "binance:spot:orders";

struct RateLimitConfig {
    default_quota: Option<Quota>,
    keyed_quotas: Vec<(String, Quota)>,
    order_keys: Vec<String>,
}

/// Low-level HTTP client for Binance Spot REST API with SBE encoding.
///
/// Handles:
/// - Base URL resolution by environment.
/// - Optional HMAC SHA256 signing for private endpoints.
/// - Rate limiting using Spot API quotas.
/// - SBE decoding to Binance-specific response types.
///
/// Methods are named to match Binance API endpoints and return
/// venue-specific types (decoded from SBE).
#[derive(Debug, Clone)]
pub struct BinanceRawSpotHttpClient {
    client: HttpClient,
    base_url: String,
    credential: Option<Credential>,
    recv_window: Option<u64>,
    order_rate_keys: Vec<String>,
}

impl BinanceRawSpotHttpClient {
    /// Creates a new Binance Spot raw HTTP client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying [`HttpClient`] fails to build.
    pub fn new(
        environment: BinanceEnvironment,
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url_override: Option<String>,
        recv_window: Option<u64>,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
    ) -> BinanceSpotHttpResult<Self> {
        let RateLimitConfig {
            default_quota,
            keyed_quotas,
            order_keys,
        } = Self::rate_limit_config();

        let credential = match (api_key, api_secret) {
            (Some(key), Some(secret)) => Some(Credential::new(key, secret)),
            (None, None) => None,
            _ => return Err(BinanceSpotHttpError::MissingCredentials),
        };

        let base_url = base_url_override.unwrap_or_else(|| {
            get_http_base_url(BinanceProductType::Spot, environment).to_string()
        });

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
            credential,
            recv_window,
            order_rate_keys: order_keys,
        })
    }

    /// Returns the SBE schema ID.
    #[must_use]
    pub const fn schema_id() -> u16 {
        SBE_SCHEMA_ID
    }

    /// Returns the SBE schema version.
    #[must_use]
    pub const fn schema_version() -> u16 {
        SBE_SCHEMA_VERSION
    }

    /// Performs a GET request and returns raw response bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn get<P>(&self, path: &str, params: Option<&P>) -> BinanceSpotHttpResult<Vec<u8>>
    where
        P: Serialize + ?Sized,
    {
        self.request(Method::GET, path, params, false, false).await
    }

    /// Performs a signed GET request and returns raw response bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing or the request fails.
    pub async fn get_signed<P>(
        &self,
        path: &str,
        params: Option<&P>,
    ) -> BinanceSpotHttpResult<Vec<u8>>
    where
        P: Serialize + ?Sized,
    {
        self.request(Method::GET, path, params, true, false).await
    }

    async fn request<P>(
        &self,
        method: Method,
        path: &str,
        params: Option<&P>,
        signed: bool,
        use_order_quota: bool,
    ) -> BinanceSpotHttpResult<Vec<u8>>
    where
        P: Serialize + ?Sized,
    {
        let mut query = params
            .map(serde_urlencoded::to_string)
            .transpose()
            .map_err(|e| BinanceSpotHttpError::ValidationError(e.to_string()))?
            .unwrap_or_default();

        let mut headers = HashMap::new();
        if signed {
            let cred = self
                .credential
                .as_ref()
                .ok_or(BinanceSpotHttpError::MissingCredentials)?;

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
                None,
                None,
                Some(keys),
            )
            .await?;

        if !response.status.is_success() {
            return self.parse_error_response(response);
        }

        Ok(response.body.to_vec())
    }

    fn build_url(&self, path: &str, query: &str) -> String {
        let normalized_path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };

        let mut url = format!("{}{}{}", self.base_url, SPOT_API_PATH, normalized_path);
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

    fn parse_error_response<T>(&self, response: HttpResponse) -> BinanceSpotHttpResult<T> {
        let status = response.status.as_u16();
        let body = &response.body;

        // Binance may return JSON errors even when SBE was requested
        if let Ok(body_str) = std::str::from_utf8(body)
            && let Ok(err) = serde_json::from_str::<BinanceErrorResponse>(body_str)
        {
            return Err(BinanceSpotHttpError::BinanceError {
                code: err.code,
                message: err.msg,
            });
        }

        // Try to decode SBE error response
        if let Some((code, message)) = Self::try_decode_sbe_error(body) {
            return Err(BinanceSpotHttpError::BinanceError {
                code: code.into(),
                message,
            });
        }

        Err(BinanceSpotHttpError::UnexpectedStatus {
            status,
            body: hex::encode(body),
        })
    }

    /// Attempts to decode an SBE error response.
    ///
    /// Returns Some((code, message)) if successfully decoded, None otherwise.
    fn try_decode_sbe_error(body: &[u8]) -> Option<(i16, String)> {
        const HEADER_LEN: usize = 8;
        if body.len() < HEADER_LEN + error_response_codec::SBE_BLOCK_LENGTH as usize {
            return None;
        }

        let buf = ReadBuf::new(body);

        // Decode message header
        let header = MessageHeaderDecoder::default().wrap(buf, 0);
        if header.template_id() != error_response_codec::SBE_TEMPLATE_ID {
            return None;
        }

        // Decode error response
        let mut decoder = ErrorResponseDecoder::default().header(header, 0);
        let code = decoder.code();

        // Decode the message string (VAR_DATA with 2-byte length prefix)
        let msg_coords = decoder.msg_decoder();
        let msg_bytes = decoder.msg_slice(msg_coords);
        let message = String::from_utf8_lossy(msg_bytes).into_owned();

        Some((code, message))
    }

    fn default_headers(credential: &Option<Credential>) -> HashMap<String, String> {
        let mut headers = HashMap::new();
        headers.insert("User-Agent".to_string(), NAUTILUS_USER_AGENT.to_string());
        headers.insert("Accept".to_string(), "application/sbe".to_string());
        headers.insert("X-MBX-SBE".to_string(), SBE_SCHEMA_HEADER.to_string());
        if let Some(cred) = credential {
            headers.insert("X-MBX-APIKEY".to_string(), cred.api_key().to_string());
        }
        headers
    }

    fn rate_limit_config() -> RateLimitConfig {
        let quotas = BINANCE_SPOT_RATE_LIMITS;
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
                Quota::with_period(std::time::Duration::from_secs(86_400))
                    .map(|q| q.allow_burst(burst))
            }
        }
    }

    /// Tests connectivity to the API.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    pub async fn ping(&self) -> BinanceSpotHttpResult<()> {
        let bytes = self.get("ping", None::<&()>).await?;
        parse::decode_ping(&bytes)?;
        Ok(())
    }

    /// Returns the server time in **microseconds** since epoch.
    ///
    /// Note: SBE provides microsecond precision vs JSON's milliseconds.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    pub async fn server_time(&self) -> BinanceSpotHttpResult<i64> {
        let bytes = self.get("time", None::<&()>).await?;
        let timestamp = parse::decode_server_time(&bytes)?;
        Ok(timestamp)
    }

    /// Returns exchange information including trading symbols.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    pub async fn exchange_info(
        &self,
    ) -> BinanceSpotHttpResult<super::models::BinanceExchangeInfoSbe> {
        let bytes = self.get("exchangeInfo", None::<&()>).await?;
        let info = parse::decode_exchange_info(&bytes)?;
        Ok(info)
    }

    /// Returns order book depth for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    pub async fn depth(&self, params: &DepthParams) -> BinanceSpotHttpResult<BinanceDepth> {
        let bytes = self.get("depth", Some(params)).await?;
        let depth = parse::decode_depth(&bytes)?;
        Ok(depth)
    }

    /// Returns recent trades for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    pub async fn trades(
        &self,
        symbol: &str,
        limit: Option<u32>,
    ) -> BinanceSpotHttpResult<BinanceTrades> {
        let params = TradesParams {
            symbol: symbol.to_string(),
            limit,
        };
        let bytes = self.get("trades", Some(&params)).await?;
        let trades = parse::decode_trades(&bytes)?;
        Ok(trades)
    }

    /// Returns kline (candlestick) data for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    pub async fn klines(
        &self,
        symbol: &str,
        interval: &str,
        start_time: Option<i64>,
        end_time: Option<i64>,
        limit: Option<u32>,
    ) -> BinanceSpotHttpResult<BinanceKlines> {
        let params = KlinesParams {
            symbol: symbol.to_string(),
            interval: interval.to_string(),
            start_time,
            end_time,
            time_zone: None,
            limit,
        };
        let bytes = self.get("klines", Some(&params)).await?;
        let klines = parse::decode_klines(&bytes)?;
        Ok(klines)
    }

    /// Performs a public GET request that returns JSON.
    async fn get_json<P>(&self, path: &str, params: Option<&P>) -> BinanceSpotHttpResult<Vec<u8>>
    where
        P: Serialize + ?Sized,
    {
        let query = params
            .map(serde_urlencoded::to_string)
            .transpose()
            .map_err(|e| BinanceSpotHttpError::ValidationError(e.to_string()))?
            .unwrap_or_default();

        let url = self.build_url(path, &query);
        let keys = vec![BINANCE_GLOBAL_RATE_KEY.to_string()];

        let response = self
            .client
            .request(
                Method::GET,
                url,
                None::<&HashMap<String, Vec<String>>>,
                None,
                None,
                None,
                Some(keys),
            )
            .await?;

        if !response.status.is_success() {
            return self.parse_error_response(response);
        }

        Ok(response.body.to_vec())
    }

    /// Returns 24-hour ticker price change statistics.
    ///
    /// If `symbol` is None, returns statistics for all symbols.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn ticker_24hr(
        &self,
        symbol: Option<&str>,
    ) -> BinanceSpotHttpResult<Vec<Ticker24hr>> {
        let params = symbol.map(TickerParams::for_symbol);
        let bytes = self.get_json("ticker/24hr", params.as_ref()).await?;

        // Single symbol returns object, multiple returns array
        if symbol.is_some() {
            let ticker: Ticker24hr = serde_json::from_slice(&bytes)
                .map_err(|e| BinanceSpotHttpError::JsonError(e.to_string()))?;
            Ok(vec![ticker])
        } else {
            let tickers: Vec<Ticker24hr> = serde_json::from_slice(&bytes)
                .map_err(|e| BinanceSpotHttpError::JsonError(e.to_string()))?;
            Ok(tickers)
        }
    }

    /// Returns latest price for a symbol or all symbols.
    ///
    /// If `symbol` is None, returns prices for all symbols.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn ticker_price(
        &self,
        symbol: Option<&str>,
    ) -> BinanceSpotHttpResult<Vec<TickerPrice>> {
        let params = symbol.map(TickerParams::for_symbol);
        let bytes = self.get_json("ticker/price", params.as_ref()).await?;

        // Single symbol returns object, multiple returns array
        if symbol.is_some() {
            let ticker: TickerPrice = serde_json::from_slice(&bytes)
                .map_err(|e| BinanceSpotHttpError::JsonError(e.to_string()))?;
            Ok(vec![ticker])
        } else {
            let tickers: Vec<TickerPrice> = serde_json::from_slice(&bytes)
                .map_err(|e| BinanceSpotHttpError::JsonError(e.to_string()))?;
            Ok(tickers)
        }
    }

    /// Returns best bid/ask price for a symbol or all symbols.
    ///
    /// If `symbol` is None, returns book ticker for all symbols.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn ticker_book(
        &self,
        symbol: Option<&str>,
    ) -> BinanceSpotHttpResult<Vec<BookTicker>> {
        let params = symbol.map(TickerParams::for_symbol);
        let bytes = self.get_json("ticker/bookTicker", params.as_ref()).await?;

        // Single symbol returns object, multiple returns array
        if symbol.is_some() {
            let ticker: BookTicker = serde_json::from_slice(&bytes)
                .map_err(|e| BinanceSpotHttpError::JsonError(e.to_string()))?;
            Ok(vec![ticker])
        } else {
            let tickers: Vec<BookTicker> = serde_json::from_slice(&bytes)
                .map_err(|e| BinanceSpotHttpError::JsonError(e.to_string()))?;
            Ok(tickers)
        }
    }

    /// Returns current average price for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn avg_price(&self, symbol: &str) -> BinanceSpotHttpResult<AvgPrice> {
        let params = AvgPriceParams::new(symbol);
        let bytes = self.get_json("avgPrice", Some(&params)).await?;

        let avg_price: AvgPrice = serde_json::from_slice(&bytes)
            .map_err(|e| BinanceSpotHttpError::JsonError(e.to_string()))?;
        Ok(avg_price)
    }

    /// Returns trading fee rates for symbols.
    ///
    /// If `symbol` is None, returns fee rates for all symbols.
    /// Uses SAPI endpoint (requires authentication).
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing or the request fails.
    pub async fn get_trade_fee(
        &self,
        symbol: Option<&str>,
    ) -> BinanceSpotHttpResult<Vec<TradeFee>> {
        let params = symbol.map(TradeFeeParams::for_symbol);
        let bytes = self
            .get_signed_sapi("asset/tradeFee", params.as_ref())
            .await?;

        let fees: Vec<TradeFee> = serde_json::from_slice(&bytes)
            .map_err(|e| BinanceSpotHttpError::JsonError(e.to_string()))?;
        Ok(fees)
    }

    /// Performs a signed GET request to SAPI endpoints (returns JSON).
    async fn get_signed_sapi<P>(
        &self,
        path: &str,
        params: Option<&P>,
    ) -> BinanceSpotHttpResult<Vec<u8>>
    where
        P: Serialize + ?Sized,
    {
        let cred = self
            .credential
            .as_ref()
            .ok_or(BinanceSpotHttpError::MissingCredentials)?;

        let mut query = params
            .map(serde_urlencoded::to_string)
            .transpose()
            .map_err(|e| BinanceSpotHttpError::ValidationError(e.to_string()))?
            .unwrap_or_default();

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

        // Build SAPI URL (different from regular API path)
        let normalized_path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };

        let mut url = format!("{}/sapi/v1{}", self.base_url, normalized_path);
        if !query.is_empty() {
            url.push('?');
            url.push_str(&query);
        }

        let mut headers = HashMap::new();
        headers.insert("X-MBX-APIKEY".to_string(), cred.api_key().to_string());

        let keys = vec![BINANCE_GLOBAL_RATE_KEY.to_string()];

        let response = self
            .client
            .request(
                Method::GET,
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

        Ok(response.body.to_vec())
    }

    /// Percent-encodes a string for use in URL query parameters.
    fn percent_encode(input: &str) -> String {
        let mut result = String::with_capacity(input.len() * 3);
        for byte in input.bytes() {
            match byte {
                // Unreserved characters (RFC 3986)
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

    /// Submits multiple orders in a single request (up to 5 orders).
    ///
    /// Each order in the batch is processed independently. The response contains
    /// the result for each order, which can be either a success or an error.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or
    /// JSON parsing fails. Individual order failures are returned in the
    /// response array as `BatchOrderResult::Error`.
    pub async fn batch_submit_orders(
        &self,
        orders: &[BatchOrderItem],
    ) -> BinanceSpotHttpResult<Vec<BatchOrderResult>> {
        if orders.is_empty() {
            return Ok(Vec::new());
        }

        if orders.len() > 5 {
            return Err(BinanceSpotHttpError::ValidationError(
                "Batch order limit is 5 orders maximum".to_string(),
            ));
        }

        let batch_json = serde_json::to_string(orders)
            .map_err(|e| BinanceSpotHttpError::ValidationError(e.to_string()))?;

        let bytes = self
            .batch_request(Method::POST, "batchOrders", &batch_json)
            .await?;

        let results: Vec<BatchOrderResult> = serde_json::from_slice(&bytes)
            .map_err(|e| BinanceSpotHttpError::JsonError(e.to_string()))?;

        Ok(results)
    }

    /// Cancels multiple orders in a single request (up to 5 orders).
    ///
    /// Each cancel in the batch is processed independently. The response contains
    /// the result for each cancel, which can be either a success or an error.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or
    /// JSON parsing fails. Individual cancel failures are returned in the
    /// response array as `BatchCancelResult::Error`.
    pub async fn batch_cancel_orders(
        &self,
        cancels: &[BatchCancelItem],
    ) -> BinanceSpotHttpResult<Vec<BatchCancelResult>> {
        if cancels.is_empty() {
            return Ok(Vec::new());
        }

        if cancels.len() > 5 {
            return Err(BinanceSpotHttpError::ValidationError(
                "Batch cancel limit is 5 orders maximum".to_string(),
            ));
        }

        let batch_json = serde_json::to_string(cancels)
            .map_err(|e| BinanceSpotHttpError::ValidationError(e.to_string()))?;

        let bytes = self
            .batch_request(Method::DELETE, "batchOrders", &batch_json)
            .await?;

        let results: Vec<BatchCancelResult> = serde_json::from_slice(&bytes)
            .map_err(|e| BinanceSpotHttpError::JsonError(e.to_string()))?;

        Ok(results)
    }

    /// Performs a signed batch request with the batchOrders parameter.
    async fn batch_request(
        &self,
        method: Method,
        path: &str,
        batch_json: &str,
    ) -> BinanceSpotHttpResult<Vec<u8>> {
        let cred = self
            .credential
            .as_ref()
            .ok_or(BinanceSpotHttpError::MissingCredentials)?;

        let encoded_batch = Self::percent_encode(batch_json);
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

        let keys = self.rate_limit_keys(true);

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

        Ok(response.body.to_vec())
    }

    /// Returns account information including balances.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    pub async fn account(
        &self,
        params: &AccountInfoParams,
    ) -> BinanceSpotHttpResult<BinanceAccountInfo> {
        let bytes = self.get_signed("account", Some(params)).await?;
        let response = parse::decode_account(&bytes)?;
        Ok(response)
    }

    /// Returns account trade history for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    pub async fn account_trades(
        &self,
        symbol: &str,
        order_id: Option<i64>,
        start_time: Option<i64>,
        end_time: Option<i64>,
        limit: Option<u32>,
    ) -> BinanceSpotHttpResult<Vec<BinanceAccountTrade>> {
        let params = AccountTradesParams {
            symbol: symbol.to_string(),
            order_id,
            start_time,
            end_time,
            from_id: None,
            limit,
        };
        let bytes = self.get_signed("myTrades", Some(&params)).await?;
        let response = parse::decode_account_trades(&bytes)?;
        Ok(response)
    }

    /// Queries an order's status.
    ///
    /// Either `order_id` or `client_order_id` must be provided.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    pub async fn query_order(
        &self,
        symbol: &str,
        order_id: Option<i64>,
        client_order_id: Option<&str>,
    ) -> BinanceSpotHttpResult<BinanceOrderResponse> {
        let params = QueryOrderParams {
            symbol: symbol.to_string(),
            order_id,
            orig_client_order_id: client_order_id.map(|s| s.to_string()),
        };
        let bytes = self.get_signed("order", Some(&params)).await?;
        let response = parse::decode_order(&bytes)?;
        Ok(response)
    }

    /// Returns all open orders for a symbol or all symbols.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    pub async fn open_orders(
        &self,
        symbol: Option<&str>,
    ) -> BinanceSpotHttpResult<Vec<BinanceOrderResponse>> {
        let params = OpenOrdersParams {
            symbol: symbol.map(|s| s.to_string()),
        };
        let bytes = self.get_signed("openOrders", Some(&params)).await?;
        let response = parse::decode_orders(&bytes)?;
        Ok(response)
    }

    /// Returns all orders (including closed) for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    pub async fn all_orders(
        &self,
        symbol: &str,
        start_time: Option<i64>,
        end_time: Option<i64>,
        limit: Option<u32>,
    ) -> BinanceSpotHttpResult<Vec<BinanceOrderResponse>> {
        let params = AllOrdersParams {
            symbol: symbol.to_string(),
            order_id: None,
            start_time,
            end_time,
            limit,
        };
        let bytes = self.get_signed("allOrders", Some(&params)).await?;
        let response = parse::decode_orders(&bytes)?;
        Ok(response)
    }

    /// Performs a signed POST request for order operations.
    async fn post_order<P>(&self, path: &str, params: Option<&P>) -> BinanceSpotHttpResult<Vec<u8>>
    where
        P: Serialize + ?Sized,
    {
        self.request(Method::POST, path, params, true, true).await
    }

    /// Performs a signed DELETE request for cancel operations.
    async fn delete_order<P>(
        &self,
        path: &str,
        params: Option<&P>,
    ) -> BinanceSpotHttpResult<Vec<u8>>
    where
        P: Serialize + ?Sized,
    {
        self.request(Method::DELETE, path, params, true, true).await
    }

    /// Creates a new order.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    #[allow(clippy::too_many_arguments)]
    pub async fn new_order(
        &self,
        symbol: &str,
        side: BinanceSide,
        order_type: BinanceSpotOrderType,
        time_in_force: Option<BinanceTimeInForce>,
        quantity: Option<&str>,
        price: Option<&str>,
        client_order_id: Option<&str>,
        stop_price: Option<&str>,
    ) -> BinanceSpotHttpResult<BinanceNewOrderResponse> {
        let params = NewOrderParams {
            symbol: symbol.to_string(),
            side,
            order_type,
            time_in_force,
            quantity: quantity.map(|s| s.to_string()),
            quote_order_qty: None,
            price: price.map(|s| s.to_string()),
            new_client_order_id: client_order_id.map(|s| s.to_string()),
            stop_price: stop_price.map(|s| s.to_string()),
            trailing_delta: None,
            iceberg_qty: None,
            new_order_resp_type: Some(BinanceOrderResponseType::Full),
            self_trade_prevention_mode: None,
            strategy_id: None,
            strategy_type: None,
        };
        let bytes = self.post_order("order", Some(&params)).await?;
        let response = parse::decode_new_order_full(&bytes)?;
        Ok(response)
    }

    /// Cancels an existing order and places a new order atomically.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    #[allow(clippy::too_many_arguments)]
    pub async fn cancel_replace_order(
        &self,
        symbol: &str,
        side: BinanceSide,
        order_type: BinanceSpotOrderType,
        time_in_force: Option<BinanceTimeInForce>,
        quantity: Option<&str>,
        price: Option<&str>,
        cancel_order_id: Option<i64>,
        cancel_client_order_id: Option<&str>,
        new_client_order_id: Option<&str>,
    ) -> BinanceSpotHttpResult<BinanceNewOrderResponse> {
        let params = CancelReplaceOrderParams {
            symbol: symbol.to_string(),
            side,
            order_type,
            cancel_replace_mode: BinanceCancelReplaceMode::StopOnFailure,
            time_in_force,
            quantity: quantity.map(|s| s.to_string()),
            quote_order_qty: None,
            price: price.map(|s| s.to_string()),
            cancel_order_id,
            cancel_orig_client_order_id: cancel_client_order_id.map(|s| s.to_string()),
            new_client_order_id: new_client_order_id.map(|s| s.to_string()),
            stop_price: None,
            trailing_delta: None,
            iceberg_qty: None,
            new_order_resp_type: Some(BinanceOrderResponseType::Full),
            self_trade_prevention_mode: None,
        };
        let bytes = self
            .post_order("order/cancelReplace", Some(&params))
            .await?;
        let response = parse::decode_new_order_full(&bytes)?;
        Ok(response)
    }

    /// Cancels an existing order.
    ///
    /// Either `order_id` or `client_order_id` must be provided.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    pub async fn cancel_order(
        &self,
        symbol: &str,
        order_id: Option<i64>,
        client_order_id: Option<&str>,
    ) -> BinanceSpotHttpResult<BinanceCancelOrderResponse> {
        let params = match (order_id, client_order_id) {
            (Some(id), _) => CancelOrderParams::by_order_id(symbol, id),
            (None, Some(id)) => CancelOrderParams::by_client_order_id(symbol, id.to_string()),
            (None, None) => {
                return Err(BinanceSpotHttpError::ValidationError(
                    "Either order_id or client_order_id must be provided".to_string(),
                ));
            }
        };
        let bytes = self.delete_order("order", Some(&params)).await?;
        let response = parse::decode_cancel_order(&bytes)?;
        Ok(response)
    }

    /// Cancels all open orders for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    pub async fn cancel_open_orders(
        &self,
        symbol: &str,
    ) -> BinanceSpotHttpResult<Vec<BinanceCancelOrderResponse>> {
        let params = CancelOpenOrdersParams::new(symbol.to_string());
        let bytes = self.delete_order("openOrders", Some(&params)).await?;
        let response = parse::decode_cancel_open_orders(&bytes)?;
        Ok(response)
    }

    /// Performs an API-key authenticated request (no signature) that returns JSON.
    async fn request_with_api_key<P>(
        &self,
        method: Method,
        path: &str,
        params: Option<&P>,
    ) -> BinanceSpotHttpResult<Vec<u8>>
    where
        P: Serialize + ?Sized,
    {
        let cred = self
            .credential
            .as_ref()
            .ok_or(BinanceSpotHttpError::MissingCredentials)?;

        let query = params
            .map(serde_urlencoded::to_string)
            .transpose()
            .map_err(|e| BinanceSpotHttpError::ValidationError(e.to_string()))?
            .unwrap_or_default();

        let url = self.build_url(path, &query);

        let mut headers = HashMap::new();
        headers.insert("X-MBX-APIKEY".to_string(), cred.api_key().to_string());

        let keys = vec![BINANCE_GLOBAL_RATE_KEY.to_string()];

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

        Ok(response.body.to_vec())
    }

    /// Creates a new listen key for the user data stream.
    ///
    /// Listen keys are valid for 60 minutes. Use `extend_listen_key` to keep
    /// the stream alive.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing or the request fails.
    pub async fn create_listen_key(&self) -> BinanceSpotHttpResult<ListenKeyResponse> {
        let bytes = self
            .request_with_api_key(Method::POST, "userDataStream", None::<&()>)
            .await?;

        let response: ListenKeyResponse = serde_json::from_slice(&bytes)
            .map_err(|e| BinanceSpotHttpError::JsonError(e.to_string()))?;

        Ok(response)
    }

    /// Extends the validity of a listen key by 60 minutes.
    ///
    /// Should be called periodically to keep the user data stream alive.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing or the request fails.
    pub async fn extend_listen_key(&self, listen_key: &str) -> BinanceSpotHttpResult<()> {
        let params = ListenKeyParams::new(listen_key);
        self.request_with_api_key(Method::PUT, "userDataStream", Some(&params))
            .await?;
        Ok(())
    }

    /// Closes a listen key, terminating the user data stream.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing or the request fails.
    pub async fn close_listen_key(&self, listen_key: &str) -> BinanceSpotHttpResult<()> {
        let params = ListenKeyParams::new(listen_key);
        self.request_with_api_key(Method::DELETE, "userDataStream", Some(&params))
            .await?;
        Ok(())
    }
}

/// High-level HTTP client for Binance Spot API.
///
/// Wraps [`BinanceRawSpotHttpClient`] and provides domain-level methods:
/// - Simple types (ping, server_time): Pass through from raw client.
/// - Complex types (instruments, orders): Transform to Nautilus domain types.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.binance")
)]
pub struct BinanceSpotHttpClient {
    inner: Arc<BinanceRawSpotHttpClient>,
    instruments_cache: Arc<DashMap<Ustr, InstrumentAny>>,
}

impl Clone for BinanceSpotHttpClient {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            instruments_cache: self.instruments_cache.clone(),
        }
    }
}

impl Debug for BinanceSpotHttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BinanceSpotHttpClient))
            .field("inner", &self.inner)
            .field("instruments_cached", &self.instruments_cache.len())
            .finish()
    }
}

impl BinanceSpotHttpClient {
    /// Creates a new Binance Spot HTTP client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be created.
    pub fn new(
        environment: BinanceEnvironment,
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url_override: Option<String>,
        recv_window: Option<u64>,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
    ) -> BinanceSpotHttpResult<Self> {
        let inner = BinanceRawSpotHttpClient::new(
            environment,
            api_key,
            api_secret,
            base_url_override,
            recv_window,
            timeout_secs,
            proxy_url,
        )?;

        Ok(Self {
            inner: Arc::new(inner),
            instruments_cache: Arc::new(DashMap::new()),
        })
    }

    /// Returns a reference to the inner raw client.
    #[must_use]
    pub fn inner(&self) -> &BinanceRawSpotHttpClient {
        &self.inner
    }

    /// Returns the SBE schema ID.
    #[must_use]
    pub const fn schema_id() -> u16 {
        SBE_SCHEMA_ID
    }

    /// Returns the SBE schema version.
    #[must_use]
    pub const fn schema_version() -> u16 {
        SBE_SCHEMA_VERSION
    }

    /// Generates a timestamp for initialization.
    fn generate_ts_init(&self) -> UnixNanos {
        UnixNanos::from(chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0) as u64)
    }

    /// Retrieves an instrument from the cache.
    fn instrument_from_cache(&self, symbol: Ustr) -> anyhow::Result<InstrumentAny> {
        self.instruments_cache
            .get(&symbol)
            .map(|entry| entry.value().clone())
            .ok_or_else(|| anyhow::anyhow!("Instrument {symbol} not in cache"))
    }

    /// Caches multiple instruments.
    pub fn cache_instruments(&self, instruments: Vec<InstrumentAny>) {
        for inst in instruments {
            self.instruments_cache
                .insert(inst.raw_symbol().inner(), inst);
        }
    }

    /// Caches a single instrument.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        self.instruments_cache
            .insert(instrument.raw_symbol().inner(), instrument);
    }

    /// Gets an instrument from the cache by symbol.
    #[must_use]
    pub fn get_instrument(&self, symbol: &Ustr) -> Option<InstrumentAny> {
        self.instruments_cache
            .get(symbol)
            .map(|entry| entry.value().clone())
    }

    /// Tests connectivity to the API.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    pub async fn ping(&self) -> BinanceSpotHttpResult<()> {
        self.inner.ping().await
    }

    /// Returns the server time in **microseconds** since epoch.
    ///
    /// Note: SBE provides microsecond precision vs JSON's milliseconds.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    pub async fn server_time(&self) -> BinanceSpotHttpResult<i64> {
        self.inner.server_time().await
    }

    /// Returns exchange information including trading symbols.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    pub async fn exchange_info(
        &self,
    ) -> BinanceSpotHttpResult<super::models::BinanceExchangeInfoSbe> {
        self.inner.exchange_info().await
    }

    /// Requests Nautilus instruments for all trading symbols.
    ///
    /// Fetches exchange info via SBE and parses each symbol into a CurrencyPair.
    /// Non-trading symbols are skipped with a debug log.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    pub async fn request_instruments(&self) -> BinanceSpotHttpResult<Vec<InstrumentAny>> {
        let info = self.exchange_info().await?;
        let ts_init = self.generate_ts_init();

        let mut instruments = Vec::with_capacity(info.symbols.len());
        for symbol in &info.symbols {
            match parse_spot_instrument_sbe(symbol, ts_init, ts_init) {
                Ok(instrument) => instruments.push(instrument),
                Err(e) => {
                    log::debug!(
                        "Skipping symbol during instrument parsing: symbol={}, error={e}",
                        symbol.symbol
                    );
                }
            }
        }

        // Cache instruments for use by other domain methods
        self.cache_instruments(instruments.clone());

        log::info!("Loaded spot instruments: count={}", instruments.len());
        Ok(instruments)
    }

    /// Requests recent trades for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails, the instrument is not cached,
    /// or trade parsing fails.
    pub async fn request_trades(
        &self,
        instrument_id: InstrumentId,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<TradeTick>> {
        let symbol = instrument_id.symbol.inner();
        let instrument = self.instrument_from_cache(symbol)?;
        let ts_init = self.generate_ts_init();

        let trades = self
            .inner
            .trades(symbol.as_str(), limit)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        parse_spot_trades_sbe(&trades, &instrument, ts_init)
    }

    /// Requests bar (kline/candlestick) data.
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
                anyhow::bail!("Binance Spot does not support second-level kline intervals")
            }
            BarAggregation::Minute => format!("{step}m"),
            BarAggregation::Hour => format!("{step}h"),
            BarAggregation::Day => format!("{step}d"),
            BarAggregation::Week => format!("{step}w"),
            BarAggregation::Month => format!("{step}M"),
            a => anyhow::bail!("Binance does not support {a:?} aggregation"),
        };

        let symbol = bar_type.instrument_id().symbol;
        let instrument = self.instrument_from_cache(symbol.inner())?;
        let ts_init = self.generate_ts_init();

        let klines = self
            .inner
            .klines(
                symbol.as_str(),
                &interval,
                start.map(|dt| dt.timestamp_millis()),
                end.map(|dt| dt.timestamp_millis()),
                limit,
            )
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        parse_klines_to_bars(&klines, bar_type, &instrument, ts_init)
    }

    /// Requests the account state with Nautilus types.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    pub async fn request_account_state(
        &self,
        account_id: AccountId,
    ) -> anyhow::Result<AccountState> {
        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let params = AccountInfoParams::default();
        let account_info = self.inner.account(&params).await?;
        Ok(account_info.to_account_state(account_id, ts_init))
    }

    /// Requests the status of a specific order.
    ///
    /// Either `venue_order_id` or `client_order_id` must be provided.
    ///
    /// # Errors
    ///
    /// Returns an error if neither identifier is provided, the request fails,
    /// instrument is not cached, or parsing fails.
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

        let symbol = instrument_id.symbol.inner();
        let instrument = self.instrument_from_cache(symbol)?;
        let ts_init = self.generate_ts_init();

        let order_id = venue_order_id
            .map(|id| id.inner().parse::<i64>())
            .transpose()
            .map_err(|_| anyhow::anyhow!("Invalid venue order ID"))?;

        let client_id_str = client_order_id.map(|id| id.to_string());

        let order = self
            .inner
            .query_order(symbol.as_str(), order_id, client_id_str.as_deref())
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        parse_order_status_report_sbe(&order, account_id, &instrument, ts_init)
    }

    /// Requests order status reports.
    ///
    /// When `open_only` is true, returns only open orders (instrument_id optional).
    /// When `open_only` is false, returns order history (instrument_id required).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails, any order's instrument is not cached,
    /// or parsing fails.
    #[allow(clippy::too_many_arguments)]
    pub async fn request_order_status_reports(
        &self,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        open_only: bool,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let ts_init = self.generate_ts_init();
        let symbol = instrument_id.map(|id| id.symbol.to_string());

        let orders = if open_only {
            self.inner
                .open_orders(symbol.as_deref())
                .await
                .map_err(|e| anyhow::anyhow!(e))?
        } else {
            let symbol = symbol
                .ok_or_else(|| anyhow::anyhow!("instrument_id is required when open_only=false"))?;
            self.inner
                .all_orders(
                    &symbol,
                    start.map(|dt| dt.timestamp_millis()),
                    end.map(|dt| dt.timestamp_millis()),
                    limit,
                )
                .await
                .map_err(|e| anyhow::anyhow!(e))?
        };

        orders
            .iter()
            .map(|order| {
                let symbol = Ustr::from(&order.symbol);
                let instrument = self.instrument_from_cache(symbol)?;
                parse_order_status_report_sbe(order, account_id, &instrument, ts_init)
            })
            .collect()
    }

    /// Requests fill reports (trade history) for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails, any trade's instrument is not cached,
    /// or parsing fails.
    #[allow(clippy::too_many_arguments)]
    pub async fn request_fill_reports(
        &self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        venue_order_id: Option<VenueOrderId>,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<FillReport>> {
        let ts_init = self.generate_ts_init();
        let symbol = instrument_id.symbol.inner();

        let order_id = venue_order_id
            .map(|id| id.inner().parse::<i64>())
            .transpose()
            .map_err(|_| anyhow::anyhow!("Invalid venue order ID"))?;

        let trades = self
            .inner
            .account_trades(
                symbol.as_str(),
                order_id,
                start.map(|dt| dt.timestamp_millis()),
                end.map(|dt| dt.timestamp_millis()),
                limit,
            )
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        trades
            .iter()
            .map(|trade| {
                let symbol = Ustr::from(&trade.symbol);
                let instrument = self.instrument_from_cache(symbol)?;
                let commission_currency = get_currency(&trade.commission_asset);
                parse_fill_report_sbe(trade, account_id, &instrument, commission_currency, ts_init)
            })
            .collect()
    }

    /// Submits a new order to the venue.
    ///
    /// Converts Nautilus domain types to Binance-specific parameters
    /// and returns an `OrderStatusReport`.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The instrument is not cached.
    /// - The order type or time-in-force is unsupported.
    /// - Stop orders are submitted without a trigger price.
    /// - The request fails or SBE decoding fails.
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
        post_only: bool,
    ) -> anyhow::Result<OrderStatusReport> {
        let symbol = instrument_id.symbol.inner();
        let instrument = self.instrument_from_cache(symbol)?;
        let ts_init = self.generate_ts_init();

        let binance_side = BinanceSide::try_from(order_side)?;
        let binance_order_type = order_type_to_binance_spot(order_type, post_only)?;

        // Validate trigger price for stop orders
        let is_stop_order = matches!(order_type, OrderType::StopMarket | OrderType::StopLimit);
        if is_stop_order && trigger_price.is_none() {
            anyhow::bail!("Stop orders require a trigger price");
        }

        // Validate price for order types that require it
        let requires_price = matches!(
            binance_order_type,
            BinanceSpotOrderType::Limit
                | BinanceSpotOrderType::StopLossLimit
                | BinanceSpotOrderType::TakeProfitLimit
                | BinanceSpotOrderType::LimitMaker
        );
        if requires_price && price.is_none() {
            anyhow::bail!("{binance_order_type:?} orders require a price");
        }

        // Only send TIF for order types that support it
        let supports_tif = matches!(
            binance_order_type,
            BinanceSpotOrderType::Limit
                | BinanceSpotOrderType::StopLossLimit
                | BinanceSpotOrderType::TakeProfitLimit
        );
        let binance_tif = if supports_tif {
            Some(BinanceTimeInForce::try_from(time_in_force)?)
        } else {
            None
        };

        let qty_str = quantity.to_string();
        let price_str = price.map(|p| p.to_string());
        let stop_price_str = trigger_price.map(|p| p.to_string());
        let client_id_str = client_order_id.to_string();

        let response = self
            .inner
            .new_order(
                symbol.as_str(),
                binance_side,
                binance_order_type,
                binance_tif,
                Some(&qty_str),
                price_str.as_deref(),
                Some(&client_id_str),
                stop_price_str.as_deref(),
            )
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        parse_new_order_response_sbe(&response, account_id, &instrument, ts_init)
    }

    /// Submits multiple orders in a single batch request.
    ///
    /// Binance limits batch submit to 5 orders maximum.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or JSON parsing fails.
    pub async fn submit_order_list(
        &self,
        orders: &[BatchOrderItem],
    ) -> BinanceSpotHttpResult<Vec<BatchOrderResult>> {
        self.inner.batch_submit_orders(orders).await
    }

    /// Modifies an existing order (cancel and replace atomically).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The instrument is not cached.
    /// - The order type or time-in-force is unsupported.
    /// - The request fails or SBE decoding fails.
    #[allow(clippy::too_many_arguments)]
    pub async fn modify_order(
        &self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        venue_order_id: VenueOrderId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: TimeInForce,
        price: Option<Price>,
    ) -> anyhow::Result<OrderStatusReport> {
        let symbol = instrument_id.symbol.inner();
        let instrument = self.instrument_from_cache(symbol)?;
        let ts_init = self.generate_ts_init();

        let binance_side = BinanceSide::try_from(order_side)?;
        let binance_order_type = order_type_to_binance_spot(order_type, false)?;
        let binance_tif = BinanceTimeInForce::try_from(time_in_force)?;

        let cancel_order_id: i64 = venue_order_id
            .inner()
            .parse()
            .map_err(|_| anyhow::anyhow!("Invalid venue order ID: {venue_order_id}"))?;

        let qty_str = quantity.to_string();
        let price_str = price.map(|p| p.to_string());
        let client_id_str = client_order_id.to_string();

        let response = self
            .inner
            .cancel_replace_order(
                symbol.as_str(),
                binance_side,
                binance_order_type,
                Some(binance_tif),
                Some(&qty_str),
                price_str.as_deref(),
                Some(cancel_order_id),
                None,
                Some(&client_id_str),
            )
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        parse_new_order_response_sbe(&response, account_id, &instrument, ts_init)
    }

    /// Cancels an existing order on the venue.
    ///
    /// Either `venue_order_id` or `client_order_id` must be provided.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    pub async fn cancel_order(
        &self,
        instrument_id: InstrumentId,
        venue_order_id: Option<VenueOrderId>,
        client_order_id: Option<ClientOrderId>,
    ) -> anyhow::Result<VenueOrderId> {
        let symbol = instrument_id.symbol.inner();

        let order_id = venue_order_id
            .map(|id| id.inner().parse::<i64>())
            .transpose()
            .map_err(|_| anyhow::anyhow!("Invalid venue order ID"))?;

        let client_id_str = client_order_id.map(|id| id.to_string());

        let response = self
            .inner
            .cancel_order(symbol.as_str(), order_id, client_id_str.as_deref())
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        Ok(VenueOrderId::new(response.order_id.to_string()))
    }

    /// Cancels multiple orders in a single batch request.
    ///
    /// Binance limits batch cancel to 5 orders maximum.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or JSON parsing fails.
    pub async fn batch_cancel_orders(
        &self,
        cancels: &[BatchCancelItem],
    ) -> BinanceSpotHttpResult<Vec<BatchCancelResult>> {
        self.inner.batch_cancel_orders(cancels).await
    }

    /// Cancels all open orders for a symbol.
    ///
    /// Returns the venue order IDs of all canceled orders.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    pub async fn cancel_all_orders(
        &self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<Vec<(VenueOrderId, ClientOrderId)>> {
        let symbol = instrument_id.symbol.inner();

        let responses = self
            .inner
            .cancel_open_orders(symbol.as_str())
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        Ok(responses
            .into_iter()
            .map(|r| {
                (
                    VenueOrderId::new(r.order_id.to_string()),
                    ClientOrderId::new(&r.orig_client_order_id),
                )
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_schema_constants() {
        assert_eq!(BinanceRawSpotHttpClient::schema_id(), 3);
        assert_eq!(BinanceRawSpotHttpClient::schema_version(), 2);
        assert_eq!(BinanceSpotHttpClient::schema_id(), 3);
        assert_eq!(BinanceSpotHttpClient::schema_version(), 2);
    }

    #[rstest]
    fn test_sbe_schema_header() {
        assert_eq!(SBE_SCHEMA_HEADER, "3:2");
    }

    #[rstest]
    fn test_default_headers_include_sbe() {
        let headers = BinanceRawSpotHttpClient::default_headers(&None);

        assert_eq!(headers.get("Accept"), Some(&"application/sbe".to_string()));
        assert_eq!(headers.get("X-MBX-SBE"), Some(&"3:2".to_string()));
    }

    #[rstest]
    fn test_rate_limit_config() {
        let config = BinanceRawSpotHttpClient::rate_limit_config();

        assert!(config.default_quota.is_some());
        // Spot has 2 ORDERS quotas (SECOND and DAY)
        assert_eq!(config.order_keys.len(), 2);
    }
}
