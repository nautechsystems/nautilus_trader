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
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use chrono::{DateTime, Utc};
use nautilus_core::{
    MUTEX_POISONED, consts::NAUTILUS_USER_AGENT, nanos::UnixNanos, time::get_atomic_clock_realtime,
};
use nautilus_model::{
    data::{Bar, BarType, TradeTick},
    enums::{BarAggregation, OrderSide, OrderType, PositionSideSpecified, TimeInForce},
    events::account::state::AccountState,
    identifiers::{AccountId, ClientOrderId, InstrumentId, Symbol, VenueOrderId},
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
        BybitFeeRate, BybitFeeRateResponse, BybitInstrumentInverseResponse,
        BybitInstrumentLinearResponse, BybitInstrumentOptionResponse, BybitInstrumentSpotResponse,
        BybitKlinesResponse, BybitOpenOrdersResponse, BybitOrderHistoryResponse,
        BybitPlaceOrderResponse, BybitPositionListResponse, BybitServerTimeResponse,
        BybitSetLeverageResponse, BybitSetMarginModeResponse, BybitSwitchModeResponse,
        BybitTradeHistoryResponse, BybitTradesResponse, BybitWalletBalanceResponse,
    },
    query::{
        BybitAmendOrderParamsBuilder, BybitBatchAmendOrderEntryBuilder,
        BybitBatchCancelOrderEntryBuilder, BybitBatchCancelOrderParamsBuilder,
        BybitBatchPlaceOrderEntryBuilder, BybitCancelAllOrdersParamsBuilder,
        BybitCancelOrderParamsBuilder, BybitFeeRateParams, BybitInstrumentsInfoParams,
        BybitKlinesParams, BybitKlinesParamsBuilder, BybitOpenOrdersParamsBuilder,
        BybitOrderHistoryParamsBuilder, BybitPlaceOrderParamsBuilder, BybitPositionListParams,
        BybitSetLeverageParamsBuilder, BybitSetMarginModeParamsBuilder,
        BybitSwitchModeParamsBuilder, BybitTickersParams, BybitTradeHistoryParams,
        BybitTradesParams, BybitTradesParamsBuilder, BybitWalletBalanceParams,
    },
};
use crate::{
    common::{
        consts::BYBIT_NAUTILUS_BROKER_ID,
        credential::Credential,
        enums::{
            BybitAccountType, BybitEnvironment, BybitKlineInterval, BybitMarginMode,
            BybitOrderSide, BybitOrderType, BybitPositionMode, BybitProductType, BybitTimeInForce,
        },
        models::BybitResponse,
        parse::{
            make_bybit_symbol, parse_account_state, parse_fill_report, parse_inverse_instrument,
            parse_kline_bar, parse_linear_instrument, parse_option_instrument,
            parse_order_status_report, parse_position_status_report, parse_spot_instrument,
            parse_trade_tick,
        },
        symbol::BybitSymbol,
        urls::bybit_http_base_url,
    },
    http::query::BybitFeeRateParamsBuilder,
};

const DEFAULT_RECV_WINDOW_MS: u64 = 5_000;

/// Default Bybit REST API rate limit.
///
/// Bybit implements rate limiting per endpoint with varying limits.
/// We use a conservative 10 requests per second as a general default.
pub static BYBIT_REST_QUOTA: LazyLock<Quota> = LazyLock::new(|| {
    Quota::per_second(NonZeroU32::new(10).expect("Should be a valid non-zero u32"))
});

const BYBIT_GLOBAL_RATE_KEY: &str = "bybit:global";

/// Inner HTTP client implementation containing the actual HTTP logic.
pub struct BybitHttpInnerClient {
    base_url: String,
    client: HttpClient,
    credential: Option<Credential>,
    recv_window_ms: u64,
    retry_manager: RetryManager<BybitHttpError>,
    cancellation_token: CancellationToken,
    instruments_cache: Arc<Mutex<HashMap<Ustr, InstrumentAny>>>,
    use_spot_position_reports: AtomicBool,
}

