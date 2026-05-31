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

//! Raw and domain HTTP clients for Lighter REST endpoints.

use std::{
    collections::HashMap,
    num::NonZeroU32,
    sync::{Arc, LazyLock},
};

use chrono::{DateTime, Utc};
use nautilus_core::{
    AtomicTime, UnixNanos, consts::NAUTILUS_USER_AGENT, time::get_atomic_clock_realtime,
};
use nautilus_model::{
    data::{Bar, BarType, FundingRateUpdate, OrderBookDeltas, TradeTick},
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{
    http::{HttpClient, HttpResponse, Method, USER_AGENT},
    ratelimiter::quota::Quota,
    retry::{RetryManager, create_http_retry_manager},
};
use serde::{Serialize, de::DeserializeOwned};

use crate::{
    common::{
        enums::{
            LighterCandleResolution, LighterEnvironment, LighterFundingResolution,
            LighterMarketStatus,
        },
        symbol::MarketRegistry,
        urls::lighter_http_base_url,
    },
    http::{
        error::{
            LighterHttpError, LighterHttpResult, create_lighter_http_timeout_error,
            should_retry_lighter_http_error,
        },
        models::{
            LighterCandle, LighterCandles, LighterFundings, LighterNextNonce,
            LighterOrderBookDetails, LighterOrderBookOrders, LighterOrderBooks, LighterOrders,
            LighterResultCode, LighterSendTxBatchRequest, LighterSendTxBatchResponse,
            LighterSendTxRequest, LighterSendTxResponse, LighterTrade, LighterTrades,
        },
        parse::{
            parse_candle_bar, parse_funding_rate_update,
            parse_order_book_details_instruments_with_status, parse_order_book_snapshot,
            parse_trade_tick, register_order_books, register_perp_order_book_details,
            register_spot_order_book_details,
        },
        query::{
            LighterAccountActiveOrdersQuery, LighterAccountInactiveOrdersQuery,
            LighterCandlesQuery, LighterFundingsQuery, LighterNextNonceQuery,
            LighterOrderBookDetailsQuery, LighterOrderBookOrdersQuery, LighterOrderBooksQuery,
            LighterRecentTradesQuery, LighterTradesQuery,
        },
    },
};

const API_V1: &str = "/api/v1";
const ENDPOINT_ACCOUNT_ACTIVE_ORDERS: &str = "/api/v1/accountActiveOrders";
const ENDPOINT_ACCOUNT_INACTIVE_ORDERS: &str = "/api/v1/accountInactiveOrders";
const ENDPOINT_CANDLES: &str = "/api/v1/candles";
const ENDPOINT_FUNDINGS: &str = "/api/v1/fundings";
const ENDPOINT_NEXT_NONCE: &str = "/api/v1/nextNonce";
const ENDPOINT_ORDER_BOOK_DETAILS: &str = "/api/v1/orderBookDetails";
const ENDPOINT_ORDER_BOOK_ORDERS: &str = "/api/v1/orderBookOrders";
const ENDPOINT_ORDER_BOOKS: &str = "/api/v1/orderBooks";
const ENDPOINT_RECENT_TRADES: &str = "/api/v1/recentTrades";
const ENDPOINT_SEND_TX: &str = "/api/v1/sendTx";
const ENDPOINT_SEND_TX_BATCH: &str = "/api/v1/sendTxBatch";
const ENDPOINT_TRADES: &str = "/api/v1/trades";
const MULTIPART_BOUNDARY: &str = "nautilus-lighter-form-boundary";

/// Conservative Lighter REST rate limit for standard accounts.
///
/// Lighter documents 60 REST requests per rolling minute for standard accounts. Builder and
/// premium accounts can authenticate requests to get higher weighted limits.
pub static LIGHTER_REST_QUOTA: LazyLock<Quota> =
    LazyLock::new(|| Quota::per_minute(NonZeroU32::new(60).expect("non-zero")));

/// Maximum page size accepted by Lighter REST list endpoints (`/api/v1/trades`,
/// `/api/v1/accountInactiveOrders`). Values above this trigger `20001 invalid
/// param` from the venue, so reconciliation paginates at this cap and follows
/// `next_cursor` until the response is empty.
pub const LIGHTER_REST_PAGE_SIZE: u16 = 100;
pub const LIGHTER_CANDLES_MAX_LIMIT: u16 = 500;

const DEFAULT_BARS_LIMIT: usize = LIGHTER_CANDLES_MAX_LIMIT as usize;
const DEFAULT_FUNDING_RATES_LIMIT: usize = 100;
const MAX_BAR_REQUEST_PAGES: usize = 500;

trait LighterResponseCheck {
    fn response_code(&self) -> i32;
    fn response_message(&self) -> Option<&str>;
}

macro_rules! impl_lighter_response_check {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl LighterResponseCheck for $ty {
                fn response_code(&self) -> i32 {
                    self.code
                }

                fn response_message(&self) -> Option<&str> {
                    self.message.as_deref()
                }
            }
        )+
    };
}

