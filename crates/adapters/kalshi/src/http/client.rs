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

//! Typed Kalshi REST reads and V2 order transport.

use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use nautilus_common::config::ConfigError;
use nautilus_core::{string::secret::SecretString, time::get_atomic_clock_realtime};
use nautilus_network::{
    http::{HttpClient, HttpClientError, HttpRedirectPolicy, HttpResponse, Method},
    retry::{RetryConfig, RetryError, RetryManager},
};
use serde::{Serialize, de::DeserializeOwned};
use tokio_util::sync::CancellationToken;
use url::Url;

use super::{
    error::KalshiHttpError,
    models::*,
    parse::{Pages, Pagination, decode_body, decode_response, unexpected_response},
    query::*,
    quota::{Bucket, Charge, KalshiQuota},
};
use crate::{
    common::credential::{KalshiCredential, credential_env_vars},
    config::KalshiDataClientConfig,
};

static QUOTAS: LazyLock<Mutex<QuotaOwners>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// REST transport for public discovery, account reports, and V2 order requests.
///
/// Public construction does not resolve credentials. Authenticated construction validates them
/// before traffic. Safe reads retry transient failures twice at the adapter, acquiring quota before
/// each fresh timestamp and signature. Redirects are never followed.
/// Typed mutations use one adapter-owned attempt. Lower-level transport retries can replay the
/// original signed request without entering adapter quota or signing. Callers reconcile uncertain
/// or mismatched acknowledgements.
#[derive(Clone)]
pub struct KalshiHttpClient {
    client: HttpClient,
    base_url: SecretString,
    credential: Option<Arc<KalshiCredential>>,
    cancellation: CancellationToken,
    dispatch_check: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    quota: Arc<KalshiQuota>,
    timeout_ms: u64,
}

impl KalshiHttpClient {
    /// Validates the data-client settings and creates the public transport.
    ///
    /// Explicit proxies override direct routing. Ambient proxy variables are ignored.
    ///
    /// # Errors
    ///
    /// Returns an error if config validation or transport construction fails.
    pub fn new(config: &KalshiDataClientConfig) -> anyhow::Result<Self> {
        Self::build(config)
    }

    /// Creates an authenticated transport from explicit or selected-environment credentials.
    ///
    /// # Errors
    ///
    /// Returns an error if config, credentials, key encoding, or transport construction is invalid.
    pub fn new_authenticated(config: &KalshiDataClientConfig) -> anyhow::Result<Self> {
        Self::authenticated(config, None)
    }

    /// Creates an authenticated transport using the supplied account quota owner.
    ///
    /// Share one owner across data and execution clients, including distinct keys for the same
    /// account. A key already bound to another owner is rejected before traffic.
    ///
    /// # Errors
    ///
    /// Returns an error if settings or credentials are invalid, or quota ownership conflicts.
    pub fn new_authenticated_with_quota(
        config: &KalshiDataClientConfig,
        quota: Arc<KalshiQuota>,
    ) -> anyhow::Result<Self> {
        Self::authenticated(config, Some(quota))
    }

    /// Creates a public data transport sharing an authenticated account's quota.
    ///
    /// # Errors
    ///
    /// Returns an error if settings or transport construction is invalid.
    pub fn new_with_quota(
        config: &KalshiDataClientConfig,
        quota: Arc<KalshiQuota>,
    ) -> anyhow::Result<Self> {
        let mut client = Self::build(config)?;
        client.quota = quota;
        Ok(client)
    }

    /// Refreshes authenticated limits and endpoint costs for the shared account owner.
    ///
    /// Transports with credentials refresh lazily before ordinary traffic, including public reads.
    /// Fresh discovery or an active five-second fallback window returns without traffic.
    /// Failed discovery uses Basic read limits, one balance read per second, and a conservative
    /// 50-token read cost during the fallback window. Expiry uses those read budgets until refresh;
    /// writes wait for discovery and remain disabled if it fails.
    ///
    /// # Errors
    ///
    /// Returns an error if either discovery request or its advertised limits/costs is invalid.
    pub async fn refresh_quota(&self) -> Result<(), KalshiHttpError> {
        let _guard = tokio::select! {
            biased;
            () = self.cancellation.cancelled() => return Err(KalshiHttpError::Canceled { dispatched: false }),
            guard = self.quota.refresh.lock() => guard,
        };

        if !self.quota.needs_refresh() {
            return Ok(());
        }

        let limits = self.get_account_limits().await;
        let costs = self.get_endpoint_costs().await;
        if let Err(e) = limits.and(costs.map(|_| ())) {
            if !self.cancellation.is_cancelled() {
                self.quota.discovery_failed();
            }

            return Err(e);
        }

        Ok(())
    }

    pub(crate) async fn batch_size(&self, method: Method) -> Result<usize, KalshiHttpError> {
        self.refresh_quota().await?;
        let charge = request_charge(
            &method,
            &["portfolio", "events", "orders", "batched"],
            "",
            None,
            Operation::Write,
        )?;
        self.quota.batch_size(&charge)
    }

    pub(crate) fn with_dispatch_check(
        mut self,
        cancellation: CancellationToken,
        check: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Self {
        self.cancellation = cancellation;
        self.dispatch_check = Some(Arc::new(check));
        self
    }

    /// Fetches the public exchange status without authentication.
    ///
    /// Non-success HTTP statuses remain responses for the caller to inspect.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or exceeds the configured timeout.
    pub async fn get_exchange_status(&self) -> Result<HttpResponse, HttpClientError> {
        self.send_raw(
            Method::GET,
            &["exchange", "status"],
            &(),
            Option::<&()>::None,
            false,
            Operation::Read,
        )
        .await
        .map_err(|e| match e {
            KalshiHttpError::Transport(e) => e,
            e => HttpClientError::Error(e.to_string()),
        })
    }

    /// Fetches the authenticated balance on an explicit exchange shard.
    ///
    /// # Errors
    ///
    /// Returns an error if authentication, quota admission, retries, or transport fails.
    pub async fn get_balance(&self, exchange_index: u32) -> Result<HttpResponse, KalshiHttpError> {
        let response = self
            .send_raw(
                Method::GET,
                &["portfolio", "balance"],
                &[("exchange_index", exchange_index)],
                Option::<&()>::None,
                true,
                Operation::Read,
            )
            .await?;

        if !response.status.is_success() {
            return Err(KalshiHttpError::Status(response.status.as_u16()));
        }

        Ok(response)
    }

    /// Cancels typed REST requests on this client and its clones.
    ///
    /// Cancellation after dispatch does not establish whether a mutation reached the venue.
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }

