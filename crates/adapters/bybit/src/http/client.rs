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

//! Provides the HTTP client integration for the [Bybit](https://bybit.com) REST API.
//!
//! Bybit API reference <https://bybit-exchange.github.io/docs/>.

use std::{
    collections::HashMap,
    fmt::Debug,
    num::NonZeroU32,
    sync::{Arc, LazyLock, Mutex},
};

use nautilus_core::{
    consts::NAUTILUS_USER_AGENT, nanos::UnixNanos, time::get_atomic_clock_realtime,
};
use nautilus_model::{
    data::{Bar, BarType, TradeTick},
    enums::{BarAggregation, OrderSide, OrderType, TimeInForce},
    events::account::state::AccountState,
    identifiers::{AccountId, ClientOrderId, InstrumentId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{Price, Quantity},
};
use nautilus_network::{
    http::HttpClient,
    ratelimiter::quota::Quota,
    retry::{RetryConfig, RetryManager},
};
use reqwest::{Method, header::USER_AGENT};
use serde::{Serialize, de::DeserializeOwned};
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{
    error::BybitHttpError,
    models::{
        BybitInstrumentInverseResponse, BybitInstrumentLinearResponse,
        BybitInstrumentOptionResponse, BybitInstrumentSpotResponse, BybitKlinesResponse,
        BybitOpenOrdersResponse, BybitPlaceOrderResponse, BybitServerTimeResponse,
        BybitTradesResponse,
    },
    query::{
        BybitAmendOrderParamsBuilder, BybitBatchAmendOrderEntryBuilder,
        BybitBatchCancelOrderEntryBuilder, BybitBatchPlaceOrderEntryBuilder,
        BybitCancelAllOrdersParamsBuilder, BybitCancelOrderParamsBuilder,
        BybitInstrumentsInfoParams, BybitKlinesParams, BybitPlaceOrderParamsBuilder,
        BybitTradesParams,
    },
};
use crate::common::{
    consts::BYBIT_NAUTILUS_BROKER_ID,
    credential::Credential,
    enums::{
        BybitEnvironment, BybitKlineInterval, BybitOrderSide, BybitOrderType, BybitProductType,
        BybitTimeInForce,
    },
    models::BybitResponse,
    parse::{
        parse_account_state, parse_fill_report, parse_inverse_instrument, parse_kline_bar,
        parse_linear_instrument, parse_option_instrument, parse_order_status_report,
        parse_position_status_report, parse_spot_instrument, parse_trade_tick,
    },
    urls::bybit_http_base_url,
};

const DEFAULT_RECV_WINDOW_MS: u64 = 5_000;

/// Default Bybit REST API rate limit.
///
/// Bybit implements rate limiting per endpoint with varying limits.
/// We use a conservative 10 requests per second as a general default.
pub static BYBIT_REST_QUOTA: LazyLock<Quota> =
    LazyLock::new(|| Quota::per_second(NonZeroU32::new(10).expect("10 is a valid non-zero u32")));

const BYBIT_GLOBAL_RATE_KEY: &str = "bybit:global";

/// Inner HTTP client implementation containing the actual HTTP logic.
pub struct BybitHttpInnerClient {
    base_url: String,
    client: HttpClient,
    credential: Option<Credential>,
    recv_window_ms: u64,
    retry_manager: RetryManager<BybitHttpError>,
    cancellation_token: CancellationToken,
    instruments: Arc<Mutex<HashMap<Ustr, InstrumentAny>>>,
}

impl Default for BybitHttpInnerClient {
    fn default() -> Self {
        Self::new(None, Some(60), None, None, None)
            .expect("Failed to create default BybitHttpInnerClient")
    }
}

impl Debug for BybitHttpInnerClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BybitHttpInnerClient")
            .field("base_url", &self.base_url)
            .field("has_credentials", &self.credential.is_some())
            .field("recv_window_ms", &self.recv_window_ms)
            .finish()
    }
}

impl BybitHttpInnerClient {
    /// Cancel all pending HTTP requests.
    pub fn cancel_all_requests(&self) {
        self.cancellation_token.cancel();
    }

