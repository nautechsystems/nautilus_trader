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

//! Provides the HTTP client for the Polymarket CLOB REST API.

use std::{collections::HashMap, result::Result as StdResult, str::from_utf8};

use nautilus_core::{
    consts::NAUTILUS_USER_AGENT,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_network::http::{HttpClient, HttpClientError, Method, USER_AGENT};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::{
    common::{
        credential::Credential,
        enums::PolymarketOrderType,
        urls::{clob_http_url, gamma_api_url},
    },
    http::{
        error::{Error, Result},
        models::{
            GammaMarket, PolymarketOpenOrder, PolymarketOrder, PolymarketTradeReport,
            TickSizeResponse,
        },
        query::{
            BalanceAllowance, BatchCancelResponse, CancelMarketOrdersParams, CancelResponse,
            GetBalanceAllowanceParams, GetGammaMarketsParams, GetOrdersParams, GetTradesParams,
            OrderResponse, PaginatedResponse,
        },
        rate_limits::POLYMARKET_REST_QUOTA,
    },
};

const CURSOR_START: &str = "MA==";
const CURSOR_END: &str = "LTE=";

const PATH_ORDERS: &str = "/data/orders";
const PATH_TRADES: &str = "/data/trades";
const PATH_BALANCE_ALLOWANCE: &str = "/balance-allowance";
const PATH_POST_ORDER: &str = "/order";
const PATH_POST_ORDERS: &str = "/orders";
const PATH_CANCEL_ALL: &str = "/cancel-all";
const PATH_CANCEL_MARKET_ORDERS: &str = "/cancel-market-orders";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PostOrderBody<'a> {
    order: &'a PolymarketOrder,
    owner: &'a str,
    order_type: PolymarketOrderType,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    post_only: bool,
}

#[derive(Serialize)]
struct CancelOrderBody<'a> {
    #[serde(rename = "orderID")]
    order_id: &'a str,
}

/// Provides a raw HTTP client for Polymarket CLOB REST API operations.
///
/// Handles HTTP transport, L2 HMAC-SHA256 auth signing, pagination, and raw
/// API calls that closely match Polymarket endpoint specifications.
#[derive(Debug, Clone)]
pub struct PolymarketRawHttpClient {
    client: HttpClient,
    base_url: String,
    gamma_base_url: String,
    credential: Option<Credential>,
    address: Option<String>,
    clock: &'static AtomicTime,
}