    /// Fetches typed public exchange availability.
    ///
    /// Documented maintenance responses (HTTP 500, 503, and 504) return their typed status body.
    /// Use [`Self::get_exchange_status`] when the HTTP status and response headers are required.
    ///
    /// # Errors
    ///
    /// Returns an error if transport, cancellation, status, or response decoding fails.
    pub async fn get_status(&self) -> Result<ExchangeStatus, KalshiHttpError> {
        let response = self
            .send_raw(
                Method::GET,
                &["exchange", "status"],
                &(),
                Option::<&()>::None,
                false,
                Operation::Read,
            )
            .await?;

        if matches!(response.status.as_u16(), 500 | 503 | 504) {
            return decode_body(response.clone()).or_else(|_| decode_response(response));
        }

        decode_response(response)
    }

    /// Fetches one live market discovery page.
    ///
    /// # Errors
    ///
    /// Returns an error if encoding, transport, cancellation, status, or decoding fails.
    pub async fn get_markets(
        &self,
        query: &MarketsQuery,
    ) -> Result<MarketsResponse, KalshiHttpError> {
        self.get(&["markets"], query, false).await
    }

    /// Fetches one market by ticker.
    ///
    /// # Errors
    ///
    /// Returns an error if the ticker, transport, status, cancellation, or decoding is invalid.
    pub async fn get_market(&self, ticker: &str) -> Result<MarketResponse, KalshiHttpError> {
        self.get(&["markets", ticker], &(), false).await
    }

    /// Fetches one archived market discovery page.
    ///
    /// # Errors
    ///
    /// Returns an error if encoding, transport, cancellation, status, or decoding fails.
    pub async fn get_historical_markets(
        &self,
        query: &HistoricalMarketsQuery,
    ) -> Result<MarketsResponse, KalshiHttpError> {
        self.get(&["historical", "markets"], query, false).await
    }

    /// Fetches one archived market by ticker.
    ///
    /// # Errors
    ///
    /// Returns an error if the ticker, transport, status, cancellation, or decoding is invalid.
    pub async fn get_historical_market(
        &self,
        ticker: &str,
    ) -> Result<MarketResponse, KalshiHttpError> {
        self.get(&["historical", "markets", ticker], &(), false)
            .await
    }

    /// Fetches one event discovery page.
    ///
    /// # Errors
    ///
    /// Returns an error if encoding, transport, cancellation, status, or decoding fails.
    pub async fn get_events(&self, query: &EventsQuery) -> Result<EventsResponse, KalshiHttpError> {
        self.get(&["events"], query, false).await
    }

    /// Fetches one event and its markets.
    ///
    /// # Errors
    ///
    /// Returns an error if the ticker, encoding, transport, cancellation, status, or decoding fails.
    pub async fn get_event(
        &self,
        ticker: &str,
        query: &EventQuery,
    ) -> Result<EventResponse, KalshiHttpError> {
        self.get(&["events", ticker], query, false).await
    }

    /// Fetches series definitions.
    ///
    /// # Errors
    ///
    /// Returns an error if encoding, transport, cancellation, status, or decoding fails.
    pub async fn get_series_list(
        &self,
        query: &SeriesQuery,
    ) -> Result<SeriesListResponse, KalshiHttpError> {
        self.get(&["series"], query, false).await
    }

    /// Fetches one series definition.
    ///
    /// # Errors
    ///
    /// Returns an error if the ticker, transport, cancellation, status, or decoding is invalid.
    pub async fn get_series(&self, ticker: &str) -> Result<SeriesResponse, KalshiHttpError> {
        self.get(&["series", ticker], &(), false).await
    }

    /// Fetches exact YES and NO bid levels; zero depth requests all levels.
    ///
    /// # Errors
    ///
    /// Returns an error if depth exceeds 100, or transport, cancellation, status, or decoding fails.
    pub async fn get_orderbook(
        &self,
        ticker: &str,
        depth: u32,
    ) -> Result<OrderbookResponse, KalshiHttpError> {
        if depth > 100 {
            return Err(KalshiHttpError::Validation("depth exceeds 100"));
        }

        self.get(
            &["markets", ticker, "orderbook"],
            &[("depth", depth)],
            false,
        )
        .await
    }

    /// Fetches one public trade page from the live tier.
    ///
    /// # Errors
    ///
    /// Returns an error if encoding, transport, cancellation, status, or decoding fails.
    pub async fn get_trades(&self, query: &TradesQuery) -> Result<TradesResponse, KalshiHttpError> {
        self.get(&["markets", "trades"], query, false).await
    }

    /// Fetches one public trade page from the archive.
    ///
    /// # Errors
    ///
    /// Returns an error if encoding, transport, cancellation, status, or decoding fails.
    pub async fn get_historical_trades(
        &self,
        query: &TradesQuery,
    ) -> Result<TradesResponse, KalshiHttpError> {
        self.get(&["historical", "trades"], query, false).await
    }

    /// Fetches live market candles for a supported interval.
    ///
    /// # Errors
    ///
    /// Returns an error if the window, interval, transport, status, cancellation, or decoding fails.
    pub async fn get_candlesticks(
        &self,
        series: &str,
        ticker: &str,
        query: &CandlesticksQuery,
    ) -> Result<CandlesticksResponse, KalshiHttpError> {
        validate_candlesticks(query)?;
        self.get(
            &["series", series, "markets", ticker, "candlesticks"],
            query,
            false,
        )
        .await
    }

    /// Fetches archived market candles.
    ///
    /// # Errors
    ///
    /// Returns an error if the window, interval, transport, status, cancellation, or decoding fails.
    pub async fn get_historical_candlesticks(
        &self,
        ticker: &str,
        query: &CandlesticksQuery,
    ) -> Result<HistoricalCandlesticksResponse, KalshiHttpError> {
        validate_candlesticks(query)?;
        self.get(
            &["historical", "markets", ticker, "candlesticks"],
            query,
            false,
        )
        .await
    }

    /// Fetches each archive partition's authoritative cutoff.
    ///
    /// # Errors
    ///
    /// Returns an error if transport, cancellation, status, or decoding fails.
    pub async fn get_historical_cutoff(&self) -> Result<HistoricalCutoff, KalshiHttpError> {
        self.get(&["historical", "cutoff"], &(), false).await
    }

    /// Fetches typed collateral for an explicit primary-account shard.
    ///
    /// # Errors
    ///
    /// Returns an error if scope, authentication, transport, cancellation, status, or decoding fails.
    pub async fn get_account_balance(
        &self,
        query: &AccountQuery,
    ) -> Result<Balance, KalshiHttpError> {
        validate_primary(query.subaccount)?;
        self.get(&["portfolio", "balance"], query, true).await
    }

