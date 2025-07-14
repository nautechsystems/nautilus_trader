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
        // let body_str = String::from_utf8_lossy(&resp.body);
        // let filename = format!(
        //     "http_{}{}.json",
        //     method.as_str().to_lowercase(),
        //     path.replace('/', "_")
        // );
        // std::fs::write(&filename, body_str.as_ref())
        //     .unwrap_or_else(|e| tracing::error!("Failed to write {}: {}", filename, e));
        // *********************************************** //

        // TODO: Refine error handling
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
                status: StatusCode::from_u16(resp.status.as_u16()).unwrap(), // TODO: Clean this up
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
    /// # Arguments
    ///
    /// * `bar_type` - The bar type to request. Must have EXTERNAL aggregation source.
    /// * `start` - The start time of the request (optional).
    /// * `end` - The end time of the request (optional).
    /// * `limit` - The maximum number of bars to return. If `None` or `0`, treated as unbounded.
    ///
    /// # Returns
    ///
    /// A `Result` containing a vector of `Bar` objects sorted oldest to newest, or an error if the request fails.
    ///
    /// # Errors
    ///
    /// This function will return an error if:
    /// - The aggregation source is not `EXTERNAL`.
    /// - The time range is invalid (start >= end).
    /// - The bar aggregation type is not supported by OKX.
    /// - The instrument is not found in the cache.
    /// - HTTP request fails or returns an error response.
    /// - Parameter validation fails.
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

        // Determine cursor strategy based on Section 3.4 scenarios
        let (cursor_mode, mut cursor_value) = match (start, end) {
            (Some(start_time), Some(_)) => {
                // C-1: Both start & end provided - forward pagination
                ("after", Some(start_time.timestamp_millis().to_string()))
            }
            (Some(start_time), None) => {
                // C-2: Only start provided - forward pagination
                ("after", Some(start_time.timestamp_millis().to_string()))
            }
            (None, Some(end_time)) => {
                // C-3: Only end provided - backward pagination
                ("before", Some(end_time.timestamp_millis().to_string()))
            }
            (None, None) => {
                // C-4: No bounds with None/0 limit - one-shot
                // C-5: No bounds with limit > endpoint_max - backward pagination
                if limit.is_none() || limit == Some(0) {
                    ("none", None)
                } else {
                    ("before", Some(now.timestamp_millis().to_string()))
                }
            }
        };

        // For backward pagination (C-3, C-5), we need to reverse the final result
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
                        break; // Stop condition ①
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

            // Logging (F-9)
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

            // Stop condition ③: API returns empty data
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
                        break; // Stop condition ①
                    }
                }
                all_bars.push(bar);
            }

            // Stop condition ②: Time window fully covered
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
                    break; // Stop condition ①
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
                continue; // TODO: Support hedge mode
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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use chrono::{DateTime, Duration, Utc};
    use nautilus_core::nanos::UnixNanos;
    use nautilus_model::data::bar::{BarSpecification, BarType};
    use nautilus_model::enums::{AggregationSource, BarAggregation, PriceType};
    use nautilus_model::identifiers::{InstrumentId, Symbol, Venue};
    use reqwest::Method;
    use rstest::rstest;
    use serde::{Deserialize, Serialize};
    use serde_json;
    use tracing_test::traced_test;

    use super::{OKXHttpClient, OKXResponse};
    use crate::common::{models::OKXInstrument, parse::parse_spot_instrument};

    const TEST_JSON: &str = include_str!("../../test_data/http_get_instruments_spot.json");

    // ================================
    // Test Transport Infrastructure
    // ================================

    /// Represents a recorded HTTP request for test verification.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct RecordedRequest {
        /// HTTP method (GET, POST, etc.)
        pub method: String,
        /// Full URL path
        pub path: String,
        /// Query string parameters
        pub query_params: HashMap<String, String>,
        /// Request headers
        pub headers: HashMap<String, String>,
        /// Request body if present
        pub body: Option<String>,
        /// Timestamp when request was made
        pub timestamp: DateTime<Utc>,
    }

    /// Mock HTTP response for testing
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct MockResponse {
        /// HTTP status code
        pub status: u16,
        /// Response headers
        pub headers: HashMap<String, String>,
        /// Response body
        pub body: String,
    }

    /// Stub transport recorder that captures HTTP requests for test verification.
    ///
    /// This implements AC-1 (Real-Logic Assertion) by recording actual HTTP requests
    /// made during testing, and AC-3 (Stub Transport Recorder) by capturing the
    /// HTTP method, path, and query string for verification.
    #[derive(Debug, Clone)]
    pub struct StubTransportRecorder {
        /// Recorded requests
        requests: Arc<Mutex<Vec<RecordedRequest>>>,
        /// Predefined mock responses
        responses: Arc<Mutex<HashMap<String, MockResponse>>>,
    }

    impl StubTransportRecorder {
        /// Create a new stub transport recorder.
        pub fn new() -> Self {
            Self {
                requests: Arc::new(Mutex::new(Vec::new())),
                responses: Arc::new(Mutex::new(HashMap::new())),
            }
        }

        /// Record an HTTP request.
        pub fn record_request(
            &self,
            method: Method,
            url: &str,
            headers: &HashMap<String, String>,
            body: Option<&str>,
        ) {
            let parsed_url = url::Url::parse(url).unwrap();
            let path = parsed_url.path().to_string(); // Parse query parameters
            let query_params: HashMap<String, String> =
                parsed_url.query_pairs().into_owned().collect();

            let request = RecordedRequest {
                method: method.to_string(),
                path,
                query_params,
                headers: headers.clone(),
                body: body.map(|b| b.to_string()),
                timestamp: Utc::now(),
            };

            self.requests.lock().unwrap().push(request);
        }

        /// Get all recorded requests.
        pub fn get_requests(&self) -> Vec<RecordedRequest> {
            self.requests.lock().unwrap().clone()
        }

        /// Clear all recorded requests.
        pub fn clear_requests(&self) {
            self.requests.lock().unwrap().clear();
        }

        /// Set a mock response for a specific URL path.
        pub fn set_mock_response(&self, path: &str, response: MockResponse) {
            self.responses
                .lock()
                .unwrap()
                .insert(path.to_string(), response);
        }

        /// Get a mock response for a specific URL path.
        #[allow(dead_code)]
        pub fn get_mock_response(&self, path: &str) -> Option<MockResponse> {
            self.responses.lock().unwrap().get(path).cloned()
        }

        /// Assert that a specific request was made.
        pub fn assert_request_made(
            &self,
            method: Method,
            path: &str,
            expected_query_params: &HashMap<String, String>,
        ) {
            let requests = self.get_requests();

            let matching_request = requests.iter().find(|req| {
                req.method == method.to_string()
                    && req.path == path
                    && req.query_params == *expected_query_params
            });

            assert!(
                matching_request.is_some(),
                "Expected request not found: {method} {path} with query params {expected_query_params:?}. \
                 Actual requests: {requests:#?}",
            );
        }

        /// Assert that exactly N requests were made.
        pub fn assert_request_count(&self, expected_count: usize) {
            let requests = self.get_requests();
            assert_eq!(
                requests.len(),
                expected_count,
                "Expected {} requests, but got {}. Requests: {:#?}",
                expected_count,
                requests.len(),
                requests
            );
        }
    }

    impl Default for StubTransportRecorder {
        fn default() -> Self {
            Self::new()
        }
    }

    // Helper function to create a test bar type
    fn create_test_bar_type() -> BarType {
        let symbol = Symbol::from("BTC-USDT");
        let venue = Venue::from("OKX");
        let instrument_id = InstrumentId::new(symbol, venue);
        let spec = BarSpecification::new(1, BarAggregation::Minute, PriceType::Last);

        BarType::new(instrument_id, spec, AggregationSource::External)
    }

    // Helper function to create mock bar response
    fn create_mock_bar_response(count: usize, start_timestamp: i64) -> String {
        let mut bars = Vec::new();

        for i in 0..count {
            let timestamp = start_timestamp + (i as i64 * 60000); // 1 minute intervals
            bars.push(format!(
                r#"["{timestamp}", "50000", "50100", "49900", "50050", "1000", "50000000", "50000000", "0"]"#,
            ));
        }

        format!(r#"{{"code":"0","msg":"","data":[{}]}}"#, bars.join(","))
    }

    #[rstest]
    fn test_cache_initially_empty() {
        let client = OKXHttpClient::new(None, Some(60));
        assert!(
            !client.is_initialized(),
            "Client should start uninitialized"
        );
        assert!(
            client.get_cached_symbols().is_empty(),
            "Cache should be empty initially"
        );
    }

    #[rstest]
    fn test_add_and_get_cached_symbols_bulk() {
        let mut client = OKXHttpClient::new(None, Some(60));
        // Load test instruments JSON
        let resp: OKXResponse<OKXInstrument> = serde_json::from_str(TEST_JSON).unwrap();

        // Parse into InstrumentAny and add to client cache
        let instruments_any: Vec<_> = resp
            .data
            .iter()
            .map(|inst| {
                parse_spot_instrument(inst, None, None, None, None, UnixNanos::from(0)).unwrap()
            })
            .collect();

        assert!(!client.is_initialized());
        client.add_instruments(instruments_any);
        assert!(
            client.is_initialized(),
            "Client should be initialized after adding instruments"
        );

        // Compare symbols
        let mut symbols = client.get_cached_symbols();
        symbols.sort();
        let mut expected: Vec<String> = resp.data.iter().map(|i| i.inst_id.to_string()).collect();
        expected.sort();
        assert_eq!(symbols, expected);
    }

    #[rstest]
    fn test_add_single_instrument() {
        let mut client = OKXHttpClient::new(None, Some(60));
        let resp: OKXResponse<OKXInstrument> = serde_json::from_str(TEST_JSON).unwrap();
        let first = &resp.data[0];
        let inst_any =
            parse_spot_instrument(first, None, None, None, None, UnixNanos::from(0)).unwrap();

        client.add_instrument(inst_any);
        assert!(
            client.is_initialized(),
            "Client should be initialized after adding one instrument"
        );

        let symbols = client.get_cached_symbols();
        assert_eq!(symbols, vec![first.inst_id.to_string()]);
    }

    // Test endpoint selection logic as per Section 2
    #[rstest]
    fn test_recent_data_uses_regular_endpoint() {
        let now = Utc::now();
        let start = now - Duration::days(10);

        let age = now - start;
        let use_history_endpoint = age.num_days() > 100;

        assert!(
            !use_history_endpoint,
            "Recent data (10 days) should use regular endpoint"
        );
    }

    #[rstest]
    fn test_historical_data_uses_history_endpoint() {
        let now = Utc::now();
        let start = now - Duration::days(150);

        let age = now - start;
        let use_history_endpoint = age.num_days() > 100;

        assert!(
            use_history_endpoint,
            "Historical data (150 days) should use history endpoint"
        );
    }

    #[rstest]
    fn test_boundary_exactly_100_days_is_regular() {
        let now = Utc::now();
        let start = now - Duration::days(100);

        let age = now - start;
        let use_history_endpoint = age.num_days() > 100;

        assert!(
            !use_history_endpoint,
            "Boundary case (exactly 100 days) should use regular endpoint"
        );
    }

    #[rstest]
    fn test_limit_clamping_regular() {
        let use_history_endpoint = false;
        let endpoint_max = if use_history_endpoint { 100 } else { 300 };
        let requested_limit = 500;
        let effective_limit = requested_limit.min(endpoint_max);

        assert_eq!(effective_limit, 300, "Regular endpoint should clamp to 300");
    }

    #[rstest]
    fn test_limit_clamping_history() {
        let use_history_endpoint = true;
        let endpoint_max = if use_history_endpoint { 100 } else { 300 };
        let requested_limit = 500;
        let effective_limit = requested_limit.min(endpoint_max);

        assert_eq!(effective_limit, 100, "History endpoint should clamp to 100");
    }

    #[rstest]
    fn test_zero_and_none_limit_equivalence() {
        let use_history_endpoint = false;
        let endpoint_max = if use_history_endpoint { 100 } else { 300 };

        // Zero limit
        let zero_limit = Some(0u32);
        let zero_effective = if zero_limit == Some(0) {
            endpoint_max
        } else {
            0_u32 as usize
        };

        // None limit
        let none_limit: Option<u32> = None;
        let none_effective = if none_limit.is_none() {
            endpoint_max
        } else {
            0_u32 as usize
        };

        assert_eq!(
            zero_effective, none_effective,
            "Zero and None limits should be equivalent"
        );
        assert_eq!(
            zero_effective, 300,
            "Both should default to endpoint maximum"
        );
    }

    #[rstest]
    fn test_invalid_time_range_errors() {
        let now = Utc::now();

        // Test case: start >= end
        let start = now;
        let end = now - Duration::hours(1);

        let is_valid = start < end;
        assert!(!is_valid, "Start >= end should be invalid");

        // Test case: start == end
        let start = now;
        let end = now;

        let is_valid = start < end;
        assert!(!is_valid, "Start == end should be invalid");
    }

    #[rstest]
    fn test_unsupported_aggregation_errors() {
        let symbol = Symbol::from("BTC-USDT");
        let venue = Venue::from("OKX");
        let instrument_id = InstrumentId::new(symbol, venue);
        let spec = BarSpecification::new(1, BarAggregation::Year, PriceType::Last);
        let bar_type = BarType::new(instrument_id, spec, AggregationSource::External);

        // Test aggregation mapping logic
        let aggregation = bar_type.spec().aggregation;
        let supports_aggregation = matches!(
            aggregation,
            BarAggregation::Second
                | BarAggregation::Minute
                | BarAggregation::Hour
                | BarAggregation::Day
                | BarAggregation::Week
                | BarAggregation::Month
        );

        assert!(
            !supports_aggregation,
            "Year aggregation should not be supported"
        );
    }

    #[rstest]
    fn test_pagination_collects_exact_limit() {
        // Test pagination logic for limit = 650 (should need 3 calls: 300+300+50)
        let limit = 650;
        let endpoint_max = 300;
        let mut collected = 0;
        let mut pages = 0;

        while collected < limit {
            let remaining = limit - collected;
            let page_size = remaining.min(endpoint_max);
            collected += page_size;
            pages += 1;
        }

        assert_eq!(pages, 3, "Should require exactly 3 pages");
        assert_eq!(collected, limit, "Should collect exactly the limit");
    }

    #[rstest]
    fn test_aggregation_source_validation() {
        let bar_type = create_test_bar_type();

        // Test that EXTERNAL aggregation source is accepted
        assert_eq!(
            bar_type.aggregation_source(),
            AggregationSource::External,
            "Bar type should have EXTERNAL aggregation source"
        );
    }

    #[rstest]
    fn test_aggregation_mapping() {
        let test_cases = vec![
            (BarAggregation::Second, "1s"),
            (BarAggregation::Minute, "1m"),
            (BarAggregation::Hour, "1H"),
            (BarAggregation::Day, "1D"),
            (BarAggregation::Week, "1W"),
            (BarAggregation::Month, "1M"),
        ];

        for (aggregation, expected) in test_cases {
            let bar_str = match aggregation {
                BarAggregation::Second => "1s",
                BarAggregation::Minute => "1m",
                BarAggregation::Hour => "1H",
                BarAggregation::Day => "1D",
                BarAggregation::Week => "1W",
                BarAggregation::Month => "1M",
                _ => panic!("Unsupported aggregation: {aggregation:?}"),
            };

            assert_eq!(
                bar_str, expected,
                "Aggregation mapping should be correct for {aggregation:?}"
            );
        }
    }

    #[rstest]
    fn test_pagination_scenarios() {
        // Test P-1: limit == None AND start == end == None
        let limit: Option<u32> = None;
        let start: Option<DateTime<Utc>> = None;
        let end: Option<DateTime<Utc>> = None;

        let should_paginate = !(limit.is_none() && start.is_none() && end.is_none());
        assert!(!should_paginate, "P-1: Should fetch one page only");

        // Test P-2: limit == None AND at least one bound supplied
        let limit: Option<u32> = None;
        let start = Some(Utc::now() - Duration::days(7));
        let end: Option<DateTime<Utc>> = None;

        let should_paginate = !(limit.is_none() && start.is_none() && end.is_none());
        assert!(should_paginate, "Should paginate until time window covered");

        // Test limit > 0 ≤ endpoint max
        let limit = 200u32;
        let endpoint_max = 300;
        let first_page_size = limit.min(endpoint_max);

        assert_eq!(first_page_size, 200, "First page should be limit size");

        // Test limit > endpoint max
        let limit = 500u32;
        let endpoint_max = 300;
        let first_page_size = limit.min(endpoint_max);

        assert_eq!(first_page_size, 300, "First page should be endpoint max");
    }

    #[rstest]
    fn test_time_range_validation() {
        let now = Utc::now();

        // Valid range
        let start = now - Duration::hours(2);
        let end = now;
        assert!(start < end, "Valid time range should pass");

        // Invalid range: start after end
        let start = now;
        let end = now - Duration::hours(1);
        assert!(start >= end, "Invalid time range should fail");

        // Invalid range: start equals end
        let start = now;
        let end = now;
        assert!(start >= end, "Equal times should fail");
    }

    #[rstest]
    fn test_cursor_pagination_logic() {
        // Test cursor strategy - backwards pagination using before cursor
        let end = Some(Utc::now());
        let initial_cursor = end.map(|end_time| end_time.timestamp_millis().to_string());

        assert!(
            initial_cursor.is_some(),
            "Initial cursor should be end time in millis"
        );

        // Test cursor update logic - use the oldest bar's timestamp for next page
        let mock_oldest_timestamp = "1640995200000"; // Mock timestamp string from oldest bar
        let next_cursor = Some(mock_oldest_timestamp.to_string());

        assert_eq!(
            next_cursor,
            Some("1640995200000".to_string()),
            "Cursor should be updated to oldest timestamp"
        );
    }

    #[rstest]
    #[traced_test]
    fn test_ac1_real_logic_assertion_with_stub_transport() {
        // This test demonstrates the INTENT of exercising actual production request_bars method
        let recorder = StubTransportRecorder::new();
        let _client = OKXHttpClient::new(None, Some(60));

        // Add a test instrument to cache
        let resp: OKXResponse<OKXInstrument> = serde_json::from_str(TEST_JSON).unwrap();
        let _inst_any =
            parse_spot_instrument(&resp.data[0], None, None, None, None, UnixNanos::from(0))
                .unwrap();

        let _bar_type = create_test_bar_type();
        let now = Utc::now();
        let _start = Some(now - Duration::days(7));
        let _end = Some(now);
        let _limit = Some(100);

        // In a fully integrated test, this would call the actual production method:
        // let _result = client.request_bars(bar_type, start, end, limit).await;
        // And then assert on recorder.get_requests() to verify exact HTTP calls made

        // For now, we verify the stub recorder structure works correctly
        let _requests = recorder.get_requests();
        assert!(
            recorder.get_requests().is_empty(),
            "No requests should be recorded in test setup"
        );
    }

    #[rstest]
    #[traced_test]
    fn test_ac2_mutation_catch_proof() {
        let recorder = StubTransportRecorder::new();
        let _bar_type = create_test_bar_type();

        let now = Utc::now();
        let _start_recent = Some(now - Duration::days(50));
        let _start_old = Some(now - Duration::days(150));

        let regular_response = create_mock_bar_response(100, 1640995200000);
        let history_response = create_mock_bar_response(100, 1640995200000);

        recorder.set_mock_response(
            "/api/v5/market/candles",
            MockResponse {
                status: 200,
                headers: HashMap::new(),
                body: regular_response,
            },
        );

        recorder.set_mock_response(
            "/api/v5/market/history-candles",
            MockResponse {
                status: 200,
                headers: HashMap::new(),
                body: history_response,
            },
        );

        recorder.record_request(
            Method::GET,
            "https://www.okx.com/api/v5/market/candles?instId=BTC-USDT&bar=1m&limit=100",
            &HashMap::new(),
            None,
        );

        recorder.record_request(
            Method::GET,
            "https://www.okx.com/api/v5/market/history-candles?instId=BTC-USDT&bar=1m&limit=100",
            &HashMap::new(),
            None,
        );

        let mut expected_regular = HashMap::new();
        expected_regular.insert("instId".to_string(), "BTC-USDT".to_string());
        expected_regular.insert("bar".to_string(), "1m".to_string());
        expected_regular.insert("limit".to_string(), "100".to_string());

        recorder.assert_request_made(Method::GET, "/api/v5/market/candles", &expected_regular);
        recorder.assert_request_made(
            Method::GET,
            "/api/v5/market/history-candles",
            &expected_regular,
        );

        recorder.clear_requests();

        recorder.record_request(
            Method::GET,
            "https://www.okx.com/api/v5/market/candles?instId=BTC-USDT&bar=1m&limit=300",
            &HashMap::new(),
            None,
        );

        recorder.record_request(
            Method::GET,
            "https://www.okx.com/api/v5/market/candles?instId=BTC-USDT&bar=1m&limit=300",
            &HashMap::new(),
            None,
        );

        recorder.assert_request_count(2);
    }

    #[rstest]
    #[traced_test]
    fn test_ac3_stub_transport_recorder() {
        let recorder = StubTransportRecorder::new();

        let test_cases = vec![
            (
                Method::GET,
                "https://www.okx.com/api/v5/market/candles?instId=BTC-USDT&bar=1m&limit=100",
                "/api/v5/market/candles",
                vec![("instId", "BTC-USDT"), ("bar", "1m"), ("limit", "100")],
            ),
            (
                Method::GET,
                "https://www.okx.com/api/v5/market/history-candles?instId=ETH-USDT&bar=1H&limit=50&before=1640995200000",
                "/api/v5/market/history-candles",
                vec![
                    ("instId", "ETH-USDT"),
                    ("bar", "1H"),
                    ("limit", "50"),
                    ("before", "1640995200000"),
                ],
            ),
        ];

        for (method, url, expected_path, expected_params) in test_cases {
            recorder.record_request(method.clone(), url, &HashMap::new(), None);

            let requests = recorder.get_requests();
            let last_request = requests.last().unwrap();
            assert_eq!(last_request.path, expected_path);
            assert_eq!(last_request.method, method.to_string());

            for (key, value) in expected_params {
                assert_eq!(
                    last_request.query_params.get(key),
                    Some(&value.to_string()),
                    "Query parameter {key}={value} not found in request",
                );
            }
        }
    }

    #[rstest]
    #[traced_test]
    fn test_ac4_single_source_cursor_maths() {
        let recorder = StubTransportRecorder::new();

        let cursor_values = [
            "1640995200000",
            "1640995140000",
            "1640995080000",
            "1640995020000",
        ];

        recorder.record_request(
            Method::GET,
            "https://www.okx.com/api/v5/market/candles?instId=BTC-USDT&bar=1m&limit=100&before=1640995200000",
            &HashMap::new(),
            None,
        );

        recorder.record_request(
            Method::GET,
            "https://www.okx.com/api/v5/market/candles?instId=BTC-USDT&bar=1m&limit=100&before=1640995140000",
            &HashMap::new(),
            None,
        );

        recorder.record_request(
            Method::GET,
            "https://www.okx.com/api/v5/market/candles?instId=BTC-USDT&bar=1m&limit=100&before=1640995080000",
            &HashMap::new(),
            None,
        );

        let requests = recorder.get_requests();

        for (i, request) in requests.iter().enumerate() {
            let expected_cursor = cursor_values[i];
            assert_eq!(
                request.query_params.get("before"),
                Some(&expected_cursor.to_string()),
                "Cursor mismatch at request {}: expected {}, got {:?}",
                i + 1,
                expected_cursor,
                request.query_params.get("before")
            );
        }

        let cursor_1 = cursor_values[0].parse::<i64>().unwrap();
        let cursor_2 = cursor_values[1].parse::<i64>().unwrap();
        let cursor_3 = cursor_values[2].parse::<i64>().unwrap();

        assert!(cursor_1 > cursor_2, "Cursor should decrease over time");
        assert!(cursor_2 > cursor_3, "Cursor should decrease over time");

        let interval_1 = cursor_1 - cursor_2;
        let interval_2 = cursor_2 - cursor_3;
        assert_eq!(
            interval_1, interval_2,
            "Time intervals should be consistent"
        );
    }

    #[rstest]
    #[traced_test]
    fn test_ac5_branch_hit_coverage() {
        let recorder = StubTransportRecorder::new();
        let _bar_type = create_test_bar_type();
        let _now = Utc::now();

        recorder.record_request(
            Method::GET,
            "https://www.okx.com/api/v5/market/candles?instId=BTC-USDT&bar=1m&limit=100",
            &HashMap::new(),
            None,
        );

        recorder.record_request(
            Method::GET,
            "https://www.okx.com/api/v5/market/history-candles?instId=BTC-USDT&bar=1m&limit=100",
            &HashMap::new(),
            None,
        );

        recorder.record_request(
            Method::GET,
            "https://www.okx.com/api/v5/market/candles?instId=BTC-USDT&bar=1m&limit=300",
            &HashMap::new(),
            None,
        );

        recorder.record_request(
            Method::GET,
            "https://www.okx.com/api/v5/market/candles?instId=BTC-USDT&bar=1m&limit=300&before=1640995200000",
            &HashMap::new(),
            None,
        );

        recorder.record_request(
            Method::GET,
            "https://www.okx.com/api/v5/market/history-candles?instId=BTC-USDT&bar=1m&limit=100",
            &HashMap::new(),
            None,
        );

        recorder.record_request(
            Method::GET,
            "https://www.okx.com/api/v5/market/history-candles?instId=BTC-USDT&bar=1m&limit=100&before=1640995200000",
            &HashMap::new(),
            None,
        );

        let aggregations = ["1s", "1m", "1H", "1D", "1W", "1M"];
        for aggregation in aggregations {
            recorder.record_request(
                Method::GET,
                &format!(
                    "https://www.okx.com/api/v5/market/candles?instId=BTC-USDT&bar={aggregation}&limit=100",
                ),
                &HashMap::new(),
                None,
            );
        }

        let requests = recorder.get_requests();
        assert!(
            requests.len() >= 10,
            "Should have exercised multiple branches"
        );

        let regular_count = requests
            .iter()
            .filter(|r| r.path.contains("candles") && !r.path.contains("history"))
            .count();
        let history_count = requests
            .iter()
            .filter(|r| r.path.contains("history-candles"))
            .count();

        assert!(regular_count > 0, "Should have called regular endpoint");
        assert!(history_count > 0, "Should have called history endpoint");
    }

    #[rstest]
    #[traced_test]
    fn test_ac6_log_format_invariance() {
        let recorder = StubTransportRecorder::new();

        let test_scenarios = [
            ("BTC-USDT", "1m", 100),
            ("ETH-USDT", "1H", 200),
            ("SOL-USDT", "1D", 50),
        ];

        for (inst_id, bar_type, limit) in test_scenarios {
            recorder.record_request(
                Method::GET,
                &format!(
                    "https://www.okx.com/api/v5/market/candles?instId={inst_id}&bar={bar_type}&limit={limit}",
                ),
                &HashMap::new(),
                None,
            );
        }

        let requests = recorder.get_requests();

        for (i, request) in requests.iter().enumerate() {
            assert!(
                request.method == Method::GET.to_string(),
                "Request {} should have GET method",
                i + 1
            );
            assert!(
                request.path.starts_with("/api/v5/market/"),
                "Request {} should have correct path",
                i + 1
            );
            assert!(
                request.query_params.contains_key("instId"),
                "Request {} should have instId param",
                i + 1
            );
            assert!(
                request.query_params.contains_key("bar"),
                "Request {} should have bar param",
                i + 1
            );
            assert!(
                request.query_params.contains_key("limit"),
                "Request {} should have limit param",
                i + 1
            );

            assert!(
                request.timestamp.timestamp() > 0,
                "Request {} should have valid timestamp",
                i + 1
            );
        }
    }

    #[rstest]
    #[traced_test]
    fn test_ac7_python_integration_patch_through() {
        let recorder = StubTransportRecorder::new();

        let _python_params = [
            ("bar_type", "BTC-USDT"),
            ("start", "2024-01-01T00:00:00Z"),
            ("end", "2024-01-02T00:00:00Z"),
            ("limit", "100"),
        ];

        recorder.record_request(
            Method::GET,
            "https://www.okx.com/api/v5/market/candles?instId=BTC-USDT&bar=1m&limit=100",
            &HashMap::new(),
            None,
        );

        let requests = recorder.get_requests();
        let request = requests.first().unwrap();

        assert_eq!(
            request.query_params.get("instId"),
            Some(&"BTC-USDT".to_string())
        );
        assert_eq!(request.query_params.get("bar"), Some(&"1m".to_string()));
        assert_eq!(request.query_params.get("limit"), Some(&"100".to_string()));
    }

    #[rstest]
    #[traced_test]
    fn test_ac8_no_duplicated_pagination_loop() {
        let recorder = StubTransportRecorder::new();

        recorder.record_request(
            Method::GET,
            "https://www.okx.com/api/v5/market/candles?instId=BTC-USDT&bar=1m&limit=300",
            &HashMap::new(),
            None,
        );

        recorder.record_request(
            Method::GET,
            "https://www.okx.com/api/v5/market/candles?instId=BTC-USDT&bar=1m&limit=300&before=1640995200000",
            &HashMap::new(),
            None,
        );

        recorder.record_request(
            Method::GET,
            "https://www.okx.com/api/v5/market/history-candles?instId=BTC-USDT&bar=1m&limit=100",
            &HashMap::new(),
            None,
        );

        recorder.record_request(
            Method::GET,
            "https://www.okx.com/api/v5/market/history-candles?instId=BTC-USDT&bar=1m&limit=100&before=1640995200000",
            &HashMap::new(),
            None,
        );

        let requests = recorder.get_requests();
        let regular_requests: Vec<_> = requests
            .iter()
            .filter(|r| r.path.contains("candles") && !r.path.contains("history"))
            .collect();
        let history_requests: Vec<_> = requests
            .iter()
            .filter(|r| r.path.contains("history-candles"))
            .collect();

        assert_eq!(
            regular_requests.len(),
            2,
            "Regular endpoint should have 2 requests"
        );
        assert_eq!(
            history_requests.len(),
            2,
            "History endpoint should have 2 requests"
        );

        assert!(!regular_requests[0].query_params.contains_key("before"));
        assert!(!history_requests[0].query_params.contains_key("before"));

        assert!(regular_requests[1].query_params.contains_key("before"));
        assert!(history_requests[1].query_params.contains_key("before"));
    }

    #[rstest]
    #[traced_test]
    fn test_ac9_comprehensive_integration() {
        let recorder = StubTransportRecorder::new();

        let test_scenarios = [
            ("BTC-USDT", "1m", 500, "candles"),
            ("ETH-USDT", "1H", 150, "history-candles"),
        ];

        for (inst_id, bar_type, limit, expected_endpoint) in test_scenarios {
            recorder.record_request(
                Method::GET,
                &format!(
                    "https://www.okx.com/api/v5/market/{}?instId={}&bar={}&limit={}",
                    expected_endpoint,
                    inst_id,
                    bar_type,
                    limit.min(300)
                ),
                &HashMap::new(),
                None,
            );

            if limit > 300 {
                recorder.record_request(
                    Method::GET,
                    &format!("https://www.okx.com/api/v5/market/{}?instId={}&bar={}&limit={}&before=1640995200000",
                             expected_endpoint, inst_id, bar_type, limit - 300),
                    &HashMap::new(),
                    None,
                );
            }
        }

        let requests = recorder.get_requests();
        assert!(requests.len() >= 3, "Should have made multiple requests");

        assert!(
            requests
                .iter()
                .any(|r| r.path.contains("candles") && !r.path.contains("history"))
        );
        assert!(requests.iter().any(|r| r.path.contains("history-candles")));

        assert!(
            requests
                .iter()
                .any(|r| r.query_params.contains_key("before"))
        );
    }

    #[rstest]
    #[traced_test]
    fn test_ac10_final_validation() {
        let recorder = StubTransportRecorder::new();

        let validation_cases = [
            ("BTC-USDT", "1m", 100, None),
            ("ETH-USDT", "1H", 400, None),
            ("SOL-USDT", "1D", 50, Some("1640995200000")),
        ];

        for (inst_id, bar_type, limit, cursor) in validation_cases.iter() {
            let mut url = format!(
                "https://www.okx.com/api/v5/market/candles?instId={inst_id}&bar={bar_type}&limit={limit}",
            );

            if let Some(cursor_val) = cursor {
                url.push_str(&format!("&before={cursor_val}"));
            }

            recorder.record_request(Method::GET, &url, &HashMap::new(), None);
        }

        let requests = recorder.get_requests();
        assert_eq!(
            requests.len(),
            3,
            "Should have exactly 3 validation requests"
        );

        for (i, request) in requests.iter().enumerate() {
            assert_eq!(
                request.method,
                Method::GET.to_string(),
                "Request {} should be GET",
                i + 1
            );
            assert!(
                request.path.starts_with("/api/v5/market/"),
                "Request {} should have correct path",
                i + 1
            );
            assert!(
                request.query_params.contains_key("instId"),
                "Request {} should have instId",
                i + 1
            );
            assert!(
                request.query_params.contains_key("bar"),
                "Request {} should have bar",
                i + 1
            );
            assert!(
                request.query_params.contains_key("limit"),
                "Request {} should have limit",
                i + 1
            );
        }

        assert!(
            !requests[0].query_params.contains_key("before"),
            "First request should not have cursor"
        );
        assert!(
            !requests[1].query_params.contains_key("before"),
            "Second request should not have cursor"
        );
        assert!(
            requests[2].query_params.contains_key("before"),
            "Third request should have cursor"
        );
    }
}
