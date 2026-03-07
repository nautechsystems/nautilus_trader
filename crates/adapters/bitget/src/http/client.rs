// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fmt,
    num::NonZeroU32,
    sync::LazyLock,
};

use anyhow::{anyhow, bail, Result};
use nautilus_core::{
    consts::NAUTILUS_USER_AGENT,
    time::{get_atomic_clock_realtime, AtomicTime},
    UnixNanos,
};
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas},
    enums::{BookAction, OrderSide, RecordFlag},
    identifiers::{InstrumentId, Symbol, Venue},
    instruments::{CryptoFuture, CryptoPerpetual, CurrencyPair, Instrument, InstrumentAny},
    types::{Currency, Price, Quantity},
};
#[cfg(feature = "python")]
use nautilus_model::{
    data::{Data, OrderBookDeltas_API},
    python::{data::data_to_pycapsule, instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any}},
};
use nautilus_network::{
    http::{HttpClient, USER_AGENT},
    ratelimiter::quota::Quota,
};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::{
    common::{
        enums::{BitgetEnvironment, BitgetProductType},
        signing::rest_sign_base64,
        symbol::{nautilus_symbol_for_delivery, nautilus_symbol_for_perp, nautilus_symbol_for_spot},
        urls::get_http_base_url,
    },
    http::models::{
        BitgetApiResponse, BitgetContractSymbol, BitgetCurrentFundingRate,
        BitgetFillInfo, BitgetFundingHistoryPage, BitgetHistoricalFundingRate,
        BitgetMixFillsPage, BitgetMixOrdersPage, BitgetOrderBookSnapshot, BitgetOrderInfo,
        BitgetPositionInfo, BitgetSpotSymbol,
    },
};

const BITGET_SPOT_SYMBOLS_PATH: &str = "/api/v2/spot/public/symbols";
const BITGET_CONTRACT_CONFIG_PATH: &str = "/api/v2/mix/market/contracts";
const BITGET_SPOT_CANDLES_PATH: &str = "/api/v2/spot/market/candles";
const BITGET_MIX_CANDLES_PATH: &str = "/api/v2/mix/market/candles";
const BITGET_MIX_CURRENT_FUNDING_RATE_PATH: &str = "/api/v2/mix/market/current-fund-rate";
const BITGET_V3_HISTORY_FUNDING_RATE_PATH: &str = "/api/v3/market/history-fund-rate";
const BITGET_SPOT_MERGE_DEPTH_PATH: &str = "/api/v2/spot/market/merge-depth";
const BITGET_MIX_MERGE_DEPTH_PATH: &str = "/api/v2/mix/market/merge-depth";
const BITGET_SPOT_PLACE_ORDER_PATH: &str = "/api/v2/spot/trade/place-order";
const BITGET_SPOT_CANCEL_ORDER_PATH: &str = "/api/v2/spot/trade/cancel-order";
const BITGET_SPOT_CANCEL_SYMBOL_ORDER_PATH: &str = "/api/v2/spot/trade/cancel-symbol-order";
const BITGET_SPOT_BATCH_CANCEL_ORDER_PATH: &str = "/api/v2/spot/trade/batch-cancel-order";
const BITGET_SPOT_CANCEL_REPLACE_ORDER_PATH: &str = "/api/v2/spot/trade/cancel-replace-order";
const BITGET_SPOT_ORDER_INFO_PATH: &str = "/api/v2/spot/trade/orderInfo";
const BITGET_SPOT_UNFILLED_ORDERS_PATH: &str = "/api/v2/spot/trade/unfilled-orders";
const BITGET_SPOT_HISTORY_ORDERS_PATH: &str = "/api/v2/spot/trade/history-orders";
const BITGET_SPOT_FILLS_PATH: &str = "/api/v2/spot/trade/fills";
const BITGET_MIX_PLACE_ORDER_PATH: &str = "/api/v2/mix/order/place-order";
const BITGET_MIX_CANCEL_ORDER_PATH: &str = "/api/v2/mix/order/cancel-order";
const BITGET_MIX_CANCEL_ALL_ORDERS_PATH: &str = "/api/v2/mix/order/cancel-all-orders";
const BITGET_MIX_BATCH_CANCEL_ORDERS_PATH: &str = "/api/v2/mix/order/batch-cancel-orders";
const BITGET_MIX_MODIFY_ORDER_PATH: &str = "/api/v2/mix/order/modify-order";
const BITGET_MIX_ORDER_DETAIL_PATH: &str = "/api/v2/mix/order/detail";
const BITGET_MIX_ORDERS_PENDING_PATH: &str = "/api/v2/mix/order/orders-pending";
const BITGET_MIX_ORDERS_HISTORY_PATH: &str = "/api/v2/mix/order/orders-history";
const BITGET_MIX_FILL_HISTORY_PATH: &str = "/api/v2/mix/order/fill-history";
const BITGET_MIX_ALL_POSITION_PATH: &str = "/api/v2/mix/position/all-position";
const BITGET_GLOBAL_RATE_KEY: &str = "bitget:global";
const BITGET_DEMO_REST_HEADER: &str = "paptrading";
const BITGET_DEMO_REST_HEADER_VALUE: &str = "1";
const BITGET_ACCESS_KEY_HEADER: &str = "ACCESS-KEY";
const BITGET_ACCESS_SIGN_HEADER: &str = "ACCESS-SIGN";
const BITGET_ACCESS_TIMESTAMP_HEADER: &str = "ACCESS-TIMESTAMP";
const BITGET_ACCESS_PASSPHRASE_HEADER: &str = "ACCESS-PASSPHRASE";
const BITGET_LOCALE_HEADER: &str = "locale";
const BITGET_LOCALE_HEADER_VALUE: &str = "en-US";

pub static BITGET_REST_QUOTA: LazyLock<Quota> =
    LazyLock::new(|| {
        Quota::per_second(NonZeroU32::new(20).expect("non-zero")).expect("Bitget quota should be valid")
    });

fn is_success_code(code: &str) -> bool {
    matches!(code, "00000" | "0")
}

fn parse_bitget_response<T: DeserializeOwned>(body: &[u8]) -> Result<T> {
    let payload = serde_json::from_slice::<BitgetApiResponse<Value>>(body)
        .map_err(|e| anyhow!("Failed to deserialize Bitget response: {e}"))?;

    if !is_success_code(&payload.code) {
        bail!("Bitget API returned error {}: {}", payload.code, payload.msg);
    }

    serde_json::from_value(payload.data).map_err(|e| {
        anyhow!("Failed to deserialize Bitget response data payload: {e}")
    })
}

/// Minimal async Bitget HTTP client for public endpoints.
#[derive(Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bitget")
)]
pub struct BitgetHttpClient {
    pub environment: BitgetEnvironment,
    pub base_url: String,
    pub api_key: Option<String>,
    pub api_secret: Option<String>,
    pub api_passphrase: Option<String>,
    client: HttpClient,
    clock: &'static AtomicTime,
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    subscribed_instruments: HashSet<String>,
}

impl fmt::Debug for BitgetHttpClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BitgetHttpClient")
            .field("environment", &self.environment)
            .field("base_url", &self.base_url)
            .field("api_key_set", &self.api_key.is_some())
            .field("api_secret_set", &self.api_secret.is_some())
            .field("api_passphrase_set", &self.api_passphrase.is_some())
            .field("subscribed_instruments", &self.subscribed_instruments)
            .finish()
    }
}

impl BitgetHttpClient {
    #[must_use]
    pub fn new(environment: BitgetEnvironment) -> Self {
        let base_url = get_http_base_url(environment).to_string();

        Self::with_client(environment, base_url, None, None, None)
    }

