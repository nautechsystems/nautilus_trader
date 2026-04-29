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

//! Instrument filters for the Polymarket adapter.

use std::fmt::Debug;

use nautilus_core::UnixNanos;
use nautilus_model::instruments::{Instrument, InstrumentAny};

use crate::{
    http::query::{GetGammaEventsParams, GetGammaMarketsParams, GetSearchParams},
    websocket::messages::PolymarketNewMarket,
};

/// Declarative filter for controlling which instruments are loaded.
///
/// All three methods default to `None`. At least one must return `Some` for
/// the filter to be useful. Methods are called on each `load_all()` cycle,
/// enabling dynamic re-evaluation (e.g., time-based slug generation).
pub trait InstrumentFilter: Debug + Send + Sync {
    /// Market slugs for concurrent per-slug fetching via `GET /markets?slug=`.
    fn market_slugs(&self) -> Option<Vec<String>> {
        None
    }

    /// Event slugs for fetching event containers via `GET /events?slug=`.
    /// Each event returns multiple markets.
    fn event_slugs(&self) -> Option<Vec<String>> {
        None
    }

    /// Gamma API query params for bulk filtered fetching.
    fn query_params(&self) -> Option<GetGammaMarketsParams> {
        None
    }

    /// Event query: two-phase fetch that resolves an event slug to condition IDs,
    /// then queries `/markets` with those IDs and the given params.
    /// Enables server-side sorting and limiting of markets within an event.
    fn event_queries(&self) -> Option<Vec<(String, GetGammaMarketsParams)>> {
        None
    }

    /// Gamma events API query params for fetching events with full filtering.
    fn event_params(&self) -> Option<GetGammaEventsParams> {
        None
    }

    /// Public search API params for text-based instrument discovery.
    fn search_params(&self) -> Option<GetSearchParams> {
        None
    }

    /// Post-fetch predicate: only instruments where this returns `true` are kept.
    /// Default accepts all instruments. Override to refine results after fetching.
    fn accept(&self, instrument: &InstrumentAny) -> bool {
        let _ = instrument;
        true
    }

    /// Pre-fetch gate for new market WS events: returns `true` if the market
    /// should proceed to Gamma HTTP fetch. Used when this filter is set as
    /// the dedicated `new_market_filter` in the data client config.
    /// Default accepts all new markets.
    fn accept_new_market(&self, new_market: &PolymarketNewMarket) -> bool {
        let _ = new_market;
        true
    }
}

/// Filter that provides market slugs, optionally via a dynamic closure.
pub struct MarketSlugFilter {
    slug_fn: Box<dyn Fn() -> Vec<String> + Send + Sync>,
}

impl MarketSlugFilter {
    /// Creates a new [`MarketSlugFilter`] from a closure that generates slugs.
    ///
    /// The closure is re-evaluated on each `load_all()` call, enabling
    /// time-based or stateful slug generation.
    pub fn new<F: Fn() -> Vec<String> + Send + Sync + 'static>(slug_fn: F) -> Self {
        Self {
            slug_fn: Box::new(slug_fn),
        }
    }

    /// Creates a new [`MarketSlugFilter`] from a static list of market slugs.
    pub fn from_slugs(slugs: Vec<String>) -> Self {
        Self {
            slug_fn: Box::new(move || slugs.clone()),
        }
    }
}

impl Debug for MarketSlugFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(MarketSlugFilter))
            .field("slug_fn", &"<closure>")
            .finish()
    }
}

impl InstrumentFilter for MarketSlugFilter {
    fn market_slugs(&self) -> Option<Vec<String>> {
        Some((self.slug_fn)())
    }
}

/// Filter that provides event slugs, optionally via a dynamic closure.
pub struct EventSlugFilter {
    slug_fn: Box<dyn Fn() -> Vec<String> + Send + Sync>,
}

