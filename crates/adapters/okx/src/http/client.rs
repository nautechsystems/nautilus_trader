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

//! Provides the HTTP client integration for the [OKX](https://okx.com) REST API.
//!
//! This module defines and implements a strongly-typed [`OKXHttpClient`] for
//! sending requests to various OKX endpoints. It handles request signing
//! (when credentials are provided), constructs valid HTTP requests
//! using the [`HttpClient`], and parses the responses back into structured data or a [`OKXHttpError`].

use std::{
    collections::HashMap,
    fmt::Debug,
    num::NonZeroU32,
    sync::{Arc, LazyLock, Mutex},
};

use ahash::AHashSet;
use chrono::{DateTime, Utc};
use nautilus_core::{
    UnixNanos, consts::NAUTILUS_USER_AGENT, env::get_env_var, time::get_atomic_clock_realtime,
};
use nautilus_model::{
    data::{Bar, BarType, IndexPriceUpdate, MarkPriceUpdate, TradeTick},
    enums::{AggregationSource, BarAggregation},
    events::AccountState,
    identifiers::{AccountId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
};
use nautilus_network::{http::HttpClient, ratelimiter::quota::Quota};
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use ustr::Ustr;

use super::{
    error::OKXHttpError,
    models::{
        OKXAccount, OKXIndexTicker, OKXMarkPrice, OKXOrderHistory, OKXPosition, OKXPositionHistory,
        OKXPositionTier, OKXTransactionDetail,
    },
    query::{
        GetCandlesticksParams, GetCandlesticksParamsBuilder, GetIndexTickerParams,
        GetIndexTickerParamsBuilder, GetInstrumentsParams, GetInstrumentsParamsBuilder,
        GetMarkPriceParams, GetMarkPriceParamsBuilder, GetOrderHistoryParams,
        GetOrderHistoryParamsBuilder, GetOrderListParams, GetOrderListParamsBuilder,
        GetPositionTiersParams, GetPositionsHistoryParams, GetPositionsParams,
        GetPositionsParamsBuilder, GetTradesParams, GetTradesParamsBuilder,
        GetTransactionDetailsParams, GetTransactionDetailsParamsBuilder, SetPositionModeParams,
        SetPositionModeParamsBuilder,
    },
};
use crate::{
    common::{
        consts::OKX_HTTP_URL,
        credential::Credential,
        enums::{OKXInstrumentType, OKXPositionMode},
        models::OKXInstrument,
        parse::{
            okx_instrument_type, parse_account_state, parse_candlestick, parse_fill_report,
            parse_index_price_update, parse_instrument_any, parse_mark_price_update,
            parse_order_status_report, parse_position_status_report, parse_trade_tick,
        },
    },
    http::{
        models::{OKXCandlestick, OKXTrade},
        query::{GetOrderParams, GetPendingOrdersParams},
    },
};

const OKX_SUCCESS_CODE: &str = "0";

/// Default OKX REST API rate limit: 500 requests per 2 seconds.
///
/// - Sub-account order limit: 1000 requests per 2 seconds.
/// - Account balance: 10 requests per 2 seconds.
/// - Account instruments: 20 requests per 2 seconds.
///
/// We use a conservative 250 requests per second (500 per 2 seconds) as a general limit
/// that should accommodate most use cases while respecting OKX's documented limits.
pub static OKX_REST_QUOTA: LazyLock<Quota> =
    LazyLock::new(|| Quota::per_second(NonZeroU32::new(250).unwrap()));

/// Represents an OKX HTTP response.
#[derive(Debug, Serialize, Deserialize)]
pub struct OKXResponse<T> {
    /// The OKX response code, which is `"0"` for success.
    pub code: String,
    /// A message string which can be informational or describe an error cause.
    pub msg: String,
    /// The typed data returned by the OKX endpoint.
    pub data: Vec<T>,
}

/// Provides a HTTP client for connecting to the [OKX](https://okx.com) REST API.
///
/// This client wraps the underlying [`HttpClient`] to handle functionality
/// specific to OKX, such as request signing (for authenticated endpoints),
/// forming request URLs, and deserializing responses into specific data models.
pub struct OKXHttpInnerClient {
    base_url: String,
    client: HttpClient,
    credential: Option<Credential>,
}

impl Default for OKXHttpInnerClient {
    fn default() -> Self {
        Self::new(None, Some(60))
    }
}

impl Debug for OKXHttpInnerClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let credential = self.credential.as_ref().map(|_| "<redacted>");
        f.debug_struct(stringify!(OKXHttpInnerClient))
            .field("base_url", &self.base_url)
            .field("credential", &credential)
            .finish_non_exhaustive()
    }
}