impl_lighter_response_check!(
    LighterCandles,
    LighterFundings,
    LighterNextNonce,
    LighterOrderBookDetails,
    LighterOrderBookOrders,
    LighterOrderBooks,
    LighterOrders,
    LighterResultCode,
    LighterSendTxBatchResponse,
    LighterSendTxResponse,
    LighterTrades,
);

/// Raw HTTP client for Lighter REST API operations.
///
/// This client owns the transport, base URL, default headers, and rate limit. Methods map directly
/// to venue endpoints and return venue response models without converting to Nautilus domain types.
#[derive(Clone, Debug)]
pub struct LighterRawHttpClient {
    base_url: String,
    environment: LighterEnvironment,
    client: HttpClient,
    retry_manager: RetryManager<LighterHttpError>,
}

impl Default for LighterRawHttpClient {
    fn default() -> Self {
        Self::new(LighterEnvironment::Mainnet, None, 60, None)
            .expect("failed to create default Lighter raw HTTP client")
    }
}

impl LighterRawHttpClient {
    /// Creates a new [`LighterRawHttpClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be created.
    pub fn new(
        environment: LighterEnvironment,
        base_url: Option<String>,
        timeout_secs: u64,
        proxy_url: Option<String>,
    ) -> LighterHttpResult<Self> {
        let base_url = base_url
            .unwrap_or_else(|| lighter_http_base_url(environment).to_string())
            .trim_end_matches('/')
            .to_string();

        Ok(Self {
            base_url,
            environment,
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                vec![],
                Some(*LIGHTER_REST_QUOTA),
                Some(timeout_secs),
                proxy_url,
            )?,
            retry_manager: create_http_retry_manager(),
        })
    }

    /// Returns the configured REST base URL.
    #[must_use]
    pub fn base_url(&self) -> &str {
        self.base_url.as_str()
    }

    /// Returns the configured Lighter environment.
    #[must_use]
    pub const fn environment(&self) -> LighterEnvironment {
        self.environment
    }

    /// Overrides the REST base URL. Intended for mock-server tests.
    pub fn set_base_url(&mut self, base_url: &str) {
        self.base_url = base_url.trim_end_matches('/').to_string();
    }

    /// Overrides the retry manager. Intended for mock-server tests that need
    /// shorter backoff than [`create_http_retry_manager`] produces.
    pub fn set_retry_manager(&mut self, retry_manager: RetryManager<LighterHttpError>) {
        self.retry_manager = retry_manager;
    }

    /// Calls `GET /api/v1/orderBooks`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is invalid.
    pub async fn get_order_books(
        &self,
        query: &LighterOrderBooksQuery,
    ) -> LighterHttpResult<LighterOrderBooks> {
        self.send_get_request(ENDPOINT_ORDER_BOOKS, Some(query))
            .await
    }

    /// Calls `GET /api/v1/orderBookDetails`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is invalid.
    pub async fn get_order_book_details(
        &self,
        query: &LighterOrderBookDetailsQuery,
    ) -> LighterHttpResult<LighterOrderBookDetails> {
        self.send_get_request(ENDPOINT_ORDER_BOOK_DETAILS, Some(query))
            .await
    }

    /// Calls `GET /api/v1/orderBookOrders`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is invalid.
    pub async fn get_order_book_orders(
        &self,
        query: &LighterOrderBookOrdersQuery,
    ) -> LighterHttpResult<LighterOrderBookOrders> {
        self.send_get_request(ENDPOINT_ORDER_BOOK_ORDERS, Some(query))
            .await
    }

    /// Calls `GET /api/v1/recentTrades`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is invalid.
    pub async fn get_recent_trades(
        &self,
        query: &LighterRecentTradesQuery,
    ) -> LighterHttpResult<LighterTrades> {
        self.send_get_request(ENDPOINT_RECENT_TRADES, Some(query))
            .await
    }

    /// Calls `GET /api/v1/trades`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is invalid.
    pub async fn get_trades(&self, query: &LighterTradesQuery) -> LighterHttpResult<LighterTrades> {
        self.send_get_request(ENDPOINT_TRADES, Some(query)).await
    }

    /// Calls `GET /api/v1/candles`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is invalid.
    pub async fn get_candles(
        &self,
        query: &LighterCandlesQuery,
    ) -> LighterHttpResult<LighterCandles> {
        self.send_get_request(ENDPOINT_CANDLES, Some(query)).await
    }

    /// Calls `GET /api/v1/fundings`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is invalid.
    pub async fn get_fundings(
        &self,
        query: &LighterFundingsQuery,
    ) -> LighterHttpResult<LighterFundings> {
        self.send_get_request(ENDPOINT_FUNDINGS, Some(query)).await
    }

    /// Calls `GET /api/v1/accountActiveOrders`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is invalid.
    pub async fn get_account_active_orders(
        &self,
        query: &LighterAccountActiveOrdersQuery,
    ) -> LighterHttpResult<LighterOrders> {
        self.send_get_request(ENDPOINT_ACCOUNT_ACTIVE_ORDERS, Some(query))
            .await
    }

    /// Calls `GET /api/v1/accountInactiveOrders`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is invalid.
    pub async fn get_account_inactive_orders(
        &self,
        query: &LighterAccountInactiveOrdersQuery,
    ) -> LighterHttpResult<LighterOrders> {
        self.send_get_request(ENDPOINT_ACCOUNT_INACTIVE_ORDERS, Some(query))
            .await
    }

    /// Calls `GET /api/v1/nextNonce`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is invalid.
    pub async fn get_next_nonce(
        &self,
        query: &LighterNextNonceQuery,
    ) -> LighterHttpResult<LighterNextNonce> {
        self.send_get_request(ENDPOINT_NEXT_NONCE, Some(query))
            .await
    }

    /// Calls `POST /api/v1/sendTx`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is invalid.
    pub async fn send_tx(
        &self,
        request: &LighterSendTxRequest,
    ) -> LighterHttpResult<LighterSendTxResponse> {
        let fields = request.form_fields();
        self.send_post_form(ENDPOINT_SEND_TX, &fields).await
    }

    /// Calls `POST /api/v1/sendTxBatch`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is invalid.
    pub async fn send_tx_batch(
        &self,
        request: &LighterSendTxBatchRequest,
    ) -> LighterHttpResult<LighterSendTxBatchResponse> {
        let fields = request.form_fields();
        self.send_post_form(ENDPOINT_SEND_TX_BATCH, &fields).await
    }

    async fn send_get_request<T, P>(
        &self,
        endpoint: &str,
        params: Option<&P>,
    ) -> LighterHttpResult<T>
    where
        T: DeserializeOwned + LighterResponseCheck,
        P: Serialize,
    {
        let url = self.url(endpoint);
        let rate_limit_keys = Self::rate_limit_keys(endpoint);
        self.retry_manager
            .execute_with_retry(
                endpoint,
                || {
                    let url = url.clone();
                    let rate_limit_keys = rate_limit_keys.clone();
                    async move {
                        let response = self
                            .client
                            .request_with_params(
                                Method::GET,
                                url,
                                params,
                                None,
                                None,
                                None,
                                Some(rate_limit_keys),
                            )
                            .await?;
                        Self::parse_response(&response)
                    }
                },
                should_retry_lighter_http_error,
                create_lighter_http_timeout_error,
            )
            .await
    }

    // Single-shot: sendTx / sendTxBatch carry a signed nonce; transport-layer
    // retry could double-submit if the original landed and only the ack was lost.
    async fn send_post_form<T>(
        &self,
        endpoint: &str,
        fields: &[(&str, String)],
    ) -> LighterHttpResult<T>
    where
        T: DeserializeOwned + LighterResponseCheck,
    {
        let response = self
            .client
            .request(
                Method::POST,
                self.url(endpoint),
                None,
                Some(multipart_headers()),
                Some(multipart_form_bytes(fields)),
                None,
                Some(Self::rate_limit_keys(endpoint)),
            )
            .await?;

        Self::parse_response(&response)
    }

    fn parse_response<T>(response: &HttpResponse) -> LighterHttpResult<T>
    where
        T: DeserializeOwned + LighterResponseCheck,
    {
        if !response.status.is_success() {
            let status = response.status.as_u16();
            let body = String::from_utf8_lossy(&response.body).to_string();

            // Status-first: a `{code,message}` body must not override the
            // retry decision for 5xx / 429.
            if status >= 500 {
                return Err(LighterHttpError::Http { status, body });
            }

            if status == 429 {
                return Err(LighterHttpError::RateLimit(body));
            }

            if let Ok(result) = serde_json::from_slice::<LighterResultCode>(&response.body)
                && result.code != 200
            {
                Self::result_code_error(result.code, result.message.as_deref(), "HTTP error")?;
            }

            return Err(LighterHttpError::Http { status, body });
        }

        if let Ok(result) = serde_json::from_slice::<LighterResultCode>(&response.body) {
            Self::check_response(&result)?;
        }

        let payload: T = serde_json::from_slice(&response.body)?;
        Self::check_response(&payload)?;
        Ok(payload)
    }

    fn check_response<T: LighterResponseCheck>(payload: &T) -> LighterHttpResult<()> {
        Self::result_code_error(
            payload.response_code(),
            payload.response_message(),
            "Lighter request failed",
        )
    }

    fn result_code_error(
        code: i32,
        message: Option<&str>,
        default_message: &str,
    ) -> LighterHttpResult<()> {
        match code {
            200 => Ok(()),
            405 | 429 => Err(LighterHttpError::RateLimit(
                message.unwrap_or("Lighter rate limit exceeded").to_string(),
            )),
            code => Err(venue_error(code, message, default_message)),
        }
    }

    fn url(&self, endpoint: &str) -> String {
        format!("{}{}", self.base_url, endpoint)
    }

    fn rate_limit_keys(endpoint: &str) -> Vec<String> {
        let route = endpoint.strip_prefix(API_V1).unwrap_or(endpoint);
        vec![
            "lighter:rest".to_string(),
            format!("lighter:{}", route.trim_start_matches('/')),
        ]
    }

    fn default_headers() -> HashMap<String, String> {
        HashMap::from([(USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string())])
    }
}