    #[must_use]
    pub fn with_credentials(
        environment: BitgetEnvironment,
        api_key: String,
        api_secret: String,
        api_passphrase: String,
    ) -> Self {
        let base_url = get_http_base_url(environment).to_string();
        Self::with_client(
            environment,
            base_url,
            Some(api_key),
            Some(api_secret),
            Some(api_passphrase),
        )
    }

    fn with_client(
        environment: BitgetEnvironment,
        base_url: String,
        api_key: Option<String>,
        api_secret: Option<String>,
        api_passphrase: Option<String>,
    ) -> Self {
        let base_url = base_url.trim_end_matches('/').to_string();

        let client = HttpClient::new(
            Self::default_headers(environment),
            vec![],
            Self::rate_limiter_quotas(),
            Some(*BITGET_REST_QUOTA),
            Some(60),
            None,
        )
        .expect("HTTP client should be created");

        Self {
            environment,
            base_url,
            api_key,
            api_secret,
            api_passphrase,
            client,
            clock: get_atomic_clock_realtime(),
            subscribed_instruments: HashSet::new(),
        }
    }

    fn default_headers(environment: BitgetEnvironment) -> HashMap<String, String> {
        let mut headers = HashMap::from([
            (USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string()),
            ("Content-Type".to_string(), "application/json".to_string()),
        ]);

        if matches!(environment, BitgetEnvironment::Demo) {
            headers.insert(
                BITGET_DEMO_REST_HEADER.to_string(),
                BITGET_DEMO_REST_HEADER_VALUE.to_string(),
            );
        }

        headers
    }

    fn rate_limiter_quotas() -> Vec<(String, Quota)> {
        vec![
            (BITGET_GLOBAL_RATE_KEY.to_string(), *BITGET_REST_QUOTA),
            (format!("bitget:{BITGET_SPOT_SYMBOLS_PATH}"), *BITGET_REST_QUOTA),
            (
                format!("bitget:{BITGET_CONTRACT_CONFIG_PATH}"),
                *BITGET_REST_QUOTA,
            ),
            (format!("bitget:{BITGET_SPOT_MERGE_DEPTH_PATH}"), *BITGET_REST_QUOTA),
            (format!("bitget:{BITGET_MIX_MERGE_DEPTH_PATH}"), *BITGET_REST_QUOTA),
            (format!("bitget:{BITGET_SPOT_CANDLES_PATH}"), *BITGET_REST_QUOTA),
            (format!("bitget:{BITGET_MIX_CANDLES_PATH}"), *BITGET_REST_QUOTA),
            (format!("bitget:{BITGET_MIX_CURRENT_FUNDING_RATE_PATH}"), *BITGET_REST_QUOTA),
            (format!("bitget:{BITGET_V3_HISTORY_FUNDING_RATE_PATH}"), *BITGET_REST_QUOTA),
        ]
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn rate_limit_keys(path: &str) -> Vec<String> {
        let route = path.split('?').next().unwrap_or(path);
        vec![
            BITGET_GLOBAL_RATE_KEY.to_string(),
            format!("bitget:{route}"),
        ]
    }

    async fn request<T: DeserializeOwned>(
        &self,
        path: &str,
        query: Option<&HashMap<String, Vec<String>>>,
    ) -> Result<T> {
        let response = self
            .client
            .get(
                self.url(path),
                query,
                None,
                Some(5),
                Some(Self::rate_limit_keys(path)),
            )
            .await
            .map_err(|e| anyhow!("HTTP request failed: {e}"))?;

        if !response.status.is_success() {
            bail!("HTTP request failed with status {}", response.status.as_u16());
        }

        parse_bitget_response::<T>(&response.body)
    }

    fn credentials(&self) -> Result<(&str, &str, &str)> {
        let api_key = self
            .api_key
            .as_deref()
            .ok_or_else(|| anyhow!("Bitget API key is required for authenticated requests"))?;
        let api_secret = self
            .api_secret
            .as_deref()
            .ok_or_else(|| anyhow!("Bitget API secret is required for authenticated requests"))?;
        let api_passphrase = self
            .api_passphrase
            .as_deref()
            .ok_or_else(|| anyhow!("Bitget API passphrase is required for authenticated requests"))?;
        Ok((api_key, api_secret, api_passphrase))
    }

    fn build_query_string(query: &BTreeMap<String, Vec<String>>) -> String {
        query
            .iter()
            .flat_map(|(key, values)| {
                values.iter().map(move |value| format!("{key}={value}"))
            })
            .collect::<Vec<_>>()
            .join("&")
    }

    fn authenticated_headers(
        &self,
        method: &str,
        path: &str,
        query_string: Option<&str>,
        body: Option<&[u8]>,
    ) -> Result<HashMap<String, String>> {
        let (api_key, api_secret, api_passphrase) = self.credentials()?;
        let timestamp_ms = self.clock.get_time_ms() as i64;
        let signature = rest_sign_base64(
            api_secret,
            timestamp_ms,
            method,
            path,
            query_string,
            body,
        );

        let mut headers = Self::default_headers(self.environment);
        headers.insert(BITGET_ACCESS_KEY_HEADER.to_string(), api_key.to_string());
        headers.insert(
            BITGET_ACCESS_SIGN_HEADER.to_string(),
            signature,
        );
        headers.insert(
            BITGET_ACCESS_TIMESTAMP_HEADER.to_string(),
            timestamp_ms.to_string(),
        );
        headers.insert(
            BITGET_ACCESS_PASSPHRASE_HEADER.to_string(),
            api_passphrase.to_string(),
        );
        headers.insert(
            BITGET_LOCALE_HEADER.to_string(),
            BITGET_LOCALE_HEADER_VALUE.to_string(),
        );
        Ok(headers)
    }

    async fn signed_get_value(
        &self,
        path: &str,
        query: &BTreeMap<String, Vec<String>>,
    ) -> Result<Value> {
        let query_string = Self::build_query_string(query);
        let request_path = if query_string.is_empty() {
            path.to_string()
        } else {
            format!("{path}?{query_string}")
        };
        let headers = self.authenticated_headers(
            "GET",
            path,
            if query_string.is_empty() {
                None
            } else {
                Some(query_string.as_str())
            },
            None,
        )?;

        let response = self
            .client
            .get(
                self.url(&request_path),
                None,
                Some(headers),
                Some(5),
                Some(Self::rate_limit_keys(path)),
            )
            .await
            .map_err(|e| anyhow!("HTTP request failed: {e}"))?;

        if !response.status.is_success() {
            bail!("HTTP request failed with status {}", response.status.as_u16());
        }

        parse_bitget_response::<Value>(&response.body)
    }

    async fn signed_post_value<B: Serialize>(&self, path: &str, body: &B) -> Result<Value> {
        let body_bytes = serde_json::to_vec(body)?;
        let headers = self.authenticated_headers("POST", path, None, Some(&body_bytes))?;

        let response = self
            .client
            .post(
                self.url(path),
                None,
                Some(headers),
                Some(body_bytes),
                Some(5),
                Some(Self::rate_limit_keys(path)),
            )
            .await
            .map_err(|e| anyhow!("HTTP request failed: {e}"))?;

        if !response.status.is_success() {
            bail!("HTTP request failed with status {}", response.status.as_u16());
        }

        parse_bitget_response::<Value>(&response.body)
    }

    fn margin_coin_for_product_type(product_type: BitgetProductType) -> Option<&'static str> {
        match product_type {
            BitgetProductType::UsdtFutures => Some("USDT"),
            BitgetProductType::UsdcFutures => Some("USDC"),
            BitgetProductType::CoinFutures => None,
            BitgetProductType::Spot => None,
        }
    }

