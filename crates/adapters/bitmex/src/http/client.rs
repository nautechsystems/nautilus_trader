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

//! Provides the HTTP client integration for the [BitMEX](https://bitmex.com) REST API.
//!
//! This module defines and implements a [`BitmexHttpClient`] for
//! sending requests to various BitMEX endpoints. It handles request signing
//! (when credentials are provided), constructs valid HTTP requests
//! using the [`HttpClient`], and parses the responses back into structured data or a [`BitmexHttpError`].
//!
//! # Quick links to official docs
//! | Domain                               | BitMEX reference                                                          |
//! |--------------------------------------|---------------------------------------------------------------------------|
//! | Market data                          | <https://www.bitmex.com/api/explorer/#/default>                          |
//! | Account & positions                  | <https://www.bitmex.com/api/explorer/#/default>                          |
//! | Order management                     | <https://www.bitmex.com/api/explorer/#/default>                          |

use std::{
    collections::HashMap,
    num::NonZeroU32,
    sync::{Arc, LazyLock, Mutex},
};

use ahash::AHashMap;
use chrono::Utc;
use nautilus_core::{consts::NAUTILUS_USER_AGENT, env::get_env_var};
use nautilus_model::{
    identifiers::Symbol,
    instruments::{Instrument as InstrumentTrait, InstrumentAny},
};
use nautilus_network::{http::HttpClient, ratelimiter::quota::Quota};
use reqwest::{Method, StatusCode};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use ustr::Ustr;

use super::{
    error::{BitmexErrorResponse, BitmexHttpError},
    models::{
        BitmexExecution, BitmexInstrument, BitmexMargin, BitmexOrder, BitmexPosition, BitmexTrade,
        BitmexWallet,
    },
    query::{
        DeleteOrderParams, GetExecutionParams, GetOrderParams, GetPositionParams, GetTradeParams,
        PostOrderParams, PutOrderParams,
    },
};
use crate::{
    common::{
        consts::{BITMEX_HTTP_TESTNET_URL, BITMEX_HTTP_URL},
        credential::Credential,
    },
    websocket::messages::BitmexMarginMsg,
};

/// Default BitMEX REST API rate limit.
///
/// BitMEX rate limits are complex and vary by endpoint:
/// - Public endpoints: 150 requests per 5 minutes
/// - Private endpoints: 300 requests per 5 minutes
/// - Order placement: 200 requests per minute
///
/// We use a conservative 10 requests per second as a general limit.
pub static BITMEX_REST_QUOTA: LazyLock<Quota> =
    LazyLock::new(|| Quota::per_second(NonZeroU32::new(10).unwrap()));

/// Represents a BitMEX HTTP response.
#[derive(Debug, Serialize, Deserialize)]
pub struct BitmexResponse<T> {
    /// The typed data returned by the BitMEX endpoint.
    pub data: Vec<T>,
}

/// Provides a lower-level HTTP client for connecting to the [BitMEX](https://bitmex.com) REST API.
///
/// This client wraps the underlying [`HttpClient`] to handle functionality
/// specific to BitMEX, such as request signing (for authenticated endpoints),
/// forming request URLs, and deserializing responses into specific data models.
#[derive(Debug, Clone)]
pub struct BitmexHttpInnerClient {
    base_url: String,
    client: HttpClient,
    credential: Option<Credential>,
}

impl Default for BitmexHttpInnerClient {
    fn default() -> Self {
        Self::new(None, Some(60))
    }
}

