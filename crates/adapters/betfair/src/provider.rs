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

//! Betfair instrument provider for loading instruments from the Navigation
//! and Betting APIs.

use std::{collections::HashMap, sync::Arc, time::SystemTime};

use ahash::AHashSet;
use async_trait::async_trait;
use nautilus_common::providers::{InstrumentProvider, InstrumentStore};
use nautilus_core::UnixNanos;
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::InstrumentAny,
    types::{Currency, Money},
};
use ustr::Ustr;

use crate::{
    common::{
        consts::{METHOD_GET_ACCOUNT_DETAILS, METHOD_LIST_MARKET_CATALOGUE},
        enums::MarketProjection,
        parse::{extract_market_id, parse_betfair_timestamp, parse_market_catalogue},
        types::MarketId,
    },
    http::{
        client::BetfairHttpClient,
        models::{
            AccountDetailsResponse, FlattenedMarket, ListMarketCatalogueParams, MarketCatalogue,
            MarketFilter, Navigation, NavigationChild, TimeRange,
        },
    },
};

/// Maximum number of market IDs per `listMarketCatalogue` request.
const CATALOGUE_BATCH_SIZE: usize = 50;

/// Filters for selecting markets from the Betfair navigation tree.
///
/// All fields use AND-logic: a market must match every provided filter.
/// `None` fields impose no constraint.
#[derive(Debug, Clone, Default)]
pub struct NavigationFilter {
    /// Event type IDs to include (e.g., "7" for Horse Racing).
    pub event_type_ids: Option<Vec<String>>,
    /// Event type names to include (e.g., "Horse Racing").
    pub event_type_names: Option<Vec<String>>,
    /// Event IDs to include.
    pub event_ids: Option<Vec<String>>,
    /// Country codes to include (e.g., "GB", "AU").
    pub country_codes: Option<Vec<String>>,
    /// Market types to include (e.g., "WIN", "PLACE").
    pub market_types: Option<Vec<String>>,
    /// Specific market IDs to include.
    pub market_ids: Option<Vec<String>>,
    /// Minimum market start time (ISO 8601 date, e.g. "2024-01-15T00:00:00Z").
    pub min_market_start_time: Option<String>,
    /// Maximum market start time (ISO 8601 date, e.g. "2024-12-31T00:00:00Z").
    pub max_market_start_time: Option<String>,
}

impl NavigationFilter {
    /// Returns `true` if the given market passes all filter criteria.
    #[must_use]
    pub fn matches(&self, market: &FlattenedMarket) -> bool {
        if let Some(ids) = &self.event_type_ids {
            match &market.event_type_id {
                Some(id) => {
                    if !ids.iter().any(|f| f == id) {
                        return false;
                    }
                }
                None => return false,
            }
        }

        if let Some(names) = &self.event_type_names {
            match &market.event_type_name {
                Some(name) => {
                    if !names.iter().any(|f| f == name.as_str()) {
                        return false;
                    }
                }
                None => return false,
            }
        }

        if let Some(ids) = &self.event_ids {
            match &market.event_id {
                Some(id) => {
                    if !ids.iter().any(|f| f == id) {
                        return false;
                    }
                }
                None => return false,
            }
        }

        if let Some(codes) = &self.country_codes {
            match &market.event_country_code {
                Some(cc) => {
                    if !codes.iter().any(|f| f == cc.as_str()) {
                        return false;
                    }
                }
                None => return false,
            }
        }

        if let Some(types) = &self.market_types {
            match &market.market_type {
                Some(mt) => {
                    if !types.iter().any(|f| f == mt.as_str()) {
                        return false;
                    }
                }
                None => return false,
            }
        }

        if let Some(ids) = &self.market_ids {
            match &market.market_id {
                Some(id) => {
                    if !ids.iter().any(|f| f == id) {
                        return false;
                    }
                }
                None => return false,
            }
        }

        if let Some(min_time) = &self.min_market_start_time {
            match (
                &market.market_start_time,
                parse_betfair_timestamp(min_time).ok(),
            ) {
                (Some(start_str), Some(min_ts)) => {
                    if let Ok(start_ts) = parse_betfair_timestamp(start_str)
                        && start_ts < min_ts
                    {
                        return false;
                    }
                }
                (None, _) => return false,
                _ => {}
            }
        }

        if let Some(max_time) = &self.max_market_start_time {
            match (
                &market.market_start_time,
                parse_betfair_timestamp(max_time).ok(),
            ) {
                (Some(start_str), Some(max_ts)) => {
                    if let Ok(start_ts) = parse_betfair_timestamp(start_str)
                        && start_ts > max_ts
                    {
                        return false;
                    }
                }
                (None, _) => return false,
                _ => {}
            }
        }

        true
    }
}

