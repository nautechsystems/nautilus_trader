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

//! Provides the HTTP client for the Polymarket Gamma API.
//!
//! Gamma keyset constraints honored by the paginators and `load_ids` chunker:
//!
//! - `/markets/keyset` accepts at most 100 items per page.
//! - `/events/keyset` accepts at most 500 items per page.
//! - Keyset endpoints reject `offset`; the paginators apply a requested initial
//!   offset locally for compatibility.
//! - `next_cursor` is absent on the final page.
//! - `condition_ids=` accepts at most 100 IDs per request, so `load_ids` for
//!   larger sets chunks the request and unions the responses.

use std::{collections::HashMap, future::Future, result::Result as StdResult, sync::Arc};

use ahash::AHashMap;
use jiff::Timestamp;
use nautilus_common::live::dst;
use nautilus_core::{
    UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::instruments::InstrumentAny;
use nautilus_network::{
    http::{
        HttpClient, HttpClientError, HttpResponse, Method, create_standard_nautilus_headers,
        parse_retry_after,
    },
    retry::{RetryConfig, RetryError, RetryManager},
    websocket::proxy::ProxyUrl,
};
use parking_lot::Mutex;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, value::RawValue};

use crate::{
    common::{parse::validate_market_version, urls::gamma_api_url},
    filters::set_market_closed,
    http::{
        error::{Error, Result, decode_response},
        models::{GammaEvent, GammaMarket, GammaTag, SearchResponse},
        pagination::{Completion, CursorProtocol, FetchOutcome, Paginator, WindowedCollect},
        parse::{
            create_instrument_from_def, enrich_market_fee_schedule,
            parse_gamma_market_with_event_cache,
        },
        query::{GetGammaEventsParams, GetGammaMarketsParams, GetSearchParams},
        rate_limits::{HEADER_RETRY_AFTER, POLYMARKET_GAMMA_REST_QUOTA},
    },
};

const GAMMA_MARKETS_KEYSET_PAGE_LIMIT: u32 = 100;
const GAMMA_EVENTS_KEYSET_PAGE_LIMIT: u32 = 500;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum GammaStop {
    CallerCapped,
}

/// Provides a raw HTTP client for the Polymarket Gamma API.
///
/// Handles HTTP transport for fetching market data from the public Gamma API.
/// Retries transient failures per request. No authentication is required.
/// Uses three retries with exponential backoff and a 180-second total budget.
/// The configured HTTP timeout bounds each attempt.
#[derive(Debug, Clone)]
pub struct PolymarketGammaRawHttpClient {
    client: HttpClient,
    base_url: String,
    retry_manager: Arc<RetryManager<Error>>,
}

impl PolymarketGammaRawHttpClient {
    /// Creates a new [`PolymarketGammaRawHttpClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn new(base_url: Option<String>, timeout_secs: u64) -> StdResult<Self, HttpClientError> {
        Self::new_with_proxy(base_url, timeout_secs, None)
    }

    /// Creates a new raw client with an optional validated proxy URL.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be created.
    pub fn new_with_proxy(
        base_url: Option<String>,
        timeout_secs: u64,
        proxy_url: Option<ProxyUrl>,
    ) -> StdResult<Self, HttpClientError> {
        Ok(Self {
            client: HttpClient::builder()
                .headers(Self::default_headers())
                .header_keys(vec![HEADER_RETRY_AFTER.to_string()])
                .default_quota(*POLYMARKET_GAMMA_REST_QUOTA)
                .timeout_secs(timeout_secs)
                .maybe_proxy_url(proxy_url.map(|url| url.expose().to_string()))
                .build()?,
            base_url: base_url
                .unwrap_or_else(|| gamma_api_url().to_string())
                .trim_end_matches('/')
                .to_string(),
            retry_manager: Arc::new(RetryManager::new(RetryConfig {
                jitter_ms: 1_000,
                operation_timeout_ms: None,
                max_elapsed_ms: Some(180_000),
                ..RetryConfig::default()
            })),
        })
    }

    fn default_headers() -> HashMap<String, String> {
        let mut headers: HashMap<String, String> =
            create_standard_nautilus_headers().into_iter().collect();
        headers.insert("Content-Type".to_string(), "application/json".to_string());
        headers
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base_url)
    }

    async fn send_get<P: Serialize, T: DeserializeOwned>(
        &self,
        path: &str,
        params: Option<&P>,
    ) -> Result<T> {
        self.send_request(path, || {
            let url = self.url(path);
            self.client
                .request_with_params(Method::GET, url, params, None, None, None, None)
        })
        .await
    }

    async fn send_get_query_map<T: DeserializeOwned>(
        &self,
        path: &str,
        params: Option<&HashMap<String, Vec<String>>>,
    ) -> Result<T> {
        self.send_request(path, || {
            let url = self.url(path);
            self.client
                .request(Method::GET, url, params, None, None, None, None)
        })
        .await
    }

    async fn send_request<T, F, Fut>(&self, path: &str, mut request: F) -> Result<T>
    where
        T: DeserializeOwned,
        F: FnMut() -> Fut,
        Fut: Future<Output = StdResult<HttpResponse, HttpClientError>>,
    {
        let retry_after = Mutex::new(None);

        // Bound the layout of retry futures nested in instrument discovery
        Box::pin(
            self.retry_manager
                .invocation(
                    path,
                    || {
                        let response = request();
                        async {
                            *retry_after.lock() = None;
                            let response = response.await.map_err(Error::from_http_client)?;
                            *retry_after.lock() = response
                                .headers
                                .get(HEADER_RETRY_AFTER)
                                .and_then(|value| parse_retry_after(value, Timestamp::now()))
                                .filter(|delay| {
                                    dst::time::Instant::now().checked_add(*delay).is_some()
                                });
                            decode_response(&response)
                        }
                    },
                    Error::is_retryable,
                    |e| match e {
                        RetryError::OperationTimeout { .. } => Error::Timeout,
                        RetryError::InvalidConfiguration { message } => Error::bad_request(message),
                        e => Error::transport(e.to_string()),
                    },
                )
                .retry_delay(&|_| *retry_after.lock())
                .execute(),
        )
        .await
    }

    /// Fetches markets from the Gamma API.
    ///
    /// Handles both bare array and `{"data": [...]}` response schemas.
    pub async fn get_gamma_markets(
        &self,
        params: GetGammaMarketsParams,
    ) -> Result<Vec<GammaMarket>> {
        let query_params = gamma_markets_query_params(params)?;
        let raw: Box<RawValue> = self
            .send_get_query_map("/markets", Some(&query_params))
            .await?;
        parse_gamma_markets_response(&raw)
    }

    async fn get_gamma_markets_keyset(
        &self,
        mut params: GetGammaMarketsParams,
        after_cursor: Option<&str>,
    ) -> Result<GammaMarketsKeysetResponse> {
        params.validate_keyset().map_err(Error::decode)?;
        params.offset = None;
        let mut query_params = gamma_markets_query_params(params)?;
        if let Some(after_cursor) = after_cursor {
            query_params.insert("after_cursor".to_string(), vec![after_cursor.to_string()]);
        }
        self.send_get_query_map("/markets/keyset", Some(&query_params))
            .await
    }

    /// Fetches a single market by ID from the Gamma API.
    pub async fn get_gamma_market(&self, market_id: &str) -> Result<GammaMarket> {
        let path = format!("/markets/{market_id}");
        self.send_get::<(), _>(&path, None::<&()>).await
    }

    /// Fetches a market from the Gamma API `GET /markets/slug/{slug}`.
    pub async fn get_gamma_market_by_slug(&self, slug: &str) -> Result<GammaMarket> {
        let path = format!("/markets/slug/{slug}");
        self.send_get::<(), _>(&path, None::<&()>).await
    }

    /// Fetches events from the Gamma API `GET /events?slug=`.
    pub async fn get_gamma_events_by_slug(&self, slug: &str) -> Result<Vec<GammaEvent>> {
        #[derive(Serialize)]
        struct EventSlugParams<'a> {
            slug: &'a str,
        }
        let params = EventSlugParams { slug };
        self.send_get("/events", Some(&params)).await
    }

    /// Fetches events from the Gamma API `GET /events` with full query params.
    pub async fn get_gamma_events(&self, params: GetGammaEventsParams) -> Result<Vec<GammaEvent>> {
        let query_params = gamma_events_query_params(params)?;
        self.send_get_query_map("/events", Some(&query_params))
            .await
    }

    async fn get_gamma_events_keyset(
        &self,
        mut params: GetGammaEventsParams,
        after_cursor: Option<&str>,
    ) -> Result<GammaEventsKeysetResponse> {
        params.validate_keyset().map_err(Error::decode)?;
        params.offset = None;
        let mut query_params = gamma_events_query_params(params)?;
        if let Some(after_cursor) = after_cursor {
            query_params.insert("after_cursor".to_string(), vec![after_cursor.to_string()]);
        }
        self.send_get_query_map("/events/keyset", Some(&query_params))
            .await
    }

    /// Fetches available tags from the Gamma API `GET /tags`.
    pub async fn get_gamma_tags(&self) -> Result<Vec<GammaTag>> {
        self.send_get::<(), _>("/tags", None::<&()>).await
    }

    /// Searches the Gamma API via `GET /public-search`.
    pub async fn get_public_search(&self, params: GetSearchParams) -> Result<SearchResponse> {
        self.send_get("/public-search", Some(&params)).await
    }
}