impl OKXHttpInnerClient {
    /// Creates a new [`OKXHttpClient`] using the default OKX HTTP URL,
    /// optionally overridden with a custom base URL.
    ///
    /// This version of the client has **no credentials**, so it can only
    /// call publicly accessible endpoints.
    pub fn new(base_url: Option<String>, timeout_secs: Option<u64>) -> Self {
        Self {
            base_url: base_url.unwrap_or(OKX_HTTP_URL.to_string()),
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                vec![],
                Some(*OKX_REST_QUOTA),
                timeout_secs,
            ),
            credential: None,
        }
    }

    /// Creates a new [`OKXHttpClient`] configured with credentials
    /// for authenticated requests, optionally using a custom base URL.
    pub fn with_credentials(
        api_key: String,
        api_secret: String,
        api_passphrase: String,
        base_url: String,
        timeout_secs: Option<u64>,
    ) -> Self {
        Self {
            base_url,
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                vec![],
                Some(*OKX_REST_QUOTA),
                timeout_secs,
            ),
            credential: Some(Credential::new(api_key, api_secret, api_passphrase)),
        }
    }

    /// Builds the default headers to include with each request (e.g., `User-Agent`).
    fn default_headers() -> HashMap<String, String> {
        HashMap::from([("user-agent".to_string(), NAUTILUS_USER_AGENT.to_string())])
    }

    /// Combine a base path with a `serde_urlencoded` query string if one exists.
    ///
    /// # Errors
    ///
    /// Returns an error if the query string serialization fails.
    fn build_path<S: Serialize>(base: &str, params: &S) -> Result<String, OKXHttpError> {
        let query = serde_urlencoded::to_string(params)
            .map_err(|e| OKXHttpError::JsonError(e.to_string()))?;
        if query.is_empty() {
            Ok(base.to_owned())
        } else {
            Ok(format!("{base}?{query}"))
        }
    }

    /// Signs an OKX request with timestamp, API key, passphrase, and signature.
    ///
    /// # Errors
    ///
    /// Returns [`OKXHttpError::MissingCredentials`] if no credentials are set
    /// but the request requires authentication.
    fn sign_request(
        &self,
        method: &Method,
        path: &str,
        body: Option<&[u8]>,
    ) -> Result<HashMap<String, String>, OKXHttpError> {
        let credential = match self.credential.as_ref() {
            Some(c) => c,
            None => return Err(OKXHttpError::MissingCredentials),
        };

        let body_str = body
            .and_then(|b| String::from_utf8(b.to_vec()).ok())
            .unwrap_or_default();

        tracing::debug!("{method} {path}");

        let api_key = credential.api_key.clone().to_string();
        let api_passphrase = credential.api_passphrase.clone().to_string();
        let timestamp = Utc::now().format("%Y-%m-%dT%H:%M:%S.%3fZ").to_string();
        let signature = credential.sign(&timestamp, method.as_str(), path, &body_str);

        let mut headers = HashMap::new();
        headers.insert("OK-ACCESS-KEY".to_string(), api_key);
        headers.insert("OK-ACCESS-PASSPHRASE".to_string(), api_passphrase);
        headers.insert("OK-ACCESS-TIMESTAMP".to_string(), timestamp);
        headers.insert("OK-ACCESS-SIGN".to_string(), signature);

        Ok(headers)
    }

    /// Sends an HTTP request to OKX and parses the response into `Vec<T>`.
    ///
    /// Internally, this method handles:
    /// - Building the URL from `base_url` + `path`
    /// - Optionally signing the request
    /// - Deserializing JSON responses into typed models, or returning a [`OKXHttpError`]
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// - The HTTP request fails.
    /// - Authentication is required but credentials are missing.
    /// - The response cannot be deserialized into the expected type.
    /// - The OKX API returns an error response.
    async fn send_request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
        authenticate: bool,
    ) -> Result<Vec<T>, OKXHttpError> {
        let url = format!("{}{path}", self.base_url);

        let mut headers = if authenticate {
            self.sign_request(&method, path, body.as_deref())?
        } else {
            HashMap::new()
        };

        // Always set Content-Type header when body is present
        if body.is_some() {
            headers.insert("Content-Type".to_string(), "application/json".to_string());
        }

        let resp = self
            .client
            .request(method.clone(), url, Some(headers), body, None, None)
            .await?;

        tracing::trace!("Response: {resp:?}");

        if resp.status.is_success() {
            let okx_response: OKXResponse<T> = serde_json::from_slice(&resp.body).map_err(|e| {
                tracing::error!("Failed to deserialize OKXResponse: {e}");
                OKXHttpError::JsonError(e.to_string())
            })?;

            if okx_response.code != OKX_SUCCESS_CODE {
                return Err(OKXHttpError::OkxError {
                    error_code: okx_response.code,
                    message: okx_response.msg,
                });
            }

            Ok(okx_response.data)
        } else {
            let error_body = String::from_utf8_lossy(&resp.body);
            tracing::error!(
                "HTTP error {} with body: {error_body}",
                resp.status.as_str()
            );

            if let Ok(parsed_error) = serde_json::from_slice::<OKXResponse<T>>(&resp.body) {
                return Err(OKXHttpError::OkxError {
                    error_code: parsed_error.code,
                    message: parsed_error.msg,
                });
            }

            Err(OKXHttpError::UnexpectedStatus {
                status: StatusCode::from_u16(resp.status.as_u16()).unwrap(),
                body: error_body.to_string(),
            })
        }
    }

    /// Set the position mode for an account.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#trading-account-rest-api-set-position-mode>
    pub async fn http_set_position_mode(
        &self,
        params: SetPositionModeParams,
    ) -> Result<Vec<serde_json::Value>, OKXHttpError> {
        let path = "/api/v5/account/set-position-mode";
        let body = serde_json::to_vec(&params).expect("Failed to serialize position mode params");
        self.send_request(Method::POST, path, Some(body), true)
            .await
    }

    /// Requests position tiers information, maximum leverage depends on your borrowings and margin ratio.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#public-data-rest-api-get-position-tiers>
    pub async fn http_get_position_tiers(
        &self,
        params: GetPositionTiersParams,
    ) -> Result<Vec<OKXPositionTier>, OKXHttpError> {
        let path = Self::build_path("/api/v5/public/position-tiers", &params)?;
        self.send_request(Method::GET, &path, None, false).await
    }

    /// Request a list of instruments with open contracts.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#public-data-rest-api-get-instruments>
    pub async fn http_get_instruments(
        &self,
        params: GetInstrumentsParams,
    ) -> Result<Vec<OKXInstrument>, OKXHttpError> {
        let path = Self::build_path("/api/v5/public/instruments", &params)?;
        self.send_request(Method::GET, &path, None, false).await
    }

    /// Requests a mark price.
    ///
    /// We set the mark price based on the SPOT index and at a reasonable basis to prevent individual
    /// users from manipulating the market and causing the contract price to fluctuate.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#public-data-rest-api-get-mark-price>
    pub async fn http_get_mark_price(
        &self,
        params: GetMarkPriceParams,
    ) -> Result<Vec<OKXMarkPrice>, OKXHttpError> {
        let path = Self::build_path("/api/v5/public/mark-price", &params)?;
        self.send_request(Method::GET, &path, None, false).await
    }

    /// Requests the latest index price.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#public-data-rest-api-get-index-tickers>
    pub async fn http_get_index_ticker(
        &self,
        params: GetIndexTickerParams,
    ) -> Result<Vec<OKXIndexTicker>, OKXHttpError> {
        let path = Self::build_path("/api/v5/market/index-tickers", &params)?;
        self.send_request(Method::GET, &path, None, false).await
    }

    /// Requests trades history.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-market-data-get-trades-history>
    pub async fn http_get_trades(
        &self,
        params: GetTradesParams,
    ) -> Result<Vec<OKXTrade>, OKXHttpError> {
        let path = Self::build_path("/api/v5/market/history-trades", &params)?;
        self.send_request(Method::GET, &path, None, false).await
    }

    /// Requests recent candlestick data.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-market-data-get-candlesticks>
    pub async fn http_get_candlesticks(
        &self,
        params: GetCandlesticksParams,
    ) -> Result<Vec<OKXCandlestick>, OKXHttpError> {
        let path = Self::build_path("/api/v5/market/candles", &params)?;
        self.send_request(Method::GET, &path, None, false).await
    }

    /// Requests historical candlestick data.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-market-data-get-candlesticks-history>
    pub async fn http_get_candlesticks_history(
        &self,
        params: GetCandlesticksParams,
    ) -> Result<Vec<OKXCandlestick>, OKXHttpError> {
        let path = Self::build_path("/api/v5/market/history-candles", &params)?;
        self.send_request(Method::GET, &path, None, false).await
    }

    /// Lists current open orders.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-trade-get-orders-pending>
    pub async fn http_get_pending_orders(
        &self,
        params: GetPendingOrdersParams,
    ) -> Result<Vec<OKXOrderHistory>, OKXHttpError> {
        let path = Self::build_path("/api/v5/trade/orders-pending", &params)?;
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Retrieves a single order’s details.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-trade-get-order>
    pub async fn http_get_order(
        &self,
        params: GetOrderParams,
    ) -> Result<Vec<OKXOrderHistory>, OKXHttpError> {
        let path = Self::build_path("/api/v5/trade/order", &params)?;
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Requests a list of assets (with non-zero balance), remaining balance, and available amount
    /// in the trading account.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#trading-account-rest-api-get-balance>
    pub async fn http_get_balance(&self) -> Result<Vec<OKXAccount>, OKXHttpError> {
        let path = "/api/v5/account/balance";
        self.send_request(Method::GET, path, None, true).await
    }

    /// Requests historical order records.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-trade-get-orders-history>
    pub async fn http_get_order_history(
        &self,
        params: GetOrderHistoryParams,
    ) -> Result<Vec<OKXOrderHistory>, OKXHttpError> {
        let path = Self::build_path("/api/v5/trade/orders-history", &params)?;
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Requests order list (pending orders).
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-trade-get-order-list>
    pub async fn http_get_order_list(
        &self,
        params: GetOrderListParams,
    ) -> Result<Vec<OKXOrderHistory>, OKXHttpError> {
        let path = Self::build_path("/api/v5/trade/orders-pending", &params)?;
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Requests information on your positions. When the account is in net mode, net positions will
    /// be displayed, and when the account is in long/short mode, long or short positions will be
    /// displayed. Return in reverse chronological order using ctime.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#trading-account-rest-api-get-positions>
    pub async fn http_get_positions(
        &self,
        params: GetPositionsParams,
    ) -> Result<Vec<OKXPosition>, OKXHttpError> {
        let path = Self::build_path("/api/v5/account/positions", &params)?;
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Requests closed or historical position data.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#trading-account-rest-api-get-positions-history>
    pub async fn http_get_position_history(
        &self,
        params: GetPositionsHistoryParams,
    ) -> Result<Vec<OKXPositionHistory>, OKXHttpError> {
        let path = Self::build_path("/api/v5/account/positions-history", &params)?;
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Requests transaction details (fills) for the given parameters.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-trade-get-transaction-details-last-3-days>
    pub async fn http_get_transaction_details(
        &self,
        params: GetTransactionDetailsParams,
    ) -> Result<Vec<OKXTransactionDetail>, OKXHttpError> {
        let path = Self::build_path("/api/v5/trade/fills", &params)?;
        self.send_request(Method::GET, &path, None, true).await
    }
}

/// Provides a higher-level HTTP client for the [OKX](https://okx.com) REST API.
///
/// This client wraps the underlying `OKXHttpInnerClient` to handle conversions
/// into the Nautilus domain model.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct OKXHttpClient {
    pub(crate) inner: Arc<OKXHttpInnerClient>,
    pub(crate) instruments_cache: Arc<Mutex<HashMap<Ustr, InstrumentAny>>>,
    cache_initialized: bool,
}

impl Default for OKXHttpClient {
    fn default() -> Self {
        Self::new(None, Some(60))
    }
}

impl OKXHttpClient {
    /// Creates a new [`OKXHttpClient`] using the default OKX HTTP URL,
    /// optionally overridden with a custom base url.
    ///
    /// This version of the client has **no credentials**, so it can only
    /// call publicly accessible endpoints.
    pub fn new(base_url: Option<String>, timeout_secs: Option<u64>) -> Self {
        Self {
            inner: Arc::new(OKXHttpInnerClient::new(base_url, timeout_secs)),
            instruments_cache: Arc::new(Mutex::new(HashMap::new())),
            cache_initialized: false,
        }
    }

    /// Creates a new authenticated [`OKXHttpClient`] using environment variables and
    /// the default OKX HTTP base url.
    pub fn from_env() -> anyhow::Result<Self> {
        Self::with_credentials(None, None, None, None, None)
    }

    /// Creates a new [`OKXHttpClient`] configured with credentials
    /// for authenticated requests, optionally using a custom base url.
    pub fn with_credentials(
        api_key: Option<String>,
        api_secret: Option<String>,
        api_passphrase: Option<String>,
        base_url: Option<String>,
        timeout_secs: Option<u64>,
    ) -> anyhow::Result<Self> {
        let api_key = api_key.unwrap_or(get_env_var("OKX_API_KEY")?);
        let api_secret = api_secret.unwrap_or(get_env_var("OKX_API_SECRET")?);
        let api_passphrase = api_passphrase.unwrap_or(get_env_var("OKX_API_PASSPHRASE")?);
        let base_url = base_url.unwrap_or(OKX_HTTP_URL.to_string());

        Ok(Self {
            inner: Arc::new(OKXHttpInnerClient::with_credentials(
                api_key,
                api_secret,
                api_passphrase,
                base_url,
                timeout_secs,
            )),
            instruments_cache: Arc::new(Mutex::new(HashMap::new())),
            cache_initialized: false,
        })
    }

    /// Retrieves an instrument from the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument is not found in the cache.
    fn get_instrument_from_cache(&self, symbol: Ustr) -> anyhow::Result<InstrumentAny> {
        self.instruments_cache
            .lock()
            .expect("`instruments_cache` lock poisoned")
            .get(&symbol)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Instrument {symbol} not in cache"))
    }

    /// Generates a timestamp for initialization.
    fn generate_ts_init(&self) -> UnixNanos {
        get_atomic_clock_realtime().get_time_ns()
    }

    /// Returns the base url being used by the client.
    pub fn base_url(&self) -> &str {
        self.inner.base_url.as_str()
    }

    /// Returns the public API key being used by the client.
    pub fn api_key(&self) -> Option<&str> {
        self.inner.credential.clone().map(|c| c.api_key.as_str())
    }

    /// Checks if the client is initialized.
    ///
    /// The client is considered initialized if any instruments have been cached from the venue.
    #[must_use]
    pub const fn is_initialized(&self) -> bool {
        self.cache_initialized
    }

    /// Returns the cached instrument symbols.
    #[must_use]
    pub fn get_cached_symbols(&self) -> Vec<String> {
        self.instruments_cache
            .lock()
            .unwrap()
            .keys()
            .map(std::string::ToString::to_string)
            .collect()
    }

    /// Adds the `instruments` to the clients instrument cache.
    ///
    /// Any existing instruments will be replaced.
    pub fn add_instruments(&mut self, instruments: Vec<InstrumentAny>) {
        for inst in instruments {
            self.instruments_cache
                .lock()
                .unwrap()
                .insert(inst.raw_symbol().inner(), inst);
        }
        self.cache_initialized = true;
    }

    /// Adds the `instrument` to the clients instrument cache.
    ///
    /// Any existing instrument will be replaced.
    pub fn add_instrument(&mut self, instrument: InstrumentAny) {
        self.instruments_cache
            .lock()
            .unwrap()
            .insert(instrument.raw_symbol().inner(), instrument);
        self.cache_initialized = true;
    }

    /// Requests the account state for the `account_id` from OKX.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or no account state is returned.
    pub async fn request_account_state(
        &self,
        account_id: AccountId,
    ) -> anyhow::Result<AccountState> {
        let resp = self
            .inner
            .http_get_balance()
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let ts_init = self.generate_ts_init();
        let raw = resp
            .first()
            .ok_or_else(|| anyhow::anyhow!("No account state returned from OKX"))?;
        let account_state = parse_account_state(raw, account_id, ts_init)?;

        Ok(account_state)
    }

    /// Sets the position mode for the account.
    ///
    /// Defaults to NetMode if no position mode is provided.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the position mode cannot be set.
    ///
    /// # Note
    ///
    /// This endpoint only works for accounts with derivatives trading enabled.
    /// If the account only has spot trading, this will return an error.
    pub async fn set_position_mode(&self, position_mode: OKXPositionMode) -> anyhow::Result<()> {
        let mut params = SetPositionModeParamsBuilder::default();
        params.pos_mode(position_mode);
        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        match self.inner.http_set_position_mode(params).await {
            Ok(_) => Ok(()),
            Err(e) => {
                // Check if this is the "Invalid request type" error for accounts without derivatives
                if let crate::http::error::OKXHttpError::OkxError {
                    error_code,
                    message,
                } = &e
                {
                    if error_code == "50115" {
                        tracing::warn!(
                            "Account does not support position mode setting (derivatives trading not enabled): {message}"
                        );
                        return Ok(()); // Gracefully handle this case
                    }
                }
                anyhow::bail!(e)
            }
        }
    }

    /// Requests all instruments for the `instrument_type` from OKX.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or instrument parsing fails.
    pub async fn request_instruments(
        &self,
        instrument_type: OKXInstrumentType,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let mut params = GetInstrumentsParamsBuilder::default();
        params.inst_type(instrument_type);
        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let resp = self
            .inner
            .http_get_instruments(params)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let ts_init = self.generate_ts_init();

        let mut instruments: Vec<InstrumentAny> = Vec::new();

        for inst in &resp {
            let instrument_any = parse_instrument_any(inst, ts_init)?;
            if let Some(instrument_any) = instrument_any {
                instruments.push(instrument_any);
            }
        }

        Ok(instruments)
    }

    /// Requests the latest mark price for the `instrument_type` from OKX.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or no mark price is returned.
    pub async fn request_mark_price(
        &self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<MarkPriceUpdate> {
        let mut params = GetMarkPriceParamsBuilder::default();
        params.inst_id(instrument_id.symbol.inner());
        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let resp = self
            .inner
            .http_get_mark_price(params)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let raw = resp
            .first()
            .ok_or_else(|| anyhow::anyhow!("No mark price returned from OKX"))?;
        let inst = self.get_instrument_from_cache(instrument_id.symbol.inner())?;
        let ts_init = self.generate_ts_init();

        let mark_price =
            parse_mark_price_update(raw, instrument_id, inst.price_precision(), ts_init)
                .map_err(|e| anyhow::anyhow!(e))?;
        Ok(mark_price)
    }

    /// Requests the latest index price for the `instrument_id` from OKX.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or no index price is returned.
    pub async fn request_index_price(
        &self,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<IndexPriceUpdate> {
        let mut params = GetIndexTickerParamsBuilder::default();
        params.inst_id(instrument_id.symbol.inner());
        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let resp = self
            .inner
            .http_get_index_ticker(params)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let raw = resp
            .first()
            .ok_or_else(|| anyhow::anyhow!("No index price returned from OKX"))?;
        let inst = self.get_instrument_from_cache(instrument_id.symbol.inner())?;
        let ts_init = self.generate_ts_init();

        let index_price =
            parse_index_price_update(raw, instrument_id, inst.price_precision(), ts_init)
                .map_err(|e| anyhow::anyhow!(e))?;
        Ok(index_price)
    }

    /// Requests trades for the `instrument_id` and `start` -> `end` time range.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or trade parsing fails.
    pub async fn request_trades(
        &self,
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<TradeTick>> {
        let mut params = GetTradesParamsBuilder::default();

        params.inst_id(instrument_id.symbol.inner());
        if let Some(s) = start {
            params.before(s.timestamp_millis().to_string());
        }
        if let Some(e) = end {
            params.after(e.timestamp_millis().to_string());
        }
        if let Some(l) = limit {
            params.limit(l);
        }

        let params = params.build().map_err(anyhow::Error::new)?;

        // Fetch raw trades
        let raw_trades = self
            .inner
            .http_get_trades(params)
            .await
            .map_err(anyhow::Error::new)?;

        let ts_init = self.generate_ts_init();
        let inst = self.get_instrument_from_cache(instrument_id.symbol.inner())?;

        let mut trades = Vec::with_capacity(raw_trades.len());
        for raw in raw_trades {
            match parse_trade_tick(
                &raw,
                instrument_id,
                inst.price_precision(),
                inst.size_precision(),
                ts_init,
            ) {
                Ok(trade) => trades.push(trade),
                Err(e) => tracing::error!("{e}"),
            }
        }

        Ok(trades)
    }

    /// Requests historical bars for the given bar type and time range.
    ///
    /// The aggregation source must be `EXTERNAL`. Time range validation ensures start < end.
    /// Returns bars sorted oldest to newest.
    ///
    /// # Endpoint Selection
    ///
    /// The OKX API has different endpoints with different limits:
    /// - Regular endpoint (`/api/v5/market/candles`): ≤ 300 rows/call, ≤ 40 req/2s
    ///   - Used when: start is None OR age ≤ 100 days
    /// - History endpoint (`/api/v5/market/history-candles`): ≤ 100 rows/call, ≤ 20 req/2s
    ///   - Used when: start is Some AND age > 100 days
    ///
    /// Age is calculated as `Utc::now() - start` at the time of the first request.
    ///
    /// # Supported Aggregations
    ///
    /// Maps to OKX bar query parameter:
    /// - `Second` → `{n}s`
    /// - `Minute` → `{n}m`
    /// - `Hour` → `{n}H`
    /// - `Day` → `{n}D`
    /// - `Week` → `{n}W`
    /// - `Month` → `{n}M`
    ///
    /// # Pagination
    ///
    /// - Uses `before` parameter for backwards pagination
    /// - Pages backwards from end time (or now) to start time
    /// - Stops when: limit reached, time window covered, or API returns empty
    /// - Rate limit safety: ≥ 50ms between requests
    ///
    /// # References
    ///
    /// - <https://tr.okx.com/docs-v5/en/#order-book-trading-market-data-get-candlesticks>
    /// - <https://tr.okx.com/docs-v5/en/#order-book-trading-market-data-get-candlesticks-history>
    pub async fn request_bars(
        &self,
        bar_type: BarType,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<Bar>> {
        // Validate aggregation source
        let source = bar_type.aggregation_source();
        if source != AggregationSource::External {
            anyhow::bail!("Invalid aggregation source: {source:?}");
        }

        // Validate time range consistency
        if let (Some(start_time), Some(end_time)) = (start, end) {
            if start_time >= end_time {
                anyhow::bail!("Invalid time range: start={start_time:?} end={end_time:?}");
            }
        }

        let symbol = bar_type.instrument_id().symbol;

        // Validate instrument is in cache
        let inst = self.get_instrument_from_cache(symbol.inner())?;

        let spec = bar_type.spec();

        // Map aggregation to OKX bar parameter
        let bar_str = match spec.aggregation {
            BarAggregation::Second => {
                let step = spec.step.get();
                format!("{step}s")
            }
            BarAggregation::Minute => {
                let step = spec.step.get();
                format!("{step}m")
            }
            BarAggregation::Hour => {
                let step = spec.step.get();
                format!("{step}H")
            }
            BarAggregation::Day => {
                let step = spec.step.get();
                format!("{step}D")
            }
            BarAggregation::Week => {
                let step = spec.step.get();
                format!("{step}W")
            }
            BarAggregation::Month => {
                let step = spec.step.get();
                format!("{step}M")
            }
            agg => {
                anyhow::bail!("OKX does not support {agg:?} aggregation");
            }
        };

        // Determine endpoint based on data age
        let now = Utc::now();
        let use_history_endpoint = if let Some(start_time) = start {
            let age = now - start_time;
            age.num_days() > 100
        } else {
            false
        };

        let endpoint_max = if use_history_endpoint { 100 } else { 300 };

        // Pre-allocate result vector
        let mut all_bars = Vec::with_capacity(limit.unwrap_or_default() as usize);

        // Determine cursor strategy
        let (cursor_mode, mut cursor_value) = match (start, end) {
            (Some(start_time), Some(_)) => {
                // Both start & end provided - forward pagination
                ("after", Some(start_time.timestamp_millis().to_string()))
            }
            (Some(start_time), None) => {
                // Only start provided - forward pagination
                ("after", Some(start_time.timestamp_millis().to_string()))
            }
            (None, Some(end_time)) => {
                // Only end provided - backward pagination
                ("before", Some(end_time.timestamp_millis().to_string()))
            }
            (None, None) => {
                // No bounds with None/0 limit - one-shot
                // No bounds with limit > endpoint_max - backward pagination
                if limit.is_none() || limit == Some(0) {
                    ("none", None)
                } else {
                    ("before", Some(now.timestamp_millis().to_string()))
                }
            }
        };

        // For backward pagination, we need to reverse the final result
        let needs_final_reverse = matches!(cursor_mode, "before");

        // Pagination loop
        loop {
            // Calculate effective limit for this page
            let effective_limit = if let Some(l) = limit {
                if l == 0 {
                    // Zero limit treated as unbounded
                    endpoint_max
                } else {
                    let remaining = l as usize - all_bars.len();
                    if remaining == 0 {
                        break; // Stop condition: limit reached
                    }
                    remaining.min(endpoint_max)
                }
            } else {
                endpoint_max
            };

            // Build request parameters
            let mut builder = GetCandlesticksParamsBuilder::default();
            builder.inst_id(symbol.as_str());
            builder.bar(&bar_str);
            builder.limit(effective_limit as u32);

            // Set cursor based on pagination mode
            match cursor_mode {
                "after" => {
                    if let Some(ref cursor) = cursor_value {
                        builder.after(cursor.clone());
                    }
                }
                "before" => {
                    if let Some(ref cursor) = cursor_value {
                        builder.before(cursor.clone());
                    }
                }
                "none" => {
                    // No cursor for one-shot requests
                }
                _ => unreachable!(),
            }

            let params = builder.build().map_err(anyhow::Error::new)?;

            // Make HTTP request
            let page = if use_history_endpoint {
                self.inner
                    .http_get_candlesticks_history(params)
                    .await
                    .map_err(anyhow::Error::new)?
            } else {
                self.inner
                    .http_get_candlesticks(params)
                    .await
                    .map_err(anyhow::Error::new)?
            };

            // Logging
            let endpoint = if use_history_endpoint {
                "history"
            } else {
                "regular"
            };
            tracing::debug!(
                "Requesting bars: endpoint={endpoint}, instId={symbol}, bar={bar_str}, effective_limit={effective_limit}, page_rows={page_rows}, accum_rows={accum_rows}, cursor={cursor_type}={cursor_value}",
                page_rows = page.len(),
                accum_rows = all_bars.len(),
                cursor_type = cursor_mode,
                cursor_value = cursor_value.as_deref().unwrap_or("none")
            );

            // Stop condition: API returns empty data
            if page.is_empty() {
                break;
            }

            // Process page based on cursor mode
            let mut page_bars = Vec::with_capacity(page.len());
            let ts_init = self.generate_ts_init();

            for raw in &page {
                let bar = parse_candlestick(
                    raw,
                    bar_type,
                    inst.price_precision(),
                    inst.size_precision(),
                    ts_init,
                )?;
                page_bars.push(bar);
            }

            // Handle ordering based on cursor mode
            match cursor_mode {
                "after" => {
                    // Forward pagination: reverse each page, then append
                    page_bars.reverse();
                }
                "before" => {
                    // Backward pagination: keep API order (desc), append
                    // No reversal needed here
                }
                "none" => {
                    // One-shot: reverse to get oldest → newest
                    page_bars.reverse();
                }
                _ => unreachable!(),
            }

            // Add to results with limit checking
            for bar in page_bars {
                if let Some(l) = limit {
                    if all_bars.len() >= l as usize {
                        break; // Stop condition: limit reached
                    }
                }
                all_bars.push(bar);
            }

            // Stop condition: Time window fully covered
            if let (Some(start_time), Some(end_time)) = (start, end) {
                if let (Some(first_bar), Some(last_bar)) = (all_bars.first(), all_bars.last()) {
                    let start_nanos = start_time.timestamp_nanos_opt().unwrap_or_default() as u64;
                    let end_nanos = end_time.timestamp_nanos_opt().unwrap_or_default() as u64;
                    if first_bar.ts_event <= start_nanos && last_bar.ts_event >= end_nanos {
                        break;
                    }
                }
            }

            // Check if we've reached the limit
            if let Some(l) = limit {
                if all_bars.len() >= l as usize {
                    break; // Stop condition: limit reached
                }
            }

            // Update cursor for next page
            match cursor_mode {
                "after" => {
                    // Forward pagination: use oldest timestamp from current page
                    if let Some(last_raw) = page.last() {
                        cursor_value = Some(last_raw.0.clone());
                    } else {
                        break;
                    }
                }
                "before" => {
                    // Backward pagination: use newest timestamp from current page
                    if let Some(first_raw) = page.first() {
                        cursor_value = Some(first_raw.0.clone());
                    } else {
                        break;
                    }
                }
                "none" => {
                    // One-shot: no pagination
                    break;
                }
                _ => unreachable!(),
            }

            // Rate limit safety
            tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        }

        // Final processing based on cursor mode
        if needs_final_reverse {
            all_bars.reverse();
        }

        Ok(all_bars)
    }

    /// Requests historical order status reports for the given parameters.
    ///
    /// # References
    ///
    /// - <https://www.okx.com/docs-v5/en/#order-book-trading-trade-get-order-history-last-7-days>.
    /// - <https://www.okx.com/docs-v5/en/#order-book-trading-trade-get-order-history-last-3-months>.
    #[allow(clippy::too_many_arguments)]
    pub async fn request_order_status_reports(
        &self,
        account_id: AccountId,
        instrument_type: Option<OKXInstrumentType>,
        instrument_id: Option<InstrumentId>,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        open_only: bool,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        // Build params for order history
        let mut history_params = GetOrderHistoryParamsBuilder::default();

        let instrument_type = if let Some(instrument_type) = instrument_type {
            instrument_type
        } else {
            let instrument_id = instrument_id.ok_or_else(|| {
                anyhow::anyhow!("Instrument ID required if `instrument_type` not provided")
            })?;
            let instrument = self.get_instrument_from_cache(instrument_id.symbol.inner())?;
            okx_instrument_type(&instrument)?
        };

        history_params.inst_type(instrument_type);

        if let Some(instrument_id) = instrument_id.as_ref() {
            history_params.inst_id(instrument_id.symbol.inner().to_string());
        }

        if let Some(limit) = limit {
            history_params.limit(limit);
        }

        let history_params = history_params.build().map_err(|e| anyhow::anyhow!(e))?;

        // Build params for pending orders
        let mut pending_params = GetOrderListParamsBuilder::default();
        pending_params.inst_type(instrument_type);

        if let Some(instrument_id) = instrument_id.as_ref() {
            pending_params.inst_id(instrument_id.symbol.inner().to_string());
        }

        if let Some(limit) = limit {
            pending_params.limit(limit);
        }

        let pending_params = pending_params.build().map_err(|e| anyhow::anyhow!(e))?;

        let combined_resp = if open_only {
            // Only request pending/open orders
            self.inner
                .http_get_order_list(pending_params)
                .await
                .map_err(|e| anyhow::anyhow!(e))?
        } else {
            // Make both requests concurrently
            let (history_resp, pending_resp) = tokio::try_join!(
                self.inner.http_get_order_history(history_params),
                self.inner.http_get_order_list(pending_params)
            )
            .map_err(|e| anyhow::anyhow!(e))?;

            // Combine both responses
            let mut combined_resp = history_resp;
            combined_resp.extend(pending_resp);
            combined_resp
        };

        // Prepare time range filter
        let start_ns = start.map(UnixNanos::from);
        let end_ns = end.map(UnixNanos::from);

        let ts_init = self.generate_ts_init();
        let mut reports = Vec::with_capacity(combined_resp.len());

        // Use a seen filter in case pending orders are within the histories "2hr reserve window"
        let mut seen = AHashSet::new();

        for order in combined_resp {
            if seen.contains(&order.cl_ord_id) {
                continue; // Reserved pending already reported
            }

            seen.insert(order.cl_ord_id);

            let inst = self.get_instrument_from_cache(order.inst_id)?;

            let report = parse_order_status_report(
                order,
                account_id,
                inst.id(),
                inst.price_precision(),
                inst.size_precision(),
                ts_init,
            );

            if let Some(start_ns) = start_ns {
                if report.ts_last < start_ns {
                    continue;
                }
            }

            if let Some(end_ns) = end_ns {
                if report.ts_last > end_ns {
                    continue;
                }
            }

            reports.push(report);
        }

        Ok(reports)
    }

    /// Requests fill reports (transaction details) for the given parameters.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-trade-get-transaction-details-last-3-days>.
    pub async fn request_fill_reports(
        &self,
        account_id: AccountId,
        instrument_type: Option<OKXInstrumentType>,
        instrument_id: Option<InstrumentId>,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<FillReport>> {
        let mut params = GetTransactionDetailsParamsBuilder::default();

        let instrument_type = if let Some(instrument_type) = instrument_type {
            instrument_type
        } else {
            let instrument_id = instrument_id.ok_or_else(|| {
                anyhow::anyhow!("Instrument ID required if `instrument_type` not provided")
            })?;
            let instrument = self.get_instrument_from_cache(instrument_id.symbol.inner())?;
            okx_instrument_type(&instrument)?
        };

        params.inst_type(instrument_type);

        if let Some(instrument_id) = instrument_id {
            let instrument = self.get_instrument_from_cache(instrument_id.symbol.inner())?;
            let instrument_type = okx_instrument_type(&instrument)?;
            params.inst_type(instrument_type);
            params.inst_id(instrument_id.symbol.inner().to_string());
        }

        if let Some(limit) = limit {
            params.limit(limit);
        }

        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let resp = self
            .inner
            .http_get_transaction_details(params)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        // Prepare time range filter
        let start_ns = start.map(UnixNanos::from);
        let end_ns = end.map(UnixNanos::from);

        let ts_init = self.generate_ts_init();
        let mut reports = Vec::with_capacity(resp.len());

        for detail in resp {
            let inst = self.get_instrument_from_cache(detail.inst_id)?;

            let report = parse_fill_report(
                detail,
                account_id,
                inst.id(),
                inst.price_precision(),
                inst.size_precision(),
                ts_init,
            )?;

            if let Some(start_ns) = start_ns {
                if report.ts_event < start_ns {
                    continue;
                }
            }

            if let Some(end_ns) = end_ns {
                if report.ts_event > end_ns {
                    continue;
                }
            }

            reports.push(report);
        }

        Ok(reports)
    }

    /// Requests current position status reports for the given parameters.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#trading-account-rest-api-get-positions>.
    pub async fn request_position_status_reports(
        &self,
        account_id: AccountId,
        instrument_type: Option<OKXInstrumentType>,
        instrument_id: Option<InstrumentId>,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let mut params = GetPositionsParamsBuilder::default();

        if let Some(instrument_type) = instrument_type {
            params.inst_type(instrument_type);
        }

        instrument_id
            .as_ref()
            .map(|i| params.inst_id(i.symbol.inner()));

        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let resp = self
            .inner
            .http_get_positions(params)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let ts_init = self.generate_ts_init();
        let mut reports = Vec::with_capacity(resp.len());

        for position in resp {
            if position.pos_id.is_some() {
                continue; // Skip hedge mode positions (not currently supported)
            }
            let inst = self.get_instrument_from_cache(position.inst_id)?;

            let report = parse_position_status_report(
                position,
                account_id,
                inst.id(),
                inst.size_precision(),
                ts_init,
            );
            reports.push(report);
        }

        Ok(reports)
    }
}
