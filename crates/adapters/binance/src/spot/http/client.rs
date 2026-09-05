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
//! - `X-MBX-SBE: 3:5` (schema ID:version)

use std::{collections::HashMap, fmt::Debug, num::NonZeroU32, sync::Arc};

use ahash::AHashMap;
use jiff::Timestamp;
use nautilus_common::cache::InstrumentLookupError;
use nautilus_core::{
    collections::AtomicMap, consts::NAUTILUS_USER_AGENT, datetime::SECONDS_IN_DAY, hex,
    nanos::UnixNanos, time::AtomicTime,
};
use nautilus_model::{
    data::{Bar, BarType, BookOrder, TradeTick},
    enums::{
        AggregationSource, BarAggregation, BookType, MarketStatusAction, OrderSide, OrderType,
        TimeInForce,
    },
    events::AccountState,
    identifiers::{AccountId, ClientOrderId, InstrumentId, Symbol, VenueOrderId},
    instruments::{Instrument, any::InstrumentAny},
    orderbook::OrderBook,
    reports::{FillReport, OrderStatusReport},
    types::{Price, Quantity},
};
use nautilus_network::{
    http::{HttpClient, HttpResponse, Method, USER_AGENT},
    ratelimiter::quota::Quota,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use super::{
    error::{BinanceSpotHttpError, BinanceSpotHttpResult},
    models::{
        AvgPrice, BatchCancelResult, BatchOrderResult, BinanceAccountCommission,
        BinanceAccountInfo, BinanceAccountRatesJson, BinanceAccountTrade, BinanceAggTrade,
        BinanceAggTrades, BinanceBalance, BinanceCancelOrderResponse, BinanceDepth,
        BinanceExchangeInfoJson, BinanceKline, BinanceKlines, BinanceNewOrderResponse,
        BinanceOrderFill, BinanceOrderResponse, BinancePriceLevel, BinanceTrade, BinanceTrades,
        BookTicker, ListenKeyResponse, NewOcoOrderListResponse, Ticker24hr, TickerPrice, TradeFee,
    },
    parse,
    query::{
        AccountCommissionParams, AccountInfoParams, AccountTradesParams, AggTradesParams,
        AllOrdersParams, AvgPriceParams, BatchCancelItem, BatchOrderItem, CancelOpenOrdersParams,
        CancelOrderParams, CancelReplaceOrderParams, DepthParams, KlinesParams, ListenKeyParams,
        NewOcoOrderListParams, NewOrderParams, OpenOrdersParams, QueryOrderParams, TickerParams,
        TradeFeeParams, TradesParams, cancel_replace_cancel_id,
    },
};
use crate::{
    common::{
        consts::{
            BINANCE_API_KEY_HEADER, BINANCE_NAUTILUS_SPOT_BROKER_ID, BINANCE_NO_SUCH_ORDER_CODE,
            BINANCE_SPOT_RATE_LIMITS, BINANCE_VENUE, BinanceRateLimitQuota,
        },
        credential::SigningCredential,
        encoder::{decode_client_order_id, encode_broker_id},
        enums::{
            BinanceEnvironment, BinanceOrderStatus, BinanceProductType, BinanceRateLimitInterval,
            BinanceRateLimitType, BinanceSelfTradePreventionMode, BinanceSide, BinanceTimeInForce,
        },
        fees::BINANCE_SPOT_FEE_DEFAULT,
        instruments::BinanceInstrumentSelector,
        models::BinanceErrorResponse,
        parse::{
            get_currency, parse_fill_report_sbe, parse_klines_to_binance_bars,
            parse_new_order_response_sbe, parse_order_status_report_sbe,
            parse_spot_instrument_json_with_fees, parse_spot_instrument_sbe_with_fees,
            parse_spot_trades_sbe,
        },
        urls::get_http_base_url,
    },
    config::BinanceInstrumentProviderConfig,
    spot::{
        enums::{
            BinanceCancelReplaceMode, BinanceOrderResponseType, BinanceSpotOrderType,
            order_type_to_binance_spot, time_in_force_to_binance_spot,
        },
        sbe::{
            generated::symbol_status::SymbolStatus,
            spot::{
                ReadBuf, SBE_SCHEMA_ID, SBE_SCHEMA_VERSION,
                error_response_codec::{self, ErrorResponseDecoder},
                message_header_codec::MessageHeaderDecoder,
                order_side::OrderSide as SbeOrderSide,
                order_status::OrderStatus as SbeOrderStatus,
                order_type::OrderType as SbeOrderType,
                self_trade_prevention_mode::SelfTradePreventionMode as SbeSelfTradePreventionMode,
                time_in_force::TimeInForce as SbeTimeInForce,
            },
        },
    },
};

/// SBE schema header value (`X-MBX-SBE`) sent on Spot API requests.
///
/// Requests the current `3:5` schema. The decoder accepts any version within schema ID `3`
/// (see `parse::MessageHeader::validate`), so compatible responses continue to decode.
pub const SBE_SCHEMA_HEADER: &str = "3:5";

use crate::common::consts::{
    BINANCE_SAPI_PATH as SAPI_PATH, BINANCE_SPOT_API_PATH as SPOT_API_PATH,
};

/// Global rate limit key.
const BINANCE_GLOBAL_RATE_KEY: &str = "binance:spot:global";

/// Orders rate limit key prefix.
const BINANCE_ORDERS_RATE_KEY: &str = "binance:spot:orders";

struct RateLimitConfig {
    default_quota: Option<Quota>,
    keyed_quotas: Vec<(String, Quota)>,
    order_keys: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotAccountJson {
    #[serde(default)]
    maker_commission: i64,
    #[serde(default)]
    taker_commission: i64,
    #[serde(default)]
    buyer_commission: i64,
    #[serde(default)]
    seller_commission: i64,
    #[serde(default)]
    commission_rates: Option<SpotCommissionRatesJson>,
    can_trade: bool,
    can_withdraw: bool,
    can_deposit: bool,
    #[serde(default)]
    require_self_trade_prevention: bool,
    #[serde(default)]
    prevent_sor: bool,
    update_time: i64,
    account_type: String,
    balances: Vec<SpotBalanceJson>,
}

#[derive(Debug, Deserialize)]
struct SpotCommissionRatesJson {
    maker: String,
    taker: String,
    buyer: String,
    seller: String,
}

#[derive(Debug, Deserialize)]
struct SpotBalanceJson {
    asset: String,
    free: String,
    locked: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotOrderJson {
    symbol: String,
    order_id: i64,
    #[serde(default)]
    order_list_id: Option<i64>,
    client_order_id: String,
    #[serde(default)]
    orig_client_order_id: String,
    #[serde(default)]
    transact_time: i64,
    #[serde(default)]
    price: String,
    #[serde(default)]
    orig_qty: String,
    #[serde(default)]
    executed_qty: String,
    #[serde(default, rename = "cummulativeQuoteQty")]
    cummulative_quote_qty: String,
    status: BinanceOrderStatus,
    time_in_force: BinanceTimeInForce,
    #[serde(rename = "type")]
    order_type: BinanceSpotOrderType,
    side: BinanceSide,
    #[serde(default)]
    stop_price: String,
    #[serde(default)]
    iceberg_qty: String,
    #[serde(default)]
    time: i64,
    #[serde(default)]
    update_time: i64,
    #[serde(default)]
    is_working: bool,
    #[serde(default)]
    working_time: Option<i64>,
    #[serde(default)]
    orig_quote_order_qty: String,
    #[serde(default)]
    self_trade_prevention_mode: Option<BinanceSelfTradePreventionMode>,
    #[serde(default)]
    fills: Vec<SpotOrderFillJson>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotCancelReplaceJson {
    new_order_response: SpotOrderJson,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotOrderFillJson {
    price: String,
    qty: String,
    commission: String,
    commission_asset: String,
    #[serde(default)]
    trade_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotAccountTradeJson {
    symbol: String,
    id: i64,
    order_id: i64,
    #[serde(default)]
    order_list_id: Option<i64>,
    price: String,
    qty: String,
    quote_qty: String,
    commission: String,
    commission_asset: String,
    time: i64,
    is_buyer: bool,
    is_maker: bool,
    is_best_match: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotDepthJson {
    last_update_id: i64,
    bids: Vec<[String; 2]>,
    asks: Vec<[String; 2]>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SpotTradeJson {
    id: i64,
    price: String,
    qty: String,
    quote_qty: String,
    time: i64,
    is_buyer_maker: bool,
    is_best_match: bool,
}

#[derive(Debug, Deserialize)]
struct SpotAggTradeJson {
    #[serde(rename = "a")]
    id: i64,
    #[serde(rename = "p")]
    price: String,
    #[serde(rename = "q")]
    qty: String,
    #[serde(rename = "f")]
    first_trade_id: i64,
    #[serde(rename = "l")]
    last_trade_id: i64,
    #[serde(rename = "T")]
    time: i64,
    #[serde(rename = "m")]
    is_buyer_maker: bool,
    #[serde(rename = "M")]
    is_best_match: bool,
}

type SpotKlineJson = (
    i64,
    String,
    String,
    String,
    String,
    String,
    i64,
    String,
    i64,
    String,
    String,
    String,
);

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
    credential: Option<SigningCredential>,
    recv_window: Option<u64>,
    order_rate_keys: Vec<String>,
    json_responses: bool,
}

impl BinanceRawSpotHttpClient {
    /// Returns whether signed requests can be made.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        self.credential.is_some()
    }

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
        Self::new_with_json_responses(
            environment,
            api_key,
            api_secret,
            base_url_override,
            recv_window,
            timeout_secs,
            proxy_url,
            false,
        )
    }

    /// Creates a raw Spot client with JSON responses instead of Global SBE responses.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying [`HttpClient`] fails to build.
    #[expect(clippy::too_many_arguments)]
    pub fn new_with_json_responses(
        environment: BinanceEnvironment,
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url_override: Option<String>,
        recv_window: Option<u64>,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
        json_responses: bool,
    ) -> BinanceSpotHttpResult<Self> {
        let RateLimitConfig {
            default_quota,
            keyed_quotas,
            order_keys,
        } = Self::rate_limit_config();

        let credential = match (api_key, api_secret) {
            (Some(key), Some(secret)) => Some(SigningCredential::new(key, secret)),
            (None, None) => None,
            _ => return Err(BinanceSpotHttpError::MissingCredentials),
        };

        let base_url = base_url_override.unwrap_or_else(|| {
            get_http_base_url(BinanceProductType::Spot, environment).to_string()
        });

        let headers = Self::default_headers(&credential, json_responses);

        let client = HttpClient::builder()
            .headers(headers)
            .header_keys(vec![BINANCE_API_KEY_HEADER.to_string()])
            .keyed_quotas(keyed_quotas)
            .maybe_default_quota(default_quota)
            .maybe_timeout_secs(timeout_secs)
            .maybe_proxy_url(proxy_url)
            .build()?;

        Ok(Self {
            client,
            base_url,
            credential,
            recv_window,
            order_rate_keys: order_keys,
            json_responses,
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

    /// Performs a signed GET request and requests a JSON response.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing or the request fails.
    pub async fn get_signed_json<P>(
        &self,
        path: &str,
        params: Option<&P>,
    ) -> BinanceSpotHttpResult<Vec<u8>>
    where
        P: Serialize + ?Sized,
    {
        self.request_with_extra_headers(
            Method::GET,
            path,
            params,
            true,
            false,
            Some(HashMap::from([(
                "Accept".to_string(),
                "application/json".to_string(),
            )])),
        )
        .await
    }

    /// Performs a signed POST request and returns raw response bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing or the request fails.
    pub async fn post_signed<P>(
        &self,
        path: &str,
        params: Option<&P>,
    ) -> BinanceSpotHttpResult<Vec<u8>>
    where
        P: Serialize + ?Sized,
    {
        self.request(Method::POST, path, params, true, true).await
    }

    /// Performs a signed POST request and requests a JSON response.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing or the request fails.
    pub async fn post_signed_json<P>(
        &self,
        path: &str,
        params: Option<&P>,
    ) -> BinanceSpotHttpResult<Vec<u8>>
    where
        P: Serialize + ?Sized,
    {
        self.request_with_extra_headers(
            Method::POST,
            path,
            params,
            true,
            true,
            Some(HashMap::from([(
                "Accept".to_string(),
                "application/json".to_string(),
            )])),
        )
        .await
    }

    /// Performs a signed DELETE request and returns raw response bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing or the request fails.
    pub async fn delete_signed<P>(
        &self,
        path: &str,
        params: Option<&P>,
    ) -> BinanceSpotHttpResult<Vec<u8>>
    where
        P: Serialize + ?Sized,
    {
        self.request(Method::DELETE, path, params, true, true).await
    }

    /// Performs a signed DELETE request and requests a JSON response.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing or the request fails.
    pub async fn delete_signed_json<P>(
        &self,
        path: &str,
        params: Option<&P>,
    ) -> BinanceSpotHttpResult<Vec<u8>>
    where
        P: Serialize + ?Sized,
    {
        self.request_with_extra_headers(
            Method::DELETE,
            path,
            params,
            true,
            true,
            Some(HashMap::from([(
                "Accept".to_string(),
                "application/json".to_string(),
            )])),
        )
        .await
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
        self.request_with_extra_headers(method, path, params, signed, use_order_quota, None)
            .await
    }

    async fn request_with_extra_headers<P>(
        &self,
        method: Method,
        path: &str,
        params: Option<&P>,
        signed: bool,
        use_order_quota: bool,
        extra_headers: Option<HashMap<String, String>>,
    ) -> BinanceSpotHttpResult<Vec<u8>>
    where
        P: Serialize + ?Sized,
    {
        let mut query = params
            .map(serde_urlencoded::to_string)
            .transpose()
            .map_err(|e| BinanceSpotHttpError::ValidationError(e.to_string()))?
            .unwrap_or_default();

        let mut headers = extra_headers.unwrap_or_default();

        if signed {
            let cred = self
                .credential
                .as_ref()
                .ok_or(BinanceSpotHttpError::MissingCredentials)?;

            if !query.is_empty() {
                query.push('&');
            }

            let timestamp = Timestamp::now().as_millisecond();
            query.push_str(&format!("timestamp={timestamp}"));

            if let Some(recv_window) = self.recv_window {
                query.push_str(&format!("&recvWindow={recv_window}"));
            }

            let signature = Self::percent_encode(&cred.sign(&query));
            query.push_str(&format!("&signature={signature}"));
            headers.insert(
                BINANCE_API_KEY_HEADER.to_string(),
                cred.api_key().to_string(),
            );
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
            return self.parse_error_response(&response);
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

    fn parse_error_response<T>(&self, response: &HttpResponse) -> BinanceSpotHttpResult<T> {
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

    fn default_headers(
        credential: &Option<SigningCredential>,
        json_responses: bool,
    ) -> HashMap<String, String> {
        let mut headers = HashMap::new();
        headers.insert(USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string());
        if json_responses {
            headers.insert("Accept".to_string(), "application/json".to_string());
        } else {
            headers.insert("Accept".to_string(), "application/sbe".to_string());
            headers.insert("X-MBX-SBE".to_string(), SBE_SCHEMA_HEADER.to_string());
        }

        if let Some(cred) = credential {
            headers.insert(
                BINANCE_API_KEY_HEADER.to_string(),
                cred.api_key().to_string(),
            );
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

        let default_quota = default.unwrap_or_else(|| {
            Quota::per_second(NonZeroU32::new(10).expect("non-zero")).expect("valid constant")
        });

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
            BinanceRateLimitInterval::Second => Quota::per_second(burst),
            BinanceRateLimitInterval::Minute => Some(Quota::per_minute(burst)),
            BinanceRateLimitInterval::Day => {
                Quota::with_period(std::time::Duration::from_secs(SECONDS_IN_DAY))
                    .map(|q| q.allow_burst(burst))
            }
            BinanceRateLimitInterval::Unknown => None,
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
        if self.json_responses {
            #[derive(Deserialize)]
            #[serde(rename_all = "camelCase")]
            struct ServerTimeJson {
                server_time: i64,
            }

            let bytes = self.get_json("time", None::<&()>).await?;
            let response: ServerTimeJson = serde_json::from_slice(&bytes)
                .map_err(|e| BinanceSpotHttpError::JsonError(e.to_string()))?;
            return millis_to_micros(response.server_time);
        }

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

    /// Returns JSON exchange information for endpoints that do not support SBE.
    ///
    /// # Errors
    ///
    /// Returns an error if the request or JSON decode fails.
    pub async fn exchange_info_json(&self) -> BinanceSpotHttpResult<BinanceExchangeInfoJson> {
        let bytes = self.get_json("exchangeInfo", None::<&()>).await?;
        serde_json::from_slice(&bytes).map_err(|e| BinanceSpotHttpError::JsonError(e.to_string()))
    }

    /// Returns order book depth for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    pub async fn depth(&self, params: &DepthParams) -> BinanceSpotHttpResult<BinanceDepth> {
        if self.json_responses {
            let bytes = self.get_json("depth", Some(params)).await?;
            let response: SpotDepthJson = serde_json::from_slice(&bytes)
                .map_err(|e| BinanceSpotHttpError::JsonError(e.to_string()))?;
            spot_depth_from_json(response)
        } else {
            let bytes = self.get("depth", Some(params)).await?;
            Ok(parse::decode_depth(&bytes)?)
        }
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

        if self.json_responses {
            let bytes = self.get_json("trades", Some(&params)).await?;
            let response: Vec<SpotTradeJson> = serde_json::from_slice(&bytes)
                .map_err(|e| BinanceSpotHttpError::JsonError(e.to_string()))?;
            spot_trades_from_json(response)
        } else {
            let bytes = self.get("trades", Some(&params)).await?;
            Ok(parse::decode_trades(&bytes)?)
        }
    }

    /// Returns aggregate trades for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the bounds are invalid or the request cannot be decoded.
    pub async fn agg_trades(
        &self,
        params: &AggTradesParams,
    ) -> BinanceSpotHttpResult<BinanceAggTrades> {
        if params.limit.is_some_and(|limit| limit > 1000) {
            return Err(BinanceSpotHttpError::ValidationError(
                "aggregate trade limit must not exceed 1000".to_string(),
            ));
        }

        if matches!((params.start_time, params.end_time), (Some(start), Some(end)) if start > end) {
            return Err(BinanceSpotHttpError::ValidationError(
                "aggregate trade startTime must not exceed endTime".to_string(),
            ));
        }

        if self.json_responses {
            let bytes = self.get_json("aggTrades", Some(params)).await?;
            let response: Vec<SpotAggTradeJson> = serde_json::from_slice(&bytes)
                .map_err(|e| BinanceSpotHttpError::JsonError(e.to_string()))?;
            spot_agg_trades_from_json(response)
        } else {
            let bytes = self.get("aggTrades", Some(params)).await?;
            Ok(parse::decode_agg_trades(&bytes)?)
        }
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

        if self.json_responses {
            let bytes = self.get_json("klines", Some(&params)).await?;
            let response: Vec<SpotKlineJson> = serde_json::from_slice(&bytes)
                .map_err(|e| BinanceSpotHttpError::JsonError(e.to_string()))?;
            spot_klines_from_json(response)
        } else {
            let bytes = self.get("klines", Some(&params)).await?;
            Ok(parse::decode_klines(&bytes)?)
        }
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
            return self.parse_error_response(&response);
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

        let timestamp = Timestamp::now().as_millisecond();
        query.push_str(&format!("timestamp={timestamp}"));

        if let Some(recv_window) = self.recv_window {
            query.push_str(&format!("&recvWindow={recv_window}"));
        }

        let signature = Self::percent_encode(&cred.sign(&query));
        query.push_str(&format!("&signature={signature}"));

        // Build SAPI URL (different from regular API path)
        let normalized_path = if path.starts_with('/') {
            path.to_string()
        } else {
            format!("/{path}")
        };

        let mut url = format!("{}{}{}", self.base_url, SAPI_PATH, normalized_path);

        if !query.is_empty() {
            url.push('?');
            url.push_str(&query);
        }

        let mut headers = HashMap::new();
        headers.insert(
            BINANCE_API_KEY_HEADER.to_string(),
            cred.api_key().to_string(),
        );

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
            return self.parse_error_response(&response);
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
        let timestamp = Timestamp::now().as_millisecond();
        let mut query = format!("batchOrders={encoded_batch}&timestamp={timestamp}");

        if let Some(recv_window) = self.recv_window {
            query.push_str(&format!("&recvWindow={recv_window}"));
        }

        let signature = Self::percent_encode(&cred.sign(&query));
        query.push_str(&format!("&signature={signature}"));

        let url = self.build_url(path, &query);

        let mut headers = HashMap::new();
        headers.insert(
            BINANCE_API_KEY_HEADER.to_string(),
            cred.api_key().to_string(),
        );

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
            return self.parse_error_response(&response);
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
        if self.json_responses {
            let bytes = self.get_signed_json("account", Some(params)).await?;
            let response: SpotAccountJson = serde_json::from_slice(&bytes)
                .map_err(|e| BinanceSpotHttpError::JsonError(e.to_string()))?;
            spot_account_from_json(response)
        } else {
            let bytes = self.get_signed("account", Some(params)).await?;
            Ok(parse::decode_account(&bytes)?)
        }
    }

    /// Returns account-specific commission rates for one symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or JSON is malformed.
    pub async fn account_commission(
        &self,
        symbol: &str,
    ) -> BinanceSpotHttpResult<BinanceAccountCommission> {
        let params = AccountCommissionParams::new(symbol);
        let bytes = self
            .get_signed_json("account/commission", Some(&params))
            .await?;
        serde_json::from_slice(&bytes).map_err(|e| BinanceSpotHttpError::JsonError(e.to_string()))
    }

    /// Returns the minimal JSON account commission view used by Binance US.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or JSON is malformed.
    pub async fn account_rates_json(&self) -> BinanceSpotHttpResult<BinanceAccountRatesJson> {
        let params = AccountInfoParams::default();
        let bytes = self.get_signed_json("account", Some(&params)).await?;
        serde_json::from_slice(&bytes).map_err(|e| BinanceSpotHttpError::JsonError(e.to_string()))
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
        self.account_trades_with_cursor(symbol, order_id, start_time, end_time, None, limit)
            .await
    }

    async fn account_trades_with_cursor(
        &self,
        symbol: &str,
        order_id: Option<i64>,
        start_time: Option<i64>,
        end_time: Option<i64>,
        from_id: Option<i64>,
        limit: Option<u32>,
    ) -> BinanceSpotHttpResult<Vec<BinanceAccountTrade>> {
        let params = AccountTradesParams {
            symbol: symbol.to_string(),
            order_id,
            start_time,
            end_time,
            from_id,
            limit,
        };

        if self.json_responses {
            let bytes = self.get_signed_json("myTrades", Some(&params)).await?;
            let response: Vec<SpotAccountTradeJson> = serde_json::from_slice(&bytes)
                .map_err(|e| BinanceSpotHttpError::JsonError(e.to_string()))?;
            response
                .into_iter()
                .map(spot_account_trade_from_json)
                .collect()
        } else {
            let bytes = self.get_signed("myTrades", Some(&params)).await?;
            Ok(parse::decode_account_trades(&bytes)?)
        }
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

        if self.json_responses {
            let bytes = self.get_signed_json("order", Some(&params)).await?;
            let response: SpotOrderJson = serde_json::from_slice(&bytes)
                .map_err(|e| BinanceSpotHttpError::JsonError(e.to_string()))?;
            spot_order_from_json(response)
        } else {
            let bytes = self.get_signed("order", Some(&params)).await?;
            Ok(parse::decode_order(&bytes)?)
        }
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

        if self.json_responses {
            let bytes = self.get_signed_json("openOrders", Some(&params)).await?;
            let response: Vec<SpotOrderJson> = serde_json::from_slice(&bytes)
                .map_err(|e| BinanceSpotHttpError::JsonError(e.to_string()))?;
            response.into_iter().map(spot_order_from_json).collect()
        } else {
            let bytes = self.get_signed("openOrders", Some(&params)).await?;
            Ok(parse::decode_orders(&bytes)?)
        }
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

        if self.json_responses {
            let bytes = self.get_signed_json("allOrders", Some(&params)).await?;
            let response: Vec<SpotOrderJson> = serde_json::from_slice(&bytes)
                .map_err(|e| BinanceSpotHttpError::JsonError(e.to_string()))?;
            response.into_iter().map(spot_order_from_json).collect()
        } else {
            let bytes = self.get_signed("allOrders", Some(&params)).await?;
            Ok(parse::decode_orders(&bytes)?)
        }
    }

    /// Performs a signed POST request for order operations.
    async fn post_order<P>(&self, path: &str, params: Option<&P>) -> BinanceSpotHttpResult<Vec<u8>>
    where
        P: Serialize + ?Sized,
    {
        if self.json_responses {
            self.post_signed_json(path, params).await
        } else {
            self.post_signed(path, params).await
        }
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
        if self.json_responses {
            self.delete_signed_json(path, params).await
        } else {
            self.delete_signed(path, params).await
        }
    }

    /// Creates a new order.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    #[expect(clippy::too_many_arguments)]
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
        self.decode_new_order_response(&bytes)
    }

    /// Creates a new order with full parameter support.
    ///
    /// Extends [`new_order`](Self::new_order) with `quote_order_qty` (for market
    /// orders denominated in quote currency) and `iceberg_qty` (display
    /// quantity for iceberg orders).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    #[expect(clippy::too_many_arguments)]
    pub async fn new_order_full(
        &self,
        symbol: &str,
        side: BinanceSide,
        order_type: BinanceSpotOrderType,
        time_in_force: Option<BinanceTimeInForce>,
        quantity: Option<&str>,
        quote_order_qty: Option<&str>,
        price: Option<&str>,
        client_order_id: Option<&str>,
        stop_price: Option<&str>,
        iceberg_qty: Option<&str>,
    ) -> BinanceSpotHttpResult<BinanceNewOrderResponse> {
        let params = NewOrderParams {
            symbol: symbol.to_string(),
            side,
            order_type,
            time_in_force,
            quantity: quantity.map(|s| s.to_string()),
            quote_order_qty: quote_order_qty.map(|s| s.to_string()),
            price: price.map(|s| s.to_string()),
            new_client_order_id: client_order_id.map(|s| s.to_string()),
            stop_price: stop_price.map(|s| s.to_string()),
            trailing_delta: None,
            iceberg_qty: iceberg_qty.map(|s| s.to_string()),
            new_order_resp_type: Some(BinanceOrderResponseType::Full),
            self_trade_prevention_mode: None,
            strategy_id: None,
            strategy_type: None,
        };
        let bytes = self.post_order("order", Some(&params)).await?;
        self.decode_new_order_response(&bytes)
    }

    /// Creates a new OCO order list.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or JSON decoding fails.
    pub async fn new_oco_order_list(
        &self,
        params: &NewOcoOrderListParams,
    ) -> BinanceSpotHttpResult<NewOcoOrderListResponse> {
        let bytes = self.post_signed_json("orderList/oco", Some(params)).await?;
        serde_json::from_slice(&bytes).map_err(|e| BinanceSpotHttpError::JsonError(e.to_string()))
    }

    /// Cancels an existing order and places a new order atomically.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or SBE decoding fails.
    #[expect(clippy::too_many_arguments)]
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
            cancel_new_client_order_id: Some(cancel_replace_cancel_id(cancel_order_id)),
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

        if self.json_responses {
            let response: SpotCancelReplaceJson = serde_json::from_slice(&bytes)
                .map_err(|e| BinanceSpotHttpError::JsonError(e.to_string()))?;
            spot_new_order_from_json(response.new_order_response)
        } else {
            self.decode_new_order_response(&bytes)
        }
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
        self.decode_cancel_order_response(&bytes)
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
        if self.json_responses {
            let response: Vec<SpotOrderJson> = serde_json::from_slice(&bytes)
                .map_err(|e| BinanceSpotHttpError::JsonError(e.to_string()))?;
            response
                .into_iter()
                .map(spot_cancel_order_from_json)
                .collect()
        } else {
            Ok(parse::decode_cancel_open_orders(&bytes)?)
        }
    }

    fn decode_new_order_response(
        &self,
        bytes: &[u8],
    ) -> BinanceSpotHttpResult<BinanceNewOrderResponse> {
        if self.json_responses {
            let response: SpotOrderJson = serde_json::from_slice(bytes)
                .map_err(|e| BinanceSpotHttpError::JsonError(e.to_string()))?;
            spot_new_order_from_json(response)
        } else {
            Ok(parse::decode_new_order_full(bytes)?)
        }
    }

    fn decode_cancel_order_response(
        &self,
        bytes: &[u8],
    ) -> BinanceSpotHttpResult<BinanceCancelOrderResponse> {
        if self.json_responses {
            let response: SpotOrderJson = serde_json::from_slice(bytes)
                .map_err(|e| BinanceSpotHttpError::JsonError(e.to_string()))?;
            spot_cancel_order_from_json(response)
        } else {
            Ok(parse::decode_cancel_order(bytes)?)
        }
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
        headers.insert(
            BINANCE_API_KEY_HEADER.to_string(),
            cred.api_key().to_string(),
        );

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
            return self.parse_error_response(&response);
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

fn spot_depth_from_json(response: SpotDepthJson) -> BinanceSpotHttpResult<BinanceDepth> {
    let price_scale = decimal_common_scale(
        response
            .bids
            .iter()
            .chain(&response.asks)
            .map(|level| level[0].as_str()),
    )?;
    let qty_scale = decimal_common_scale(
        response
            .bids
            .iter()
            .chain(&response.asks)
            .map(|level| level[1].as_str()),
    )?;
    let parse_levels = |levels: Vec<[String; 2]>| {
        levels
            .into_iter()
            .map(|level| {
                Ok(BinancePriceLevel {
                    price_mantissa: decimal_mantissa(&level[0], price_scale)?,
                    qty_mantissa: decimal_mantissa(&level[1], qty_scale)?,
                })
            })
            .collect::<BinanceSpotHttpResult<Vec<_>>>()
    };

    Ok(BinanceDepth {
        last_update_id: response.last_update_id,
        price_exponent: decimal_exponent(price_scale)?,
        qty_exponent: decimal_exponent(qty_scale)?,
        bids: parse_levels(response.bids)?,
        asks: parse_levels(response.asks)?,
    })
}

fn spot_trades_from_json(response: Vec<SpotTradeJson>) -> BinanceSpotHttpResult<BinanceTrades> {
    let price_scale = decimal_common_scale(
        response
            .iter()
            .flat_map(|trade| [trade.price.as_str(), trade.quote_qty.as_str()]),
    )?;
    let qty_scale = decimal_common_scale(response.iter().map(|trade| trade.qty.as_str()))?;
    let trades = response
        .into_iter()
        .map(|trade| {
            Ok(BinanceTrade {
                id: trade.id,
                price_mantissa: decimal_mantissa(&trade.price, price_scale)?,
                qty_mantissa: decimal_mantissa(&trade.qty, qty_scale)?,
                quote_qty_mantissa: decimal_mantissa(&trade.quote_qty, price_scale)?,
                time: millis_to_micros(trade.time)?,
                is_buyer_maker: trade.is_buyer_maker,
                is_best_match: trade.is_best_match,
            })
        })
        .collect::<BinanceSpotHttpResult<Vec<_>>>()?;

    Ok(BinanceTrades {
        price_exponent: decimal_exponent(price_scale)?,
        qty_exponent: decimal_exponent(qty_scale)?,
        trades,
    })
}

fn spot_agg_trades_from_json(
    response: Vec<SpotAggTradeJson>,
) -> BinanceSpotHttpResult<BinanceAggTrades> {
    let price_scale = decimal_common_scale(response.iter().map(|trade| trade.price.as_str()))?;
    let qty_scale = decimal_common_scale(response.iter().map(|trade| trade.qty.as_str()))?;
    let trades = response
        .into_iter()
        .map(|trade| {
            Ok(BinanceAggTrade {
                id: trade.id,
                price_mantissa: decimal_mantissa(&trade.price, price_scale)?,
                qty_mantissa: decimal_mantissa(&trade.qty, qty_scale)?,
                first_trade_id: trade.first_trade_id,
                last_trade_id: trade.last_trade_id,
                time: millis_to_micros(trade.time)?,
                is_buyer_maker: trade.is_buyer_maker,
                is_best_match: trade.is_best_match,
            })
        })
        .collect::<BinanceSpotHttpResult<Vec<_>>>()?;

    Ok(BinanceAggTrades {
        price_exponent: decimal_exponent(price_scale)?,
        qty_exponent: decimal_exponent(qty_scale)?,
        trades,
    })
}

fn spot_klines_from_json(response: Vec<SpotKlineJson>) -> BinanceSpotHttpResult<BinanceKlines> {
    let price_scale = decimal_common_scale(response.iter().flat_map(|kline| {
        [
            kline.1.as_str(),
            kline.2.as_str(),
            kline.3.as_str(),
            kline.4.as_str(),
            kline.7.as_str(),
            kline.10.as_str(),
        ]
    }))?;
    let qty_scale = decimal_common_scale(
        response
            .iter()
            .flat_map(|kline| [kline.5.as_str(), kline.9.as_str()]),
    )?;
    let klines = response
        .into_iter()
        .map(|kline| {
            let volume = decimal_i128_bytes(&kline.5, qty_scale)?;
            let quote_volume = decimal_i128_bytes(&kline.7, price_scale)?;
            let taker_buy_base_volume = decimal_i128_bytes(&kline.9, qty_scale)?;
            let taker_buy_quote_volume = decimal_i128_bytes(&kline.10, price_scale)?;
            Ok(BinanceKline {
                open_time: millis_to_micros(kline.0)?,
                open_price: decimal_mantissa(&kline.1, price_scale)?,
                high_price: decimal_mantissa(&kline.2, price_scale)?,
                low_price: decimal_mantissa(&kline.3, price_scale)?,
                close_price: decimal_mantissa(&kline.4, price_scale)?,
                volume,
                close_time: millis_to_micros(kline.6)?,
                quote_volume,
                num_trades: kline.8,
                taker_buy_base_volume,
                taker_buy_quote_volume,
            })
        })
        .collect::<BinanceSpotHttpResult<Vec<_>>>()?;

    Ok(BinanceKlines {
        price_exponent: decimal_exponent(price_scale)?,
        qty_exponent: decimal_exponent(qty_scale)?,
        klines,
    })
}

fn decimal_i128_bytes(value: &str, scale: u32) -> BinanceSpotHttpResult<[u8; 16]> {
    let mut decimal = Decimal::from_str_exact(value)
        .map_err(|e| BinanceSpotHttpError::ResponseParseError(e.to_string()))?;
    decimal.rescale(scale);
    Ok(decimal.mantissa().to_le_bytes())
}

fn spot_account_from_json(response: SpotAccountJson) -> BinanceSpotHttpResult<BinanceAccountInfo> {
    let commission_rates = response.commission_rates.map_or_else(
        || {
            [
                Decimal::new(response.maker_commission, 4).to_string(),
                Decimal::new(response.taker_commission, 4).to_string(),
                Decimal::new(response.buyer_commission, 4).to_string(),
                Decimal::new(response.seller_commission, 4).to_string(),
            ]
        },
        |rates| [rates.maker, rates.taker, rates.buyer, rates.seller],
    );
    let commission_scale = decimal_common_scale(commission_rates.iter().map(String::as_str))?;
    let balances = response
        .balances
        .into_iter()
        .map(|balance| {
            let scale = decimal_common_scale([balance.free.as_str(), balance.locked.as_str()])?;
            Ok(BinanceBalance {
                asset: balance.asset,
                free_mantissa: decimal_mantissa(&balance.free, scale)?,
                locked_mantissa: decimal_mantissa(&balance.locked, scale)?,
                exponent: decimal_exponent(scale)?,
            })
        })
        .collect::<BinanceSpotHttpResult<Vec<_>>>()?;

    Ok(BinanceAccountInfo {
        commission_exponent: decimal_exponent(commission_scale)?,
        maker_commission_mantissa: decimal_mantissa(&commission_rates[0], commission_scale)?,
        taker_commission_mantissa: decimal_mantissa(&commission_rates[1], commission_scale)?,
        buyer_commission_mantissa: decimal_mantissa(&commission_rates[2], commission_scale)?,
        seller_commission_mantissa: decimal_mantissa(&commission_rates[3], commission_scale)?,
        can_trade: response.can_trade,
        can_withdraw: response.can_withdraw,
        can_deposit: response.can_deposit,
        require_self_trade_prevention: response.require_self_trade_prevention,
        prevent_sor: response.prevent_sor,
        update_time: millis_to_micros(response.update_time)?,
        account_type: response.account_type,
        balances,
    })
}

fn spot_new_order_from_json(
    response: SpotOrderJson,
) -> BinanceSpotHttpResult<BinanceNewOrderResponse> {
    let (price_scale, qty_scale) = spot_order_scales(&response)?;
    let fills = response
        .fills
        .iter()
        .map(|fill| {
            let commission_scale = decimal_common_scale([fill.commission.as_str()])?;
            Ok(BinanceOrderFill {
                price_mantissa: decimal_mantissa(&fill.price, price_scale)?,
                qty_mantissa: decimal_mantissa(&fill.qty, qty_scale)?,
                commission_mantissa: decimal_mantissa(&fill.commission, commission_scale)?,
                commission_exponent: decimal_exponent(commission_scale)?,
                commission_asset: fill.commission_asset.clone(),
                trade_id: fill.trade_id,
            })
        })
        .collect::<BinanceSpotHttpResult<Vec<_>>>()?;

    Ok(BinanceNewOrderResponse {
        price_exponent: decimal_exponent(price_scale)?,
        qty_exponent: decimal_exponent(qty_scale)?,
        order_id: response.order_id,
        order_list_id: valid_order_list_id(response.order_list_id),
        transact_time: millis_to_micros(response.transact_time)?,
        price_mantissa: decimal_mantissa(&response.price, price_scale)?,
        orig_qty_mantissa: decimal_mantissa(&response.orig_qty, qty_scale)?,
        executed_qty_mantissa: decimal_mantissa(&response.executed_qty, qty_scale)?,
        cummulative_quote_qty_mantissa: decimal_mantissa(
            &response.cummulative_quote_qty,
            price_scale + qty_scale,
        )?,
        status: spot_sbe_order_status(response.status),
        time_in_force: spot_sbe_time_in_force(response.time_in_force),
        order_type: spot_sbe_order_type(response.order_type),
        side: spot_sbe_order_side(response.side),
        stop_price_mantissa: decimal_optional_mantissa(&response.stop_price, price_scale)?,
        working_time: response.working_time.map(millis_to_micros).transpose()?,
        self_trade_prevention_mode: spot_sbe_stp(response.self_trade_prevention_mode),
        client_order_id: response.client_order_id,
        symbol: response.symbol,
        fills,
        expiry_reason: None,
    })
}

fn spot_cancel_order_from_json(
    response: SpotOrderJson,
) -> BinanceSpotHttpResult<BinanceCancelOrderResponse> {
    let (price_scale, qty_scale) = spot_order_scales(&response)?;
    Ok(BinanceCancelOrderResponse {
        price_exponent: decimal_exponent(price_scale)?,
        qty_exponent: decimal_exponent(qty_scale)?,
        order_id: response.order_id,
        order_list_id: valid_order_list_id(response.order_list_id),
        transact_time: millis_to_micros(response.transact_time)?,
        price_mantissa: decimal_mantissa(&response.price, price_scale)?,
        orig_qty_mantissa: decimal_mantissa(&response.orig_qty, qty_scale)?,
        executed_qty_mantissa: decimal_mantissa(&response.executed_qty, qty_scale)?,
        cummulative_quote_qty_mantissa: decimal_mantissa(
            &response.cummulative_quote_qty,
            price_scale + qty_scale,
        )?,
        status: spot_sbe_order_status(response.status),
        time_in_force: spot_sbe_time_in_force(response.time_in_force),
        order_type: spot_sbe_order_type(response.order_type),
        side: spot_sbe_order_side(response.side),
        self_trade_prevention_mode: spot_sbe_stp(response.self_trade_prevention_mode),
        client_order_id: response.client_order_id,
        orig_client_order_id: response.orig_client_order_id,
        symbol: response.symbol,
    })
}

fn spot_order_from_json(response: SpotOrderJson) -> BinanceSpotHttpResult<BinanceOrderResponse> {
    let (price_scale, qty_scale) = spot_order_scales(&response)?;
    Ok(BinanceOrderResponse {
        price_exponent: decimal_exponent(price_scale)?,
        qty_exponent: decimal_exponent(qty_scale)?,
        order_id: response.order_id,
        order_list_id: valid_order_list_id(response.order_list_id),
        price_mantissa: decimal_mantissa(&response.price, price_scale)?,
        orig_qty_mantissa: decimal_mantissa(&response.orig_qty, qty_scale)?,
        executed_qty_mantissa: decimal_mantissa(&response.executed_qty, qty_scale)?,
        cummulative_quote_qty_mantissa: decimal_mantissa(
            &response.cummulative_quote_qty,
            price_scale + qty_scale,
        )?,
        status: spot_sbe_order_status(response.status),
        time_in_force: spot_sbe_time_in_force(response.time_in_force),
        order_type: spot_sbe_order_type(response.order_type),
        side: spot_sbe_order_side(response.side),
        stop_price_mantissa: decimal_optional_mantissa(&response.stop_price, price_scale)?,
        iceberg_qty_mantissa: decimal_optional_mantissa(&response.iceberg_qty, qty_scale)?,
        time: millis_to_micros(response.time)?,
        update_time: millis_to_micros(response.update_time)?,
        is_working: response.is_working,
        working_time: response.working_time.map(millis_to_micros).transpose()?,
        orig_quote_order_qty_mantissa: decimal_mantissa(
            &response.orig_quote_order_qty,
            price_scale + qty_scale,
        )?,
        self_trade_prevention_mode: spot_sbe_stp(response.self_trade_prevention_mode),
        client_order_id: response.client_order_id,
        symbol: response.symbol,
        expiry_reason: None,
    })
}

fn spot_account_trade_from_json(
    response: SpotAccountTradeJson,
) -> BinanceSpotHttpResult<BinanceAccountTrade> {
    let price_scale = decimal_common_scale([response.price.as_str()])?;
    let qty_scale = decimal_common_scale([response.qty.as_str()])?;
    let commission_scale = decimal_common_scale([response.commission.as_str()])?;
    Ok(BinanceAccountTrade {
        price_exponent: decimal_exponent(price_scale)?,
        qty_exponent: decimal_exponent(qty_scale)?,
        commission_exponent: decimal_exponent(commission_scale)?,
        id: response.id,
        order_id: response.order_id,
        order_list_id: valid_order_list_id(response.order_list_id),
        price_mantissa: decimal_mantissa(&response.price, price_scale)?,
        qty_mantissa: decimal_mantissa(&response.qty, qty_scale)?,
        quote_qty_mantissa: decimal_mantissa(&response.quote_qty, price_scale + qty_scale)?,
        commission_mantissa: decimal_mantissa(&response.commission, commission_scale)?,
        time: millis_to_micros(response.time)?,
        is_buyer: response.is_buyer,
        is_maker: response.is_maker,
        is_best_match: response.is_best_match,
        symbol: response.symbol,
        commission_asset: response.commission_asset,
    })
}

fn spot_order_scales(response: &SpotOrderJson) -> BinanceSpotHttpResult<(u32, u32)> {
    let price_scale = decimal_common_scale(
        [response.price.as_str(), response.stop_price.as_str()]
            .into_iter()
            .chain(response.fills.iter().map(|fill| fill.price.as_str())),
    )?;
    let qty_scale = decimal_common_scale(
        [
            response.orig_qty.as_str(),
            response.executed_qty.as_str(),
            response.iceberg_qty.as_str(),
        ]
        .into_iter()
        .chain(response.fills.iter().map(|fill| fill.qty.as_str())),
    )?;
    Ok((price_scale, qty_scale))
}

fn decimal_common_scale<'a>(
    values: impl IntoIterator<Item = &'a str>,
) -> BinanceSpotHttpResult<u32> {
    values
        .into_iter()
        .filter(|value| !value.is_empty())
        .try_fold(0, |scale, value| {
            let decimal = Decimal::from_str_exact(value)
                .map_err(|e| BinanceSpotHttpError::ResponseParseError(e.to_string()))?;
            Ok(scale.max(decimal.scale()))
        })
}

fn decimal_mantissa(value: &str, scale: u32) -> BinanceSpotHttpResult<i64> {
    if value.is_empty() {
        return Ok(0);
    }
    let mut decimal = Decimal::from_str_exact(value)
        .map_err(|e| BinanceSpotHttpError::ResponseParseError(e.to_string()))?;
    decimal.rescale(scale);
    i64::try_from(decimal.mantissa()).map_err(|_| {
        BinanceSpotHttpError::ResponseParseError(format!(
            "decimal mantissa is outside i64 range: {value}"
        ))
    })
}

fn decimal_optional_mantissa(value: &str, scale: u32) -> BinanceSpotHttpResult<Option<i64>> {
    let mantissa = decimal_mantissa(value, scale)?;
    Ok((mantissa != 0).then_some(mantissa))
}

fn decimal_exponent(scale: u32) -> BinanceSpotHttpResult<i8> {
    i8::try_from(scale)
        .map(|scale| -scale)
        .map_err(|_| BinanceSpotHttpError::ResponseParseError("decimal scale exceeds i8".into()))
}

fn millis_to_micros(timestamp: i64) -> BinanceSpotHttpResult<i64> {
    timestamp.checked_mul(1_000).ok_or_else(|| {
        BinanceSpotHttpError::ResponseParseError(format!(
            "timestamp overflows microseconds: {timestamp}"
        ))
    })
}

fn valid_order_list_id(order_list_id: Option<i64>) -> Option<i64> {
    order_list_id.filter(|value| *value >= 0)
}

const fn spot_sbe_order_status(status: BinanceOrderStatus) -> SbeOrderStatus {
    match status {
        BinanceOrderStatus::New => SbeOrderStatus::New,
        BinanceOrderStatus::PendingNew => SbeOrderStatus::PendingNew,
        BinanceOrderStatus::PartiallyFilled => SbeOrderStatus::PartiallyFilled,
        BinanceOrderStatus::Filled => SbeOrderStatus::Filled,
        BinanceOrderStatus::Canceled => SbeOrderStatus::Canceled,
        BinanceOrderStatus::PendingCancel => SbeOrderStatus::PendingCancel,
        BinanceOrderStatus::Rejected => SbeOrderStatus::Rejected,
        BinanceOrderStatus::Expired => SbeOrderStatus::Expired,
        BinanceOrderStatus::ExpiredInMatch => SbeOrderStatus::ExpiredInMatch,
        BinanceOrderStatus::NewInsurance
        | BinanceOrderStatus::NewAdl
        | BinanceOrderStatus::Unknown => SbeOrderStatus::Unknown,
    }
}

const fn spot_sbe_time_in_force(time_in_force: BinanceTimeInForce) -> SbeTimeInForce {
    match time_in_force {
        BinanceTimeInForce::Gtc => SbeTimeInForce::Gtc,
        BinanceTimeInForce::Ioc => SbeTimeInForce::Ioc,
        BinanceTimeInForce::Fok => SbeTimeInForce::Fok,
        BinanceTimeInForce::Gtx
        | BinanceTimeInForce::Gtd
        | BinanceTimeInForce::Rpi
        | BinanceTimeInForce::Unknown => SbeTimeInForce::NonRepresentable,
    }
}

const fn spot_sbe_order_type(order_type: BinanceSpotOrderType) -> SbeOrderType {
    match order_type {
        BinanceSpotOrderType::Market => SbeOrderType::Market,
        BinanceSpotOrderType::Limit => SbeOrderType::Limit,
        BinanceSpotOrderType::StopLoss => SbeOrderType::StopLoss,
        BinanceSpotOrderType::StopLossLimit => SbeOrderType::StopLossLimit,
        BinanceSpotOrderType::TakeProfit => SbeOrderType::TakeProfit,
        BinanceSpotOrderType::TakeProfitLimit => SbeOrderType::TakeProfitLimit,
        BinanceSpotOrderType::LimitMaker => SbeOrderType::LimitMaker,
        BinanceSpotOrderType::Unknown => SbeOrderType::NonRepresentable,
    }
}

const fn spot_sbe_order_side(side: BinanceSide) -> SbeOrderSide {
    match side {
        BinanceSide::Buy => SbeOrderSide::Buy,
        BinanceSide::Sell => SbeOrderSide::Sell,
    }
}

fn spot_sbe_stp(mode: Option<BinanceSelfTradePreventionMode>) -> SbeSelfTradePreventionMode {
    match mode.unwrap_or(BinanceSelfTradePreventionMode::None) {
        BinanceSelfTradePreventionMode::None => SbeSelfTradePreventionMode::None,
        BinanceSelfTradePreventionMode::ExpireMaker => SbeSelfTradePreventionMode::ExpireMaker,
        BinanceSelfTradePreventionMode::ExpireTaker => SbeSelfTradePreventionMode::ExpireTaker,
        BinanceSelfTradePreventionMode::ExpireBoth => SbeSelfTradePreventionMode::ExpireBoth,
        BinanceSelfTradePreventionMode::Decrement => SbeSelfTradePreventionMode::Decrement,
        BinanceSelfTradePreventionMode::Transfer => SbeSelfTradePreventionMode::Transfer,
        BinanceSelfTradePreventionMode::Unknown => SbeSelfTradePreventionMode::NonRepresentable,
    }
}

/// High-level HTTP client for Binance Spot API.
///
/// Wraps [`BinanceRawSpotHttpClient`] and provides domain-level methods:
/// - Simple types (ping, server_time): Pass through from raw client.
/// - Complex types (instruments, orders): Transform to Nautilus domain types.
pub struct BinanceSpotHttpClient {
    inner: Arc<BinanceRawSpotHttpClient>,
    clock: &'static AtomicTime,
    instruments_cache: Arc<AtomicMap<Ustr, InstrumentAny>>,
}

impl Clone for BinanceSpotHttpClient {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            clock: self.clock,
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
    #[expect(clippy::too_many_arguments)]
    pub fn new(
        environment: BinanceEnvironment,
        clock: &'static AtomicTime,
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url_override: Option<String>,
        recv_window: Option<u64>,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
    ) -> BinanceSpotHttpResult<Self> {
        Self::new_with_json_responses(
            environment,
            clock,
            api_key,
            api_secret,
            base_url_override,
            recv_window,
            timeout_secs,
            proxy_url,
            false,
        )
    }

    /// Creates a Spot client for an endpoint that returns JSON REST payloads.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be created.
    #[expect(clippy::too_many_arguments)]
    pub fn new_with_json_responses(
        environment: BinanceEnvironment,
        clock: &'static AtomicTime,
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url_override: Option<String>,
        recv_window: Option<u64>,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
        json_responses: bool,
    ) -> BinanceSpotHttpResult<Self> {
        let inner = BinanceRawSpotHttpClient::new_with_json_responses(
            environment,
            api_key,
            api_secret,
            base_url_override,
            recv_window,
            timeout_secs,
            proxy_url,
            json_responses,
        )?;

        Ok(Self {
            inner: Arc::new(inner),
            clock,
            instruments_cache: Arc::new(AtomicMap::new()),
        })
    }

    /// Returns a reference to the inner raw client.
    #[must_use]
    pub fn inner(&self) -> &BinanceRawSpotHttpClient {
        &self.inner
    }

    /// Returns whether signed requests can be made.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        self.inner.has_credentials()
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
        self.clock.get_time_ns()
    }

    fn command_validation_error(message: impl Into<String>) -> anyhow::Error {
        anyhow::anyhow!(BinanceSpotHttpError::ValidationError(message.into()))
    }

    fn response_parse_error(message: impl Into<String>) -> anyhow::Error {
        anyhow::anyhow!(BinanceSpotHttpError::ResponseParseError(message.into()))
    }

    /// Retrieves an instrument from the cache.
    fn instrument_from_cache(&self, symbol: Ustr) -> anyhow::Result<InstrumentAny> {
        self.instruments_cache
            .get_cloned(&symbol)
            .ok_or_else(|| anyhow::anyhow!("Instrument {symbol} not in cache"))
    }

    /// Caches multiple instruments.
    pub fn cache_instruments(&self, instruments: Vec<InstrumentAny>) {
        self.instruments_cache.rcu(move |cache| {
            for instrument in &instruments {
                cache.insert(instrument.raw_symbol().inner(), instrument.clone());
            }
        });
    }

    /// Replaces the complete instrument cache.
    pub fn replace_instruments(&self, instruments: &[InstrumentAny]) {
        let cache = instruments
            .iter()
            .map(|instrument| (instrument.raw_symbol().inner(), instrument.clone()))
            .collect();
        self.instruments_cache.store(cache);
    }

    /// Caches a single instrument.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        self.instruments_cache
            .insert(instrument.raw_symbol().inner(), instrument);
    }

    /// Gets an instrument from the cache by symbol.
    #[must_use]
    pub fn get_instrument(&self, symbol: &Ustr) -> Option<InstrumentAny> {
        self.instruments_cache.get_cloned(symbol)
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

    /// Returns a fresh status snapshot for Global or US Spot symbols.
    ///
    /// # Errors
    ///
    /// Returns an error if exchange info cannot be requested or decoded.
    pub async fn request_symbol_statuses(
        &self,
        us: bool,
    ) -> BinanceSpotHttpResult<AHashMap<InstrumentId, MarketStatusAction>> {
        let mut statuses = AHashMap::new();

        if us {
            let info = self.inner.exchange_info_json().await?;
            for symbol in info.symbols {
                let instrument_id =
                    InstrumentId::new(Symbol::from(symbol.symbol.as_str()), *BINANCE_VENUE);
                statuses.insert(instrument_id, spot_json_market_status(&symbol.status));
            }
        } else {
            let info = self.exchange_info().await?;
            for symbol in info.symbols {
                let instrument_id =
                    InstrumentId::new(Symbol::from(symbol.symbol.as_str()), *BINANCE_VENUE);
                statuses.insert(
                    instrument_id,
                    MarketStatusAction::from(SymbolStatus::from(symbol.status)),
                );
            }
        }
        Ok(statuses)
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
        self.request_instruments_with_config(&BinanceInstrumentProviderConfig::default(), false)
            .await
    }

    /// Requests configured Nautilus instruments with populated maker and taker fees.
    ///
    /// # Errors
    ///
    /// Returns an error if configuration, exchange info, or required parsing fails.
    pub async fn request_instruments_with_config(
        &self,
        config: &BinanceInstrumentProviderConfig,
        us: bool,
    ) -> BinanceSpotHttpResult<Vec<InstrumentAny>> {
        config
            .validate(BinanceProductType::Spot)
            .map_err(|e| BinanceSpotHttpError::ValidationError(e.to_string()))?;
        let selector = BinanceInstrumentSelector::new(config)
            .map_err(|e| BinanceSpotHttpError::ValidationError(e.to_string()))?;
        let ts_init = self.generate_ts_init();
        let fallback_fees = self.spot_fallback_fees(us).await;

        let mut instruments = if us {
            if config.query_commission_rates {
                if config.log_warnings {
                    log::warn!(
                        "Binance US does not expose the Global account/commission endpoint; using account-wide commission rates"
                    );
                } else {
                    log::debug!(
                        "Binance US exact per-symbol commission query disabled; using account-wide rates"
                    );
                }
            }
            let info = self.inner.exchange_info_json().await?;
            let mut instruments = Vec::with_capacity(info.symbols.len());
            for symbol in &info.symbols {
                let instrument_id =
                    InstrumentId::new(Symbol::from(symbol.symbol.as_str()), *BINANCE_VENUE);

                if !selector.includes(
                    instrument_id,
                    &symbol.symbol,
                    &symbol.base_asset,
                    &symbol.quote_asset,
                    None,
                ) {
                    continue;
                }

                match parse_spot_instrument_json_with_fees(
                    symbol,
                    Some(fallback_fees.0),
                    Some(fallback_fees.1),
                    ts_init,
                    ts_init,
                ) {
                    Ok(instrument) => instruments.push(instrument),
                    Err(e) => log_instrument_parse_error(config, &symbol.symbol, &e),
                }
            }
            instruments
        } else {
            let info = self.exchange_info().await?;
            let mut instruments = Vec::with_capacity(info.symbols.len());
            for symbol in &info.symbols {
                let instrument_id =
                    InstrumentId::new(Symbol::from(symbol.symbol.as_str()), *BINANCE_VENUE);

                if !selector.includes(
                    instrument_id,
                    &symbol.symbol,
                    &symbol.base_asset,
                    &symbol.quote_asset,
                    None,
                ) {
                    continue;
                }

                let fees = self
                    .spot_symbol_fees(config, &symbol.symbol, fallback_fees)
                    .await;

                match parse_spot_instrument_sbe_with_fees(
                    symbol,
                    Some(fees.0),
                    Some(fees.1),
                    ts_init,
                    ts_init,
                ) {
                    Ok(instrument) => instruments.push(instrument),
                    Err(e) => log_instrument_parse_error(config, &symbol.symbol, &e),
                }
            }
            instruments
        };

        instruments.shrink_to_fit();
        self.replace_instruments(&instruments);

        log::debug!("Loaded spot instruments: count={}", instruments.len());
        Ok(instruments)
    }

    async fn spot_fallback_fees(&self, us: bool) -> (Decimal, Decimal) {
        if !self.has_credentials() {
            return (BINANCE_SPOT_FEE_DEFAULT, BINANCE_SPOT_FEE_DEFAULT);
        }

        let result = if us {
            self.inner.account_rates_json().await.map(|account| {
                parse_commission_rates(
                    &account.commission_rates.maker,
                    &account.commission_rates.taker,
                )
            })
        } else {
            self.inner
                .account(&AccountInfoParams::default())
                .await
                .map(|account| {
                    Ok((
                        decimal_from_mantissa_exponent(
                            account.maker_commission_mantissa,
                            account.commission_exponent,
                        ),
                        decimal_from_mantissa_exponent(
                            account.taker_commission_mantissa,
                            account.commission_exponent,
                        ),
                    ))
                })
        };

        match result {
            Ok(Ok(fees)) => fees,
            Ok(Err(e)) => {
                log::warn!("Invalid Binance Spot account commission rates: {e}; using fallback");
                (BINANCE_SPOT_FEE_DEFAULT, BINANCE_SPOT_FEE_DEFAULT)
            }
            Err(e) => {
                log::warn!("Binance Spot account commission query failed: {e}; using fallback");
                (BINANCE_SPOT_FEE_DEFAULT, BINANCE_SPOT_FEE_DEFAULT)
            }
        }
    }

    async fn spot_symbol_fees(
        &self,
        config: &BinanceInstrumentProviderConfig,
        symbol: &str,
        fallback: (Decimal, Decimal),
    ) -> (Decimal, Decimal) {
        if !config.query_commission_rates || !self.has_credentials() {
            return fallback;
        }

        match self.inner.account_commission(symbol).await {
            Ok(response) => match parse_commission_rates(
                &response.standard_commission.maker,
                &response.standard_commission.taker,
            ) {
                Ok(fees) => fees,
                Err(e) => {
                    log::warn!(
                        "Invalid Binance Spot commission response for {symbol}: {e}; using fallback"
                    );
                    fallback
                }
            },
            Err(e) => {
                log::warn!(
                    "Binance Spot commission query failed for {symbol}: {e}; using fallback"
                );
                fallback
            }
        }
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
        let instrument = self.instrument_from_cache_by_id(instrument_id)?;
        let ts_init = self.generate_ts_init();

        let trades = self
            .inner
            .trades(symbol.as_str(), limit)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        parse_spot_trades_sbe(&trades, &instrument, ts_init)
    }

    /// Requests bounded aggregate trades for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails, the instrument is not cached, or parsing fails.
    pub async fn request_agg_trades(
        &self,
        instrument_id: InstrumentId,
        start: Option<Timestamp>,
        end: Option<Timestamp>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<TradeTick>> {
        let symbol = instrument_id.symbol.inner();
        let instrument = self.instrument_from_cache_by_id(instrument_id)?;
        let params = AggTradesParams {
            symbol: symbol.to_string(),
            from_id: None,
            start_time: start.map(|dt| dt.as_millisecond()),
            end_time: end.map(|dt| dt.as_millisecond()),
            limit,
        };
        let response = self.inner.agg_trades(&params).await?;
        let trades = BinanceTrades {
            price_exponent: response.price_exponent,
            qty_exponent: response.qty_exponent,
            trades: response
                .trades
                .into_iter()
                .map(|trade| super::models::BinanceTrade {
                    id: trade.id,
                    price_mantissa: trade.price_mantissa,
                    qty_mantissa: trade.qty_mantissa,
                    quote_qty_mantissa: 0,
                    time: trade.time,
                    is_buyer_maker: trade.is_buyer_maker,
                    is_best_match: trade.is_best_match,
                })
                .collect(),
        };

        let mut parsed = parse_spot_trades_sbe(&trades, &instrument, UnixNanos::default())?;
        for trade in &mut parsed {
            trade.ts_init = trade.ts_event;
        }
        Ok(parsed)
    }

    /// Requests bar (kline/candlestick) data.
    ///
    /// # Errors
    ///
    /// Returns an error if the bar type is not supported, instrument is not cached,
    /// or the request fails.
    pub async fn request_binance_bars(
        &self,
        bar_type: BarType,
        start: Option<Timestamp>,
        end: Option<Timestamp>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<crate::common::bar::BinanceBar>> {
        anyhow::ensure!(
            bar_type.aggregation_source() == AggregationSource::External,
            "Only EXTERNAL aggregation is supported"
        );

        let spec = bar_type.spec();
        let step = spec.step.get();
        let interval = match spec.aggregation {
            BarAggregation::Second if step == 1 => "1s".to_string(),
            BarAggregation::Second => {
                anyhow::bail!("Binance Spot supports only the 1s kline interval")
            }
            BarAggregation::Minute => format!("{step}m"),
            BarAggregation::Hour => format!("{step}h"),
            BarAggregation::Day => format!("{step}d"),
            BarAggregation::Week => format!("{step}w"),
            BarAggregation::Month => format!("{step}M"),
            a => anyhow::bail!("Binance does not support {a:?} aggregation"),
        };

        let instrument_id = bar_type.instrument_id();
        let symbol = instrument_id.symbol;
        let instrument = self.instrument_from_cache_by_id(instrument_id)?;
        let klines = self
            .inner
            .klines(
                symbol.as_str(),
                &interval,
                start.map(|dt| dt.as_millisecond()),
                end.map(|dt| dt.as_millisecond()),
                limit,
            )
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let mut bars =
            parse_klines_to_binance_bars(&klines, bar_type, &instrument, UnixNanos::default())?;
        let now = self.clock.get_time_ns();
        bars.retain(|bar| bar.ts_event < now);
        for bar in &mut bars {
            bar.ts_init = bar.ts_event;
        }
        Ok(bars)
    }

    /// Requests core bars for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the bar type is unsupported or the request fails.
    pub async fn request_bars(
        &self,
        bar_type: BarType,
        start: Option<Timestamp>,
        end: Option<Timestamp>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<Bar>> {
        Ok(self
            .request_binance_bars(bar_type, start, end, limit)
            .await?
            .into_iter()
            .map(|bar| bar.bar())
            .collect())
    }

    /// Requests an explicit L2 order-book snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error for an invalid depth, missing instrument, request failure, or invalid level.
    pub async fn request_book_snapshot(
        &self,
        instrument_id: InstrumentId,
        depth: Option<u32>,
    ) -> anyhow::Result<OrderBook> {
        if depth.is_some_and(|value| value == 0 || value > 5000) {
            anyhow::bail!("Binance Spot order-book depth must be between 1 and 5000");
        }
        let instrument = self.instrument_from_cache_by_id(instrument_id)?;
        let params = DepthParams {
            symbol: instrument_id.symbol.to_string(),
            limit: depth,
        };
        let snapshot = self.inner.depth(&params).await?;
        let ts_event = self.generate_ts_init();
        Self::parse_book_snapshot_response(instrument_id, &instrument, &snapshot, ts_event)
    }

    fn parse_book_snapshot_response(
        instrument_id: InstrumentId,
        instrument: &InstrumentAny,
        snapshot: &BinanceDepth,
        ts_event: UnixNanos,
    ) -> anyhow::Result<OrderBook> {
        let sequence = u64::try_from(snapshot.last_update_id)
            .map_err(|_| anyhow::anyhow!("invalid negative order-book update ID"))?;
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
        let mut add_level = |level: &super::models::BinancePriceLevel,
                             side: OrderSide,
                             order_id: usize,
                             name: &str|
         -> anyhow::Result<()> {
            let price = Price::from_mantissa_exponent_checked(
                level.price_mantissa,
                snapshot.price_exponent,
                instrument.price_precision(),
            )
            .map_err(|e| anyhow::anyhow!("invalid {name} price: {e}"))?;
            anyhow::ensure!(price.is_positive(), "invalid non-positive {name} price");
            let qty_mantissa = u64::try_from(level.qty_mantissa)
                .map_err(|_| anyhow::anyhow!("invalid negative {name} quantity"))?;
            let quantity = Quantity::from_mantissa_exponent_checked(
                qty_mantissa,
                snapshot.qty_exponent,
                instrument.size_precision(),
            )
            .map_err(|e| anyhow::anyhow!("invalid {name} quantity: {e}"))?;
            anyhow::ensure!(
                quantity.is_positive(),
                "invalid non-positive {name} quantity"
            );
            let order = BookOrder::new(
                side,
                price,
                quantity,
                u64::try_from(order_id)
                    .map_err(|_| anyhow::anyhow!("order-book level index overflow"))?,
            );
            book.add(order, 0, sequence, ts_event);
            Ok(())
        };

        for (index, level) in snapshot.bids.iter().enumerate() {
            add_level(level, OrderSide::Buy, index, "bid")?;
        }
        let bid_count = snapshot.bids.len();
        for (index, level) in snapshot.asks.iter().enumerate() {
            let order_id = bid_count
                .checked_add(index)
                .ok_or_else(|| anyhow::anyhow!("order-book level index overflow"))?;
            add_level(level, OrderSide::Sell, order_id, "ask")?;
        }
        Ok(book)
    }

    fn instrument_from_cache_by_id(
        &self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<InstrumentAny> {
        self.instruments_cache
            .get_cloned(&instrument_id.symbol.inner())
            .ok_or_else(|| InstrumentLookupError::not_found(instrument_id).into())
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
        let ts_init = self.clock.get_time_ns();
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
    /// Returns an error if neither identifier is provided, the request fails for any
    /// reason other than a missing order, instrument is not cached, or parsing fails.
    pub async fn request_order_status_report(
        &self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        venue_order_id: Option<VenueOrderId>,
        client_order_id: Option<ClientOrderId>,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
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

        let client_id_str =
            client_order_id.map(|id| encode_broker_id(&id, BINANCE_NAUTILUS_SPOT_BROKER_ID));

        let order = match self
            .inner
            .query_order(symbol.as_str(), order_id, client_id_str.as_deref())
            .await
        {
            Ok(order) => order,
            Err(e) if Self::is_no_such_order_error(&e) => {
                log::debug!("Binance Spot order not found: instrument_id={instrument_id}");
                return Ok(None);
            }
            Err(e) => anyhow::bail!(e),
        };

        parse_order_status_report_sbe(
            &order,
            account_id,
            &instrument,
            BINANCE_NAUTILUS_SPOT_BROKER_ID,
            ts_init,
        )
        .map(Some)
    }

    const fn is_no_such_order_error(error: &BinanceSpotHttpError) -> bool {
        matches!(
            error,
            BinanceSpotHttpError::BinanceError { code, .. } if *code == BINANCE_NO_SUCH_ORDER_CODE
        )
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
    pub async fn request_order_status_reports(
        &self,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
        start: Option<Timestamp>,
        end: Option<Timestamp>,
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
                    start.map(|dt| dt.as_millisecond()),
                    end.map(|dt| dt.as_millisecond()),
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
                parse_order_status_report_sbe(
                    order,
                    account_id,
                    &instrument,
                    BINANCE_NAUTILUS_SPOT_BROKER_ID,
                    ts_init,
                )
            })
            .collect()
    }

    /// Requests fill reports (trade history) for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails, any trade's instrument is not cached,
    /// or parsing fails.
    pub async fn request_fill_reports(
        &self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        venue_order_id: Option<VenueOrderId>,
        start: Option<Timestamp>,
        end: Option<Timestamp>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<FillReport>> {
        self.request_fill_reports_with_cursor(
            account_id,
            instrument_id,
            venue_order_id,
            start,
            end,
            None,
            limit,
        )
        .await
    }

    #[expect(clippy::too_many_arguments)]
    pub(crate) async fn request_fill_reports_with_cursor(
        &self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        venue_order_id: Option<VenueOrderId>,
        start: Option<Timestamp>,
        end: Option<Timestamp>,
        from_id: Option<i64>,
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
            .account_trades_with_cursor(
                symbol.as_str(),
                order_id,
                start.map(|dt| dt.as_millisecond()),
                end.map(|dt| dt.as_millisecond()),
                from_id,
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
    #[expect(clippy::too_many_arguments)]
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
        quote_quantity: bool,
        display_qty: Option<Quantity>,
        use_gtd: bool,
    ) -> anyhow::Result<OrderStatusReport> {
        let symbol = instrument_id.symbol.inner();
        let instrument = self
            .instrument_from_cache(symbol)
            .map_err(|e| Self::command_validation_error(e.to_string()))?;
        let ts_init = self.generate_ts_init();

        let binance_side = BinanceSide::try_from(order_side)
            .map_err(|e| Self::command_validation_error(e.to_string()))?;
        let binance_order_type = order_type_to_binance_spot(order_type, post_only)
            .map_err(|e| Self::command_validation_error(e.to_string()))?;

        // Validate trigger price for conditional orders
        let requires_trigger = matches!(
            order_type,
            OrderType::StopMarket
                | OrderType::StopLimit
                | OrderType::MarketIfTouched
                | OrderType::LimitIfTouched
        );

        if requires_trigger && trigger_price.is_none() {
            return Err(Self::command_validation_error(
                "Conditional orders require a trigger price",
            ));
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
            return Err(Self::command_validation_error(format!(
                "{binance_order_type:?} orders require a price"
            )));
        }

        // Only send TIF for order types that support it
        let supports_tif = matches!(
            binance_order_type,
            BinanceSpotOrderType::Limit
                | BinanceSpotOrderType::StopLossLimit
                | BinanceSpotOrderType::TakeProfitLimit
        );
        let binance_tif = if supports_tif {
            Some(
                time_in_force_to_binance_spot(time_in_force, use_gtd)
                    .map_err(|e| Self::command_validation_error(e.to_string()))?,
            )
        } else {
            None
        };

        let qty_str = quantity.to_string();
        let price_str = price.map(|p| p.to_string());
        let stop_price_str = trigger_price.map(|p| p.to_string());
        let iceberg_qty_str = display_qty.map(|q| q.to_string());
        let client_id_str = encode_broker_id(&client_order_id, BINANCE_NAUTILUS_SPOT_BROKER_ID);

        if quote_quantity && binance_order_type != BinanceSpotOrderType::Market {
            return Err(Self::command_validation_error(
                "quoteOrderQty is only supported for MARKET orders",
            ));
        }

        let (base_qty, quote_qty) = if quote_quantity {
            (None, Some(qty_str.as_str()))
        } else {
            (Some(qty_str.as_str()), None)
        };

        let response = self
            .inner
            .new_order_full(
                symbol.as_str(),
                binance_side,
                binance_order_type,
                binance_tif,
                base_qty,
                quote_qty,
                price_str.as_deref(),
                Some(&client_id_str),
                stop_price_str.as_deref(),
                iceberg_qty_str.as_deref(),
            )
            .await?;

        parse_new_order_response_sbe(
            &response,
            account_id,
            &instrument,
            BINANCE_NAUTILUS_SPOT_BROKER_ID,
            ts_init,
        )
        .map_err(|e| Self::response_parse_error(e.to_string()))
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

    /// Submits a Spot OCO order list.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or JSON parsing fails.
    pub async fn submit_oco_order_list(
        &self,
        params: &NewOcoOrderListParams,
    ) -> BinanceSpotHttpResult<NewOcoOrderListResponse> {
        self.inner.new_oco_order_list(params).await
    }

    /// Modifies an existing order (cancel and replace atomically).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The instrument is not cached.
    /// - The order type or time-in-force is unsupported.
    /// - The request fails or SBE decoding fails.
    #[expect(clippy::too_many_arguments)]
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
        use_gtd: bool,
    ) -> anyhow::Result<OrderStatusReport> {
        let symbol = instrument_id.symbol.inner();
        let instrument = self
            .instrument_from_cache(symbol)
            .map_err(|e| Self::command_validation_error(e.to_string()))?;
        let ts_init = self.generate_ts_init();

        let binance_side = BinanceSide::try_from(order_side)
            .map_err(|e| Self::command_validation_error(e.to_string()))?;
        let binance_order_type = order_type_to_binance_spot(order_type, false)
            .map_err(|e| Self::command_validation_error(e.to_string()))?;
        let binance_tif = time_in_force_to_binance_spot(time_in_force, use_gtd)
            .map_err(|e| Self::command_validation_error(e.to_string()))?;

        let cancel_order_id: i64 = venue_order_id.inner().parse().map_err(|_| {
            Self::command_validation_error(format!("Invalid venue order ID: {venue_order_id}"))
        })?;

        let qty_str = quantity.to_string();
        let price_str = price.map(|p| p.to_string());
        let client_id_str = encode_broker_id(&client_order_id, BINANCE_NAUTILUS_SPOT_BROKER_ID);

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

        parse_new_order_response_sbe(
            &response,
            account_id,
            &instrument,
            BINANCE_NAUTILUS_SPOT_BROKER_ID,
            ts_init,
        )
        .map_err(|e| Self::response_parse_error(e.to_string()))
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

        let order_id = match venue_order_id {
            Some(venue_order_id) => match venue_order_id.inner().parse::<i64>() {
                Ok(order_id) => Some(order_id),
                Err(e) if client_order_id.is_some() => {
                    log::warn!(
                        "Unable to parse venue_order_id {venue_order_id} for cancel, canceling by client_order_id: {e}"
                    );
                    None
                }
                Err(e) => {
                    return Err(Self::command_validation_error(format!(
                        "Invalid venue order ID: {e}"
                    )));
                }
            },
            None => None,
        };

        let client_id_str =
            client_order_id.map(|id| encode_broker_id(&id, BINANCE_NAUTILUS_SPOT_BROKER_ID));

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

        responses
            .into_iter()
            .map(|r| {
                Ok((
                    VenueOrderId::new(r.order_id.to_string()),
                    decode_client_order_id(
                        &r.orig_client_order_id,
                        BINANCE_NAUTILUS_SPOT_BROKER_ID,
                    )?,
                ))
            })
            .collect()
    }
}

fn parse_commission_rates(maker: &str, taker: &str) -> anyhow::Result<(Decimal, Decimal)> {
    Ok((
        Decimal::from_str_exact(maker)?,
        Decimal::from_str_exact(taker)?,
    ))
}

fn spot_json_market_status(status: &str) -> MarketStatusAction {
    match status {
        "TRADING" => MarketStatusAction::Trading,
        "BREAK" => MarketStatusAction::Pause,
        _ => MarketStatusAction::NotAvailableForTrading,
    }
}

fn decimal_from_mantissa_exponent(mantissa: i64, exponent: i8) -> Decimal {
    if exponent >= 0 {
        Decimal::from(mantissa) * Decimal::from(10_i64.pow(exponent as u32))
    } else {
        Decimal::new(mantissa, (-exponent) as u32)
    }
}

fn log_instrument_parse_error(
    config: &BinanceInstrumentProviderConfig,
    symbol: &str,
    error: &anyhow::Error,
) {
    if config.log_warnings {
        log::warn!("Skipping Binance Spot instrument {symbol}: {error}");
    } else {
        log::debug!("Skipping Binance Spot instrument {symbol}: {error}");
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::instruments::stubs::currency_pair_btcusdt;
    use rstest::rstest;

    use super::*;
    use crate::spot::http::models::BinancePriceLevel;

    #[rstest]
    fn test_schema_constants() {
        assert_eq!(BinanceRawSpotHttpClient::schema_id(), 3);
        assert_eq!(BinanceRawSpotHttpClient::schema_version(), 5);
        assert_eq!(BinanceSpotHttpClient::schema_id(), 3);
        assert_eq!(BinanceSpotHttpClient::schema_version(), 5);
    }

    #[rstest]
    fn test_sbe_schema_header() {
        assert_eq!(SBE_SCHEMA_HEADER, "3:5");
    }

    #[rstest]
    fn test_parse_book_snapshot_response_rejects_negative_update_id() {
        let instrument = InstrumentAny::CurrencyPair(currency_pair_btcusdt());
        let snapshot = BinanceDepth {
            last_update_id: -1,
            price_exponent: -2,
            qty_exponent: -5,
            bids: vec![],
            asks: vec![],
        };

        let error = BinanceSpotHttpClient::parse_book_snapshot_response(
            InstrumentId::from("BTCUSDT.BINANCE"),
            &instrument,
            &snapshot,
            UnixNanos::from(1_700_000_000_000_000_001u64),
        )
        .unwrap_err();

        assert_eq!(error.to_string(), "invalid negative order-book update ID");
    }

    #[rstest]
    fn test_parse_book_snapshot_response_rejects_negative_quantity() {
        let instrument = InstrumentAny::CurrencyPair(currency_pair_btcusdt());
        let snapshot = BinanceDepth {
            last_update_id: 12345,
            price_exponent: -2,
            qty_exponent: -5,
            bids: vec![BinancePriceLevel {
                price_mantissa: 4_200_001,
                qty_mantissa: -1,
            }],
            asks: vec![],
        };

        let error = BinanceSpotHttpClient::parse_book_snapshot_response(
            InstrumentId::from("BTCUSDT.BINANCE"),
            &instrument,
            &snapshot,
            UnixNanos::from(1_700_000_000_000_000_001u64),
        )
        .unwrap_err();

        assert_eq!(error.to_string(), "invalid negative bid quantity");
    }

    #[rstest]
    fn test_default_headers_include_sbe() {
        let headers = BinanceRawSpotHttpClient::default_headers(&None, false);

        assert_eq!(headers.get("Accept"), Some(&"application/sbe".to_string()));
        assert_eq!(headers.get("X-MBX-SBE"), Some(&"3:5".to_string()));
    }

    #[rstest]
    fn test_json_headers_exclude_sbe() {
        let headers = BinanceRawSpotHttpClient::default_headers(&None, true);

        assert_eq!(headers.get("Accept"), Some(&"application/json".to_string()));
        assert_eq!(headers.get("X-MBX-SBE"), None);
    }

    #[rstest]
    fn test_spot_trades_from_json_preserves_all_fields() {
        let response: Vec<SpotTradeJson> = serde_json::from_value(serde_json::json!([{
            "id": 17,
            "price": "123.45",
            "qty": "0.06789",
            "quoteQty": "8.3810205",
            "time": 1_700_000_000_123_i64,
            "isBuyerMaker": true,
            "isBestMatch": false
        }]))
        .unwrap();

        let parsed = spot_trades_from_json(response).unwrap();

        assert_eq!(parsed.price_exponent, -7);
        assert_eq!(parsed.qty_exponent, -5);
        assert_eq!(parsed.trades.len(), 1);
        assert_eq!(parsed.trades[0].id, 17);
        assert_eq!(parsed.trades[0].price_mantissa, 1_234_500_000);
        assert_eq!(parsed.trades[0].qty_mantissa, 6_789);
        assert_eq!(parsed.trades[0].quote_qty_mantissa, 83_810_205);
        assert_eq!(parsed.trades[0].time, 1_700_000_000_123_000);
        assert!(parsed.trades[0].is_buyer_maker);
        assert!(!parsed.trades[0].is_best_match);
    }

    #[rstest]
    fn test_spot_agg_trades_from_json_preserves_all_fields() {
        let response: Vec<SpotAggTradeJson> = serde_json::from_value(serde_json::json!([{
            "a": 21,
            "p": "432.10",
            "q": "1.234",
            "f": 31,
            "l": 32,
            "T": 1_700_000_000_456_i64,
            "m": false,
            "M": true
        }]))
        .unwrap();

        let parsed = spot_agg_trades_from_json(response).unwrap();

        assert_eq!(parsed.price_exponent, -2);
        assert_eq!(parsed.qty_exponent, -3);
        assert_eq!(parsed.trades.len(), 1);
        assert_eq!(parsed.trades[0].id, 21);
        assert_eq!(parsed.trades[0].price_mantissa, 43_210);
        assert_eq!(parsed.trades[0].qty_mantissa, 1_234);
        assert_eq!(parsed.trades[0].first_trade_id, 31);
        assert_eq!(parsed.trades[0].last_trade_id, 32);
        assert_eq!(parsed.trades[0].time, 1_700_000_000_456_000);
        assert!(!parsed.trades[0].is_buyer_maker);
        assert!(parsed.trades[0].is_best_match);
    }

    #[rstest]
    fn test_spot_klines_from_json_preserves_all_fields() {
        let response: Vec<SpotKlineJson> = serde_json::from_value(serde_json::json!([[
            1_700_000_000_000_i64,
            "10.10",
            "11.20",
            "9.30",
            "10.40",
            "12.345",
            1_700_000_059_999_i64,
            "128.765",
            37,
            "5.432",
            "56.789",
            "0"
        ]]))
        .unwrap();

        let parsed = spot_klines_from_json(response).unwrap();
        let kline = &parsed.klines[0];

        assert_eq!(parsed.price_exponent, -3);
        assert_eq!(parsed.qty_exponent, -3);
        assert_eq!(parsed.klines.len(), 1);
        assert_eq!(kline.open_time, 1_700_000_000_000_000);
        assert_eq!(kline.open_price, 10_100);
        assert_eq!(kline.high_price, 11_200);
        assert_eq!(kline.low_price, 9_300);
        assert_eq!(kline.close_price, 10_400);
        assert_eq!(i128::from_le_bytes(kline.volume), 12_345);
        assert_eq!(kline.close_time, 1_700_000_059_999_000);
        assert_eq!(i128::from_le_bytes(kline.quote_volume), 128_765);
        assert_eq!(kline.num_trades, 37);
        assert_eq!(i128::from_le_bytes(kline.taker_buy_base_volume), 5_432);
        assert_eq!(i128::from_le_bytes(kline.taker_buy_quote_volume), 56_789);
    }

    #[rstest]
    fn test_spot_account_from_json_preserves_all_fields() {
        let response: SpotAccountJson = serde_json::from_value(serde_json::json!({
            "commissionRates": {
                "maker": "0.0008",
                "taker": "0.0011",
                "buyer": "0.0002",
                "seller": "0.0003"
            },
            "canTrade": true,
            "canWithdraw": false,
            "canDeposit": true,
            "requireSelfTradePrevention": true,
            "preventSor": false,
            "updateTime": 1_700_000_000_789_i64,
            "accountType": "SPOT",
            "balances": [{"asset": "USD", "free": "123.45", "locked": "6.789"}]
        }))
        .unwrap();

        let account = spot_account_from_json(response).unwrap();

        assert_eq!(account.commission_exponent, -4);
        assert_eq!(account.maker_commission_mantissa, 8);
        assert_eq!(account.taker_commission_mantissa, 11);
        assert_eq!(account.buyer_commission_mantissa, 2);
        assert_eq!(account.seller_commission_mantissa, 3);
        assert!(account.can_trade);
        assert!(!account.can_withdraw);
        assert!(account.can_deposit);
        assert!(account.require_self_trade_prevention);
        assert!(!account.prevent_sor);
        assert_eq!(account.update_time, 1_700_000_000_789_000);
        assert_eq!(account.account_type, "SPOT");
        assert_eq!(account.balances.len(), 1);
        assert_eq!(account.balances[0].asset, "USD");
        assert_eq!(account.balances[0].free_mantissa, 123_450);
        assert_eq!(account.balances[0].locked_mantissa, 6_789);
        assert_eq!(account.balances[0].exponent, -3);
    }

    #[rstest]
    fn test_spot_cancel_replace_json_uses_nested_new_order_response() {
        let response: SpotCancelReplaceJson = serde_json::from_value(serde_json::json!({
            "cancelResult": "SUCCESS",
            "newOrderResult": "SUCCESS",
            "cancelResponse": {},
            "newOrderResponse": {
                "symbol": "ETHUSD",
                "orderId": 101,
                "orderListId": -1,
                "clientOrderId": "new-order",
                "transactTime": 1_700_000_000_123_i64,
                "price": "12.34",
                "origQty": "5.678",
                "executedQty": "1.234",
                "cummulativeQuoteQty": "15.22756",
                "status": "PARTIALLY_FILLED",
                "timeInForce": "GTC",
                "type": "LIMIT",
                "side": "BUY",
                "stopPrice": "11.11",
                "workingTime": 1_700_000_000_124_i64,
                "selfTradePreventionMode": "EXPIRE_MAKER",
                "fills": [{
                    "price": "12.34",
                    "qty": "1.234",
                    "commission": "0.001234",
                    "commissionAsset": "USD",
                    "tradeId": 44
                }]
            }
        }))
        .unwrap();

        let order = spot_new_order_from_json(response.new_order_response).unwrap();

        assert_eq!(order.price_exponent, -2);
        assert_eq!(order.qty_exponent, -3);
        assert_eq!(order.order_id, 101);
        assert_eq!(order.order_list_id, None);
        assert_eq!(order.transact_time, 1_700_000_000_123_000);
        assert_eq!(order.price_mantissa, 1_234);
        assert_eq!(order.orig_qty_mantissa, 5_678);
        assert_eq!(order.executed_qty_mantissa, 1_234);
        assert_eq!(order.cummulative_quote_qty_mantissa, 1_522_756);
        assert_eq!(order.status, SbeOrderStatus::PartiallyFilled);
        assert_eq!(order.time_in_force, SbeTimeInForce::Gtc);
        assert_eq!(order.order_type, SbeOrderType::Limit);
        assert_eq!(order.side, SbeOrderSide::Buy);
        assert_eq!(order.stop_price_mantissa, Some(1_111));
        assert_eq!(order.working_time, Some(1_700_000_000_124_000));
        assert_eq!(
            order.self_trade_prevention_mode,
            SbeSelfTradePreventionMode::ExpireMaker
        );
        assert_eq!(order.client_order_id, "new-order");
        assert_eq!(order.symbol, "ETHUSD");
        assert_eq!(order.fills.len(), 1);
        assert_eq!(order.fills[0].price_mantissa, 1_234);
        assert_eq!(order.fills[0].qty_mantissa, 1_234);
        assert_eq!(order.fills[0].commission_mantissa, 1_234);
        assert_eq!(order.fills[0].commission_exponent, -6);
        assert_eq!(order.fills[0].commission_asset, "USD");
        assert_eq!(order.fills[0].trade_id, Some(44));
        assert_eq!(order.expiry_reason, None);
    }

    #[rstest]
    fn test_rate_limit_config() {
        let config = BinanceRawSpotHttpClient::rate_limit_config();

        assert!(config.default_quota.is_some());
        // Spot has 2 ORDERS quotas (SECOND and DAY)
        assert_eq!(config.order_keys.len(), 2);
    }

    #[rstest]
    fn test_quota_from_unknown_interval_returns_none() {
        let quota = BinanceRateLimitQuota {
            rate_limit_type: BinanceRateLimitType::Orders,
            interval: BinanceRateLimitInterval::Unknown,
            interval_num: 1,
            limit: 10,
        };

        assert!(BinanceRawSpotHttpClient::quota_from(&quota).is_none());
    }

    fn create_test_raw_client() -> BinanceRawSpotHttpClient {
        BinanceRawSpotHttpClient::new(
            BinanceEnvironment::Live,
            None,
            None,
            Some("http://127.0.0.1:1".to_string()),
            None,
            Some(1),
            None,
        )
        .unwrap()
    }

    #[rstest]
    #[case::limit(
        AggTradesParams {
            symbol: "BTCUSDT".to_string(),
            from_id: None,
            start_time: None,
            end_time: None,
            limit: Some(1001),
        },
        "Validation error: aggregate trade limit must not exceed 1000"
    )]
    #[case::bounds(
        AggTradesParams {
            symbol: "BTCUSDT".to_string(),
            from_id: None,
            start_time: Some(2000),
            end_time: Some(1000),
            limit: Some(1000),
        },
        "Validation error: aggregate trade startTime must not exceed endTime"
    )]
    #[tokio::test]
    async fn test_agg_trades_rejects_invalid_bounds(
        #[case] params: AggTradesParams,
        #[case] expected: &str,
    ) {
        let error = create_test_raw_client()
            .agg_trades(&params)
            .await
            .unwrap_err();

        assert_eq!(error.to_string(), expected);
    }
}