    /// Fetches one live market/event position page.
    ///
    /// # Errors
    ///
    /// Returns an error if scope, authentication, encoding, transport, cancellation, status, or decoding fails.
    pub async fn get_positions(
        &self,
        query: &PositionsQuery,
    ) -> Result<PositionsResponse, KalshiHttpError> {
        validate_primary(query.subaccount)?;
        self.get(&["portfolio", "positions"], query, true).await
    }

    /// Fetches one archived market/event position page.
    ///
    /// # Errors
    ///
    /// Returns an error if scope, authentication, encoding, transport, cancellation, status, or decoding fails.
    pub async fn get_historical_positions(
        &self,
        query: &HistoricalPositionsQuery,
    ) -> Result<PositionsResponse, KalshiHttpError> {
        validate_primary(query.subaccount)?;
        self.get(&["historical", "positions"], query, true).await
    }

    /// Fetches one live fills page.
    ///
    /// # Errors
    ///
    /// Returns an error if scope, authentication, encoding, transport, cancellation, status, or decoding fails.
    pub async fn get_fills(&self, query: &FillsQuery) -> Result<FillsResponse, KalshiHttpError> {
        validate_primary(query.subaccount)?;
        self.get(&["portfolio", "fills"], query, true).await
    }

    /// Fetches one archived fills page.
    ///
    /// # Errors
    ///
    /// Returns an error if scope, authentication, encoding, transport, cancellation, status, or decoding fails.
    pub async fn get_historical_fills(
        &self,
        query: &HistoryQuery,
    ) -> Result<FillsResponse, KalshiHttpError> {
        validate_primary(query.subaccount)?;
        self.get(&["historical", "fills"], query, true).await
    }

    /// Fetches one live orders page.
    ///
    /// # Errors
    ///
    /// Returns an error if scope, authentication, encoding, transport, cancellation, status, or decoding fails.
    pub async fn get_orders(&self, query: &OrdersQuery) -> Result<OrdersResponse, KalshiHttpError> {
        validate_primary(query.subaccount)?;
        self.get(&["portfolio", "orders"], query, true).await
    }

    /// Fetches one order by its venue identity.
    ///
    /// # Errors
    ///
    /// Returns an error if identity, authentication, transport, cancellation, status, or decoding fails.
    pub async fn get_order(&self, order_id: &str) -> Result<OrderResponse, KalshiHttpError> {
        self.get(&["portfolio", "orders", order_id], &(), true)
            .await
    }

    /// Fetches one archived orders page.
    ///
    /// # Errors
    ///
    /// Returns an error if scope, authentication, encoding, transport, cancellation, status, or decoding fails.
    pub async fn get_historical_orders(
        &self,
        query: &HistoryQuery,
    ) -> Result<OrdersResponse, KalshiHttpError> {
        validate_primary(query.subaccount)?;
        self.get(&["historical", "orders"], query, true).await
    }

    /// Fetches one page of settlements still held in the live tier.
    ///
    /// # Errors
    ///
    /// Returns an error if scope, authentication, encoding, transport, cancellation, status, or decoding fails.
    pub async fn get_settlements(
        &self,
        query: &SettlementsQuery,
    ) -> Result<SettlementsResponse, KalshiHttpError> {
        validate_primary(query.subaccount)?;
        self.get(&["portfolio", "settlements"], query, true).await
    }

    /// Fetches token limits and updates the shared account quota owner.
    ///
    /// # Errors
    ///
    /// Returns an error if authentication, transport, cancellation, status, decoding, or quota validation fails.
    pub async fn get_account_limits(&self) -> Result<AccountLimits, KalshiHttpError> {
        let limits: AccountLimits = self.get(&["account", "limits"], &(), true).await?;
        self.quota
            .set_limits(&limits, get_atomic_clock_realtime().get_time_ms() as i64)
            .map_err(|e| KalshiHttpError::AfterSend(Box::new(e)))?;
        Ok(limits)
    }

    /// Fetches endpoint token costs and updates the shared account quota owner.
    ///
    /// # Errors
    ///
    /// Returns an error if transport, cancellation, status, decoding, or quota validation fails.
    pub async fn get_endpoint_costs(&self) -> Result<EndpointCosts, KalshiHttpError> {
        let costs: EndpointCosts = self.get(&["account", "endpoint_costs"], &(), false).await?;
        self.quota
            .set_costs(costs.clone())
            .map_err(|e| KalshiHttpError::AfterSend(Box::new(e)))?;
        Ok(costs)
    }

    /// Sends one V2 limit-order request without replay.
    ///
    /// # Errors
    ///
    /// Returns an error if validation, authentication, encoding, transport, cancellation, status, or decoding fails.
    pub async fn create_order(
        &self,
        request: &CreateOrderRequest,
    ) -> Result<CreateOrderResponse, KalshiHttpError> {
        validate_create(request)?;
        self.send(
            Method::POST,
            &["portfolio", "events", "orders"],
            &(),
            Some(request),
            true,
        )
        .await
    }

    /// Sends independent V2 order requests once in one batch.
    ///
    /// Each child requires a unique, non-empty client order identity. Reply identities must match.
    ///
    /// # Errors
    ///
    /// Returns an error if validation, authentication, encoding, transport, cancellation, status, or decoding fails.
    pub async fn batch_create_orders(
        &self,
        request: &BatchCreateRequest,
    ) -> Result<BatchCreateResponse, KalshiHttpError> {
        self.create_batch(request, true).await
    }

    pub(crate) async fn create_batch(
        &self,
        request: &BatchCreateRequest,
        complete: bool,
    ) -> Result<BatchCreateResponse, KalshiHttpError> {
        if request.orders.is_empty() {
            return Err(KalshiHttpError::Validation("empty order batch"));
        }

        let mut identities = HashSet::new();

        for order in &request.orders {
            validate_create(order)?;
            let identity = order
                .client_order_id
                .as_deref()
                .filter(|id| !id.is_empty())
                .ok_or(KalshiHttpError::Validation(
                    "batch children require client order identities",
                ))?;

            if !identities.insert(identity) {
                return Err(KalshiHttpError::Validation(
                    "duplicate batch client order identity",
                ));
            }
        }

        let raw = self
            .send_raw(
                Method::POST,
                &["portfolio", "events", "orders", "batched"],
                &(),
                Some(request),
                true,
                Operation::Write,
            )
            .await?;
        let response: BatchCreateResponse = decode_response(raw.clone())?;
        let mut returned = HashSet::new();

        if complete
            && (response.orders.len() != identities.len()
                || response
                    .orders
                    .iter()
                    .any(|order| match &order.client_order_id {
                        Field::Value(identity) => {
                            !identities.contains(identity.as_str())
                                || !returned.insert(identity.as_str())
                        }
                        _ => true,
                    }))
        {
            return Err(unexpected_response(
                raw,
                "batch create result identities do not match requests",
            ));
        }

        Ok(response)
    }

