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

//! Instrument provider for the Polymarket adapter.

use std::{collections::HashMap, fmt::Debug, sync::Arc};

use ahash::{AHashMap, AHashSet};
use async_trait::async_trait;
use nautilus_common::providers::{InstrumentProvider, InstrumentStore};
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use ustr::Ustr;

use crate::{
    filters::InstrumentFilter,
    http::{gamma::PolymarketGammaHttpClient, models::GammaTag, query::GetGammaMarketsParams},
};

/// Provides Polymarket instruments via the Gamma API.
///
/// Wraps [`PolymarketGammaHttpClient`] with an [`InstrumentStore`] and a
/// token_id index for resolving WebSocket asset IDs to instruments.
///
/// Optional [`InstrumentFilter`]s control which instruments are loaded
/// during `load_all()`. Without filters, all active markets are fetched.
pub struct PolymarketInstrumentProvider {
    store: InstrumentStore,
    http_client: PolymarketGammaHttpClient,
    token_index: AHashMap<Ustr, InstrumentId>,
    filters: Vec<Arc<dyn InstrumentFilter>>,
}

impl Debug for PolymarketInstrumentProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PolymarketInstrumentProvider))
            .field("store", &self.store)
            .field("http_client", &self.http_client)
            .field("token_index_len", &self.token_index.len())
            .field("filters", &self.filters)
            .finish()
    }
}

impl PolymarketInstrumentProvider {
    /// Creates a new [`PolymarketInstrumentProvider`] with an empty store and no filters.
    #[must_use]
    pub fn new(http_client: PolymarketGammaHttpClient) -> Self {
        Self {
            store: InstrumentStore::new(),
            http_client,
            token_index: AHashMap::new(),
            filters: Vec::new(),
        }
    }

    /// Creates a new [`PolymarketInstrumentProvider`] with multiple filters.
    #[must_use]
    pub fn with_filters(
        http_client: PolymarketGammaHttpClient,
        filters: Vec<Arc<dyn InstrumentFilter>>,
    ) -> Self {
        Self {
            store: InstrumentStore::new(),
            http_client,
            token_index: AHashMap::new(),
            filters,
        }
    }

    /// Creates a new [`PolymarketInstrumentProvider`] with a single filter.
    #[must_use]
    pub fn with_filter(
        http_client: PolymarketGammaHttpClient,
        filter: Arc<dyn InstrumentFilter>,
    ) -> Self {
        Self {
            store: InstrumentStore::new(),
            http_client,
            token_index: AHashMap::new(),
            filters: vec![filter],
        }
    }

    /// Adds an instrument filter for subsequent `load_all()` calls.
    pub fn add_filter(&mut self, filter: Arc<dyn InstrumentFilter>) {
        self.filters.push(filter);
    }

    /// Clears all instrument filters, reverting to bulk load behavior.
    pub fn clear_filters(&mut self) {
        self.filters.clear();
    }

    /// Returns the instrument for the given token ID, if found.
    #[must_use]
    pub fn get_by_token_id(&self, token_id: &Ustr) -> Option<&InstrumentAny> {
        let instrument_id = self.token_index.get(token_id)?;
        self.store.find(instrument_id)
    }

    /// Builds a frozen snapshot mapping token IDs to instruments.
    ///
    /// Used to provide the WS handler task with a read-only lookup
    /// table after instruments have been loaded.
    #[must_use]
    pub fn build_token_map(&self) -> AHashMap<Ustr, InstrumentAny> {
        self.token_index
            .iter()
            .filter_map(|(token_id, instrument_id)| {
                self.store
                    .find(instrument_id)
                    .map(|inst| (*token_id, inst.clone()))
            })
            .collect()
    }

    /// Loads instruments for the given slugs additively into the store.
    ///
    /// Unlike [`Self::load_all`], this does **not** clear existing instruments or
    /// mark the store as initialized, allowing incremental loading of
    /// slug-based markets alongside bulk data.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request or parsing fails.
    pub async fn load_by_slugs(&mut self, slugs: Vec<String>) -> anyhow::Result<()> {
        let instruments = self.http_client.request_instruments_by_slugs(slugs).await?;

        for instrument in &instruments {
            self.token_index.insert(
                Ustr::from(instrument.raw_symbol().as_str()),
                instrument.id(),
            );
        }

        self.store.add_bulk(instruments);

        Ok(())
    }