fn multipart_headers() -> HashMap<String, String> {
    HashMap::from([
        ("Accept".to_string(), "application/json".to_string()),
        (
            "Content-Type".to_string(),
            format!("multipart/form-data; boundary={MULTIPART_BOUNDARY}"),
        ),
    ])
}

fn multipart_form_bytes(fields: &[(&str, String)]) -> Vec<u8> {
    let mut body = String::new();
    for (name, value) in fields {
        body.push_str("--");
        body.push_str(MULTIPART_BOUNDARY);
        body.push_str("\r\nContent-Disposition: form-data; name=\"");
        body.push_str(name);
        body.push_str("\"\r\n\r\n");
        body.push_str(value);
        body.push_str("\r\n");
    }
    body.push_str("--");
    body.push_str(MULTIPART_BOUNDARY);
    body.push_str("--\r\n");
    body.into_bytes()
}

/// Domain HTTP client for Lighter REST operations.
///
/// This client wraps [`LighterRawHttpClient`] and converts selected endpoint responses into
/// Nautilus domain data. Market metadata calls also populate the shared [`MarketRegistry`].
#[derive(Clone, Debug)]
pub struct LighterHttpClient {
    pub(crate) inner: Arc<LighterRawHttpClient>,
    market_registry: Arc<MarketRegistry>,
    clock: &'static AtomicTime,
}