impl EventSlugFilter {
    /// Creates a new [`EventSlugFilter`] from a closure that generates slugs.
    ///
    /// The closure is re-evaluated on each `load_all()` call, enabling
    /// time-based or stateful slug generation.
    pub fn new<F: Fn() -> Vec<String> + Send + Sync + 'static>(slug_fn: F) -> Self {
        Self {
            slug_fn: Box::new(slug_fn),
        }
    }

    /// Creates a new [`EventSlugFilter`] from a static list of event slugs.
    pub fn from_slugs(slugs: Vec<String>) -> Self {
        Self {
            slug_fn: Box::new(move || slugs.clone()),
        }
    }
}

impl Debug for EventSlugFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(EventSlugFilter))
            .field("slug_fn", &"<closure>")
            .finish()
    }
}

impl InstrumentFilter for EventSlugFilter {
    fn event_slugs(&self) -> Option<Vec<String>> {
        Some((self.slug_fn)())
    }
}

/// Filter that provides Gamma API query parameters for bulk filtered fetching.
#[derive(Debug, Clone)]
pub struct GammaQueryFilter {
    params: GetGammaMarketsParams,
}

impl GammaQueryFilter {
    /// Creates a new [`GammaQueryFilter`] from query parameters.
    pub fn new(params: GetGammaMarketsParams) -> Self {
        Self { params }
    }
}

impl InstrumentFilter for GammaQueryFilter {
    fn query_params(&self) -> Option<GetGammaMarketsParams> {
        Some(self.params.clone())
    }
}

/// Two-phase event filter: resolves event slug → condition IDs, then queries
/// `/markets` with those IDs and the given params (sorting, limiting, etc.).
///
/// This enables server-side sorting and limiting of markets within an event,
/// unlike [`EventSlugFilter`] which returns all markets from the event.
#[derive(Debug, Clone)]
pub struct EventQueryFilter {
    queries: Vec<(String, GetGammaMarketsParams)>,
}

impl EventQueryFilter {
    /// Creates a new [`EventQueryFilter`] for a single event slug with query params.
    pub fn new(event_slug: impl Into<String>, params: GetGammaMarketsParams) -> Self {
        Self {
            queries: vec![(event_slug.into(), params)],
        }
    }

    /// Creates a new [`EventQueryFilter`] from multiple event slug + params pairs.
    pub fn from_queries(queries: Vec<(String, GetGammaMarketsParams)>) -> Self {
        Self { queries }
    }
}

impl InstrumentFilter for EventQueryFilter {
    fn event_queries(&self) -> Option<Vec<(String, GetGammaMarketsParams)>> {
        Some(self.queries.clone())
    }
}

/// Pure post-fetch filter that accepts/rejects instruments via a closure.
///
/// Does not provide any slugs or query params: combine with source filters
/// via the provider's `with_filters()` or `add_filter()` methods.
pub struct PredicateFilter {
    predicate: Box<dyn Fn(&InstrumentAny) -> bool + Send + Sync>,
    label: String,
}

impl PredicateFilter {
    /// Creates a new [`PredicateFilter`] with a label and closure predicate.
    pub fn new<F: Fn(&InstrumentAny) -> bool + Send + Sync + 'static>(
        label: impl Into<String>,
        predicate: F,
    ) -> Self {
        Self {
            predicate: Box::new(predicate),
            label: label.into(),
        }
    }

    /// Convenience: keep only instruments with a matching outcome value.
    ///
    /// Only [`InstrumentAny::BinaryOption`] instruments are checked; all other variants are rejected.
    pub fn outcome(value: impl Into<String>) -> Self {
        let value: String = value.into();
        let label = format!("outcome={value}");
        Self::new(label, move |instrument| {
            if let InstrumentAny::BinaryOption(opt) = instrument {
                opt.outcome.as_deref() == Some(value.as_str())
            } else {
                false
            }
        })
    }

    /// Convenience: reject instruments past expiration.
    ///
    /// The caller provides the current time as [`UnixNanos`] so the filter
    /// works correctly with both real-time and simulated (backtest) clocks.
    /// Only [`InstrumentAny::BinaryOption`] instruments are checked; non-binary variants are accepted.
    pub fn not_expired(now_ns: UnixNanos) -> Self {
        Self::new("not_expired", move |instrument| {
            if let Some(expiration_ns) = Instrument::expiration_ns(instrument) {
                expiration_ns > now_ns
            } else {
                true // no expiration means not expired
            }
        })
    }
}

