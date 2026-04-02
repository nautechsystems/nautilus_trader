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
use nautilus_model::{
    data::BookOrder,
    enums::{BookType, OrderSide},
    identifiers::InstrumentId,
    orderbook::OrderBook,
};
use nautilus_network::http::{HttpClient, HttpClientError, Method, USER_AGENT};
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    common::{credential::Credential, enums::PolymarketOrderType, urls::clob_http_url},
    http::{
        error::{Error, Result},
        models::{
            ClobBookResponse, FeeRateResponse, PolymarketOpenOrder, PolymarketOrder,
            PolymarketTradeReport, TickSizeResponse,
        },
        query::{
            BalanceAllowance, BatchCancelResponse, CancelMarketOrdersParams, CancelResponse,
            GetBalanceAllowanceParams, GetOrdersParams, GetTradesParams, OrderResponse,
            PaginatedResponse,
        },
        rate_limits::POLYMARKET_CLOB_REST_QUOTA,
    },
    websocket::parse::{parse_price, parse_quantity},
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

/// Provides an authenticated HTTP client for the Polymarket CLOB REST API.
///
/// Handles HTTP transport, L2 HMAC-SHA256 auth signing, pagination, and raw
/// API calls that closely match Polymarket endpoint specifications.
/// Credential is always present: the CLOB API requires authentication.
#[derive(Debug, Clone)]
pub struct PolymarketClobHttpClient {
    client: HttpClient,
    base_url: String,
    credential: Credential,
    address: String,
    clock: &'static AtomicTime,
}