impl BitmexHttpInnerClient {
    /// Creates a new [`BitmexHttpInnerClient`] using the default BitMEX HTTP URL,
    /// optionally overridden with a custom base URL.
    ///
    /// This version of the client has **no credentials**, so it can only
    /// call publicly accessible endpoints.
    #[must_use]
    pub fn new(base_url: Option<String>, timeout_secs: Option<u64>) -> Self {
        Self {
            base_url: base_url.unwrap_or(BITMEX_HTTP_URL.to_string()),
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                vec![],
                Some(*BITMEX_REST_QUOTA),
                timeout_secs,
            ),
            credential: None,
        }
    }

    /// Creates a new [`BitmexHttpInnerClient`] configured with credentials
    /// for authenticated requests, optionally using a custom base URL.
    #[must_use]
    pub fn with_credentials(
        api_key: String,
        api_secret: String,
        base_url: String,
        timeout_secs: Option<u64>,
    ) -> Self {
        Self {
            base_url,
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                vec![],
                Some(*BITMEX_REST_QUOTA),
                timeout_secs,
            ),
            credential: Some(Credential::new(api_key, api_secret)),
        }
    }

    fn default_headers() -> HashMap<String, String> {
        HashMap::from([("user-agent".to_string(), NAUTILUS_USER_AGENT.to_string())])
    }

    fn sign_request(
        &self,
        method: &Method,
        endpoint: &str,
        body: Option<&[u8]>,
    ) -> Result<HashMap<String, String>, BitmexHttpError> {
        let credential = self
            .credential
            .as_ref()
            .ok_or(BitmexHttpError::MissingCredentials)?;

        let expires = Utc::now().timestamp() + 10;
        let body_str = body
            .and_then(|b| String::from_utf8(b.to_vec()).ok())
            .unwrap_or_default();

        let full_path = if endpoint.starts_with("/api/v1") {
            endpoint.to_string()
        } else {
            format!("/api/v1{endpoint}")
        };

        tracing::debug!("Signing with body: '{}'", body_str);
        tracing::debug!("Method: {}", method.as_str());
        tracing::debug!("Path: {}", full_path);
        tracing::debug!("Expires: {}", expires);

        let signature = credential.sign(method.as_str(), &full_path, expires, &body_str);

        let mut headers = HashMap::new();
        headers.insert("api-expires".to_string(), expires.to_string());
        headers.insert("api-key".to_string(), credential.api_key.to_string());
        headers.insert("api-signature".to_string(), signature);

        Ok(headers)
    }

    async fn send_request<T: DeserializeOwned>(
        &self,
        method: Method,
        endpoint: &str,
        body: Option<Vec<u8>>,
        authenticate: bool,
    ) -> Result<T, BitmexHttpError> {
        let url = format!("{}{}", self.base_url, endpoint);

        let headers = if authenticate {
            Some(self.sign_request(&method, endpoint, body.as_deref())?)
        } else {
            None
        };

        let resp = self
            .client
            .request(method, url, headers, None, None, None)
            .await?;

        if resp.status.is_success() {
            serde_json::from_slice(&resp.body).map_err(Into::into)
        } else {
            // Try to parse as BitMEX error response
            if let Ok(error_resp) = serde_json::from_slice::<BitmexErrorResponse>(&resp.body) {
                Err(error_resp.into())
            } else {
                Err(BitmexHttpError::UnexpectedStatus {
                    status: StatusCode::from_u16(resp.status.as_u16()).unwrap(),
                    body: String::from_utf8_lossy(&resp.body).to_string(),
                })
            }
        }
    }

    // ========================================================================
    // Raw HTTP API methods
    // ========================================================================

    /// Get all instruments.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails, the response cannot be parsed, or the API returns an error.
    pub async fn http_get_instruments(
        &self,
        active_only: bool,
    ) -> Result<Vec<BitmexInstrument>, BitmexHttpError> {
        let path = if active_only {
            "/instrument/active"
        } else {
            "/instrument"
        };
        self.send_request(Method::GET, path, None, false).await
    }

    /// Get instrument by symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails, the response cannot be parsed, or the API returns an error.
    pub async fn http_get_instrument(
        &self,
        symbol: &str,
    ) -> Result<Vec<BitmexInstrument>, BitmexHttpError> {
        let path = &format!("/instrument?symbol={symbol}");
        self.send_request(Method::GET, path, None, false).await
    }

    /// Get user wallet information.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or the API returns an error.
    pub async fn http_get_wallet(&self) -> Result<BitmexWallet, BitmexHttpError> {
        let endpoint = "/user/wallet";
        self.send_request(Method::GET, endpoint, None, true).await
    }

    /// Get user margin information.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or the API returns an error.
    pub async fn http_get_margin(&self, currency: &str) -> Result<BitmexMargin, BitmexHttpError> {
        let path = format!("/user/margin?currency={currency}");
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Get historical trades.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the parameters cannot be serialized (should never happen with valid builder-generated params).
    pub async fn http_get_trades(
        &self,
        params: GetTradeParams,
    ) -> Result<Vec<BitmexTrade>, BitmexHttpError> {
        let query = serde_urlencoded::to_string(&params).expect("Invalid parameters");
        let path = format!("/trade?{query}");
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Get user orders.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the parameters cannot be serialized (should never happen with valid builder-generated params).
    pub async fn http_get_orders(
        &self,
        params: GetOrderParams,
    ) -> Result<Vec<BitmexOrder>, BitmexHttpError> {
        let query = serde_urlencoded::to_string(&params).expect("Invalid parameters");
        let path = format!("/order?{query}");
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Place a new order.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, order validation fails, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the parameters cannot be serialized (should never happen with valid builder-generated params).
    pub async fn http_place_order(
        &self,
        params: PostOrderParams,
    ) -> Result<Value, BitmexHttpError> {
        let query = serde_urlencoded::to_string(&params).expect("Invalid parameters");
        let path = format!("/order?{query}");
        self.send_request(Method::POST, &path, None, true).await
    }

    /// Cancel user orders.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, the order doesn't exist, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the parameters cannot be serialized (should never happen with valid builder-generated params).
    pub async fn http_cancel_orders(
        &self,
        params: DeleteOrderParams,
    ) -> Result<Value, BitmexHttpError> {
        let query = serde_urlencoded::to_string(&params).expect("Invalid parameters");
        let path = format!("/order?{query}");
        self.send_request(Method::DELETE, &path, None, true).await
    }

    /// Amend an existing order.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, the order doesn't exist, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the parameters cannot be serialized (should never happen with valid builder-generated params).
    pub async fn http_amend_order(&self, params: PutOrderParams) -> Result<Value, BitmexHttpError> {
        let query = serde_urlencoded::to_string(&params).expect("Invalid parameters");
        let path = format!("/order?{query}");
        self.send_request(Method::PUT, &path, None, true).await
    }

    /// Get user executions.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the parameters cannot be serialized (should never happen with valid builder-generated params).
    pub async fn http_get_executions(
        &self,
        params: GetExecutionParams,
    ) -> Result<Vec<BitmexExecution>, BitmexHttpError> {
        let query = serde_urlencoded::to_string(&params).expect("Invalid parameters");
        let path = format!("/execution/tradeHistory?{query}");
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Get user positions.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the parameters cannot be serialized (should never happen with valid builder-generated params).
    pub async fn http_get_positions(
        &self,
        params: GetPositionParams,
    ) -> Result<Vec<BitmexPosition>, BitmexHttpError> {
        let query = serde_urlencoded::to_string(&params).expect("Invalid parameters");
        let path = format!("/position?{query}");
        self.send_request(Method::GET, &path, None, true).await
    }
}

/// Provides a HTTP client for connecting to the [BitMEX](https://bitmex.com) REST API.
///
/// This is the high-level client that wraps the inner client and provides
/// Nautilus-specific functionality for trading operations.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct BitmexHttpClient {
    inner: Arc<BitmexHttpInnerClient>,
    instruments_cache: Arc<Mutex<AHashMap<Ustr, InstrumentAny>>>,
}

impl Default for BitmexHttpClient {
    fn default() -> Self {
        Self::new(None, None, None, false, Some(60))
    }
}

impl BitmexHttpClient {
    /// Creates a new [`BitmexHttpClient`] instance.
    #[must_use]
    pub fn new(
        base_url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        testnet: bool,
        timeout_secs: Option<u64>,
    ) -> Self {
        // Determine the base URL
        let url = base_url.unwrap_or_else(|| {
            if testnet {
                BITMEX_HTTP_TESTNET_URL.to_string()
            } else {
                BITMEX_HTTP_URL.to_string()
            }
        });

        let inner = match (api_key, api_secret) {
            (Some(key), Some(secret)) => {
                BitmexHttpInnerClient::with_credentials(key, secret, url, timeout_secs)
            }
            _ => BitmexHttpInnerClient::new(Some(url), timeout_secs),
        };

        Self {
            inner: Arc::new(inner),
            instruments_cache: Arc::new(Mutex::new(AHashMap::new())),
        }
    }

    /// Creates a new [`BitmexHttpClient`] instance using environment variables and
    /// the default BitMEX HTTP base URL.
    ///
    /// # Errors
    ///
    /// Returns an error if required environment variables are not set or invalid.
    pub fn from_env() -> anyhow::Result<Self> {
        Self::with_credentials(None, None, None, None)
    }

    /// Creates a new [`BitmexHttpClient`] configured with credentials
    /// for authenticated requests.
    ///
    /// If `api_key` or `api_secret` are `None`, they will be sourced from the
    /// `BITMEX_API_KEY` and `BITMEX_API_SECRET` environment variables.
    ///
    /// # Errors
    ///
    /// Returns an error if one credential is provided without the other.
    pub fn with_credentials(
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url: Option<String>,
        timeout_secs: Option<u64>,
    ) -> anyhow::Result<Self> {
        let api_key = api_key.or_else(|| get_env_var("BITMEX_API_KEY").ok());
        let api_secret = api_secret.or_else(|| get_env_var("BITMEX_API_SECRET").ok());

        // Determine testnet from URL if provided
        let testnet = base_url.as_ref().is_some_and(|url| url.contains("testnet"));

        // If we're trying to create an authenticated client, we need both key and secret
        if api_key.is_some() && api_secret.is_none() {
            anyhow::bail!("BITMEX_API_SECRET is required when BITMEX_API_KEY is provided");
        }
        if api_key.is_none() && api_secret.is_some() {
            anyhow::bail!("BITMEX_API_KEY is required when BITMEX_API_SECRET is provided");
        }

        Ok(Self::new(
            base_url,
            api_key,
            api_secret,
            testnet,
            timeout_secs,
        ))
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

    /// Get all instruments.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails, the response cannot be parsed, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the inner mutex is poisoned.
    pub async fn get_instruments(
        &self,
        active_only: bool,
    ) -> Result<Vec<BitmexInstrument>, BitmexHttpError> {
        let inner = self.inner.clone();
        inner.http_get_instruments(active_only).await
    }

    /// Get instrument by symbol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails, the response cannot be parsed, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the inner mutex is poisoned.
    pub async fn get_instrument(
        &self,
        symbol: &Symbol,
    ) -> Result<Vec<BitmexInstrument>, BitmexHttpError> {
        let inner = self.inner.clone();
        inner.http_get_instrument(symbol.as_ref()).await
    }

    /// Get user wallet information.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the inner mutex is poisoned.
    pub async fn get_wallet(&self) -> Result<BitmexWallet, BitmexHttpError> {
        let inner = self.inner.clone();
        inner.http_get_wallet().await
    }

    /// Get historical trades.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the inner mutex is poisoned.
    pub async fn get_trades(
        &self,
        params: GetTradeParams,
    ) -> Result<Vec<BitmexTrade>, BitmexHttpError> {
        let inner = self.inner.clone();
        inner.http_get_trades(params).await
    }

    /// Get user orders.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the inner mutex is poisoned.
    pub async fn get_orders(
        &self,
        params: GetOrderParams,
    ) -> Result<Vec<BitmexOrder>, BitmexHttpError> {
        let inner = self.inner.clone();
        inner.http_get_orders(params).await
    }

    /// Place a new order.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, order validation fails, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the inner mutex is poisoned.
    pub async fn place_order(&self, params: PostOrderParams) -> Result<Value, BitmexHttpError> {
        let inner = self.inner.clone();
        inner.http_place_order(params).await
    }

    /// Cancel user orders.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, the order doesn't exist, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the inner mutex is poisoned.
    pub async fn cancel_orders(&self, params: DeleteOrderParams) -> Result<Value, BitmexHttpError> {
        let inner = self.inner.clone();
        inner.http_cancel_orders(params).await
    }

    /// Amend an existing order.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, the order doesn't exist, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the inner mutex is poisoned.
    pub async fn amend_order(&self, params: PutOrderParams) -> Result<Value, BitmexHttpError> {
        let inner = self.inner.clone();
        inner.http_amend_order(params).await
    }

    /// Get user executions.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the inner mutex is poisoned.
    pub async fn get_executions(
        &self,
        params: GetExecutionParams,
    ) -> Result<Vec<BitmexExecution>, BitmexHttpError> {
        let inner = self.inner.clone();
        inner.http_get_executions(params).await
    }

    /// Get user positions.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or the API returns an error.
    ///
    /// # Panics
    ///
    /// Panics if the inner mutex is poisoned.
    pub async fn get_positions(
        &self,
        params: GetPositionParams,
    ) -> Result<Vec<BitmexPosition>, BitmexHttpError> {
        let inner = self.inner.clone();
        inner.http_get_positions(params).await
    }

    /// Add an instrument to the cache for precision lookups.
    ///
    /// # Panics
    ///
    /// Panics if the instruments cache mutex is poisoned.
    pub fn add_instrument(&mut self, instrument: InstrumentAny) {
        self.instruments_cache
            .lock()
            .unwrap()
            .insert(instrument.raw_symbol().inner(), instrument);
    }

    /// Get user margin information.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the request fails, or the API returns an error.
    pub async fn http_get_margin(&self, currency: &str) -> anyhow::Result<BitmexMargin> {
        self.inner
            .http_get_margin(currency)
            .await
            .map_err(|e| anyhow::anyhow!(e))
    }

    /// Request account state for the given account.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or no account state is returned.
    pub async fn request_account_state(
        &self,
        account_id: nautilus_model::identifiers::AccountId,
    ) -> anyhow::Result<nautilus_model::events::AccountState> {
        // Get margin data for XBt (Bitcoin) by default
        let margin = self
            .inner
            .http_get_margin("XBt")
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let ts_init = nautilus_core::nanos::UnixNanos::from(
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default() as u64,
        );

        // Convert HTTP Margin to WebSocket MarginMsg for parsing
        let margin_msg = BitmexMarginMsg {
            account: margin.account,
            currency: margin.currency,
            risk_limit: margin.risk_limit,
            amount: margin.amount,
            prev_realised_pnl: margin.prev_realised_pnl,
            gross_comm: margin.gross_comm,
            gross_open_cost: margin.gross_open_cost,
            gross_open_premium: margin.gross_open_premium,
            gross_exec_cost: margin.gross_exec_cost,
            gross_mark_value: margin.gross_mark_value,
            risk_value: margin.risk_value,
            init_margin: margin.init_margin,
            maint_margin: margin.maint_margin,
            target_excess_margin: margin.target_excess_margin,
            realised_pnl: margin.realised_pnl,
            unrealised_pnl: margin.unrealised_pnl,
            wallet_balance: margin.wallet_balance,
            margin_balance: margin.margin_balance,
            margin_leverage: margin.margin_leverage,
            margin_used_pcnt: margin.margin_used_pcnt,
            excess_margin: margin.excess_margin,
            available_margin: margin.available_margin,
            withdrawable_margin: margin.withdrawable_margin,
            maker_fee_discount: None, // Not in HTTP response
            taker_fee_discount: None, // Not in HTTP response
            timestamp: margin.timestamp.unwrap_or_else(chrono::Utc::now),
            foreign_margin_balance: None,
            foreign_requirement: None,
        };

        crate::common::parse::parse_account_state(&margin_msg, account_id, ts_init)
    }
}