impl Default for BybitHttpInnerClient {
    fn default() -> Self {
        Self::new(None, Some(60), None, None, None, None, None)
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
        recv_window_ms: Option<u64>,
        proxy_url: Option<String>,
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
                proxy_url,
            )
            .map_err(|e| {
                BybitHttpError::NetworkError(format!("Failed to create HTTP client: {e}"))
            })?,
            credential: None,
            recv_window_ms: recv_window_ms.unwrap_or(DEFAULT_RECV_WINDOW_MS),
            retry_manager,
            cancellation_token: CancellationToken::new(),
            instruments_cache: Arc::new(Mutex::new(HashMap::new())),
            use_spot_position_reports: AtomicBool::new(false),
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
        recv_window_ms: Option<u64>,
        proxy_url: Option<String>,
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
                proxy_url,
            )
            .map_err(|e| {
                BybitHttpError::NetworkError(format!("Failed to create HTTP client: {e}"))
            })?,
            credential: Some(Credential::new(api_key, api_secret)),
            recv_window_ms: recv_window_ms.unwrap_or(DEFAULT_RECV_WINDOW_MS),
            retry_manager,
            cancellation_token: CancellationToken::new(),
            instruments_cache: Arc::new(Mutex::new(HashMap::new())),
            use_spot_position_reports: AtomicBool::new(false),
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
                BybitHttpError::Canceled("Adapter disconnecting or shutting down".to_string())
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
    /// - <https://bybit-exchange.github.io/docs/v5/market/instrument>
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
    /// - <https://bybit-exchange.github.io/docs/v5/market/instrument>
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
    /// - <https://bybit-exchange.github.io/docs/v5/market/instrument>
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
    /// - <https://bybit-exchange.github.io/docs/v5/market/instrument>
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
    /// - <https://bybit-exchange.github.io/docs/v5/market/instrument>
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
        params: &BybitWalletBalanceParams,
    ) -> Result<BybitWalletBalanceResponse, BybitHttpError> {
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
        params: &BybitFeeRateParams,
    ) -> Result<BybitFeeRateResponse, BybitHttpError> {
        let path = Self::build_path("/v5/account/fee-rate", params)?;
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Sets the margin mode for the account.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - The API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if required parameters are not provided (should not happen with current implementation).
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/account/set-margin-mode>
    pub async fn http_set_margin_mode(
        &self,
        margin_mode: BybitMarginMode,
    ) -> Result<BybitSetMarginModeResponse, BybitHttpError> {
        let params = BybitSetMarginModeParamsBuilder::default()
            .set_margin_mode(margin_mode)
            .build()
            .expect("Failed to build BybitSetMarginModeParams");

        let body = serde_json::to_vec(&params)?;
        self.send_request(
            Method::POST,
            "/v5/account/set-margin-mode",
            Some(body),
            true,
        )
        .await
    }

    /// Sets leverage for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - The API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if required parameters are not provided (should not happen with current implementation).
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/position/leverage>
    pub async fn http_set_leverage(
        &self,
        product_type: BybitProductType,
        symbol: &str,
        buy_leverage: &str,
        sell_leverage: &str,
    ) -> Result<BybitSetLeverageResponse, BybitHttpError> {
        let params = BybitSetLeverageParamsBuilder::default()
            .category(product_type)
            .symbol(symbol.to_string())
            .buy_leverage(buy_leverage.to_string())
            .sell_leverage(sell_leverage.to_string())
            .build()
            .expect("Failed to build BybitSetLeverageParams");

        let body = serde_json::to_vec(&params)?;
        self.send_request(Method::POST, "/v5/position/set-leverage", Some(body), true)
            .await
    }

    /// Switches position mode for a product type.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - The API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if required parameters are not provided (should not happen with current implementation).
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/position/position-mode>
    pub async fn http_switch_mode(
        &self,
        product_type: BybitProductType,
        mode: BybitPositionMode,
        symbol: Option<String>,
        coin: Option<String>,
    ) -> Result<BybitSwitchModeResponse, BybitHttpError> {
        let mut builder = BybitSwitchModeParamsBuilder::default();
        builder.category(product_type);
        builder.mode(mode);

        if let Some(s) = symbol {
            builder.symbol(s);
        }
        if let Some(c) = coin {
            builder.coin(c);
        }

        let params = builder
            .build()
            .expect("Failed to build BybitSwitchModeParams");

        let body = serde_json::to_vec(&params)?;
        self.send_request(Method::POST, "/v5/position/switch-mode", Some(body), true)
            .await
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
        params: &BybitTickersParams,
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
        params: &BybitTradeHistoryParams,
    ) -> Result<BybitTradeHistoryResponse, BybitHttpError> {
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
    /// - <https://bybit-exchange.github.io/docs/v5/position>
    pub async fn http_get_positions(
        &self,
        params: &BybitPositionListParams,
    ) -> Result<BybitPositionListResponse, BybitHttpError> {
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
        let mut cache = self.instruments_cache.lock().expect(MUTEX_POISONED);
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
    pub fn instrument_from_cache(&self, symbol: &Symbol) -> anyhow::Result<InstrumentAny> {
        let cache = self.instruments_cache.lock().expect(MUTEX_POISONED);
        cache.get(&symbol.inner()).cloned().ok_or_else(|| {
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

    /// Set whether to generate position reports from wallet balances for SPOT positions.
    pub fn set_use_spot_position_reports(&self, use_spot_position_reports: bool) {
        self.use_spot_position_reports
            .store(use_spot_position_reports, Ordering::Relaxed);
    }

    /// Generate SPOT position reports from wallet balances.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The wallet balance request fails.
    /// - Parsing fails.
    async fn generate_spot_position_reports_from_wallet(
        &self,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        use std::str::FromStr;

        use rust_decimal::Decimal;

        let params = BybitWalletBalanceParams {
            account_type: BybitAccountType::Unified,
            coin: None,
        };

        let response = self.http_get_wallet_balance(&params).await?;
        let ts_init = self.generate_ts_init();

        // Build lookup table of wallet balances by coin
        let mut wallet_by_coin: HashMap<Ustr, Decimal> = HashMap::new();

        for wallet in &response.result.list {
            for coin_balance in &wallet.coin {
                let balance =
                    Decimal::from_str(&coin_balance.wallet_balance).unwrap_or(Decimal::ZERO);
                *wallet_by_coin
                    .entry(coin_balance.coin)
                    .or_insert(Decimal::ZERO) += balance;
            }
        }

        let mut reports = Vec::new();
        let cache = self.instruments_cache.lock().unwrap();

        if let Some(instrument_id) = instrument_id {
            // Generate report for specific instrument
            if let Some(instrument) = cache.get(&instrument_id.symbol.inner()) {
                let base_currency = instrument
                    .base_currency()
                    .expect("SPOT instrument should have base currency");
                let coin = base_currency.code;
                let wallet_balance = wallet_by_coin.get(&coin).copied().unwrap_or(Decimal::ZERO);

                // Handle negative balances (borrowed assets) by using absolute value
                let balance_f64 = wallet_balance.to_string().parse::<f64>().unwrap_or(0.0);
                let quantity = Quantity::new(balance_f64.abs(), instrument.size_precision());

                let side = if balance_f64 > 0.0 {
                    PositionSideSpecified::Long
                } else if balance_f64 < 0.0 {
                    PositionSideSpecified::Short
                } else {
                    PositionSideSpecified::Flat
                };

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
            // Generate reports for all SPOT instruments with non-zero balance
            for (symbol, instrument) in cache.iter() {
                // Only consider SPOT instruments
                if !symbol.as_str().ends_with("-SPOT") {
                    continue;
                }

                let base_currency = match instrument.base_currency() {
                    Some(currency) => currency,
                    None => continue,
                };

                let coin = base_currency.code;
                let wallet_balance = wallet_by_coin.get(&coin).copied().unwrap_or(Decimal::ZERO);

                if wallet_balance.is_zero() {
                    continue;
                }

                // Handle negative balances (borrowed assets) by using absolute value
                let balance_f64 = wallet_balance.to_string().parse::<f64>().unwrap_or(0.0);
                let quantity = Quantity::new(balance_f64.abs(), instrument.size_precision());

                if quantity.raw == 0 {
                    continue;
                }

                let side = if balance_f64 > 0.0 {
                    PositionSideSpecified::Long
                } else if balance_f64 < 0.0 {
                    PositionSideSpecified::Short
                } else {
                    PositionSideSpecified::Flat
                };

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
        account_id: AccountId,
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
        let instrument = self.instrument_from_cache(&instrument_id.symbol)?;
        let bybit_symbol = BybitSymbol::new(instrument_id.symbol.as_str())?;

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
        order_entry.symbol(bybit_symbol.raw_symbol().to_string());
        order_entry.side(bybit_side);
        order_entry.order_type(bybit_order_type);
        order_entry.qty(quantity.to_string());
        order_entry.time_in_force(Some(bybit_tif));
        order_entry.order_link_id(client_order_id.to_string());

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
        let mut query_params = BybitOpenOrdersParamsBuilder::default();
        query_params.category(product_type);
        query_params.order_id(order_id.as_str().to_string());

        let query_params = query_params.build().map_err(|e| anyhow::anyhow!(e))?;
        let path = Self::build_path("/v5/order/realtime", &query_params)?;
        let order_response: BybitOpenOrdersResponse =
            self.send_request(Method::GET, &path, None, true).await?;

        let order = order_response
            .result
            .list
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No order returned after submission"))?;

        // Only bail on rejection if there are no fills
        // If the order has fills (cum_exec_qty > 0), let the parser remap Rejected -> Canceled
        if order.order_status == crate::common::enums::BybitOrderStatus::Rejected
            && (order.cum_exec_qty.as_str() == "0" || order.cum_exec_qty.is_empty())
        {
            anyhow::bail!("Order rejected: {}", order.reject_reason);
        }

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
        account_id: AccountId,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> anyhow::Result<OrderStatusReport> {
        let instrument = self.instrument_from_cache(&instrument_id.symbol)?;
        let bybit_symbol = BybitSymbol::new(instrument_id.symbol.as_str())?;

        let mut cancel_entry = BybitBatchCancelOrderEntryBuilder::default();
        cancel_entry.symbol(bybit_symbol.raw_symbol().to_string());

        if let Some(venue_order_id) = venue_order_id {
            cancel_entry.order_id(venue_order_id.to_string());
        } else if let Some(client_order_id) = client_order_id {
            cancel_entry.order_link_id(client_order_id.to_string());
        } else {
            anyhow::bail!("Either client_order_id or venue_order_id must be provided");
        }

        let cancel_entry = cancel_entry.build().map_err(|e| anyhow::anyhow!(e))?;

        let mut params = BybitCancelOrderParamsBuilder::default();
        params.category(product_type);
        params.order(cancel_entry);

        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;
        let body = serde_json::to_vec(&params)?;

        let response: BybitPlaceOrderResponse = self
            .send_request(Method::POST, "/v5/order/cancel", Some(body), true)
            .await?;

        let order_id = response
            .result
            .order_id
            .ok_or_else(|| anyhow::anyhow!("No order_id in cancel response"))?;

        // Query the order to get full details after cancellation
        let mut query_params = BybitOpenOrdersParamsBuilder::default();
        query_params.category(product_type);
        query_params.order_id(order_id.as_str().to_string());

        let query_params = query_params.build().map_err(|e| anyhow::anyhow!(e))?;
        let path = Self::build_path("/v5/order/history", &query_params)?;
        let order_response: BybitOrderHistoryResponse =
            self.send_request(Method::GET, &path, None, true).await?;

        let order = order_response
            .result
            .list
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No order returned in cancel response"))?;

        let ts_init = self.generate_ts_init();

        parse_order_status_report(&order, &instrument, account_id, ts_init)
    }

    /// Batch cancel multiple orders.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - Any of the orders don't exist.
    /// - The API returns an error.
    pub async fn batch_cancel_orders(
        &self,
        account_id: AccountId,
        product_type: BybitProductType,
        instrument_ids: Vec<InstrumentId>,
        client_order_ids: Vec<Option<ClientOrderId>>,
        venue_order_ids: Vec<Option<VenueOrderId>>,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        if instrument_ids.len() != client_order_ids.len()
            || instrument_ids.len() != venue_order_ids.len()
        {
            anyhow::bail!(
                "instrument_ids, client_order_ids, and venue_order_ids must have the same length"
            );
        }

        if instrument_ids.is_empty() {
            return Ok(Vec::new());
        }

        if instrument_ids.len() > 20 {
            anyhow::bail!("Batch cancel limit is 20 orders per request");
        }

        let mut cancel_entries = Vec::new();

        for ((instrument_id, client_order_id), venue_order_id) in instrument_ids
            .iter()
            .zip(client_order_ids.iter())
            .zip(venue_order_ids.iter())
        {
            let bybit_symbol = BybitSymbol::new(instrument_id.symbol.as_str())?;
            let mut cancel_entry = BybitBatchCancelOrderEntryBuilder::default();
            cancel_entry.symbol(bybit_symbol.raw_symbol().to_string());

            if let Some(venue_order_id) = venue_order_id {
                cancel_entry.order_id(venue_order_id.to_string());
            } else if let Some(client_order_id) = client_order_id {
                cancel_entry.order_link_id(client_order_id.to_string());
            } else {
                anyhow::bail!(
                    "Either client_order_id or venue_order_id must be provided for each order"
                );
            }

            cancel_entries.push(cancel_entry.build().map_err(|e| anyhow::anyhow!(e))?);
        }

        let mut params = BybitBatchCancelOrderParamsBuilder::default();
        params.category(product_type);
        params.request(cancel_entries);

        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;
        let body = serde_json::to_vec(&params)?;

        let _response: BybitPlaceOrderResponse = self
            .send_request(Method::POST, "/v5/order/cancel-batch", Some(body), true)
            .await?;

        // Query each order to get full details after cancellation
        let mut reports = Vec::new();
        for (instrument_id, (client_order_id, venue_order_id)) in instrument_ids
            .iter()
            .zip(client_order_ids.iter().zip(venue_order_ids.iter()))
        {
            let instrument = self.instrument_from_cache(&instrument_id.symbol)?;
            let bybit_symbol = BybitSymbol::new(instrument_id.symbol.as_str())?;

            let mut query_params = BybitOpenOrdersParamsBuilder::default();
            query_params.category(product_type);
            query_params.symbol(bybit_symbol.raw_symbol().to_string());

            if let Some(venue_order_id) = venue_order_id {
                query_params.order_id(venue_order_id.to_string());
            } else if let Some(client_order_id) = client_order_id {
                query_params.order_link_id(client_order_id.to_string());
            }

            let query_params = query_params.build().map_err(|e| anyhow::anyhow!(e))?;
            let path = Self::build_path("/v5/order/history", &query_params)?;
            let order_response: BybitOrderHistoryResponse =
                self.send_request(Method::GET, &path, None, true).await?;

            if let Some(order) = order_response.result.list.into_iter().next() {
                let ts_init = self.generate_ts_init();
                let report = parse_order_status_report(&order, &instrument, account_id, ts_init)?;
                reports.push(report);
            }
        }

        Ok(reports)
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
        account_id: AccountId,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let instrument = self.instrument_from_cache(&instrument_id.symbol)?;
        let bybit_symbol = BybitSymbol::new(instrument_id.symbol.as_str())?;

        let mut params = BybitCancelAllOrdersParamsBuilder::default();
        params.category(product_type);
        params.symbol(bybit_symbol.raw_symbol().to_string());

        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;
        let body = serde_json::to_vec(&params)?;

        let _response: crate::common::models::BybitListResponse<serde_json::Value> = self
            .send_request(Method::POST, "/v5/order/cancel-all", Some(body), true)
            .await?;

        // Query the order history to get all canceled orders
        let mut query_params = BybitOrderHistoryParamsBuilder::default();
        query_params.category(product_type);
        query_params.symbol(bybit_symbol.raw_symbol().to_string());
        query_params.limit(50u32);

        let query_params = query_params.build().map_err(|e| anyhow::anyhow!(e))?;
        let path = Self::build_path("/v5/order/history", &query_params)?;
        let order_response: BybitOrderHistoryResponse =
            self.send_request(Method::GET, &path, None, true).await?;

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
    #[allow(clippy::too_many_arguments)]
    pub async fn modify_order(
        &self,
        account_id: AccountId,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
        quantity: Option<Quantity>,
        price: Option<Price>,
    ) -> anyhow::Result<OrderStatusReport> {
        let instrument = self.instrument_from_cache(&instrument_id.symbol)?;
        let bybit_symbol = BybitSymbol::new(instrument_id.symbol.as_str())?;

        let mut amend_entry = BybitBatchAmendOrderEntryBuilder::default();
        amend_entry.symbol(bybit_symbol.raw_symbol().to_string());

        if let Some(venue_order_id) = venue_order_id {
            amend_entry.order_id(venue_order_id.to_string());
        } else if let Some(client_order_id) = client_order_id {
            amend_entry.order_link_id(client_order_id.to_string());
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

        let response: BybitPlaceOrderResponse = self
            .send_request(Method::POST, "/v5/order/amend", Some(body), true)
            .await?;

        let order_id = response
            .result
            .order_id
            .ok_or_else(|| anyhow::anyhow!("No order_id in amend response"))?;

        // Query the order to get full details after amendment
        let mut query_params = BybitOpenOrdersParamsBuilder::default();
        query_params.category(product_type);
        query_params.order_id(order_id.as_str().to_string());

        let query_params = query_params.build().map_err(|e| anyhow::anyhow!(e))?;
        let path = Self::build_path("/v5/order/realtime", &query_params)?;
        let order_response: BybitOpenOrdersResponse =
            self.send_request(Method::GET, &path, None, true).await?;

        let order = order_response
            .result
            .list
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("No order returned after modification"))?;

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
        account_id: AccountId,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        tracing::info!(
            "query_order called: instrument_id={}, client_order_id={:?}, venue_order_id={:?}",
            instrument_id,
            client_order_id,
            venue_order_id
        );

        let bybit_symbol = BybitSymbol::new(instrument_id.symbol.as_str())?;

        let mut params = BybitOpenOrdersParamsBuilder::default();
        params.category(product_type);
        // Use the raw Bybit symbol (e.g., "ETHUSDT") not the full instrument symbol
        params.symbol(bybit_symbol.raw_symbol().to_string());

        if let Some(venue_order_id) = venue_order_id {
            params.order_id(venue_order_id.to_string());
        } else if let Some(client_order_id) = client_order_id {
            params.order_link_id(client_order_id.to_string());
        } else {
            anyhow::bail!("Either client_order_id or venue_order_id must be provided");
        }

        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;
        let path = Self::build_path("/v5/order/realtime", &params)?;

        let mut response: BybitOpenOrdersResponse =
            self.send_request(Method::GET, &path, None, true).await?;

        // If not found in open orders, check order history
        if response.result.list.is_empty() {
            tracing::debug!("Order not found in open orders, checking order history");

            let mut history_params = BybitOrderHistoryParamsBuilder::default();
            history_params.category(product_type);
            history_params.symbol(bybit_symbol.raw_symbol().to_string());

            if let Some(venue_order_id) = venue_order_id {
                history_params.order_id(venue_order_id.to_string());
            } else if let Some(client_order_id) = client_order_id {
                history_params.order_link_id(client_order_id.to_string());
            }

            let history_params = history_params.build().map_err(|e| anyhow::anyhow!(e))?;
            let history_path = Self::build_path("/v5/order/history", &history_params)?;

            let history_response: BybitOrderHistoryResponse = self
                .send_request(Method::GET, &history_path, None, true)
                .await?;

            if history_response.result.list.is_empty() {
                tracing::debug!("Order not found in order history either");
                return Ok(None);
            }

            // Move the order from history response to the response list
            response.result.list = history_response.result.list;
        }

        let order = &response.result.list[0];
        let ts_init = self.generate_ts_init();

        tracing::debug!(
            "Query order response: symbol={}, order_id={}, order_link_id={}",
            order.symbol.as_str(),
            order.order_id.as_str(),
            order.order_link_id.as_str()
        );

        // Get instrument from cache with better error context
        let instrument = self
            .instrument_from_cache(&instrument_id.symbol)
            .map_err(|e| {
                tracing::error!(
                    "Instrument cache miss for symbol '{}': {}",
                    instrument_id.symbol.as_str(),
                    e
                );
                anyhow::anyhow!(
                    "Failed to query order {}: {}",
                    client_order_id
                        .as_ref()
                        .map(|id| id.to_string())
                        .or_else(|| venue_order_id.as_ref().map(|id| id.to_string()))
                        .unwrap_or_else(|| "unknown".to_string()),
                    e
                )
            })?;

        tracing::debug!("Retrieved instrument from cache: id={}", instrument.id());

        let report =
            parse_order_status_report(order, &instrument, account_id, ts_init).map_err(|e| {
                tracing::error!(
                    "Failed to parse order status report for {}: {}",
                    order.order_link_id.as_str(),
                    e
                );
                e
            })?;

        tracing::debug!(
            "Successfully created OrderStatusReport for {}",
            order.order_link_id.as_str()
        );

        Ok(Some(report))
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

        let params = BybitInstrumentsInfoParams {
            category: product_type,
            symbol,
            status: None,
            base_coin: None,
            limit: None,
            cursor: None,
        };

        let mut instruments = Vec::new();

        let default_fee_rate = |symbol: ustr::Ustr| BybitFeeRate {
            symbol,
            taker_fee_rate: "0.001".to_string(),
            maker_fee_rate: "0.001".to_string(),
            base_coin: None,
        };

        match product_type {
            BybitProductType::Spot => {
                let response: BybitInstrumentSpotResponse =
                    self.http_get_instruments(&params).await?;

                // Try to get fee rates, use defaults if credentials are missing
                let fee_map: HashMap<_, _> = {
                    let mut fee_params = BybitFeeRateParamsBuilder::default();
                    fee_params.category(product_type);
                    if let Ok(params) = fee_params.build() {
                        match self.http_get_fee_rate(&params).await {
                            Ok(fee_response) => fee_response
                                .result
                                .list
                                .into_iter()
                                .map(|f| (f.symbol, f))
                                .collect(),
                            Err(BybitHttpError::MissingCredentials) => {
                                tracing::warn!("Missing credentials for fee rates, using defaults");
                                HashMap::new()
                            }
                            Err(e) => return Err(e.into()),
                        }
                    } else {
                        HashMap::new()
                    }
                };

                for definition in response.result.list {
                    let fee_rate = fee_map
                        .get(&definition.symbol)
                        .cloned()
                        .unwrap_or_else(|| default_fee_rate(definition.symbol));
                    if let Ok(instrument) =
                        parse_spot_instrument(&definition, &fee_rate, ts_init, ts_init)
                    {
                        instruments.push(instrument);
                    }
                }
            }
            BybitProductType::Linear => {
                let response: BybitInstrumentLinearResponse =
                    self.http_get_instruments(&params).await?;

                // Try to get fee rates, use defaults if credentials are missing
                let fee_map: HashMap<_, _> = {
                    let mut fee_params = BybitFeeRateParamsBuilder::default();
                    fee_params.category(product_type);
                    if let Ok(params) = fee_params.build() {
                        match self.http_get_fee_rate(&params).await {
                            Ok(fee_response) => fee_response
                                .result
                                .list
                                .into_iter()
                                .map(|f| (f.symbol, f))
                                .collect(),
                            Err(BybitHttpError::MissingCredentials) => {
                                tracing::warn!("Missing credentials for fee rates, using defaults");
                                HashMap::new()
                            }
                            Err(e) => return Err(e.into()),
                        }
                    } else {
                        HashMap::new()
                    }
                };

                for definition in response.result.list {
                    let fee_rate = fee_map
                        .get(&definition.symbol)
                        .cloned()
                        .unwrap_or_else(|| default_fee_rate(definition.symbol));
                    if let Ok(instrument) =
                        parse_linear_instrument(&definition, &fee_rate, ts_init, ts_init)
                    {
                        instruments.push(instrument);
                    }
                }
            }
            BybitProductType::Inverse => {
                let response: BybitInstrumentInverseResponse =
                    self.http_get_instruments(&params).await?;

                // Try to get fee rates, use defaults if credentials are missing
                let fee_map: HashMap<_, _> = {
                    let mut fee_params = BybitFeeRateParamsBuilder::default();
                    fee_params.category(product_type);
                    if let Ok(params) = fee_params.build() {
                        match self.http_get_fee_rate(&params).await {
                            Ok(fee_response) => fee_response
                                .result
                                .list
                                .into_iter()
                                .map(|f| (f.symbol, f))
                                .collect(),
                            Err(BybitHttpError::MissingCredentials) => {
                                tracing::warn!("Missing credentials for fee rates, using defaults");
                                HashMap::new()
                            }
                            Err(e) => return Err(e.into()),
                        }
                    } else {
                        HashMap::new()
                    }
                };

                for definition in response.result.list {
                    let fee_rate = fee_map
                        .get(&definition.symbol)
                        .cloned()
                        .unwrap_or_else(|| default_fee_rate(definition.symbol));
                    if let Ok(instrument) =
                        parse_inverse_instrument(&definition, &fee_rate, ts_init, ts_init)
                    {
                        instruments.push(instrument);
                    }
                }
            }
            BybitProductType::Option => {
                let response: BybitInstrumentOptionResponse =
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

    /// Request recent trade tick history for a given symbol.
    ///
    /// Returns the most recent public trades from Bybit's `/v5/market/recent-trade` endpoint.
    /// This endpoint only provides recent trades (up to 1000 most recent), typically covering
    /// only the last few minutes for active markets.
    ///
    /// **Note**: For historical trade data with time ranges, use the klines endpoint instead.
    /// The Bybit public API does not support fetching historical trades by time range.
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
        let instrument = self.instrument_from_cache(&instrument_id.symbol)?;
        let bybit_symbol = BybitSymbol::new(instrument_id.symbol.as_str())?;

        let mut params_builder = BybitTradesParamsBuilder::default();
        params_builder.category(product_type);
        params_builder.symbol(bybit_symbol.raw_symbol().to_string());
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
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
        timestamp_on_close: bool,
    ) -> anyhow::Result<Vec<Bar>> {
        let instrument = self.instrument_from_cache(&bar_type.instrument_id().symbol)?;
        let bybit_symbol = BybitSymbol::new(bar_type.instrument_id().symbol.as_str())?;

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

        let start_ms = start.map(|dt| dt.timestamp_millis());
        let mut all_bars: Vec<Bar> = Vec::new();
        let mut seen_timestamps: std::collections::HashSet<i64> = std::collections::HashSet::new();

        // Pagination strategy: work backwards from end time
        // - Each page fetched is older than the previous page
        // - Within each page, bars are in chronological order (oldest to newest)
        // - We insert each new (older) page at the front to maintain overall chronological order
        // Example with 2 pages:
        //   Page 1 (most recent): bars [T=2000..2999]
        //   Page 2 (older):       bars [T=1000..1999]
        //   Result after splice:  bars [T=1000..1999, T=2000..2999] ✓ chronological
        let mut current_end = end.map(|dt| dt.timestamp_millis());
        let mut page_count = 0;

        loop {
            page_count += 1;

            let mut params_builder = BybitKlinesParamsBuilder::default();
            params_builder.category(product_type);
            params_builder.symbol(bybit_symbol.raw_symbol().to_string());
            params_builder.interval(interval);
            params_builder.limit(1000u32); // Limit for data size per page (maximum for the Bybit API)

            if let Some(start_val) = start_ms {
                params_builder.start(start_val);
            }
            if let Some(end_val) = current_end {
                params_builder.end(end_val);
            }

            let params = params_builder.build().map_err(|e| anyhow::anyhow!(e))?;
            let response = self.http_get_klines(&params).await?;

            let klines = response.result.list;
            if klines.is_empty() {
                break;
            }

            // Sort klines by start time
            let mut sorted_klines = klines;
            sorted_klines.sort_by_key(|k| k.start.parse::<i64>().unwrap_or(0));

            // Parse klines to bars, filtering duplicates
            let ts_init = self.generate_ts_init();
            let mut new_bars = Vec::new();

            for kline in &sorted_klines {
                let start_time = kline.start.parse::<i64>().unwrap_or(0);
                if !seen_timestamps.contains(&start_time)
                    && let Ok(bar) =
                        parse_kline_bar(kline, &instrument, bar_type, timestamp_on_close, ts_init)
                {
                    new_bars.push(bar);
                }
            }

            // If no new bars were added (all were duplicates), we've reached the end
            if new_bars.is_empty() {
                break;
            }

            // Insert older pages at the front to maintain chronological order
            // (we're fetching backwards, so each new page is older than what we already have)
            all_bars.splice(0..0, new_bars);
            seen_timestamps.extend(
                sorted_klines
                    .iter()
                    .filter_map(|k| k.start.parse::<i64>().ok()),
            );

            // Check if we've reached the requested limit
            if let Some(limit_val) = limit
                && all_bars.len() >= limit_val as usize
            {
                break;
            }

            // Move end time backwards to get earlier data
            // Set new end to be 1ms before the first bar of this page
            let earliest_bar_time = sorted_klines[0].start.parse::<i64>().unwrap_or(0);
            if let Some(start_val) = start_ms
                && earliest_bar_time <= start_val
            {
                break;
            }

            current_end = Some(earliest_bar_time - 1);

            // Safety check to prevent infinite loops
            if page_count > 100 {
                break;
            }
        }

        // all_bars is now in chronological order (oldest to newest)
        // If limit is specified and we have more bars, return the last N bars (most recent)
        if let Some(limit_val) = limit {
            let limit_usize = limit_val as usize;
            if all_bars.len() > limit_usize {
                let start_idx = all_bars.len() - limit_usize;
                return Ok(all_bars[start_idx..].to_vec());
            }
        }

        Ok(all_bars)
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
    ) -> anyhow::Result<Vec<BybitFeeRate>> {
        let params = BybitFeeRateParams {
            category: product_type,
            symbol,
            base_coin,
        };

        let response = self.http_get_fee_rate(&params).await?;
        Ok(response.result.list)
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
        account_type: BybitAccountType,
        account_id: AccountId,
    ) -> anyhow::Result<AccountState> {
        let params = BybitWalletBalanceParams {
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
        account_id: AccountId,
        product_type: BybitProductType,
        instrument_id: Option<InstrumentId>,
        open_only: bool,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        // Extract symbol parameter from instrument_id if provided
        let symbol_param = if let Some(id) = instrument_id.as_ref() {
            let symbol_str = id.symbol.as_str();
            if symbol_str.is_empty() {
                None
            } else {
                Some(BybitSymbol::new(symbol_str)?.raw_symbol().to_string())
            }
        } else {
            None
        };

        // For LINEAR without symbol, query all settle coins to avoid filtering
        // For INVERSE, never use settle_coin parameter
        let settle_coins_to_query: Vec<Option<String>> =
            if product_type == BybitProductType::Linear && symbol_param.is_none() {
                vec![Some("USDT".to_string()), Some("USDC".to_string())]
            } else {
                match product_type {
                    BybitProductType::Inverse => vec![None],
                    _ => vec![None],
                }
            };

        let mut all_collected_orders = Vec::new();
        let mut total_collected_across_coins = 0;

        for settle_coin in settle_coins_to_query {
            let remaining_limit = if let Some(limit) = limit {
                let remaining = (limit as usize).saturating_sub(total_collected_across_coins);
                if remaining == 0 {
                    break;
                }
                Some(remaining as u32)
            } else {
                None
            };

            let orders_for_coin = if open_only {
                let mut all_orders = Vec::new();
                let mut cursor: Option<String> = None;
                let mut total_orders = 0;

                loop {
                    let remaining = if let Some(limit) = remaining_limit {
                        (limit as usize).saturating_sub(total_orders)
                    } else {
                        usize::MAX
                    };

                    if remaining == 0 {
                        break;
                    }

                    // Max 50 per Bybit API
                    let page_limit = std::cmp::min(remaining, 50);

                    let mut p = BybitOpenOrdersParamsBuilder::default();
                    p.category(product_type);
                    if let Some(symbol) = symbol_param.clone() {
                        p.symbol(symbol);
                    }
                    if let Some(coin) = settle_coin.clone() {
                        p.settle_coin(coin);
                    }
                    p.limit(page_limit as u32);
                    if let Some(c) = cursor {
                        p.cursor(c);
                    }
                    let params = p.build().map_err(|e| anyhow::anyhow!(e))?;
                    let path = Self::build_path("/v5/order/realtime", &params)?;
                    let response: BybitOpenOrdersResponse =
                        self.send_request(Method::GET, &path, None, true).await?;

                    total_orders += response.result.list.len();
                    all_orders.extend(response.result.list);

                    cursor = response.result.next_page_cursor;
                    if cursor.is_none() || cursor.as_ref().is_none_or(|c| c.is_empty()) {
                        break;
                    }
                }

                all_orders
            } else {
                // Query both realtime and history endpoints
                // Realtime has current open orders, history may lag for recent orders
                let mut all_orders = Vec::new();
                let mut open_orders = Vec::new();
                let mut cursor: Option<String> = None;
                let mut total_open_orders = 0;

                loop {
                    let remaining = if let Some(limit) = remaining_limit {
                        (limit as usize).saturating_sub(total_open_orders)
                    } else {
                        usize::MAX
                    };

                    if remaining == 0 {
                        break;
                    }

                    // Max 50 per Bybit API
                    let page_limit = std::cmp::min(remaining, 50);

                    let mut open_params = BybitOpenOrdersParamsBuilder::default();
                    open_params.category(product_type);
                    if let Some(symbol) = symbol_param.clone() {
                        open_params.symbol(symbol);
                    }
                    if let Some(coin) = settle_coin.clone() {
                        open_params.settle_coin(coin);
                    }
                    open_params.limit(page_limit as u32);
                    if let Some(c) = cursor {
                        open_params.cursor(c);
                    }
                    let open_params = open_params.build().map_err(|e| anyhow::anyhow!(e))?;
                    let open_path = Self::build_path("/v5/order/realtime", &open_params)?;
                    let open_response: BybitOpenOrdersResponse = self
                        .send_request(Method::GET, &open_path, None, true)
                        .await?;

                    total_open_orders += open_response.result.list.len();
                    open_orders.extend(open_response.result.list);

                    cursor = open_response.result.next_page_cursor;
                    if cursor.is_none() || cursor.as_ref().is_none_or(|c| c.is_empty()) {
                        break;
                    }
                }

                let seen_order_ids: std::collections::HashSet<Ustr> =
                    open_orders.iter().map(|o| o.order_id).collect();

                all_orders.extend(open_orders);

                let mut cursor: Option<String> = None;
                let mut total_history_orders = 0;

                loop {
                    let total_orders = total_open_orders + total_history_orders;
                    let remaining = if let Some(limit) = remaining_limit {
                        (limit as usize).saturating_sub(total_orders)
                    } else {
                        usize::MAX
                    };

                    if remaining == 0 {
                        break;
                    }

                    // Max 50 per Bybit API
                    let page_limit = std::cmp::min(remaining, 50);

                    let mut history_params = BybitOrderHistoryParamsBuilder::default();
                    history_params.category(product_type);
                    if let Some(symbol) = symbol_param.clone() {
                        history_params.symbol(symbol);
                    }
                    if let Some(coin) = settle_coin.clone() {
                        history_params.settle_coin(coin);
                    }
                    history_params.limit(page_limit as u32);
                    if let Some(c) = cursor {
                        history_params.cursor(c);
                    }
                    let history_params = history_params.build().map_err(|e| anyhow::anyhow!(e))?;
                    let history_path = Self::build_path("/v5/order/history", &history_params)?;
                    let history_response: BybitOrderHistoryResponse = self
                        .send_request(Method::GET, &history_path, None, true)
                        .await?;

                    // Open orders might appear in both realtime and history
                    for order in history_response.result.list {
                        if !seen_order_ids.contains(&order.order_id) {
                            all_orders.push(order);
                            total_history_orders += 1;
                        }
                    }

                    cursor = history_response.result.next_page_cursor;
                    if cursor.is_none() || cursor.as_ref().is_none_or(|c| c.is_empty()) {
                        break;
                    }
                }

                all_orders
            };

            total_collected_across_coins += orders_for_coin.len();
            all_collected_orders.extend(orders_for_coin);
        }

        let ts_init = self.generate_ts_init();

        let mut reports = Vec::new();
        for order in all_collected_orders {
            if let Some(ref instrument_id) = instrument_id {
                let instrument = self.instrument_from_cache(&instrument_id.symbol)?;
                if let Ok(report) =
                    parse_order_status_report(&order, &instrument, account_id, ts_init)
                {
                    reports.push(report);
                }
            } else {
                // Bybit returns raw symbol (e.g. "ETHUSDT"), need to add product suffix for cache lookup
                // Note: instruments are stored in cache by symbol only (without venue)
                if !order.symbol.is_empty() {
                    let symbol_with_product =
                        Symbol::from_ustr_unchecked(make_bybit_symbol(order.symbol, product_type));
                    if let Ok(instrument) = self.instrument_from_cache(&symbol_with_product)
                        && let Ok(report) =
                            parse_order_status_report(&order, &instrument, account_id, ts_init)
                    {
                        reports.push(report);
                    }
                }
            }
        }

        Ok(reports)
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
        account_id: AccountId,
        product_type: BybitProductType,
        instrument_id: Option<InstrumentId>,
        start: Option<i64>,
        end: Option<i64>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<FillReport>> {
        // Build query parameters
        let symbol = if let Some(id) = instrument_id {
            let bybit_symbol = BybitSymbol::new(id.symbol.as_str())?;
            Some(bybit_symbol.raw_symbol().to_string())
        } else {
            None
        };

        // Fetch all executions with pagination
        let mut all_executions = Vec::new();
        let mut cursor: Option<String> = None;
        let mut total_executions = 0;

        loop {
            // Calculate how many more executions we can request
            let remaining = if let Some(limit) = limit {
                (limit as usize).saturating_sub(total_executions)
            } else {
                usize::MAX
            };

            // If we've reached the limit, stop
            if remaining == 0 {
                break;
            }

            // Size the page request to respect caller's limit (max 100 per Bybit API)
            let page_limit = std::cmp::min(remaining, 100);

            let params = BybitTradeHistoryParams {
                category: product_type,
                symbol: symbol.clone(),
                base_coin: None,
                order_id: None,
                order_link_id: None,
                start_time: start,
                end_time: end,
                exec_type: None,
                limit: Some(page_limit as u32),
                cursor: cursor.clone(),
            };

            let response = self.http_get_trade_history(&params).await?;
            let list_len = response.result.list.len();
            all_executions.extend(response.result.list);
            total_executions += list_len;

            cursor = response.result.next_page_cursor;
            if cursor.is_none() || cursor.as_ref().is_none_or(|c| c.is_empty()) {
                break;
            }
        }

        let ts_init = self.generate_ts_init();
        let mut reports = Vec::new();

        for execution in all_executions {
            // Get instrument for this execution
            // Bybit returns raw symbol (e.g. "ETHUSDT"), need to add product suffix for cache lookup
            let symbol_with_product =
                Symbol::from_ustr_unchecked(make_bybit_symbol(execution.symbol, product_type));
            let instrument = self.instrument_from_cache(&symbol_with_product)?;

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
    /// <https://bybit-exchange.github.io/docs/v5/position>
    pub async fn request_position_status_reports(
        &self,
        account_id: AccountId,
        product_type: BybitProductType,
        instrument_id: Option<InstrumentId>,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        // Handle SPOT position reports via wallet balances if flag is enabled
        if product_type == BybitProductType::Spot {
            if self.use_spot_position_reports.load(Ordering::Relaxed) {
                return self
                    .generate_spot_position_reports_from_wallet(account_id, instrument_id)
                    .await;
            } else {
                // Return empty vector when SPOT position reports are disabled
                return Ok(Vec::new());
            }
        }

        let ts_init = self.generate_ts_init();
        let mut reports = Vec::new();

        // Build query parameters based on whether a specific instrument is requested
        let symbol = if let Some(id) = instrument_id {
            let symbol_str = id.symbol.as_str();
            if symbol_str.is_empty() {
                anyhow::bail!("InstrumentId symbol is empty");
            }
            let bybit_symbol = BybitSymbol::new(symbol_str)?;
            Some(bybit_symbol.raw_symbol().to_string())
        } else {
            None
        };

        // For LINEAR category, the API requires either symbol OR settleCoin
        // When querying all positions (no symbol), we must iterate through settle coins
        if product_type == BybitProductType::Linear && symbol.is_none() {
            // Query positions for each known settle coin with pagination
            for settle_coin in ["USDT", "USDC"] {
                let mut cursor: Option<String> = None;

                loop {
                    let params = BybitPositionListParams {
                        category: product_type,
                        symbol: None,
                        base_coin: None,
                        settle_coin: Some(settle_coin.to_string()),
                        limit: Some(200), // Max 200 per request
                        cursor: cursor.clone(),
                    };

                    let response = self.http_get_positions(&params).await?;

                    for position in response.result.list {
                        if position.symbol.is_empty() {
                            continue;
                        }

                        let symbol_with_product = Symbol::new(format!(
                            "{}{}",
                            position.symbol.as_str(),
                            product_type.suffix()
                        ));

                        if let Ok(instrument) = self.instrument_from_cache(&symbol_with_product)
                            && let Ok(report) = parse_position_status_report(
                                &position,
                                account_id,
                                &instrument,
                                ts_init,
                            )
                        {
                            reports.push(report);
                        }
                    }

                    cursor = response.result.next_page_cursor;
                    if cursor.is_none() || cursor.as_ref().is_none_or(|c| c.is_empty()) {
                        break;
                    }
                }
            }
        } else {
            // For other product types or when a specific symbol is requested with pagination
            let mut cursor: Option<String> = None;

            loop {
                let params = BybitPositionListParams {
                    category: product_type,
                    symbol: symbol.clone(),
                    base_coin: None,
                    settle_coin: None,
                    limit: Some(200), // Max 200 per request
                    cursor: cursor.clone(),
                };

                let response = self.http_get_positions(&params).await?;

                for position in response.result.list {
                    if position.symbol.is_empty() {
                        continue;
                    }

                    let symbol_with_product = Symbol::new(format!(
                        "{}{}",
                        position.symbol.as_str(),
                        product_type.suffix()
                    ));

                    if let Ok(instrument) = self.instrument_from_cache(&symbol_with_product)
                        && let Ok(report) = parse_position_status_report(
                            &position,
                            account_id,
                            &instrument,
                            ts_init,
                        )
                    {
                        reports.push(report);
                    }
                }

                cursor = response.result.next_page_cursor;
                if cursor.is_none() || cursor.as_ref().is_none_or(|c| c.is_empty()) {
                    break;
                }
            }
        }

        Ok(reports)
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
        Self::new(None, Some(60), None, None, None, None, None)
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
        recv_window_ms: Option<u64>,
        proxy_url: Option<String>,
    ) -> Result<Self, BybitHttpError> {
        Ok(Self {
            inner: Arc::new(BybitHttpInnerClient::new(
                base_url,
                timeout_secs,
                max_retries,
                retry_delay_ms,
                retry_delay_max_ms,
                recv_window_ms,
                proxy_url,
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
        recv_window_ms: Option<u64>,
        proxy_url: Option<String>,
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
                recv_window_ms,
                proxy_url,
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

    /// Set whether to generate position reports from wallet balances for SPOT positions.
    pub fn set_use_spot_position_reports(&self, use_spot_position_reports: bool) {
        self.inner
            .set_use_spot_position_reports(use_spot_position_reports);
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
    /// - <https://bybit-exchange.github.io/docs/v5/market/instrument>
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
    /// - <https://bybit-exchange.github.io/docs/v5/market/instrument>
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
    /// - <https://bybit-exchange.github.io/docs/v5/market/instrument>
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
    /// - <https://bybit-exchange.github.io/docs/v5/market/instrument>
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
    /// - <https://bybit-exchange.github.io/docs/v5/market/instrument>
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

    /// Fetches wallet balance (requires authentication).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The request fails.
    /// - The response cannot be parsed.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/account/wallet-balance>
    pub async fn http_get_wallet_balance(
        &self,
        params: &BybitWalletBalanceParams,
    ) -> Result<BybitWalletBalanceResponse, BybitHttpError> {
        self.inner.http_get_wallet_balance(params).await
    }

    /// Fetches position information (requires authentication).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - The API returns an error.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/position>
    pub async fn http_get_positions(
        &self,
        params: &BybitPositionListParams,
    ) -> Result<BybitPositionListResponse, BybitHttpError> {
        self.inner.http_get_positions(params).await
    }

    /// Fetches fee rate (requires authentication).
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - The API returns an error.
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/account/fee-rate>
    pub async fn http_get_fee_rate(
        &self,
        params: &BybitFeeRateParams,
    ) -> Result<BybitFeeRateResponse, BybitHttpError> {
        self.inner.http_get_fee_rate(params).await
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
        account_id: AccountId,
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
                account_id,
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
    #[allow(clippy::too_many_arguments)]
    pub async fn modify_order(
        &self,
        account_id: AccountId,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
        quantity: Option<Quantity>,
        price: Option<Price>,
    ) -> anyhow::Result<OrderStatusReport> {
        self.inner
            .modify_order(
                account_id,
                product_type,
                instrument_id,
                client_order_id,
                venue_order_id,
                quantity,
                price,
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
        account_id: AccountId,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> anyhow::Result<OrderStatusReport> {
        self.inner
            .cancel_order(
                account_id,
                product_type,
                instrument_id,
                client_order_id,
                venue_order_id,
            )
            .await
    }

    /// Batch cancel multiple orders.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - Any of the orders don't exist.
    /// - The API returns an error.
    pub async fn batch_cancel_orders(
        &self,
        account_id: AccountId,
        product_type: BybitProductType,
        instrument_ids: Vec<InstrumentId>,
        client_order_ids: Vec<Option<ClientOrderId>>,
        venue_order_ids: Vec<Option<VenueOrderId>>,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        self.inner
            .batch_cancel_orders(
                account_id,
                product_type,
                instrument_ids,
                client_order_ids,
                venue_order_ids,
            )
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
        account_id: AccountId,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        self.inner
            .cancel_all_orders(account_id, product_type, instrument_id)
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
        account_id: AccountId,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        self.inner
            .query_order(
                account_id,
                product_type,
                instrument_id,
                client_order_id,
                venue_order_id,
            )
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
        account_id: AccountId,
        product_type: BybitProductType,
        instrument_id: Option<InstrumentId>,
        open_only: bool,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        self.inner
            .request_order_status_reports(account_id, product_type, instrument_id, open_only, limit)
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

    /// Request recent trade tick history for a given symbol.
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
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
        timestamp_on_close: bool,
    ) -> anyhow::Result<Vec<Bar>> {
        self.inner
            .request_bars(
                product_type,
                bar_type,
                start,
                end,
                limit,
                timestamp_on_close,
            )
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
        account_id: AccountId,
        product_type: BybitProductType,
        instrument_id: Option<InstrumentId>,
        start: Option<i64>,
        end: Option<i64>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<FillReport>> {
        self.inner
            .request_fill_reports(account_id, product_type, instrument_id, start, end, limit)
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
    /// <https://bybit-exchange.github.io/docs/v5/position>
    pub async fn request_position_status_reports(
        &self,
        account_id: AccountId,
        product_type: BybitProductType,
        instrument_id: Option<InstrumentId>,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        self.inner
            .request_position_status_reports(account_id, product_type, instrument_id)
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
        account_id: AccountId,
    ) -> anyhow::Result<AccountState> {
        self.inner
            .request_account_state(account_type, account_id)
            .await
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
    ) -> anyhow::Result<Vec<BybitFeeRate>> {
        self.inner
            .request_fee_rates(product_type, symbol, base_coin)
            .await
    }

    /// Sets the margin mode for the account.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - The API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if required parameters are not provided (should not happen with current implementation).
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/account/set-margin-mode>
    pub async fn http_set_margin_mode(
        &self,
        margin_mode: BybitMarginMode,
    ) -> Result<BybitSetMarginModeResponse, BybitHttpError> {
        self.inner.http_set_margin_mode(margin_mode).await
    }

    /// Sets leverage for a symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - The API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if required parameters are not provided (should not happen with current implementation).
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/position/leverage>
    pub async fn http_set_leverage(
        &self,
        product_type: BybitProductType,
        symbol: &str,
        buy_leverage: &str,
        sell_leverage: &str,
    ) -> Result<BybitSetLeverageResponse, BybitHttpError> {
        self.inner
            .http_set_leverage(product_type, symbol, buy_leverage, sell_leverage)
            .await
    }

    /// Switches position mode for a product type.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Credentials are missing.
    /// - The request fails.
    /// - The API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if required parameters are not provided (should not happen with current implementation).
    ///
    /// # References
    ///
    /// - <https://bybit-exchange.github.io/docs/v5/position/position-mode>
    pub async fn http_switch_mode(
        &self,
        product_type: BybitProductType,
        mode: BybitPositionMode,
        symbol: Option<String>,
        coin: Option<String>,
    ) -> Result<BybitSwitchModeResponse, BybitHttpError> {
        self.inner
            .http_switch_mode(product_type, mode, symbol, coin)
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
        let client = BybitHttpClient::new(None, Some(60), None, None, None, None, None);
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