    fn effective_margin_coin(
        product_type: BitgetProductType,
        margin_coin: Option<&str>,
    ) -> Option<String> {
        margin_coin
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string)
            .or_else(|| Self::margin_coin_for_product_type(product_type).map(ToString::to_string))
    }

    fn product_type_from_contract_symbol(symbol: &BitgetContractSymbol) -> BitgetProductType {
        match symbol.product_type.as_deref().unwrap_or("USDT-FUTURES") {
            "COIN-FUTURES" => BitgetProductType::CoinFutures,
            "USDC-FUTURES" => BitgetProductType::UsdcFutures,
            "SPOT" => BitgetProductType::Spot,
            _ => BitgetProductType::UsdtFutures,
        }
    }

    /// Requests spot symbols from `GET /api/v2/spot/public/symbols`.
    pub async fn get_spot_symbols(&self) -> Result<Vec<BitgetSpotSymbol>> {
        self.request::<Vec<BitgetSpotSymbol>>(BITGET_SPOT_SYMBOLS_PATH, None)
            .await
    }

    /// Requests contract definitions from
    /// `GET /api/v2/mix/market/contracts`.
    pub async fn get_contract_config(&self) -> Result<Vec<BitgetContractSymbol>> {
        let mut symbols = Vec::new();

        for product_type in [
            BitgetProductType::UsdtFutures,
            BitgetProductType::CoinFutures,
            BitgetProductType::UsdcFutures,
        ] {
            symbols.extend(self.get_contract_config_by_product_type(product_type).await?);
        }

        Ok(symbols)
    }

    /// Requests contract definitions from
    /// `GET /api/v2/mix/market/contracts`.
    pub async fn get_contract_config_by_product_type(
        &self,
        product_type: BitgetProductType,
    ) -> Result<Vec<BitgetContractSymbol>> {
        let mut query: HashMap<String, Vec<String>> = HashMap::new();
        query.insert("productType".to_string(), vec![product_type.as_api_str().to_string()]);

        self.request::<Vec<BitgetContractSymbol>>(BITGET_CONTRACT_CONFIG_PATH, Some(&query))
            .await
    }

    /// Requests candlestick data for spot instruments from
    /// `GET /api/v2/spot/market/candles`.
    pub async fn get_spot_candles(
        &self,
        symbol: &str,
        granularity: &str,
        start_time: Option<i64>,
        end_time: Option<i64>,
        limit: Option<u32>,
    ) -> Result<Vec<Vec<String>>> {
        let mut query: HashMap<String, Vec<String>> = HashMap::new();
        query.insert("symbol".to_string(), vec![symbol.to_string()]);
        query.insert("granularity".to_string(), vec![granularity.to_string()]);

        if let Some(start_time) = start_time {
            query.insert("startTime".to_string(), vec![start_time.to_string()]);
        }
        if let Some(end_time) = end_time {
            query.insert("endTime".to_string(), vec![end_time.to_string()]);
        }
        if let Some(limit) = limit {
            query.insert("limit".to_string(), vec![limit.to_string()]);
        }

        self.request::<Vec<Vec<String>>>(BITGET_SPOT_CANDLES_PATH, Some(&query))
            .await
    }

    /// Requests candlestick data for mix instruments from
    /// `GET /api/v2/mix/market/candles`.
    pub async fn get_mix_candles(
        &self,
        symbol: &str,
        product_type: BitgetProductType,
        granularity: &str,
        start_time: Option<i64>,
        end_time: Option<i64>,
        limit: Option<u32>,
    ) -> Result<Vec<Vec<String>>> {
        let mut query: HashMap<String, Vec<String>> = HashMap::new();
        query.insert("symbol".to_string(), vec![symbol.to_string()]);
        query.insert("granularity".to_string(), vec![granularity.to_string()]);
        query.insert(
            "productType".to_string(),
            vec![product_type.as_api_str().to_string()],
        );

        if let Some(start_time) = start_time {
            query.insert("startTime".to_string(), vec![start_time.to_string()]);
        }
        if let Some(end_time) = end_time {
            query.insert("endTime".to_string(), vec![end_time.to_string()]);
        }
        if let Some(limit) = limit {
            query.insert("limit".to_string(), vec![limit.to_string()]);
        }

        self.request::<Vec<Vec<String>>>(BITGET_MIX_CANDLES_PATH, Some(&query))
            .await
    }

    /// Requests candlestick data for an instrument.
    pub async fn request_bars(
        &self,
        product_type: BitgetProductType,
        symbol: &str,
        granularity: &str,
        start_time: Option<i64>,
        end_time: Option<i64>,
        limit: Option<u32>,
    ) -> Result<Vec<Vec<String>>> {
        match product_type {
            BitgetProductType::Spot => {
                self.get_spot_candles(symbol, granularity, start_time, end_time, limit).await
            }
            BitgetProductType::UsdtFutures
            | BitgetProductType::CoinFutures
            | BitgetProductType::UsdcFutures => {
                self.get_mix_candles(
                    symbol,
                    product_type,
                    granularity,
                    start_time,
                    end_time,
                    limit,
                )
                .await
            }
        }
    }

    /// Requests current mix funding rate from
    /// `GET /api/v2/mix/market/current-fund-rate`.
    pub async fn request_funding_rates(
        &self,
        product_type: BitgetProductType,
        symbol: Option<&str>,
    ) -> Result<Vec<BitgetCurrentFundingRate>> {
        if matches!(product_type, BitgetProductType::Spot) {
            return Ok(Vec::new());
        }

        let mut query: HashMap<String, Vec<String>> = HashMap::new();
        query.insert(
            "productType".to_string(),
            vec![product_type.as_api_str().to_string()],
        );
        if let Some(symbol) = symbol {
            query.insert("symbol".to_string(), vec![symbol.to_string()]);
        }

        self.request::<Vec<BitgetCurrentFundingRate>>(
            BITGET_MIX_CURRENT_FUNDING_RATE_PATH,
            Some(&query),
        )
        .await
    }

    /// Requests historical funding rates from
    /// `GET /api/v3/market/history-fund-rate`.
    pub async fn request_funding_rate_history(
        &self,
        product_type: BitgetProductType,
        symbol: &str,
        cursor: u32,
        limit: u32,
    ) -> Result<Vec<BitgetHistoricalFundingRate>> {
        if matches!(product_type, BitgetProductType::Spot) {
            return Ok(Vec::new());
        }

        let mut query: HashMap<String, Vec<String>> = HashMap::new();
        query.insert(
            "category".to_string(),
            vec![product_type.as_api_str().to_string()],
        );
        query.insert("symbol".to_string(), vec![symbol.to_string()]);
        query.insert("pageNo".to_string(), vec![cursor.to_string()]);
        query.insert("pageSize".to_string(), vec![limit.to_string()]);

        Ok(self
            .request::<BitgetFundingHistoryPage>(BITGET_V3_HISTORY_FUNDING_RATE_PATH, Some(&query))
            .await?
            .result_list)
    }

    pub async fn get_spot_merge_depth(&self, symbol: &str) -> Result<BitgetOrderBookSnapshot> {
        let mut query: HashMap<String, Vec<String>> = HashMap::new();
        query.insert("symbol".to_string(), vec![symbol.to_string()]);

        self.request::<BitgetOrderBookSnapshot>(BITGET_SPOT_MERGE_DEPTH_PATH, Some(&query))
            .await
    }

    pub async fn get_mix_merge_depth(
        &self,
        symbol: &str,
        product_type: BitgetProductType,
    ) -> Result<BitgetOrderBookSnapshot> {
        let mut query: HashMap<String, Vec<String>> = HashMap::new();
        query.insert("symbol".to_string(), vec![symbol.to_string()]);
        query.insert(
            "productType".to_string(),
            vec![product_type.as_api_str().to_string()],
        );

        self.request::<BitgetOrderBookSnapshot>(BITGET_MIX_MERGE_DEPTH_PATH, Some(&query))
            .await
    }

    pub async fn request_order_book_snapshot(
        &self,
        symbol: &str,
        product_type: BitgetProductType,
    ) -> Result<BitgetOrderBookSnapshot> {
        match product_type {
            BitgetProductType::Spot => self.get_spot_merge_depth(symbol).await,
            BitgetProductType::UsdtFutures
            | BitgetProductType::CoinFutures
            | BitgetProductType::UsdcFutures => self.get_mix_merge_depth(symbol, product_type).await,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn submit_order(
        &self,
        product_type: BitgetProductType,
        symbol: &str,
        margin_coin: Option<String>,
        client_oid: Option<String>,
        side: &str,
        order_type: &str,
        size: &str,
        force: Option<String>,
        price: Option<String>,
        reduce_only: bool,
    ) -> Result<Value> {
        let mut body = serde_json::Map::new();
        body.insert("symbol".to_string(), Value::String(symbol.to_string()));
        body.insert("side".to_string(), Value::String(side.to_string()));
        body.insert("orderType".to_string(), Value::String(order_type.to_string()));
        body.insert("size".to_string(), Value::String(size.to_string()));
        if let Some(client_oid) = client_oid {
            body.insert("clientOid".to_string(), Value::String(client_oid));
        }
        if let Some(force) = force {
            body.insert("force".to_string(), Value::String(force));
        }
        if let Some(price) = price {
            body.insert("price".to_string(), Value::String(price));
        }

        let path = match product_type {
            BitgetProductType::Spot => BITGET_SPOT_PLACE_ORDER_PATH,
            _ => {
                body.insert(
                    "productType".to_string(),
                    Value::String(product_type.as_api_str().to_string()),
                );
                if let Some(margin_coin) =
                    Self::effective_margin_coin(product_type, margin_coin.as_deref())
                {
                    body.insert(
                        "marginCoin".to_string(),
                        Value::String(margin_coin),
                    );
                }
                body.insert(
                    "reduceOnly".to_string(),
                    Value::String(if reduce_only { "YES" } else { "NO" }.to_string()),
                );
                BITGET_MIX_PLACE_ORDER_PATH
            }
        };

        self.signed_post_value(path, &Value::Object(body)).await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn modify_order(
        &self,
        product_type: BitgetProductType,
        symbol: &str,
        margin_coin: Option<String>,
        client_oid: Option<String>,
        order_id: Option<String>,
        new_client_oid: Option<String>,
        size: Option<String>,
        price: Option<String>,
    ) -> Result<Value> {
        let mut body = serde_json::Map::new();
        body.insert("symbol".to_string(), Value::String(symbol.to_string()));
        if let Some(client_oid) = client_oid {
            body.insert("clientOid".to_string(), Value::String(client_oid));
        }
        if let Some(order_id) = order_id {
            body.insert("orderId".to_string(), Value::String(order_id));
        }
        if let Some(new_client_oid) = new_client_oid {
            body.insert("newClientOid".to_string(), Value::String(new_client_oid));
        }
        if let Some(size) = size {
            body.insert("size".to_string(), Value::String(size));
        }
        if let Some(price) = price {
            body.insert("price".to_string(), Value::String(price));
        }

        let path = match product_type {
            BitgetProductType::Spot => BITGET_SPOT_CANCEL_REPLACE_ORDER_PATH,
            _ => {
                body.insert(
                    "productType".to_string(),
                    Value::String(product_type.as_api_str().to_string()),
                );
                if let Some(margin_coin) =
                    Self::effective_margin_coin(product_type, margin_coin.as_deref())
                {
                    body.insert(
                        "marginCoin".to_string(),
                        Value::String(margin_coin),
                    );
                }
                BITGET_MIX_MODIFY_ORDER_PATH
            }
        };

        self.signed_post_value(path, &Value::Object(body)).await
    }

    pub async fn cancel_order(
        &self,
        product_type: BitgetProductType,
        symbol: &str,
        margin_coin: Option<String>,
        client_oid: Option<String>,
        order_id: Option<String>,
    ) -> Result<Value> {
        let mut body = serde_json::Map::new();
        body.insert("symbol".to_string(), Value::String(symbol.to_string()));
        if let Some(client_oid) = client_oid {
            body.insert("clientOid".to_string(), Value::String(client_oid));
        }
        if let Some(order_id) = order_id {
            body.insert("orderId".to_string(), Value::String(order_id));
        }

        let path = match product_type {
            BitgetProductType::Spot => BITGET_SPOT_CANCEL_ORDER_PATH,
            _ => {
                body.insert(
                    "productType".to_string(),
                    Value::String(product_type.as_api_str().to_string()),
                );
                if let Some(margin_coin) =
                    Self::effective_margin_coin(product_type, margin_coin.as_deref())
                {
                    body.insert(
                        "marginCoin".to_string(),
                        Value::String(margin_coin),
                    );
                }
                BITGET_MIX_CANCEL_ORDER_PATH
            }
        };

        self.signed_post_value(path, &Value::Object(body)).await
    }

    pub async fn cancel_all_orders(
        &self,
        product_type: BitgetProductType,
        symbol: Option<String>,
        margin_coin: Option<String>,
    ) -> Result<Value> {
        let mut body = serde_json::Map::new();
        let path = match product_type {
            BitgetProductType::Spot => {
                if let Some(symbol) = symbol {
                    body.insert("symbol".to_string(), Value::String(symbol));
                }
                BITGET_SPOT_CANCEL_SYMBOL_ORDER_PATH
            }
            _ => {
                body.insert(
                    "productType".to_string(),
                    Value::String(product_type.as_api_str().to_string()),
                );
                if let Some(symbol) = symbol {
                    body.insert("symbol".to_string(), Value::String(symbol));
                }
                if let Some(margin_coin) =
                    Self::effective_margin_coin(product_type, margin_coin.as_deref())
                {
                    body.insert(
                        "marginCoin".to_string(),
                        Value::String(margin_coin),
                    );
                }
                BITGET_MIX_CANCEL_ALL_ORDERS_PATH
            }
        };

        self.signed_post_value(path, &Value::Object(body)).await
    }

    pub async fn batch_cancel_orders(
        &self,
        product_type: BitgetProductType,
        symbol: &str,
        margin_coin: Option<String>,
        client_oids: Vec<String>,
        order_ids: Vec<String>,
    ) -> Result<Value> {
        let mut body = serde_json::Map::new();
        body.insert("symbol".to_string(), Value::String(symbol.to_string()));

        match product_type {
            BitgetProductType::Spot => {
                let order_list: Vec<Value> = client_oids
                    .into_iter()
                    .map(|client_oid| {
                        serde_json::json!({
                            "clientOid": client_oid,
                        })
                    })
                    .chain(order_ids.into_iter().map(|order_id| {
                        serde_json::json!({
                            "orderId": order_id,
                        })
                    }))
                    .collect();
                body.insert("orderList".to_string(), Value::Array(order_list));
                self.signed_post_value(BITGET_SPOT_BATCH_CANCEL_ORDER_PATH, &Value::Object(body))
                    .await
            }
            _ => {
                body.insert(
                    "productType".to_string(),
                    Value::String(product_type.as_api_str().to_string()),
                );
                if let Some(margin_coin) =
                    Self::effective_margin_coin(product_type, margin_coin.as_deref())
                {
                    body.insert(
                        "marginCoin".to_string(),
                        Value::String(margin_coin),
                    );
                }
                let order_id_list: Vec<Value> = client_oids
                    .into_iter()
                    .map(|client_oid| serde_json::json!({ "clientOid": client_oid }))
                    .chain(order_ids.into_iter().map(|order_id| serde_json::json!({ "orderId": order_id })))
                    .collect();
                body.insert("orderIdList".to_string(), Value::Array(order_id_list));
                self.signed_post_value(BITGET_MIX_BATCH_CANCEL_ORDERS_PATH, &Value::Object(body))
                    .await
            }
        }
    }

    pub async fn request_order_status_report(
        &self,
        product_type: BitgetProductType,
        symbol: &str,
        margin_coin: Option<String>,
        client_oid: Option<String>,
        order_id: Option<String>,
    ) -> Result<Option<BitgetOrderInfo>> {
        let mut query = BTreeMap::new();
        query.insert("symbol".to_string(), vec![symbol.to_string()]);
        if let Some(client_oid) = client_oid {
            query.insert("clientOid".to_string(), vec![client_oid]);
        }
        if let Some(order_id) = order_id {
            query.insert("orderId".to_string(), vec![order_id]);
        }

        match product_type {
            BitgetProductType::Spot => {
                let data = self.signed_get_value(BITGET_SPOT_ORDER_INFO_PATH, &query).await?;
                let reports = serde_json::from_value::<Vec<BitgetOrderInfo>>(data)?;
                Ok(reports.into_iter().next())
            }
            _ => {
                query.insert(
                    "productType".to_string(),
                    vec![product_type.as_api_str().to_string()],
                );
                if let Some(margin_coin) =
                    Self::effective_margin_coin(product_type, margin_coin.as_deref())
                {
                    query.insert("marginCoin".to_string(), vec![margin_coin]);
                }
                let data = self.signed_get_value(BITGET_MIX_ORDER_DETAIL_PATH, &query).await?;
                Ok(Some(serde_json::from_value::<BitgetOrderInfo>(data)?))
            }
        }
    }

    pub async fn request_order_status_reports(
        &self,
        product_type: BitgetProductType,
        symbol: Option<String>,
        margin_coin: Option<String>,
        open_only: bool,
        start: Option<i64>,
        end: Option<i64>,
        limit: Option<u32>,
    ) -> Result<Vec<BitgetOrderInfo>> {
        let mut query = BTreeMap::new();
        if let Some(symbol) = symbol {
            query.insert("symbol".to_string(), vec![symbol]);
        }
        if let Some(start) = start {
            query.insert("startTime".to_string(), vec![start.to_string()]);
        }
        if let Some(end) = end {
            query.insert("endTime".to_string(), vec![end.to_string()]);
        }
        if let Some(limit) = limit {
            query.insert("limit".to_string(), vec![limit.to_string()]);
        }

        match product_type {
            BitgetProductType::Spot => {
                let mut reports =
                    serde_json::from_value::<Vec<BitgetOrderInfo>>(
                        self.signed_get_value(BITGET_SPOT_UNFILLED_ORDERS_PATH, &query).await?,
                    )?;
                if !open_only {
                    let history =
                        serde_json::from_value::<Vec<BitgetOrderInfo>>(
                            self.signed_get_value(BITGET_SPOT_HISTORY_ORDERS_PATH, &query).await?,
                        )?;
                    let existing: HashSet<String> =
                        reports.iter().map(|report| report.order_id.clone()).collect();
                    for report in history {
                        if !existing.contains(&report.order_id) {
                            reports.push(report);
                        }
                    }
                }
                Ok(reports)
            }
            _ => {
                query.insert(
                    "productType".to_string(),
                    vec![product_type.as_api_str().to_string()],
                );
                if let Some(margin_coin) =
                    Self::effective_margin_coin(product_type, margin_coin.as_deref())
                {
                    query.insert("marginCoin".to_string(), vec![margin_coin]);
                }
                let mut reports = serde_json::from_value::<BitgetMixOrdersPage>(
                    self.signed_get_value(BITGET_MIX_ORDERS_PENDING_PATH, &query).await?,
                )?
                .entrusted_list;
                if !open_only {
                    let history = serde_json::from_value::<BitgetMixOrdersPage>(
                        self.signed_get_value(BITGET_MIX_ORDERS_HISTORY_PATH, &query).await?,
                    )?
                    .entrusted_list;
                    let existing: HashSet<String> =
                        reports.iter().map(|report| report.order_id.clone()).collect();
                    for report in history {
                        if !existing.contains(&report.order_id) {
                            reports.push(report);
                        }
                    }
                }
                Ok(reports)
            }
        }
    }

    pub async fn request_fill_reports(
        &self,
        product_type: BitgetProductType,
        symbol: Option<String>,
        margin_coin: Option<String>,
        order_id: Option<String>,
        start: Option<i64>,
        end: Option<i64>,
        limit: Option<u32>,
    ) -> Result<Vec<BitgetFillInfo>> {
        let mut query = BTreeMap::new();
        if let Some(symbol) = symbol {
            query.insert("symbol".to_string(), vec![symbol]);
        }
        if let Some(order_id) = order_id {
            query.insert("orderId".to_string(), vec![order_id]);
        }
        if let Some(start) = start {
            query.insert("startTime".to_string(), vec![start.to_string()]);
        }
        if let Some(end) = end {
            query.insert("endTime".to_string(), vec![end.to_string()]);
        }
        if let Some(limit) = limit {
            query.insert("limit".to_string(), vec![limit.to_string()]);
        }

        match product_type {
            BitgetProductType::Spot => Ok(serde_json::from_value::<Vec<BitgetFillInfo>>(
                self.signed_get_value(BITGET_SPOT_FILLS_PATH, &query).await?,
            )?),
            _ => {
                query.insert(
                    "productType".to_string(),
                    vec![product_type.as_api_str().to_string()],
                );
                if let Some(margin_coin) =
                    Self::effective_margin_coin(product_type, margin_coin.as_deref())
                {
                    query.insert("marginCoin".to_string(), vec![margin_coin]);
                }
                Ok(serde_json::from_value::<BitgetMixFillsPage>(
                    self.signed_get_value(BITGET_MIX_FILL_HISTORY_PATH, &query).await?,
                )?
                .fill_list)
            }
        }
    }

    pub async fn request_position_status_reports(
        &self,
        product_type: BitgetProductType,
        symbol: Option<String>,
        margin_coin: Option<String>,
    ) -> Result<Vec<BitgetPositionInfo>> {
        if matches!(product_type, BitgetProductType::Spot) {
            return Ok(Vec::new());
        }

        let mut query = BTreeMap::new();
        query.insert(
            "productType".to_string(),
            vec![product_type.as_api_str().to_string()],
        );
        if let Some(symbol) = symbol {
            query.insert("symbol".to_string(), vec![symbol]);
        }
        if let Some(margin_coin) = Self::effective_margin_coin(product_type, margin_coin.as_deref()) {
            query.insert("marginCoin".to_string(), vec![margin_coin]);
        }

        Ok(serde_json::from_value::<Vec<BitgetPositionInfo>>(
            self.signed_get_value(BITGET_MIX_ALL_POSITION_PATH, &query).await?,
        )?)
    }

    pub fn build_order_book_snapshot_deltas(
        &self,
        snapshot: &BitgetOrderBookSnapshot,
        instrument: &InstrumentAny,
        ts_init: UnixNanos,
    ) -> Result<OrderBookDeltas> {
        let millis = snapshot
            .ts
            .parse::<u64>()
            .map_err(|e| anyhow!("invalid Bitget snapshot timestamp: {e}"))?;
        let ts_event = UnixNanos::from(millis * 1_000_000);
        let ts_init = if ts_init.is_zero() { ts_event } else { ts_init };
        let update_id = millis;
        let total_levels = snapshot.bids.len() + snapshot.asks.len();
        let mut deltas = Vec::with_capacity(total_levels + 1);

        let mut clear = OrderBookDelta::clear(instrument.id(), update_id, ts_event, ts_init);
        if total_levels == 0 {
            clear.flags |= RecordFlag::F_LAST as u8;
        }
        deltas.push(clear);

        let mut processed = 0_usize;
        let mut push_level = |level: &[String; 2], side: OrderSide| -> Result<()> {
            processed += 1;
            let mut flags = RecordFlag::F_MBP as u8;
            if processed == total_levels {
                flags |= RecordFlag::F_LAST as u8;
            }

            let order = BookOrder::new(
                side,
                Price::from(level[0].as_str()),
                Quantity::from(level[1].as_str()),
                update_id,
            );
            deltas.push(
                OrderBookDelta::new_checked(
                    instrument.id(),
                    BookAction::Add,
                    order,
                    flags,
                    update_id,
                    ts_event,
                    ts_init,
                )
                .map_err(|e| anyhow!("failed to construct Bitget snapshot delta: {e}"))?,
            );
            Ok(())
        };

        for level in &snapshot.bids {
            push_level(level, OrderSide::Buy)?;
        }
        for level in &snapshot.asks {
            push_level(level, OrderSide::Sell)?;
        }

        OrderBookDeltas::new_checked(instrument.id(), deltas)
            .map_err(|e| anyhow!("failed to assemble Bitget snapshot deltas: {e}"))
    }

    fn parse_delivery_time_ms(time: &str) -> Option<i64> {
        let trimmed = time.trim();
        if trimmed.is_empty() {
            return None;
        }

        trimmed.parse::<i64>().ok().filter(|v| *v > 0)
    }

    fn is_perpetual_contract(symbol_type: &str) -> bool {
        let symbol_type = symbol_type.to_ascii_lowercase();
        matches!(symbol_type.as_str(), "perp" | "perpetual")
    }

    fn to_instrument_id(&self, symbol: &str) -> InstrumentId {
        InstrumentId::new(Symbol::new(symbol), Venue::new("BITGET"))
    }

    fn build_spot_instrument(
        &self,
        symbol: &BitgetSpotSymbol,
        ts_init: UnixNanos,
    ) -> Option<InstrumentAny> {
        if symbol.symbol.is_empty() || symbol.base_coin.is_empty() || symbol.quote_coin.is_empty() {
            return None;
        }

        let raw_symbol = nautilus_symbol_for_spot(&symbol.symbol);

        Some(InstrumentAny::CurrencyPair(CurrencyPair::new(
            self.to_instrument_id(&raw_symbol),
            Symbol::new(&raw_symbol),
            Currency::get_or_create_crypto_with_context(&symbol.base_coin, Some("Bitget spot base currency")),
            Currency::get_or_create_crypto_with_context(&symbol.quote_coin, Some("Bitget spot quote currency")),
            1,   // price_precision
            1,   // size_precision
            Price::from("0.1"),
            Quantity::from("0.1"),
            None, // multiplier
            Some(Quantity::from("1")),
            None, // max_quantity
            None, // min_quantity
            None, // max_notional
            None, // min_notional
            None, // max_price
            None, // min_price
            None, // margin_init
            None, // margin_maint
            None, // maker_fee
            None, // taker_fee
            None,
            ts_init,
            ts_init,
        )))
    }

    fn build_perpetual_instrument(
        &self,
        symbol: &BitgetContractSymbol,
        ts_init: UnixNanos,
    ) -> Option<InstrumentAny> {
        let raw_symbol = symbol.symbol.trim();
        let base_coin = symbol.base_coin.as_deref().unwrap_or_default().trim();
        let quote_coin = symbol.quote_coin.as_deref().unwrap_or_default().trim();

        if raw_symbol.is_empty() || base_coin.is_empty() || quote_coin.is_empty() {
            return None;
        }

        let nautilus_symbol = nautilus_symbol_for_perp(raw_symbol);
        let base_currency =
            Currency::get_or_create_crypto_with_context(base_coin, Some("Bitget perpetual base currency"));
        let quote_currency =
            Currency::get_or_create_crypto_with_context(quote_coin, Some("Bitget perpetual quote currency"));
        let product_type = Self::product_type_from_contract_symbol(symbol);
        let settlement_currency = if matches!(product_type, BitgetProductType::CoinFutures) {
            base_currency
        } else {
            quote_currency
        };
        let is_inverse = matches!(product_type, BitgetProductType::CoinFutures);

        Some(InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
            self.to_instrument_id(&nautilus_symbol),
            Symbol::new(&nautilus_symbol),
            base_currency,
            quote_currency,
            settlement_currency,
            is_inverse,
            1,
            1,
            Price::from("0.1"),
            Quantity::from("0.1"),
            None,
            Some(Quantity::from("1")),
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
            ts_init,
            ts_init,
        )))
    }

    fn build_delivery_instrument(
        &self,
        symbol: &BitgetContractSymbol,
        ts_init: UnixNanos,
    ) -> Option<InstrumentAny> {
        let raw_symbol = symbol.symbol.trim();
        let base_coin = symbol.base_coin.as_deref().unwrap_or_default().trim();
        let quote_coin = symbol.quote_coin.as_deref().unwrap_or_default().trim();
        let delivery_time = symbol.delivery_time.as_deref().unwrap_or_default();
        let delivery_time_ms = Self::parse_delivery_time_ms(delivery_time)?;

        if raw_symbol.is_empty() || base_coin.is_empty() || quote_coin.is_empty() {
            return None;
        }

        let delivery_time_ns = u64::try_from(delivery_time_ms)
            .ok()
            .and_then(|ms| ms.checked_mul(1_000_000))
            .map(UnixNanos::from)?;
        let nautilus_symbol = nautilus_symbol_for_delivery(raw_symbol, delivery_time_ms);
        let base_currency =
            Currency::get_or_create_crypto_with_context(base_coin, Some("Bitget future base currency"));
        let quote_currency =
            Currency::get_or_create_crypto_with_context(quote_coin, Some("Bitget future quote currency"));
        let product_type = Self::product_type_from_contract_symbol(symbol);
        let settlement_currency = if matches!(product_type, BitgetProductType::CoinFutures) {
            base_currency
        } else {
            quote_currency
        };
        let is_inverse = matches!(product_type, BitgetProductType::CoinFutures);

        Some(InstrumentAny::CryptoFuture(CryptoFuture::new(
            self.to_instrument_id(&nautilus_symbol),
            Symbol::new(&nautilus_symbol),
            base_currency,
            quote_currency,
            settlement_currency,
            is_inverse,
            ts_init,
            delivery_time_ns,
            1,
            1,
            Price::from("0.1"),
            Quantity::from("0.1"),
            None,
            Some(Quantity::from("1")),
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
            ts_init,
            ts_init,
        )))
    }

    /// Build and return instrument objects for provided spot and contract API responses.
    #[allow(clippy::too_many_arguments)]
    pub fn build_instruments(
        &self,
        spot_symbols: &[BitgetSpotSymbol],
        contract_symbols: &[BitgetContractSymbol],
        ts_init: UnixNanos,
    ) -> Vec<InstrumentAny> {
        let mut instruments = Vec::new();

        for symbol in spot_symbols {
            if let Some(instrument) = self.build_spot_instrument(symbol, ts_init) {
                instruments.push(instrument);
            }
        }

        for symbol in contract_symbols {
            let symbol_type = symbol.symbol_type.clone().unwrap_or_default();
            let instrument = if Self::is_perpetual_contract(&symbol_type) {
                self.build_perpetual_instrument(symbol, ts_init)
            } else {
                self.build_delivery_instrument(symbol, ts_init)
            };

            if let Some(instrument) = instrument {
                instruments.push(instrument);
            }
        }

        instruments
    }

    /// Requests and returns all available instruments for spot + futures.
    pub async fn request_instruments(&self) -> Result<Vec<InstrumentAny>> {
        let ts_init = self.clock.get_time_ns();
        let spot_symbols = self.get_spot_symbols().await?;
        let contract_symbols = self.get_contract_config().await?;

        Ok(self.build_instruments(&spot_symbols, &contract_symbols, ts_init))
    }

    pub fn cache_instrument(&mut self, symbol: &str) {
        self.subscribed_instruments.insert(symbol.to_string());
    }

    #[must_use]
    pub fn cached_instruments(&self) -> Vec<String> {
        self.subscribed_instruments.iter().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::enums::BitgetEnvironment;

    #[test]
    fn test_parse_bitget_response_surfaces_api_error_before_data() {
        let payload = br#"{
            "code":"40000",
            "msg":"invalid product type",
            "requestTime":1700000000000,
            "data":{"unexpected":"shape"}
        }"#;

        let result = parse_bitget_response::<Vec<BitgetSpotSymbol>>(payload);
        assert!(result.is_err());

        let err = result.unwrap_err().to_string();
        assert!(err.contains("40000"));
        assert!(err.contains("invalid product type"));
    }

    #[test]
    fn test_contract_product_filter_allows_non_usdt_futures() {
        let client = BitgetHttpClient::new(BitgetEnvironment::Mainnet);

        let spot_symbols: Vec<BitgetSpotSymbol> = vec![];
        let contract_symbols = vec![
            BitgetContractSymbol {
                symbol: "BTCUSDT".to_string(),
                product_type: Some("usdt-futures".to_string()),
                symbol_type: Some("perpetual".to_string()),
                base_coin: Some("BTC".to_string()),
                quote_coin: Some("USDT".to_string()),
                delivery_time: None,
            },
            BitgetContractSymbol {
                symbol: "ETHUSDT".to_string(),
                product_type: Some("COIN-FUTURES".to_string()),
                symbol_type: Some("perpetual".to_string()),
                base_coin: Some("ETH".to_string()),
                quote_coin: Some("USDT".to_string()),
                delivery_time: None,
            },
        ];

        let instruments = client.build_instruments(
            &spot_symbols,
            &contract_symbols,
            UnixNanos::from(1_700_000_000_000_000_000_u64),
        );

        assert_eq!(instruments.len(), 2);
        assert!(matches!(&instruments[0], InstrumentAny::CryptoPerpetual(_)));
        assert!(matches!(&instruments[1], InstrumentAny::CryptoPerpetual(_)));
        assert!(
            symbols_contain_product_type(&instruments, "BTCUSDT-PERP")
                && symbols_contain_product_type(&instruments, "ETHUSDT-PERP")
        );
    }

    #[test]
    fn test_demo_environment_adds_demo_rest_header() {
        let mainnet_headers = BitgetHttpClient::default_headers(BitgetEnvironment::Mainnet);
        let demo_headers = BitgetHttpClient::default_headers(BitgetEnvironment::Demo);

        assert!(!mainnet_headers.contains_key(BITGET_DEMO_REST_HEADER));
        assert_eq!(
            demo_headers.get(BITGET_DEMO_REST_HEADER),
            Some(&BITGET_DEMO_REST_HEADER_VALUE.to_string())
        );
    }

    fn symbols_contain_product_type(
        instruments: &[InstrumentAny],
        raw_symbol: &str,
    ) -> bool {
        instruments.iter().any(|instrument| match instrument {
            InstrumentAny::CryptoPerpetual(perpetual) => perpetual.raw_symbol.to_string() == raw_symbol,
            _ => false,
        })
    }
}

