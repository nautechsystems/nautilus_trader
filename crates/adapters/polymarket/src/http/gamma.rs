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

use std::{collections::HashMap, result::Result as StdResult, sync::Arc};

use nautilus_core::{
    UnixNanos,
    consts::NAUTILUS_USER_AGENT,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::instruments::InstrumentAny;
use nautilus_network::http::{HttpClient, HttpClientError, Method, USER_AGENT};
use serde::Serialize;
use serde_json::Value;

use crate::{
    common::urls::gamma_api_url,
    http::{
        error::{Error, Result},
        models::{GammaEvent, GammaMarket, GammaTag, SearchResponse},
        parse::{create_instrument_from_def, parse_gamma_market},
        query::{GetGammaEventsParams, GetGammaMarketsParams, GetSearchParams},
        rate_limits::POLYMARKET_GAMMA_REST_QUOTA,
    },
};

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
    pub fn new(
        base_url: Option<String>,
        timeout_secs: Option<u64>,
    ) -> StdResult<Self, HttpClientError> {
        Ok(Self {
            client: HttpClient::new(
                Self::default_headers(),
                vec![],
                vec![],
                Some(*POLYMARKET_GAMMA_REST_QUOTA),
                timeout_secs,
                None,
            )?,
            base_url: base_url
                .unwrap_or_else(|| gamma_api_url().to_string())
                .trim_end_matches('/')
                .to_string(),
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

    async fn send_get<P: Serialize, T: serde::de::DeserializeOwned>(
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
        let value: Value = self.send_get("/markets", Some(&params)).await?;

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
        self.send_get("/events", Some(&params)).await
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

/// Parses a slice of [`GammaMarket`]s into Nautilus instruments.
///
/// Failures are logged and skipped so that one bad market does not
/// prevent the remaining markets from being returned.
fn parse_markets_to_instruments(markets: &[GammaMarket], ts_init: UnixNanos) -> Vec<InstrumentAny> {
    let mut instruments = Vec::new();
    for market in markets {
        match parse_gamma_market(market) {
            Ok(defs) => {
                for def in defs {
                    match create_instrument_from_def(&def, ts_init) {
                        Ok(instrument) => instruments.push(instrument),
                        Err(e) => log::warn!("Failed to create instrument: {e}"),
                    }
                }
            }
            Err(e) => log::warn!("Failed to parse gamma market: {e}"),
        }
    }
    instruments
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
}

impl PolymarketGammaHttpClient {
    /// Creates a new [`PolymarketGammaHttpClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying HTTP client cannot be created.
    pub fn new(
        gamma_base_url: Option<String>,
        timeout_secs: Option<u64>,
    ) -> StdResult<Self, HttpClientError> {
        Ok(Self {
            inner: Arc::new(PolymarketGammaRawHttpClient::new(
                gamma_base_url,
                timeout_secs,
            )?),
            clock: get_atomic_clock_realtime(),
        })
    }

    /// Fetches markets from the Gamma API with the given base params, paginating automatically.
    async fn fetch_gamma_markets_paginated(
        &self,
        base_params: GetGammaMarketsParams,
    ) -> anyhow::Result<Vec<GammaMarket>> {
        const PAGE_LIMIT: u32 = 500;
        let page_size = base_params.limit.unwrap_or(PAGE_LIMIT);
        let max_markets = base_params.max_markets;
        let mut all_markets = Vec::new();
        let mut offset: u32 = base_params.offset.unwrap_or(0);

        loop {
            let params = GetGammaMarketsParams {
                limit: Some(page_size),
                offset: Some(offset),
                ..base_params.clone()
            };

            let page = self.inner.get_gamma_markets(params).await?;
            let page_len = page.len() as u32;
            all_markets.extend(page);

            if let Some(cap) = max_markets
                && all_markets.len() as u32 >= cap
            {
                all_markets.truncate(cap as usize);
                break;
            }

            if page_len < page_size {
                break;
            }

            offset += page_size;
        }

        Ok(all_markets)
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
        let markets = self.fetch_all_gamma_markets().await?;
        let ts_init = self.clock.get_time_ns();
        let instruments = parse_markets_to_instruments(&markets, ts_init);
        log::info!("Parsed {} instruments from Gamma API", instruments.len());
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
                    slug: Some(slug.clone()),
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
            let (slug, markets) = result;
            if markets.is_empty() {
                log::warn!("No markets found for slug '{slug}'");
                continue;
            }
            instruments.extend(parse_markets_to_instruments(&markets, ts_init));
        }

        if succeeded == 0 && total_slugs > 0 {
            anyhow::bail!("All {total_slugs} slug requests failed");
        }

        log::info!("Parsed {} instruments from slug queries", instruments.len());
        Ok(instruments)
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
            let markets: Vec<GammaMarket> = events.into_iter().flat_map(|e| e.markets).collect();
            if markets.is_empty() {
                log::warn!("No markets found in event slug '{slug}'");
                continue;
            }
            instruments.extend(parse_markets_to_instruments(&markets, ts_init));
        }

        if succeeded == 0 && total > 0 {
            anyhow::bail!("All {total} event slug requests failed");
        }

        log::info!(
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
        let markets = self.fetch_gamma_markets_paginated(base_params).await?;
        let ts_init = self.clock.get_time_ns();
        let instruments = parse_markets_to_instruments(&markets, ts_init);
        log::debug!("Parsed {} instruments from params query", instruments.len());
        Ok(instruments)
    }

    /// Fetches instruments from an event slug with client-side sorting and limiting.
    ///
    /// The `/events?slug=` response already includes the full markets array,
    /// so no second API call is needed. Sorting and truncation are applied
    /// client-side using fields from `GetGammaMarketsParams`:
    /// - `order` — sort field (`"liquidity"`, `"volume"`, `"volume24hr"`)
    /// - `ascending` — sort direction (default: descending)
    /// - `max_markets` — truncate after sorting
    pub async fn request_instruments_by_event_query(
        &self,
        event_slug: &str,
        params: GetGammaMarketsParams,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let events = self.inner.get_gamma_events_by_slug(event_slug).await?;
        let mut markets: Vec<GammaMarket> = events.into_iter().flat_map(|e| e.markets).collect();

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
                        .unwrap_or(0.0)
                        .partial_cmp(&b.liquidity_num.unwrap_or(0.0)),
                    "volume" => a
                        .volume_num
                        .unwrap_or(0.0)
                        .partial_cmp(&b.volume_num.unwrap_or(0.0)),
                    "volume24hr" => a
                        .volume_24hr
                        .unwrap_or(0.0)
                        .partial_cmp(&b.volume_24hr.unwrap_or(0.0)),
                    "competitive" => a
                        .competitive
                        .unwrap_or(0.0)
                        .partial_cmp(&b.competitive.unwrap_or(0.0)),
                    "spread" => a
                        .spread
                        .unwrap_or(f64::MAX)
                        .partial_cmp(&b.spread.unwrap_or(f64::MAX)),
                    "best_bid" => a
                        .best_bid
                        .unwrap_or(0.0)
                        .partial_cmp(&b.best_bid.unwrap_or(0.0)),
                    "one_day_price_change" => a
                        .one_day_price_change
                        .unwrap_or(0.0)
                        .partial_cmp(&b.one_day_price_change.unwrap_or(0.0)),
                    "volume_1wk" => a
                        .volume_1wk
                        .unwrap_or(0.0)
                        .partial_cmp(&b.volume_1wk.unwrap_or(0.0)),
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
        const PAGE_LIMIT: u32 = 100;
        let page_size = base_params.limit.unwrap_or(PAGE_LIMIT);
        let max_events = base_params.max_events;
        let mut all_events = Vec::new();
        let mut offset: u32 = base_params.offset.unwrap_or(0);

        loop {
            let params = GetGammaEventsParams {
                limit: Some(page_size),
                offset: Some(offset),
                ..base_params.clone()
            };

            let page = self.inner.get_gamma_events(params).await?;
            let page_len = page.len() as u32;
            all_events.extend(page);

            if let Some(cap) = max_events
                && all_events.len() as u32 >= cap
            {
                all_events.truncate(cap as usize);
                break;
            }

            if page_len < page_size {
                break;
            }

            offset += page_size;
        }

        Ok(all_events)
    }

    /// Fetches instruments from events matching full query params (paginated).
    pub async fn request_instruments_by_event_params(
        &self,
        params: GetGammaEventsParams,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let events = self.fetch_gamma_events_paginated(params).await?;
        let ts_init = self.clock.get_time_ns();
        let markets: Vec<GammaMarket> = events.into_iter().flat_map(|e| e.markets).collect();
        let instruments = parse_markets_to_instruments(&markets, ts_init);
        log::debug!(
            "Parsed {} instruments from event params query",
            instruments.len()
        );
        Ok(instruments)
    }

    /// Searches for instruments via the Gamma public search endpoint.
    pub async fn request_instruments_by_search(
        &self,
        params: GetSearchParams,
    ) -> anyhow::Result<Vec<InstrumentAny>> {
        let response = self.inner.get_public_search(params).await?;
        let ts_init = self.clock.get_time_ns();

        let mut instruments = Vec::new();

        if let Some(markets) = &response.markets {
            instruments.extend(parse_markets_to_instruments(markets, ts_init));
        }

        if let Some(events) = &response.events {
            let event_markets: Vec<GammaMarket> =
                events.iter().flat_map(|e| e.markets.clone()).collect();
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
