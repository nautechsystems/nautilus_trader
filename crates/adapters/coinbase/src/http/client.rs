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

//! Provides the HTTP client for the Coinbase Advanced Trade REST API.
//!
//! Two-layer architecture:
//! - [`CoinbaseRawHttpClient`]: low-level endpoint methods, JWT auth, rate limiting.
//! - [`CoinbaseHttpClient`]: domain wrapper with instrument caching and Nautilus type conversions.

use std::{
    collections::HashMap,
    num::NonZeroU32,
    sync::{Arc, LazyLock},
};

use arc_swap::ArcSwap;
use chrono::{DateTime, Utc};
use nautilus_core::{
    AtomicMap, UnixNanos,
    consts::NAUTILUS_USER_AGENT,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    enums::{OrderSide, OrderType, TimeInForce},
    events::AccountState,
    identifiers::{AccountId, ClientOrderId, InstrumentId, Symbol, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{MarginBalance, Price, Quantity},
};
use nautilus_network::{
    http::{HttpClient, HttpClientError, HttpResponse, Method, USER_AGENT},
    ratelimiter::quota::Quota,
    retry::{RetryConfig, RetryManager},
};
use rust_decimal::Decimal;
use serde_json::Value;
use tokio_util::sync::CancellationToken;
use url::form_urlencoded;
use ustr::Ustr;

use crate::{
    common::{
        consts::{ACCOUNTS_PAGE_LIMIT, ORDER_STATUS_OPEN, REST_API_PATH},
        credential::CoinbaseCredential,
        enums::{
            CoinbaseEnvironment, CoinbaseMarginType, CoinbaseOrderSide, CoinbaseProductType,
            CoinbaseStopDirection,
        },
        parse::format_rfc3339_from_nanos,
        urls,
    },
    http::{
        error::{Error, Result},
        models::{
            Account, AccountsResponse, CancelOrdersResponse, CfmBalanceSummary,
            CfmBalanceSummaryResponse, CfmPositionResponse, CfmPositionsResponse,
            CreateOrderResponse, EditOrderResponse, Fill, FillsResponse, Order, OrderResponse,
            OrdersListResponse, ProductsResponse,
        },
        parse::{
            parse_account_state, parse_cfm_account_state, parse_cfm_margin_balances,
            parse_cfm_position_status_report, parse_fill_report, parse_instrument,
            parse_order_status_report,
        },
        query::{
            CancelOrdersRequest, CreateOrderRequest, EditOrderRequest, FillListQuery, LimitFok,
            LimitFokParams, LimitGtc, LimitGtcParams, LimitGtd, LimitGtdParams, MarketFok,
            MarketIoc, MarketParams, OrderConfiguration, OrderListQuery, StopLimitGtc,
            StopLimitGtcParams, StopLimitGtd, StopLimitGtdParams,
        },
    },
};

/// Default Coinbase Advanced Trade REST rate limit (30 requests per second).
pub static COINBASE_REST_QUOTA: LazyLock<Quota> = LazyLock::new(|| {
    Quota::per_second(NonZeroU32::new(30).expect("non-zero")).expect("valid constant")
});

/// Returns the default retry configuration for the Coinbase HTTP client.
#[must_use]
pub fn default_retry_config() -> RetryConfig {
    RetryConfig {
        max_retries: 3,
        initial_delay_ms: 100,
        max_delay_ms: 5_000,
        backoff_factor: 2.0,
        jitter_ms: 250,
        operation_timeout_ms: Some(60_000),
        immediate_first: false,
        max_elapsed_ms: Some(180_000),
    }
}

/// Returns the retry configuration for the Coinbase data client.
///
/// Historical requests spawn detached tasks outside the client's
/// cancellation token; `max_retries = 0` keeps them bounded by a single
/// HTTP timeout so a shut-down client cannot keep emitting `DataResponse`s.
#[must_use]
pub fn data_client_retry_config() -> RetryConfig {
    RetryConfig {
        max_retries: 0,
        initial_delay_ms: 100,
        max_delay_ms: 100,
        backoff_factor: 1.0,
        jitter_ms: 0,
        operation_timeout_ms: None,
        immediate_first: false,
        max_elapsed_ms: None,
    }
}

// Builds a query string from `(key, value)` pairs, percent-encoding both
// halves. Coinbase cursors and RFC 3339 timestamps (`+00:00`) contain
// reserved characters that must be encoded to avoid the server reading
// them as a different query.
fn encode_query(params: &[(&str, &str)]) -> String {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    for (k, v) in params {
        serializer.append_pair(k, v);
    }
    serializer.finish()
}

/// Provides a raw HTTP client for low-level Coinbase Advanced Trade REST API operations.
///
/// Handles JWT authentication, request construction, and response parsing.
/// Each request generates a fresh ES256 JWT for authentication.
#[derive(Debug)]
pub struct CoinbaseRawHttpClient {
    client: HttpClient,
    credential: Option<CoinbaseCredential>,
    base_url: ArcSwap<String>,
    environment: CoinbaseEnvironment,
    retry_manager: RetryManager<Error>,
    cancellation_token: CancellationToken,
}

impl CoinbaseRawHttpClient {
    /// Creates a new [`CoinbaseRawHttpClient`] for public endpoints only.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn new(
        environment: CoinbaseEnvironment,
        timeout_secs: u64,
        proxy_url: Option<String>,
        retry_config: Option<RetryConfig>,
    ) -> std::result::Result<Self, HttpClientError> {
        Ok(Self {
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                vec![],
                Some(*COINBASE_REST_QUOTA),
                Some(timeout_secs),
                proxy_url,
            )?,
            credential: None,
            base_url: ArcSwap::from_pointee(urls::rest_url(environment).to_string()),
            environment,
            retry_manager: RetryManager::new(retry_config.unwrap_or_else(default_retry_config)),
            cancellation_token: CancellationToken::new(),
        })
    }

    /// Creates a new [`CoinbaseRawHttpClient`] with credentials for authenticated requests.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn with_credentials(
        credential: CoinbaseCredential,
        environment: CoinbaseEnvironment,
        timeout_secs: u64,
        proxy_url: Option<String>,
        retry_config: Option<RetryConfig>,
    ) -> std::result::Result<Self, HttpClientError> {
        Ok(Self {
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                vec![],
                Some(*COINBASE_REST_QUOTA),
                Some(timeout_secs),
                proxy_url,
            )?,
            credential: Some(credential),
            base_url: ArcSwap::from_pointee(urls::rest_url(environment).to_string()),
            environment,
            retry_manager: RetryManager::new(retry_config.unwrap_or_else(default_retry_config)),
            cancellation_token: CancellationToken::new(),
        })
    }

    /// Creates an authenticated client from environment variables.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Auth`] if required environment variables are not set.
    pub fn from_env(environment: CoinbaseEnvironment) -> Result<Self> {
        let credential = CoinbaseCredential::from_env()
            .map_err(|e| Error::auth(format!("Missing credentials in environment: {e}")))?;
        Self::with_credentials(credential, environment, 10, None, None)
            .map_err(|e| Error::auth(format!("Failed to create HTTP client: {e}")))
    }

    /// Creates a new [`CoinbaseRawHttpClient`] with explicit credentials.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Auth`] if credentials are invalid.
    pub fn from_credentials(
        api_key: &str,
        api_secret: &str,
        environment: CoinbaseEnvironment,
        timeout_secs: u64,
        proxy_url: Option<String>,
        retry_config: Option<RetryConfig>,
    ) -> Result<Self> {
        let credential = CoinbaseCredential::new(api_key.to_string(), api_secret.to_string());
        Self::with_credentials(
            credential,
            environment,
            timeout_secs,
            proxy_url,
            retry_config,
        )
        .map_err(|e| Error::auth(format!("Failed to create HTTP client: {e}")))
    }

    /// Returns the cancellation token shared by in-flight requests.
    #[must_use]
    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.cancellation_token
    }

    /// Overrides the base REST URL (for testing with mock servers).
    ///
    /// Lock-free; safe to call after the client has been cloned.
    pub fn set_base_url(&self, url: String) {
        self.base_url.store(Arc::new(url));
    }

    /// Returns the configured environment.
    #[must_use]
    pub fn environment(&self) -> CoinbaseEnvironment {
        self.environment
    }

    /// Returns true if this client has credentials for authenticated requests.
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.credential.is_some()
    }

    fn default_headers() -> HashMap<String, String> {
        HashMap::from([
            (USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string()),
            ("Content-Type".to_string(), "application/json".to_string()),
        ])
    }

    fn build_url(&self, path: &str) -> String {
        format!("{}{REST_API_PATH}{path}", self.base_url.load())
    }

    // JWT uri claim must match the actual request host
    fn build_jwt_uri(&self, method: &str, path: &str) -> String {
        let base = self.base_url.load();
        let host = base
            .strip_prefix("https://")
            .or_else(|| base.strip_prefix("http://"))
            .unwrap_or(base.as_str());
        format!("{method} {host}{REST_API_PATH}{path}")
    }

    fn auth_headers(&self, method: &str, path: &str) -> Result<HashMap<String, String>> {
        let credential = self
            .credential
            .as_ref()
            .ok_or_else(|| Error::auth("No credentials configured"))?;

        let uri = self.build_jwt_uri(method, path);
        let jwt = credential.build_rest_jwt(&uri)?;

        Ok(HashMap::from([(
            "Authorization".to_string(),
            format!("Bearer {jwt}"),
        )]))
    }

    fn parse_response(&self, response: &HttpResponse) -> Result<Value> {
        if !response.status.is_success() {
            return Err(Error::from_http_status(
                response.status.as_u16(),
                &response.body,
            ));
        }

        if response.body.is_empty() {
            return Ok(Value::Null);
        }

        serde_json::from_slice(&response.body).map_err(Error::Serde)
    }

    // Retries are gated to GET/DELETE because Coinbase POST endpoints
    // (`/orders`, `/orders/edit`, `/orders/batch_cancel`) mutate live state
    // and a replay could submit, edit, or cancel twice. JWT headers are
    // rebuilt on each attempt because Coinbase JWTs expire after 120s.
    async fn send_request(
        &self,
        method: Method,
        url: String,
        sign_method: Option<&'static str>,
        sign_path: Option<&str>,
        body: Option<Vec<u8>>,
    ) -> Result<Value> {
        let sign_path_owned = sign_path.map(ToOwned::to_owned);
        let operation_name = sign_path_owned
            .as_deref()
            .unwrap_or(url.as_str())
            .to_string();

        let is_idempotent = matches!(method, Method::GET | Method::DELETE);

        let operation = || {
            let method = method.clone();
            let url = url.clone();
            let body = body.clone();
            let sign_path = sign_path_owned.clone();

            async move {
                let headers = match (sign_method, sign_path.as_deref()) {
                    (Some(m), Some(p)) => Some(self.auth_headers(m, p)?),
                    _ => None,
                };

                let response = self
                    .client
                    .request(method, url, None, headers, body, None, None)
                    .await
                    .map_err(Error::from_http_client)?;

                self.parse_response(&response)
            }
        };

        let should_retry = move |err: &Error| is_idempotent && err.is_retryable();

        self.retry_manager
            .execute_with_retry_with_cancel(
                &operation_name,
                operation,
                should_retry,
                Error::transport,
                &self.cancellation_token,
            )
            .await
    }

    /// Sends a GET request to a public endpoint (no auth required).
    pub async fn get_public(&self, path: &str) -> Result<Value> {
        let url = self.build_url(path);
        self.send_request(Method::GET, url, None, None, None).await
    }

    /// Sends a GET request with query parameters to a public endpoint.
    pub async fn get_public_with_query(&self, path: &str, query: &str) -> Result<Value> {
        let full_path = if query.is_empty() {
            path.to_string()
        } else {
            format!("{path}?{query}")
        };
        let url = self.build_url(&full_path);
        self.send_request(Method::GET, url, None, None, None).await
    }

    /// Sends an authenticated GET request.
    pub async fn get(&self, path: &str) -> Result<Value> {
        let url = self.build_url(path);
        self.send_request(Method::GET, url, Some("GET"), Some(path), None)
            .await
    }

    /// Sends an authenticated GET request with query parameters appended to the path.
    ///
    /// The JWT URI claim covers only `{METHOD} {host}{path}` without the
    /// query string, matching the Coinbase SDK convention. Query parameters
    /// are appended to the URL but excluded from the signing input.
    pub async fn get_with_query(&self, path: &str, query: &str) -> Result<Value> {
        let full_url_path = if query.is_empty() {
            path.to_string()
        } else {
            format!("{path}?{query}")
        };
        let url = self.build_url(&full_url_path);
        // Sign with the bare path only (no query string).
        self.send_request(Method::GET, url, Some("GET"), Some(path), None)
            .await
    }

    /// Sends an authenticated POST request with a JSON body.
    pub async fn post(&self, path: &str, body: &Value) -> Result<Value> {
        let url = self.build_url(path);
        let body_bytes = serde_json::to_vec(body).map_err(Error::Serde)?;
        self.send_request(
            Method::POST,
            url,
            Some("POST"),
            Some(path),
            Some(body_bytes),
        )
        .await
    }

    /// Sends an authenticated DELETE request.
    pub async fn delete(&self, path: &str) -> Result<Value> {
        let url = self.build_url(path);
        self.send_request(Method::DELETE, url, Some("DELETE"), Some(path), None)
            .await
    }

    /// Gets all available products via the public `/market/products` endpoint.
    pub async fn get_products(&self) -> Result<Value> {
        self.get_public("/market/products").await
    }

    /// Gets a specific product by ID via the public endpoint.
    pub async fn get_product(&self, product_id: &str) -> Result<Value> {
        self.get_public(&format!("/market/products/{product_id}"))
            .await
    }

    /// Gets candles for a product via the public endpoint.
    pub async fn get_candles(
        &self,
        product_id: &str,
        start: &str,
        end: &str,
        granularity: &str,
    ) -> Result<Value> {
        let query = format!("start={start}&end={end}&granularity={granularity}");
        self.get_public_with_query(&format!("/market/products/{product_id}/candles"), &query)
            .await
    }

    /// Gets market trades for a product via the public endpoint.
    pub async fn get_market_trades(&self, product_id: &str, limit: u32) -> Result<Value> {
        let query = format!("limit={limit}");
        self.get_public_with_query(&format!("/market/products/{product_id}/ticker"), &query)
            .await
    }

    /// Gets best bid/ask for one or more products.
    ///
    /// No public `/market/` equivalent exists for this endpoint; requires
    /// authentication.
    pub async fn get_best_bid_ask(&self, product_ids: &[&str]) -> Result<Value> {
        let query = product_ids
            .iter()
            .map(|id| format!("product_ids={id}"))
            .collect::<Vec<_>>()
            .join("&");
        self.get_with_query("/best_bid_ask", &query).await
    }

    /// Gets the product order book via the public endpoint.
    pub async fn get_product_book(&self, product_id: &str, limit: Option<u32>) -> Result<Value> {
        let mut query = format!("product_id={product_id}");

        if let Some(limit) = limit {
            query.push_str(&format!("&limit={limit}"));
        }
        self.get_public_with_query("/market/product_book", &query)
            .await
    }

    /// Gets all accounts.
    pub async fn get_accounts(&self) -> Result<Value> {
        self.get("/accounts").await
    }

    /// Gets accounts with a query string (for pagination via `cursor` / `limit`).
    pub async fn get_accounts_with_query(&self, query: &str) -> Result<Value> {
        if query.is_empty() {
            self.get("/accounts").await
        } else {
            self.get_with_query("/accounts", query).await
        }
    }

    /// Gets a specific account by UUID.
    pub async fn get_account(&self, account_id: &str) -> Result<Value> {
        self.get(&format!("/accounts/{account_id}")).await
    }

    /// Lists all portfolios visible to the authenticated key.
    pub async fn get_portfolios(&self) -> Result<Value> {
        self.get("/portfolios").await
    }

    /// Gets historical orders.
    pub async fn get_orders(&self, query: &str) -> Result<Value> {
        self.get_with_query("/orders/historical/batch", query).await
    }

    /// Gets a specific order by ID.
    pub async fn get_order(&self, order_id: &str) -> Result<Value> {
        self.get(&format!("/orders/historical/{order_id}")).await
    }

    /// Gets fills (trade executions).
    pub async fn get_fills(&self, query: &str) -> Result<Value> {
        self.get_with_query("/orders/historical/fills", query).await
    }

    /// Gets fee transaction summary.
    pub async fn get_transaction_summary(&self) -> Result<Value> {
        self.get("/transaction_summary").await
    }

    /// Gets the CFM (Coinbase Financial Markets) futures balance summary.
    ///
    /// # References
    ///
    /// - <https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/perpetuals/get-fcm-balance-summary>
    pub async fn get_cfm_balance_summary(&self) -> Result<CfmBalanceSummaryResponse> {
        let json = self.get("/cfm/balance_summary").await?;
        serde_json::from_value(json).map_err(Error::Serde)
    }

    /// Gets all CFM futures positions for the account.
    ///
    /// # References
    ///
    /// - <https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/perpetuals/get-fcm-positions>
    pub async fn get_cfm_positions(&self) -> Result<CfmPositionsResponse> {
        let json = self.get("/cfm/positions").await?;
        serde_json::from_value(json).map_err(Error::Serde)
    }

    /// Gets a single CFM futures position by product ID.
    ///
    /// # References
    ///
    /// - <https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/perpetuals/get-fcm-position>
    pub async fn get_cfm_position(&self, product_id: &str) -> Result<CfmPositionResponse> {
        let json = self.get(&format!("/cfm/positions/{product_id}")).await?;
        serde_json::from_value(json).map_err(Error::Serde)
    }

    /// Fetches every account, following Coinbase's cursor pagination.
    ///
    /// Returns the deserialized [`Account`] vector. Domain callers compose
    /// this with [`parse_account_state`] to build a Nautilus [`AccountState`].
    pub async fn fetch_all_accounts(&self) -> Result<Vec<Account>> {
        let mut all = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let mut pairs: Vec<(&str, &str)> = vec![("limit", ACCOUNTS_PAGE_LIMIT)];
            if let Some(c) = cursor.as_deref().filter(|s| !s.is_empty()) {
                pairs.push(("cursor", c));
            }
            let query_str = encode_query(&pairs);

            let json = self.get_accounts_with_query(&query_str).await?;
            let response: AccountsResponse = serde_json::from_value(json).map_err(Error::Serde)?;

            all.extend(response.accounts);

            if !response.has_next || response.cursor.is_empty() {
                break;
            }
            cursor = Some(response.cursor);
        }

        Ok(all)
    }

    /// Fetches every order matching the query, following cursor pagination.
    ///
    /// Honors `OrderListQuery::client_order_id_filter` as a client-side
    /// filter applied to each page (the venue endpoint does not accept that
    /// parameter directly). Stops once the configured `limit` is reached.
    pub async fn fetch_all_orders(&self, query: &OrderListQuery) -> Result<Vec<Order>> {
        let mut collected: Vec<Order> = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let start_str = query.start.map(|s| s.to_rfc3339());
            let end_str = query.end.map(|e| e.to_rfc3339());
            let limit_str = query.limit.map(|l| l.to_string());

            let mut pairs: Vec<(&str, &str)> = Vec::new();

            // Coinbase accepts `product_ids` as a repeated array parameter on
            // `/orders/historical/batch`; the singular form is silently ignored.
            if let Some(pid) = query.product_id.as_deref() {
                pairs.push(("product_ids", pid));
            }

            if query.open_only {
                pairs.push(("order_status", ORDER_STATUS_OPEN));
            }

            if let Some(s) = start_str.as_deref() {
                pairs.push(("start_date", s));
            }

            if let Some(e) = end_str.as_deref() {
                pairs.push(("end_date", e));
            }

            if let Some(l) = limit_str.as_deref() {
                pairs.push(("limit", l));
            }

            if let Some(c) = cursor.as_deref().filter(|s| !s.is_empty()) {
                pairs.push(("cursor", c));
            }

            let query_str = encode_query(&pairs);
            let json = self.get_orders(&query_str).await?;
            let response: OrdersListResponse =
                serde_json::from_value(json).map_err(Error::Serde)?;

            for order in response.orders {
                if let Some(cid) = query.client_order_id_filter.as_deref()
                    && order.client_order_id != cid
                {
                    continue;
                }
                collected.push(order);
            }

            if let Some(limit) = query.limit
                && collected.len() >= limit as usize
            {
                collected.truncate(limit as usize);
                break;
            }

            if !response.has_next || response.cursor.is_empty() {
                break;
            }
            cursor = Some(response.cursor);
        }

        Ok(collected)
    }

    /// Fetches every fill matching the query, following cursor pagination.
    pub async fn fetch_all_fills(&self, query: &FillListQuery) -> Result<Vec<Fill>> {
        let mut collected: Vec<Fill> = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let start_str = query.start.map(|s| s.to_rfc3339());
            let end_str = query.end.map(|e| e.to_rfc3339());
            let limit_str = query.limit.map(|l| l.to_string());

            let mut pairs: Vec<(&str, &str)> = Vec::new();

            // `/orders/historical/fills` takes repeated array filters for
            // product and order IDs. Singular keys are accepted by the server
            // but silently ignored, which would scan the full fill history.
            if let Some(pid) = query.product_id.as_deref() {
                pairs.push(("product_ids", pid));
            }

            if let Some(vid) = query.venue_order_id.as_deref() {
                pairs.push(("order_ids", vid));
            }

            if let Some(s) = start_str.as_deref() {
                pairs.push(("start_sequence_timestamp", s));
            }

            if let Some(e) = end_str.as_deref() {
                pairs.push(("end_sequence_timestamp", e));
            }

            if let Some(l) = limit_str.as_deref() {
                pairs.push(("limit", l));
            }

            if let Some(c) = cursor.as_deref().filter(|s| !s.is_empty()) {
                pairs.push(("cursor", c));
            }

            let query_str = encode_query(&pairs);
            let json = self.get_fills(&query_str).await?;
            let response: FillsResponse = serde_json::from_value(json).map_err(Error::Serde)?;

            collected.extend(response.fills);

            if let Some(limit) = query.limit
                && collected.len() >= limit as usize
            {
                collected.truncate(limit as usize);
                break;
            }

            if response.cursor.is_empty() {
                break;
            }
            cursor = Some(response.cursor);
        }

        Ok(collected)
    }

    /// Creates a new order via `POST /orders`.
    ///
    /// # References
    ///
    /// - <https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/orders/create-order>
    pub async fn create_order(&self, request: &CreateOrderRequest) -> Result<CreateOrderResponse> {
        let body = serde_json::to_value(request).map_err(Error::Serde)?;
        let json = self.post("/orders", &body).await?;
        serde_json::from_value(json).map_err(Error::Serde)
    }

    /// Cancels one or more orders via `POST /orders/batch_cancel`.
    ///
    /// # References
    ///
    /// - <https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/orders/cancel-order>
    pub async fn cancel_orders(
        &self,
        request: &CancelOrdersRequest,
    ) -> Result<CancelOrdersResponse> {
        let body = serde_json::to_value(request).map_err(Error::Serde)?;
        let json = self.post("/orders/batch_cancel", &body).await?;
        serde_json::from_value(json).map_err(Error::Serde)
    }

    /// Edits an existing order via `POST /orders/edit`.
    ///
    /// Coinbase restricts edits to GTC orders (LIMIT, STOP_LIMIT, Bracket);
    /// other order types require cancel-and-replace.
    ///
    /// # References
    ///
    /// - <https://docs.cdp.coinbase.com/api-reference/advanced-trade-api/rest-api/orders/edit-order>
    pub async fn edit_order(&self, request: &EditOrderRequest) -> Result<EditOrderResponse> {
        let body = serde_json::to_value(request).map_err(Error::Serde)?;
        let json = self.post("/orders/edit", &body).await?;
        serde_json::from_value(json).map_err(Error::Serde)
    }
}