    /// Get the cancellation token for this client.
    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.cancellation_token
    }

    /// Creates a new [`BybitHttpInnerClient`] using the default Bybit HTTP URL.
    ///
    /// # Errors
    ///
    /// Returns an error if the retry manager cannot be created.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        base_url: Option<String>,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
    ) -> Result<Self, BybitHttpError> {
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

        let retry_manager = RetryManager::new(retry_config).map_err(|e| {
            BybitHttpError::NetworkError(format!("Failed to create retry manager: {e}"))
        })?;

        Ok(Self {
            base_url: base_url
                .unwrap_or_else(|| bybit_http_base_url(BybitEnvironment::Mainnet).to_string()),
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                Self::rate_limiter_quotas(),
                Some(*BYBIT_REST_QUOTA),
                timeout_secs,
            ),
            credential: None,
            recv_window_ms: DEFAULT_RECV_WINDOW_MS,
            retry_manager,
            cancellation_token: CancellationToken::new(),
            instruments: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Creates a new [`BybitHttpInnerClient`] configured with credentials.
    ///
    /// # Errors
    ///
    /// Returns an error if the retry manager cannot be created.
    #[allow(clippy::too_many_arguments)]
    pub fn with_credentials(
        api_key: String,
        api_secret: String,
        base_url: Option<String>,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
    ) -> Result<Self, BybitHttpError> {
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

        let retry_manager = RetryManager::new(retry_config).map_err(|e| {
            BybitHttpError::NetworkError(format!("Failed to create retry manager: {e}"))
        })?;

        Ok(Self {
            base_url: base_url
                .unwrap_or_else(|| bybit_http_base_url(BybitEnvironment::Mainnet).to_string()),
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                Self::rate_limiter_quotas(),
                Some(*BYBIT_REST_QUOTA),
                timeout_secs,
            ),
            credential: Some(Credential::new(api_key, api_secret)),
            recv_window_ms: DEFAULT_RECV_WINDOW_MS,
            retry_manager,
            cancellation_token: CancellationToken::new(),
            instruments: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    fn default_headers() -> HashMap<String, String> {
        HashMap::from([
            (USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string()),
            ("Referer".to_string(), BYBIT_NAUTILUS_BROKER_ID.to_string()),
        ])
    }

    fn rate_limiter_quotas() -> Vec<(String, Quota)> {
        vec![(BYBIT_GLOBAL_RATE_KEY.to_string(), *BYBIT_REST_QUOTA)]
    }

    fn rate_limit_keys(endpoint: &str) -> Vec<String> {
        let normalized = endpoint.split('?').next().unwrap_or(endpoint);
        let route = format!("bybit:{normalized}");

        vec![BYBIT_GLOBAL_RATE_KEY.to_string(), route]
    }

    fn sign_request(
        &self,
        timestamp: &str,
        params: Option<&str>,
    ) -> Result<HashMap<String, String>, BybitHttpError> {
        let credential = self
            .credential
            .as_ref()
            .ok_or(BybitHttpError::MissingCredentials)?;

        let signature = credential.sign_with_payload(timestamp, self.recv_window_ms, params);

        let mut headers = HashMap::new();
        headers.insert(
            "X-BAPI-API-KEY".to_string(),
            credential.api_key().to_string(),
        );
        headers.insert("X-BAPI-TIMESTAMP".to_string(), timestamp.to_string());
        headers.insert("X-BAPI-SIGN".to_string(), signature);
        headers.insert(
            "X-BAPI-RECV-WINDOW".to_string(),
            self.recv_window_ms.to_string(),
        );

        Ok(headers)
    }

    async fn send_request<T: DeserializeOwned>(
        &self,
        method: Method,
        endpoint: &str,
        body: Option<Vec<u8>>,
        authenticate: bool,
    ) -> Result<T, BybitHttpError> {
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

                if authenticate {
                    let timestamp = get_atomic_clock_realtime().get_time_ms().to_string();
                    let params_str = if method == Method::GET {
                        endpoint.split('?').nth(1)
                    } else {
                        body.as_ref().and_then(|b| std::str::from_utf8(b).ok())
                    };

                    let auth_headers = self.sign_request(&timestamp, params_str)?;
                    headers.extend(auth_headers);
                }

                if method == Method::POST || method == Method::PUT {
                    headers.insert("Content-Type".to_string(), "application/json".to_string());
                }

                let rate_limit_keys = Self::rate_limit_keys(&endpoint);

                let response = self
                    .client
                    .request(
                        method,
                        url,
                        Some(headers),
                        body,
                        None,
                        Some(rate_limit_keys),
                    )
                    .await?;

                if response.status.as_u16() >= 400 {
                    let body = String::from_utf8_lossy(&response.body).to_string();
                    return Err(BybitHttpError::UnexpectedStatus {
                        status: response.status.as_u16(),
                        body,
                    });
                }

                // Parse as BybitResponse to check retCode
                let bybit_response: BybitResponse<serde_json::Value> =
                    serde_json::from_slice(&response.body)?;

                if bybit_response.ret_code != 0 {
                    return Err(BybitHttpError::BybitError {
                        error_code: bybit_response.ret_code as i32,
                        message: bybit_response.ret_msg,
                    });
                }

                // Deserialize the full response
                let result: T = serde_json::from_slice(&response.body)?;
                Ok(result)
            }
        };

        let should_retry = |error: &BybitHttpError| -> bool {
            match error {
                BybitHttpError::NetworkError(_) => true,
                BybitHttpError::UnexpectedStatus { status, .. } => *status >= 500,
                _ => false,
            }
        };

        let create_error = |msg: String| -> BybitHttpError {
            if msg == "canceled" {
                BybitHttpError::NetworkError("Request canceled".to_string())
            } else {
                BybitHttpError::NetworkError(msg)
            }
        };

        self.retry_manager
            .execute_with_retry_with_cancel(
                endpoint.as_str(),
                operation,
                should_retry,
                create_error,
                &self.cancellation_token,
            )
            .await
    }

    fn build_path<S: Serialize>(base: &str, params: &S) -> Result<String, BybitHttpError> {
        let query = serde_urlencoded::to_string(params)
            .map_err(|e| BybitHttpError::JsonError(e.to_string()))?;
        if query.is_empty() {
            Ok(base.to_owned())
        } else {
            Ok(format!("{base}?{query}"))
        }
    }

    // =========================================================================
    // Low-level HTTP API methods
    // =========================================================================

    /// Fetches the current server time from Bybit.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/time>
    pub async fn http_get_server_time(&self) -> Result<BybitServerTimeResponse, BybitHttpError> {
        self.send_request(Method::GET, "/v5/market/time", None, false)
            .await
    }

    /// Fetches instrument information from Bybit for a given product category.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/instruments-info>
    pub async fn http_get_instruments<T: DeserializeOwned>(
        &self,
        params: &BybitInstrumentsInfoParams,
    ) -> Result<T, BybitHttpError> {
        let path = Self::build_path("/v5/market/instruments-info", params)?;
        self.send_request(Method::GET, &path, None, false).await
    }

    /// Fetches spot instrument information from Bybit.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/instruments-info>
    pub async fn http_get_instruments_spot(
        &self,
        params: &BybitInstrumentsInfoParams,
    ) -> Result<BybitInstrumentSpotResponse, BybitHttpError> {
        self.http_get_instruments(params).await
    }

    /// Fetches linear instrument information from Bybit.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/instruments-info>
    pub async fn http_get_instruments_linear(
        &self,
        params: &BybitInstrumentsInfoParams,
    ) -> Result<BybitInstrumentLinearResponse, BybitHttpError> {
        self.http_get_instruments(params).await
    }

    /// Fetches inverse instrument information from Bybit.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/instruments-info>
    pub async fn http_get_instruments_inverse(
        &self,
        params: &BybitInstrumentsInfoParams,
    ) -> Result<BybitInstrumentInverseResponse, BybitHttpError> {
        self.http_get_instruments(params).await
    }

    /// Fetches option instrument information from Bybit.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/instruments-info>
    pub async fn http_get_instruments_option(
        &self,
        params: &BybitInstrumentsInfoParams,
    ) -> Result<BybitInstrumentOptionResponse, BybitHttpError> {
        self.http_get_instruments(params).await
    }

    /// Fetches kline/candlestick data from Bybit.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/kline>
    pub async fn http_get_klines(
        &self,
        params: &BybitKlinesParams,
    ) -> Result<BybitKlinesResponse, BybitHttpError> {
        let path = Self::build_path("/v5/market/kline", params)?;
        self.send_request(Method::GET, &path, None, false).await
    }

    /// Fetches recent trades from Bybit.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/recent-trade>
    pub async fn http_get_recent_trades(
        &self,
        params: &BybitTradesParams,
    ) -> Result<BybitTradesResponse, BybitHttpError> {
        let path = Self::build_path("/v5/market/recent-trade", params)?;
        self.send_request(Method::GET, &path, None, false).await
    }

    /// Fetches open orders (requires authentication).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/order/open-order>
    pub async fn http_get_open_orders(
        &self,
        category: BybitProductType,
        symbol: Option<&str>,
    ) -> Result<BybitOpenOrdersResponse, BybitHttpError> {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Params<'a> {
            category: BybitProductType,
            #[serde(skip_serializing_if = "Option::is_none")]
            symbol: Option<&'a str>,
        }

        let params = Params { category, symbol };
        let path = Self::build_path("/v5/order/realtime", &params)?;
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Places a new order (requires authentication).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/order/create-order>
    pub async fn http_place_order(
        &self,
        request: &serde_json::Value,
    ) -> Result<BybitPlaceOrderResponse, BybitHttpError> {
        let body = serde_json::to_vec(request)?;
        self.send_request(Method::POST, "/v5/order/create", Some(body), true)
            .await
    }

    /// Fetches wallet balance (requires authentication).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/account/wallet-balance>
    pub async fn http_get_wallet_balance(
        &self,
        params: &super::query::BybitWalletBalanceParams,
    ) -> Result<super::models::BybitWalletBalanceResponse, BybitHttpError> {
        let path = Self::build_path("/v5/account/wallet-balance", params)?;
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Fetches trading fee rates for symbols.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/account/fee-rate>
    pub async fn http_get_fee_rate(
        &self,
        params: &super::query::BybitFeeRateParams,
    ) -> Result<super::models::BybitFeeRateResponse, BybitHttpError> {
        let path = Self::build_path("/v5/account/fee-rate", params)?;
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Fetches tickers for market data.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/tickers>
    pub async fn http_get_tickers<T: DeserializeOwned>(
        &self,
        params: &super::query::BybitTickersParams,
    ) -> Result<T, BybitHttpError> {
        let path = Self::build_path("/v5/market/tickers", params)?;
        self.send_request(Method::GET, &path, None, false).await
    }

    /// Fetches trade execution history (requires authentication).
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/order/execution>
    pub async fn http_get_trade_history(
        &self,
        params: &super::query::BybitTradeHistoryParams,
    ) -> Result<super::models::BybitTradeHistoryResponse, BybitHttpError> {
        let path = Self::build_path("/v5/execution/list", params)?;
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Fetches position information (requires authentication).
    ///
    /// # Errors
    ///
    /// This function returns an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - The API returns an error.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/position/position-info>
    pub async fn http_get_positions(
        &self,
        params: &super::query::BybitPositionListParams,
    ) -> Result<super::models::BybitPositionListResponse, BybitHttpError> {
        let path = Self::build_path("/v5/position/list", params)?;
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Returns the base URL used for requests.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Returns the configured receive window in milliseconds.
    #[must_use]
    pub fn recv_window_ms(&self) -> u64 {
        self.recv_window_ms
    }

    /// Returns the API credential if configured.
    #[must_use]
    pub fn credential(&self) -> Option<&Credential> {
        self.credential.as_ref()
    }

    /// Add an instrument to the cache.
    ///
    /// # Panics
    ///
    /// Panics if the instruments cache mutex is poisoned.
    pub fn add_instrument(&self, instrument: InstrumentAny) {
        let mut cache = self.instruments.lock().unwrap();
        let symbol = Ustr::from(instrument.id().symbol.as_str());
        cache.insert(symbol, instrument);
    }

    /// Get an instrument from the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument is not found in the cache.
    ///
    /// # Panics
    ///
    /// Panics if the instruments cache mutex is poisoned.
    pub fn instrument_from_cache(&self, symbol: &str) -> anyhow::Result<InstrumentAny> {
        let symbol_ustr = Ustr::from(symbol);
        let cache = self.instruments.lock().unwrap();
        cache.get(&symbol_ustr).cloned().ok_or_else(|| {
            anyhow::anyhow!(
                "Instrument {symbol} not found in cache, ensure instruments loaded first"
            )
        })
    }

    /// Generate a timestamp for initialization.
    #[must_use]
    pub fn generate_ts_init(&self) -> UnixNanos {
        get_atomic_clock_realtime().get_time_ns()
    }

    // =========================================================================
    // High-level domain methods
    // =========================================================================

    /// Submit a new order.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - Order validation fails.
    /// - The order is rejected.
    /// - The API returns an error.
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_order(
        &self,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: TimeInForce,
        price: Option<Price>,
        reduce_only: bool,
    ) -> anyhow::Result<OrderStatusReport> {
        let instrument = self.instrument_from_cache(instrument_id.symbol.as_str())?;

        let bybit_side = match order_side {
            OrderSide::Buy => BybitOrderSide::Buy,
            OrderSide::Sell => BybitOrderSide::Sell,
            _ => anyhow::bail!("Invalid order side: {order_side:?}"),
        };

        let bybit_order_type = match order_type {
            OrderType::Market => BybitOrderType::Market,
            OrderType::Limit => BybitOrderType::Limit,
            _ => anyhow::bail!("Unsupported order type: {order_type:?}"),
        };

        let bybit_tif = match time_in_force {
            TimeInForce::Gtc => BybitTimeInForce::Gtc,
            TimeInForce::Ioc => BybitTimeInForce::Ioc,
            TimeInForce::Fok => BybitTimeInForce::Fok,
            _ => anyhow::bail!("Unsupported time in force: {time_in_force:?}"),
        };

        let mut order_entry = BybitBatchPlaceOrderEntryBuilder::default();
        order_entry.symbol(instrument_id.symbol.as_str().to_string());
        order_entry.side(bybit_side);
        order_entry.order_type(bybit_order_type);
        order_entry.qty(quantity.to_string());
        order_entry.time_in_force(Some(bybit_tif));
        order_entry.order_link_id(Some(client_order_id.to_string()));

        if let Some(price) = price {
            order_entry.price(Some(price.to_string()));
        }

        if reduce_only {
            order_entry.reduce_only(Some(true));
        }

        let order_entry = order_entry.build().map_err(|e| anyhow::anyhow!(e))?;

        let mut params = BybitPlaceOrderParamsBuilder::default();
        params.category(product_type);
        params.order(order_entry);

        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let body = serde_json::to_value(&params)?;
        let response = self.http_place_order(&body).await?;

        let order_id = response
            .result
            .order_id
            .ok_or_else(|| anyhow::anyhow!("No order_id in response"))?;

        // Query the order to get full details
        let mut query_params = super::query::BybitOpenOrdersParamsBuilder::default();
        query_params.category(product_type);
        query_params.order_id(Some(order_id.as_str().to_string()));

        let query_params = query_params.build().map_err(|e| anyhow::anyhow!(e))?;
        let path = Self::build_path("/v5/order/realtime", &query_params)?;
        let order_response: super::models::BybitOpenOrdersResponse =
            self.send_request(Method::GET, &path, None, true).await?;

        let order = order_response
            .result
            .list
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No order returned after submission"))?;

        if order.order_status == crate::common::enums::BybitOrderStatus::Rejected {
            anyhow::bail!("Order rejected: {}", order.reject_reason);
        }

        let account_id = AccountId::new("BYBIT");
        let ts_init = self.generate_ts_init();

        parse_order_status_report(&order, &instrument, account_id, ts_init)
    }

    /// Cancel an order.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - The order doesn't exist.
    /// - The API returns an error.
    pub async fn cancel_order(
        &self,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> anyhow::Result<OrderStatusReport> {
        let instrument = self.instrument_from_cache(instrument_id.symbol.as_str())?;

        let mut cancel_entry = BybitBatchCancelOrderEntryBuilder::default();
        cancel_entry.symbol(instrument_id.symbol.as_str().to_string());

        if let Some(venue_order_id) = venue_order_id {
            cancel_entry.order_id(Some(venue_order_id.to_string()));
        } else if let Some(client_order_id) = client_order_id {
            cancel_entry.order_link_id(Some(client_order_id.to_string()));
        } else {
            anyhow::bail!("Either client_order_id or venue_order_id must be provided");
        }

        let cancel_entry = cancel_entry.build().map_err(|e| anyhow::anyhow!(e))?;

        let mut params = BybitCancelOrderParamsBuilder::default();
        params.category(product_type);
        params.order(cancel_entry);

        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;
        let body = serde_json::to_vec(&params)?;

        let response: super::models::BybitPlaceOrderResponse = self
            .send_request(Method::POST, "/v5/order/cancel", Some(body), true)
            .await?;

        let order_id = response
            .result
            .order_id
            .ok_or_else(|| anyhow::anyhow!("No order_id in cancel response"))?;

        // Query the order to get full details after cancellation
        let mut query_params = super::query::BybitOpenOrdersParamsBuilder::default();
        query_params.category(product_type);
        query_params.order_id(Some(order_id.as_str().to_string()));

        let query_params = query_params.build().map_err(|e| anyhow::anyhow!(e))?;
        let path = Self::build_path("/v5/order/history", &query_params)?;
        let order_response: super::models::BybitOrderHistoryResponse =
            self.send_request(Method::GET, &path, None, true).await?;

        let order = order_response
            .result
            .list
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No order returned in cancel response"))?;

        let account_id = AccountId::new("BYBIT");
        let ts_init = self.generate_ts_init();

        parse_order_status_report(&order, &instrument, account_id, ts_init)
    }

    /// Cancel all orders for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - The API returns an error.
    pub async fn cancel_all_orders(
        &self,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let instrument = self.instrument_from_cache(instrument_id.symbol.as_str())?;

        let mut params = BybitCancelAllOrdersParamsBuilder::default();
        params.category(product_type);
        params.symbol(Some(instrument_id.symbol.as_str().to_string()));

        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;
        let body = serde_json::to_vec(&params)?;

        let _response: crate::common::models::BybitListResponse<serde_json::Value> = self
            .send_request(Method::POST, "/v5/order/cancel-all", Some(body), true)
            .await?;

        // Query the order history to get all canceled orders
        let mut query_params = super::query::BybitOrderHistoryParamsBuilder::default();
        query_params.category(product_type);
        query_params.symbol(Some(instrument_id.symbol.as_str().to_string()));
        query_params.limit(Some(50));

        let query_params = query_params.build().map_err(|e| anyhow::anyhow!(e))?;
        let path = Self::build_path("/v5/order/history", &query_params)?;
        let order_response: super::models::BybitOrderHistoryResponse =
            self.send_request(Method::GET, &path, None, true).await?;

        let account_id = AccountId::new("BYBIT");
        let ts_init = self.generate_ts_init();

        let mut reports = Vec::new();
        for order in order_response.result.list {
            if let Ok(report) = parse_order_status_report(&order, &instrument, account_id, ts_init)
            {
                reports.push(report);
            }
        }

        Ok(reports)
    }

    /// Modify an existing order.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - The order doesn't exist.
    /// - The order is already closed.
    /// - The API returns an error.
    pub async fn modify_order(
        &self,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
        quantity: Option<Quantity>,
        price: Option<Price>,
    ) -> anyhow::Result<OrderStatusReport> {
        let instrument = self.instrument_from_cache(instrument_id.symbol.as_str())?;

        let mut amend_entry = BybitBatchAmendOrderEntryBuilder::default();
        amend_entry.symbol(instrument_id.symbol.as_str().to_string());

        if let Some(venue_order_id) = venue_order_id {
            amend_entry.order_id(Some(venue_order_id.to_string()));
        } else if let Some(client_order_id) = client_order_id {
            amend_entry.order_link_id(Some(client_order_id.to_string()));
        } else {
            anyhow::bail!("Either client_order_id or venue_order_id must be provided");
        }

        if let Some(quantity) = quantity {
            amend_entry.qty(Some(quantity.to_string()));
        }

        if let Some(price) = price {
            amend_entry.price(Some(price.to_string()));
        }

        let amend_entry = amend_entry.build().map_err(|e| anyhow::anyhow!(e))?;

        let mut params = BybitAmendOrderParamsBuilder::default();
        params.category(product_type);
        params.order(amend_entry);

        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;
        let body = serde_json::to_vec(&params)?;

        let response: super::models::BybitPlaceOrderResponse = self
            .send_request(Method::POST, "/v5/order/amend", Some(body), true)
            .await?;

        let order_id = response
            .result
            .order_id
            .ok_or_else(|| anyhow::anyhow!("No order_id in amend response"))?;

        // Query the order to get full details after amendment
        let mut query_params = super::query::BybitOpenOrdersParamsBuilder::default();
        query_params.category(product_type);
        query_params.order_id(Some(order_id.as_str().to_string()));

        let query_params = query_params.build().map_err(|e| anyhow::anyhow!(e))?;
        let path = Self::build_path("/v5/order/realtime", &query_params)?;
        let order_response: super::models::BybitOpenOrdersResponse =
            self.send_request(Method::GET, &path, None, true).await?;

        let order = order_response
            .result
            .list
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No order returned after modification"))?;

        let account_id = AccountId::new("BYBIT");
        let ts_init = self.generate_ts_init();

        parse_order_status_report(&order, &instrument, account_id, ts_init)
    }

    /// Query a single order by client order ID or venue order ID.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - The API returns an error.
    pub async fn query_order(
        &self,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let instrument = self.instrument_from_cache(instrument_id.symbol.as_str())?;

        let mut params = super::query::BybitOpenOrdersParamsBuilder::default();
        params.category(product_type);

        if let Some(venue_order_id) = venue_order_id {
            params.order_id(Some(venue_order_id.to_string()));
        } else if let Some(client_order_id) = client_order_id {
            params.order_link_id(Some(client_order_id.to_string()));
        } else {
            anyhow::bail!("Either client_order_id or venue_order_id must be provided");
        }

        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;
        let path = Self::build_path("/v5/order/realtime", &params)?;

        let response: super::models::BybitOpenOrdersResponse =
            self.send_request(Method::GET, &path, None, true).await?;

        if response.result.list.is_empty() {
            return Ok(None);
        }

        let order = &response.result.list[0];
        let account_id = AccountId::new("BYBIT");
        let ts_init = self.generate_ts_init();

        let report = parse_order_status_report(order, &instrument, account_id, ts_init)?;
        Ok(Some(report))
    }

    /// Request multiple order status reports.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - The API returns an error.
    pub async fn request_order_status_reports(
        &self,
        product_type: BybitProductType,
        instrument_id: Option<InstrumentId>,
        open_only: bool,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let params = if open_only {
            let mut p = super::query::BybitOpenOrdersParamsBuilder::default();
            p.category(product_type);
            if let Some(instrument_id) = &instrument_id {
                p.symbol(Some(instrument_id.symbol.as_str().to_string()));
            }
            let params = p.build().map_err(|e| anyhow::anyhow!(e))?;
            let path = Self::build_path("/v5/order/realtime", &params)?;
            let response: super::models::BybitOpenOrdersResponse =
                self.send_request(Method::GET, &path, None, true).await?;
            response.result.list
        } else {
            let mut p = super::query::BybitOrderHistoryParamsBuilder::default();
            p.category(product_type);
            if let Some(instrument_id) = &instrument_id {
                p.symbol(Some(instrument_id.symbol.as_str().to_string()));
            }
            if let Some(limit) = limit {
                p.limit(Some(limit));
            }
            let params = p.build().map_err(|e| anyhow::anyhow!(e))?;
            let path = Self::build_path("/v5/order/history", &params)?;
            let response: super::models::BybitOrderHistoryResponse =
                self.send_request(Method::GET, &path, None, true).await?;
            response.result.list
        };

        let account_id = AccountId::new("BYBIT");
        let ts_init = self.generate_ts_init();

        let mut reports = Vec::new();
        for order in params {
            if let Some(ref instrument_id) = instrument_id {
                let instrument = self.instrument_from_cache(instrument_id.symbol.as_str())?;
                if let Ok(report) =
                    parse_order_status_report(&order, &instrument, account_id, ts_init)
                {
                    reports.push(report);
                }
            } else {
                // Try to get instrument from symbol
                if let Ok(instrument) = self.instrument_from_cache(order.symbol.as_str())
                    && let Ok(report) =
                        parse_order_status_report(&order, &instrument, account_id, ts_init)
                {
                    reports.push(report);
                }
            }
        }

        Ok(reports)
    }

    /// Request instruments for a given product type.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The request fails.
    /// - Parsing fails.
    pub async fn request_instruments(
        &self,
        product_type: BybitProductType,
        symbol: Option<String>,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let ts_init = self.generate_ts_init();

        let params = super::query::BybitInstrumentsInfoParams {
            category: product_type,
            symbol,
            status: None,
            base_coin: None,
            limit: None,
            cursor: None,
        };

        let mut instruments = Vec::new();

        match product_type {
            BybitProductType::Spot => {
                let response: super::models::BybitInstrumentSpotResponse =
                    self.http_get_instruments(&params).await?;

                // Get fee rates for all symbols
                let mut fee_params = super::query::BybitFeeRateParamsBuilder::default();
                fee_params.category(product_type);
                let fee_params = fee_params.build().map_err(|e| anyhow::anyhow!(e))?;
                let fee_response = self.http_get_fee_rate(&fee_params).await?;

                let fee_map: std::collections::HashMap<_, _> = fee_response
                    .result
                    .list
                    .into_iter()
                    .map(|f| (f.symbol, f))
                    .collect();

                for definition in response.result.list {
                    if let Some(fee_rate) = fee_map.get(&definition.symbol)
                        && let Ok(instrument) =
                            parse_spot_instrument(&definition, fee_rate, ts_init, ts_init)
                    {
                        instruments.push(instrument);
                    }
                }
            }
            BybitProductType::Linear => {
                let response: super::models::BybitInstrumentLinearResponse =
                    self.http_get_instruments(&params).await?;

                let mut fee_params = super::query::BybitFeeRateParamsBuilder::default();
                fee_params.category(product_type);
                let fee_params = fee_params.build().map_err(|e| anyhow::anyhow!(e))?;
                let fee_response = self.http_get_fee_rate(&fee_params).await?;

                let fee_map: std::collections::HashMap<_, _> = fee_response
                    .result
                    .list
                    .into_iter()
                    .map(|f| (f.symbol, f))
                    .collect();

                for definition in response.result.list {
                    if let Some(fee_rate) = fee_map.get(&definition.symbol)
                        && let Ok(instrument) =
                            parse_linear_instrument(&definition, fee_rate, ts_init, ts_init)
                    {
                        instruments.push(instrument);
                    }
                }
            }
            BybitProductType::Inverse => {
                let response: super::models::BybitInstrumentInverseResponse =
                    self.http_get_instruments(&params).await?;

                let mut fee_params = super::query::BybitFeeRateParamsBuilder::default();
                fee_params.category(product_type);
                let fee_params = fee_params.build().map_err(|e| anyhow::anyhow!(e))?;
                let fee_response = self.http_get_fee_rate(&fee_params).await?;

                let fee_map: std::collections::HashMap<_, _> = fee_response
                    .result
                    .list
                    .into_iter()
                    .map(|f| (f.symbol, f))
                    .collect();

                for definition in response.result.list {
                    if let Some(fee_rate) = fee_map.get(&definition.symbol)
                        && let Ok(instrument) =
                            parse_inverse_instrument(&definition, fee_rate, ts_init, ts_init)
                    {
                        instruments.push(instrument);
                    }
                }
            }
            BybitProductType::Option => {
                let response: super::models::BybitInstrumentOptionResponse =
                    self.http_get_instruments(&params).await?;

                for definition in response.result.list {
                    if let Ok(instrument) = parse_option_instrument(&definition, ts_init, ts_init) {
                        instruments.push(instrument);
                    }
                }
            }
        }

        // Add all instruments to cache
        for instrument in &instruments {
            self.add_instrument(instrument.clone());
        }

        Ok(instruments)
    }

    /// Request trade tick history for a given symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The instrument is not found in cache.
    /// - The request fails.
    /// - Parsing fails.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/market/recent-trade>
    pub async fn request_trades(
        &self,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<TradeTick>> {
        let instrument = self.instrument_from_cache(instrument_id.symbol.as_str())?;

        let mut params_builder = super::query::BybitTradesParamsBuilder::default();
        params_builder.category(product_type);
        params_builder.symbol(instrument_id.symbol.as_str().to_string());
        if let Some(limit_val) = limit {
            params_builder.limit(limit_val);
        }

        let params = params_builder.build().map_err(|e| anyhow::anyhow!(e))?;
        let response = self.http_get_recent_trades(&params).await?;

        let ts_init = self.generate_ts_init();
        let mut trades = Vec::new();

        for trade in response.result.list {
            if let Ok(trade_tick) = parse_trade_tick(&trade, &instrument, ts_init) {
                trades.push(trade_tick);
            }
        }

        Ok(trades)
    }

    /// Request bar/kline history for a given symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The instrument is not found in cache.
    /// - The request fails.
    /// - Parsing fails.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/market/kline>
    pub async fn request_bars(
        &self,
        product_type: BybitProductType,
        bar_type: BarType,
        start: Option<i64>,
        end: Option<i64>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<Bar>> {
        let instrument = self.instrument_from_cache(bar_type.instrument_id().symbol.as_str())?;

        // Convert Nautilus BarAggregation to BybitKlineInterval
        let interval = match bar_type.spec().aggregation {
            BarAggregation::Minute => BybitKlineInterval::Minute1,
            BarAggregation::Hour => BybitKlineInterval::Hour1,
            BarAggregation::Day => BybitKlineInterval::Day1,
            _ => anyhow::bail!(
                "Unsupported bar aggregation: {:?}",
                bar_type.spec().aggregation
            ),
        };

        let mut params_builder = super::query::BybitKlinesParamsBuilder::default();
        params_builder.category(product_type);
        params_builder.symbol(bar_type.instrument_id().symbol.as_str().to_string());
        params_builder.interval(interval);

        if let Some(start_ts) = start {
            params_builder.start(start_ts);
        }
        if let Some(end_ts) = end {
            params_builder.end(end_ts);
        }
        if let Some(limit_val) = limit {
            params_builder.limit(limit_val);
        }

        let params = params_builder.build().map_err(|e| anyhow::anyhow!(e))?;
        let response = self.http_get_klines(&params).await?;

        let ts_init = self.generate_ts_init();
        let mut bars = Vec::new();

        for kline in response.result.list {
            if let Ok(bar) = parse_kline_bar(&kline, &instrument, bar_type, false, ts_init) {
                bars.push(bar);
            }
        }

        Ok(bars)
    }

    /// Fetches execution history (fills) for the account and returns a list of [`FillReport`]s.
    ///
    /// # Errors
    ///
    /// This function returns an error if:
    /// - Required instruments are not cached.
    /// - The instrument is not found in cache.
    /// - The request fails.
    /// - Parsing fails.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/order/execution>
    pub async fn request_fill_reports(
        &self,
        product_type: BybitProductType,
        instrument_id: Option<InstrumentId>,
        start: Option<i64>,
        end: Option<i64>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<FillReport>> {
        let account_id = AccountId::new("BYBIT");

        // Build query parameters
        let symbol = instrument_id.map(|id| id.symbol.as_str().to_string());
        let params = super::query::BybitTradeHistoryParams {
            category: product_type,
            symbol,
            base_coin: None,
            order_id: None,
            order_link_id: None,
            start_time: start,
            end_time: end,
            exec_type: None,
            limit,
            cursor: None,
        };

        let response = self.http_get_trade_history(&params).await?;
        let ts_init = self.generate_ts_init();
        let mut reports = Vec::new();

        for execution in response.result.list {
            // Get instrument for this execution
            let symbol_str = execution.symbol.as_str();
            let instrument = self.instrument_from_cache(symbol_str)?;

            if let Ok(report) = parse_fill_report(&execution, account_id, &instrument, ts_init) {
                reports.push(report);
            }
        }

        Ok(reports)
    }

    /// Fetches position information for the account and returns a list of [`PositionStatusReport`]s.
    ///
    /// # Errors
    ///
    /// This function returns an error if:
    /// - Required instruments are not cached.
    /// - The instrument is not found in cache.
    /// - The request fails.
    /// - Parsing fails.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/position/position-info>
    pub async fn request_position_status_reports(
        &self,
        product_type: BybitProductType,
        instrument_id: Option<InstrumentId>,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let account_id = AccountId::new("BYBIT");

        // Build query parameters
        let symbol = instrument_id.map(|id| id.symbol.as_str().to_string());
        let params = super::query::BybitPositionListParams {
            category: product_type,
            symbol,
            base_coin: None,
            settle_coin: None,
            limit: None,
            cursor: None,
        };

        let response = self.http_get_positions(&params).await?;
        let ts_init = self.generate_ts_init();
        let mut reports = Vec::new();

        for position in response.result.list {
            // Get instrument for this position
            let symbol_str = position.symbol.as_str();
            let instrument = self.instrument_from_cache(symbol_str)?;

            if let Ok(report) =
                parse_position_status_report(&position, account_id, &instrument, ts_init)
            {
                reports.push(report);
            }
        }

        Ok(reports)
    }

    /// Requests the current account state for the specified account type.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The request fails.
    /// - Parsing fails.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/account/wallet-balance>
    pub async fn request_account_state(
        &self,
        account_type: crate::common::enums::BybitAccountType,
    ) -> anyhow::Result<AccountState> {
        let account_id = AccountId::new("BYBIT");

        let params = super::query::BybitWalletBalanceParams {
            account_type,
            coin: None,
        };

        let response = self.http_get_wallet_balance(&params).await?;
        let ts_init = self.generate_ts_init();

        // Take the first wallet balance from the list
        let wallet_balance = response
            .result
            .list
            .first()
            .ok_or_else(|| anyhow::anyhow!("No wallet balance found in response"))?;

        parse_account_state(wallet_balance, account_id, ts_init)
    }

    /// Requests trading fee rates for the specified product type and optional filters.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The request fails.
    /// - Parsing fails.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/account/fee-rate>
    pub async fn request_fee_rates(
        &self,
        product_type: BybitProductType,
        symbol: Option<String>,
        base_coin: Option<String>,
    ) -> anyhow::Result<Vec<super::models::BybitFeeRate>> {
        let params = super::query::BybitFeeRateParams {
            category: product_type,
            symbol,
            base_coin,
        };

        let response = self.http_get_fee_rate(&params).await?;
        Ok(response.result.list)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Outer Client
////////////////////////////////////////////////////////////////////////////////

/// Provides a HTTP client for connecting to the [Bybit](https://bybit.com) REST API.
#[derive(Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct BybitHttpClient {
    pub(crate) inner: Arc<BybitHttpInnerClient>,
}

impl Default for BybitHttpClient {
    fn default() -> Self {
        Self::new(None, Some(60), None, None, None)
            .expect("Failed to create default BybitHttpClient")
    }
}

impl Debug for BybitHttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BybitHttpClient")
            .field("inner", &self.inner)
            .finish()
    }
}

impl BybitHttpClient {
    /// Creates a new [`BybitHttpClient`] using the default Bybit HTTP URL.
    ///
    /// # Errors
    ///
    /// Returns an error if the retry manager cannot be created.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        base_url: Option<String>,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
    ) -> Result<Self, BybitHttpError> {
        Ok(Self {
            inner: Arc::new(BybitHttpInnerClient::new(
                base_url,
                timeout_secs,
                max_retries,
                retry_delay_ms,
                retry_delay_max_ms,
            )?),
        })
    }

    /// Creates a new [`BybitHttpClient`] configured with credentials.
    ///
    /// # Errors
    ///
    /// Returns an error if the retry manager cannot be created.
    #[allow(clippy::too_many_arguments)]
    pub fn with_credentials(
        api_key: String,
        api_secret: String,
        base_url: Option<String>,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
    ) -> Result<Self, BybitHttpError> {
        Ok(Self {
            inner: Arc::new(BybitHttpInnerClient::with_credentials(
                api_key,
                api_secret,
                base_url,
                timeout_secs,
                max_retries,
                retry_delay_ms,
                retry_delay_max_ms,
            )?),
        })
    }

    /// Returns the base URL used for requests.
    #[must_use]
    pub fn base_url(&self) -> &str {
        self.inner.base_url()
    }

    /// Returns the configured receive window in milliseconds.
    #[must_use]
    pub fn recv_window_ms(&self) -> u64 {
        self.inner.recv_window_ms()
    }

    /// Returns the API credential if configured.
    #[must_use]
    pub fn credential(&self) -> Option<&Credential> {
        self.inner.credential()
    }

    /// Cancel all pending HTTP requests.
    pub fn cancel_all_requests(&self) {
        self.inner.cancel_all_requests();
    }

    /// Get the cancellation token for this client.
    pub fn cancellation_token(&self) -> &CancellationToken {
        self.inner.cancellation_token()
    }

    // =========================================================================
    // Low-level HTTP API methods
    // =========================================================================

    /// Fetches the current server time from Bybit.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The request fails.
    /// - The response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/time>
    pub async fn http_get_server_time(&self) -> Result<BybitServerTimeResponse, BybitHttpError> {
        self.inner.http_get_server_time().await
    }

    /// Fetches instrument information from Bybit for a given product category.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The request fails.
    /// - The response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/instruments-info>
    pub async fn http_get_instruments<T: DeserializeOwned>(
        &self,
        params: &BybitInstrumentsInfoParams,
    ) -> Result<T, BybitHttpError> {
        self.inner.http_get_instruments(params).await
    }

    /// Fetches spot instrument information from Bybit.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The request fails.
    /// - The response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/instruments-info>
    pub async fn http_get_instruments_spot(
        &self,
        params: &BybitInstrumentsInfoParams,
    ) -> Result<BybitInstrumentSpotResponse, BybitHttpError> {
        self.inner.http_get_instruments_spot(params).await
    }

    /// Fetches linear instrument information from Bybit.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The request fails.
    /// - The response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/instruments-info>
    pub async fn http_get_instruments_linear(
        &self,
        params: &BybitInstrumentsInfoParams,
    ) -> Result<BybitInstrumentLinearResponse, BybitHttpError> {
        self.inner.http_get_instruments_linear(params).await
    }

    /// Fetches inverse instrument information from Bybit.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The request fails.
    /// - The response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/instruments-info>
    pub async fn http_get_instruments_inverse(
        &self,
        params: &BybitInstrumentsInfoParams,
    ) -> Result<BybitInstrumentInverseResponse, BybitHttpError> {
        self.inner.http_get_instruments_inverse(params).await
    }

    /// Fetches option instrument information from Bybit.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The request fails.
    /// - The response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/instruments-info>
    pub async fn http_get_instruments_option(
        &self,
        params: &BybitInstrumentsInfoParams,
    ) -> Result<BybitInstrumentOptionResponse, BybitHttpError> {
        self.inner.http_get_instruments_option(params).await
    }

    /// Fetches kline/candlestick data from Bybit.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The request fails.
    /// - The response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/kline>
    pub async fn http_get_klines(
        &self,
        params: &BybitKlinesParams,
    ) -> Result<BybitKlinesResponse, BybitHttpError> {
        self.inner.http_get_klines(params).await
    }

    /// Fetches recent trades from Bybit.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The request fails.
    /// - The response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/market/recent-trade>
    pub async fn http_get_recent_trades(
        &self,
        params: &BybitTradesParams,
    ) -> Result<BybitTradesResponse, BybitHttpError> {
        self.inner.http_get_recent_trades(params).await
    }

    /// Fetches open orders (requires authentication).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The request fails.
    /// - The response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/order/open-order>
    pub async fn http_get_open_orders(
        &self,
        category: BybitProductType,
        symbol: Option<&str>,
    ) -> Result<BybitOpenOrdersResponse, BybitHttpError> {
        self.inner.http_get_open_orders(category, symbol).await
    }

    /// Places a new order (requires authentication).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The request fails.
    /// - The response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/order/create-order>
    pub async fn http_place_order(
        &self,
        request: &serde_json::Value,
    ) -> Result<BybitPlaceOrderResponse, BybitHttpError> {
        self.inner.http_place_order(request).await
    }

    // =========================================================================
    // High-level methods using Nautilus domain objects
    // =========================================================================

    /// Add an instrument to the cache.
    pub fn add_instrument(&self, instrument: InstrumentAny) {
        self.inner.add_instrument(instrument);
    }

    /// Submit a new order.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - Order validation fails.
    /// - The order is rejected.
    /// - The API returns an error.
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_order(
        &self,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: TimeInForce,
        price: Option<Price>,
        reduce_only: bool,
    ) -> anyhow::Result<OrderStatusReport> {
        self.inner
            .submit_order(
                product_type,
                instrument_id,
                client_order_id,
                order_side,
                order_type,
                quantity,
                time_in_force,
                price,
                reduce_only,
            )
            .await
    }

    /// Cancel an order.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - The order doesn't exist.
    /// - The API returns an error.
    pub async fn cancel_order(
        &self,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> anyhow::Result<OrderStatusReport> {
        self.inner
            .cancel_order(product_type, instrument_id, client_order_id, venue_order_id)
            .await
    }

    /// Cancel all orders for an instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - The API returns an error.
    pub async fn cancel_all_orders(
        &self,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        self.inner
            .cancel_all_orders(product_type, instrument_id)
            .await
    }

    /// Modify an existing order.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - The order doesn't exist.
    /// - The order is already closed.
    /// - The API returns an error.
    pub async fn modify_order(
        &self,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
        quantity: Option<Quantity>,
        price: Option<Price>,
    ) -> anyhow::Result<OrderStatusReport> {
        self.inner
            .modify_order(
                product_type,
                instrument_id,
                client_order_id,
                venue_order_id,
                quantity,
                price,
            )
            .await
    }

    /// Query a single order by client order ID or venue order ID.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - The API returns an error.
    pub async fn query_order(
        &self,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        self.inner
            .query_order(product_type, instrument_id, client_order_id, venue_order_id)
            .await
    }

    /// Request multiple order status reports.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - The API returns an error.
    pub async fn request_order_status_reports(
        &self,
        product_type: BybitProductType,
        instrument_id: Option<InstrumentId>,
        open_only: bool,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        self.inner
            .request_order_status_reports(product_type, instrument_id, open_only, limit)
            .await
    }

    /// Request instruments for a given product type.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The request fails.
    /// - Parsing fails.
    pub async fn request_instruments(
        &self,
        product_type: BybitProductType,
        symbol: Option<String>,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        self.inner.request_instruments(product_type, symbol).await
    }

    /// Request trade tick history for a given symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The instrument is not found in cache.
    /// - The request fails.
    /// - Parsing fails.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/market/recent-trade>
    pub async fn request_trades(
        &self,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<TradeTick>> {
        self.inner
            .request_trades(product_type, instrument_id, limit)
            .await
    }

    /// Request bar/kline history for a given symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The instrument is not found in cache.
    /// - The request fails.
    /// - Parsing fails.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/market/kline>
    pub async fn request_bars(
        &self,
        product_type: BybitProductType,
        bar_type: BarType,
        start: Option<i64>,
        end: Option<i64>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<Bar>> {
        self.inner
            .request_bars(product_type, bar_type, start, end, limit)
            .await
    }

    /// Fetches execution history (fills) for the account.
    ///
    /// # Errors
    ///
    /// This function returns an error if:
    /// - Required instruments are not cached.
    /// - The instrument is not found in cache.
    /// - The request fails.
    /// - Parsing fails.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/order/execution>
    pub async fn request_fill_reports(
        &self,
        product_type: BybitProductType,
        instrument_id: Option<InstrumentId>,
        start: Option<i64>,
        end: Option<i64>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<FillReport>> {
        self.inner
            .request_fill_reports(product_type, instrument_id, start, end, limit)
            .await
    }

    /// Fetches position information for the account.
    ///
    /// # Errors
    ///
    /// This function returns an error if:
    /// - Required instruments are not cached.
    /// - The instrument is not found in cache.
    /// - The request fails.
    /// - Parsing fails.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/position/position-info>
    pub async fn request_position_status_reports(
        &self,
        product_type: BybitProductType,
        instrument_id: Option<InstrumentId>,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        self.inner
            .request_position_status_reports(product_type, instrument_id)
            .await
    }

    /// Requests the current account state for the specified account type.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The request fails.
    /// - Parsing fails.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/account/wallet-balance>
    pub async fn request_account_state(
        &self,
        account_type: crate::common::enums::BybitAccountType,
    ) -> anyhow::Result<AccountState> {
        self.inner.request_account_state(account_type).await
    }

    /// Requests trading fee rates for the specified product type and optional filters.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The request fails.
    /// - Parsing fails.
    ///
    /// # References
    ///
    /// <https://bybit-exchange.github.io/docs/v5/account/fee-rate>
    pub async fn request_fee_rates(
        &self,
        product_type: BybitProductType,
        symbol: Option<String>,
        base_coin: Option<String>,
    ) -> anyhow::Result<Vec<super::models::BybitFeeRate>> {
        self.inner
            .request_fee_rates(product_type, symbol, base_coin)
            .await
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
    fn test_client_creation() {
        let client = BybitHttpClient::new(None, Some(60), None, None, None);
        assert!(client.is_ok());

        let client = client.unwrap();
        assert!(client.base_url().contains("bybit.com"));
        assert!(client.credential().is_none());
    }

    #[rstest]
    fn test_client_with_credentials() {
        let client = BybitHttpClient::with_credentials(
            "test_key".to_string(),
            "test_secret".to_string(),
            Some("https://api-testnet.bybit.com".to_string()),
            Some(60),
            None,
            None,
            None,
        );
        assert!(client.is_ok());

        let client = client.unwrap();
        assert!(client.credential().is_some());
    }

    #[rstest]
    fn test_build_path_with_params() {
        #[derive(Serialize)]
        struct TestParams {
            category: String,
            symbol: String,
        }

        let params = TestParams {
            category: "linear".to_string(),
            symbol: "BTCUSDT".to_string(),
        };

        let path = BybitHttpInnerClient::build_path("/v5/market/test", &params);
        assert!(path.is_ok());
        assert!(path.unwrap().contains("category=linear"));
    }

    #[rstest]
    fn test_build_path_without_params() {
        let params = ();
        let path = BybitHttpInnerClient::build_path("/v5/market/time", &params);
        assert!(path.is_ok());
        assert_eq!(path.unwrap(), "/v5/market/time");
    }
}
