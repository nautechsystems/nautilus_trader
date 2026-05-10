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

//! HTTP client for the Bullet REST API (`/fapi/v1/...`, `/fapi/v3/...`).

use std::collections::HashMap;

use nautilus_core::consts::NAUTILUS_USER_AGENT;
use nautilus_network::http::{HttpClient, Method, USER_AGENT};

use crate::common::{
    error::BulletError,
    models::{
        Account, Balance, ExchangeInfo, FundingRate, OpenOrder, OrderBook, SubmitTxRequest,
        SubmitTxResponse,
    },
};
use crate::http::error::parse_api_error;

/// HTTP client for the Bullet REST API.
///
/// Wraps `nautilus_network::HttpClient` and provides typed methods for each endpoint.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.bullet",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.bullet")
)]
pub struct BulletHttpClient {
    base_url: String,
    inner: HttpClient,
}

impl BulletHttpClient {
    /// Create a new [`BulletHttpClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be constructed.
    pub fn new(
        base_url: impl Into<String>,
        timeout_secs: u64,
        proxy_url: Option<String>,
    ) -> Result<Self, BulletError> {
        let base_url = base_url.into();
        let mut headers = HashMap::new();
        headers.insert(USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string());
        let inner = HttpClient::new(headers, vec![], vec![], None, Some(timeout_secs), proxy_url)
            .map_err(|e| BulletError::Http(e.to_string()))?;
        Ok(Self { base_url, inner })
    }

    /// Return the base URL.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    async fn get_text(
        &self,
        url: &str,
        params: Option<HashMap<String, Vec<String>>>,
    ) -> Result<String, BulletError> {
        let response = self
            .inner
            .request(Method::GET, url.to_string(), params.as_ref(), None, None, None, None)
            .await
            .map_err(|e| BulletError::Http(e.to_string()))?;

        let status = response.status.as_u16();
        let body = String::from_utf8_lossy(&response.body).to_string();
        if !(200..300).contains(&status) {
            return Err(parse_api_error(&body, status));
        }
        Ok(body)
    }

    async fn post_json(&self, url: &str, body: &str) -> Result<String, BulletError> {
        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_string(), "application/json".to_string());

        let response = self
            .inner
            .request(
                Method::POST,
                url.to_string(),
                None,
                Some(headers),
                Some(body.as_bytes().to_vec()),
                None,
                None,
            )
            .await
            .map_err(|e| BulletError::Http(e.to_string()))?;

        let status = response.status.as_u16();
        let resp_body = String::from_utf8_lossy(&response.body).to_string();
        if !(200..300).contains(&status) {
            return Err(parse_api_error(&resp_body, status));
        }
        Ok(resp_body)
    }

    // ── Public endpoints ──────────────────────────────────────────────────────

    /// Fetch exchange info (chain parameters, symbol filters, rate limits, etc.).
    ///
    /// `GET /fapi/v1/exchangeInfo`
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn exchange_info(&self) -> Result<ExchangeInfo, BulletError> {
        let url = self.url("/fapi/v1/exchangeInfo");
        let body = self.get_text(&url, None).await?;
        serde_json::from_str(&body).map_err(|e| BulletError::Parse(e.to_string()))
    }

    /// Fetch raw exchange info JSON string.
    ///
    /// `GET /fapi/v1/exchangeInfo`
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails.
    pub async fn exchange_info_raw(&self) -> Result<String, BulletError> {
        let url = self.url("/fapi/v1/exchangeInfo");
        self.get_text(&url, None).await
    }

    /// Fetch L2 order book snapshot.
    ///
    /// `GET /fapi/v1/depth?symbol=<sym>&limit=<n>`
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn depth(&self, symbol: &str, limit: Option<u32>) -> Result<OrderBook, BulletError> {
        let url = self.url("/fapi/v1/depth");
        let mut params: HashMap<String, Vec<String>> = HashMap::new();
        params.insert("symbol".to_string(), vec![symbol.to_string()]);
        if let Some(n) = limit {
            params.insert("limit".to_string(), vec![n.to_string()]);
        }
        let body = self.get_text(&url, Some(params)).await?;
        serde_json::from_str(&body).map_err(|e| BulletError::Parse(e.to_string()))
    }

    /// Fetch latest funding rate for a symbol.
    ///
    /// `GET /fapi/v1/fundingRate?symbol=<sym>`
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    /// `GET /fapi/v1/fundingRate?symbol=<sym>` returns an array; we return the most recent entry.
    pub async fn funding_rate(&self, symbol: &str) -> Result<FundingRate, BulletError> {
        let url = self.url("/fapi/v1/fundingRate");
        let mut params: HashMap<String, Vec<String>> = HashMap::new();
        params.insert("symbol".to_string(), vec![symbol.to_string()]);
        let body = self.get_text(&url, Some(params)).await?;
        let mut rates: Vec<FundingRate> =
            serde_json::from_str(&body).map_err(|e| BulletError::Parse(e.to_string()))?;
        rates.pop().ok_or_else(|| BulletError::Parse("empty fundingRate response".to_string()))
    }

    // ── Private (address-authenticated) endpoints ─────────────────────────────

    /// Fetch account state (positions, margins, balances) for an address.
    ///
    /// `GET /fapi/v3/account?address=<addr>`
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn account(&self, address: &str) -> Result<Account, BulletError> {
        let url = self.url("/fapi/v3/account");
        let mut params: HashMap<String, Vec<String>> = HashMap::new();
        params.insert("address".to_string(), vec![address.to_string()]);
        let body = self.get_text(&url, Some(params)).await?;
        serde_json::from_str(&body).map_err(|e| BulletError::Parse(e.to_string()))
    }

    /// Fetch per-asset balances for an address.
    ///
    /// `GET /fapi/v3/balance?address=<addr>`
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn balances(&self, address: &str) -> Result<Vec<Balance>, BulletError> {
        let url = self.url("/fapi/v3/balance");
        let mut params: HashMap<String, Vec<String>> = HashMap::new();
        params.insert("address".to_string(), vec![address.to_string()]);
        let body = self.get_text(&url, Some(params)).await?;
        serde_json::from_str(&body).map_err(|e| BulletError::Parse(e.to_string()))
    }

    /// Fetch open orders for an address on a symbol.
    ///
    /// `GET /fapi/v1/openOrders?address=<addr>&symbol=<sym>`
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn open_orders(
        &self,
        address: &str,
        symbol: &str,
    ) -> Result<Vec<OpenOrder>, BulletError> {
        let url = self.url("/fapi/v1/openOrders");
        let mut params: HashMap<String, Vec<String>> = HashMap::new();
        params.insert("address".to_string(), vec![address.to_string()]);
        params.insert("symbol".to_string(), vec![symbol.to_string()]);
        let body = self.get_text(&url, Some(params)).await?;
        serde_json::from_str(&body).map_err(|e| BulletError::Parse(e.to_string()))
    }

    // ── Transaction submission ─────────────────────────────────────────────────

    /// Submit a signed transaction.
    ///
    /// `POST /tx/submit`
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or the exchange rejects the transaction.
    pub async fn submit_tx(&self, tx_base64: String) -> Result<SubmitTxResponse, BulletError> {
        let url = self.url("/tx/submit");
        let req = SubmitTxRequest { body: tx_base64 };
        let payload =
            serde_json::to_string(&req).map_err(|e| BulletError::Parse(e.to_string()))?;
        let body = self.post_json(&url, &payload).await?;
        serde_json::from_str(&body).map_err(|e| BulletError::Parse(e.to_string()))
    }
}