/// Context accumulated while descending the navigation tree.
#[derive(Debug, Clone, Default)]
struct NavContext {
    event_type_id: Option<String>,
    event_type_name: Option<Ustr>,
    event_id: Option<String>,
    event_name: Option<String>,
    event_country_code: Option<Ustr>,
}

/// Flattens the Betfair navigation tree into a list of [`FlattenedMarket`]s.
///
/// Recursively walks `EventType → Group → Event → Race → Market` nodes,
/// propagating parent context (event type, event, country) down to each
/// leaf market node.
#[must_use]
pub fn flatten_navigation(nav: &Navigation) -> Vec<FlattenedMarket> {
    let mut markets = Vec::new();

    if let Some(children) = &nav.children {
        collect_markets(children, &NavContext::default(), &mut markets);
    }
    markets
}

fn collect_markets(children: &[NavigationChild], ctx: &NavContext, out: &mut Vec<FlattenedMarket>) {
    for child in children {
        match child {
            NavigationChild::EventType(et) => {
                let new_ctx = NavContext {
                    event_type_id: et.id.clone(),
                    event_type_name: et.name,
                    ..ctx.clone()
                };

                if let Some(kids) = &et.children {
                    collect_markets(kids, &new_ctx, out);
                }
            }
            NavigationChild::Group(g) => {
                if let Some(kids) = &g.children {
                    collect_markets(kids, ctx, out);
                }
            }
            NavigationChild::Event(e) => {
                let new_ctx = NavContext {
                    event_id: e.id.clone(),
                    event_name: e.name.clone(),
                    event_country_code: e.country_code,
                    ..ctx.clone()
                };

                if let Some(kids) = &e.children {
                    collect_markets(kids, &new_ctx, out);
                }
            }
            NavigationChild::Race(r) => {
                if let Some(kids) = &r.children {
                    collect_markets(kids, ctx, out);
                }
            }
            NavigationChild::Market(m) => {
                out.push(FlattenedMarket {
                    event_type_id: ctx.event_type_id.clone(),
                    event_type_name: ctx.event_type_name,
                    event_id: ctx.event_id.clone(),
                    event_name: ctx.event_name.clone(),
                    event_country_code: ctx.event_country_code,
                    market_id: m.id.clone(),
                    market_name: m.name.clone(),
                    market_type: m.market_type,
                    market_start_time: m.market_start_time.clone(),
                    number_of_winners: m.number_of_winners,
                });
            }
        }
    }
}

/// Loads instruments from the Betfair Navigation and Betting APIs.
///
/// 1. Fetches the navigation tree via `send_navigation`
/// 2. Flattens and filters to matching market IDs
/// 3. Batches market IDs (max 50 per request)
/// 4. Calls `listMarketCatalogue` for each batch
/// 5. Parses results into [`InstrumentAny`] via `parse_market_catalogue`
///
/// # Errors
///
/// Returns an error if any API request fails or instrument parsing fails.
pub async fn load_instruments(
    client: &BetfairHttpClient,
    filter: &NavigationFilter,
    currency: Currency,
    min_notional: Option<Money>,
) -> anyhow::Result<Vec<InstrumentAny>> {
    let navigation: Navigation = client
        .send_navigation()
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let all_markets = flatten_navigation(&navigation);

    let filtered: Vec<&FlattenedMarket> =
        all_markets.iter().filter(|m| filter.matches(m)).collect();

    log::info!("Found {} markets matching filter", filtered.len());

    let market_ids: Vec<MarketId> = filtered
        .iter()
        .filter_map(|m| m.market_id.clone())
        .collect::<AHashSet<_>>()
        .into_iter()
        .collect();

    let time_range =
        if filter.min_market_start_time.is_some() || filter.max_market_start_time.is_some() {
            Some(TimeRange {
                from: filter.min_market_start_time.clone(),
                to: filter.max_market_start_time.clone(),
            })
        } else {
            None
        };

    let ts_init = UnixNanos::from(SystemTime::now());
    let mut all_instruments = Vec::new();

    for chunk in market_ids.chunks(CATALOGUE_BATCH_SIZE) {
        let params = ListMarketCatalogueParams {
            filter: MarketFilter {
                market_ids: Some(chunk.to_vec()),
                market_start_time: time_range.clone(),
                ..Default::default()
            },
            market_projection: Some(vec![
                MarketProjection::EventType,
                MarketProjection::Event,
                MarketProjection::Competition,
                MarketProjection::MarketDescription,
                MarketProjection::RunnerDescription,
                MarketProjection::RunnerMetadata,
                MarketProjection::MarketStartTime,
            ]),
            max_results: Some(chunk.len() as u32),
            sort: None,
            locale: None,
        };

        let catalogues: Vec<MarketCatalogue> = client
            .send_betting(METHOD_LIST_MARKET_CATALOGUE, &params)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        for catalogue in &catalogues {
            match parse_market_catalogue(catalogue, currency, ts_init, min_notional) {
                Ok(instruments) => all_instruments.extend(instruments),
                Err(e) => {
                    log::warn!("Failed to parse catalogue {}: {e}", catalogue.market_id);
                }
            }
        }
    }

    log::info!("Loaded {} instruments", all_instruments.len());
    Ok(all_instruments)
}