    /// Sends a V2 amendment once. Expiration remains a raw protocol field.
    ///
    /// Callers verify returned order and client identities, then obtain authoritative order reports.
    ///
    /// # Errors
    ///
    /// Returns an error if scope, values, authentication, encoding, transport, cancellation, status, or decoding fails.
    pub async fn amend_order(
        &self,
        order_id: &str,
        request: &AmendOrderRequest,
    ) -> Result<AmendOrderResponse, KalshiHttpError> {
        validate_order_values(&request.ticker, request.price, request.count)?;
        self.send(
            Method::POST,
            &["portfolio", "events", "orders", order_id, "amend"],
            &[("subaccount", 0)],
            Some(request),
            true,
        )
        .await
    }

    /// Sends a V2 decrease once, using exactly one selector.
    ///
    /// Callers verify returned order and client identities, then obtain authoritative order reports.
    ///
    /// # Errors
    ///
    /// Returns an error if selectors, values, authentication, encoding, transport, cancellation, status, or decoding fails.
    pub async fn decrease_order(
        &self,
        order_id: &str,
        request: &DecreaseOrderRequest,
    ) -> Result<DecreaseOrderResponse, KalshiHttpError> {
        match (request.reduce_by, request.reduce_to) {
            (Some(value), None)
                if value.0 > rust_decimal::Decimal::ZERO && value.0.scale() <= 2 => {}
            (None, Some(value))
                if value.0 >= rust_decimal::Decimal::ZERO && value.0.scale() <= 2 => {}
            _ => {
                return Err(KalshiHttpError::Validation(
                    "exactly one valid decrease selector is required",
                ));
            }
        }

        self.send(
            Method::POST,
            &["portfolio", "events", "orders", order_id, "decrease"],
            &[("subaccount", 0)],
            Some(request),
            true,
        )
        .await
    }

    /// Cancels one selected V2 order once on an explicit shard.
    ///
    /// Callers verify returned order and client identities, then obtain authoritative order reports.
    ///
    /// # Errors
    ///
    /// Returns an error if scope, authentication, encoding, transport, cancellation, status, or decoding fails.
    pub async fn cancel_order(
        &self,
        order_id: &str,
        query: &AccountQuery,
    ) -> Result<CancelOrderResponse, KalshiHttpError> {
        self.cancel_order_for_market(order_id, query, None).await
    }

    pub(crate) async fn cancel_order_for_market(
        &self,
        order_id: &str,
        query: &AccountQuery,
        ticker: Option<&str>,
    ) -> Result<CancelOrderResponse, KalshiHttpError> {
        validate_primary(query.subaccount)?;
        self.send(
            Method::DELETE,
            &["portfolio", "events", "orders", order_id],
            &CancelQuery {
                account: query,
                market_ticker: ticker,
            },
            Option::<&()>::None,
            true,
        )
        .await
    }

    /// Cancels only the explicitly selected V2 orders, without replay.
    ///
    /// Selected order identities must be unique. Reply identities must match the selected set.
    ///
    /// # Errors
    ///
    /// Returns an error if scope, identities, authentication, encoding, transport, cancellation, status, or decoding fails.
    pub async fn batch_cancel_orders(
        &self,
        request: &BatchCancelRequest,
    ) -> Result<BatchCancelResponse, KalshiHttpError> {
        self.cancel_batch(request, true).await
    }

    pub(crate) async fn cancel_batch(
        &self,
        request: &BatchCancelRequest,
        complete: bool,
    ) -> Result<BatchCancelResponse, KalshiHttpError> {
        if request.orders.is_empty() {
            return Err(KalshiHttpError::Validation("empty cancel batch"));
        }

        let mut identities = HashSet::new();

        for order in &request.orders {
            validate_primary(order.subaccount)?;
            if order.order_id.is_empty() {
                return Err(KalshiHttpError::Validation("empty order identity"));
            }

            if !identities.insert(order.order_id.as_str()) {
                return Err(KalshiHttpError::Validation(
                    "duplicate batch cancel order identity",
                ));
            }
        }

        let raw = self
            .send_raw(
                Method::DELETE,
                &["portfolio", "events", "orders", "batched"],
                &(),
                Some(request),
                true,
                Operation::Write,
            )
            .await?;
        let response: BatchCancelResponse = decode_response(raw.clone())?;
        let mut returned = HashSet::new();

        if complete
            && (response.orders.len() != identities.len()
                || response.orders.iter().any(|order| {
                    !identities.contains(order.order_id.as_str())
                        || !returned.insert(order.order_id.as_str())
                }))
        {
            return Err(unexpected_response(
                raw,
                "batch cancel result identities do not match requests",
            ));
        }

        Ok(response)
    }

    /// Collects live market pages with stable-ticker deduplication and a page budget.
    ///
    /// # Errors
    ///
    /// Returns an error if any source fails, the budget is zero, or pagination makes no progress.
    pub async fn get_all_markets(
        &self,
        query: MarketsQuery,
        max_pages: u32,
    ) -> Result<Pages<Market>, KalshiHttpError> {
        self.collect_pages(
            query,
            max_pages,
            |q| async move { self.get_markets(&q).await },
            |m| m.ticker.clone(),
        )
        .await
    }

    /// Collects archived market pages with stable-ticker deduplication.
    ///
    /// # Errors
    ///
    /// Returns an error if any source fails, the budget is zero, or pagination makes no progress.
    pub async fn get_all_historical_markets(
        &self,
        query: HistoricalMarketsQuery,
        max_pages: u32,
    ) -> Result<Pages<Market>, KalshiHttpError> {
        self.collect_pages(
            query,
            max_pages,
            |q| async move { self.get_historical_markets(&q).await },
            |m| m.ticker.clone(),
        )
        .await
    }

    /// Collects event pages with stable-event-identity deduplication.
    ///
    /// # Errors
    ///
    /// Returns an error if any source fails, the budget is zero, or pagination makes no progress.
    pub async fn get_all_events(
        &self,
        query: EventsQuery,
        max_pages: u32,
    ) -> Result<Pages<Event>, KalshiHttpError> {
        self.collect_pages(
            query,
            max_pages,
            |q| async move { self.get_events(&q).await },
            |e| e.event_ticker.clone(),
        )
        .await
    }

    /// Collects live trades, deduplicating inclusive boundaries by trade identity.
    ///
    /// # Errors
    ///
    /// Returns an error if any source fails, the budget is zero, or pagination makes no progress.
    pub async fn get_all_trades(
        &self,
        query: TradesQuery,
        max_pages: u32,
    ) -> Result<Pages<Trade>, KalshiHttpError> {
        self.collect_pages(
            query,
            max_pages,
            |q| async move { self.get_trades(&q).await },
            |t| t.trade_id.clone(),
        )
        .await
    }

