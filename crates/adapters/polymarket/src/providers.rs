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
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::{
    common::consts::GAMMA_CONDITION_IDS_BATCH_SIZE,
    config::PolymarketInstrumentProviderConfig,
    filters::InstrumentFilter,
    http::{
        gamma::PolymarketGammaHttpClient,
        models::GammaTag,
        query::{GetGammaEventsParams, GetGammaMarketsParams},
    },
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
    config: PolymarketInstrumentProviderConfig,
}

impl Debug for PolymarketInstrumentProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PolymarketInstrumentProvider))
            .field("store", &self.store)
            .field("http_client", &self.http_client)
            .field("token_index_len", &self.token_index.len())
            .field("filters", &self.filters)
            .field("config", &self.config)
            .finish()
    }
}

impl PolymarketInstrumentProvider {
    /// Creates a new [`PolymarketInstrumentProvider`] with an empty store and no filters.
    #[must_use]
    pub fn new(
        http_client: PolymarketGammaHttpClient,
        config: Option<PolymarketInstrumentProviderConfig>,
    ) -> Self {
        Self {
            store: InstrumentStore::new(),
            http_client,
            token_index: AHashMap::new(),
            filters: Vec::new(),
            config: config.unwrap_or_default(),
        }
    }

    /// Creates a new [`PolymarketInstrumentProvider`] with multiple filters.
    #[must_use]
    pub fn with_filters(
        http_client: PolymarketGammaHttpClient,
        config: Option<PolymarketInstrumentProviderConfig>,
        filters: Vec<Arc<dyn InstrumentFilter>>,
    ) -> Self {
        Self {
            store: InstrumentStore::new(),
            http_client,
            token_index: AHashMap::new(),
            filters,
            config: config.unwrap_or_default(),
        }
    }