impl PolymarketClobHttpClient {
    /// Creates a new authenticated [`PolymarketClobHttpClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn new(
        credential: Credential,
        address: String,
        base_url: Option<String>,
        timeout_secs: u64,
    ) -> StdResult<Self, HttpClientError> {
        Ok(Self {
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                vec![],
                Some(*POLYMARKET_CLOB_REST_QUOTA),
                Some(timeout_secs),
                None,
            )?,
            base_url: base_url
                .unwrap_or_else(|| clob_http_url().to_string())
                .trim_end_matches('/')
                .to_string(),
            credential,
            address,
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

    fn timestamp(&self) -> String {
        (self.clock.get_time_ns().as_u64() / 1_000_000_000).to_string()
    }

    fn auth_headers(&self, method: &str, path: &str, body: &str) -> HashMap<String, String> {
        let timestamp = self.timestamp();
        let signature = self.credential.sign(&timestamp, method, path, body);

        HashMap::from([
            ("POLY_ADDRESS".to_string(), self.address.clone()),
            ("POLY_SIGNATURE".to_string(), signature),
            ("POLY_TIMESTAMP".to_string(), timestamp),
            (
                "POLY_API_KEY".to_string(),
                self.credential.api_key().to_string(),
            ),
            (
                "POLY_PASSPHRASE".to_string(),
                self.credential.passphrase().to_string(),
            ),
        ])
    }

    async fn send_get<P: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        params: Option<&P>,
        auth: bool,
    ) -> Result<T> {
        let headers = if auth {
            Some(self.auth_headers("GET", path, ""))
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

    /// Like [`send_get`] but returns `Ok(None)` for empty or `null` response bodies
    /// instead of a serde deserialization error.
    async fn send_get_optional<P: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        params: Option<&P>,
        auth: bool,
    ) -> Result<Option<T>> {
        let headers = if auth {
            Some(self.auth_headers("GET", path, ""))
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
            if response.body.is_empty() || response.body.as_ref() == b"null" {
                Ok(None)
            } else {
                serde_json::from_slice(&response.body)
                    .map(Some)
                    .map_err(Error::Serde)
            }
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
        let headers = Some(self.auth_headers("POST", path, body_str));
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
        let headers = Some(self.auth_headers("DELETE", path, body_str));
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

    /// Fetches a single open order by ID, returning `None` for empty/null responses.
    pub async fn get_order_optional(&self, order_id: &str) -> Result<Option<PolymarketOpenOrder>> {
        let path = format!("/data/order/{order_id}");
        self.send_get_optional::<(), _>(&path, None::<&()>, true)
            .await
    }

    /// Fetches a single open order by ID.
    ///
    /// Returns an error if the order is not found (empty/null response).
    pub async fn get_order(&self, order_id: &str) -> Result<PolymarketOpenOrder> {
        self.get_order_optional(order_id)
            .await?
            .ok_or_else(|| Error::decode(format!("Order {order_id} not found (empty response)")))
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
        let headers = Some(self.auth_headers("GET", PATH_BALANCE_ALLOWANCE, ""));
        let url = self.url(PATH_BALANCE_ALLOWANCE);
        let response = self
            .client
            .request_with_params(Method::GET, url, Some(&params), headers, None, None, None)
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

    /// Submits a single signed order to the exchange.
    pub async fn post_order(
        &self,
        order: &PolymarketOrder,
        order_type: PolymarketOrderType,
        post_only: bool,
    ) -> Result<OrderResponse> {
        let owner = self.credential.api_key().to_string();
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
        let owner = self.credential.api_key().to_string();
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

    /// Fetches the tick size for a token from the CLOB API.
    pub async fn get_tick_size(&self, token_id: &str) -> Result<TickSizeResponse> {
        let params = [("token_id", token_id)];
        self.send_get("/tick-size", Some(&params), false).await
    }

    /// Fetches the fee rate (in basis points) for a token from the CLOB API.
    pub async fn get_fee_rate(&self, token_id: &str) -> Result<FeeRateResponse> {
        let params = [("token_id", token_id)];
        self.send_get("/fee-rate", Some(&params), false).await
    }

    /// Fetches the order book for a token from the CLOB API (public endpoint).
    pub async fn get_book(&self, token_id: &str) -> Result<ClobBookResponse> {
        let params = [("token_id", token_id)];
        self.send_get("/book", Some(&params), false).await
    }
}

/// Provides an unauthenticated HTTP client for public CLOB endpoints.
///
/// Unlike [`PolymarketClobHttpClient`], this client does not require credentials
/// and is suitable for the data client which only needs public market data.
#[derive(Debug, Clone)]
pub struct PolymarketClobPublicClient {
    client: HttpClient,
    base_url: String,
}

impl PolymarketClobPublicClient {
    /// Creates a new [`PolymarketClobPublicClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn new(base_url: Option<String>, timeout_secs: u64) -> StdResult<Self, HttpClientError> {
        Ok(Self {
            client: HttpClient::new(
                HashMap::from([
                    (USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string()),
                    ("Content-Type".to_string(), "application/json".to_string()),
                ]),
                vec![],
                vec![],
                Some(*POLYMARKET_CLOB_REST_QUOTA),
                Some(timeout_secs),
                None,
            )?,
            base_url: base_url
                .unwrap_or_else(|| clob_http_url().to_string())
                .trim_end_matches('/')
                .to_string(),
        })
    }

    /// Fetches the order book for a token from the CLOB API.
    pub async fn get_book(&self, token_id: &str) -> Result<ClobBookResponse> {
        let params = [("token_id", token_id)];
        let url = format!("{}/book", self.base_url);
        let response = self
            .client
            .request_with_params(Method::GET, url, Some(&params), None, None, None, None)
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

    /// Requests an order book snapshot and builds an [`OrderBook`].
    pub async fn request_book_snapshot(
        &self,
        instrument_id: InstrumentId,
        token_id: &str,
        price_precision: u8,
        size_precision: u8,
    ) -> anyhow::Result<OrderBook> {
        let resp = self
            .get_book(token_id)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

        for (i, level) in resp.bids.iter().enumerate() {
            let price = parse_price(&level.price, price_precision)?;
            let size = parse_quantity(&level.size, size_precision)?;
            let order = BookOrder::new(OrderSide::Buy, price, size, i as u64);
            book.add(order, 0, i as u64, Default::default());
        }

        let bids_len = resp.bids.len();
        for (i, level) in resp.asks.iter().enumerate() {
            let price = parse_price(&level.price, price_precision)?;
            let size = parse_quantity(&level.size, size_precision)?;
            let order = BookOrder::new(OrderSide::Sell, price, size, (bids_len + i) as u64);
            book.add(order, 0, (bids_len + i) as u64, Default::default());
        }

        log::info!(
            "Fetched order book for {} with {} bids and {} asks",
            instrument_id,
            resp.bids.len(),
            resp.asks.len(),
        );

        Ok(book)
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::{BookType, OrderSide},
        identifiers::InstrumentId,
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use super::*;
    use crate::http::models::{ClobBookLevel, ClobBookResponse};

    fn build_book_from_response(resp: &ClobBookResponse) -> OrderBook {
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        let price_precision = 2u8;
        let size_precision = 2u8;
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

        for (i, level) in resp.bids.iter().enumerate() {
            let price = parse_price(&level.price, price_precision).unwrap();
            let size = parse_quantity(&level.size, size_precision).unwrap();
            let order = BookOrder::new(OrderSide::Buy, price, size, i as u64);
            book.add(order, 0, i as u64, Default::default());
        }

        let bids_len = resp.bids.len();
        for (i, level) in resp.asks.iter().enumerate() {
            let price = parse_price(&level.price, price_precision).unwrap();
            let size = parse_quantity(&level.size, size_precision).unwrap();
            let order = BookOrder::new(OrderSide::Sell, price, size, (bids_len + i) as u64);
            book.add(order, 0, (bids_len + i) as u64, Default::default());
        }

        book
    }

    #[rstest]
    fn test_build_order_book_from_clob_response() {
        let resp = ClobBookResponse {
            bids: vec![
                ClobBookLevel {
                    price: "0.48".to_string(),
                    size: "100.00".to_string(),
                },
                ClobBookLevel {
                    price: "0.49".to_string(),
                    size: "200.00".to_string(),
                },
                ClobBookLevel {
                    price: "0.50".to_string(),
                    size: "150.00".to_string(),
                },
            ],
            asks: vec![
                ClobBookLevel {
                    price: "0.51".to_string(),
                    size: "120.00".to_string(),
                },
                ClobBookLevel {
                    price: "0.52".to_string(),
                    size: "180.00".to_string(),
                },
            ],
        };

        let book = build_book_from_response(&resp);

        assert_eq!(book.instrument_id, InstrumentId::from("TEST.POLYMARKET"));
        assert_eq!(book.book_type, BookType::L2_MBP);
        assert_eq!(book.best_bid_price(), Some(Price::from("0.50")));
        assert_eq!(book.best_ask_price(), Some(Price::from("0.51")));
        assert_eq!(book.best_bid_size(), Some(Quantity::from("150.00")));
        assert_eq!(book.best_ask_size(), Some(Quantity::from("120.00")));
        assert_eq!(book.bids(None).count(), 3);
        assert_eq!(book.asks(None).count(), 2);
    }

    #[rstest]
    fn test_build_order_book_empty_response() {
        let resp = ClobBookResponse {
            bids: vec![],
            asks: vec![],
        };

        let book = build_book_from_response(&resp);

        assert!(book.best_bid_price().is_none());
        assert!(book.best_ask_price().is_none());
    }
}