    /// Collects archived trades with stable-trade-identity deduplication.
    ///
    /// # Errors
    ///
    /// Returns an error if any source fails, the budget is zero, or pagination makes no progress.
    pub async fn get_all_historical_trades(
        &self,
        query: TradesQuery,
        max_pages: u32,
    ) -> Result<Pages<Trade>, KalshiHttpError> {
        self.collect_pages(
            query,
            max_pages,
            |q| async move { self.get_historical_trades(&q).await },
            |t| t.trade_id.clone(),
        )
        .await
    }

    /// Collects live fills, deduplicating by shard and fill identity.
    ///
    /// # Errors
    ///
    /// Returns an error if any source fails, the budget is zero, or pagination makes no progress.
    pub async fn get_all_fills(
        &self,
        query: FillsQuery,
        max_pages: u32,
    ) -> Result<Pages<Fill>, KalshiHttpError> {
        self.collect_pages(
            query,
            max_pages,
            |q| async move { self.get_fills(&q).await },
            |f| format!("{}:{}", f.exchange_index, f.fill_id),
        )
        .await
    }

    /// Collects archived fills, deduplicating by shard and fill identity.
    ///
    /// # Errors
    ///
    /// Returns an error if any source fails, the budget is zero, or pagination makes no progress.
    pub async fn get_all_historical_fills(
        &self,
        query: HistoryQuery,
        max_pages: u32,
    ) -> Result<Pages<Fill>, KalshiHttpError> {
        self.collect_pages(
            query,
            max_pages,
            |q| async move { self.get_historical_fills(&q).await },
            |f| format!("{}:{}", f.exchange_index, f.fill_id),
        )
        .await
    }

    /// Collects live order reports with stable-order-identity deduplication.
    ///
    /// # Errors
    ///
    /// Returns an error if any source fails, the budget is zero, or pagination makes no progress.
    pub async fn get_all_orders(
        &self,
        query: OrdersQuery,
        max_pages: u32,
    ) -> Result<Pages<Order>, KalshiHttpError> {
        self.collect_pages(
            query,
            max_pages,
            |q| async move { self.get_orders(&q).await },
            |o| o.order_id.clone(),
        )
        .await
    }

    /// Collects archived order reports with stable-order-identity deduplication.
    ///
    /// # Errors
    ///
    /// Returns an error if any source fails, the budget is zero, or pagination makes no progress.
    pub async fn get_all_historical_orders(
        &self,
        query: HistoryQuery,
        max_pages: u32,
    ) -> Result<Pages<Order>, KalshiHttpError> {
        self.collect_pages(
            query,
            max_pages,
            |q| async move { self.get_historical_orders(&q).await },
            |o| o.order_id.clone(),
        )
        .await
    }

    /// Collects live settlement records with shard/ticker deduplication.
    ///
    /// # Errors
    ///
    /// Returns an error if any source fails, the budget is zero, or pagination makes no progress.
    pub async fn get_all_settlements(
        &self,
        query: SettlementsQuery,
        max_pages: u32,
    ) -> Result<Pages<Settlement>, KalshiHttpError> {
        self.collect_pages(
            query,
            max_pages,
            |q| async move { self.get_settlements(&q).await },
            |s| format!("{}:{}", s.exchange_index, s.ticker),
        )
        .await
    }

    /// Collects live market and event positions without discarding either list.
    ///
    /// # Errors
    ///
    /// Returns an error if any source fails, the budget is zero, or pagination makes no progress.
    pub async fn get_all_positions(
        &self,
        query: PositionsQuery,
        max_pages: u32,
    ) -> Result<PositionPages, KalshiHttpError> {
        self.collect_positions(
            query,
            max_pages,
            |q| async move { self.get_positions(&q).await },
        )
        .await
    }

    /// Collects archived market and event positions; shard identity stays record-level.
    ///
    /// # Errors
    ///
    /// Returns an error if any source fails, the budget is zero, or pagination makes no progress.
    pub async fn get_all_historical_positions(
        &self,
        query: HistoricalPositionsQuery,
        max_pages: u32,
    ) -> Result<PositionPages, KalshiHttpError> {
        self.collect_positions(query, max_pages, |q| async move {
            self.get_historical_positions(&q).await
        })
        .await
    }

    async fn collect_positions<Q, F, Fut>(
        &self,
        mut query: Q,
        max_pages: u32,
        fetch: F,
    ) -> Result<PositionPages, KalshiHttpError>
    where
        Q: CursorQuery,
        F: Fn(Q) -> Fut,
        Fut: std::future::Future<Output = Result<PositionsResponse, KalshiHttpError>>,
    {
        if max_pages == 0 {
            return Err(KalshiHttpError::Validation("page budget must be positive"));
        }

        let mut pagination = Pagination::new(query.cursor());

        let mut result = PositionPages {
            market_positions: Vec::new(),
            event_positions: Vec::new(),
            complete: false,
            next_cursor: query.cursor().map(str::to_owned),
        };

        for _ in 0..max_pages {
            let page = fetch(query.clone()).await?;
            let empty = page.market_positions.is_empty() && page.event_positions.is_empty();
            let previous = result.market_positions.len() + result.event_positions.len();
            result
                .market_positions
                .extend(page.market_positions.into_iter().filter(|p| {
                    pagination.retain(format!("market:{}:{}", p.exchange_index, p.ticker))
                }));

            result.event_positions.extend(
                page.event_positions
                    .into_iter()
                    .filter(|p| pagination.retain(format!("event:{}", p.event_ticker))),
            );
            result.next_cursor = pagination.advance(page.cursor, empty)?;
            if result.next_cursor.is_none() {
                result.complete = true;
                break;
            }

            if result.market_positions.len() + result.event_positions.len() == previous {
                return Err(KalshiHttpError::Pagination(
                    "page contains no new identities",
                ));
            }

            query.set_cursor(result.next_cursor.clone());
        }

        Ok(result)
    }

    async fn collect_pages<Q, P, F, Fut>(
        &self,
        mut query: Q,
        max_pages: u32,
        fetch: F,
        identity: fn(&P::Item) -> String,
    ) -> Result<Pages<P::Item>, KalshiHttpError>
    where
        Q: CursorQuery,
        P: CursorPage,
        F: Fn(Q) -> Fut,
        Fut: std::future::Future<Output = Result<P, KalshiHttpError>>,
    {
        if max_pages == 0 {
            return Err(KalshiHttpError::Validation("page budget must be positive"));
        }

        let mut pagination = Pagination::new(query.cursor());

        let mut result = Pages {
            records: Vec::new(),
            complete: false,
            next_cursor: query.cursor().map(str::to_owned),
        };

        for _ in 0..max_pages {
            let (records, cursor) = fetch(query.clone()).await?.into_parts();
            let empty = records.is_empty();
            let previous = result.records.len();
            result.records.extend(
                records
                    .into_iter()
                    .filter(|r| pagination.retain(identity(r))),
            );
            result.next_cursor = pagination.advance(cursor, empty)?;
            if result.next_cursor.is_none() {
                result.complete = true;
                break;
            }

            if result.records.len() == previous {
                return Err(KalshiHttpError::Pagination(
                    "page contains no new identities",
                ));
            }

            query.set_cursor(result.next_cursor.clone());
        }

        Ok(result)
    }