#[derive(Debug, Deserialize)]
struct GammaMarketsKeysetResponse {
    markets: Vec<GammaMarket>,
    next_cursor: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GammaEventsKeysetResponse {
    events: Vec<GammaEvent>,
    next_cursor: Option<String>,
}

fn gamma_markets_query_params(
    params: GetGammaMarketsParams,
) -> Result<HashMap<String, Vec<String>>> {
    let mut scalar_params = params;
    let id = scalar_params.id.take();
    let slug = scalar_params.slug.take();
    let clob_token_ids = scalar_params.clob_token_ids.take();
    let condition_ids = scalar_params.condition_ids.take();
    let question_ids = scalar_params.question_ids.take();
    let market_maker_address = scalar_params.market_maker_address.take();
    let tag_id = scalar_params.tag_id.take();
    let sports_market_types = scalar_params.sports_market_types.take();
    let value = serde_json::to_value(&scalar_params).map_err(Error::Serde)?;
    let fields = value
        .as_object()
        .ok_or_else(|| Error::decode("Gamma markets params must encode to an object"))?;
    let mut params = HashMap::with_capacity(fields.len());

    for (key, value) in fields {
        if let Some(value) = gamma_query_value(value)? {
            params.insert(key.clone(), vec![value]);
        }
    }

    insert_repeated_param(&mut params, "id", id);
    insert_repeated_param(&mut params, "slug", slug);
    insert_repeated_param(&mut params, "clob_token_ids", clob_token_ids);
    insert_repeated_param(&mut params, "condition_ids", condition_ids);
    insert_repeated_param(&mut params, "question_ids", question_ids);
    insert_repeated_param(&mut params, "market_maker_address", market_maker_address);
    insert_repeated_param(&mut params, "tag_id", tag_id);
    insert_repeated_param(&mut params, "sports_market_types", sports_market_types);

    Ok(params)
}

fn gamma_events_query_params(params: GetGammaEventsParams) -> Result<HashMap<String, Vec<String>>> {
    let mut scalar_params = params;
    let id = scalar_params.id.take();
    let slug = scalar_params.slug.take();
    let tag_id = scalar_params.tag_id.take();
    let exclude_tag_id = scalar_params.exclude_tag_id.take();
    let series_id = scalar_params.series_id.take();
    let game_id = scalar_params.game_id.take();
    let created_by = scalar_params.created_by.take();
    let value = serde_json::to_value(&scalar_params).map_err(Error::Serde)?;
    let fields = value
        .as_object()
        .ok_or_else(|| Error::decode("Gamma events params must encode to an object"))?;
    let mut params = HashMap::with_capacity(fields.len());

    for (key, value) in fields {
        if let Some(value) = gamma_query_value(value)? {
            params.insert(key.clone(), vec![value]);
        }
    }

    insert_repeated_param(&mut params, "id", id);
    insert_repeated_param(&mut params, "slug", slug);
    insert_repeated_param(&mut params, "tag_id", tag_id);
    insert_repeated_param(&mut params, "exclude_tag_id", exclude_tag_id);
    insert_repeated_param(&mut params, "series_id", series_id);
    insert_repeated_param(&mut params, "game_id", game_id);
    insert_repeated_param(&mut params, "created_by", created_by);

    Ok(params)
}

fn insert_repeated_param<T: ToString>(
    params: &mut HashMap<String, Vec<String>>,
    key: &str,
    values: Option<Vec<T>>,
) {
    let Some(values) = values else {
        return;
    };

    params.insert(
        key.to_string(),
        values
            .into_iter()
            .map(|value| value.to_string().trim().to_string())
            .collect(),
    );
}

fn gamma_query_value(value: &Value) -> Result<Option<String>> {
    match value {
        Value::Null => Ok(None),
        Value::String(value) => Ok(Some(value.clone())),
        Value::Bool(value) => Ok(Some(value.to_string())),
        Value::Number(value) => Ok(Some(value.to_string())),
        other => Err(Error::decode(format!(
            "Unsupported Gamma query value: {other}"
        ))),
    }
}

fn parse_markets_to_instruments(markets: &[GammaMarket], ts_init: UnixNanos) -> Vec<InstrumentAny> {
    let (instruments, _transient) = parse_markets_with_transient(markets, ts_init);
    instruments
}

// Returns parsed instruments alongside condition IDs of markets still in the
// CLOB hydration window (empty or empty-entry `clob_token_ids`), so callers
// can retry rather than treating them as terminal.
//
// This is the single funnel through which live instruments reach the client caches, so Gamma's
// `closed` state is recorded here rather than in `create_instrument_from_def`. Historical loader
// instruments share that constructor and must not carry terminal state in `info`; they expose it
// through `resolution_metadata` instead.
pub(crate) fn parse_markets_with_transient(
    markets: &[GammaMarket],
    ts_init: UnixNanos,
) -> (Vec<InstrumentAny>, Vec<String>) {
    let mut instruments = Vec::new();
    let mut transient = Vec::new();

    // Parents outlive this batch; pointer keys keep distinct discovery snapshots separate
    let mut event_metadata = AHashMap::new();

    for market in markets {
        if let Err(e) = validate_market_version(market.version.as_deref()) {
            log::warn!("Skipping Gamma market {}: {e}", market.condition_id);
            continue;
        }

        if is_transient_clob_token_ids(&market.clob_token_ids) {
            transient.push(market.condition_id.clone());
            continue;
        }

        match parse_gamma_market_with_event_cache(market, &mut event_metadata) {
            Ok(defs) => {
                for def in defs {
                    match create_instrument_from_def(&def, ts_init) {
                        Ok(InstrumentAny::BinaryOption(mut binary)) => {
                            set_market_closed(&mut binary, def.closed);
                            instruments.push(InstrumentAny::BinaryOption(binary));
                        }
                        Ok(other) => instruments.push(other),
                        Err(e) => log::warn!("Failed to create instrument: {e}"),
                    }
                }
            }
            Err(e) => log::warn!("Failed to parse gamma market: {e}"),
        }
    }

    if !transient.is_empty() {
        log::debug!(
            "{} market(s) without usable clob_token_ids deferred as transient (CLOB hydration)",
            transient.len(),
        );
    }
    (instruments, transient)
}

// Treats bare empty string, encoded empty array, and arrays with empty entries
// as transient. Unparsable payloads fall through to `parse_gamma_market` so
// real schema errors still surface.
fn is_transient_clob_token_ids(raw: &str) -> bool {
    if raw.is_empty() {
        return true;
    }

    match serde_json::from_str::<Vec<String>>(raw) {
        Ok(ids) => ids.is_empty() || ids.iter().any(|t| t.is_empty()),
        Err(_) => false,
    }
}

pub(crate) fn flatten_event_markets(events: Vec<GammaEvent>) -> Vec<GammaMarket> {
    events
        .into_iter()
        .flat_map(|mut event| {
            let markets = std::mem::take(&mut event.markets);
            let event = Arc::new(event);

            markets.into_iter().map(move |mut market| {
                if market.game_id.is_none() {
                    market.game_id.clone_from(&event.game_id);
                }

                market.parent_event = Some(event.clone());
                market
            })
        })
        .collect()
}

/// Provides a domain HTTP client for Polymarket instrument fetching.
///
/// Wraps [`PolymarketGammaRawHttpClient`] with instrument parsing: fetch from
/// the Gamma API and parse into Nautilus types. Stateless with respect to
/// instrument storage; caching is handled by the instrument provider.
#[derive(Debug, Clone)]
pub struct PolymarketGammaHttpClient {
    inner: Arc<PolymarketGammaRawHttpClient>,
    clock: &'static AtomicTime,
    retry_manager: Arc<RetryManager<Error>>,
}

impl PolymarketGammaHttpClient {
    /// Creates a new [`PolymarketGammaHttpClient`].
    ///
    /// `retry_config` controls indexing-lag retries, independently of HTTP retries.
    /// `retry_config.operation_timeout_ms` is ignored; `timeout_secs` bounds each HTTP attempt.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be created.
    pub fn new(
        gamma_base_url: Option<String>,
        timeout_secs: u64,
        retry_config: RetryConfig,
    ) -> StdResult<Self, HttpClientError> {
        Self::new_with_proxy(gamma_base_url, timeout_secs, retry_config, None)
    }

