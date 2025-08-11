// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use std::collections::HashMap;

use chrono::Utc;
use nautilus_core::consts::NAUTILUS_USER_AGENT;
use nautilus_model::identifiers::Symbol;
use nautilus_network::http::HttpClient;
use reqwest::{Method, StatusCode};
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::{
    error::{BitmexErrorResponse, BitmexHttpError},
    models::{Execution, Instrument, Order, Position, Trade, Wallet},
    query::{
        DeleteOrderParams, GetExecutionParams, GetOrderParams, GetPositionParams, GetTradeParams,
        PostOrderParams, PutOrderParams,
    },
};
use crate::{consts::BITMEX_HTTP_URL, credential::Credential};

/// Provides a HTTP client for connecting to the [BitMEX](https://bitmex.com) Rest API.
#[derive(Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct BitmexHttpClient {
    base_url: String,
    client: HttpClient,
    credential: Option<Credential>,
}

impl Default for BitmexHttpClient {
    fn default() -> Self {
        Self::new(None)
    }
}

impl BitmexHttpClient {
    pub fn new(base_url: Option<&str>) -> Self {
        Self {
            base_url: base_url.unwrap_or(BITMEX_HTTP_URL).to_string(),
            client: HttpClient::new(Self::default_headers(), vec![], vec![], None, None), // TODO: Rate limits TBD
            credential: None,
        }
    }

    pub fn with_credentials(api_key: &str, api_secret: &str, base_url: Option<&str>) -> Self {
        Self {
            base_url: base_url.unwrap_or(BITMEX_HTTP_URL).to_string(),
            client: HttpClient::new(Self::default_headers(), vec![], vec![], None, None), // TODO: Rate limits TBD
            credential: Some(Credential::new(api_key.to_string(), api_secret.to_string())),
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
            format!("/api/v1{}", endpoint)
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

        tracing::trace!("{resp:?}"); // TODO: Remove after development

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

    /// Get all instruments.
    pub async fn get_instruments(
        &self,
        active_only: bool,
    ) -> Result<Vec<Instrument>, BitmexHttpError> {
        let endpoint = if active_only {
            "/instrument/active"
        } else {
            "/instrument"
        };
        self.send_request(Method::GET, endpoint, None, false).await
    }

    /// Get instrument by symbol.
    pub async fn get_instrument(
        &self,
        symbol: &Symbol,
    ) -> Result<Vec<Instrument>, BitmexHttpError> {
        let endpoint = &format!("/instrument?symbol={}", symbol);
        self.send_request(Method::GET, endpoint, None, false).await
    }

    /// Get user wallet information.
    pub async fn get_wallet(&self) -> Result<Wallet, BitmexHttpError> {
        let endpoint = "/user/wallet";
        self.send_request(Method::GET, endpoint, None, true).await
    }

    /// Get historical trades.
    pub async fn get_trades(&self, params: GetTradeParams) -> Result<Vec<Trade>, BitmexHttpError> {
        let query = serde_urlencoded::to_string(&params).expect("Invalid parameters");
        let path = format!("/trade?{}", query);
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Get user orders.
    pub async fn get_orders(&self, params: GetOrderParams) -> Result<Vec<Order>, BitmexHttpError> {
        let query = serde_urlencoded::to_string(&params).expect("Invalid parameters");
        let path = format!("/order?{}", query);
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Get user orders.
    pub async fn place_order(&self, params: PostOrderParams) -> Result<Value, BitmexHttpError> {
        let query = serde_urlencoded::to_string(&params).expect("Invalid parameters");
        let path = format!("/order?{}", query);
        self.send_request(Method::POST, &path, None, true).await
    }

    /// Cancel user orders.
    pub async fn cancel_orders(&self, params: DeleteOrderParams) -> Result<Value, BitmexHttpError> {
        let query = serde_urlencoded::to_string(&params).expect("Invalid parameters");
        let path = format!("/order?{}", query);
        self.send_request(Method::DELETE, &path, None, true).await
    }

    /// Cancel user orders.
    pub async fn amend_order(&self, params: PutOrderParams) -> Result<Value, BitmexHttpError> {
        let query = serde_urlencoded::to_string(&params).expect("Invalid parameters");
        let path = format!("/order?{}", query);
        self.send_request(Method::PUT, &path, None, true).await
    }

    /// Get user executions.
    pub async fn get_executions(
        &self,
        params: GetExecutionParams,
    ) -> Result<Vec<Execution>, BitmexHttpError> {
        let query = serde_urlencoded::to_string(&params).expect("Invalid parameters");
        let path = format!("/execution/tradeHistory?{}", query);
        self.send_request(Method::GET, &path, None, true).await
    }

    /// Get user positions.
    pub async fn get_positions(
        &self,
        params: GetPositionParams,
    ) -> Result<Vec<Position>, BitmexHttpError> {
        let query = serde_urlencoded::to_string(&params).expect("Invalid parameters");
        let path = format!("/position?{}", query);
        self.send_request(Method::GET, &path, None, true).await
    }
}