    async fn get<Q: Serialize, R: DeserializeOwned>(
        &self,
        path: &[&str],
        query: &Q,
        authenticated: bool,
    ) -> Result<R, KalshiHttpError> {
        let response = Box::pin(self.send_raw(
            Method::GET,
            path,
            query,
            Option::<&()>::None,
            authenticated,
            Operation::Read,
        ))
        .await?;
        decode_response(response)
    }

    async fn send<Q: Serialize, B: Serialize, R: DeserializeOwned>(
        &self,
        method: Method,
        path: &[&str],
        query: &Q,
        body: Option<&B>,
        authenticated: bool,
    ) -> Result<R, KalshiHttpError> {
        let response = self
            .send_raw(method, path, query, body, authenticated, Operation::Write)
            .await?;
        decode_response(response)
    }

    async fn send_raw<Q: Serialize, B: Serialize>(
        &self,
        method: Method,
        path: &[&str],
        query: &Q,
        body: Option<&B>,
        authenticated: bool,
        operation: Operation,
    ) -> Result<HttpResponse, KalshiHttpError> {
        if self.cancellation.is_cancelled() {
            return Err(KalshiHttpError::Canceled { dispatched: false });
        }

        if authenticated && self.credential.is_none() {
            return Err(
                ConfigError::invalid_value("credentials", "authenticated client required").into(),
            );
        }

        if path.iter().any(|p| p.is_empty() || *p == "." || *p == "..") {
            return Err(KalshiHttpError::Validation(
                "empty or relative path component",
            ));
        }

        let mut url = Url::parse(self.base_url.expose_secret())
            .map_err(|_| KalshiHttpError::Validation("invalid base URL"))?;
        url.path_segments_mut()
            .map_err(|()| KalshiHttpError::Validation("invalid base URL"))?
            .pop_if_empty()
            .extend(path);
        let query = serde_urlencoded::to_string(query).map_err(|_| KalshiHttpError::Encoding)?;
        if !query.is_empty() {
            url.set_query(Some(&query));
        }

        let value = body
            .map(serde_json::to_value)
            .transpose()
            .map_err(|_| KalshiHttpError::Encoding)?;
        let charge = request_charge(&method, path, &query, value.as_ref(), operation)?;
        let body = value
            .map(stable_object_key_bytes)
            .transpose()
            .map_err(|_| KalshiHttpError::Encoding)?;
        let discovery = matches!(path, ["account", "limits" | "endpoint_costs"]);
        self.request(PreparedRequest {
            method,
            url,
            body,
            authenticated,
            operation,
            charge,
            discovery,
        })
        .await
    }

    async fn request(&self, request: PreparedRequest) -> Result<HttpResponse, KalshiHttpError> {
        let dispatched = AtomicBool::new(false);
        let last_response = Mutex::new(None);

        let retries = RetryManager::new(RetryConfig {
            max_retries: 2,
            initial_delay_ms: 250,
            max_delay_ms: 1_000,
            jitter_ms: 0,
            operation_timeout_ms: None,
            max_elapsed_ms: Some(self.timeout_ms.saturating_mul(3).saturating_add(2_000)),
            ..Default::default()
        });

        let result = retries
            .invocation(
                "Kalshi HTTP request",
                || async {
                    *last_response.lock().expect("response lock is not poisoned") = None;
                    let response = self.send_attempt(&request, &dispatched).await?;
                    if !response.status.is_success() {
                        let e = decode_response::<serde_json::Value>(response.clone())
                            .expect_err("non-success response is an error");

                        if e.is_retryable() && request.operation == Operation::Read {
                            *last_response.lock().expect("response lock is not poisoned") =
                                Some(response);
                            return Err(e);
                        }
                    }

                    Ok(response)
                },
                |e| request.operation == Operation::Read && e.is_retryable(),
                |reason| match reason {
                    RetryError::Canceled => KalshiHttpError::Canceled {
                        dispatched: dispatched.load(Ordering::Acquire),
                    },
                    reason => KalshiHttpError::Retry {
                        reason,
                        dispatched: dispatched.load(Ordering::Acquire),
                    },
                },
            )
            .cancellation_token(&self.cancellation)
            .execute()
            .await;

        let result = match result {
            Err(KalshiHttpError::Http { .. } | KalshiHttpError::Venue { .. }) => Ok(last_response
                .into_inner()
                .expect("response lock is not poisoned")
                .expect("classified response retained")),
            result => result,
        };

        result.map_err(|e| {
            if dispatched.load(Ordering::Acquire) && e.is_definitely_not_sent() {
                KalshiHttpError::AfterSend(Box::new(e))
            } else {
                e
            }
        })
    }

    async fn send_attempt(
        &self,
        request: &PreparedRequest,
        dispatched: &AtomicBool,
    ) -> Result<HttpResponse, KalshiHttpError> {
        loop {
            if !request.discovery && self.credential.is_some() && self.quota.needs_refresh() {
                Box::pin(self.refresh_quota())
                    .await
                    .map_err(|e| KalshiHttpError::Discovery(Box::new(e)))?;
            }

            match self.quota.acquire(&request.charge).await {
                Err(_)
                    if !request.discovery
                        && self.credential.is_some()
                        && self.quota.needs_refresh() => {}
                result => {
                    result?;
                    break;
                }
            }
        }

        let mut headers = HashMap::from([("Accept".into(), "application/json".into())]);
        if request.body.is_some() {
            headers.insert("Content-Type".into(), "application/json".into());
        }

        if request.authenticated {
            let credential = self
                .credential
                .as_ref()
                .expect("authentication checked before quota wait");
            headers.extend(
                credential
                    .headers(
                        get_atomic_clock_realtime().get_time_ms(),
                        request.method.as_str(),
                        request.url.path(),
                    )?
                    .into_iter()
                    .map(|(name, value)| (name, value.into_inner())),
            );
        }

        if self.cancellation.is_cancelled()
            || request.operation == Operation::Write
                && self.dispatch_check.as_ref().is_some_and(|check| !check())
        {
            return Err(KalshiHttpError::Canceled {
                dispatched: dispatched.load(Ordering::Acquire),
            });
        }

        dispatched.store(true, Ordering::Release);
        self.client
            .request(
                request.method.clone(),
                request.url.as_str().to_owned(),
                None,
                Some(headers),
                request.body.clone(),
                None,
                None,
            )
            .await
            .map_err(KalshiHttpError::Transport)
    }