impl PolymarketRawHttpClient {
    /// Creates a new unauthenticated [`PolymarketRawHttpClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn new(
        base_url: Option<String>,
        gamma_base_url: Option<String>,
        timeout_secs: Option<u64>,
    ) -> StdResult<Self, HttpClientError> {
        Ok(Self {
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                vec![],
                Some(*POLYMARKET_REST_QUOTA),
                timeout_secs,
                None,
            )?,
            base_url: base_url
                .unwrap_or_else(|| clob_http_url().to_string())
                .trim_end_matches('/')
                .to_string(),
            gamma_base_url: gamma_base_url
                .unwrap_or_else(|| gamma_api_url().to_string())
                .trim_end_matches('/')
                .to_string(),
            credential: None,
            address: None,
            clock: get_atomic_clock_realtime(),
        })
    }

    /// Creates a new authenticated [`PolymarketRawHttpClient`] with L2 credentials.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn with_credential(
        credential: Credential,
        address: String,
        base_url: Option<String>,
        gamma_base_url: Option<String>,
        timeout_secs: Option<u64>,
    ) -> StdResult<Self, HttpClientError> {
        Ok(Self {
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                vec![],
                Some(*POLYMARKET_REST_QUOTA),
                timeout_secs,
                None,
            )?,
            base_url: base_url
                .unwrap_or_else(|| clob_http_url().to_string())
                .trim_end_matches('/')
                .to_string(),
            gamma_base_url: gamma_base_url
                .unwrap_or_else(|| gamma_api_url().to_string())
                .trim_end_matches('/')
                .to_string(),
            credential: Some(credential),
            address: Some(address),
            clock: get_atomic_clock_realtime(),
        })
    }

    fn default_headers() -> HashMap<String, String> {
        HashMap::from([
            (USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string()),
            ("Content-Type".to_string(), "application/json".to_string()),
        ])
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    fn gamma_url(&self, path: &str) -> String {
        format!("{}{path}", self.gamma_base_url)
    }

    fn timestamp(&self) -> String {
        (self.clock.get_time_ns().as_u64() / 1_000_000_000).to_string()
    }

    fn auth_headers(
        &self,
        method: &str,
        path: &str,
        body: &str,
    ) -> Result<HashMap<String, String>> {
        let cred = self
            .credential
            .as_ref()
            .ok_or_else(|| Error::auth("No credential configured"))?;
        let address = self
            .address
            .as_ref()
            .ok_or_else(|| Error::auth("No address configured"))?;

        let timestamp = self.timestamp();
        let signature = cred.sign(&timestamp, method, path, body);

        Ok(HashMap::from([
            ("POLY_ADDRESS".to_string(), address.clone()),
            ("POLY_SIGNATURE".to_string(), signature),
            ("POLY_TIMESTAMP".to_string(), timestamp),
            ("POLY_API_KEY".to_string(), cred.api_key().to_string()),
            ("POLY_PASSPHRASE".to_string(), cred.passphrase().to_string()),
        ]))
    }

    async fn send_get<P: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        params: Option<&P>,
        auth: bool,
    ) -> Result<T> {
        let headers = if auth {
            Some(self.auth_headers("GET", path, "")?)
        } else {
            None
        };
        let url = self.url(path);
        let response = self
            .client
            .request_with_params(Method::GET, url, params, headers, None, None, None)
            .await
            .map_err(Error::from_http_client)?;

        if response.status.is_success() {
            serde_json::from_slice(&response.body).map_err(Error::Serde)
        } else {
            Err(Error::from_status_code(
                response.status.as_u16(),
                &response.body,
            ))
        }
    }

    async fn send_post<T: DeserializeOwned>(&self, path: &str, body_bytes: Vec<u8>) -> Result<T> {
        let body_str =
            from_utf8(&body_bytes).map_err(|e| Error::decode(format!("UTF-8 error: {e}")))?;
        let headers = Some(self.auth_headers("POST", path, body_str)?);
        let url = self.url(path);
        let response = self
            .client
            .request(
                Method::POST,
                url,
                None,
                headers,
                Some(body_bytes),
                None,
                None,
            )
            .await
            .map_err(Error::from_http_client)?;

        if response.status.is_success() {
            serde_json::from_slice(&response.body).map_err(Error::Serde)
        } else {
            Err(Error::from_status_code(
                response.status.as_u16(),
                &response.body,
            ))
        }
    }

    async fn send_delete<T: DeserializeOwned>(
        &self,
        path: &str,
        body_bytes: Option<Vec<u8>>,
    ) -> Result<T> {
        let body_str = body_bytes
            .as_deref()
            .map(|b| from_utf8(b).map_err(|e| Error::decode(format!("UTF-8 error: {e}"))))
            .transpose()?
            .unwrap_or("");
        let headers = Some(self.auth_headers("DELETE", path, body_str)?);
        let url = self.url(path);
        let response = self
            .client
            .request(Method::DELETE, url, None, headers, body_bytes, None, None)
            .await
            .map_err(Error::from_http_client)?;

        if response.status.is_success() {
            serde_json::from_slice(&response.body).map_err(Error::Serde)
        } else {
            Err(Error::from_status_code(
                response.status.as_u16(),
                &response.body,
            ))
        }
    }

    /// Fetches all open orders matching the given parameters (auto-paginated).
    pub async fn get_orders(
        &self,
        mut params: GetOrdersParams,
    ) -> Result<Vec<PolymarketOpenOrder>> {
        if params.next_cursor.is_none() {
            params.next_cursor = Some(CURSOR_START.to_string());
        }
        let mut all = Vec::new();
        loop {
            let page: PaginatedResponse<PolymarketOpenOrder> =
                self.send_get(PATH_ORDERS, Some(&params), true).await?;
            all.extend(page.data);
            if page.next_cursor == CURSOR_END {
                break;
            }
            params.next_cursor = Some(page.next_cursor);
        }
        Ok(all)
    }

    /// Fetches a single open order by ID.
    pub async fn get_order(&self, order_id: &str) -> Result<PolymarketOpenOrder> {
        let path = format!("/data/order/{order_id}");
        self.send_get::<(), _>(&path, None::<&()>, true).await
    }

    /// Fetches all trades matching the given parameters (auto-paginated).
    pub async fn get_trades(
        &self,
        mut params: GetTradesParams,
    ) -> Result<Vec<PolymarketTradeReport>> {
        if params.next_cursor.is_none() {
            params.next_cursor = Some(CURSOR_START.to_string());
        }
        let mut all = Vec::new();
        loop {
            let page: PaginatedResponse<PolymarketTradeReport> =
                self.send_get(PATH_TRADES, Some(&params), true).await?;
            all.extend(page.data);
            if page.next_cursor == CURSOR_END {
                break;
            }
            params.next_cursor = Some(page.next_cursor);
        }
        Ok(all)
    }

    /// Fetches balance and allowance for the given parameters.
    pub async fn get_balance_allowance(
        &self,
        params: GetBalanceAllowanceParams,
    ) -> Result<BalanceAllowance> {
        self.send_get(PATH_BALANCE_ALLOWANCE, Some(&params), true)
            .await
    }

    /// Submits a single signed order to the exchange.
    pub async fn post_order(
        &self,
        order: &PolymarketOrder,
        order_type: PolymarketOrderType,
        post_only: bool,
    ) -> Result<OrderResponse> {
        let owner = self
            .credential
            .as_ref()
            .ok_or_else(|| Error::auth("No credential configured"))?
            .api_key()
            .to_string();
        let body = PostOrderBody {
            order,
            owner: &owner,
            order_type,
            post_only,
        };
        let body_bytes = serde_json::to_vec(&body).map_err(Error::Serde)?;
        self.send_post(PATH_POST_ORDER, body_bytes).await
    }

    /// Submits a batch of signed orders to the exchange.
    ///
    /// Each entry is `(order, order_type, post_only)`.
    pub async fn post_orders(
        &self,
        orders: &[(&PolymarketOrder, PolymarketOrderType, bool)],
    ) -> Result<Vec<OrderResponse>> {
        let owner = self
            .credential
            .as_ref()
            .ok_or_else(|| Error::auth("No credential configured"))?
            .api_key()
            .to_string();
        let entries: Vec<PostOrderBody<'_>> = orders
            .iter()
            .map(|(order, order_type, post_only)| PostOrderBody {
                order,
                owner: &owner,
                order_type: *order_type,
                post_only: *post_only,
            })
            .collect();
        let body_bytes = serde_json::to_vec(&entries).map_err(Error::Serde)?;
        self.send_post(PATH_POST_ORDERS, body_bytes).await
    }

    /// Cancels a single order by ID.
    pub async fn cancel_order(&self, order_id: &str) -> Result<CancelResponse> {
        let body = CancelOrderBody { order_id };
        let body_bytes = serde_json::to_vec(&body).map_err(Error::Serde)?;
        self.send_delete("/order", Some(body_bytes)).await
    }

    /// Cancels multiple orders by ID.
    pub async fn cancel_orders(&self, order_ids: &[&str]) -> Result<BatchCancelResponse> {
        let body_bytes = serde_json::to_vec(order_ids).map_err(Error::Serde)?;
        self.send_delete("/orders", Some(body_bytes)).await
    }

    /// Cancels all open orders.
    pub async fn cancel_all(&self) -> Result<BatchCancelResponse> {
        self.send_delete(PATH_CANCEL_ALL, None).await
    }

    /// Cancels all orders for a specific market.
    pub async fn cancel_market_orders(
        &self,
        params: CancelMarketOrdersParams,
    ) -> Result<BatchCancelResponse> {
        let body_bytes = serde_json::to_vec(&params).map_err(Error::Serde)?;
        self.send_delete(PATH_CANCEL_MARKET_ORDERS, Some(body_bytes))
            .await
    }

    async fn send_gamma_get<P: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        params: Option<&P>,
    ) -> Result<T> {
        let url = self.gamma_url(path);
        let response = self
            .client
            .request_with_params(Method::GET, url, params, None, None, None, None)
            .await
            .map_err(Error::from_http_client)?;

        if response.status.is_success() {
            serde_json::from_slice(&response.body).map_err(Error::Serde)
        } else {
            Err(Error::from_status_code(
                response.status.as_u16(),
                &response.body,
            ))
        }
    }

    /// Fetches markets from the Gamma API.
    ///
    /// Handles both bare array and `{"data": [...]}` response schemas.
    pub async fn get_gamma_markets(
        &self,
        params: GetGammaMarketsParams,
    ) -> Result<Vec<GammaMarket>> {
        let value: Value = self.send_gamma_get("/markets", Some(&params)).await?;

        let array = match value {
            Value::Array(_) => value,
            Value::Object(ref map) if map.contains_key("data") => {
                map.get("data").cloned().unwrap_or(Value::Array(vec![]))
            }
            _ => {
                return Err(Error::decode(
                    "Unrecognized Gamma markets response schema".to_string(),
                ));
            }
        };

        serde_json::from_value(array).map_err(Error::Serde)
    }

    /// Fetches a single market by ID from the Gamma API.
    pub async fn get_gamma_market(&self, market_id: &str) -> Result<GammaMarket> {
        let path = format!("/markets/{market_id}");
        self.send_gamma_get::<(), _>(&path, None::<&()>).await
    }

    /// Fetches the tick size for a token from the CLOB API.
    pub async fn get_tick_size(&self, token_id: &str) -> Result<TickSizeResponse> {
        let params = [("token_id", token_id)];
        self.send_get("/tick-size", Some(&params), false).await
    }
}
