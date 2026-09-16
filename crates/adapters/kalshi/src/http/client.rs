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

//! HTTP client for the Kalshi Trade API.

use std::{
    collections::HashMap,
    time::{SystemTime, UNIX_EPOCH},
};

use nautilus_network::http::HttpClient;
use serde::de::DeserializeOwned;

use crate::{
    common::{
        enums::KalshiMarketStatus,
        urls::{
            API_PATH_PREFIX, PATH_BALANCE, PATH_EVENT_ORDERS, PATH_EVENT_ORDERS_BATCHED,
            PATH_EVENTS, PATH_EXCHANGE_STATUS, PATH_FILLS, PATH_HISTORICAL_CUTOFF, PATH_MARKETS,
            PATH_ORDERS, PATH_POSITIONS, PATH_TRADES, amend_order_path, cancel_order_path,
            event_path, market_orderbook_path, market_path, order_path,
        },
    },
    http::{
        auth::KalshiAuth,
        error::{Error, Result, is_success},
        models::{
            KalshiAmendOrderRequest, KalshiAmendOrderResponse, KalshiBalanceResponse,
            KalshiBatchCreateOrdersRequest, KalshiBatchCreateOrdersResponse,
            KalshiCancelOrderResponse, KalshiCreateOrderRequest, KalshiCreateOrderResponse,
            KalshiEvent, KalshiEventResponse, KalshiEventsResponse, KalshiExchangeStatus,
            KalshiFill, KalshiFillsResponse, KalshiHistoricalCutoff, KalshiMarket,
            KalshiMarketResponse, KalshiMarketsResponse, KalshiOrder, KalshiOrderResponse,
            KalshiOrderbook, KalshiOrderbookResponse, KalshiOrdersResponse,
            KalshiPositionsResponse, KalshiTradesResponse,
        },
    },
};

/// The default request timeout in seconds.
pub const DEFAULT_TIMEOUT_SECS: u64 = 10;

/// The largest number of pages a paginated request will fetch before reporting that it cannot
/// terminate, which bounds a runaway cursor.
pub const MAX_PAGES: usize = 100;

/// A client for the Kalshi Trade API.
///
/// Unauthenticated endpoints are available without credentials. Authenticated endpoints require an
/// [`KalshiAuth`], and report [`Error::MissingCredential`] when one is absent rather than sending a
/// request the exchange will reject.
#[derive(Clone, Debug)]
pub struct KalshiHttpClient {
    client: HttpClient,
    base_url: String,
    auth: Option<KalshiAuth>,
    timeout_secs: Option<u64>,
}