    fn authenticated(
        config: &KalshiDataClientConfig,
        quota: Option<Arc<KalshiQuota>>,
    ) -> anyhow::Result<Self> {
        config.validate()?;
        let credential = KalshiCredential::resolve(
            config.api_key_id.clone(),
            config.private_key.clone(),
            config.environment,
        )?;

        let identity = config
            .api_key_id
            .clone()
            .or_else(|| {
                std::env::var(credential_env_vars(config.environment).0)
                    .ok()
                    .map(SecretString::from)
            })
            .ok_or_else(|| anyhow::anyhow!("missing quota credential identity"))?;

        let fingerprint = aws_lc_rs::digest::digest(
            &aws_lc_rs::digest::SHA256,
            identity.expose_secret().as_bytes(),
        )
        .as_ref()
        .to_vec();
        let origin = Url::parse(config.http_url())?
            .origin()
            .ascii_serialization();
        let key = (origin, fingerprint);
        let mut owners = QUOTAS.lock().expect("quota registry lock is not poisoned");
        if let (Some(existing), Some(requested)) = (owners.get(&key), quota.as_ref()) {
            anyhow::ensure!(
                Arc::ptr_eq(existing, requested),
                "credential already has a different quota owner"
            );
        }

        let quota = owners
            .entry(key)
            .or_insert_with(|| quota.unwrap_or_else(|| Arc::new(KalshiQuota::new())))
            .clone();
        drop(owners);
        let mut client = Self::build(config)?;
        client.credential = Some(Arc::new(credential));
        client.quota = quota;
        Ok(client)
    }

    fn build(config: &KalshiDataClientConfig) -> anyhow::Result<Self> {
        config.validate()?;
        let base_url = SecretString::from(config.http_url().trim_end_matches('/'));
        let client = HttpClient::builder()
            .timeout_secs(config.http_timeout_secs)
            .header_keys(vec!["retry-after".into(), "x-request-id".into()])
            .maybe_proxy_url(
                config
                    .proxy_url
                    .as_ref()
                    .map(|s| s.expose_secret().to_owned()),
            )
            .redirect_policy(HttpRedirectPolicy::Reject)
            .use_system_proxy(false)
            .redact_url(true)
            .build()
            .map_err(|_| anyhow::anyhow!("failed to build Kalshi HTTP transport"))?;
        Ok(Self {
            client,
            base_url,
            credential: None,
            cancellation: CancellationToken::new(),
            dispatch_check: None,
            quota: Arc::new(KalshiQuota::new()),
            timeout_ms: config.http_timeout_secs.saturating_mul(1_000),
        })
    }
}

type QuotaOwners = HashMap<(String, Vec<u8>), Arc<KalshiQuota>>;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Operation {
    Read,
    Write,
}

struct PreparedRequest {
    method: Method,
    url: Url,
    body: Option<Vec<u8>>,
    authenticated: bool,
    operation: Operation,
    charge: Charge,
    discovery: bool,
}

fn request_charge(
    method: &Method,
    path: &[&str],
    query: &str,
    body: Option<&serde_json::Value>,
    operation: Operation,
) -> Result<Charge, KalshiHttpError> {
    let batch =
        operation == Operation::Write && path == ["portfolio", "events", "orders", "batched"];

    let items = if batch {
        u32::try_from(
            body.and_then(|value| value.get("orders"))
                .and_then(serde_json::Value::as_array)
                .map_or(0, Vec::len),
        )
        .map_err(|_| KalshiHttpError::Quota("batch item count overflow"))?
    } else {
        1
    };

    let shard = body
        .and_then(|value| value.get("exchange_index"))
        .and_then(serde_json::Value::as_u64)
        .or_else(|| {
            url::form_urlencoded::parse(query.as_bytes())
                .find(|(name, _)| name == "exchange_index")
                .and_then(|(_, value)| value.parse().ok())
        });

    let bucket = match operation {
        Operation::Read if path == ["portfolio", "balance"] => Bucket::Balance,
        Operation::Read => Bucket::Read,
        Operation::Write if batch => Bucket::Write(0),
        Operation::Write => match shard {
            Some(shard) => Bucket::Write(
                u32::try_from(shard)
                    .map_err(|_| KalshiHttpError::Quota("invalid exchange shard"))?,
            ),
            None => Bucket::Auto,
        },
    };

    Ok(Charge {
        method: method.as_str().into(),
        path: format!("/trade-api/v2/{}", path.join("/")),
        bucket,
        items,
    })
}

// Workspace builds enable serde_json/preserve_order through another crate, so
// object key order would otherwise follow that feature.
/// Encodes one JSON value with object keys sorted at every level.
///
/// Workspace builds can enable `serde_json/preserve_order`. Sorted keys keep the encoded body
/// independent of map insertion order. The signer does not include the body.
///
/// # Errors
///
/// Returns an error if JSON encoding fails.
pub fn stable_object_key_bytes(value: serde_json::Value) -> Result<Vec<u8>, serde_json::Error> {
    serde_json::to_vec(&sorted_object_keys(value))
}

fn sorted_object_keys(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut entries: Vec<_> = map.into_iter().collect();
            entries.sort_by(|left, right| left.0.cmp(&right.0));
            serde_json::Value::Object(
                entries
                    .into_iter()
                    .map(|(key, child)| (key, sorted_object_keys(child)))
                    .collect(),
            )
        }
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(sorted_object_keys).collect())
        }
        value => value,
    }
}

trait CursorPage {
    type Item;

    fn into_parts(self) -> (Vec<Self::Item>, Field<String>);
}

macro_rules! cursor_page {
    ($($response:ty => $field:ident : $record:ty),+ $(,)?) => { $(
        impl CursorPage for $response {
            type Item = $record;
            fn into_parts(self) -> (Vec<Self::Item>, Field<String>) { (self.$field, self.cursor) }
        }
    )+ };
}

cursor_page!(MarketsResponse => markets: Market, EventsResponse => events: Event, TradesResponse => trades: Trade, FillsResponse => fills: Fill, OrdersResponse => orders: Order, SettlementsResponse => settlements: Settlement);

fn validate_primary(subaccount: u32) -> Result<(), KalshiHttpError> {
    if subaccount != 0 {
        return Err(KalshiHttpError::Validation(
            "only primary subaccount 0 is supported",
        ));
    }

    Ok(())
}