    /// Creates a new domain client with an optional validated proxy URL.
    ///
    /// `retry_config` controls indexing-lag retries, independently of HTTP retries.
    /// `retry_config.operation_timeout_ms` is ignored; `timeout_secs` bounds each HTTP attempt.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be created.
    pub fn new_with_proxy(
        gamma_base_url: Option<String>,
        timeout_secs: u64,
        mut retry_config: RetryConfig,
        proxy_url: Option<ProxyUrl>,
    ) -> StdResult<Self, HttpClientError> {
        // HTTP attempts own their timeouts; indexing retries include HTTP backoff
        retry_config.operation_timeout_ms = None;

        Ok(Self {
            inner: Arc::new(PolymarketGammaRawHttpClient::new_with_proxy(
                gamma_base_url,
                timeout_secs,
                proxy_url,
            )?),
            clock: get_atomic_clock_realtime(),
            retry_manager: Arc::new(RetryManager::new(retry_config)),
        })
    }

    /// Fetches markets from the Gamma API with the given base params, paginating automatically.
    async fn fetch_gamma_markets_paginated(
        &self,
        base_params: GetGammaMarketsParams,
    ) -> anyhow::Result<Vec<GammaMarket>> {
        let page_size = base_params
            .limit
            .unwrap_or(GAMMA_MARKETS_KEYSET_PAGE_LIMIT)
            .min(GAMMA_MARKETS_KEYSET_PAGE_LIMIT);
        let protocol = CursorProtocol::<GammaStop>::gamma("Gamma market");
        let reducer = WindowedCollect::new(
            base_params.offset.unwrap_or(0) as usize,
            base_params.max_markets.map(|value| value as usize),
            GammaStop::CallerCapped,
        );
        let paginator = Paginator::new("Gamma market", protocol, reducer);
        let completed = paginator
            .run(
                |position| {
                    let after_cursor = position.map(|cursor| cursor.as_ref().to_string());
                    let params = GetGammaMarketsParams {
                        limit: Some(page_size),
                        offset: None,
                        ..base_params.clone()
                    };
                    async move {
                        let response = self
                            .inner
                            .get_gamma_markets_keyset(params, after_cursor.as_deref())
                            .await?;
                        Ok::<_, anyhow::Error>(FetchOutcome::Page {
                            rows: response.markets,
                            wire: response.next_cursor,
                        })
                    }
                },
                anyhow::Error::new,
            )
            .await?;

        match completed.completion {
            Completion::WireExhausted | Completion::Stopped(GammaStop::CallerCapped) => {
                Ok(completed.output)
            }
        }
    }

    /// Fetches all active markets from the Gamma API, paginating automatically.
    async fn fetch_all_gamma_markets(&self) -> anyhow::Result<Vec<GammaMarket>> {
        self.fetch_gamma_markets_paginated(GetGammaMarketsParams {
            active: Some(true),
            closed: Some(false),
            ..Default::default()
        })
        .await
    }

    /// Fetches instruments from the Gamma API and returns Nautilus domain types.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request or parsing fails.
    pub async fn request_instruments(&self) -> anyhow::Result<Vec<InstrumentAny>> {
        let mut markets = self.fetch_all_gamma_markets().await?;
        enrich_markets(&mut markets);
        let ts_init = self.clock.get_time_ns();
        let instruments = parse_markets_to_instruments(&markets, ts_init);
        log::debug!("Parsed {} instruments from Gamma API", instruments.len());
        Ok(instruments)
    }

    /// Fetches instruments for the given slugs concurrently.
    ///
    /// Each slug is queried individually via the Gamma API. Missing or
    /// unparsable slugs are logged and skipped.
    ///
    /// # Errors
    ///
    /// Returns an error if all slug requests fail. Individual slug failures
    /// are warned and skipped when at least one slug succeeds.
    pub async fn request_instruments_by_slugs(
        &self,
        slugs: Vec<String>,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let ts_init = self.clock.get_time_ns();

        let futures = slugs.into_iter().map(|slug| {
            let inner = Arc::clone(&self.inner);
            async move {
                let params = GetGammaMarketsParams {
                    slug: Some(vec![slug.clone()]),
                    ..Default::default()
                };

                match inner.get_gamma_markets(params).await {
                    Ok(markets) => Some((slug, markets)),
                    Err(e) => {
                        log::warn!("Failed to fetch slug '{slug}': {e}");
                        None
                    }
                }
            }
        });

        let results = futures_util::future::join_all(futures).await;

        let total_slugs = results.len();
        let succeeded = results.iter().filter(|r| r.is_some()).count();
        let mut instruments = Vec::new();

        for result in results.into_iter().flatten() {
            let (slug, mut markets) = result;
            if markets.is_empty() {
                log::debug!("No markets found for slug '{slug}'");
                continue;
            }

            enrich_markets(&mut markets);
            instruments.extend(parse_markets_to_instruments(&markets, ts_init));
        }

        if succeeded == 0 && total_slugs > 0 {
            anyhow::bail!("All {total_slugs} slug requests failed");
        }

        log::debug!("Parsed {} instruments from slug queries", instruments.len());
        Ok(instruments)
    }