impl Default for LighterHttpClient {
    fn default() -> Self {
        Self::new(LighterEnvironment::Mainnet, None, 60, None)
            .expect("failed to create default Lighter HTTP client")
    }
}

impl LighterHttpClient {
    /// Creates a new [`LighterHttpClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying raw HTTP client cannot be created.
    pub fn new(
        environment: LighterEnvironment,
        base_url: Option<String>,
        timeout_secs: u64,
        proxy_url: Option<String>,
    ) -> LighterHttpResult<Self> {
        let raw_client = LighterRawHttpClient::new(environment, base_url, timeout_secs, proxy_url)?;
        Ok(Self::from_raw(raw_client))
    }

    /// Wraps an existing raw HTTP client.
    #[must_use]
    pub fn from_raw(raw_client: LighterRawHttpClient) -> Self {
        Self::from_raw_with_registry(raw_client, Arc::new(MarketRegistry::new()))
    }

    /// Wraps an existing raw HTTP client and shared market registry.
    #[must_use]
    pub fn from_raw_with_registry(
        raw_client: LighterRawHttpClient,
        market_registry: Arc<MarketRegistry>,
    ) -> Self {
        Self {
            inner: Arc::new(raw_client),
            market_registry,
            clock: get_atomic_clock_realtime(),
        }
    }

    /// Returns the configured REST base URL.
    #[must_use]
    pub fn base_url(&self) -> &str {
        self.inner.base_url()
    }

