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

//! Provides the HTTP client integration for the [Coinbase International](https://www.coinbase.com/en/international-exchange) REST API.
//!
//! This module defines and implements a [`CoinbaseIntxHttpClient`] for
//! sending requests to various Coinbase endpoints. It handles request signing
//! (when credentials are provided), constructs valid HTTP requests
//! using the [`HttpClient`], and parses the responses back into structured data or a [`CoinbaseIntxHttpError`].

use std::{
    collections::HashMap,
    num::NonZeroU32,
    sync::{Arc, LazyLock, Mutex},
};

use chrono::{DateTime, Utc};
use nautilus_core::{
    MUTEX_POISONED, UnixNanos, consts::NAUTILUS_USER_AGENT, env::get_or_env_var,
    time::get_atomic_clock_realtime,
};
use nautilus_model::{
    enums::{OrderSide, OrderType, TimeInForce},
    events::AccountState,
    identifiers::{AccountId, ClientOrderId, Symbol, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{Price, Quantity},
};
use nautilus_network::{http::HttpClient, ratelimiter::quota::Quota};
use reqwest::{Method, StatusCode, header::USER_AGENT};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use ustr::Ustr;

use super::{
    error::CoinbaseIntxHttpError,
    models::{
        CoinbaseIntxAsset, CoinbaseIntxBalance, CoinbaseIntxFeeTier, CoinbaseIntxFillList,
        CoinbaseIntxInstrument, CoinbaseIntxOrder, CoinbaseIntxOrderList, CoinbaseIntxPortfolio,
        CoinbaseIntxPortfolioDetails, CoinbaseIntxPortfolioFeeRates, CoinbaseIntxPortfolioSummary,
        CoinbaseIntxPosition,
    },
    parse::{
        parse_account_state, parse_fill_report, parse_instrument_any, parse_order_status_report,
        parse_position_status_report,
    },
    query::{
        CancelOrderParams, CancelOrdersParams, CreateOrderParams, CreateOrderParamsBuilder,
        GetOrderParams, GetOrdersParams, GetOrdersParamsBuilder, GetPortfolioFillsParams,
        GetPortfolioFillsParamsBuilder, ModifyOrderParams,
    },
};
use crate::{
    common::{
        consts::COINBASE_INTX_REST_URL,
        credential::Credential,
        enums::{CoinbaseIntxOrderType, CoinbaseIntxSide, CoinbaseIntxTimeInForce},
    },
    http::{
        error::ErrorBody,
        query::{CancelOrdersParamsBuilder, ModifyOrderParamsBuilder},
    },
};

/// Represents an Coinbase HTTP response.
#[derive(Debug, Serialize, Deserialize)]
pub struct CoinbaseIntxResponse<T> {
    /// The Coinbase response code, which is `"0"` for success.
    pub code: String,
    /// A message string which can be informational or describe an error cause.
    pub msg: String,
    /// The typed data returned by the Coinbase endpoint.
    pub data: Vec<T>,
}

// https://docs.cdp.coinbase.com/intx/docs/rate-limits#rest-api-rate-limits
pub static COINBASE_INTX_REST_QUOTA: LazyLock<Quota> =
    LazyLock::new(|| Quota::per_second(NonZeroU32::new(100).unwrap()));

/// Provides a lower-level HTTP client for connecting to the [Coinbase International](https://coinbase.com) REST API.
///
/// This client wraps the underlying `HttpClient` to handle functionality
/// specific to Coinbase, such as request signing (for authenticated endpoints),
/// forming request URLs, and deserializing responses into specific data models.
#[derive(Debug, Clone)]
pub struct CoinbaseIntxHttpInnerClient {
    base_url: String,
    client: HttpClient,
    credential: Option<Credential>,
}

impl Default for CoinbaseIntxHttpInnerClient {
    fn default() -> Self {
        Self::new(None, Some(60))
    }
}

impl CoinbaseIntxHttpInnerClient {
    /// Creates a new [`CoinbaseIntxHttpClient`] using the default Coinbase HTTP URL,
    /// optionally overridden with a custom base url.
    ///
    /// This version of the client has **no credentials**, so it can only
    /// call publicly accessible endpoints.
    #[must_use]
    pub fn new(base_url: Option<String>, timeout_secs: Option<u64>) -> Self {
        Self {
            base_url: base_url.unwrap_or(COINBASE_INTX_REST_URL.to_string()),
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                vec![],
                Some(*COINBASE_INTX_REST_QUOTA),
                timeout_secs,
            ),
            credential: None,
        }
    }

    /// Creates a new [`CoinbaseIntxHttpClient`] configured with credentials
    /// for authenticated requests, optionally using a custom base url.
    #[must_use]
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
                Some(*COINBASE_INTX_REST_QUOTA),
                timeout_secs,
            ),
            credential: Some(Credential::new(api_key, api_secret, api_passphrase)),
        }
    }

    /// Builds the default headers to include with each request (e.g., `User-Agent`).
    fn default_headers() -> HashMap<String, String> {
        HashMap::from([(USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string())])
    }

    /// Signs an Coinbase request with timestamp, API key, passphrase, and signature.
    ///
    /// # Errors
    ///
    /// Returns [`CoinbaseHttpError::MissingCredentials`] if no credentials are set
    /// but the request requires authentication.
    fn sign_request(
        &self,
        method: &Method,
        path: &str,
        body: Option<&[u8]>,
    ) -> Result<HashMap<String, String>, CoinbaseIntxHttpError> {
        let credential = match self.credential.as_ref() {
            Some(c) => c,
            None => return Err(CoinbaseIntxHttpError::MissingCredentials),
        };

        let api_key = credential.api_key.clone().to_string();
        let api_passphrase = credential.api_passphrase.clone().to_string();
        let timestamp = Utc::now().timestamp().to_string();
        let body_str = body
            .and_then(|b| String::from_utf8(b.to_vec()).ok())
            .unwrap_or_default();

        let signature = credential.sign(&timestamp, method.as_str(), path, &body_str);

        let mut headers = HashMap::new();
        headers.insert("Accept".to_string(), "application/json".to_string());
        headers.insert("CB-ACCESS-KEY".to_string(), api_key);
        headers.insert("CB-ACCESS-PASSPHRASE".to_string(), api_passphrase);
        headers.insert("CB-ACCESS-SIGN".to_string(), signature);
        headers.insert("CB-ACCESS-TIMESTAMP".to_string(), timestamp);
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        Ok(headers)
    }

    /// Sends an HTTP request to Coinbase International and parses the response into type `T`.
    ///
    /// Internally, this method handles:
    /// - Building the URL from `base_url` + `path`.
    /// - Optionally signing the request.
    /// - Deserializing JSON responses into typed models, or returning a [`CoinbaseIntxHttpError`].
    async fn send_request<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<Vec<u8>>,
        authenticate: bool,
    ) -> Result<T, CoinbaseIntxHttpError> {
        let url = format!("{}{}", self.base_url, path);

        let headers = if authenticate {
            Some(self.sign_request(&method, path, body.as_deref())?)
        } else {
            None
        };

        tracing::trace!("Request: {url:?} {body:?}");

        let resp = self
            .client
            .request(method.clone(), url, headers, body, None, None)
            .await?;

        tracing::trace!("Response: {resp:?}");

        if resp.status.is_success() {
            let coinbase_response: T = serde_json::from_slice(&resp.body).map_err(|e| {
                tracing::error!("Failed to deserialize CoinbaseResponse: {e}");
                CoinbaseIntxHttpError::JsonError(e.to_string())
            })?;

            Ok(coinbase_response)
        } else {
            let error_body = String::from_utf8_lossy(&resp.body);
            tracing::error!(
                "HTTP error {} with body: {error_body}",
                resp.status.as_str()
            );

            if let Ok(parsed_error) = serde_json::from_slice::<CoinbaseIntxResponse<T>>(&resp.body)
            {
                return Err(CoinbaseIntxHttpError::CoinbaseError {
                    error_code: parsed_error.code,
                    message: parsed_error.msg,
                });
            }

            if let Ok(parsed_error) = serde_json::from_slice::<ErrorBody>(&resp.body)
                && let (Some(title), Some(error)) = (parsed_error.title, parsed_error.error)
            {
                return Err(CoinbaseIntxHttpError::CoinbaseError {
                    error_code: error,
                    message: title,
                });
            }

            Err(CoinbaseIntxHttpError::UnexpectedStatus {
                status: StatusCode::from_u16(resp.status.as_u16()).unwrap(),
                body: error_body.to_string(),
            })
        }
    }

    /// Requests a list of all supported assets.
    ///
    /// See <https://docs.cdp.coinbase.com/intx/reference/getassets>.
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn http_list_assets(&self) -> Result<Vec<CoinbaseIntxAsset>, CoinbaseIntxHttpError> {
        let path = "/api/v1/assets";
        self.send_request(Method::GET, path, None, false).await
    }

    /// Requests information for a specific asset.
    ///
    /// See <https://docs.cdp.coinbase.com/intx/reference/getasset>.
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn http_get_asset_details(
        &self,
        asset: &str,
    ) -> Result<CoinbaseIntxAsset, CoinbaseIntxHttpError> {
        let path = format!("/api/v1/assets/{asset}");
        self.send_request(Method::GET, &path, None, false).await
    }

    /// Requests all instruments available for trading.
    ///
    /// See <https://docs.cdp.coinbase.com/intx/reference/getinstruments>.
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn http_list_instruments(
        &self,
    ) -> Result<Vec<CoinbaseIntxInstrument>, CoinbaseIntxHttpError> {
        let path = "/api/v1/instruments";
        self.send_request(Method::GET, path, None, false).await
    }

    /// Retrieve a list of instruments with open contracts.
    ///
    /// See <https://docs.cdp.coinbase.com/intx/reference/getinstrument>.
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn http_get_instrument_details(
        &self,
        symbol: &str,
    ) -> Result<CoinbaseIntxInstrument, CoinbaseIntxHttpError> {
        let path = format!("/api/v1/instruments/{symbol}");
        self.send_request(Method::GET, &path, None, false).await
    }

    /// Return all the fee rate tiers.
    ///
    /// See <https://docs.cdp.coinbase.com/intx/reference/getassets>.
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn http_list_fee_rate_tiers(
        &self,
    ) -> Result<Vec<CoinbaseIntxFeeTier>, CoinbaseIntxHttpError> {
        let path = "/api/v1/fee-rate-tiers";
        self.send_request(Method::GET, path, None, true).await
    }

    /// List all user portfolios.
    ///
    /// See <https://docs.cdp.coinbase.com/intx/reference/getportfolios>.
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn http_list_portfolios(
        &self,
    ) -> Result<Vec<CoinbaseIntxPortfolio>, CoinbaseIntxHttpError> {
        let path = "/api/v1/portfolios";
        self.send_request(Method::GET, path, None, true).await
    }

    /// Returns the user's specified portfolio.
    ///
    /// See <https://docs.cdp.coinbase.com/intx/reference/getportfolio>.
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn http_get_portfolio(
        &self,
        portfolio_id: &str,
    ) -> Result<CoinbaseIntxPortfolio, CoinbaseIntxHttpError> {
        let path = format!("/api/v1/portfolios/{portfolio_id}");
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Retrieves the summary, positions, and balances of a portfolio.
    ///
    /// See <https://docs.cdp.coinbase.com/intx/reference/getportfoliodetail>.
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn http_get_portfolio_details(
        &self,
        portfolio_id: &str,
    ) -> Result<CoinbaseIntxPortfolioDetails, CoinbaseIntxHttpError> {
        let path = format!("/api/v1/portfolios/{portfolio_id}/detail");
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Retrieves the high level overview of a portfolio.
    ///
    /// See <https://docs.cdp.coinbase.com/intx/reference/getportfoliosummary>.
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn http_get_portfolio_summary(
        &self,
        portfolio_id: &str,
    ) -> Result<CoinbaseIntxPortfolioSummary, CoinbaseIntxHttpError> {
        let path = format!("/api/v1/portfolios/{portfolio_id}/summary");
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Returns all balances for a given portfolio.
    ///
    /// See <https://docs.cdp.coinbase.com/intx/reference/getportfoliobalances>.
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn http_list_portfolio_balances(
        &self,
        portfolio_id: &str,
    ) -> Result<Vec<CoinbaseIntxBalance>, CoinbaseIntxHttpError> {
        let path = format!("/api/v1/portfolios/{portfolio_id}/balances");
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Retrieves the balance for a given portfolio and asset.
    ///
    /// See <https://docs.cdp.coinbase.com/intx/reference/getportfoliobalance>.
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn http_get_portfolio_balance(
        &self,
        portfolio_id: &str,
        asset: &str,
    ) -> Result<CoinbaseIntxBalance, CoinbaseIntxHttpError> {
        let path = format!("/api/v1/portfolios/{portfolio_id}/balances/{asset}");
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Returns all fills for a given portfolio.
    ///
    /// See <https://docs.cdp.coinbase.com/intx/reference/getportfoliofills>.
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn http_list_portfolio_fills(
        &self,
        portfolio_id: &str,
        params: GetPortfolioFillsParams,
    ) -> Result<CoinbaseIntxFillList, CoinbaseIntxHttpError> {
        let query = serde_urlencoded::to_string(&params)
            .map_err(|e| CoinbaseIntxHttpError::JsonError(e.to_string()))?;
        let path = format!("/api/v1/portfolios/{portfolio_id}/fills?{query}");
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Returns all positions for a given portfolio.
    ///
    /// See <https://docs.cdp.coinbase.com/intx/reference/getportfoliopositions>.
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn http_list_portfolio_positions(
        &self,
        portfolio_id: &str,
    ) -> Result<Vec<CoinbaseIntxPosition>, CoinbaseIntxHttpError> {
        let path = format!("/api/v1/portfolios/{portfolio_id}/positions");
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Retrieves the position for a given portfolio and symbol.
    ///
    /// See <https://docs.cdp.coinbase.com/intx/reference/getportfolioposition>.
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn http_get_portfolio_position(
        &self,
        portfolio_id: &str,
        symbol: &str,
    ) -> Result<CoinbaseIntxPosition, CoinbaseIntxHttpError> {
        let path = format!("/api/v1/portfolios/{portfolio_id}/positions/{symbol}");
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Retrieves the Perpetual Future and Spot fee rate tiers for the user.
    ///
    /// See <https://docs.cdp.coinbase.com/intx/reference/getportfoliosfeerates>.
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn http_list_portfolio_fee_rates(
        &self,
    ) -> Result<Vec<CoinbaseIntxPortfolioFeeRates>, CoinbaseIntxHttpError> {
        let path = "/api/v1/portfolios/fee-rates";
        self.send_request(Method::GET, path, None, true).await
    }

    /// Create a new order.
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn http_create_order(
        &self,
        params: CreateOrderParams,
    ) -> Result<CoinbaseIntxOrder, CoinbaseIntxHttpError> {
        let path = "/api/v1/orders";
        let body = serde_json::to_vec(&params)
            .map_err(|e| CoinbaseIntxHttpError::JsonError(e.to_string()))?;
        self.send_request(Method::POST, path, Some(body), true)
            .await
    }

    /// Retrieves a single order. The order retrieved can be either active or inactive.
    ///
    /// See <https://docs.cdp.coinbase.com/intx/reference/getorder>.
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn http_get_order(
        &self,
        venue_order_id: &str,
        portfolio_id: &str,
    ) -> Result<CoinbaseIntxOrder, CoinbaseIntxHttpError> {
        let params = GetOrderParams {
            portfolio: portfolio_id.to_string(),
        };
        let query = serde_urlencoded::to_string(&params)
            .map_err(|e| CoinbaseIntxHttpError::JsonError(e.to_string()))?;
        let path = format!("/api/v1/orders/{venue_order_id}?{query}");
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Returns a list of active orders resting on the order book matching the requested criteria.
    /// Does not return any rejected, cancelled, or fully filled orders as they are not active.
    ///
    /// See <https://docs.cdp.coinbase.com/intx/reference/getorders>.
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn http_list_open_orders(
        &self,
        params: GetOrdersParams,
    ) -> Result<CoinbaseIntxOrderList, CoinbaseIntxHttpError> {
        let query = serde_urlencoded::to_string(&params)
            .map_err(|e| CoinbaseIntxHttpError::JsonError(e.to_string()))?;
        let path = format!("/api/v1/orders?{query}");
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Cancels a single open order.
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn http_cancel_order(
        &self,
        client_order_id: &str,
        portfolio_id: &str,
    ) -> Result<CoinbaseIntxOrder, CoinbaseIntxHttpError> {
        let params = CancelOrderParams {
            portfolio: portfolio_id.to_string(),
        };
        let query = serde_urlencoded::to_string(&params)
            .map_err(|e| CoinbaseIntxHttpError::JsonError(e.to_string()))?;
        let path = format!("/api/v1/orders/{client_order_id}?{query}");
        self.send_request(Method::DELETE, &path, None, true).await
    }

    /// Cancel user orders.
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn http_cancel_orders(
        &self,
        params: CancelOrdersParams,
    ) -> Result<Vec<CoinbaseIntxOrder>, CoinbaseIntxHttpError> {
        let query = serde_urlencoded::to_string(&params)
            .map_err(|e| CoinbaseIntxHttpError::JsonError(e.to_string()))?;
        let path = format!("/api/v1/orders?{query}");
        self.send_request(Method::DELETE, &path, None, true).await
    }

    /// Modify an open order.
    ///
    /// See <https://docs.cdp.coinbase.com/intx/reference/modifyorder>.
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn http_modify_order(
        &self,
        order_id: &str,
        params: ModifyOrderParams,
    ) -> Result<CoinbaseIntxOrder, CoinbaseIntxHttpError> {
        let path = format!("/api/v1/orders/{order_id}");
        let body = serde_json::to_vec(&params)
            .map_err(|e| CoinbaseIntxHttpError::JsonError(e.to_string()))?;
        self.send_request(Method::PUT, &path, Some(body), true)
            .await
    }
}

/// Provides a higher-level HTTP client for the [Coinbase International](https://coinbase.com) REST API.
///
/// This client wraps the underlying `CoinbaseIntxHttpInnerClient` to handle conversions
/// into the Nautilus domain model.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct CoinbaseIntxHttpClient {
    pub(crate) inner: Arc<CoinbaseIntxHttpInnerClient>,
    pub(crate) instruments_cache: Arc<Mutex<HashMap<Ustr, InstrumentAny>>>,
    cache_initialized: bool,
}

impl Default for CoinbaseIntxHttpClient {
    fn default() -> Self {
        Self::new(None, Some(60))
    }
}

impl CoinbaseIntxHttpClient {
    /// Creates a new [`CoinbaseIntxHttpClient`] using the default Coinbase HTTP URL,
    /// optionally overridden with a custom base url.
    ///
    /// This version of the client has **no credentials**, so it can only
    /// call publicly accessible endpoints.
    #[must_use]
    pub fn new(base_url: Option<String>, timeout_secs: Option<u64>) -> Self {
        Self {
            inner: Arc::new(CoinbaseIntxHttpInnerClient::new(base_url, timeout_secs)),
            instruments_cache: Arc::new(Mutex::new(HashMap::new())),
            cache_initialized: false,
        }
    }

    /// Creates a new authenticated [`CoinbaseIntxHttpClient`] using environment variables and
    /// the default Coinbase International HTTP base url.
    ///
    /// # Errors
    ///
    /// Returns an error if required environment variables are missing or invalid.
    pub fn from_env() -> anyhow::Result<Self> {
        Self::with_credentials(None, None, None, None, None)
    }

    /// Creates a new [`CoinbaseIntxHttpClient`] configured with credentials
    /// for authenticated requests, optionally using a custom base url.
    ///
    /// # Errors
    ///
    /// Returns an error if required environment variables are missing or invalid.
    pub fn with_credentials(
        api_key: Option<String>,
        api_secret: Option<String>,
        api_passphrase: Option<String>,
        base_url: Option<String>,
        timeout_secs: Option<u64>,
    ) -> anyhow::Result<Self> {
        let api_key = get_or_env_var(api_key, "COINBASE_INTX_API_KEY")?;
        let api_secret = get_or_env_var(api_secret, "COINBASE_INTX_API_SECRET")?;
        let api_passphrase = get_or_env_var(api_passphrase, "COINBASE_INTX_API_PASSPHRASE")?;
        let base_url = base_url.unwrap_or(COINBASE_INTX_REST_URL.to_string());
        Ok(Self {
            inner: Arc::new(CoinbaseIntxHttpInnerClient::with_credentials(
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

    fn get_instrument_from_cache(&self, symbol: Ustr) -> anyhow::Result<InstrumentAny> {
        match self
            .instruments_cache
            .lock()
            .expect(MUTEX_POISONED)
            .get(&symbol)
        {
            Some(inst) => Ok(inst.clone()), // TODO: Remove this clone
            None => anyhow::bail!("Unable to process request, instrument {symbol} not in cache"),
        }
    }

    fn generate_ts_init(&self) -> UnixNanos {
        get_atomic_clock_realtime().get_time_ns()
    }

    /// Returns the base url being used by the client.
    #[must_use]
    pub fn base_url(&self) -> &str {
        self.inner.base_url.as_str()
    }

    /// Returns the public API key being used by the client.
    #[must_use]
    pub fn api_key(&self) -> Option<&str> {
        self.inner.credential.as_ref().map(|c| c.api_key.as_str())
    }

    /// Checks if the client is initialized.
    ///
    /// The client is considered initialized if any instruments have been cached from the venue.
    #[must_use]
    pub const fn is_initialized(&self) -> bool {
        self.cache_initialized
    }

    /// Returns the cached instrument symbols.
    ///
    /// # Panics
    ///
    /// Panics if the instrument cache mutex is poisoned.
    #[must_use]
    pub fn get_cached_symbols(&self) -> Vec<String> {
        self.instruments_cache
            .lock()
            .unwrap()
            .keys()
            .map(ToString::to_string)
            .collect()
    }

    /// Adds the given instruments into the clients instrument cache.
    ///
    /// # Panics
    ///
    /// Panics if the instrument cache mutex is poisoned.
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

    /// Adds the given instrument into the clients instrument cache.
    ///
    /// # Panics
    ///
    /// Panics if the instrument cache mutex is poisoned.
    ///
    /// Any existing instrument will be replaced.
    pub fn add_instrument(&mut self, instrument: InstrumentAny) {
        self.instruments_cache
            .lock()
            .unwrap()
            .insert(instrument.raw_symbol().inner(), instrument);
        self.cache_initialized = true;
    }

    /// Requests a list of portfolio details from Coinbase International.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn list_portfolios(&self) -> anyhow::Result<Vec<CoinbaseIntxPortfolio>> {
        let resp = self
            .inner
            .http_list_portfolios()
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        Ok(resp)
    }

    /// Requests the account state for the given account ID from Coinbase International.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn request_account_state(
        &self,
        account_id: AccountId,
    ) -> anyhow::Result<AccountState> {
        let resp = self
            .inner
            .http_list_portfolio_balances(account_id.get_issuers_id())
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let ts_init = self.generate_ts_init();
        let account_state = parse_account_state(resp, account_id, ts_init)?;

        Ok(account_state)
    }

    /// Requests all instruments from Coinbase International.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn request_instruments(&self) -> anyhow::Result<Vec<InstrumentAny>> {
        let resp = self
            .inner
            .http_list_instruments()
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let ts_init = self.generate_ts_init();

        let mut instruments: Vec<InstrumentAny> = Vec::new();
        for inst in &resp {
            let instrument_any = parse_instrument_any(inst, ts_init);
            if let Some(instrument_any) = instrument_any {
                instruments.push(instrument_any);
            }
        }

        Ok(instruments)
    }

    /// Requests the instrument for the given symbol from Coinbase International.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the instrument cannot be parsed.
    pub async fn request_instrument(&self, symbol: &Symbol) -> anyhow::Result<InstrumentAny> {
        let resp = self
            .inner
            .http_get_instrument_details(symbol.as_str())
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let ts_init = self.generate_ts_init();

        match parse_instrument_any(&resp, ts_init) {
            Some(inst) => Ok(inst),
            None => anyhow::bail!("Unable to parse instrument"),
        }
    }

    /// Requests an order status report for the given venue order ID from Coinbase International.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn request_order_status_report(
        &self,
        account_id: AccountId,
        venue_order_id: VenueOrderId,
    ) -> anyhow::Result<OrderStatusReport> {
        let portfolio_id = account_id.get_issuers_id();

        let resp = self
            .inner
            .http_get_order(venue_order_id.as_str(), portfolio_id)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let instrument = self.get_instrument_from_cache(resp.symbol)?;
        let ts_init = self.generate_ts_init();

        let report = parse_order_status_report(
            resp,
            account_id,
            instrument.price_precision(),
            instrument.size_precision(),
            ts_init,
        )?;
        Ok(report)
    }

    /// Requests order status reports for all **open** orders from Coinbase International.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn request_order_status_reports(
        &self,
        account_id: AccountId,
        symbol: Symbol,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let portfolio_id = account_id.get_issuers_id();

        let mut params = GetOrdersParamsBuilder::default();
        params.portfolio(portfolio_id);
        params.instrument(symbol.as_str());
        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let resp = self
            .inner
            .http_list_open_orders(params)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let ts_init = get_atomic_clock_realtime().get_time_ns();

        let mut reports: Vec<OrderStatusReport> = Vec::new();
        for order in resp.results {
            let instrument = self.get_instrument_from_cache(order.symbol)?;
            let report = parse_order_status_report(
                order,
                account_id,
                instrument.price_precision(),
                instrument.size_precision(),
                ts_init,
            )?;
            reports.push(report);
        }

        Ok(reports)
    }

    /// Requests all fill reports from Coinbase International.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn request_fill_reports(
        &self,
        account_id: AccountId,
        client_order_id: Option<ClientOrderId>,
        start: Option<DateTime<Utc>>,
    ) -> anyhow::Result<Vec<FillReport>> {
        let portfolio_id = account_id.get_issuers_id();

        let mut params = GetPortfolioFillsParamsBuilder::default();
        if let Some(start) = start {
            params.time_from(start);
        }
        if let Some(client_order_id) = client_order_id {
            params.client_order_id(client_order_id.to_string());
        }
        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let resp = self
            .inner
            .http_list_portfolio_fills(portfolio_id, params)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let ts_init = get_atomic_clock_realtime().get_time_ns();

        let mut reports: Vec<FillReport> = Vec::new();
        for fill in resp.results {
            let instrument = self.get_instrument_from_cache(fill.symbol)?;
            let report = parse_fill_report(
                fill,
                account_id,
                instrument.price_precision(),
                instrument.size_precision(),
                ts_init,
            )?;
            reports.push(report);
        }

        Ok(reports)
    }

    /// Requests a position status report from Coinbase International.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn request_position_status_report(
        &self,
        account_id: AccountId,
        symbol: Symbol,
    ) -> anyhow::Result<PositionStatusReport> {
        let portfolio_id = account_id.get_issuers_id();

        let resp = self
            .inner
            .http_get_portfolio_position(portfolio_id, symbol.as_str())
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let instrument = self.get_instrument_from_cache(resp.symbol)?;
        let ts_init = get_atomic_clock_realtime().get_time_ns();

        let report =
            parse_position_status_report(resp, account_id, instrument.size_precision(), ts_init)?;
        Ok(report)
    }

    /// Requests all position status reports from Coinbase International.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn request_position_status_reports(
        &self,
        account_id: AccountId,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let portfolio_id = account_id.get_issuers_id();

        let resp = self
            .inner
            .http_list_portfolio_positions(portfolio_id)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let ts_init = get_atomic_clock_realtime().get_time_ns();

        let mut reports: Vec<PositionStatusReport> = Vec::new();
        for position in resp {
            let instrument = self.get_instrument_from_cache(position.symbol)?;
            let report = parse_position_status_report(
                position,
                account_id,
                instrument.size_precision(),
                ts_init,
            )?;
            reports.push(report);
        }

        Ok(reports)
    }

    /// Submits a new order to Coinbase International.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_order(
        &self,
        account_id: AccountId,
        client_order_id: ClientOrderId,
        symbol: Symbol,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: TimeInForce,
        expire_time: Option<DateTime<Utc>>,
        price: Option<Price>,
        trigger_price: Option<Price>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
    ) -> anyhow::Result<OrderStatusReport> {
        let coinbase_side: CoinbaseIntxSide = order_side.into();
        let coinbase_order_type: CoinbaseIntxOrderType = order_type.into();
        let coinbase_tif: CoinbaseIntxTimeInForce = time_in_force.into();

        let mut params = CreateOrderParamsBuilder::default();
        params.portfolio(account_id.get_issuers_id());
        params.client_order_id(client_order_id.as_str());
        params.instrument(symbol.as_str());
        params.side(coinbase_side);
        params.size(quantity.to_string());
        params.order_type(coinbase_order_type);
        params.tif(coinbase_tif);
        if let Some(expire_time) = expire_time {
            params.expire_time(expire_time);
        }
        if let Some(price) = price {
            params.price(price.to_string());
        }
        if let Some(trigger_price) = trigger_price {
            params.stop_price(trigger_price.to_string());
        }
        if let Some(post_only) = post_only {
            params.post_only(post_only);
        }
        if let Some(reduce_only) = reduce_only {
            params.close_only(reduce_only);
        }
        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let resp = self.inner.http_create_order(params).await?;
        tracing::debug!("Submitted order: {resp:?}");

        let instrument = self.get_instrument_from_cache(resp.symbol)?;
        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let report = parse_order_status_report(
            resp,
            account_id,
            instrument.price_precision(),
            instrument.size_precision(),
            ts_init,
        )?;
        Ok(report)
    }

    /// Cancels a currently open order on Coinbase International.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn cancel_order(
        &self,
        account_id: AccountId,
        client_order_id: ClientOrderId,
    ) -> anyhow::Result<OrderStatusReport> {
        let portfolio_id = account_id.get_issuers_id();

        let resp = self
            .inner
            .http_cancel_order(client_order_id.as_str(), portfolio_id)
            .await?;
        tracing::debug!("Canceled order: {resp:?}");

        let instrument = self.get_instrument_from_cache(resp.symbol)?;
        let ts_init = get_atomic_clock_realtime().get_time_ns();

        let report = parse_order_status_report(
            resp,
            account_id,
            instrument.price_precision(),
            instrument.size_precision(),
            ts_init,
        )?;
        Ok(report)
    }

    /// Cancels all orders for the given account ID and filter params on Coinbase International.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn cancel_orders(
        &self,
        account_id: AccountId,
        symbol: Symbol,
        order_side: Option<OrderSide>,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let mut params = CancelOrdersParamsBuilder::default();
        params.portfolio(account_id.get_issuers_id());
        params.instrument(symbol.as_str());
        if let Some(side) = order_side {
            let side: CoinbaseIntxSide = side.into();
            params.side(side);
        }
        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let resp = self.inner.http_cancel_orders(params).await?;

        let instrument = self.get_instrument_from_cache(symbol.inner())?;
        let ts_init = get_atomic_clock_realtime().get_time_ns();

        let mut reports: Vec<OrderStatusReport> = Vec::with_capacity(resp.len());
        for order in resp {
            tracing::debug!("Canceled order: {order:?}");
            let report = parse_order_status_report(
                order,
                account_id,
                instrument.price_precision(),
                instrument.size_precision(),
                ts_init,
            )?;
            reports.push(report);
        }

        Ok(reports)
    }

    /// Modifies a currently open order on Coinbase International.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    #[allow(clippy::too_many_arguments)]
    pub async fn modify_order(
        &self,
        account_id: AccountId,
        client_order_id: ClientOrderId,
        new_client_order_id: ClientOrderId,
        price: Option<Price>,
        trigger_price: Option<Price>,
        quantity: Option<Quantity>,
    ) -> anyhow::Result<OrderStatusReport> {
        let mut params = ModifyOrderParamsBuilder::default();
        params.portfolio(account_id.get_issuers_id());
        params.client_order_id(new_client_order_id.as_str());
        if let Some(price) = price {
            params.price(price.to_string());
        }
        if let Some(trigger_price) = trigger_price {
            params.price(trigger_price.to_string());
        }
        if let Some(quantity) = quantity {
            params.size(quantity.to_string());
        }
        let params = params.build().map_err(|e| anyhow::anyhow!(e))?;

        let resp = self
            .inner
            .http_modify_order(client_order_id.as_str(), params)
            .await?;
        tracing::debug!("Modified order {}", resp.client_order_id);

        let instrument = self.get_instrument_from_cache(resp.symbol)?;
        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let report = parse_order_status_report(
            resp,
            account_id,
            instrument.price_precision(),
            instrument.size_precision(),
            ts_init,
        )?;
        Ok(report)
    }
}