fn validate_candlesticks(query: &CandlesticksQuery) -> Result<(), KalshiHttpError> {
    if query.start_ts > query.end_ts {
        return Err(KalshiHttpError::Validation(
            "invalid candle window or period",
        ));
    }

    Ok(())
}

fn validate_order_values(
    ticker: &str,
    price: FixedPoint,
    count: FixedPoint,
) -> Result<(), KalshiHttpError> {
    use rust_decimal::Decimal;

    if ticker.is_empty()
        || price.0 < Decimal::ZERO
        || price.0 > Decimal::ONE
        || price.0.scale() > 4
        || count.0 <= Decimal::ZERO
        || count.0.scale() > 2
    {
        return Err(KalshiHttpError::Validation(
            "invalid ticker, price, or count",
        ));
    }

    Ok(())
}

/// Validates one primary-account V2 create request before it is encoded.
///
/// # Errors
///
/// Returns an error if the account, ticker, price, count, reduce-only flag, or expiration is
/// unsupported.
pub fn validate_create(request: &CreateOrderRequest) -> Result<(), KalshiHttpError> {
    validate_primary(request.subaccount)?;
    validate_order_values(&request.ticker, request.price, request.count)?;
    if request.reduce_only == Some(true) && request.time_in_force != TimeInForce::ImmediateOrCancel
    {
        return Err(KalshiHttpError::Validation(
            "reduce-only requires immediate-or-cancel",
        ));
    }

    if request.expiration_time.is_some() && request.time_in_force != TimeInForce::GoodTillCanceled {
        return Err(KalshiHttpError::Validation(
            "expiration requires good-till-canceled",
        ));
    }

    Ok(())
}

impl Debug for KalshiHttpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(KalshiHttpClient))
            .field("base_url", &self.base_url)
            .finish_non_exhaustive()
    }
}

#[derive(serde::Serialize)]
struct CancelQuery<'a> {
    #[serde(flatten)]
    account: &'a AccountQuery,
    #[serde(skip_serializing_if = "Option::is_none")]
    market_ticker: Option<&'a str>,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    #[cfg(not(all(feature = "simulation", madsim)))]
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    #[rstest]
    fn operation_routes_and_batch_counts_follow_wire_scope() {
        let requests: serde_json::Value =
            serde_json::from_str(include_str!("../../test_data/rest_requests.json")).unwrap();
        let single = request_charge(
            &Method::POST,
            &["portfolio", "events", "orders"],
            "",
            Some(&requests["create"]),
            Operation::Write,
        )
        .unwrap();
        assert_eq!(single.bucket, Bucket::Write(3));
        assert_eq!(single.items, 1);
        let batch = request_charge(
            &Method::POST,
            &["portfolio", "events", "orders", "batched"],
            "",
            Some(&requests["batch_create"]),
            Operation::Write,
        )
        .unwrap();
        assert_eq!(batch.bucket, Bucket::Write(0));
        assert_eq!(batch.items, 2);
        let cancel = request_charge(
            &Method::DELETE,
            &["portfolio", "events", "orders", "id"],
            "exchange_index=2",
            None,
            Operation::Write,
        )
        .unwrap();
        assert_eq!(cancel.bucket, Bucket::Write(2));
        assert_eq!(cancel.method, "DELETE");
        assert_eq!(cancel.path, "/trade-api/v2/portfolio/events/orders/id");
        let auto = request_charge(
            &Method::POST,
            &["portfolio", "events", "orders"],
            "",
            None,
            Operation::Write,
        )
        .unwrap();
        assert_eq!(auto.bucket, Bucket::Auto);
        let balance = request_charge(
            &Method::GET,
            &["portfolio", "balance"],
            "exchange_index=3",
            None,
            Operation::Read,
        )
        .unwrap();
        assert_eq!(balance.bucket, Bucket::Balance);
        let market = request_charge(
            &Method::GET,
            &["markets", "batched"],
            "",
            None,
            Operation::Read,
        )
        .unwrap();
        assert_eq!(market.method, "GET");
        assert_eq!(market.path, "/trade-api/v2/markets/batched");
        assert_eq!(market.bucket, Bucket::Read);
        assert_eq!(market.items, 1);
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[tokio::test]
    async fn write_dispatch_check_runs_after_quota_admission() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let permitted = Arc::new(AtomicBool::new(true));
        let check = Arc::clone(&permitted);

        let config = KalshiDataClientConfig {
            base_url_http: Some(format!("http://{}", listener.local_addr().unwrap()).into()),
            ..Default::default()
        };

        let client = KalshiHttpClient::new(&config)
            .unwrap()
            .with_dispatch_check(CancellationToken::new(), move || {
                check.load(Ordering::Acquire)
            });

        let mut fixtures: serde_json::Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        fixtures["limits"]["write"]["refill_rate"] = 1.into();
        fixtures["limits"]["write"]["bucket_capacity"] = 1.into();
        fixtures["costs"]["default_cost"] = 1.into();
        let limits = serde_json::from_value(fixtures["limits"].clone()).unwrap();
        let costs = serde_json::from_value(fixtures["costs"].clone()).unwrap();
        let unix_ms = i64::try_from(get_atomic_clock_realtime().get_time_ms()).unwrap();
        client.quota.set_limits(&limits, unix_ms).unwrap();
        client.quota.set_costs(costs).unwrap();
        let body = serde_json::from_str::<serde_json::Value>(include_str!(
            "../../test_data/rest_requests.json"
        ))
        .unwrap()["quota_batch"]
            .clone();
        let path = ["portfolio", "events", "orders", "batched"];
        let charge =
            request_charge(&Method::POST, &path, "", Some(&body), Operation::Write).unwrap();
        client.quota.acquire(&charge).await.unwrap();
        let request = client.send_raw(
            Method::POST,
            &path,
            &(),
            Some(&body),
            false,
            Operation::Write,
        );
        tokio::pin!(request);
        assert!(futures_util::poll!(request.as_mut()).is_pending());
        permitted.store(false, Ordering::Release);
        let result = tokio::select! {
            result = &mut request => result,
            _ = listener.accept() => panic!("Invalidated write reached the HTTP transport"),
        };
        assert!(matches!(
            result,
            Err(KalshiHttpError::Canceled { dispatched: false })
        ));
        permitted.store(true, Ordering::Release);

        let peer = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut data = [0_u8; 1024];
            let read = stream.read(&mut data).await.unwrap();
            assert!(
                std::str::from_utf8(&data[..read])
                    .unwrap()
                    .starts_with("POST /portfolio/events/orders/batched ")
            );
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
        });

        let response = client
            .send_raw(
                Method::POST,
                &path,
                &(),
                Some(&body),
                false,
                Operation::Write,
            )
            .await
            .unwrap();
        assert_eq!(response.status.as_u16(), 200);
        peer.await.unwrap();
    }
}