    /// Fetches instruments for the given slugs with retry on empty results.
    ///
    /// Uses exponential backoff for indexing lag. Each HTTP request independently
    /// retries transient failures, without retrying exhausted HTTP errors at the batch level.
    pub async fn request_instruments_by_slugs_with_retry(
        &self,
        slugs: Vec<String>,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let inner = Arc::clone(&self.inner);
        let ts_init = self.clock.get_time_ns();

        let mut markets: Vec<GammaMarket> = self
            .retry_manager
            .invocation(
                "gamma_fetch_by_slugs",
                || {
                    let inner = Arc::clone(&inner);
                    let slugs = slugs.clone();
                    async move {
                        let futures = slugs.into_iter().map(|slug| {
                            let inner = Arc::clone(&inner);
                            async move {
                                let params = GetGammaMarketsParams {
                                    slug: Some(vec![slug.clone()]),
                                    ..Default::default()
                                };
                                inner
                                    .get_gamma_markets(params)
                                    .await
                                    .map(|markets| (slug, markets))
                            }
                        });

                        let results: Vec<_> = futures_util::future::join_all(futures)
                            .await
                            .into_iter()
                            .collect::<StdResult<Vec<_>, _>>()?;

                        let markets: Vec<GammaMarket> = results
                            .into_iter()
                            .flat_map(|(_, markets)| markets)
                            .collect();

                        if parse_markets_to_instruments(&markets, ts_init).is_empty()
                            && (markets.is_empty()
                                || markets.iter().any(|market| {
                                    validate_market_version(market.version.as_deref()).is_ok()
                                }))
                        {
                            return Err(Error::exchange(
                                "Gamma returned no instruments (indexing lag)",
                            ));
                        }

                        Ok(markets)
                    }
                },
                |e| matches!(e, Error::Exchange(_)),
                |e| Error::transport(e.to_string()),
            )
            .execute()
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        enrich_markets(&mut markets);
        Ok(parse_markets_to_instruments(&markets, ts_init))
    }

    /// Fetches instruments from event slugs concurrently.
    ///
    /// Each slug queries `GET /events?slug=`, extracts the markets array from
    /// the first matching event, and parses each market into instruments.
    pub async fn request_instruments_by_event_slugs(
        &self,
        event_slugs: Vec<String>,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let ts_init = self.clock.get_time_ns();

        let futures = event_slugs.into_iter().map(|slug| {
            let inner = Arc::clone(&self.inner);
            async move {
                match inner.get_gamma_events_by_slug(&slug).await {
                    Ok(events) => Some((slug, events)),
                    Err(e) => {
                        log::warn!("Failed to fetch event slug '{slug}': {e}");
                        None
                    }
                }
            }
        });

        let results = futures_util::future::join_all(futures).await;

        let total = results.len();
        let succeeded = results.iter().filter(|r| r.is_some()).count();
        let mut instruments = Vec::new();

        for result in results.into_iter().flatten() {
            let (slug, events) = result;
            let mut markets = flatten_event_markets(events);
            if markets.is_empty() {
                log::warn!("No markets found in event slug '{slug}'");
                continue;
            }

            enrich_markets(&mut markets);
            instruments.extend(parse_markets_to_instruments(&markets, ts_init));
        }

        if succeeded == 0 && total > 0 {
            anyhow::bail!("All {total} event slug requests failed");
        }

        log::debug!(
            "Parsed {} instruments from event slug queries",
            instruments.len()
        );
        Ok(instruments)
    }

    /// Fetches instruments using arbitrary Gamma API query params with auto-pagination.
    pub async fn request_instruments_by_params(
        &self,
        base_params: GetGammaMarketsParams,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let mut markets = self.fetch_gamma_markets_paginated(base_params).await?;
        enrich_markets(&mut markets);
        let ts_init = self.clock.get_time_ns();
        let instruments = parse_markets_to_instruments(&markets, ts_init);
        log::debug!("Parsed {} instruments from params query", instruments.len());
        Ok(instruments)
    }

    /// Fetches supported instruments and condition IDs awaiting CLOB token IDs.
    ///
    /// Only market protocol `v1` enters the transient list. Unlike
    /// [`Self::request_instruments_by_params`], this method errors when every returned
    /// market uses an unsupported protocol.
    ///
    /// # Errors
    ///
    /// Returns an error if the request fails or every returned market is unsupported.
    pub async fn request_instruments_by_params_with_transient(
        &self,
        base_params: GetGammaMarketsParams,
    ) -> anyhow::Result<(Vec<InstrumentAny>, Vec<String>)> {
        let mut markets = self.fetch_gamma_markets_paginated(base_params).await?;
        if !markets.is_empty()
            && markets
                .iter()
                .all(|market| validate_market_version(market.version.as_deref()).is_err())
        {
            validate_market_version(markets[0].version.as_deref())?;
        }

        enrich_markets(&mut markets);
        let ts_init = self.clock.get_time_ns();
        let (instruments, transient) = parse_markets_with_transient(&markets, ts_init);
        log::debug!(
            "Parsed {} instruments and {} transient condition_id(s) from params query",
            instruments.len(),
            transient.len(),
        );
        Ok((instruments, transient))
    }

    /// Fetches raw Gamma markets using arbitrary query params with auto-pagination.
    pub async fn request_markets_by_params(
        &self,
        base_params: GetGammaMarketsParams,
    ) -> anyhow::Result<Vec<GammaMarket>> {
        self.fetch_gamma_markets_paginated(base_params).await
    }

    /// Fetches instruments from an event's embedded markets with client-side sorting and limiting.
    ///
    /// Only these `GetGammaMarketsParams` fields are supported:
    /// - `order`: `"liquidity"`, `"volume"`, `"volume24hr"`, `"competitive"`,
    ///   `"spread"`, `"best_bid"`, `"one_day_price_change"`, or `"volume_1wk"`.
    /// - `ascending`: sort direction (default: descending); unused without `order`.
    /// - `max_markets`: truncate after sorting; without `order`, retain event order.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - A parameter or order field is unsupported, rejected before fetching.
    /// - The event request fails.
    pub async fn request_instruments_by_event_query(
        &self,
        event_slug: &str,
        params: GetGammaMarketsParams,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let unsupported = GetGammaMarketsParams {
            order: None,
            ascending: None,
            max_markets: None,
            ..params.clone()
        };

        let unsupported = serde_json::to_value(unsupported)?;
        if let Some(field) = unsupported
            .as_object()
            .and_then(|fields| fields.keys().next())
        {
            anyhow::bail!("Unsupported event query parameter '{field}'");
        }

        let compare: Option<fn(&GammaMarket, &GammaMarket) -> std::cmp::Ordering> =
            if let Some(order_field) = params.order.as_deref() {
                Some(match order_field {
                    "liquidity" => |a, b| {
                        a.liquidity_num
                            .unwrap_or(Decimal::ZERO)
                            .cmp(&b.liquidity_num.unwrap_or(Decimal::ZERO))
                    },
                    "volume" => |a, b| {
                        a.volume_num
                            .unwrap_or(Decimal::ZERO)
                            .cmp(&b.volume_num.unwrap_or(Decimal::ZERO))
                    },
                    "volume24hr" => |a, b| {
                        a.volume_24hr
                            .unwrap_or(Decimal::ZERO)
                            .cmp(&b.volume_24hr.unwrap_or(Decimal::ZERO))
                    },
                    "competitive" => |a, b| {
                        a.competitive
                            .unwrap_or(0.0)
                            .partial_cmp(&b.competitive.unwrap_or(0.0))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    },
                    "spread" => |a, b| {
                        a.spread
                            .unwrap_or(Decimal::MAX)
                            .cmp(&b.spread.unwrap_or(Decimal::MAX))
                    },
                    "best_bid" => |a, b| {
                        a.best_bid
                            .unwrap_or(Decimal::ZERO)
                            .cmp(&b.best_bid.unwrap_or(Decimal::ZERO))
                    },
                    "one_day_price_change" => |a, b| {
                        a.one_day_price_change
                            .unwrap_or(Decimal::ZERO)
                            .cmp(&b.one_day_price_change.unwrap_or(Decimal::ZERO))
                    },
                    "volume_1wk" => |a, b| {
                        a.volume_1wk
                            .unwrap_or(Decimal::ZERO)
                            .cmp(&b.volume_1wk.unwrap_or(Decimal::ZERO))
                    },
                    _ => anyhow::bail!("Unsupported event query order field '{order_field}'"),
                })
            } else {
                None
            };

        let events = self.inner.get_gamma_events_by_slug(event_slug).await?;
        let mut markets = flatten_event_markets(events);

        if markets.is_empty() {
            log::warn!("No markets found in event slug '{event_slug}'");
            return Ok(Vec::new());
        }

        log::debug!("Event '{event_slug}' returned {} markets", markets.len());

        if let Some(compare) = compare {
            let ascending = params.ascending.unwrap_or(false);
            markets.sort_by(|a, b| {
                let cmp = compare(a, b);
                if ascending { cmp } else { cmp.reverse() }
            });
        }

        if let Some(cap) = params.max_markets {
            markets.truncate(cap as usize);
        }

        enrich_markets(&mut markets);
        let ts_init = self.clock.get_time_ns();
        let instruments = parse_markets_to_instruments(&markets, ts_init);
        log::debug!(
            "Parsed {} instruments from event query '{event_slug}'",
            instruments.len()
        );
        Ok(instruments)
    }