impl Debug for PredicateFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PredicateFilter))
            .field("label", &self.label)
            .finish()
    }
}

impl InstrumentFilter for PredicateFilter {
    fn accept(&self, instrument: &InstrumentAny) -> bool {
        (self.predicate)(instrument)
    }
}

/// Filter that queries the Gamma events endpoint with full params.
#[derive(Debug, Clone)]
pub struct EventParamsFilter {
    params: GetGammaEventsParams,
}

impl EventParamsFilter {
    /// Creates a new [`EventParamsFilter`] from event query parameters.
    pub fn new(params: GetGammaEventsParams) -> Self {
        Self { params }
    }
}

impl InstrumentFilter for EventParamsFilter {
    fn event_params(&self) -> Option<GetGammaEventsParams> {
        Some(self.params.clone())
    }
}

/// Filter that uses the Gamma public search endpoint.
#[derive(Debug, Clone)]
pub struct SearchFilter {
    params: GetSearchParams,
}

impl SearchFilter {
    /// Creates a new [`SearchFilter`] from search parameters.
    pub fn new(params: GetSearchParams) -> Self {
        Self { params }
    }

    /// Convenience: search by free-text query string.
    pub fn from_query(query: impl Into<String>) -> Self {
        Self {
            params: GetSearchParams {
                q: Some(query.into()),
                ..Default::default()
            },
        }
    }
}

impl InstrumentFilter for SearchFilter {
    fn search_params(&self) -> Option<GetSearchParams> {
        Some(self.params.clone())
    }
}

/// Filter that queries markets by tag ID.
#[derive(Debug, Clone)]
pub struct TagFilter {
    inner: GammaQueryFilter,
}

impl TagFilter {
    /// Creates a new [`TagFilter`] from a known tag ID.
    pub fn from_tag_id(tag_id: impl Into<String>) -> Self {
        Self {
            inner: GammaQueryFilter::new(GetGammaMarketsParams {
                tag_id: Some(tag_id.into()),
                ..Default::default()
            }),
        }
    }
}

impl InstrumentFilter for TagFilter {
    fn query_params(&self) -> Option<GetGammaMarketsParams> {
        self.inner.query_params()
    }
}

/// Pre-fetch gate filter for new market WS events via closure.
///
/// Intended for use as `new_market_filter` in the data client config.
/// Does not affect initial instrument loading.
pub struct NewMarketPredicateFilter {
    predicate: Box<dyn Fn(&PolymarketNewMarket) -> bool + Send + Sync>,
    label: String,
}

impl NewMarketPredicateFilter {
    pub fn new<F: Fn(&PolymarketNewMarket) -> bool + Send + Sync + 'static>(
        label: impl Into<String>,
        predicate: F,
    ) -> Self {
        Self {
            predicate: Box::new(predicate),
            label: label.into(),
        }
    }

    /// Accept new markets where `question` or `description` contains the keyword
    /// (case-insensitive).
    pub fn keyword(keyword: impl Into<String>) -> Self {
        let kw = keyword.into().to_lowercase();
        let label = format!("keyword={kw}");
        Self::new(label, move |nm| {
            nm.question.to_lowercase().contains(&kw) || nm.description.to_lowercase().contains(&kw)
        })
    }
}

impl Debug for NewMarketPredicateFilter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(NewMarketPredicateFilter))
            .field("label", &self.label)
            .finish()
    }
}