/// Provides a domain-level HTTP client for the Coinbase Advanced Trade API.
///
/// Wraps [`CoinbaseRawHttpClient`] in an `Arc` and adds instrument caching
/// and Nautilus type conversions. This is the primary HTTP interface for the
/// data and execution clients.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.coinbase", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.coinbase")
)]
pub struct CoinbaseHttpClient {
    pub(crate) inner: Arc<CoinbaseRawHttpClient>,
    clock: &'static AtomicTime,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    /// Maps a product ID to its Coinbase-canonical alias (e.g. `BTC-USDC -> BTC-USD`).
    /// Coinbase consolidates aliased pairs into a single book server-side, so the
    /// WebSocket feed and user-channel echo the canonical id even when callers
    /// subscribed or submitted with the alias.
    product_aliases: Arc<AtomicMap<Ustr, Ustr>>,
}

impl Default for CoinbaseHttpClient {
    fn default() -> Self {
        Self::new(CoinbaseEnvironment::Live, 10, None, None)
            .expect("Failed to create default Coinbase HTTP client")
    }
}

impl CoinbaseHttpClient {
    /// Creates a new [`CoinbaseHttpClient`] for public endpoints only.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn new(
        environment: CoinbaseEnvironment,
        timeout_secs: u64,
        proxy_url: Option<String>,
        retry_config: Option<RetryConfig>,
    ) -> std::result::Result<Self, HttpClientError> {
        let raw = CoinbaseRawHttpClient::new(environment, timeout_secs, proxy_url, retry_config)?;
        Ok(Self::from_raw(raw))
    }

    /// Creates a new [`CoinbaseHttpClient`] with credentials for authenticated requests.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn with_credentials(
        credential: CoinbaseCredential,
        environment: CoinbaseEnvironment,
        timeout_secs: u64,
        proxy_url: Option<String>,
        retry_config: Option<RetryConfig>,
    ) -> std::result::Result<Self, HttpClientError> {
        let raw = CoinbaseRawHttpClient::with_credentials(
            credential,
            environment,
            timeout_secs,
            proxy_url,
            retry_config,
        )?;
        Ok(Self::from_raw(raw))
    }

    /// Creates an authenticated client from environment variables.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Auth`] if required environment variables are not set.
    pub fn from_env(environment: CoinbaseEnvironment) -> Result<Self> {
        let raw = CoinbaseRawHttpClient::from_env(environment)?;
        Ok(Self::from_raw(raw))
    }

    /// Creates a new [`CoinbaseHttpClient`] with explicit credentials.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Auth`] if credentials are invalid.
    pub fn from_credentials(
        api_key: &str,
        api_secret: &str,
        environment: CoinbaseEnvironment,
        timeout_secs: u64,
        proxy_url: Option<String>,
        retry_config: Option<RetryConfig>,
    ) -> Result<Self> {
        let raw = CoinbaseRawHttpClient::from_credentials(
            api_key,
            api_secret,
            environment,
            timeout_secs,
            proxy_url,
            retry_config,
        )?;
        Ok(Self::from_raw(raw))
    }

    /// Returns the cancellation token shared by in-flight requests.
    #[must_use]
    pub fn cancellation_token(&self) -> &CancellationToken {
        self.inner.cancellation_token()
    }

    fn from_raw(raw: CoinbaseRawHttpClient) -> Self {
        Self {
            inner: Arc::new(raw),
            clock: get_atomic_clock_realtime(),
            instruments: Arc::new(AtomicMap::new()),
            product_aliases: Arc::new(AtomicMap::new()),
        }
    }

    /// Overrides the base REST URL (for testing with mock servers).
    ///
    /// Safe to call regardless of how many clones share the inner client.
    pub fn set_base_url(&self, url: String) {
        self.inner.set_base_url(url);
    }

    /// Returns the configured environment.
    #[must_use]
    pub fn environment(&self) -> CoinbaseEnvironment {
        self.inner.environment()
    }

    /// Returns true if this client has credentials for authenticated requests.
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.inner.is_authenticated()
    }

    /// Returns a reference to the instrument cache.
    #[must_use]
    pub fn instruments(&self) -> &Arc<AtomicMap<InstrumentId, InstrumentAny>> {
        &self.instruments
    }

    /// Returns a reference to the product alias map (`product_id -> canonical product_id`).
    #[must_use]
    pub fn product_aliases(&self) -> &Arc<AtomicMap<Ustr, Ustr>> {
        &self.product_aliases
    }

    /// Returns the current timestamp from the atomic clock.
    #[must_use]
    pub fn ts_now(&self) -> UnixNanos {
        self.clock.get_time_ns()
    }

    /// Gets all available products.
    pub async fn get_products(&self) -> Result<Value> {
        self.inner.get_products().await
    }

    /// Gets a specific product by ID.
    pub async fn get_product(&self, product_id: &str) -> Result<Value> {
        self.inner.get_product(product_id).await
    }

    /// Gets candles for a product.
    pub async fn get_candles(
        &self,
        product_id: &str,
        start: &str,
        end: &str,
        granularity: &str,
    ) -> Result<Value> {
        self.inner
            .get_candles(product_id, start, end, granularity)
            .await
    }

    /// Gets market trades for a product.
    pub async fn get_market_trades(&self, product_id: &str, limit: u32) -> Result<Value> {
        self.inner.get_market_trades(product_id, limit).await
    }

    /// Gets best bid/ask for one or more products.
    pub async fn get_best_bid_ask(&self, product_ids: &[&str]) -> Result<Value> {
        self.inner.get_best_bid_ask(product_ids).await
    }

    /// Gets the product order book.
    pub async fn get_product_book(&self, product_id: &str, limit: Option<u32>) -> Result<Value> {
        self.inner.get_product_book(product_id, limit).await
    }

    /// Gets all accounts.
    pub async fn get_accounts(&self) -> Result<Value> {
        self.inner.get_accounts().await
    }

    /// Gets a specific account by UUID.
    pub async fn get_account(&self, account_id: &str) -> Result<Value> {
        self.inner.get_account(account_id).await
    }

    /// Lists all portfolios visible to the authenticated key.
    pub async fn get_portfolios(&self) -> Result<Value> {
        self.inner.get_portfolios().await
    }

    /// Validates an order payload against the venue without submitting it.
    ///
    /// Useful for diagnosing `account is not available` and similar errors
    /// because it returns the same error envelope as `POST /orders`.
    pub async fn preview_order(&self, body: &Value) -> Result<Value> {
        self.inner.post("/orders/preview", body).await
    }

    /// Gets historical orders.
    pub async fn get_orders(&self, query: &str) -> Result<Value> {
        self.inner.get_orders(query).await
    }

    /// Gets a specific order by ID.
    pub async fn get_order(&self, order_id: &str) -> Result<Value> {
        self.inner.get_order(order_id).await
    }

    /// Gets fills (trade executions).
    pub async fn get_fills(&self, query: &str) -> Result<Value> {
        self.inner.get_fills(query).await
    }

    /// Gets fee transaction summary.
    pub async fn get_transaction_summary(&self) -> Result<Value> {
        self.inner.get_transaction_summary().await
    }

    /// Requests all instruments from Coinbase, optionally filtered by product type.
    ///
    /// Parses each supported product into a Nautilus [`InstrumentAny`] and caches
    /// the results in the shared instrument map. Unsupported products (non-crypto
    /// futures, `UNKNOWN` product types) are skipped with a debug log.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn request_instruments(
        &self,
        product_type: Option<CoinbaseProductType>,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let json = self
            .inner
            .get_products()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch products: {e}"))?;
        let response: ProductsResponse =
            serde_json::from_value(json).map_err(|e| anyhow::anyhow!(e))?;

        let ts_init = self.ts_now();
        let mut instruments = Vec::with_capacity(response.products.len());

        for product in &response.products {
            if let Some(filter) = product_type
                && product.product_type != filter
            {
                continue;
            }

            match parse_instrument(product, ts_init) {
                Ok(instrument) => instruments.push(instrument),
                Err(e) => {
                    log::debug!(
                        "Skipping product '{}' during parse: {e}",
                        product.product_id
                    );
                }
            }
        }

        self.cache_instruments(&instruments);
        self.record_product_aliases(&response.products);
        Ok(instruments)
    }

    /// Requests a single instrument by product ID.
    ///
    /// Caches the result on success.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP request fails, deserialization fails,
    /// or the product cannot be parsed into a supported instrument.
    pub async fn request_instrument(&self, product_id: &str) -> anyhow::Result<InstrumentAny> {
        let json = self
            .inner
            .get_product(product_id)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch product '{product_id}': {e}"))?;
        let product: crate::http::models::Product =
            serde_json::from_value(json).map_err(|e| anyhow::anyhow!(e))?;
        let ts_init = self.ts_now();
        let instrument = parse_instrument(&product, ts_init)?;
        self.cache_instrument(&instrument);
        self.record_product_aliases(std::slice::from_ref(&product));
        Ok(instrument)
    }

    /// Requests the raw product payload for a product ID.
    ///
    /// Returns the full [`crate::http::models::Product`] so callers can read
    /// derivatives-specific fields (`future_product_details.index_price`,
    /// `funding_rate`, `funding_time`) that are stripped when parsing to a
    /// Nautilus instrument.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP request fails or the response cannot
    /// be deserialized.
    pub async fn request_raw_product(
        &self,
        product_id: &str,
    ) -> anyhow::Result<crate::http::models::Product> {
        let json = self
            .inner
            .get_product(product_id)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch product '{product_id}': {e}"))?;
        serde_json::from_value(json).map_err(|e| anyhow::anyhow!(e))
    }

    /// Requests the current account state.
    ///
    /// Builds a cash-type [`AccountState`] from `/accounts` with one balance
    /// per currency. Follows Coinbase's cursor pagination so multi-wallet
    /// accounts are reported in full. `reported` is set to `true` since the
    /// values come from the venue.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP request fails or the response cannot
    /// be parsed.
    pub async fn request_account_state(
        &self,
        account_id: AccountId,
    ) -> anyhow::Result<AccountState> {
        let accounts = self
            .inner
            .fetch_all_accounts()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch accounts: {e}"))?;
        let ts_event = self.ts_now();
        parse_account_state(&accounts, account_id, true, ts_event, ts_event)
    }

    /// Requests a single order status report by venue or client order ID.
    ///
    /// Resolves venue order IDs first via `/orders/historical/{id}`. When only a
    /// `client_order_id` is provided, paginates the order history filtered to
    /// that client ID.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP request fails, the order cannot be found,
    /// or the response cannot be parsed.
    pub async fn request_order_status_report(
        &self,
        account_id: AccountId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> anyhow::Result<OrderStatusReport> {
        let venue_order_id = match (venue_order_id, client_order_id) {
            (Some(vid), _) => vid,
            (None, Some(cid)) => {
                // Fall back to batched query when only the client order ID is known
                let query = OrderListQuery {
                    client_order_id_filter: Some(cid.as_str().to_string()),
                    ..Default::default()
                };
                let orders = self
                    .inner
                    .fetch_all_orders(&query)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to fetch orders: {e}"))?;
                let order = orders
                    .into_iter()
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("No order found for client_order_id={cid}"))?;
                let instrument = self.get_or_fetch_instrument(order.product_id).await?;
                let ts_init = self.ts_now();
                return parse_order_status_report(&order, &instrument, account_id, ts_init);
            }
            (None, None) => {
                anyhow::bail!("Either client_order_id or venue_order_id is required")
            }
        };

        let json = self
            .inner
            .get_order(venue_order_id.as_str())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch order: {e}"))?;
        let response: OrderResponse =
            serde_json::from_value(json).map_err(|e| anyhow::anyhow!(e))?;
        let instrument = self
            .get_or_fetch_instrument(response.order.product_id)
            .await?;
        let ts_init = self.ts_now();
        parse_order_status_report(&response.order, &instrument, account_id, ts_init)
    }

    /// Requests order status reports, optionally filtered by instrument, open
    /// status, and time window.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP request fails or when any response cannot
    /// be deserialized.
    pub async fn request_order_status_reports(
        &self,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
        open_only: bool,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let query = OrderListQuery {
            product_id: instrument_id.map(|id| id.symbol.as_str().to_string()),
            open_only,
            start,
            end,
            limit,
            client_order_id_filter: None,
        };

        let orders = self
            .inner
            .fetch_all_orders(&query)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch orders: {e}"))?;

        let ts_init = self.ts_now();
        let mut reports = Vec::with_capacity(orders.len());

        for order in &orders {
            let instrument = match self.get_or_fetch_instrument(order.product_id).await {
                Ok(inst) => inst,
                Err(e) => {
                    log::debug!("Skipping order {}: {e}", order.order_id);
                    continue;
                }
            };

            match parse_order_status_report(order, &instrument, account_id, ts_init) {
                Ok(report) => reports.push(report),
                Err(e) => log::warn!("Failed to parse order {}: {e}", order.order_id),
            }
        }

        Ok(reports)
    }

    /// Requests fill reports, optionally filtered by instrument, venue order ID,
    /// and time window.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn request_fill_reports(
        &self,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
        venue_order_id: Option<VenueOrderId>,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> anyhow::Result<Vec<FillReport>> {
        let query = FillListQuery {
            product_id: instrument_id.map(|id| id.symbol.as_str().to_string()),
            venue_order_id: venue_order_id.map(|id| id.as_str().to_string()),
            start,
            end,
            limit,
        };

        let fills = self
            .inner
            .fetch_all_fills(&query)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch fills: {e}"))?;

        let ts_init = self.ts_now();
        let mut reports = Vec::with_capacity(fills.len());

        for fill in &fills {
            let instrument = match self.get_or_fetch_instrument(fill.product_id).await {
                Ok(inst) => inst,
                Err(e) => {
                    log::debug!("Skipping fill {}: {e}", fill.trade_id);
                    continue;
                }
            };

            match parse_fill_report(fill, &instrument, account_id, ts_init) {
                Ok(report) => reports.push(report),
                Err(e) => log::warn!("Failed to parse fill {}: {e}", fill.trade_id),
            }
        }

        Ok(reports)
    }

    /// Caches an instrument in the shared instrument map.
    pub fn cache_instrument(&self, instrument: &InstrumentAny) {
        self.instruments.rcu(|m| {
            m.insert(instrument.id(), instrument.clone());
        });
    }

    /// Caches a batch of instruments in the shared instrument map.
    pub fn cache_instruments(&self, instruments: &[InstrumentAny]) {
        self.instruments.rcu(|m| {
            for instrument in instruments {
                m.insert(instrument.id(), instrument.clone());
            }
        });
    }

    /// Records `product_id -> alias` entries for any product whose `alias`
    /// field is non-empty. Coinbase aliases pairs to a canonical id (e.g.
    /// `BTC-USDC -> BTC-USD`) that the WebSocket and user channel use on the
    /// wire even when callers operate on the alias side.
    pub fn record_product_aliases(&self, products: &[crate::http::models::Product]) {
        let aliased: Vec<(Ustr, Ustr)> = products
            .iter()
            .filter(|p| !p.alias.is_empty())
            .map(|p| (p.product_id, p.alias))
            .collect();

        if aliased.is_empty() {
            return;
        }

        self.product_aliases.rcu(|m| {
            for (product_id, alias) in &aliased {
                m.insert(*product_id, *alias);
            }
        });
    }

    // Returns the cached instrument for a product ID, fetching it on miss.
    // Order and fill reconciliation calls parse hundreds of historical
    // records and each one needs precision metadata. Rather than forcing
    // callers to bootstrap the full instrument universe first, this lazy
    // path fetches any missing product via `/products/{id}` and caches it.
    async fn get_or_fetch_instrument(&self, product_id: Ustr) -> anyhow::Result<InstrumentAny> {
        let instrument_id = InstrumentId::new(
            Symbol::new(product_id),
            *crate::common::consts::COINBASE_VENUE,
        );

        if let Some(instrument) = self.instruments.get_cloned(&instrument_id) {
            return Ok(instrument);
        }
        // Cache miss: fetch and cache the single product. Any parse error
        // (unsupported product type, missing fields) surfaces to the caller so
        // the offending record can be skipped with a log.
        self.request_instrument(product_id.as_str()).await
    }

    /// Submits a new order built from Nautilus domain types.
    ///
    /// Maps the order side, order type, and time-in-force to Coinbase's
    /// `order_configuration` shape and posts to `/orders`. Returns the
    /// venue's create-order response; callers inspect `success` and the
    /// success/error response variants.
    ///
    /// # Errors
    ///
    /// Returns an error when the order parameters cannot be mapped to a
    /// supported Coinbase configuration, when the HTTP request fails, or
    /// when the response cannot be parsed.
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_order(
        &self,
        client_order_id: ClientOrderId,
        instrument_id: InstrumentId,
        side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: TimeInForce,
        price: Option<Price>,
        trigger_price: Option<Price>,
        expire_time: Option<UnixNanos>,
        post_only: bool,
        is_quote_quantity: bool,
        leverage: Option<Decimal>,
        margin_type: Option<CoinbaseMarginType>,
        reduce_only: bool,
        retail_portfolio_id: Option<String>,
    ) -> anyhow::Result<CreateOrderResponse> {
        let coinbase_side = map_order_side(side)?;
        let order_config = build_order_configuration(
            order_type,
            side,
            quantity,
            price,
            trigger_price,
            time_in_force,
            expire_time,
            post_only,
            is_quote_quantity,
            reduce_only,
        )?;

        let request = CreateOrderRequest {
            client_order_id: client_order_id.to_string(),
            product_id: instrument_id.symbol.inner(),
            side: coinbase_side,
            order_configuration: order_config,
            self_trade_prevention_id: None,
            leverage: leverage.map(|d| d.normalize().to_string()),
            margin_type,
            retail_portfolio_id,
            reduce_only,
        };

        self.inner
            .create_order(&request)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to submit order: {e}"))
    }

    /// Cancels one or more orders by venue order ID via batch_cancel.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP request fails or the response cannot
    /// be parsed.
    pub async fn cancel_orders(
        &self,
        venue_order_ids: &[VenueOrderId],
    ) -> anyhow::Result<CancelOrdersResponse> {
        let request = CancelOrdersRequest {
            order_ids: venue_order_ids
                .iter()
                .map(|id| id.as_str().to_string())
                .collect(),
        };
        self.inner
            .cancel_orders(&request)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to cancel orders: {e}"))
    }

    /// Fetches the CFM (futures) balance summary.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP request fails or the response cannot be
    /// deserialized.
    pub async fn request_cfm_balance_summary(&self) -> anyhow::Result<CfmBalanceSummary> {
        let response = self
            .inner
            .get_cfm_balance_summary()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch CFM balance summary: {e}"))?;
        Ok(response.balance_summary)
    }

    /// Fetches margin balances derived from the CFM balance summary.
    ///
    /// # Errors
    ///
    /// Returns an error when the summary cannot be fetched or when a balance
    /// cannot be constructed.
    pub async fn request_cfm_margin_balances(&self) -> anyhow::Result<Vec<MarginBalance>> {
        let summary = self.request_cfm_balance_summary().await?;
        parse_cfm_margin_balances(&summary)
    }

    /// Fetches a margin [`AccountState`] derived from the CFM balance summary.
    ///
    /// # Errors
    ///
    /// Returns an error when the summary cannot be fetched or when balances
    /// cannot be constructed.
    pub async fn request_cfm_account_state(
        &self,
        account_id: AccountId,
    ) -> anyhow::Result<AccountState> {
        let summary = self.request_cfm_balance_summary().await?;
        let ts_event = self.ts_now();
        parse_cfm_account_state(&summary, account_id, true, ts_event, ts_event)
    }

    /// Fetches all CFM futures positions and returns Nautilus position reports.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP request fails or a position cannot be
    /// parsed.
    pub async fn request_position_status_reports(
        &self,
        account_id: AccountId,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let response = self
            .inner
            .get_cfm_positions()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch CFM positions: {e}"))?;

        let ts_init = self.ts_now();
        let mut reports = Vec::with_capacity(response.positions.len());

        for position in &response.positions {
            let instrument = match self.get_or_fetch_instrument(position.product_id).await {
                Ok(inst) => inst,
                Err(e) => {
                    log::debug!("Skipping CFM position {}: {e}", position.product_id);
                    continue;
                }
            };

            match parse_cfm_position_status_report(position, &instrument, account_id, ts_init) {
                Ok(report) => reports.push(report),
                Err(e) => log::warn!("Failed to parse CFM position {}: {e}", position.product_id),
            }
        }

        Ok(reports)
    }

    /// Fetches a single CFM futures position and returns a position status
    /// report when the venue reports a non-flat position.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP request fails or the position cannot be
    /// parsed.
    pub async fn request_position_status_report(
        &self,
        account_id: AccountId,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<Option<PositionStatusReport>> {
        let product_id = instrument_id.symbol.as_str();
        let response = self
            .inner
            .get_cfm_position(product_id)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to fetch CFM position '{product_id}': {e}"))?;

        let instrument = self
            .get_or_fetch_instrument(response.position.product_id)
            .await?;
        let ts_init = self.ts_now();
        let report =
            parse_cfm_position_status_report(&response.position, &instrument, account_id, ts_init)?;
        Ok(Some(report))
    }

    /// Modifies an existing GTC order's price, size, or stop price.
    ///
    /// Coinbase's `/orders/edit` endpoint is documented to accept edits on
    /// these fields for supported order configurations (primarily LIMIT
    /// GTC). At least one of `price`, `quantity`, or `trigger_price` must
    /// be supplied.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP request fails or the response cannot
    /// be deserialized.
    pub async fn modify_order(
        &self,
        venue_order_id: VenueOrderId,
        price: Option<Price>,
        quantity: Option<Quantity>,
        trigger_price: Option<Price>,
    ) -> anyhow::Result<EditOrderResponse> {
        let request = EditOrderRequest {
            order_id: venue_order_id.as_str().to_string(),
            price: price.map(|p| p.to_string()),
            size: quantity.map(|q| q.to_string()),
            stop_price: trigger_price.map(|p| p.to_string()),
        };
        self.inner
            .edit_order(&request)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to edit order: {e}"))
    }
}