    /// Fetches events from the Gamma API with the given base params, paginating automatically.
    async fn fetch_gamma_events_paginated(
        &self,
        base_params: GetGammaEventsParams,
    ) -> anyhow::Result<Vec<GammaEvent>> {
        let page_size = base_params
            .limit
            .unwrap_or(GAMMA_EVENTS_KEYSET_PAGE_LIMIT)
            .min(GAMMA_EVENTS_KEYSET_PAGE_LIMIT);
        let protocol = CursorProtocol::<GammaStop>::gamma("Gamma event");
        let reducer = WindowedCollect::new(
            base_params.offset.unwrap_or(0) as usize,
            base_params.max_events.map(|value| value as usize),
            GammaStop::CallerCapped,
        );
        let paginator = Paginator::new("Gamma event", protocol, reducer);
        let completed = paginator
            .run(
                |position| {
                    let after_cursor = position.map(|cursor| cursor.as_ref().to_string());
                    let params = GetGammaEventsParams {
                        limit: Some(page_size),
                        offset: None,
                        ..base_params.clone()
                    };
                    async move {
                        let response = self
                            .inner
                            .get_gamma_events_keyset(params, after_cursor.as_deref())
                            .await?;
                        Ok::<_, anyhow::Error>(FetchOutcome::Page {
                            rows: response.events,
                            wire: response.next_cursor,
                        })
                    }
                },
                anyhow::Error::new,
            )
            .await?;

        match completed.completion {
            Completion::WireExhausted | Completion::Stopped(GammaStop::CallerCapped) => {
                Ok(completed.output)
            }
        }
    }

    /// Fetches instruments from events matching full query params (paginated).
    pub async fn request_instruments_by_event_params(
        &self,
        params: GetGammaEventsParams,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let events = self.fetch_gamma_events_paginated(params).await?;
        let ts_init = self.clock.get_time_ns();
        let total_events = events.len();
        let mut markets = flatten_event_markets(events);
        let total_markets = markets.len();
        enrich_markets(&mut markets);
        let instruments = parse_markets_to_instruments(&markets, ts_init);
        log::debug!(
            "Parsed {} instruments from {total_events} events ({total_markets} markets)",
            instruments.len(),
        );
        Ok(instruments)
    }

    /// Fetches raw Gamma events using arbitrary query params with auto-pagination.
    pub async fn request_events_by_params(
        &self,
        params: GetGammaEventsParams,
    ) -> anyhow::Result<Vec<GammaEvent>> {
        self.fetch_gamma_events_paginated(params).await
    }

    /// Searches for instruments via the Gamma public search endpoint.
    pub async fn request_instruments_by_search(
        &self,
        params: GetSearchParams,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let response = self.inner.get_public_search(params).await?;
        let ts_init = self.clock.get_time_ns();

        let mut instruments = Vec::new();

        if let Some(markets) = response.markets {
            let mut markets = markets;
            enrich_markets(&mut markets);
            instruments.extend(parse_markets_to_instruments(&markets, ts_init));
        }

        if let Some(events) = &response.events {
            let mut event_markets = flatten_event_markets(events.clone());
            enrich_markets(&mut event_markets);
            instruments.extend(parse_markets_to_instruments(&event_markets, ts_init));
        }

        log::debug!("Parsed {} instruments from search query", instruments.len());
        Ok(instruments)
    }

    /// Fetches available tags from the Gamma API.
    pub async fn request_tags(&self) -> anyhow::Result<Vec<GammaTag>> {
        Ok(self.inner.get_gamma_tags().await?)
    }

    /// Returns a reference to the underlying raw HTTP client.
    #[must_use]
    pub fn inner(&self) -> &Arc<PolymarketGammaRawHttpClient> {
        &self.inner
    }
}

fn enrich_markets(markets: &mut [GammaMarket]) {
    for market in markets {
        enrich_market_fee_schedule(market);
    }
}

fn parse_gamma_markets_response(raw: &RawValue) -> Result<Vec<GammaMarket>> {
    #[derive(Deserialize)]
    struct MarketsEnvelope {
        data: Vec<GammaMarket>,
    }

    if raw.get().starts_with('[') {
        return serde_json::from_str(raw.get()).map_err(Error::Serde);
    }
    serde_json::from_str::<MarketsEnvelope>(raw.get())
        .map(|envelope| envelope.data)
        .map_err(Error::Serde)
}

#[cfg(test)]
mod tests {
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        time::Duration,
    };

    use axum::{Router, http::StatusCode, response::IntoResponse, routing::get};
    use nautilus_common::cache::Cache;
    use nautilus_model::instruments::Instrument;
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::http::{parse::parse_gamma_market, query::GetGammaMarketsParamsBuilder};

    async fn retry_client(
        router: Router,
    ) -> (PolymarketGammaHttpClient, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let retry_config = RetryConfig {
            initial_delay_ms: 10,
            max_delay_ms: 10,
            jitter_ms: 0,
            operation_timeout_ms: None,
            ..RetryConfig::default()
        };
        let mut client = PolymarketGammaHttpClient::new(
            Some(format!("http://{addr}")),
            1,
            RetryConfig {
                max_retries: 1,
                operation_timeout_ms: Some(100),
                ..retry_config
            },
        )
        .unwrap();
        Arc::get_mut(&mut client.inner).unwrap().retry_manager =
            Arc::new(RetryManager::new(retry_config));
        (client, server)
    }

    #[rstest]
    #[tokio::test(start_paused = true)]
    async fn test_http_policy_is_independent_of_indexing_policy() {
        let client = PolymarketGammaHttpClient::new(
            None,
            90,
            RetryConfig {
                max_retries: 0,
                operation_timeout_ms: Some(30_000),
                immediate_first: true,
                max_elapsed_ms: Some(300_000),
                ..RetryConfig::default()
            },
        )
        .unwrap();
        let attempts = AtomicUsize::new(0);
        let started = tokio::time::Instant::now();
        let tags: Vec<GammaTag> = client
            .inner
            .send_request("/tags", || {
                let attempt = attempts.fetch_add(1, Ordering::Relaxed);
                async move {
                    if attempt == 0 {
                        return Err(HttpClientError::TransportError("reset".to_string()));
                    }

                    tokio::time::sleep(Duration::from_secs(65)).await;
                    Ok(HttpResponse {
                        status: 200_u16.try_into().unwrap(),
                        headers: HashMap::new(),
                        body: "[]".into(),
                    })
                }
            })
            .await
            .unwrap();

        assert!(tags.is_empty());
        assert_eq!(attempts.load(Ordering::Relaxed), 2);
        assert!(started.elapsed() >= Duration::from_secs(66));
    }