    /// Returns the configured Lighter environment.
    #[must_use]
    pub fn environment(&self) -> LighterEnvironment {
        self.inner.environment()
    }

    /// Returns the shared market registry used by this client.
    #[must_use]
    pub fn market_registry(&self) -> Arc<MarketRegistry> {
        self.market_registry.clone()
    }

    /// Overrides the REST base URL. Intended for mock-server tests.
    ///
    /// # Panics
    ///
    /// Panics if the raw client is shared by another [`Arc`].
    pub fn set_base_url(&mut self, base_url: &str) {
        Arc::get_mut(&mut self.inner)
            .expect("cannot override URL: raw client is shared")
            .set_base_url(base_url);
    }

    /// Calls `GET /api/v1/orderBooks` and registers returned markets.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is invalid.
    pub async fn get_order_books(
        &self,
        query: &LighterOrderBooksQuery,
    ) -> LighterHttpResult<LighterOrderBooks> {
        let response = self.inner.get_order_books(query).await?;
        register_order_books(&self.market_registry, &response.order_books);
        Ok(response)
    }

    /// Calls `GET /api/v1/orderBookDetails` and registers returned markets.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is invalid.
    pub async fn get_order_book_details(
        &self,
        query: &LighterOrderBookDetailsQuery,
    ) -> LighterHttpResult<LighterOrderBookDetails> {
        let response = self.inner.get_order_book_details(query).await?;
        register_perp_order_book_details(&self.market_registry, &response.order_book_details);
        register_spot_order_book_details(&self.market_registry, &response.spot_order_book_details);
        Ok(response)
    }

    /// Calls `GET /api/v1/orderBookOrders`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is invalid.
    pub async fn get_order_book_orders(
        &self,
        query: &LighterOrderBookOrdersQuery,
    ) -> LighterHttpResult<LighterOrderBookOrders> {
        self.inner.get_order_book_orders(query).await
    }

    /// Calls `GET /api/v1/recentTrades`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is invalid.
    pub async fn get_recent_trades(
        &self,
        query: &LighterRecentTradesQuery,
    ) -> LighterHttpResult<LighterTrades> {
        self.inner.get_recent_trades(query).await
    }

    /// Calls `GET /api/v1/trades`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is invalid.
    pub async fn get_trades(&self, query: &LighterTradesQuery) -> LighterHttpResult<LighterTrades> {
        self.inner.get_trades(query).await
    }

    /// Calls `GET /api/v1/candles`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is invalid.
    pub async fn get_candles(
        &self,
        query: &LighterCandlesQuery,
    ) -> LighterHttpResult<LighterCandles> {
        self.inner.get_candles(query).await
    }

    /// Calls `GET /api/v1/fundings`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is invalid.
    pub async fn get_fundings(
        &self,
        query: &LighterFundingsQuery,
    ) -> LighterHttpResult<LighterFundings> {
        self.inner.get_fundings(query).await
    }

    /// Calls `GET /api/v1/accountActiveOrders`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is invalid.
    pub async fn get_account_active_orders(
        &self,
        query: &LighterAccountActiveOrdersQuery,
    ) -> LighterHttpResult<LighterOrders> {
        self.inner.get_account_active_orders(query).await
    }

    /// Calls `GET /api/v1/accountInactiveOrders`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is invalid.
    pub async fn get_account_inactive_orders(
        &self,
        query: &LighterAccountInactiveOrdersQuery,
    ) -> LighterHttpResult<LighterOrders> {
        self.inner.get_account_inactive_orders(query).await
    }

    /// Calls `GET /api/v1/nextNonce` for `(account_index, api_key_index)`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is invalid.
    pub async fn get_next_nonce(
        &self,
        account_index: i64,
        api_key_index: u8,
    ) -> LighterHttpResult<LighterNextNonce> {
        let query = LighterNextNonceQuery {
            account_index,
            api_key_index,
        };
        self.inner.get_next_nonce(&query).await
    }

    /// Calls `POST /api/v1/sendTx`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is invalid.
    pub async fn send_tx(
        &self,
        request: &LighterSendTxRequest,
    ) -> LighterHttpResult<LighterSendTxResponse> {
        self.inner.send_tx(request).await
    }

    /// Calls `POST /api/v1/sendTxBatch`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or the response is invalid.
    pub async fn send_tx_batch(
        &self,
        request: &LighterSendTxBatchRequest,
    ) -> LighterHttpResult<LighterSendTxBatchResponse> {
        self.inner.send_tx_batch(request).await
    }