#[cfg(feature = "python")]
#[pyo3::pymethods]
impl BitgetHttpClient {
    #[new]
    fn py_new(environment: BitgetEnvironment) -> Self {
        Self::new(environment)
    }

    #[staticmethod]
    #[pyo3(name = "with_credentials")]
    fn py_with_credentials(
        environment: BitgetEnvironment,
        api_key: String,
        api_secret: String,
        api_passphrase: String,
    ) -> Self {
        Self::with_credentials(environment, api_key, api_secret, api_passphrase)
    }

    #[pyo3(name = "cache_instrument")]
    fn py_cache_instrument(&mut self, symbol: &str) {
        Self::cache_instrument(self, symbol);
    }

    #[pyo3(name = "request_instruments")]
    fn py_request_instruments<'py>(
        &self,
        py: pyo3::Python<'py>,
    ) -> pyo3::PyResult<pyo3::Bound<'py, pyo3::PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let instruments = client
                .request_instruments()
                .await
                .map_err(nautilus_core::python::to_pyvalue_err)?;

            pyo3::Python::attach(|py| {
                let py_instruments: pyo3::PyResult<Vec<_>> = instruments
                    .into_iter()
                    .map(|inst| instrument_any_to_pyobject(py, inst))
                    .collect();
                let pylist = pyo3::types::PyList::new(py, py_instruments?)
                    .unwrap()
                    .into_any()
                    .unbind();
                Ok(pylist)
            })
        })
    }

    #[pyo3(name = "request_bars")]
    fn py_request_bars<'py>(
        &self,
        py: pyo3::Python<'py>,
        product_type: BitgetProductType,
        symbol: &str,
        granularity: &str,
        start_time: Option<i64>,
        end_time: Option<i64>,
        limit: Option<u32>,
    ) -> pyo3::PyResult<pyo3::Bound<'py, pyo3::PyAny>> {
        let client = self.clone();
        let symbol = symbol.to_string();
        let granularity = granularity.to_string();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let bars = client
                .request_bars(product_type, &symbol, &granularity, start_time, end_time, limit)
                .await
                .map_err(nautilus_core::python::to_pyvalue_err)?;
            serde_json::to_string(&bars).map_err(nautilus_core::python::to_pyvalue_err)
        })
    }

    #[pyo3(name = "request_funding_rates")]
    fn py_request_funding_rates<'py>(
        &self,
        py: pyo3::Python<'py>,
        product_type: BitgetProductType,
        symbol: Option<String>,
    ) -> pyo3::PyResult<pyo3::Bound<'py, pyo3::PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let symbol = symbol.as_deref();
            let rates = client
                .request_funding_rates(product_type, symbol)
                .await
                .map_err(nautilus_core::python::to_pyvalue_err)?;
            serde_json::to_string(&rates).map_err(nautilus_core::python::to_pyvalue_err)
        })
    }

    #[pyo3(name = "request_funding_rate_history")]
    fn py_request_funding_rate_history<'py>(
        &self,
        py: pyo3::Python<'py>,
        product_type: BitgetProductType,
        symbol: &str,
        cursor: u32,
        limit: u32,
    ) -> pyo3::PyResult<pyo3::Bound<'py, pyo3::PyAny>> {
        let client = self.clone();
        let symbol = symbol.to_string();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let rates = client
                .request_funding_rate_history(product_type, &symbol, cursor, limit)
                .await
                .map_err(nautilus_core::python::to_pyvalue_err)?;
            serde_json::to_string(&rates).map_err(nautilus_core::python::to_pyvalue_err)
        })
    }

    #[pyo3(name = "request_order_book_snapshot")]
    fn py_request_order_book_snapshot<'py>(
        &self,
        py: pyo3::Python<'py>,
        symbol: &str,
        product_type: BitgetProductType,
        instrument: pyo3::Py<pyo3::PyAny>,
    ) -> pyo3::PyResult<pyo3::Bound<'py, pyo3::PyAny>> {
        let client = self.clone();
        let symbol = symbol.to_string();
        let instrument = pyobject_to_instrument_any(py, instrument)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let ts_init = client.clock.get_time_ns();
            let snapshot = client
                .request_order_book_snapshot(&symbol, product_type)
                .await
                .map_err(nautilus_core::python::to_pyvalue_err)?;
            let deltas = client
                .build_order_book_snapshot_deltas(&snapshot, &instrument, ts_init)
                .map_err(nautilus_core::python::to_pyvalue_err)?;

            pyo3::Python::attach(|py| {
                Ok(data_to_pycapsule(
                    py,
                    Data::Deltas(OrderBookDeltas_API::new(deltas)),
                ))
            })
        })
    }

    #[allow(clippy::too_many_arguments)]
    #[pyo3(name = "submit_order")]
    fn py_submit_order<'py>(
        &self,
        py: pyo3::Python<'py>,
        product_type: BitgetProductType,
        symbol: &str,
        margin_coin: Option<String>,
        client_oid: Option<String>,
        side: &str,
        order_type: &str,
        size: &str,
        force: Option<String>,
        price: Option<String>,
        reduce_only: bool,
    ) -> pyo3::PyResult<pyo3::Bound<'py, pyo3::PyAny>> {
        let client = self.clone();
        let symbol = symbol.to_string();
        let side = side.to_string();
        let order_type = order_type.to_string();
        let size = size.to_string();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client
                .submit_order(
                    product_type,
                    &symbol,
                    margin_coin,
                    client_oid,
                    &side,
                    &order_type,
                    &size,
                    force,
                    price,
                    reduce_only,
                )
                .await
                .map_err(nautilus_core::python::to_pyvalue_err)?;

            serde_json::to_string(&response).map_err(nautilus_core::python::to_pyvalue_err)
        })
    }

    #[allow(clippy::too_many_arguments)]
    #[pyo3(name = "modify_order")]
    fn py_modify_order<'py>(
        &self,
        py: pyo3::Python<'py>,
        product_type: BitgetProductType,
        symbol: &str,
        margin_coin: Option<String>,
        client_oid: Option<String>,
        order_id: Option<String>,
        new_client_oid: Option<String>,
        size: Option<String>,
        price: Option<String>,
    ) -> pyo3::PyResult<pyo3::Bound<'py, pyo3::PyAny>> {
        let client = self.clone();
        let symbol = symbol.to_string();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client
                .modify_order(
                    product_type,
                    &symbol,
                    margin_coin,
                    client_oid,
                    order_id,
                    new_client_oid,
                    size,
                    price,
                )
                .await
                .map_err(nautilus_core::python::to_pyvalue_err)?;

            serde_json::to_string(&response).map_err(nautilus_core::python::to_pyvalue_err)
        })
    }

    #[pyo3(name = "cancel_order")]
    fn py_cancel_order<'py>(
        &self,
        py: pyo3::Python<'py>,
        product_type: BitgetProductType,
        symbol: &str,
        margin_coin: Option<String>,
        client_oid: Option<String>,
        order_id: Option<String>,
    ) -> pyo3::PyResult<pyo3::Bound<'py, pyo3::PyAny>> {
        let client = self.clone();
        let symbol = symbol.to_string();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client
                .cancel_order(product_type, &symbol, margin_coin, client_oid, order_id)
                .await
                .map_err(nautilus_core::python::to_pyvalue_err)?;

            serde_json::to_string(&response).map_err(nautilus_core::python::to_pyvalue_err)
        })
    }

    #[pyo3(name = "cancel_all_orders")]
    fn py_cancel_all_orders<'py>(
        &self,
        py: pyo3::Python<'py>,
        product_type: BitgetProductType,
        symbol: Option<String>,
        margin_coin: Option<String>,
    ) -> pyo3::PyResult<pyo3::Bound<'py, pyo3::PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client
                .cancel_all_orders(product_type, symbol, margin_coin)
                .await
                .map_err(nautilus_core::python::to_pyvalue_err)?;

            serde_json::to_string(&response).map_err(nautilus_core::python::to_pyvalue_err)
        })
    }

    #[pyo3(name = "batch_cancel_orders")]
    fn py_batch_cancel_orders<'py>(
        &self,
        py: pyo3::Python<'py>,
        product_type: BitgetProductType,
        symbol: &str,
        margin_coin: Option<String>,
        client_oids: Vec<String>,
        order_ids: Vec<String>,
    ) -> pyo3::PyResult<pyo3::Bound<'py, pyo3::PyAny>> {
        let client = self.clone();
        let symbol = symbol.to_string();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client
                .batch_cancel_orders(product_type, &symbol, margin_coin, client_oids, order_ids)
                .await
                .map_err(nautilus_core::python::to_pyvalue_err)?;

            serde_json::to_string(&response).map_err(nautilus_core::python::to_pyvalue_err)
        })
    }

    #[pyo3(name = "request_order_status_report")]
    fn py_request_order_status_report<'py>(
        &self,
        py: pyo3::Python<'py>,
        product_type: BitgetProductType,
        symbol: &str,
        margin_coin: Option<String>,
        client_oid: Option<String>,
        order_id: Option<String>,
    ) -> pyo3::PyResult<pyo3::Bound<'py, pyo3::PyAny>> {
        let client = self.clone();
        let symbol = symbol.to_string();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client
                .request_order_status_report(product_type, &symbol, margin_coin, client_oid, order_id)
                .await
                .map_err(nautilus_core::python::to_pyvalue_err)?;

            response
                .map(|value| serde_json::to_string(&value))
                .transpose()
                .map_err(nautilus_core::python::to_pyvalue_err)
        })
    }

    #[pyo3(name = "request_order_status_reports")]
    fn py_request_order_status_reports<'py>(
        &self,
        py: pyo3::Python<'py>,
        product_type: BitgetProductType,
        symbol: Option<String>,
        margin_coin: Option<String>,
        open_only: bool,
        start: Option<i64>,
        end: Option<i64>,
        limit: Option<u32>,
    ) -> pyo3::PyResult<pyo3::Bound<'py, pyo3::PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client
                .request_order_status_reports(product_type, symbol, margin_coin, open_only, start, end, limit)
                .await
                .map_err(nautilus_core::python::to_pyvalue_err)?;

            serde_json::to_string(&response).map_err(nautilus_core::python::to_pyvalue_err)
        })
    }

    #[pyo3(name = "request_fill_reports")]
    fn py_request_fill_reports<'py>(
        &self,
        py: pyo3::Python<'py>,
        product_type: BitgetProductType,
        symbol: Option<String>,
        margin_coin: Option<String>,
        order_id: Option<String>,
        start: Option<i64>,
        end: Option<i64>,
        limit: Option<u32>,
    ) -> pyo3::PyResult<pyo3::Bound<'py, pyo3::PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client
                .request_fill_reports(product_type, symbol, margin_coin, order_id, start, end, limit)
                .await
                .map_err(nautilus_core::python::to_pyvalue_err)?;

            serde_json::to_string(&response).map_err(nautilus_core::python::to_pyvalue_err)
        })
    }

    #[pyo3(name = "request_position_status_reports")]
    fn py_request_position_status_reports<'py>(
        &self,
        py: pyo3::Python<'py>,
        product_type: BitgetProductType,
        symbol: Option<String>,
        margin_coin: Option<String>,
    ) -> pyo3::PyResult<pyo3::Bound<'py, pyo3::PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client
                .request_position_status_reports(product_type, symbol, margin_coin)
                .await
                .map_err(nautilus_core::python::to_pyvalue_err)?;

            serde_json::to_string(&response).map_err(nautilus_core::python::to_pyvalue_err)
        })
    }

    #[pyo3(name = "cached_instruments")]
    fn py_cached_instruments(&self) -> Vec<String> {
        Self::cached_instruments(self)
    }
}