    /// Returns a clone of the configured instrument filters.
    #[must_use]
    pub fn filters(&self) -> Vec<Arc<dyn InstrumentFilter>> {
        self.filters.clone()
    }

    /// Returns a reference to the underlying HTTP client.
    #[must_use]
    pub fn http_client(&self) -> &PolymarketGammaHttpClient {
        &self.http_client
    }

    /// Fetches available tags from the Gamma API.
    pub async fn list_tags(&self) -> anyhow::Result<Vec<GammaTag>> {
        self.http_client.request_tags().await
    }

    pub fn add_instruments(&mut self, instruments: Vec<InstrumentAny>) {
        for inst in &instruments {
            self.token_index
                .insert(Ustr::from(inst.raw_symbol().as_str()), inst.id());
        }
        self.store.add_bulk(instruments);
    }

    /// Loads instruments using all configured filters, combining results from
    /// each filter's methods that return `Some`.
    async fn load_filtered(&self) -> anyhow::Result<Vec<InstrumentAny>> {
        fetch_instruments(&self.http_client, &self.filters).await
    }
}

/// Fetches instruments from the Gamma API, respecting any configured filters.
pub async fn fetch_instruments(
    http_client: &PolymarketGammaHttpClient,
    filters: &[Arc<dyn InstrumentFilter>],
) -> anyhow::Result<Vec<InstrumentAny>> {
    if filters.is_empty() {
        return http_client.request_instruments().await;
    }

    let mut instruments = Vec::new();

    for filter in filters {
        if let Some(slugs) = filter.market_slugs()
            && !slugs.is_empty()
        {
            let result = http_client.request_instruments_by_slugs(slugs).await?;
            instruments.extend(result);
        }

        if let Some(event_slugs) = filter.event_slugs()
            && !event_slugs.is_empty()
        {
            let result = http_client
                .request_instruments_by_event_slugs(event_slugs)
                .await?;
            instruments.extend(result);
        }

        if let Some(params) = filter.query_params() {
            let result = http_client.request_instruments_by_params(params).await?;
            instruments.extend(result);
        }

        if let Some(event_queries) = filter.event_queries() {
            for (event_slug, params) in event_queries {
                let result = http_client
                    .request_instruments_by_event_query(&event_slug, params)
                    .await?;
                instruments.extend(result);
            }
        }

        if let Some(params) = filter.event_params() {
            let result = http_client
                .request_instruments_by_event_params(params)
                .await?;
            instruments.extend(result);
        }

        if let Some(params) = filter.search_params() {
            let result = http_client.request_instruments_by_search(params).await?;
            instruments.extend(result);
        }
    }

    let mut seen = AHashSet::new();
    instruments.retain(|inst| seen.insert(inst.id()));
    instruments.retain(|inst| filters.iter().all(|f| f.accept(inst)));

    Ok(instruments)
}

/// Extracts the condition ID from an instrument symbol.
///
/// Polymarket instrument symbols follow the pattern `{condition_id}-{token_id}`.
/// The condition_id is a hex string (e.g. `0xabc123...`) and the token_id is a
/// large decimal number. This extracts the condition_id by splitting at the last `-`.
pub fn extract_condition_id(instrument_id: &InstrumentId) -> anyhow::Result<String> {
    let symbol = instrument_id.symbol.as_str();
    symbol
        .rfind('-')
        .map(|idx| symbol[..idx].to_string())
        .ok_or_else(|| {
            anyhow::anyhow!("Cannot extract condition_id from symbol '{symbol}': no '-' separator")
        })
}

/// Builds `GetGammaMarketsParams` from a `HashMap<String, String>`.
pub fn build_gamma_params_from_hashmap(map: &HashMap<String, String>) -> GetGammaMarketsParams {
    let mut params = GetGammaMarketsParams::default();

    if let Some(v) = map.get("active") {
        params.active = v.parse().ok();
    }

    if let Some(v) = map.get("closed") {
        params.closed = v.parse().ok();
    }

    if let Some(v) = map.get("archived") {
        params.archived = v.parse().ok();
    }

    if let Some(v) = map.get("slug") {
        params.slug = Some(v.clone());
    }

    if let Some(v) = map.get("tag_id") {
        params.tag_id = Some(v.clone());
    }

    if let Some(v) = map.get("condition_ids") {
        params.condition_ids = Some(v.clone());
    }

    if let Some(v) = map.get("clob_token_ids") {
        params.clob_token_ids = Some(v.clone());
    }

    if let Some(v) = map.get("liquidity_num_min") {
        params.liquidity_num_min = v.parse().ok();
    }

    if let Some(v) = map.get("liquidity_num_max") {
        params.liquidity_num_max = v.parse().ok();
    }

    if let Some(v) = map.get("volume_num_min") {
        params.volume_num_min = v.parse().ok();
    }

    if let Some(v) = map.get("volume_num_max") {
        params.volume_num_max = v.parse().ok();
    }

    if let Some(v) = map.get("order") {
        params.order = Some(v.clone());
    }

    if let Some(v) = map.get("ascending") {
        params.ascending = v.parse().ok();
    }

    if let Some(v) = map.get("limit") {
        params.limit = v.parse().ok();
    }

    if let Some(v) = map.get("max_markets") {
        params.max_markets = v.parse().ok();
    }

    params
}