    #[rstest]
    #[case::server_error(&[503, 200], "[]", None, 2)]
    #[case::rate_limit(&[429, 200], "[]", None, 2)]
    #[case::rate_limit_hint(&[429, 200], "[]", Some("1"), 2)]
    #[case::server_error_hint(&[503, 200], "[]", Some("1"), 2)]
    #[case::invalid_hint(&[503, 200], "[]", Some("invalid"), 2)]
    #[case::overflow_hint(&[503, 200], "[]", Some("18446744073709551615"), 2)]
    #[case::timeout(&[0, 200], "[]", None, 2)]
    #[case::exhausted(&[503], "[]", None, 4)]
    #[case::forbidden(&[403], "[]", None, 1)]
    #[case::decode(&[200], "invalid", None, 1)]
    #[tokio::test]
    async fn test_http_retries(
        #[case] statuses: &[u16],
        #[case] body: &'static str,
        #[case] retry_after: Option<&'static str>,
        #[case] expected_requests: usize,
    ) {
        let requests = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&requests);
        let statuses = statuses.to_vec();
        let final_status = *statuses.last().unwrap();
        let router = Router::new().fallback(get(move || {
            let attempt = count.fetch_add(1, Ordering::Relaxed);
            let status = statuses[attempt.min(statuses.len() - 1)];
            async move {
                if status == 0 {
                    tokio::time::sleep(Duration::from_millis(1_400)).await;
                }
                let mut response =
                    (StatusCode::from_u16(status.max(200)).unwrap(), body).into_response();

                if let Some(value) = retry_after {
                    response
                        .headers_mut()
                        .insert(HEADER_RETRY_AFTER, value.parse().unwrap());
                }
                response
            }
        }));
        let (client, server) = retry_client(router).await;
        let started = std::time::Instant::now();
        let result = client.inner.get_gamma_tags().await;

        assert_eq!(requests.load(Ordering::Relaxed), expected_requests);

        if final_status == 200 && body == "[]" {
            result.unwrap();
        } else if final_status == 200 {
            assert!(matches!(result, Err(Error::Serde(_))));
        } else {
            assert!(matches!(result, Err(Error::Http { status, .. }) if status == final_status));
        }