impl InstrumentFilter for NewMarketPredicateFilter {
    fn accept_new_market(&self, new_market: &PolymarketNewMarket) -> bool {
        (self.predicate)(new_market)
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        enums::AssetClass,
        identifiers::{InstrumentId, Symbol},
        instruments::{BinaryOption, InstrumentAny},
        types::{Currency, Price, Quantity},
    };
    use rstest::*;
    use ustr::Ustr;

    use super::*;
    use crate::websocket::messages::PolymarketNewMarketEvent;

    fn stub_binary_option_with_expiration(
        outcome: Option<&str>,
        expiration: UnixNanos,
    ) -> InstrumentAny {
        let raw_symbol = Symbol::new("test-token-id");
        InstrumentAny::BinaryOption(BinaryOption::new(
            InstrumentId::from("test-token-id.POLYMARKET"),
            raw_symbol,
            AssetClass::Alternative,
            Currency::pUSD(),
            UnixNanos::default(),
            expiration,
            3,
            2,
            Price::from("0.001"),
            Quantity::from("0.01"),
            outcome.map(Ustr::from),
            None, // description
            None, // max_quantity
            None, // min_quantity
            None, // max_notional
            None, // min_notional
            None, // max_price
            None, // min_price
            None, // margin_init
            None, // margin_maint
            None, // maker_fee
            None, // taker_fee
            None, // info
            UnixNanos::default(),
            UnixNanos::default(),
        ))
    }

    fn stub_binary_option(outcome: Option<&str>) -> InstrumentAny {
        stub_binary_option_with_expiration(outcome, UnixNanos::from(u64::MAX))
    }

    #[rstest]
    fn test_not_expired_accepts_future_expiration() {
        let now = UnixNanos::from(1_000_000u64);
        let instrument =
            stub_binary_option_with_expiration(Some("Yes"), UnixNanos::from(2_000_000u64));
        let filter = PredicateFilter::not_expired(now);
        assert!(filter.accept(&instrument));
    }

    #[rstest]
    fn test_not_expired_rejects_past_expiration() {
        let now = UnixNanos::from(2_000_000u64);
        let instrument =
            stub_binary_option_with_expiration(Some("Yes"), UnixNanos::from(1_000_000u64));
        let filter = PredicateFilter::not_expired(now);
        assert!(!filter.accept(&instrument));
    }

    #[rstest]
    fn test_not_expired_rejects_equal_expiration() {
        let now = UnixNanos::from(1_000_000u64);
        let instrument =
            stub_binary_option_with_expiration(Some("Yes"), UnixNanos::from(1_000_000u64));
        let filter = PredicateFilter::not_expired(now);
        assert!(!filter.accept(&instrument));
    }

    #[rstest]
    fn test_not_expired_works_with_simulated_clock() {
        // Simulates a backtest scenario: clock is set to a historical time,
        // instrument expires in the "future" relative to the simulated clock
        let simulated_now = UnixNanos::from(1_000_000_000_000_000_000u64); // ~2001
        let expiration = UnixNanos::from(1_100_000_000_000_000_000u64); // ~2004
        let instrument = stub_binary_option_with_expiration(Some("Yes"), expiration);
        let filter = PredicateFilter::not_expired(simulated_now);
        assert!(filter.accept(&instrument));
    }

    #[fixture]
    fn yes_instrument() -> InstrumentAny {
        stub_binary_option(Some("Yes"))
    }

    #[fixture]
    fn no_instrument() -> InstrumentAny {
        stub_binary_option(Some("No"))
    }

    #[fixture]
    fn no_outcome_instrument() -> InstrumentAny {
        stub_binary_option(None)
    }

    #[rstest]
    fn test_predicate_filter_accepts_matching(yes_instrument: InstrumentAny) {
        let filter = PredicateFilter::new("test", |_| true);
        assert!(filter.accept(&yes_instrument));
    }

    #[rstest]
    fn test_predicate_filter_rejects_non_matching(yes_instrument: InstrumentAny) {
        let filter = PredicateFilter::new("test", |_| false);
        assert!(!filter.accept(&yes_instrument));
    }