/// Betfair instrument provider backed by the Navigation and Betting APIs.
#[derive(Debug)]
pub struct BetfairInstrumentProvider {
    store: InstrumentStore,
    http_client: Arc<BetfairHttpClient>,
    nav_filter: NavigationFilter,
    currency: Currency,
    min_notional: Option<Money>,
}

impl BetfairInstrumentProvider {
    /// Creates a new [`BetfairInstrumentProvider`] instance.
    #[must_use]
    pub fn new(
        http_client: Arc<BetfairHttpClient>,
        nav_filter: NavigationFilter,
        currency: Currency,
        min_notional: Option<Money>,
    ) -> Self {
        Self {
            store: InstrumentStore::new(),
            http_client,
            nav_filter,
            currency,
            min_notional,
        }
    }

    /// Returns the currency used for instrument definitions.
    #[must_use]
    pub fn currency(&self) -> Currency {
        self.currency
    }

    /// Returns the default minimum notional for instruments.
    #[must_use]
    pub fn min_notional(&self) -> Option<Money> {
        self.min_notional
    }

    /// Fetches the account currency from the Betfair Account API.
    ///
    /// # Errors
    ///
    /// Returns an error if the API call fails or the currency code is missing/unknown.
    pub async fn get_account_currency(&self) -> anyhow::Result<Currency> {
        let details: AccountDetailsResponse = self
            .http_client
            .send_accounts(METHOD_GET_ACCOUNT_DETAILS, &serde_json::json!({}))
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let code = details
            .currency_code
            .ok_or_else(|| anyhow::anyhow!("No currency_code in account details"))?;
        code.as_str().parse::<Currency>()
    }

    /// Builds an effective filter by merging runtime overrides with the base filter.
    fn build_effective_filter(
        &self,
        overrides: Option<&HashMap<String, String>>,
    ) -> NavigationFilter {
        let Some(overrides) = overrides else {
            return self.nav_filter.clone();
        };

        let parse_csv = |key: &str| -> Option<Vec<String>> {
            overrides
                .get(key)
                .map(|v| v.split(',').map(|s| s.trim().to_string()).collect())
        };

        NavigationFilter {
            event_type_ids: parse_csv("event_type_ids")
                .or_else(|| self.nav_filter.event_type_ids.clone()),
            event_type_names: parse_csv("event_type_names")
                .or_else(|| self.nav_filter.event_type_names.clone()),
            event_ids: parse_csv("event_ids").or_else(|| self.nav_filter.event_ids.clone()),
            country_codes: parse_csv("country_codes")
                .or_else(|| self.nav_filter.country_codes.clone()),
            market_types: parse_csv("market_types")
                .or_else(|| self.nav_filter.market_types.clone()),
            market_ids: parse_csv("market_ids").or_else(|| self.nav_filter.market_ids.clone()),
            min_market_start_time: overrides
                .get("min_market_start_time")
                .cloned()
                .or_else(|| self.nav_filter.min_market_start_time.clone()),
            max_market_start_time: overrides
                .get("max_market_start_time")
                .cloned()
                .or_else(|| self.nav_filter.max_market_start_time.clone()),
        }
    }
}

#[async_trait(?Send)]
impl InstrumentProvider for BetfairInstrumentProvider {
    fn store(&self) -> &InstrumentStore {
        &self.store
    }

    fn store_mut(&mut self) -> &mut InstrumentStore {
        &mut self.store
    }

    async fn load_all(&mut self, filters: Option<&HashMap<String, String>>) -> anyhow::Result<()> {
        self.store.clear();
        let effective_filter = self.build_effective_filter(filters);
        let instruments = load_instruments(
            &self.http_client,
            &effective_filter,
            self.currency,
            self.min_notional,
        )
        .await?;
        self.store.add_bulk(instruments);
        self.store.set_initialized();
        Ok(())
    }

