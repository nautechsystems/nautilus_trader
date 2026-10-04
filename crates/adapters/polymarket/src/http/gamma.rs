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

use std::{collections::HashMap, result::Result as StdResult, sync::Arc};

use ahash::AHashMap;
use nautilus_core::{
    UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::instruments::InstrumentAny;
use nautilus_network::{
    http::{HttpClient, HttpClientError, Method, create_standard_nautilus_headers},
    retry::{RetryConfig, RetryManager},
    websocket::proxy::ProxyUrl,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, value::RawValue};

use crate::{
    common::urls::gamma_api_url,
    filters::set_market_closed,
    http::{
        clob::PolymarketClobPublicClient,
        error::{Error, Result, decode_response},
        models::{GammaEvent, GammaMarket, GammaTag, SearchResponse},
        pagination::{Completion, CursorProtocol, FetchOutcome, Paginator, WindowedCollect},
        parse::{create_instrument_from_def, enrich_market_fee_schedule, parse_gamma_market},
        query::{GetGammaEventsParams, GetGammaMarketsParams, GetSearchParams},
        rate_limits::POLYMARKET_GAMMA_REST_QUOTA,
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
/// No authentication is required.
#[derive(Debug, Clone)]
pub struct PolymarketGammaRawHttpClient {
    client: HttpClient,
    base_url: String,
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
                .default_quota(*POLYMARKET_GAMMA_REST_QUOTA)
                .timeout_secs(timeout_secs)
                .maybe_proxy_url(proxy_url.map(|url| url.expose().to_string()))
                .build()?,
            base_url: base_url
                .unwrap_or_else(|| gamma_api_url().to_string())
                .trim_end_matches('/')
                .to_string(),
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
        let url = self.url(path);
        let response = self
            .client
            .request_with_params(Method::GET, url, params, None, None, None, None)
            .await
            .map_err(Error::from_http_client)?;

        decode_response(&response)
    }

    async fn send_get_query_map<T: DeserializeOwned>(
        &self,
        path: &str,
        params: Option<&HashMap<String, Vec<String>>>,
    ) -> Result<T> {
        let url = self.url(path);
        let response = self
            .client
            .request(Method::GET, url, params, None, None, None, None)
            .await
            .map_err(Error::from_http_client)?;

        decode_response(&response)
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

    for market in markets {
        if is_transient_clob_token_ids(&market.clob_token_ids) {
            transient.push(market.condition_id.clone());
            continue;
        }

        match parse_gamma_market(market) {
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

// Returns the first usable token ID for fee-rate fallback, if any.
fn first_token_id(market: &GammaMarket) -> Option<String> {
    serde_json::from_str::<Vec<String>>(&market.clob_token_ids)
        .ok()?
        .into_iter()
        .find(|token| !token.is_empty())
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
    clob_client: Option<PolymarketClobPublicClient>,
    fee_rate_cache: Arc<tokio::sync::Mutex<AHashMap<String, Decimal>>>,
}

impl PolymarketGammaHttpClient {
    /// Creates a new [`PolymarketGammaHttpClient`].
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
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be created.
    pub fn new_with_proxy(
        gamma_base_url: Option<String>,
        timeout_secs: u64,
        retry_config: RetryConfig,
        proxy_url: Option<ProxyUrl>,
    ) -> StdResult<Self, HttpClientError> {
        Ok(Self {
            inner: Arc::new(PolymarketGammaRawHttpClient::new_with_proxy(
                gamma_base_url,
                timeout_secs,
                proxy_url,
            )?),
            clock: get_atomic_clock_realtime(),
            retry_manager: Arc::new(RetryManager::new(retry_config)),
            clob_client: None,
            fee_rate_cache: Arc::new(tokio::sync::Mutex::new(AHashMap::new())),
        })
    }

    /// Sets the CLOB client used for fee-rate fallback on zero-rate markets.
    pub fn set_clob_client(&mut self, clob_client: PolymarketClobPublicClient) {
        self.clob_client = Some(clob_client);
    }

    /// Returns the configured CLOB client for fee-rate fallback, if any.
    #[must_use]
    pub fn clob_client(&self) -> Option<&PolymarketClobPublicClient> {
        self.clob_client.as_ref()
    }

    // Enriches fee schedules with category-resolved rebates and fills zero
    // taker rates from the CLOB fee-rate endpoint when a CLOB client is set.
    // Fallback runs only for fee-enabled markets with a zero rate; fee-free
    // and unclassifiable markets keep zero with no request.
    async fn enrich_markets(&self, markets: &mut [GammaMarket]) {
        for market in markets.iter_mut() {
            enrich_market_fee_schedule(market);
        }

        let Some(clob) = self.clob_client.as_ref() else {
            return;
        };

        for market in markets.iter_mut() {
            let needs_fallback = matches!(
                &market.fee_schedule,
                Some(schedule) if schedule.rate.is_zero() && !schedule.rebate_rate.is_zero()
            );

            if !needs_fallback {
                continue;
            }

            let Some(token_id) = first_token_id(market) else {
                continue;
            };

            if let Some(cached) = self.fee_rate_cache.lock().await.get(&token_id).copied() {
                if let Some(schedule) = market.fee_schedule.as_mut() {
                    schedule.rate = cached;
                }

                continue;
            }

            let rate = match clob.get_fee_rate(&token_id).await {
                Ok(response) => {
                    let rate = response.to_rate();
                    if rate < Decimal::ZERO {
                        log::warn!("Ignoring negative CLOB fee rate {rate} for token {token_id}");
                        continue;
                    }

                    self.fee_rate_cache.lock().await.insert(token_id, rate);
                    rate
                }
                Err(e) => {
                    log::warn!(
                        "CLOB fee-rate fallback failed for market {}: {e}",
                        market.id
                    );
                    continue;
                }
            };

            if let Some(schedule) = market.fee_schedule.as_mut() {
                schedule.rate = rate;
            }
        }
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
        self.enrich_markets(&mut markets).await;
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

            self.enrich_markets(&mut markets).await;
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
    /// Uses the client's [`RetryManager`] with exponential backoff. Gamma API
    /// may not have indexed a newly created market yet, so empty results are
    /// treated as retryable (indexing lag). HTTP errors are also retried per
    /// the standard `is_retryable()` classification.
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

                        if parse_markets_to_instruments(&markets, ts_init).is_empty() {
                            return Err(Error::transport(
                                "Gamma returned no instruments (indexing lag)",
                            ));
                        }

                        Ok(markets)
                    }
                },
                |e| e.is_retryable(),
                |e| Error::transport(e.to_string()),
            )
            .execute()
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        self.enrich_markets(&mut markets).await;
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

            self.enrich_markets(&mut markets).await;
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
        self.enrich_markets(&mut markets).await;
        let ts_init = self.clock.get_time_ns();
        let instruments = parse_markets_to_instruments(&markets, ts_init);
        log::debug!("Parsed {} instruments from params query", instruments.len());
        Ok(instruments)
    }

    /// Same as [`Self::request_instruments_by_params`] but also returns
    /// condition IDs whose markets came back from Gamma with empty
    /// `clob_token_ids`. Callers driving auto-load retries use the transient
    /// list to distinguish "still hydrating in the CLOB" from "absent on the
    /// venue".
    pub async fn request_instruments_by_params_with_transient(
        &self,
        base_params: GetGammaMarketsParams,
    ) -> anyhow::Result<(Vec<InstrumentAny>, Vec<String>)> {
        let mut markets = self.fetch_gamma_markets_paginated(base_params).await?;
        self.enrich_markets(&mut markets).await;
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

    /// Fetches instruments from an event slug with client-side sorting and limiting.
    ///
    /// The `/events?slug=` response already includes the full markets array,
    /// so no second API call is needed. Sorting and truncation are applied
    /// client-side using fields from `GetGammaMarketsParams`:
    /// - `order`: sort field (`"liquidity"`, `"volume"`, `"volume24hr"`)
    /// - `ascending`: sort direction (default: descending)
    /// - `max_markets`: truncate after sorting
    pub async fn request_instruments_by_event_query(
        &self,
        event_slug: &str,
        params: GetGammaMarketsParams,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let events = self.inner.get_gamma_events_by_slug(event_slug).await?;
        let mut markets = flatten_event_markets(events);

        if markets.is_empty() {
            log::warn!("No markets found in event slug '{event_slug}'");
            return Ok(Vec::new());
        }

        log::debug!("Event '{event_slug}' returned {} markets", markets.len());

        // Client-side sort
        if let Some(ref order_field) = params.order {
            let ascending = params.ascending.unwrap_or(false);
            markets.sort_by(|a, b| {
                let cmp = match order_field.as_str() {
                    "liquidity" => a
                        .liquidity_num
                        .unwrap_or(Decimal::ZERO)
                        .partial_cmp(&b.liquidity_num.unwrap_or(Decimal::ZERO)),
                    "volume" => a
                        .volume_num
                        .unwrap_or(Decimal::ZERO)
                        .partial_cmp(&b.volume_num.unwrap_or(Decimal::ZERO)),
                    "volume24hr" => a
                        .volume_24hr
                        .unwrap_or(Decimal::ZERO)
                        .partial_cmp(&b.volume_24hr.unwrap_or(Decimal::ZERO)),
                    "competitive" => a
                        .competitive
                        .unwrap_or(0.0)
                        .partial_cmp(&b.competitive.unwrap_or(0.0)),
                    "spread" => a
                        .spread
                        .unwrap_or(Decimal::MAX)
                        .partial_cmp(&b.spread.unwrap_or(Decimal::MAX)),
                    "best_bid" => a
                        .best_bid
                        .unwrap_or(Decimal::ZERO)
                        .partial_cmp(&b.best_bid.unwrap_or(Decimal::ZERO)),
                    "one_day_price_change" => a
                        .one_day_price_change
                        .unwrap_or(Decimal::ZERO)
                        .partial_cmp(&b.one_day_price_change.unwrap_or(Decimal::ZERO)),
                    "volume_1wk" => a
                        .volume_1wk
                        .unwrap_or(Decimal::ZERO)
                        .partial_cmp(&b.volume_1wk.unwrap_or(Decimal::ZERO)),
                    _ => None,
                };
                let cmp = cmp.unwrap_or(std::cmp::Ordering::Equal);
                if ascending { cmp } else { cmp.reverse() }
            });
        }

        // Client-side truncation
        if let Some(cap) = params.max_markets {
            markets.truncate(cap as usize);
        }

        self.enrich_markets(&mut markets).await;
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
        self.enrich_markets(&mut markets).await;
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
            self.enrich_markets(&mut markets).await;
            instruments.extend(parse_markets_to_instruments(&markets, ts_init));
        }

        if let Some(events) = &response.events {
            let mut event_markets = flatten_event_markets(events.clone());
            self.enrich_markets(&mut event_markets).await;
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
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    fn load_fee_market(filename: &str) -> GammaMarket {
        let path = format!("test_data/{filename}");
        let content = std::fs::read_to_string(path).unwrap();
        serde_json::from_str(&content).unwrap()
    }

    async fn fee_rate_test_client() -> (
        PolymarketGammaHttpClient,
        Arc<AtomicUsize>,
        tokio::task::JoinHandle<()>,
    ) {
        fee_rate_test_client_with(axum::http::StatusCode::OK, r#"{"base_fee":700}"#).await
    }

    async fn fee_rate_test_client_with(
        status: axum::http::StatusCode,
        body: &'static str,
    ) -> (
        PolymarketGammaHttpClient,
        Arc<AtomicUsize>,
        tokio::task::JoinHandle<()>,
    ) {
        let calls = Arc::new(AtomicUsize::new(0));
        let asserted = Arc::clone(&calls);

        let router = axum::Router::new().route(
            "/fee-rate",
            axum::routing::get(move || {
                let calls = Arc::clone(&calls);

                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    (status, body.to_string())
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

        let clob = PolymarketClobPublicClient::new(Some(format!("http://{address}")), 5).unwrap();
        let mut client = PolymarketGammaHttpClient::new(None, 5, RetryConfig::default()).unwrap();
        client.set_clob_client(clob);

        (client, asserted, server)
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
                assert_eq!(def.gamma_event.as_ref(), Some(&parent.raw));
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
    #[case("liquidity")]
    #[case("volume")]
    #[case("volume24hr")]
    #[case("spread")]
    #[case("best_bid")]
    #[case("one_day_price_change")]
    #[case("volume_1wk")]
    #[tokio::test]
    async fn test_event_sort_preserves_adjacent_decimal_values(#[case] field: &str) {
        use nautilus_model::instruments::Instrument;

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
                "spread" => market.spread = Some(value),
                "best_bid" => market.best_bid = Some(value),
                "one_day_price_change" => market.one_day_price_change = Some(value),
                "volume_1wk" => market.volume_1wk = Some(value),
                _ => unreachable!(),
            }
        }
        let mut event: GammaEvent =
            serde_json::from_str(include_str!("../../test_data/decimal_precision_event.json"))
                .unwrap();
        event.markets = vec![lower, higher];
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
                    ascending: Some(false),
                    max_markets: Some(1),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        server.abort();
        assert_eq!(instruments.len(), 2);
        assert_eq!(instruments[0].raw_symbol().as_str(), "3");
        assert_eq!(instruments[1].raw_symbol().as_str(), "4");
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

    #[tokio::test]
    async fn test_enrich_markets_falls_back_only_for_zero_rate_fee_enabled() {
        let (client, asserted, server) = fee_rate_test_client().await;

        let mut markets = [
            "gamma_market_fee_crypto.json",
            "gamma_market_fee_zero_rate.json",
            "gamma_market_fee_free.json",
            "gamma_market_fee_unclassifiable.json",
        ]
        .map(load_fee_market);

        client.enrich_markets(&mut markets).await;
        server.abort();

        assert_eq!(asserted.load(Ordering::SeqCst), 1);

        let crypto = markets[0].fee_schedule.as_ref().unwrap();
        assert_eq!(crypto.rate, dec!(0.07));
        assert_eq!(crypto.rebate_rate, dec!(0.20));

        let recovered = markets[1].fee_schedule.as_ref().unwrap();
        assert_eq!(recovered.rate, dec!(0.07));
        assert_eq!(recovered.rebate_rate, dec!(0.20));

        assert!(markets[2].fee_schedule.is_none());

        let unknown = markets[3].fee_schedule.as_ref().unwrap();
        assert_eq!(unknown.rate, Decimal::ZERO);
        assert_eq!(unknown.rebate_rate, Decimal::ZERO);
    }

    #[tokio::test]
    async fn test_enrich_markets_caches_fee_rate_by_token() {
        let (client, asserted, server) = fee_rate_test_client().await;

        let market = load_fee_market("gamma_market_fee_zero_rate.json");
        let mut markets = [market.clone(), market];

        client.enrich_markets(&mut markets).await;
        server.abort();

        assert_eq!(asserted.load(Ordering::SeqCst), 1);

        for market in &markets {
            let schedule = market.fee_schedule.as_ref().unwrap();
            assert_eq!(schedule.rate, dec!(0.07));
            assert_eq!(schedule.rebate_rate, dec!(0.20));
        }
    }

    #[tokio::test]
    async fn test_enrich_markets_keeps_zero_rate_when_fee_rate_fails() {
        let (client, asserted, server) = fee_rate_test_client_with(
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            r#"{"error":"unavailable"}"#,
        )
        .await;

        let mut markets = [load_fee_market("gamma_market_fee_zero_rate.json")];

        client.enrich_markets(&mut markets).await;
        server.abort();

        assert_eq!(asserted.load(Ordering::SeqCst), 1);

        let schedule = markets[0].fee_schedule.as_ref().unwrap();
        assert_eq!(schedule.rate, Decimal::ZERO);
        assert_eq!(schedule.rebate_rate, dec!(0.20));
    }

    #[tokio::test]
    async fn test_enrich_markets_ignores_negative_fee_rate() {
        let (client, asserted, server) =
            fee_rate_test_client_with(axum::http::StatusCode::OK, r#"{"base_fee":-100}"#).await;

        let mut markets = [load_fee_market("gamma_market_fee_zero_rate.json")];

        client.enrich_markets(&mut markets).await;
        server.abort();

        assert_eq!(asserted.load(Ordering::SeqCst), 1);

        let schedule = markets[0].fee_schedule.as_ref().unwrap();
        assert_eq!(schedule.rate, Decimal::ZERO);
        assert_eq!(schedule.rebate_rate, dec!(0.20));
    }

    #[tokio::test]
    async fn test_request_instruments_by_slugs_enriches_fee_schedules() {
        let market = include_str!("../../test_data/gamma_market_fee_crypto.json");
        let response = format!("[{market}]");

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
            .request_instruments_by_slugs(vec!["fee-crypto-1".to_string()])
            .await
            .unwrap();
        server.abort();

        assert_eq!(instruments.len(), 2);

        for instrument in &instruments {
            let schedule = instrument_fee_schedule(instrument);
            assert_eq!(schedule.rate, dec!(0.07));
            assert_eq!(schedule.rebate_rate, dec!(0.20));
        }
    }
}