    #[rstest]
    fn test_predicate_filter_outcome_helper(
        yes_instrument: InstrumentAny,
        no_instrument: InstrumentAny,
        no_outcome_instrument: InstrumentAny,
    ) {
        let filter = PredicateFilter::outcome("Yes");
        assert!(filter.accept(&yes_instrument));
        assert!(!filter.accept(&no_instrument));
        assert!(!filter.accept(&no_outcome_instrument));
    }

    #[rstest]
    fn test_default_accept_returns_true(yes_instrument: InstrumentAny) {
        let filter = MarketSlugFilter::from_slugs(vec!["test".to_string()]);
        assert!(filter.accept(&yes_instrument)); // default impl returns true
    }

    fn stub_new_market(
        slug: &str,
        tags: Vec<String>,
        event_slug: Option<&str>,
    ) -> PolymarketNewMarket {
        PolymarketNewMarket {
            id: "1".to_string(),
            question: "Test?".to_string(),
            market: Ustr::from("0xabc"),
            slug: slug.to_string(),
            description: String::new(),
            assets_ids: vec![],
            outcomes: vec!["Yes".to_string(), "No".to_string()],
            timestamp: "0".to_string(),
            tags,
            condition_id: "0xabc".to_string(),
            active: true,
            clob_token_ids: vec![],
            order_price_min_tick_size: None,
            group_item_title: None,
            event_message: event_slug.map(|s| PolymarketNewMarketEvent {
                id: "1".to_string(),
                ticker: s.to_string(),
                slug: s.to_string(),
                title: "Test event".to_string(),
                description: String::new(),
            }),
        }
    }

    #[rstest]
    fn test_default_accept_new_market_returns_true() {
        let filter = GammaQueryFilter::new(GetGammaMarketsParams::default());
        let nm = stub_new_market("any-market", vec![], None);
        assert!(filter.accept_new_market(&nm));
    }

    #[rstest]
    fn test_market_slug_filter_default_accept_new_market() {
        let filter = MarketSlugFilter::from_slugs(vec!["trump-win".to_string()]);
        let nm = stub_new_market("biden-win", vec![], None);
        assert!(filter.accept_new_market(&nm)); // default: accept all
    }

    #[rstest]
    fn test_event_slug_filter_default_accept_new_market() {
        let filter = EventSlugFilter::from_slugs(vec!["us-election-2024".to_string()]);
        let nm = stub_new_market("some-market", vec![], Some("uk-election-2024"));
        assert!(filter.accept_new_market(&nm)); // default: accept all
    }

    #[rstest]
    fn test_tag_filter_default_accept_new_market() {
        let filter = TagFilter::from_tag_id("123");
        let nm = stub_new_market("nvda-market", vec!["stocks".to_string()], None);
        assert!(filter.accept_new_market(&nm)); // default: accept all
    }

    #[rstest]
    fn test_new_market_predicate_keyword_matches_question() {
        let filter = NewMarketPredicateFilter::keyword("BTC");
        let mut nm = stub_new_market("btc-market", vec![], None);
        nm.question = "Will BTC reach 100k?".to_string();
        assert!(filter.accept_new_market(&nm));
    }

    #[rstest]
    fn test_new_market_predicate_keyword_matches_description() {
        let filter = NewMarketPredicateFilter::keyword("bitcoin");
        let mut nm = stub_new_market("some-market", vec![], None);
        nm.question = "Some question".to_string();
        nm.description = "This market is about Bitcoin price".to_string();
        assert!(filter.accept_new_market(&nm));
    }

    #[rstest]
    fn test_new_market_predicate_keyword_rejects_no_match() {
        let filter = NewMarketPredicateFilter::keyword("BTC");
        let mut nm = stub_new_market("eth-market", vec![], None);
        nm.question = "Will ETH reach 10k?".to_string();
        nm.description = "Ethereum price market".to_string();
        assert!(!filter.accept_new_market(&nm));
    }
}