    async fn load(
        &mut self,
        instrument_id: &InstrumentId,
        _filters: Option<&HashMap<String, String>>,
    ) -> anyhow::Result<()> {
        let market_id = extract_market_id(instrument_id)?;
        let ts_init = UnixNanos::from(SystemTime::now());

        let params = ListMarketCatalogueParams {
            filter: MarketFilter {
                market_ids: Some(vec![market_id]),
                ..Default::default()
            },
            market_projection: Some(vec![
                MarketProjection::EventType,
                MarketProjection::Event,
                MarketProjection::Competition,
                MarketProjection::MarketDescription,
                MarketProjection::RunnerDescription,
                MarketProjection::RunnerMetadata,
                MarketProjection::MarketStartTime,
            ]),
            max_results: Some(1),
            sort: None,
            locale: None,
        };

        let catalogues: Vec<MarketCatalogue> = self
            .http_client
            .send_betting(METHOD_LIST_MARKET_CATALOGUE, &params)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        for catalogue in &catalogues {
            let instruments =
                parse_market_catalogue(catalogue, self.currency, ts_init, self.min_notional)?;

            for inst in instruments {
                self.store.add(inst);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::testing::load_test_json;

    fn load_navigation_fixture() -> Navigation {
        let data = load_test_json("rest/navigation_list_navigation.json");
        serde_json::from_str(&data).unwrap()
    }

    #[rstest]
    fn test_flatten_navigation() {
        let nav = load_navigation_fixture();
        let markets = flatten_navigation(&nav);

        assert_eq!(markets.len(), 21);

        let first = &markets[0];
        assert!(first.event_type_id.is_some());
        assert!(first.event_type_name.is_some());
        assert!(first.market_id.is_some());
    }

    #[rstest]
    fn test_flatten_navigation_context_propagation() {
        let nav = load_navigation_fixture();
        let markets = flatten_navigation(&nav);

        for market in &markets {
            assert!(
                market.event_type_name.is_some(),
                "market {:?} missing event_type_name",
                market.market_id,
            );
        }
    }

    #[rstest]
    fn test_filter_default_matches_all() {
        let nav = load_navigation_fixture();
        let markets = flatten_navigation(&nav);
        let filter = NavigationFilter::default();

        assert_eq!(
            markets.iter().filter(|m| filter.matches(m)).count(),
            markets.len(),
        );
    }

    #[rstest]
    fn test_filter_by_event_type_name() {
        let nav = load_navigation_fixture();
        let markets = flatten_navigation(&nav);
        let filter = NavigationFilter {
            event_type_names: Some(vec!["Horse Racing".to_string()]),
            ..Default::default()
        };

        let matched: Vec<_> = markets.iter().filter(|m| filter.matches(m)).collect();

        assert_eq!(matched.len(), 18);
        for m in &matched {
            assert_eq!(m.event_type_name.unwrap().as_str(), "Horse Racing");
        }
    }

    #[rstest]
    fn test_filter_by_market_type() {
        let nav = load_navigation_fixture();
        let markets = flatten_navigation(&nav);
        let filter = NavigationFilter {
            market_types: Some(vec!["WIN".to_string()]),
            ..Default::default()
        };

        let matched: Vec<_> = markets.iter().filter(|m| filter.matches(m)).collect();

        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].market_type.unwrap().as_str(), "WIN");
    }

    #[rstest]
    fn test_filter_multiple_criteria() {
        let nav = load_navigation_fixture();
        let markets = flatten_navigation(&nav);
        let filter = NavigationFilter {
            event_type_names: Some(vec!["Horse Racing".to_string()]),
            market_types: Some(vec!["ANTEPOST_WIN".to_string()]),
            ..Default::default()
        };

        let matched: Vec<_> = markets.iter().filter(|m| filter.matches(m)).collect();

        assert_eq!(matched.len(), 16);
        for m in &matched {
            assert_eq!(m.event_type_name.unwrap().as_str(), "Horse Racing");
            assert_eq!(m.market_type.unwrap().as_str(), "ANTEPOST_WIN");
        }
    }

    #[rstest]
    fn test_filter_no_match() {
        let nav = load_navigation_fixture();
        let markets = flatten_navigation(&nav);
        let filter = NavigationFilter {
            event_type_names: Some(vec!["Cricket".to_string()]),
            ..Default::default()
        };

        assert_eq!(markets.iter().filter(|m| filter.matches(m)).count(), 0);
    }
}