/// Maps a Nautilus [`OrderSide`] to Coinbase's wire enum.
///
/// # Errors
///
/// Returns an error when the side is [`OrderSide::NoOrderSide`].
pub fn map_order_side(side: OrderSide) -> anyhow::Result<CoinbaseOrderSide> {
    match side {
        OrderSide::Buy => Ok(CoinbaseOrderSide::Buy),
        OrderSide::Sell => Ok(CoinbaseOrderSide::Sell),
        OrderSide::NoOrderSide => anyhow::bail!("NoOrderSide is not a valid Coinbase side"),
    }
}

/// Builds the Coinbase [`OrderConfiguration`] payload from Nautilus order
/// parameters.
///
/// Caller supplies the order type, side, quantity, optional price/trigger,
/// time-in-force, optional expire time (required for GTD), `post_only`
/// flag, and whether the quantity is denominated in the quote currency
/// (only meaningful for MARKET orders).
///
/// # Errors
///
/// Returns an error when the requested combination is not supported by
/// Coinbase (e.g. STOP_MARKET, IOC LIMIT, missing required field).
#[allow(clippy::too_many_arguments)]
pub fn build_order_configuration(
    order_type: OrderType,
    side: OrderSide,
    quantity: Quantity,
    price: Option<Price>,
    trigger_price: Option<Price>,
    time_in_force: TimeInForce,
    expire_time: Option<UnixNanos>,
    post_only: bool,
    is_quote_quantity: bool,
    reduce_only: bool,
) -> anyhow::Result<OrderConfiguration> {
    let qty = quantity.as_decimal();
    let price = price.map(|p| p.as_decimal());
    let trigger = trigger_price.map(|p| p.as_decimal());

    if reduce_only && matches!(order_type, OrderType::Market) {
        log::debug!("Coinbase MARKET orders do not accept reduce_only; ignoring flag");
    }

    match order_type {
        OrderType::Market => {
            // Coinbase exposes `market_market_ioc` and `market_market_fok` for
            // MARKET orders. Nautilus' default GTC is mapped to IOC (mirroring
            // the Bybit adapter pattern); explicit IOC and FOK are honoured;
            // DAY / GTD are rejected.
            //
            // Note: a MARKET order built with TIF=GTC will execute as IOC at
            // Coinbase. Backtest replays of the same order through the
            // matching engine treat it differently. Strategies that need
            // strict backtest/live parity should construct MarketOrders with
            // TIF=IOC or TIF=FOK explicitly.
            let params = if is_quote_quantity {
                MarketParams {
                    quote_size: Some(qty),
                    base_size: None,
                }
            } else {
                MarketParams {
                    quote_size: None,
                    base_size: Some(qty),
                }
            };

            match time_in_force {
                TimeInForce::Ioc | TimeInForce::Gtc => {
                    Ok(OrderConfiguration::MarketIoc(MarketIoc {
                        market_market_ioc: params,
                    }))
                }
                TimeInForce::Fok => Ok(OrderConfiguration::MarketFok(MarketFok {
                    market_market_fok: params,
                })),
                _ => {
                    anyhow::bail!(
                        "Unsupported TIF {time_in_force} for MARKET on Coinbase (use IOC or FOK)"
                    )
                }
            }
        }
        OrderType::Limit => {
            let limit_price =
                price.ok_or_else(|| anyhow::anyhow!("LIMIT order requires a price"))?;

            match time_in_force {
                TimeInForce::Gtc => Ok(OrderConfiguration::LimitGtc(LimitGtc {
                    limit_limit_gtc: LimitGtcParams {
                        base_size: qty,
                        limit_price,
                        post_only,
                    },
                })),
                TimeInForce::Gtd => {
                    let expire = expire_time
                        .ok_or_else(|| anyhow::anyhow!("GTD LIMIT requires expire_time"))?;
                    Ok(OrderConfiguration::LimitGtd(LimitGtd {
                        limit_limit_gtd: LimitGtdParams {
                            base_size: qty,
                            limit_price,
                            end_time: format_rfc3339_from_nanos(expire)?,
                            post_only,
                        },
                    }))
                }
                TimeInForce::Fok => Ok(OrderConfiguration::LimitFok(LimitFok {
                    limit_limit_fok: LimitFokParams {
                        base_size: qty,
                        limit_price,
                    },
                })),
                _ => anyhow::bail!("Unsupported TIF {time_in_force} for LIMIT on Coinbase"),
            }
        }
        OrderType::StopLimit => {
            let limit_price =
                price.ok_or_else(|| anyhow::anyhow!("STOP_LIMIT order requires a price"))?;
            let stop_price = trigger
                .ok_or_else(|| anyhow::anyhow!("STOP_LIMIT order requires trigger_price"))?;
            let direction = match side {
                OrderSide::Buy => CoinbaseStopDirection::StopUp,
                OrderSide::Sell => CoinbaseStopDirection::StopDown,
                OrderSide::NoOrderSide => {
                    anyhow::bail!("STOP_LIMIT requires a defined side")
                }
            };

            match time_in_force {
                TimeInForce::Gtc => Ok(OrderConfiguration::StopLimitGtc(StopLimitGtc {
                    stop_limit_stop_limit_gtc: StopLimitGtcParams {
                        base_size: qty,
                        limit_price,
                        stop_price,
                        stop_direction: direction,
                    },
                })),
                TimeInForce::Gtd => {
                    let expire = expire_time
                        .ok_or_else(|| anyhow::anyhow!("GTD STOP_LIMIT requires expire_time"))?;
                    Ok(OrderConfiguration::StopLimitGtd(StopLimitGtd {
                        stop_limit_stop_limit_gtd: StopLimitGtdParams {
                            base_size: qty,
                            limit_price,
                            stop_price,
                            stop_direction: direction,
                            end_time: format_rfc3339_from_nanos(expire)?,
                        },
                    }))
                }
                _ => anyhow::bail!("Unsupported TIF {time_in_force} for STOP_LIMIT on Coinbase"),
            }
        }
        other => anyhow::bail!("Unsupported order type for Coinbase: {other}"),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_raw_client_construction_live() {
        let client = CoinbaseRawHttpClient::new(CoinbaseEnvironment::Live, 10, None, None).unwrap();
        assert_eq!(client.environment(), CoinbaseEnvironment::Live);
        assert!(!client.is_authenticated());
    }

    #[rstest]
    fn test_raw_client_construction_sandbox() {
        let client =
            CoinbaseRawHttpClient::new(CoinbaseEnvironment::Sandbox, 10, None, None).unwrap();
        assert_eq!(client.environment(), CoinbaseEnvironment::Sandbox);
    }

    #[rstest]
    fn test_raw_build_url() {
        let client = CoinbaseRawHttpClient::new(CoinbaseEnvironment::Live, 10, None, None).unwrap();
        let url = client.build_url("/products");
        assert_eq!(url, "https://api.coinbase.com/api/v3/brokerage/products");
    }

    #[rstest]
    fn test_raw_build_jwt_uri_live() {
        let client = CoinbaseRawHttpClient::new(CoinbaseEnvironment::Live, 10, None, None).unwrap();
        let uri = client.build_jwt_uri("GET", "/accounts");
        assert_eq!(uri, "GET api.coinbase.com/api/v3/brokerage/accounts");
    }

    #[rstest]
    fn test_raw_build_jwt_uri_sandbox() {
        let client =
            CoinbaseRawHttpClient::new(CoinbaseEnvironment::Sandbox, 10, None, None).unwrap();
        let uri = client.build_jwt_uri("GET", "/accounts");
        assert_eq!(
            uri,
            "GET api-sandbox.coinbase.com/api/v3/brokerage/accounts"
        );
    }

    #[rstest]
    fn test_raw_build_jwt_uri_custom_base_url() {
        let client = CoinbaseRawHttpClient::new(CoinbaseEnvironment::Live, 10, None, None).unwrap();
        client.set_base_url("http://localhost:8080".to_string());
        let uri = client.build_jwt_uri("POST", "/orders");
        assert_eq!(uri, "POST localhost:8080/api/v3/brokerage/orders");
    }

    #[rstest]
    fn test_raw_set_base_url_safe_after_clone_via_arc() {
        let raw = Arc::new(
            CoinbaseRawHttpClient::new(CoinbaseEnvironment::Live, 10, None, None).unwrap(),
        );
        let other = Arc::clone(&raw);
        // Mutating after a clone must not panic; readers see the new value
        raw.set_base_url("http://localhost:1234".to_string());
        assert!(other.build_url("/foo").starts_with("http://localhost:1234"));
    }

    #[rstest]
    fn test_raw_auth_headers_without_credentials() {
        let client = CoinbaseRawHttpClient::new(CoinbaseEnvironment::Live, 10, None, None).unwrap();
        let result = client.auth_headers("GET", "/accounts");
        assert!(result.is_err());
        assert!(result.unwrap_err().is_auth_error());
    }

    #[rstest]
    fn test_domain_client_construction() {
        let client = CoinbaseHttpClient::new(CoinbaseEnvironment::Live, 10, None, None).unwrap();
        assert_eq!(client.environment(), CoinbaseEnvironment::Live);
        assert!(!client.is_authenticated());
    }

    #[rstest]
    fn test_domain_client_default() {
        let client = CoinbaseHttpClient::default();
        assert_eq!(client.environment(), CoinbaseEnvironment::Live);
    }

    #[rstest]
    fn test_domain_client_instruments_cache_empty() {
        let client = CoinbaseHttpClient::default();
        assert!(client.instruments().is_empty());
    }

    #[rstest]
    fn test_domain_client_set_base_url() {
        let client = CoinbaseHttpClient::new(CoinbaseEnvironment::Live, 10, None, None).unwrap();
        let cloned = client.clone();
        // Mutating after a clone must not panic; both clones observe the change
        client.set_base_url("http://localhost:9090".to_string());
        let url = cloned.inner.build_url("/test");
        assert!(url.starts_with("http://localhost:9090"));
    }

    #[rstest]
    fn test_encode_query_escapes_rfc3339_timestamps() {
        let query = encode_query(&[("start_date", "2024-01-15T10:00:00+00:00")]);
        // `+` must be escaped so the server does not read it as a space.
        assert_eq!(query, "start_date=2024-01-15T10%3A00%3A00%2B00%3A00");
    }

    #[rstest]
    fn test_encode_query_escapes_opaque_cursor() {
        let query = encode_query(&[("cursor", "a/b+c=?&x")]);
        // Reserved characters in an opaque cursor must not leak into the query structure.
        assert!(!query.contains("a/b+c=?&x"));
        assert!(query.starts_with("cursor="));
    }

    #[rstest]
    fn test_encode_query_joins_pairs_with_ampersand() {
        let query = encode_query(&[("product_id", "BTC-USD"), ("limit", "50")]);
        assert_eq!(query, "product_id=BTC-USD&limit=50");
    }

    #[rstest]
    fn test_map_order_side_rejects_no_side() {
        assert!(matches!(
            map_order_side(OrderSide::Buy).unwrap(),
            CoinbaseOrderSide::Buy
        ));
        assert!(matches!(
            map_order_side(OrderSide::Sell).unwrap(),
            CoinbaseOrderSide::Sell
        ));
        assert!(map_order_side(OrderSide::NoOrderSide).is_err());
    }

    #[rstest]
    fn test_build_order_configuration_market_base_size() {
        let cfg = build_order_configuration(
            OrderType::Market,
            OrderSide::Buy,
            Quantity::from("1.5"),
            None,
            None,
            TimeInForce::Ioc,
            None,
            false,
            false,
            false,
        )
        .unwrap();

        match cfg {
            OrderConfiguration::MarketIoc(m) => {
                assert!(m.market_market_ioc.base_size.is_some());
                assert!(m.market_market_ioc.quote_size.is_none());
            }
            other => panic!("expected MarketIoc, was {other:?}"),
        }
    }

    #[rstest]
    fn test_build_order_configuration_market_quote_size() {
        let cfg = build_order_configuration(
            OrderType::Market,
            OrderSide::Buy,
            Quantity::from("100"),
            None,
            None,
            TimeInForce::Ioc,
            None,
            false,
            true, // is_quote_quantity
            false,
        )
        .unwrap();

        match cfg {
            OrderConfiguration::MarketIoc(m) => {
                assert!(m.market_market_ioc.quote_size.is_some());
                assert!(m.market_market_ioc.base_size.is_none());
            }
            other => panic!("expected MarketIoc, was {other:?}"),
        }
    }

    #[rstest]
    fn test_build_order_configuration_market_fok() {
        let cfg = build_order_configuration(
            OrderType::Market,
            OrderSide::Buy,
            Quantity::from("0.5"),
            None,
            None,
            TimeInForce::Fok,
            None,
            false,
            false,
            false,
        )
        .unwrap();

        match cfg {
            OrderConfiguration::MarketFok(m) => {
                assert!(m.market_market_fok.base_size.is_some());
                assert!(m.market_market_fok.quote_size.is_none());
            }
            other => panic!("expected MarketFok, was {other:?}"),
        }
    }

    #[rstest]
    #[case(TimeInForce::Day)]
    #[case(TimeInForce::Gtd)]
    fn test_build_order_configuration_market_rejects_unsupported_tif(#[case] tif: TimeInForce) {
        let result = build_order_configuration(
            OrderType::Market,
            OrderSide::Buy,
            Quantity::from("1"),
            None,
            None,
            tif,
            None,
            false,
            false,
            false,
        );
        assert!(result.is_err());
    }

    #[rstest]
    fn test_build_order_configuration_limit_gtc_post_only() {
        let cfg = build_order_configuration(
            OrderType::Limit,
            OrderSide::Sell,
            Quantity::from("0.5"),
            Some(Price::from("50000.00")),
            None,
            TimeInForce::Gtc,
            None,
            true,
            false,
            false,
        )
        .unwrap();

        match cfg {
            OrderConfiguration::LimitGtc(l) => assert!(l.limit_limit_gtc.post_only),
            other => panic!("expected LimitGtc, was {other:?}"),
        }
    }

    #[rstest]
    fn test_build_order_configuration_limit_gtd_requires_expire_time() {
        let result = build_order_configuration(
            OrderType::Limit,
            OrderSide::Buy,
            Quantity::from("1"),
            Some(Price::from("100.00")),
            None,
            TimeInForce::Gtd,
            None,
            false,
            false,
            false,
        );
        assert!(result.is_err());
    }

    #[rstest]
    fn test_build_order_configuration_stop_limit_uses_correct_direction() {
        let buy_cfg = build_order_configuration(
            OrderType::StopLimit,
            OrderSide::Buy,
            Quantity::from("1"),
            Some(Price::from("100.00")),
            Some(Price::from("99.00")),
            TimeInForce::Gtc,
            None,
            false,
            false,
            false,
        )
        .unwrap();

        match buy_cfg {
            OrderConfiguration::StopLimitGtc(s) => assert_eq!(
                s.stop_limit_stop_limit_gtc.stop_direction,
                CoinbaseStopDirection::StopUp
            ),
            other => panic!("expected StopLimitGtc, was {other:?}"),
        }

        let sell_cfg = build_order_configuration(
            OrderType::StopLimit,
            OrderSide::Sell,
            Quantity::from("1"),
            Some(Price::from("100.00")),
            Some(Price::from("99.00")),
            TimeInForce::Gtc,
            None,
            false,
            false,
            false,
        )
        .unwrap();

        match sell_cfg {
            OrderConfiguration::StopLimitGtc(s) => assert_eq!(
                s.stop_limit_stop_limit_gtc.stop_direction,
                CoinbaseStopDirection::StopDown
            ),
            other => panic!("expected StopLimitGtc, was {other:?}"),
        }
    }

    #[rstest]
    fn test_build_order_configuration_market_accepts_default_gtc() {
        // Nautilus orders default to GTC; coerce to MARKET IOC silently for
        // the default case but not for any explicit non-IOC TIF.
        let cfg = build_order_configuration(
            OrderType::Market,
            OrderSide::Buy,
            Quantity::from("1"),
            None,
            None,
            TimeInForce::Gtc,
            None,
            false,
            false,
            false,
        )
        .unwrap();
        assert!(matches!(cfg, OrderConfiguration::MarketIoc(_)));
    }

    #[rstest]
    fn test_build_order_configuration_rejects_stop_market() {
        let result = build_order_configuration(
            OrderType::StopMarket,
            OrderSide::Buy,
            Quantity::from("1"),
            None,
            Some(Price::from("100.00")),
            TimeInForce::Gtc,
            None,
            false,
            false,
            false,
        );
        assert!(result.is_err());
    }

    #[rstest]
    fn test_rest_quota_matches_documented_limit() {
        assert_eq!(COINBASE_REST_QUOTA.burst_size().get(), 30);
    }

    #[rstest]
    fn test_default_retry_config_values() {
        let config = default_retry_config();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.initial_delay_ms, 100);
        assert_eq!(config.max_delay_ms, 5_000);
        assert_eq!(config.max_elapsed_ms, Some(180_000));
    }
}