/// Resolves a tag slug to a tag ID by querying the Gamma tags endpoint.
pub async fn resolve_tag_slug(
    client: &PolymarketGammaHttpClient,
    slug: &str,
) -> anyhow::Result<String> {
    let tags = client.request_tags().await?;
    tags.iter()
        .find(|t| t.slug.as_deref() == Some(slug))
        .map(|t| t.id.clone())
        .ok_or_else(|| anyhow::anyhow!("Tag slug '{slug}' not found"))
}

#[async_trait(?Send)]
impl InstrumentProvider for PolymarketInstrumentProvider {
    fn store(&self) -> &InstrumentStore {
        &self.store
    }

    fn store_mut(&mut self) -> &mut InstrumentStore {
        &mut self.store
    }

    async fn load_all(&mut self, filters: Option<&HashMap<String, String>>) -> anyhow::Result<()> {
        let instruments = if self.filters.is_empty() {
            // If HashMap filters are provided, convert to Gamma params
            if let Some(map) = filters {
                if map.is_empty() {
                    self.http_client.request_instruments().await?
                } else {
                    let params = build_gamma_params_from_hashmap(map);
                    self.http_client
                        .request_instruments_by_params(params)
                        .await?
                }
            } else {
                self.http_client.request_instruments().await?
            }
        } else {
            self.load_filtered().await?
        };

        self.store.clear();
        self.token_index.clear();
        self.add_instruments(instruments);
        self.store.set_initialized();

        Ok(())
    }

    async fn load_ids(
        &mut self,
        instrument_ids: &[InstrumentId],
        filters: Option<&HashMap<String, String>>,
    ) -> anyhow::Result<()> {
        let missing: Vec<_> = instrument_ids
            .iter()
            .filter(|id| !self.store.contains(id))
            .collect();

        if missing.is_empty() {
            return Ok(());
        }

        // Extract unique condition IDs from instrument symbols
        // Symbol format: "{condition_id}-{token_id}"
        let mut condition_ids: Vec<String> = missing
            .iter()
            .filter_map(|id| extract_condition_id(id).ok())
            .collect();
        condition_ids.sort();
        condition_ids.dedup();

        if !condition_ids.is_empty() && condition_ids.len() <= 100 {
            let params = GetGammaMarketsParams {
                condition_ids: Some(condition_ids.join(",")),
                ..Default::default()
            };
            let instruments = self
                .http_client
                .request_instruments_by_params(params)
                .await?;
            self.add_instruments(instruments);
        } else {
            // Too many to batch, fall back to full load
            self.load_all(filters).await?;
        }

        Ok(())
    }

    async fn load(
        &mut self,
        instrument_id: &InstrumentId,
        filters: Option<&HashMap<String, String>>,
    ) -> anyhow::Result<()> {
        if self.store.contains(instrument_id) {
            return Ok(());
        }

        // Try direct fetch via condition_id extracted from symbol
        if let Ok(cid) = extract_condition_id(instrument_id) {
            let params = GetGammaMarketsParams {
                condition_ids: Some(cid),
                ..Default::default()
            };

            if let Ok(instruments) = self.http_client.request_instruments_by_params(params).await {
                self.add_instruments(instruments);

                if self.store.contains(instrument_id) {
                    return Ok(());
                }
            }
        }

        // Fallback: full load_all if not initialized
        if !self.store.is_initialized() {
            self.load_all(filters).await?;
        }

        if self.store.contains(instrument_id) {
            Ok(())
        } else {
            anyhow::bail!("Instrument {instrument_id} not found on Polymarket")
        }
    }
}