        if retry_after == Some("1") {
            assert!(started.elapsed() >= Duration::from_secs(1));
        }
        server.abort();
    }

    #[rstest]
    #[tokio::test]
    async fn test_http_retry_retains_completed_pages(#[values(false, true)] events: bool) {
        let queries = Arc::new(Mutex::new(Vec::new()));
        let received = Arc::clone(&queries);
        let router = Router::new().fallback(get(move |uri: axum::http::Uri| {
            let mut queries = received.lock();
            queries.push(uri.query().unwrap_or_default().to_string());
            let response = if queries.len() == 2 {
                StatusCode::SERVICE_UNAVAILABLE.into_response()
            } else {
                let key = if events { "events" } else { "markets" };
                let row = if events {
                    serde_json::json!({"id": "event"})
                } else {
                    serde_json::to_value(load_fee_market("gamma_market.json")).unwrap()
                };
                axum::Json(serde_json::json!({
                    key: [row],
                    "next_cursor": if queries.len() == 1 { Some("next") } else { None },
                }))
                .into_response()
            };
            async { response }
        }));
        let (client, server) = retry_client(router).await;
        let count = if events {
            client
                .request_events_by_params(Default::default())
                .await
                .unwrap()
                .len()
        } else {
            client
                .request_markets_by_params(Default::default())
                .await
                .unwrap()
                .len()
        };

        assert_eq!(count, 2);
        let queries = queries.lock();
        assert_eq!(queries.len(), 3);
        assert!(!queries[0].contains("after_cursor"));
        assert!(queries[1].contains("after_cursor=next"));
        assert_eq!(queries[1], queries[2]);
        server.abort();
    }

    #[rstest]
    #[case::indexing_lag(false, false)]
    #[case::http_recovers(true, false)]
    #[case::http_exhausted(true, true)]
    #[tokio::test]
    async fn test_slug_retries_have_one_http_retry_owner(
        #[case] http_error: bool,
        #[case] exhausted: bool,
    ) {
        let requests = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&requests);
        let router = Router::new().fallback(get(move || {
            let attempt = count.fetch_add(1, Ordering::Relaxed);
            async move {
                if http_error && (attempt == 0 || exhausted) {
                    return (StatusCode::SERVICE_UNAVAILABLE, [(HEADER_RETRY_AFTER, "1")])
                        .into_response();
                }
                axum::Json(if attempt == 0 {
                    serde_json::json!([])
                } else {
                    serde_json::json!([load_fee_market("gamma_market.json")])
                })
                .into_response()
            }
        }));
        let (client, server) = retry_client(router).await;
        let result = client
            .request_instruments_by_slugs_with_retry(vec!["slug".to_string()])
            .await;

        assert_eq!(
            requests.load(Ordering::Relaxed),
            if exhausted { 4 } else { 2 }
        );

        if exhausted {
            assert!(result.unwrap_err().to_string().contains("503"));
        } else {
            assert_eq!(result.unwrap().len(), 2);
        }
        server.abort();
    }

    fn load_fee_market(filename: &str) -> GammaMarket {
        let path = format!("test_data/{filename}");
        let content = std::fs::read_to_string(path).unwrap();
        serde_json::from_str(&content).unwrap()
    }

    fn instrument_fee_schedule(instrument: &InstrumentAny) -> crate::http::models::FeeSchedule {
        let InstrumentAny::BinaryOption(binary) = instrument else {
            panic!("expected a binary option instrument");
        };

        let info = binary.info.as_ref().unwrap();
        let value = info.get("fee_schedule").unwrap();
        serde_json::from_value(value.clone()).unwrap()
    }

    #[rstest]
    #[case(Some("v1"), true)]
    #[case(Some("v2"), false)]
    #[case(None, false)]
    #[case(Some("v3"), false)]
    #[case(Some("V1"), false)]
    fn test_market_protocol_controls_discovery(
        #[case] version: Option<&str>,
        #[case] supported: bool,
        #[values("gamma_market_protocol_v1.json", "gamma_market_protocol_v2.json")] fixture: &str,
        #[values(false, true)] empty_ids: bool,
    ) {
        let mut market = load_fee_market(fixture);
        market.version = version.map(str::to_string);
        if empty_ids {
            market.clob_token_ids = "[]".into();
        }

        let condition_id = market.condition_id.clone();
        let (instruments, transient) = parse_markets_with_transient(&[market.clone()], 1.into());
        assert_eq!(
            instruments.len(),
            if supported && !empty_ids { 2 } else { 0 }
        );
        assert_eq!(
            transient,
            if supported && empty_ids {
                vec![condition_id]
            } else {
                vec![]
            }
        );

        if supported && !empty_ids {
            for instrument in instruments {
                let InstrumentAny::BinaryOption(binary) = instrument else {
                    panic!("expected binary option")
                };

                assert_eq!(
                    binary.info.as_ref().unwrap().get_str("market_version"),
                    Some("v1")
                );
            }
        } else if !supported {
            let error = crate::http::parse::parse_gamma_market(&market).unwrap_err();
            assert_eq!(
                error.to_string(),
                format!(
                    "Unsupported Polymarket market protocol {}; only v1 CTF markets are supported",
                    version.unwrap_or("missing"),
                )
            );
        }
    }

    #[rstest]
    fn test_live_instrument_funnel_retains_gamma_metadata() {
        let raw = include_str!("../../test_data/gamma_market_metadata.json");
        let market: GammaMarket = serde_json::from_str(raw).unwrap();
        let expected = raw.trim();
        let (instruments, transient) = parse_markets_with_transient(&[market], 1.into());
        assert_eq!(instruments.len(), 2);
        assert_eq!(transient, Vec::<String>::new());

        for instrument in instruments {
            let InstrumentAny::BinaryOption(binary) = instrument else {
                unreachable!()
            };

            assert_eq!(binary.event_id.map(|id| id.as_str()), Some("event-456"));
            let info = binary.info.unwrap();
            assert_eq!(info.get_str("gamma_market"), Some(expected));
            assert_eq!(info.get_bool("closed"), Some(false));
        }
    }

    #[rstest]
    fn test_event_discovery_retains_parent_metadata() {
        let raw = include_str!("../../test_data/gamma_event.json");
        let events: Vec<GammaEvent> = serde_json::from_str(raw).unwrap();
        let expected: Vec<Value> = serde_json::from_str(raw).unwrap();
        let markets = flatten_event_markets(events);
        assert_eq!(markets.len(), 2);

        for market in &markets {
            let parent = market.parent_event.as_ref().unwrap();
            let expected_event = expected
                .iter()
                .find(|event| event["id"] == parent.id)
                .unwrap();
            let expected_market = expected_event["markets"]
                .as_array()
                .unwrap()
                .iter()
                .find(|raw| raw["id"] == market.id)
                .unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&parent.raw).unwrap(),
                *expected_event
            );
            assert_eq!(
                serde_json::from_str::<Value>(&market.raw).unwrap(),
                *expected_market
            );
            let defs = parse_gamma_market(market).unwrap();
            for def in defs {
                assert_eq!(def.event_id.unwrap().as_str(), parent.id);
                assert_eq!(
                    def.gamma_event.as_ref().unwrap().as_str(),
                    Some(parent.raw.as_str())
                );
                assert_eq!(def.gamma_market, market.raw);
                let instrument = create_instrument_from_def(&def, 1.into()).unwrap();

                let InstrumentAny::BinaryOption(binary) = instrument else {
                    unreachable!()
                };

                let info = binary.info.unwrap();
                assert_eq!(binary.event_id.unwrap().as_str(), parent.id);
                assert_eq!(info.get_str("gamma_event"), Some(parent.raw.as_str()));
                assert_eq!(info.get_str("gamma_market"), Some(market.raw.as_str()));
            }
        }
    }

    #[rstest]
    #[case(1)]
    #[case(8)]
    #[case(32)]
    fn test_event_metadata_shares_storage_across_instruments_and_clones(
        #[case] market_count: usize,
    ) {
        let mut events: Vec<GammaEvent> =
            serde_json::from_str(include_str!("../../test_data/gamma_event.json")).unwrap();
        let mut event = events.remove(0);
        let market = event.markets[0].clone();
        event.markets = (0..market_count)
            .map(|index| {
                let mut market = market.clone();
                market.id = format!("market-{index}");
                market.condition_id = format!("condition-{index}");
                market.clob_token_ids = serde_json::to_string(&[
                    format!("{}", 2 * index + 1),
                    format!("{}", 2 * index + 2),
                ])
                .unwrap();
                market
            })
            .collect();

        let expected = event.raw.clone();
        let markets = flatten_event_markets(vec![event]);
        let (instruments, transient) = parse_markets_with_transient(&markets, 1.into());
        let event = instruments[0].info().unwrap().get("gamma_event").unwrap();
        let mut pointers = std::collections::HashSet::new();
        let mut cache = Cache::default();

        assert_eq!(instruments.len(), 2 * market_count);
        assert_eq!(transient, Vec::<String>::new());
        assert_eq!(event.as_str(), Some(expected.as_str()));

        for instrument in &instruments {
            cache.add_instrument(instrument.clone()).unwrap();
            let cloned = cache.instrument(&instrument.id()).unwrap().clone();
            for instrument in [instrument, &cloned] {
                let info = instrument.info().unwrap();
                let value = info.get("gamma_event").unwrap();
                pointers.insert(value.as_str().unwrap().as_ptr());
                assert!(std::ptr::eq(value, event));
                assert_eq!(info.get_bool("closed"), Some(false));
            }
        }

        assert_eq!(pointers.len(), 1);
    }

    #[rstest]
    fn test_event_metadata_keeps_distinct_snapshots_with_the_same_id() {
        let mut events: Vec<GammaEvent> =
            serde_json::from_str(include_str!("../../test_data/gamma_event.json")).unwrap();
        let event = events.remove(0);
        let mut updated = event.clone();
        let mut value: Value = serde_json::from_str(&updated.raw).unwrap();
        value["snapshot"] = Value::String("updated".to_string());
        updated.raw = serde_json::to_string(&value).unwrap();
        let original_raw = event.raw.clone();
        let updated_raw = updated.raw.clone();
        let markets_per_event = event.markets.len();
        let markets = flatten_event_markets(vec![event, updated]);
        let (instruments, transient) = parse_markets_with_transient(&markets, 1.into());
        let snapshots = instruments
            .chunks(2 * markets_per_event)
            .collect::<Vec<_>>();

        assert_eq!(transient, Vec::<String>::new());
        assert_eq!(snapshots.len(), 2);

        for (snapshot, expected) in snapshots.iter().zip([original_raw, updated_raw]) {
            let event = snapshot[0].info().unwrap().get("gamma_event").unwrap();
            assert_eq!(event.as_str(), Some(expected.as_str()));

            for instrument in *snapshot {
                assert!(std::ptr::eq(
                    instrument.info().unwrap().get("gamma_event").unwrap(),
                    event
                ));
            }
        }

        assert!(!std::ptr::eq(
            snapshots[0][0].info().unwrap().get("gamma_event").unwrap(),
            snapshots[1][0].info().unwrap().get("gamma_event").unwrap()
        ));
    }

    #[rstest]
    #[case(GetGammaMarketsParamsBuilder::default().active(false).build().unwrap(), "active")]
    #[case(GetGammaMarketsParamsBuilder::default().closed(false).build().unwrap(), "closed")]
    #[case(GetGammaMarketsParamsBuilder::default().archived(false).build().unwrap(), "archived")]
    #[case(GetGammaMarketsParamsBuilder::default().id(vec![42]).build().unwrap(), "id")]
    #[case(GetGammaMarketsParamsBuilder::default().limit(1_u32).build().unwrap(), "limit")]
    #[case(GetGammaMarketsParamsBuilder::default().offset(0_u32).build().unwrap(), "offset")]
    #[case(GetGammaMarketsParamsBuilder::default().slug(vec!["other".into()]).build().unwrap(), "slug")]
    #[case(GetGammaMarketsParamsBuilder::default().clob_token_ids(vec![]).build().unwrap(), "clob_token_ids")]
    #[case(GetGammaMarketsParamsBuilder::default().condition_ids(vec!["condition".into()]).build().unwrap(), "condition_ids")]
    #[case(GetGammaMarketsParamsBuilder::default().question_ids(vec!["question".into()]).build().unwrap(), "question_ids")]
    #[case(GetGammaMarketsParamsBuilder::default().market_maker_address(vec!["maker".into()]).build().unwrap(), "market_maker_address")]
    #[case(GetGammaMarketsParamsBuilder::default().liquidity_num_min(dec!(1.25)).build().unwrap(), "liquidity_num_min")]
    #[case(GetGammaMarketsParamsBuilder::default().liquidity_num_max(dec!(8.75)).build().unwrap(), "liquidity_num_max")]
    #[case(GetGammaMarketsParamsBuilder::default().volume_num_min(dec!(2.5)).build().unwrap(), "volume_num_min")]
    #[case(GetGammaMarketsParamsBuilder::default().volume_num_max(dec!(9.5)).build().unwrap(), "volume_num_max")]
    #[case(GetGammaMarketsParamsBuilder::default().start_date_min("2026-01-01").build().unwrap(), "start_date_min")]
    #[case(GetGammaMarketsParamsBuilder::default().start_date_max("2026-12-31").build().unwrap(), "start_date_max")]
    #[case(GetGammaMarketsParamsBuilder::default().end_date_min("2027-01-01").build().unwrap(), "end_date_min")]
    #[case(GetGammaMarketsParamsBuilder::default().end_date_max("2027-12-31").build().unwrap(), "end_date_max")]
    #[case(GetGammaMarketsParamsBuilder::default().tag_id(vec![42]).build().unwrap(), "tag_id")]
    #[case(GetGammaMarketsParamsBuilder::default().related_tags(false).build().unwrap(), "related_tags")]
    #[case(GetGammaMarketsParamsBuilder::default().tag_match("any").build().unwrap(), "tag_match")]
    #[case(GetGammaMarketsParamsBuilder::default().decimalized(false).build().unwrap(), "decimalized")]
    #[case(GetGammaMarketsParamsBuilder::default().cyom(false).build().unwrap(), "cyom")]
    #[case(GetGammaMarketsParamsBuilder::default().rfq_enabled(false).build().unwrap(), "rfq_enabled")]
    #[case(GetGammaMarketsParamsBuilder::default().uma_resolution_status("resolved").build().unwrap(), "uma_resolution_status")]
    #[case(GetGammaMarketsParamsBuilder::default().game_id("game").build().unwrap(), "game_id")]
    #[case(GetGammaMarketsParamsBuilder::default().sports_market_types(vec!["moneyline".into()]).build().unwrap(), "sports_market_types")]
    #[case(GetGammaMarketsParamsBuilder::default().include_tag(false).build().unwrap(), "include_tag")]
    #[case(GetGammaMarketsParamsBuilder::default().locale("en").build().unwrap(), "locale")]
    #[tokio::test]
    async fn test_event_query_rejects_unsupported_params_before_http(
        #[case] params: GetGammaMarketsParams,
        #[case] field: &str,
    ) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let client = PolymarketGammaHttpClient::new(
            Some(format!("http://{address}")),
            1,
            RetryConfig::default(),
        )
        .unwrap();
        let error = client
            .request_instruments_by_event_query("unused", params)
            .await
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            format!("Unsupported event query parameter '{field}'")
        );
        assert!(matches!(
            listener.accept(),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock
        ));
    }

    #[rstest]
    #[case("volumeNum")]
    #[case("liquidityNum")]
    #[case("unknown")]
    #[case("")]
    #[tokio::test]
    async fn test_event_query_rejects_unsupported_order_before_http(#[case] order: &str) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let address = listener.local_addr().unwrap();
        let client = PolymarketGammaHttpClient::new(
            Some(format!("http://{address}")),
            1,
            RetryConfig::default(),
        )
        .unwrap();
        let params = GetGammaMarketsParamsBuilder::default()
            .order(order)
            .max_markets(1_u32)
            .build()
            .unwrap();
        let error = client
            .request_instruments_by_event_query("unused", params)
            .await
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            format!("Unsupported event query order field '{order}'")
        );
        assert!(matches!(
            listener.accept(),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock
        ));
    }

    #[rstest]
    #[case("liquidity")]
    #[case("volume")]
    #[case("volume24hr")]
    #[case("competitive")]
    #[case("spread")]
    #[case("best_bid")]
    #[case("one_day_price_change")]
    #[case("volume_1wk")]
    #[tokio::test]
    async fn test_event_sort_preserves_adjacent_decimal_values(
        #[case] field: &str,
        #[values(None, Some(true), Some(false))] ascending: Option<bool>,
    ) {
        let mut lower: GammaMarket =
            serde_json::from_str(include_str!("../../test_data/gamma_market.json")).unwrap();
        lower.clob_token_ids = serde_json::to_string(&["1", "2"]).unwrap();
        let mut higher = lower.clone();
        higher.clob_token_ids = serde_json::to_string(&["3", "4"]).unwrap();

        for (market, value) in [
            (&mut lower, dec!(0.1234567890123456789012345678)),
            (&mut higher, dec!(0.1234567890123456789012345679)),
        ] {
            match field {
                "liquidity" => market.liquidity_num = Some(value),
                "volume" => market.volume_num = Some(value),
                "volume24hr" => market.volume_24hr = Some(value),
                "competitive" => {}
                "spread" => market.spread = Some(value),
                "best_bid" => market.best_bid = Some(value),
                "one_day_price_change" => market.one_day_price_change = Some(value),
                "volume_1wk" => market.volume_1wk = Some(value),
                _ => unreachable!(),
            }
        }

        lower.competitive = Some(0.25);
        higher.competitive = Some(0.75);
        let mut event: GammaEvent =
            serde_json::from_str(include_str!("../../test_data/decimal_precision_event.json"))
                .unwrap();

        event.markets = if ascending == Some(true) {
            vec![higher, lower]
        } else {
            vec![lower, higher]
        };

        let response = serde_json::to_string(&vec![event]).unwrap();
        let router = axum::Router::new().route(
            "/events",
            axum::routing::get(move || {
                let response = response.clone();
                async move { response }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let client = PolymarketGammaHttpClient::new(
            Some(format!("http://{address}")),
            5,
            RetryConfig::default(),
        )
        .unwrap();
        let instruments = client
            .request_instruments_by_event_query(
                "precision",
                GetGammaMarketsParams {
                    order: Some(field.into()),
                    ascending,
                    max_markets: Some(1),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        server.abort();
        assert_eq!(instruments.len(), 2);

        let expected = if ascending == Some(true) {
            ["1", "2"]
        } else {
            ["3", "4"]
        };

        assert_eq!(instruments[0].raw_symbol().as_str(), expected[0]);
        assert_eq!(instruments[1].raw_symbol().as_str(), expected[1]);
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn test_markets_response_preserves_decimal_precision(#[case] enveloped: bool) {
        let market = include_str!("../../test_data/decimal_precision_market.json");
        let array = format!("[{market}]");
        let raw = if enveloped {
            format!("{{\"data\":{array}}}")
        } else {
            array
        };
        let markets =
            parse_gamma_markets_response(&serde_json::from_str::<Box<RawValue>>(&raw).unwrap())
                .unwrap();
        assert_eq!(markets.len(), 1);
        assert_eq!(
            markets[0].best_bid,
            Some(dec!(0.1234567890123456789012345678))
        );
        assert_eq!(markets[0].volume_num, Some(dec!(12345678901.123457)));
        assert_eq!(
            markets[0].fee_schedule.as_ref().unwrap().rate,
            dec!(0.1234567890123456789012345678)
        );
    }

    #[rstest]
    #[case("gamma_market_fee_crypto.json", false, dec!(0.07), dec!(0.20))]
    #[case("gamma_market_fee_sports.json", false, dec!(0.05), dec!(0.15))]
    #[case("gamma_market_fee_politics.json", false, dec!(0.04), dec!(0.25))]
    #[case("gamma_market_fee_zero_rate.json", false, Decimal::ZERO, dec!(0.20))]
    #[case("gamma_market_fee_crypto.json", true, Decimal::ZERO, dec!(0.20))]
    #[tokio::test]
    async fn test_request_instruments_by_slugs_enriches_fee_schedules(
        #[case] filename: &str,
        #[case] missing_schedule: bool,
        #[case] rate: Decimal,
        #[case] rebate: Decimal,
    ) {
        let mut market = load_fee_market(filename);
        if missing_schedule {
            market.fee_schedule = None;
        }

        let response = serde_json::to_string(&vec![market]).unwrap();
        let router = axum::Router::new().route(
            "/markets",
            axum::routing::get(move || {
                let response = response.clone();
                async move { response }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let client = PolymarketGammaHttpClient::new(
            Some(format!("http://{address}")),
            5,
            RetryConfig::default(),
        )
        .unwrap();
        let instruments = client
            .request_instruments_by_slugs(vec!["fee-market".to_string()])
            .await
            .unwrap();
        server.abort();

        assert_eq!(instruments.len(), 2);

        for instrument in &instruments {
            let schedule = instrument_fee_schedule(instrument);
            assert_eq!(schedule.rate, rate);
            assert_eq!(schedule.rebate_rate, rebate);
            assert_eq!(schedule.exponent, Decimal::ONE);
            assert!(schedule.taker_only);
        }
    }
}