impl KalshiHttpClient {
    /// Creates a new [`KalshiHttpClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be built.
    pub fn new(
        base_url: Option<String>,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
        auth: Option<KalshiAuth>,
    ) -> Result<Self> {
        let base_url = base_url.unwrap_or_else(|| crate::common::urls::DEMO_REST_URL.to_string());
        let client = HttpClient::builder()
            .maybe_default_quota(None)
            .maybe_timeout_secs(timeout_secs)
            .maybe_proxy_url(proxy_url)
            .maybe_rate_limiters(None)
            .build()
            .map_err(|e| Error::HttpClient(e.to_string()))?;

        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_string(),
            auth,
            timeout_secs,
        })
    }

    /// Returns the REST base URL the client targets.
    #[must_use]
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Returns whether the client can make authenticated requests.
    #[must_use]
    pub const fn is_authenticated(&self) -> bool {
        self.auth.is_some()
    }

    /// Returns the current Unix time in milliseconds.
    ///
    /// The exchange expects a millisecond timestamp as part of every signature.
    fn now_millis() -> Result<i64> {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| Error::Signature(format!("System clock is before the Unix epoch: {e}")))?
            .as_millis();

        i64::try_from(millis)
            .map_err(|_| Error::Signature("System clock is out of range".to_string()))
    }

    /// Returns the authentication headers for a request, or an error when unsigned requests are
    /// refused.
    fn headers_for(
        &self,
        method: &str,
        path: &str,
        signed: bool,
    ) -> Result<Option<HashMap<String, String>>> {
        if !signed {
            return Ok(None);
        }

        let auth = self.auth.as_ref().ok_or_else(|| {
            Error::MissingCredential(format!("{method} {path} requires a Kalshi API credential"))
        })?;
        let signing_path = format!("{API_PATH_PREFIX}{path}");

        auth.headers(method, &signing_path, Self::now_millis()?)
            .map(Some)
    }

    /// Sends a request and decodes its body.
    async fn send<T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        params: Option<HashMap<String, Vec<String>>>,
        body: Option<Vec<u8>>,
        signed: bool,
    ) -> Result<T> {
        let url = format!("{}{path}", self.base_url);
        let headers = self.headers_for(method, path, signed)?;
        let response = match method {
            "GET" => {
                self.client
                    .get(url, params.as_ref(), headers, self.timeout_secs, None)
                    .await?
            }
            "POST" => {
                self.client
                    .post(url, params.as_ref(), headers, body, self.timeout_secs, None)
                    .await?
            }
            "DELETE" => {
                self.client
                    .delete(url, params.as_ref(), headers, self.timeout_secs, None)
                    .await?
            }
            other => {
                return Err(Error::HttpClient(format!(
                    "Unsupported HTTP method '{other}'"
                )));
            }
        };

        if !is_success(response.status.as_u16()) {
            return Err(Error::from_status_code(
                response.status.as_u16(),
                &String::from_utf8_lossy(&response.body),
            ));
        }

        serde_json::from_slice(&response.body).map_err(|e| Error::Serde(e.to_string()))
    }

    /// Returns one page of markets for the given filters.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or its body does not decode.
    pub async fn get_markets(
        &self,
        status: Option<KalshiMarketStatus>,
        event_ticker: Option<&str>,
        series_ticker: Option<&str>,
        limit: Option<u32>,
        cursor: Option<&str>,
    ) -> Result<KalshiMarketsResponse> {
        let mut params: HashMap<String, Vec<String>> = HashMap::new();

        if let Some(status) = status {
            params.insert("status".to_string(), vec![status.to_string()]);
        }

        if let Some(event_ticker) = event_ticker {
            params.insert("event_ticker".to_string(), vec![event_ticker.to_string()]);
        }

        if let Some(series_ticker) = series_ticker {
            params.insert("series_ticker".to_string(), vec![series_ticker.to_string()]);
        }

        if let Some(limit) = limit {
            params.insert("limit".to_string(), vec![limit.to_string()]);
        }

        if let Some(cursor) = cursor.filter(|value| !value.is_empty()) {
            params.insert("cursor".to_string(), vec![cursor.to_string()]);
        }

        self.send("GET", PATH_MARKETS, Some(params), None, false)
            .await
    }

    /// Returns every market matching the filters, following pagination to its end.
    ///
    /// # Errors
    ///
    /// Returns an error if any page fails, does not decode, or the cursor does not terminate.
    pub async fn get_all_markets(
        &self,
        status: Option<KalshiMarketStatus>,
        event_ticker: Option<&str>,
        series_ticker: Option<&str>,
    ) -> Result<Vec<KalshiMarket>> {
        let mut markets = Vec::new();
        let mut cursor: Option<String> = None;

        for _ in 0..MAX_PAGES {
            let page = self
                .get_markets(
                    status,
                    event_ticker,
                    series_ticker,
                    Some(crate::common::consts::MAX_PAGE_LIMIT),
                    cursor.as_deref(),
                )
                .await?;

            markets.extend(page.markets);

            match page.cursor.filter(|value| !value.is_empty()) {
                Some(next) => cursor = Some(next),
                None => return Ok(markets),
            }
        }

        Err(Error::Pagination(format!(
            "Kalshi markets pagination exceeded {MAX_PAGES} pages"
        )))
    }

    /// Returns one market by ticker.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or its body does not decode.
    pub async fn get_market(&self, ticker: &str) -> Result<KalshiMarket> {
        let response: KalshiMarketResponse = self
            .send("GET", &market_path(ticker), None, None, false)
            .await?;

        Ok(response.market)
    }

    /// Returns an event, optionally nesting its markets.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or its body does not decode.
    pub async fn get_event(
        &self,
        event_ticker: &str,
        with_nested_markets: bool,
    ) -> Result<KalshiEventResponse> {
        let params = with_nested_markets.then(|| {
            HashMap::from([("with_nested_markets".to_string(), vec!["true".to_string()])])
        });

        self.send("GET", &event_path(event_ticker), params, None, false)
            .await
    }

    /// Returns one page of events.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or its body does not decode.
    pub async fn get_events(
        &self,
        status: Option<&str>,
        series_ticker: Option<&str>,
        limit: Option<u32>,
        cursor: Option<&str>,
    ) -> Result<KalshiEventsResponse> {
        let mut params: HashMap<String, Vec<String>> = HashMap::new();

        if let Some(status) = status {
            params.insert("status".to_string(), vec![status.to_string()]);
        }

        if let Some(series_ticker) = series_ticker {
            params.insert("series_ticker".to_string(), vec![series_ticker.to_string()]);
        }

        if let Some(limit) = limit {
            params.insert("limit".to_string(), vec![limit.to_string()]);
        }

        if let Some(cursor) = cursor.filter(|value| !value.is_empty()) {
            params.insert("cursor".to_string(), vec![cursor.to_string()]);
        }

        self.send("GET", PATH_EVENTS, Some(params), None, false)
            .await
    }

    /// Returns the order book of a market.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or its body does not decode.
    pub async fn get_market_orderbook(
        &self,
        ticker: &str,
        depth: Option<u32>,
    ) -> Result<KalshiOrderbook> {
        let params =
            depth.map(|depth| HashMap::from([("depth".to_string(), vec![depth.to_string()])]));
        let response: KalshiOrderbookResponse = self
            .send("GET", &market_orderbook_path(ticker), params, None, false)
            .await?;

        Ok(response.orderbook_fp)
    }

    /// Returns the public trades of a market within an optional time window.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or its body does not decode.
    pub async fn get_trades(
        &self,
        ticker: Option<&str>,
        min_ts: Option<i64>,
        max_ts: Option<i64>,
        limit: Option<u32>,
        cursor: Option<&str>,
    ) -> Result<KalshiTradesResponse> {
        let mut params: HashMap<String, Vec<String>> = HashMap::new();

        if let Some(ticker) = ticker {
            params.insert("ticker".to_string(), vec![ticker.to_string()]);
        }

        if let Some(min_ts) = min_ts {
            params.insert("min_ts".to_string(), vec![min_ts.to_string()]);
        }

        if let Some(max_ts) = max_ts {
            params.insert("max_ts".to_string(), vec![max_ts.to_string()]);
        }

        if let Some(limit) = limit {
            params.insert("limit".to_string(), vec![limit.to_string()]);
        }

        if let Some(cursor) = cursor.filter(|value| !value.is_empty()) {
            params.insert("cursor".to_string(), vec![cursor.to_string()]);
        }

        self.send("GET", PATH_TRADES, Some(params), None, false)
            .await
    }

    /// Returns the exchange status.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or its body does not decode.
    pub async fn get_exchange_status(&self) -> Result<KalshiExchangeStatus> {
        self.send("GET", PATH_EXCHANGE_STATUS, None, None, false)
            .await
    }

    /// Returns the timestamp that separates the venue's live and historical data.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or its body does not decode.
    pub async fn get_historical_cutoff(&self) -> Result<KalshiHistoricalCutoff> {
        self.send("GET", PATH_HISTORICAL_CUTOFF, None, None, false)
            .await
    }

    /// Returns the member's balance.
    ///
    /// # Errors
    ///
    /// Returns an error if no credential is configured, the request fails, or its body does not
    /// decode.
    pub async fn get_balance(&self) -> Result<KalshiBalanceResponse> {
        self.send("GET", PATH_BALANCE, None, None, true).await
    }

    /// Returns the member's positions.
    ///
    /// # Errors
    ///
    /// Returns an error if no credential is configured, the request fails, or its body does not
    /// decode.
    pub async fn get_positions(
        &self,
        ticker: Option<&str>,
        event_ticker: Option<&str>,
        limit: Option<u32>,
        cursor: Option<&str>,
    ) -> Result<KalshiPositionsResponse> {
        let mut params: HashMap<String, Vec<String>> = HashMap::new();

        if let Some(ticker) = ticker {
            params.insert("ticker".to_string(), vec![ticker.to_string()]);
        }

        if let Some(event_ticker) = event_ticker {
            params.insert("event_ticker".to_string(), vec![event_ticker.to_string()]);
        }

        if let Some(limit) = limit {
            params.insert("limit".to_string(), vec![limit.to_string()]);
        }

        if let Some(cursor) = cursor.filter(|value| !value.is_empty()) {
            params.insert("cursor".to_string(), vec![cursor.to_string()]);
        }

        self.send("GET", PATH_POSITIONS, Some(params), None, true)
            .await
    }

    /// Returns every position the member holds, following pagination to its end.
    ///
    /// # Errors
    ///
    /// Returns an error if any page fails, does not decode, or the cursor does not terminate.
    pub async fn get_all_positions(&self) -> Result<KalshiPositionsResponse> {
        let mut positions = KalshiPositionsResponse {
            market_positions: Vec::new(),
            event_positions: Vec::new(),
            cursor: None,
        };
        let mut cursor: Option<String> = None;

        for _ in 0..MAX_PAGES {
            let page = self
                .get_positions(
                    None,
                    None,
                    Some(crate::common::consts::MAX_PAGE_LIMIT),
                    cursor.as_deref(),
                )
                .await?;

            positions.market_positions.extend(page.market_positions);
            positions.event_positions.extend(page.event_positions);

            match page.cursor.filter(|value| !value.is_empty()) {
                Some(next) => cursor = Some(next),
                None => return Ok(positions),
            }
        }

        Err(Error::Pagination(format!(
            "Kalshi positions pagination exceeded {MAX_PAGES} pages"
        )))
    }

    /// Returns the event a market belongs to, with its markets nested.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or its body does not decode.
    pub async fn get_event_for_market(&self, market: &KalshiMarket) -> Result<KalshiEvent> {
        let response = self.get_event(&market.event_ticker, true).await?;

        Ok(response.event)
    }

    /// Sends a request whose response carries no body.
    async fn send_no_content(&self, path: &str) -> Result<()> {
        let url = format!("{}{path}", self.base_url);
        let headers = self.headers_for("DELETE", path, true)?;
        let response = self
            .client
            .delete(url, None, headers, self.timeout_secs, None)
            .await?;

        if !is_success(response.status.as_u16()) {
            return Err(Error::from_status_code(
                response.status.as_u16(),
                &String::from_utf8_lossy(&response.body),
            ));
        }

        Ok(())
    }

    /// Submits one order.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or its body does not decode.
    pub async fn create_order(
        &self,
        request: &KalshiCreateOrderRequest,
    ) -> Result<KalshiCreateOrderResponse> {
        let body = serde_json::to_vec(request)?;

        self.send("POST", PATH_EVENT_ORDERS, None, Some(body), true)
            .await
    }

    /// Submits several orders in one request.
    ///
    /// A batch answers per order, so a caller has to inspect each result: the exchange reports a
    /// refused order in its own `error` field rather than failing the whole request.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or its body does not decode.
    pub async fn create_orders(
        &self,
        requests: &[KalshiCreateOrderRequest],
    ) -> Result<KalshiBatchCreateOrdersResponse> {
        let batch = KalshiBatchCreateOrdersRequest {
            orders: requests.to_vec(),
        };
        let body = serde_json::to_vec(&batch)?;

        self.send("POST", PATH_EVENT_ORDERS_BATCHED, None, Some(body), true)
            .await
    }

    /// Cancels one order.
    ///
    /// The market ticker is sent so the exchange routes the cancellation to the shard the order lives
    /// on: an order identifier alone identifies no shard, and a cancellation that reaches the wrong
    /// one cannot find the order.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or its body does not decode.
    pub async fn cancel_order(
        &self,
        order_id: &str,
        market_ticker: &str,
    ) -> Result<KalshiCancelOrderResponse> {
        let params =
            HashMap::from([("market_ticker".to_string(), vec![market_ticker.to_string()])]);

        self.send(
            "DELETE",
            &cancel_order_path(order_id),
            Some(params),
            None,
            true,
        )
        .await
    }

    /// Cancels every resting order of the member.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails.
    pub async fn cancel_all_orders(&self) -> Result<()> {
        self.send_no_content(PATH_EVENT_ORDERS).await
    }

    /// Amends a resting order.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or its body does not decode.
    pub async fn amend_order(
        &self,
        order_id: &str,
        request: &KalshiAmendOrderRequest,
    ) -> Result<KalshiAmendOrderResponse> {
        let body = serde_json::to_vec(request)?;

        self.send("POST", &amend_order_path(order_id), None, Some(body), true)
            .await
    }

    /// Returns one order by venue identifier.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or its body does not decode.
    pub async fn get_order(&self, order_id: &str) -> Result<KalshiOrder> {
        let response: KalshiOrderResponse = self
            .send("GET", &order_path(order_id), None, None, true)
            .await?;

        Ok(response.order)
    }

    /// Returns one page of the member's orders.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or its body does not decode.
    pub async fn get_orders(
        &self,
        ticker: Option<&str>,
        limit: Option<u32>,
        cursor: Option<&str>,
    ) -> Result<KalshiOrdersResponse> {
        let mut params: HashMap<String, Vec<String>> = HashMap::new();

        if let Some(ticker) = ticker {
            params.insert("ticker".to_string(), vec![ticker.to_string()]);
        }

        if let Some(limit) = limit {
            params.insert("limit".to_string(), vec![limit.to_string()]);
        }

        if let Some(cursor) = cursor.filter(|value| !value.is_empty()) {
            params.insert("cursor".to_string(), vec![cursor.to_string()]);
        }

        self.send("GET", PATH_ORDERS, Some(params), None, true)
            .await
    }

    /// Returns one page of the member's fills.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or its body does not decode.
    pub async fn get_fills(
        &self,
        ticker: Option<&str>,
        order_id: Option<&str>,
        min_ts: Option<i64>,
        max_ts: Option<i64>,
        limit: Option<u32>,
        cursor: Option<&str>,
    ) -> Result<KalshiFillsResponse> {
        let mut params: HashMap<String, Vec<String>> = HashMap::new();

        if let Some(ticker) = ticker {
            params.insert("ticker".to_string(), vec![ticker.to_string()]);
        }

        if let Some(order_id) = order_id {
            params.insert("order_id".to_string(), vec![order_id.to_string()]);
        }

        if let Some(min_ts) = min_ts {
            params.insert("min_ts".to_string(), vec![min_ts.to_string()]);
        }

        if let Some(max_ts) = max_ts {
            params.insert("max_ts".to_string(), vec![max_ts.to_string()]);
        }

        if let Some(limit) = limit {
            params.insert("limit".to_string(), vec![limit.to_string()]);
        }

        if let Some(cursor) = cursor.filter(|value| !value.is_empty()) {
            params.insert("cursor".to_string(), vec![cursor.to_string()]);
        }

        self.send("GET", PATH_FILLS, Some(params), None, true).await
    }

    /// Returns every fill matching the filters, following pagination to its end.
    ///
    /// # Errors
    ///
    /// Returns an error if any page fails, does not decode, or the cursor does not terminate.
    pub async fn get_all_fills(
        &self,
        ticker: Option<&str>,
        order_id: Option<&str>,
        min_ts: Option<i64>,
        max_ts: Option<i64>,
    ) -> Result<Vec<KalshiFill>> {
        let mut fills = Vec::new();
        let mut cursor: Option<String> = None;

        for _ in 0..MAX_PAGES {
            let page = self
                .get_fills(
                    ticker,
                    order_id,
                    min_ts,
                    max_ts,
                    Some(crate::common::consts::MAX_PAGE_LIMIT),
                    cursor.as_deref(),
                )
                .await?;

            fills.extend(page.fills);

            match page.cursor.filter(|value| !value.is_empty()) {
                Some(next) => cursor = Some(next),
                None => return Ok(fills),
            }
        }

        Err(Error::Pagination(format!(
            "Kalshi fills pagination exceeded {MAX_PAGES} pages"
        )))
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::{credential::KalshiCredential, urls};

    fn client() -> KalshiHttpClient {
        KalshiHttpClient::new(Some(urls::DEMO_REST_URL.to_string()), Some(5), None, None).unwrap()
    }

    #[rstest]
    fn test_client_normalizes_the_base_url() {
        let client = KalshiHttpClient::new(
            Some("https://example.test/trade-api/v2/".to_string()),
            None,
            None,
            None,
        )
        .unwrap();

        assert_eq!(client.base_url(), "https://example.test/trade-api/v2");
        assert!(!client.is_authenticated());
    }

    #[rstest]
    fn test_client_defaults_to_the_demo_exchange() {
        assert_eq!(client().base_url(), urls::DEMO_REST_URL);
    }

    #[rstest]
    fn test_authenticated_requests_require_a_credential() {
        let error = client().headers_for("GET", PATH_BALANCE, true).unwrap_err();

        assert!(matches!(error, Error::MissingCredential(_)));
        assert!(error.to_string().contains("/portfolio/balance"), "{error}");
        assert!(!error.is_retryable());
    }

    #[rstest]
    fn test_public_requests_carry_no_headers() {
        assert!(
            client()
                .headers_for("GET", PATH_MARKETS, false)
                .unwrap()
                .is_none()
        );
    }

    #[rstest]
    fn test_signed_path_includes_the_api_prefix() {
        let auth = KalshiAuth::new(KalshiCredential::new(
            "key".to_string(),
            "not a pem".to_string(),
        ));
        let client = KalshiHttpClient::new(
            Some(urls::DEMO_REST_URL.to_string()),
            None,
            None,
            Some(auth),
        )
        .unwrap();

        // The credential cannot sign, so the failure proves the header path was attempted.
        let error = client.headers_for("GET", PATH_BALANCE, true).unwrap_err();

        assert!(matches!(error, Error::Signature(_)));
    }
}