    /// Requests recent trades for an instrument and parses them into [`TradeTick`]s.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument has not been registered, the request fails, or a trade
    /// cannot be parsed.
    pub async fn request_recent_trades(
        &self,
        instrument: &InstrumentAny,
        limit: u16,
    ) -> LighterHttpResult<Vec<TradeTick>> {
        let market_id = self.market_index(instrument)?;
        let query = LighterRecentTradesQuery { market_id, limit };
        let response = self.inner.get_recent_trades(&query).await?;
        self.parse_trade_ticks(&response.trades, instrument)
    }

    /// Requests historical trades and parses them into [`TradeTick`]s.
    ///
    /// If `query.market_id` is `None`, the value is resolved from the shared market registry.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument has not been registered, the request fails, or a trade
    /// cannot be parsed.
    pub async fn request_trades(
        &self,
        instrument: &InstrumentAny,
        mut query: LighterTradesQuery,
    ) -> LighterHttpResult<Vec<TradeTick>> {
        if query.market_id.is_none() {
            query.market_id = Some(self.market_index(instrument)?);
        }
        let response = self.inner.get_trades(&query).await?;
        self.parse_trade_ticks(&response.trades, instrument)
    }

    /// Requests historical candles and parses them into Nautilus [`Bar`]s.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument has not been registered, the bar
    /// type is unsupported, the request fails, or a candle cannot be parsed.
    pub async fn request_bars(
        &self,
        instrument: &InstrumentAny,
        bar_type: BarType,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> LighterHttpResult<Vec<Bar>> {
        let market_id = self.market_index(instrument)?;
        let resolution = LighterCandleResolution::try_from(&bar_type)?;
        let interval_ms = resolution.interval_millis();
        let now = Utc::now();

        if let (Some(start), Some(end)) = (start, end)
            && start >= end
        {
            return Err(LighterHttpError::Parse(format!(
                "invalid bar request range: start={start}, end={end}",
            )));
        }

        let end = end.unwrap_or(now).min(now);

        if let Some(start) = start
            && start >= end
        {
            return Ok(Vec::new());
        }

        let requested_limit = limit.filter(|n| *n > 0).map(|n| n as usize);
        let target_limit = requested_limit.unwrap_or(DEFAULT_BARS_LIMIT);
        let start_was_unspecified = start.is_none();
        let end_ms = end.timestamp_millis().max(0);
        let now_ms = now.timestamp_millis();

        if end_ms == 0 {
            return Ok(Vec::new());
        }

        let start_ms = start.map_or_else(
            || {
                let lookback_bars = target_limit.saturating_add(1);
                let lookback_bars = i64::try_from(lookback_bars).unwrap_or(i64::MAX);
                let lookback_ms = interval_ms.saturating_mul(lookback_bars);
                end_ms.saturating_sub(lookback_ms)
            },
            |dt| dt.timestamp_millis().max(0),
        );

        if start_ms >= end_ms {
            return Ok(Vec::new());
        }

        let mut bars = Vec::new();
        let mut cursor_ms = start_ms;
        let mut pages = 0_usize;
        let page_span_ms = interval_ms.saturating_mul(i64::from(LIGHTER_CANDLES_MAX_LIMIT));

        while cursor_ms < end_ms && pages < MAX_BAR_REQUEST_PAGES {
            if !start_was_unspecified
                && let Some(limit) = requested_limit
                && bars.len() >= limit
            {
                break;
            }

            let window_end_ms = cursor_ms.saturating_add(page_span_ms).min(end_ms);
            if window_end_ms <= cursor_ms {
                break;
            }

            let query = LighterCandlesQuery {
                market_id,
                resolution,
                start_timestamp: cursor_ms,
                end_timestamp: window_end_ms,
                count_back: i64::from(LIGHTER_CANDLES_MAX_LIMIT),
                set_timestamp_to_end: Some(false),
            };
            let response = self.get_candles(&query).await?;
            let mut page = self.parse_bars(&response.candles, instrument, bar_type)?;

            page.sort_by_key(|bar| bar.ts_event);
            for bar in page {
                let bar_start_ms = i64::try_from(bar.ts_event.as_u64() / 1_000_000)
                    .map_err(|e| LighterHttpError::Parse(e.to_string()))?;
                if bar_start_ms < cursor_ms
                    || bar_start_ms >= end_ms
                    || bar_start_ms.saturating_add(interval_ms) > now_ms
                {
                    continue;
                }

                if bars
                    .last()
                    .is_some_and(|last: &Bar| last.ts_event == bar.ts_event)
                {
                    continue;
                }
                bars.push(bar);

                if !start_was_unspecified
                    && let Some(limit) = requested_limit
                    && bars.len() >= limit
                {
                    break;
                }
            }

            cursor_ms = window_end_ms;
            pages += 1;
        }

        if pages >= MAX_BAR_REQUEST_PAGES {
            log::warn!("Stopped Lighter bar request after {MAX_BAR_REQUEST_PAGES} pages");
        }

        if start_was_unspecified && bars.len() > target_limit {
            bars = bars.split_off(bars.len() - target_limit);
        }

        Ok(bars)
    }

    /// Requests historical funding rates and parses them into Nautilus updates.
    ///
    /// Lighter's public `/api/v1/fundings` endpoint returns settled funding
    /// rows at hourly or daily resolution. The adapter requests hourly rows and
    /// returns them in chronological order.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument is not a perpetual, the instrument
    /// has not been registered, the request range is invalid, or a row cannot
    /// be parsed.
    pub async fn request_funding_rates(
        &self,
        instrument: &InstrumentAny,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<usize>,
    ) -> LighterHttpResult<Vec<FundingRateUpdate>> {
        if !matches!(instrument, InstrumentAny::CryptoPerpetual(_)) {
            return Err(LighterHttpError::Parse(format!(
                "funding rates are only available for perpetual instruments: {}",
                instrument.id()
            )));
        }

        let market_id = self.market_index(instrument)?;
        let resolution = LighterFundingResolution::OneHour;
        let interval_ms = resolution.interval_millis();
        let now = Utc::now();

        if let (Some(start), Some(end)) = (start, end)
            && start >= end
        {
            return Err(LighterHttpError::Parse(format!(
                "invalid funding request range: start={start}, end={end}",
            )));
        }

        let end = end.unwrap_or(now).min(now);

        if let Some(start) = start
            && start >= end
        {
            return Ok(Vec::new());
        }

        let requested_limit = limit.filter(|n| *n > 0);
        let target_limit = requested_limit.unwrap_or(DEFAULT_FUNDING_RATES_LIMIT);
        let start_was_unspecified = start.is_none();
        let end_ms = end.timestamp_millis().max(0);

        if end_ms == 0 {
            return Ok(Vec::new());
        }

        let start_ms = start.map_or_else(
            || {
                let lookback_rows = target_limit.saturating_add(1);
                let lookback_rows = i64::try_from(lookback_rows).unwrap_or(i64::MAX);
                let lookback_ms = interval_ms.saturating_mul(lookback_rows);
                end_ms.saturating_sub(lookback_ms)
            },
            |dt| dt.timestamp_millis().max(0),
        );

        if start_ms >= end_ms {
            return Ok(Vec::new());
        }

        let query = LighterFundingsQuery {
            market_id,
            resolution,
            start_timestamp: start_ms,
            end_timestamp: end_ms,
            count_back: i64::try_from(target_limit).unwrap_or(i64::MAX),
        };
        let response = self.get_fundings(&query).await?;
        let ts_init = self.generate_ts_init();
        let interval = Some(resolution.interval_minutes());
        let mut funding_rates = Vec::with_capacity(response.fundings.len());

        for funding in &response.fundings {
            let update = parse_funding_rate_update(funding, instrument.id(), interval, ts_init)
                .map_err(LighterHttpError::from)?;
            let timestamp_ms = i64::try_from(update.ts_event.as_u64() / 1_000_000)
                .map_err(|e| LighterHttpError::Parse(e.to_string()))?;

            if timestamp_ms < start_ms || timestamp_ms > end_ms {
                continue;
            }
            funding_rates.push(update);
        }

        funding_rates.sort_by_key(|rate| rate.ts_event);

        if start_was_unspecified && funding_rates.len() > target_limit {
            funding_rates = funding_rates.split_off(funding_rates.len() - target_limit);
        } else if let Some(limit) = requested_limit
            && funding_rates.len() > limit
        {
            funding_rates.truncate(limit);
        }

        Ok(funding_rates)
    }

    /// Requests all instruments from Lighter order book metadata.
    ///
    /// Lighter exposes market definitions through `GET /api/v1/orderBookDetails`.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or an instrument cannot be parsed.
    pub async fn request_instruments(&self) -> LighterHttpResult<Vec<InstrumentAny>> {
        self.request_instruments_for_query(&LighterOrderBookDetailsQuery::default())
            .await
    }

    /// Requests all instruments and their market statuses from Lighter order book metadata.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or an instrument cannot be parsed.
    pub async fn request_instruments_with_status(
        &self,
    ) -> LighterHttpResult<Vec<(InstrumentAny, LighterMarketStatus)>> {
        self.request_instruments_with_status_for_query(&LighterOrderBookDetailsQuery::default())
            .await
    }

    /// Requests a single instrument from Lighter order book metadata.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails, the instrument is not found, or the instrument
    /// cannot be parsed.
    pub async fn request_instrument(
        &self,
        instrument_id: InstrumentId,
    ) -> LighterHttpResult<InstrumentAny> {
        self.request_instrument_with_status(instrument_id)
            .await
            .map(|(instrument, _)| instrument)
    }

    /// Requests a single instrument and its market status from Lighter order book metadata.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails, the instrument is not found, or the instrument
    /// cannot be parsed.
    pub async fn request_instrument_with_status(
        &self,
        instrument_id: InstrumentId,
    ) -> LighterHttpResult<(InstrumentAny, LighterMarketStatus)> {
        let query = LighterOrderBookDetailsQuery {
            market_id: self.market_registry.market_index(&instrument_id),
            filter: None,
        };
        let instruments = self
            .request_instruments_with_status_for_query(&query)
            .await?;

        instruments
            .into_iter()
            .find(|(instrument, _)| instrument.id() == instrument_id)
            .ok_or_else(|| LighterHttpError::Parse(format!("instrument {instrument_id} not found")))
    }

    /// Requests an HTTP order book snapshot and parses it into Nautilus order book deltas.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument has not been registered, the request fails, or any level
    /// cannot be parsed.
    pub async fn request_order_book_snapshot(
        &self,
        instrument: &InstrumentAny,
        limit: u16,
    ) -> LighterHttpResult<OrderBookDeltas> {
        let query = LighterOrderBookOrdersQuery {
            market_id: self.market_index(instrument)?,
            limit,
        };
        let snapshot = self.inner.get_order_book_orders(&query).await?;
        let ts_init = self.generate_ts_init();

        parse_order_book_snapshot(
            &snapshot,
            instrument.id(),
            instrument.price_precision(),
            instrument.size_precision(),
            ts_init,
            ts_init,
        )
        .map_err(LighterHttpError::from)
    }

    async fn request_instruments_for_query(
        &self,
        query: &LighterOrderBookDetailsQuery,
    ) -> LighterHttpResult<Vec<InstrumentAny>> {
        self.request_instruments_with_status_for_query(query)
            .await
            .map(|instruments| {
                instruments
                    .into_iter()
                    .map(|(instrument, _)| instrument)
                    .collect()
            })
    }

    async fn request_instruments_with_status_for_query(
        &self,
        query: &LighterOrderBookDetailsQuery,
    ) -> LighterHttpResult<Vec<(InstrumentAny, LighterMarketStatus)>> {
        let response = self.get_order_book_details(query).await?;
        let ts_init = self.generate_ts_init();
        parse_order_book_details_instruments_with_status(
            &self.market_registry,
            &response.order_book_details,
            &response.spot_order_book_details,
            ts_init,
        )
        .map_err(LighterHttpError::from)
    }

    fn parse_trade_ticks(
        &self,
        trades: &[LighterTrade],
        instrument: &InstrumentAny,
    ) -> LighterHttpResult<Vec<TradeTick>> {
        let ts_init = self.generate_ts_init();
        trades
            .iter()
            .map(|trade| parse_trade_tick(trade, instrument, ts_init).map_err(Into::into))
            .collect()
    }

    fn parse_bars(
        &self,
        candles: &[LighterCandle],
        instrument: &InstrumentAny,
        bar_type: BarType,
    ) -> LighterHttpResult<Vec<Bar>> {
        let ts_init = self.generate_ts_init();
        candles
            .iter()
            .map(|candle| {
                parse_candle_bar(candle, bar_type, instrument, ts_init).map_err(Into::into)
            })
            .collect()
    }

    fn market_index(&self, instrument: &InstrumentAny) -> LighterHttpResult<i16> {
        self.market_registry
            .market_index(&instrument.id())
            .ok_or_else(|| {
                LighterHttpError::Parse(format!(
                    "market index not registered for instrument {}",
                    instrument.id()
                ))
            })
    }

    fn generate_ts_init(&self) -> UnixNanos {
        self.clock.get_time_ns()
    }
}

fn venue_error(code: i32, message: Option<&str>, default_message: &str) -> LighterHttpError {
    LighterHttpError::Venue {
        code: i64::from(code),
        message: message.unwrap_or(default_message).to_string(),
    }
}