    /// Creates a new [`PolymarketInstrumentProvider`] with a single filter.
    #[must_use]
    pub fn with_filter(
        http_client: PolymarketGammaHttpClient,
        config: Option<PolymarketInstrumentProviderConfig>,
        filter: Arc<dyn InstrumentFilter>,
    ) -> Self {
        Self {
            store: InstrumentStore::new(),
            http_client,
            token_index: AHashMap::new(),
            filters: vec![filter],
            config: config.unwrap_or_default(),
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

    /// Returns the configured provider config.
    #[must_use]
    pub fn config(&self) -> &PolymarketInstrumentProviderConfig {
        &self.config
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

    /// Loads instruments for the given event slugs additively into the store.
    ///
    /// Unlike [`Self::load_all`], this does **not** clear existing instruments or
    /// mark the store as initialized, allowing incremental loading of
    /// event-scoped markets alongside bulk data.
    pub async fn load_by_event_slugs(&mut self, slugs: Vec<String>) -> anyhow::Result<()> {
        let instruments = self
            .http_client
            .request_instruments_by_event_slugs(slugs)
            .await?;
        self.add_instruments(instruments);
        Ok(())
    }

    /// Initializes the provider using its configured bootstrap scope.
    pub async fn initialize(&mut self, reload: bool) -> anyhow::Result<()> {
        if self.store.is_initialized() && !reload {
            return Ok(());
        }

        if self.config.should_load_all() {
            self.load_scoped_all().await?;
            self.store.set_initialized();
            return Ok(());
        }

        if self.config.has_load_ids() {
            let load_ids = self.config.load_ids.clone().unwrap_or_default();
            let filters = self.config.filters.clone();
            self.load_ids(&load_ids, filters.as_ref()).await?;
            self.store.set_initialized();
            return Ok(());
        }

        if self.config.log_warnings {
            log::warn!(
                "No Polymarket instrument bootstrap configured: set instrument_config.load_all, instrument_config.load_ids, instrument_config.event_slugs, instrument_config.market_slugs, or instrument_config.event_slug_builder"
            );
        }
        Ok(())
    }

    async fn load_scoped_all(&mut self) -> anyhow::Result<()> {
        let has_explicit_slug_scope = self.config.event_slug_builder.is_some()
            || self
                .config
                .event_slugs
                .as_ref()
                .is_some_and(|slugs| !slugs.is_empty())
            || self
                .config
                .market_slugs
                .as_ref()
                .is_some_and(|slugs| !slugs.is_empty());
        let event_slugs = self.resolve_event_slugs()?;
        let market_slugs = self
            .config
            .market_slugs
            .clone()
            .unwrap_or_default()
            .into_iter()
            .filter(|slug| !slug.trim().is_empty())
            .collect::<Vec<_>>();

        if !event_slugs.is_empty() {
            self.load_by_event_slugs(event_slugs).await?;
        }

        if !market_slugs.is_empty() {
            self.load_by_slugs(market_slugs).await?;
        }

        if has_explicit_slug_scope {
            return Ok(());
        }

        let filters = self.config.filters.clone();
        self.load_all(filters.as_ref()).await
    }

    fn resolve_event_slugs(&self) -> anyhow::Result<Vec<String>> {
        if let Some(builder) = self.config.event_slug_builder.as_ref() {
            return builder.build_event_slugs();
        }

        Ok(self
            .config
            .event_slugs
            .clone()
            .unwrap_or_default()
            .into_iter()
            .filter(|slug| !slug.trim().is_empty())
            .collect())
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

/// Fetches instruments using the configured provider bootstrap scope without
/// mutating any provider state.
pub async fn fetch_configured_instruments(
    http_client: &PolymarketGammaHttpClient,
    config: &PolymarketInstrumentProviderConfig,
    filters: &[Arc<dyn InstrumentFilter>],
) -> anyhow::Result<Vec<InstrumentAny>> {
    let mut instruments = Vec::new();

    if config.should_load_all() {
        let has_explicit_slug_scope = config.event_slug_builder.is_some()
            || config
                .event_slugs
                .as_ref()
                .is_some_and(|slugs| !slugs.is_empty())
            || config
                .market_slugs
                .as_ref()
                .is_some_and(|slugs| !slugs.is_empty());
        let event_slugs = if let Some(builder) = config.event_slug_builder.as_ref() {
            builder.build_event_slugs()?
        } else {
            config
                .event_slugs
                .clone()
                .unwrap_or_default()
                .into_iter()
                .filter(|slug| !slug.trim().is_empty())
                .collect::<Vec<_>>()
        };

        let market_slugs = config
            .market_slugs
            .clone()
            .unwrap_or_default()
            .into_iter()
            .filter(|slug| !slug.trim().is_empty())
            .collect::<Vec<_>>();

        if !event_slugs.is_empty() {
            instruments.extend(
                http_client
                    .request_instruments_by_event_slugs(event_slugs)
                    .await?,
            );
        }

        if !market_slugs.is_empty() {
            instruments.extend(
                http_client
                    .request_instruments_by_slugs(market_slugs)
                    .await?,
            );
        }

        if has_explicit_slug_scope {
            // Explicit slug scoping should never broaden into a full-universe fetch.
        } else if filters.is_empty() {
            if let Some(map) = config.filters.as_ref() {
                if map.is_empty() {
                    instruments.extend(http_client.request_instruments().await?);
                } else {
                    let params = build_gamma_params_from_hashmap(map)?;
                    instruments.extend(http_client.request_instruments_by_params(params).await?);
                }
            } else {
                instruments.extend(http_client.request_instruments().await?);
            }
        } else {
            instruments.extend(fetch_instruments(http_client, filters).await?);
        }
    } else if config.has_load_ids() {
        let base_params = config
            .filters
            .as_ref()
            .map(build_gamma_params_from_hashmap)
            .transpose()?
            .unwrap_or_default();

        let condition_ids = config
            .load_ids
            .clone()
            .unwrap_or_default()
            .into_iter()
            .filter_map(|id| extract_condition_id(&id).ok())
            .collect::<AHashSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();

        for chunk in condition_ids.chunks(GAMMA_CONDITION_IDS_BATCH_SIZE) {
            let params = GetGammaMarketsParams {
                condition_ids: Some(chunk.to_vec()),
                ..base_params.clone()
            };
            instruments.extend(http_client.request_instruments_by_params(params).await?);
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

/// Builds validated market keyset parameters from string key/value filters.
///
/// # Errors
///
/// Returns an error for unknown keys, malformed values, or invalid filter combinations.
pub fn build_gamma_params_from_hashmap(
    map: &HashMap<String, String>,
) -> anyhow::Result<GetGammaMarketsParams> {
    for key in map.keys() {
        match key.as_str() {
            "is_active"
            | "active"
            | "closed"
            | "archived"
            | "id"
            | "limit"
            | "offset"
            | "order"
            | "ascending"
            | "slug"
            | "clob_token_ids"
            | "condition_ids"
            | "question_ids"
            | "market_maker_address"
            | "liquidity_num_min"
            | "liquidity_num_max"
            | "volume_num_min"
            | "volume_num_max"
            | "start_date_min"
            | "start_date_max"
            | "end_date_min"
            | "end_date_max"
            | "tag_id"
            | "related_tags"
            | "tag_match"
            | "decimalized"
            | "cyom"
            | "rfq_enabled"
            | "uma_resolution_status"
            | "game_id"
            | "sports_market_types"
            | "include_tag"
            | "locale"
            | "max_markets" => {}
            _ => anyhow::bail!("Unknown Gamma market filter key '{key}'"),
        }
    }

    let mut params = GetGammaMarketsParams::default();

    if map
        .get("is_active")
        .map(|value| parse_gamma_filter_bool("market", "is_active", value))
        .transpose()?
        .unwrap_or(false)
    {
        params.active = Some(true);
        params.archived = Some(false);
        params.closed = Some(false);
    }

    if let Some(v) = map.get("active") {
        params.active = Some(parse_gamma_filter_bool("market", "active", v)?);
    }

    if let Some(v) = map.get("closed") {
        params.closed = Some(parse_gamma_filter_bool("market", "closed", v)?);
    }

    if let Some(v) = map.get("archived") {
        params.archived = Some(parse_gamma_filter_bool("market", "archived", v)?);
    }

    if let Some(v) = map.get("id") {
        params.id = Some(parse_gamma_numeric_filter_list("market", "id", v)?);
    }

    if let Some(v) = map.get("slug") {
        params.slug = Some(parse_gamma_filter_list("market", "slug", v)?);
    }

    if let Some(v) = map.get("tag_id") {
        params.tag_id = Some(parse_gamma_numeric_filter_list("market", "tag_id", v)?);
    }

    if let Some(v) = map.get("condition_ids") {
        params.condition_ids = Some(parse_gamma_filter_list("market", "condition_ids", v)?);
    }

    if let Some(v) = map.get("clob_token_ids") {
        params.clob_token_ids = Some(parse_gamma_filter_list("market", "clob_token_ids", v)?);
    }

    if let Some(v) = map.get("question_ids") {
        params.question_ids = Some(parse_gamma_filter_list("market", "question_ids", v)?);
    }

    if let Some(v) = map.get("market_maker_address") {
        params.market_maker_address = Some(parse_gamma_filter_list(
            "market",
            "market_maker_address",
            v,
        )?);
    }

    if let Some(v) = map.get("liquidity_num_min") {
        params.liquidity_num_min = Some(parse_gamma_filter_decimal(
            "market",
            "liquidity_num_min",
            v,
        )?);
    }

    if let Some(v) = map.get("liquidity_num_max") {
        params.liquidity_num_max = Some(parse_gamma_filter_decimal(
            "market",
            "liquidity_num_max",
            v,
        )?);
    }

    if let Some(v) = map.get("volume_num_min") {
        params.volume_num_min = Some(parse_gamma_filter_decimal("market", "volume_num_min", v)?);
    }

    if let Some(v) = map.get("volume_num_max") {
        params.volume_num_max = Some(parse_gamma_filter_decimal("market", "volume_num_max", v)?);
    }

    if let Some(v) = map.get("order") {
        params.order = Some(parse_gamma_filter_string("market", "order", v)?);
    }

    if let Some(v) = map.get("ascending") {
        params.ascending = Some(parse_gamma_filter_bool("market", "ascending", v)?);
    }

    if let Some(v) = map.get("limit") {
        params.limit = Some(parse_gamma_filter_u32("market", "limit", v)?.min(100));
    }

    if let Some(v) = map.get("offset") {
        params.offset = Some(parse_gamma_filter_u32("market", "offset", v)?);
    }

    if let Some(v) = map.get("start_date_min") {
        params.start_date_min = Some(parse_gamma_filter_string("market", "start_date_min", v)?);
    }

    if let Some(v) = map.get("start_date_max") {
        params.start_date_max = Some(parse_gamma_filter_string("market", "start_date_max", v)?);
    }

    if let Some(v) = map.get("end_date_min") {
        params.end_date_min = Some(parse_gamma_filter_string("market", "end_date_min", v)?);
    }

    if let Some(v) = map.get("end_date_max") {
        params.end_date_max = Some(parse_gamma_filter_string("market", "end_date_max", v)?);
    }

    if let Some(v) = map.get("related_tags") {
        params.related_tags = Some(parse_gamma_filter_bool("market", "related_tags", v)?);
    }

    if let Some(v) = map.get("tag_match") {
        params.tag_match = Some(parse_gamma_filter_string("market", "tag_match", v)?);
    }

    if let Some(v) = map.get("decimalized") {
        params.decimalized = Some(parse_gamma_filter_bool("market", "decimalized", v)?);
    }

    if let Some(v) = map.get("cyom") {
        params.cyom = Some(parse_gamma_filter_bool("market", "cyom", v)?);
    }

    if let Some(v) = map.get("rfq_enabled") {
        params.rfq_enabled = Some(parse_gamma_filter_bool("market", "rfq_enabled", v)?);
    }

    if let Some(v) = map.get("uma_resolution_status") {
        params.uma_resolution_status = Some(parse_gamma_filter_string(
            "market",
            "uma_resolution_status",
            v,
        )?);
    }

    if let Some(v) = map.get("game_id") {
        params.game_id = Some(parse_gamma_filter_string("market", "game_id", v)?);
    }

    if let Some(v) = map.get("sports_market_types") {
        params.sports_market_types =
            Some(parse_gamma_filter_list("market", "sports_market_types", v)?);
    }

    if let Some(v) = map.get("include_tag") {
        params.include_tag = Some(parse_gamma_filter_bool("market", "include_tag", v)?);
    }

    if let Some(v) = map.get("locale") {
        params.locale = Some(parse_gamma_filter_string("market", "locale", v)?);
    }

    if let Some(v) = map.get("max_markets") {
        params.max_markets = Some(parse_gamma_filter_u32("market", "max_markets", v)?);
    }

    params.validate_keyset().map_err(|e| anyhow::anyhow!(e))?;
    Ok(params)
}

/// Builds validated event keyset parameters from string key/value filters.
///
/// # Errors
///
/// Returns an error for unknown keys, malformed values, or invalid filter combinations.
pub fn build_gamma_event_params_from_hashmap(
    map: &HashMap<String, String>,
) -> anyhow::Result<GetGammaEventsParams> {
    for key in map.keys() {
        match key.as_str() {
            "is_active" | "active" | "closed" | "archived" | "id" | "slug" | "live"
            | "featured" | "cyom" | "title_search" | "liquidity_min" | "liquidity_max"
            | "volume_min" | "volume_max" | "start_date_min" | "start_date_max"
            | "end_date_min" | "end_date_max" | "start_time_min" | "start_time_max" | "tag_id"
            | "tag_slug" | "exclude_tag_id" | "related_tags" | "tag_match" | "series_id"
            | "game_id" | "event_date" | "event_week" | "featured_order" | "recurrence"
            | "created_by" | "parent_event_id" | "include_children" | "partner_slug"
            | "include_chat" | "include_template" | "include_best_lines" | "locale" | "order"
            | "ascending" | "limit" | "offset" | "max_events" => {}
            _ => anyhow::bail!("Unknown Gamma event filter key '{key}'"),
        }
    }

    let mut params = GetGammaEventsParams::default();

    if map
        .get("is_active")
        .map(|value| parse_gamma_filter_bool("event", "is_active", value))
        .transpose()?
        .unwrap_or(false)
    {
        params.active = Some(true);
        params.archived = Some(false);
        params.closed = Some(false);
    }

    macro_rules! set_bool {
        ($field:ident) => {
            if let Some(value) = map.get(stringify!($field)) {
                params.$field = Some(parse_gamma_filter_bool("event", stringify!($field), value)?);
            }
        };
    }
    macro_rules! set_string {
        ($field:ident) => {
            if let Some(value) = map.get(stringify!($field)) {
                params.$field = Some(parse_gamma_filter_string(
                    "event",
                    stringify!($field),
                    value,
                )?);
            }
        };
    }
    macro_rules! set_decimal {
        ($field:ident) => {
            if let Some(value) = map.get(stringify!($field)) {
                params.$field = Some(parse_gamma_filter_decimal(
                    "event",
                    stringify!($field),
                    value,
                )?);
            }
        };
    }
    macro_rules! set_u32 {
        ($field:ident) => {
            if let Some(value) = map.get(stringify!($field)) {
                params.$field = Some(parse_gamma_filter_u32("event", stringify!($field), value)?);
            }
        };
    }
    macro_rules! set_u64 {
        ($field:ident) => {
            if let Some(value) = map.get(stringify!($field)) {
                params.$field = Some(parse_gamma_filter_u64("event", stringify!($field), value)?);
            }
        };
    }
    macro_rules! set_strings {
        ($field:ident) => {
            if let Some(value) = map.get(stringify!($field)) {
                params.$field = Some(parse_gamma_filter_list("event", stringify!($field), value)?);
            }
        };
    }
    macro_rules! set_u64s {
        ($field:ident) => {
            if let Some(value) = map.get(stringify!($field)) {
                params.$field = Some(parse_gamma_numeric_filter_list(
                    "event",
                    stringify!($field),
                    value,
                )?);
            }
        };
    }

    set_bool!(active);
    set_bool!(closed);
    set_bool!(archived);
    set_bool!(live);
    set_bool!(featured);
    set_bool!(cyom);
    set_bool!(related_tags);
    set_bool!(featured_order);
    set_bool!(include_children);
    set_bool!(include_chat);
    set_bool!(include_template);
    set_bool!(include_best_lines);
    set_bool!(ascending);
    set_strings!(slug);
    set_strings!(created_by);
    set_u64s!(id);
    set_u64s!(tag_id);
    set_u64s!(exclude_tag_id);
    set_u64s!(series_id);
    set_u64s!(game_id);
    set_string!(title_search);
    set_string!(start_date_min);
    set_string!(start_date_max);
    set_string!(end_date_min);
    set_string!(end_date_max);
    set_string!(start_time_min);
    set_string!(start_time_max);
    set_string!(tag_slug);
    set_string!(tag_match);
    set_string!(event_date);
    set_string!(recurrence);
    set_string!(partner_slug);
    set_string!(locale);
    set_string!(order);
    set_decimal!(liquidity_min);
    set_decimal!(liquidity_max);
    set_decimal!(volume_min);
    set_decimal!(volume_max);
    set_u32!(event_week);
    set_u32!(limit);
    set_u32!(offset);
    set_u32!(max_events);
    set_u64!(parent_event_id);

    params.validate_keyset().map_err(anyhow::Error::msg)?;
    Ok(params)
}

fn parse_gamma_filter_bool(scope: &str, key: &str, value: &str) -> anyhow::Result<bool> {
    if value.eq_ignore_ascii_case("true") {
        Ok(true)
    } else if value.eq_ignore_ascii_case("false") {
        Ok(false)
    } else {
        anyhow::bail!("Gamma {scope} filter '{key}' must be true or false, was '{value}'")
    }
}

fn parse_gamma_filter_u32(scope: &str, key: &str, value: &str) -> anyhow::Result<u32> {
    value.parse::<u32>().map_err(|e| {
        anyhow::anyhow!("Gamma {scope} filter '{key}' must be an unsigned integer: {e}")
    })
}

fn parse_gamma_filter_u64(scope: &str, key: &str, value: &str) -> anyhow::Result<u64> {
    value.parse::<u64>().map_err(|e| {
        anyhow::anyhow!("Gamma {scope} filter '{key}' must be an unsigned integer: {e}")
    })
}

fn parse_gamma_filter_decimal(scope: &str, key: &str, value: &str) -> anyhow::Result<Decimal> {
    value
        .parse::<Decimal>()
        .map_err(|e| anyhow::anyhow!("Gamma {scope} filter '{key}' must be a decimal number: {e}"))
}

fn parse_gamma_filter_list(scope: &str, key: &str, value: &str) -> anyhow::Result<Vec<String>> {
    let values = value
        .split(',')
        .map(str::trim)
        .map(str::to_string)
        .collect::<Vec<_>>();

    if values.is_empty() || values.iter().any(String::is_empty) {
        anyhow::bail!("Gamma {scope} filter '{key}' must contain non-empty comma-separated values")
    }
    Ok(values)
}

fn parse_gamma_numeric_filter_list(
    scope: &str,
    key: &str,
    value: &str,
) -> anyhow::Result<Vec<u64>> {
    parse_gamma_filter_list(scope, key, value)?
        .into_iter()
        .map(|item| {
            item.parse::<u64>().map_err(|e| {
                anyhow::anyhow!(
                    "Gamma {scope} filter '{key}' values must be unsigned integers: {e}"
                )
            })
        })
        .collect()
}

fn parse_gamma_filter_string(scope: &str, key: &str, value: &str) -> anyhow::Result<String> {
    if value.trim().is_empty() {
        anyhow::bail!("Gamma {scope} filter '{key}' cannot be empty")
    }
    Ok(value.to_string())
}

/// Resolves a tag slug to a tag ID by querying the Gamma tags endpoint.
pub async fn resolve_tag_slug(
    client: &PolymarketGammaHttpClient,
    slug: &str,
) -> anyhow::Result<u64> {
    let tags = client.request_tags().await?;
    let tag_id = tags
        .iter()
        .find(|t| t.slug.as_deref() == Some(slug))
        .map(|t| t.id.as_str())
        .ok_or_else(|| anyhow::anyhow!("Tag slug '{slug}' not found"))?;
    tag_id
        .parse::<u64>()
        .map_err(|e| anyhow::anyhow!("Tag slug '{slug}' returned invalid ID '{tag_id}': {e}"))
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
                    let params = build_gamma_params_from_hashmap(map)?;
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

        if condition_ids.is_empty() {
            return Ok(());
        }

        let base_params = filters
            .map(build_gamma_params_from_hashmap)
            .transpose()?
            .unwrap_or_default();

        for chunk in condition_ids.chunks(GAMMA_CONDITION_IDS_BATCH_SIZE) {
            let params = GetGammaMarketsParams {
                condition_ids: Some(chunk.to_vec()),
                ..base_params.clone()
            };
            let instruments = self
                .http_client
                .request_instruments_by_params(params)
                .await?;
            self.add_instruments(instruments);
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
                condition_ids: Some(vec![cid]),
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
