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

use std::{sync::Arc, time::Duration};

use ahash::{AHashMap, AHashSet};
use dashmap::DashMap;
use nautilus_common::{live::get_runtime, messages::DataEvent, providers::InstrumentProvider};
use nautilus_core::{AtomicMap, UnixNanos, time::AtomicTime};
use nautilus_model::{
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
};
use ustr::Ustr;

use super::{PolymarketDataClient, runtime::is_instrument_retirable};
use crate::{
    common::consts::GAMMA_CONDITION_IDS_BATCH_SIZE,
    filters::{InstrumentFilter, set_venue_closed},
    http::{gamma::PolymarketGammaHttpClient, query::GetGammaMarketsParams},
    providers::extract_condition_id,
};

/// Consecutive liveness lookups a condition must be absent from before it counts as delisted.
const LIVENESS_MISSES_BEFORE_DELISTED: u32 = 3;

#[derive(Clone, Copy, Debug)]
pub(crate) struct TokenMeta {
    pub(crate) instrument_id: InstrumentId,
    pub(crate) price_precision: u8,
    pub(crate) size_precision: u8,
}

// Inserts `instrument` into the live instrument cache and updates the
// `token_meta` routing index in one step. Every path that populates the live
// cache must go through here so WS messages can always resolve token_id back
// to an InstrumentId.
pub(crate) fn cache_instrument(
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
    instrument: &InstrumentAny,
) {
    let instrument_id = instrument.id();
    token_meta.insert(
        Ustr::from(instrument.raw_symbol().as_str()),
        TokenMeta {
            instrument_id,
            price_precision: instrument.price_precision(),
            size_precision: instrument.size_precision(),
        },
    );
    instruments.insert(instrument_id, instrument.clone());
}

pub(super) fn cache_instrument_if_active(
    now_ns: UnixNanos,
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
    instrument: &InstrumentAny,
) -> bool {
    if is_instrument_retirable(instrument, now_ns) {
        return false;
    }

    cache_instrument(instruments, token_meta, instrument);
    true
}

pub(super) fn cache_and_publish_instruments(
    instruments_cache: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
    data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    now_ns: UnixNanos,
    instruments: Vec<InstrumentAny>,
) -> usize {
    let mut total = 0;

    for instrument in instruments {
        if !cache_instrument_if_active(now_ns, instruments_cache, token_meta, &instrument) {
            // A refetch that reports the market closed supersedes the cached copy, which still
            // carries the older open observation. Drop it so the expiry sweep can reclaim it
            // rather than waiting for another source to correct the cache.
            let instrument_id = instrument.id();
            if instruments_cache.load().contains_key(&instrument_id) {
                // Only the definition is corrected. Writing `token_meta` here would restore message
                // routing for a market the venue has closed, and a watchlisted instrument keeps its
                // cache entry through retirement, so every refresh would re-arm the sweep.
                instruments_cache.insert(instrument_id, instrument.clone());

                // Publish so the execution client reconciles its lookup state against the
                // venue-closed definition rather than keeping the older open one.
                if let Err(e) = data_sender.send(DataEvent::Instrument(instrument)) {
                    log::warn!("Failed to publish superseded instrument {instrument_id}: {e}");
                }
            }

            log::debug!("Skipping expired instrument {instrument_id} during live cache publish");
            continue;
        }

        let instrument_id = instrument.id();
        total += 1;

        if let Err(e) = data_sender.send(DataEvent::Instrument(instrument)) {
            log::warn!("Failed to publish instrument {instrument_id}: {e}");
        }
    }

    total
}

pub(super) async fn refresh_scoped_instruments(
    http_client: PolymarketGammaHttpClient,
    instrument_config: Option<crate::config::PolymarketInstrumentProviderConfig>,
    filters: Vec<Arc<dyn InstrumentFilter>>,
    instruments_cache: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    token_meta: &Arc<DashMap<Ustr, TokenMeta>>,
    data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    clock: &'static AtomicTime,
) -> anyhow::Result<usize> {
    // Defaulted rather than returning early: a client can carry registered filters with
    // no instrument_config, and those filters bound the refresh universe on their own.
    let instrument_config = instrument_config.unwrap_or_default();
    let refreshed =
        crate::providers::fetch_configured_instruments(&http_client, &instrument_config, &filters)
            .await?;

    Ok(cache_and_publish_instruments(
        instruments_cache,
        token_meta,
        data_sender,
        clock.get_time_ns(),
        refreshed,
    ))
}

/// Outcome of one venue liveness pass.
pub(super) struct LivenessRefresh {
    /// Carried instruments whose cached venue state was updated.
    pub updated: usize,
    /// Whether at least one Gamma lookup succeeded, so the caller can defer the next probe.
    pub probed: bool,
}

/// Re-observes venue liveness for instruments carried past their expiration.
///
/// The scope refresh only covers the configured bootstrap universe, so instruments brought in by
/// auto-load or new-market discovery would otherwise never have their liveness re-checked. Only
/// markets already past `endDate` and last reported open are refetched, so the request volume is
/// bounded by the set actually being carried past their scheduled end.
///
/// Only the venue state is taken from the refetch, so a cached definition rebuilt by a
/// `tick_size_change` keeps its precision. Updated definitions are published as well as cached, so
/// the global cache the execution client reads sees the same venue state as the data client.
pub(super) async fn refresh_expired_instrument_liveness(
    http_client: &PolymarketGammaHttpClient,
    instruments_cache: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    misses: &mut AHashMap<String, u32>,
    now_ns: UnixNanos,
) -> LivenessRefresh {
    // One pass builds the condition index, so marking a delisted condition later does not rescan.
    let mut carried: AHashMap<String, Vec<InstrumentId>> = AHashMap::new();
    {
        let loaded = instruments_cache.load();
        for (instrument_id, instrument) in loaded.iter() {
            if !crate::filters::is_expired(instrument, now_ns)
                || crate::filters::is_retirable(instrument, now_ns)
            {
                continue;
            }

            if let Ok(condition_id) = extract_condition_id(instrument_id) {
                carried
                    .entry(condition_id)
                    .or_default()
                    .push(*instrument_id);
            }
        }
    }

    if carried.is_empty() {
        // Nothing to probe is a complete pass, so the caller still defers the next one.
        return LivenessRefresh {
            updated: 0,
            probed: true,
        };
    }

    let condition_ids: Vec<String> = carried.keys().cloned().collect();
    let mut updates: Vec<InstrumentAny> = Vec::new();
    let mut probed = false;

    for chunk in condition_ids.chunks(GAMMA_CONDITION_IDS_BATCH_SIZE) {
        let params = GetGammaMarketsParams {
            condition_ids: Some(chunk.to_vec()),
            ..Default::default()
        };

        let (instruments, transient) = match http_client
            .request_instruments_by_params_with_transient(params)
            .await
        {
            Ok(result) => result,
            Err(e) => {
                log::warn!(
                    "Failed to refresh venue liveness for {} condition_id(s): {e:?}",
                    chunk.len(),
                );
                continue;
            }
        };

        probed = true;
        let mut returned = AHashSet::new();

        for instrument in instruments {
            let Ok(condition_id) = extract_condition_id(&instrument.id()) else {
                continue;
            };

            // Only refresh what is already carried, so this never widens the universe.
            if carried.contains_key(&condition_id) {
                // Take only the venue state from the refetch. A `tick_size_change` rebuilds the
                // cached definition, and swapping the Gamma copy in wholesale would revert that
                // rebuild for a market the venue is still trading.
                let closed = crate::filters::venue_reports_closed(&instrument).unwrap_or(false);
                let loaded = instruments_cache.load();

                if let Some(InstrumentAny::BinaryOption(binary)) = loaded.get(&instrument.id()) {
                    let mut binary = binary.clone();
                    set_venue_closed(&mut binary, closed);
                    updates.push(InstrumentAny::BinaryOption(binary));
                }
            }

            returned.insert(condition_id);
        }

        // Gamma omits closed markets unless asked for them, so a condition missing from the
        // open lookup is either closed or gone. Ask explicitly before deciding: a closed market
        // is a positive terminal observation and retires now, while a condition absent from both
        // lookups is only a weak signal and has to repeat before it counts.
        let missing: Vec<String> = chunk
            .iter()
            .filter(|cid| !returned.contains(*cid) && !transient.contains(cid))
            .cloned()
            .collect();

        let mut confirmed_closed = AHashSet::new();

        if !missing.is_empty() {
            let closed_params = GetGammaMarketsParams {
                condition_ids: Some(missing.clone()),
                closed: Some(true),
                ..Default::default()
            };

            match http_client
                .request_instruments_by_params(closed_params)
                .await
            {
                Ok(instruments) => {
                    for instrument in instruments {
                        if let Ok(condition_id) = extract_condition_id(&instrument.id()) {
                            confirmed_closed.insert(condition_id);
                        }
                    }
                }
                Err(e) => log::warn!(
                    "Failed to confirm closed state for {} condition_id(s): {e:?}",
                    missing.len(),
                ),
            }
        }

        for condition_id in chunk {
            if returned.contains(condition_id) || transient.contains(condition_id) {
                misses.remove(condition_id);
                continue;
            }

            if confirmed_closed.contains(condition_id) {
                misses.remove(condition_id);
            } else {
                // Not in either lookup: could be a parse failure or a transient omission, so
                // require the miss to repeat before treating it as gone.
                let seen = misses.entry(condition_id.clone()).or_insert(0);
                *seen += 1;

                if *seen < LIVENESS_MISSES_BEFORE_DELISTED {
                    log::debug!(
                        "condition_id={condition_id} absent from both liveness lookups ({seen}/{LIVENESS_MISSES_BEFORE_DELISTED})"
                    );
                    continue;
                }
            }

            // The streak has served its purpose. Leaving it behind would grow the map for the life
            // of the process, and would start a re-loaded condition above the threshold so its
            // first absence retired it with no confirmation at all.
            misses.remove(condition_id);

            let Some(instrument_ids) = carried.get(condition_id) else {
                continue;
            };

            let loaded = instruments_cache.load();
            for instrument_id in instrument_ids {
                if let Some(InstrumentAny::BinaryOption(binary)) = loaded.get(instrument_id) {
                    let mut binary = binary.clone();
                    set_venue_closed(&mut binary, true);
                    updates.push(InstrumentAny::BinaryOption(binary));
                }
            }

            log::info!(
                "Venue reports condition_id={condition_id} no longer open: marking {} carried instrument(s) closed",
                instrument_ids.len(),
            );
        }
    }

    if updates.is_empty() {
        return LivenessRefresh { updated: 0, probed };
    }

    // A single clone-and-swap for the whole batch: `AtomicMap::insert` rebuilds the map each call.
    // Only keys still present are updated, so a result captured before the request completed
    // cannot resurrect an instrument that the sweep retired in the meantime.
    instruments_cache.rcu(|map| {
        for instrument in &updates {
            let instrument_id = instrument.id();
            if map.contains_key(&instrument_id) {
                map.insert(instrument_id, instrument.clone());
            }
        }
    });

    for instrument in &updates {
        if !instruments_cache.load().contains_key(&instrument.id()) {
            continue;
        }

        if let Err(e) = data_sender.send(DataEvent::Instrument(instrument.clone())) {
            log::warn!(
                "Failed to publish refreshed instrument {}: {e}",
                instrument.id()
            );
        }
    }

    LivenessRefresh {
        updated: updates.len(),
        probed,
    }
}

impl PolymarketDataClient {
    pub(super) async fn bootstrap_instruments(&mut self) -> anyhow::Result<()> {
        self.provider.initialize(false).await?;

        let total = cache_and_publish_instruments(
            &self.instruments,
            &self.token_meta,
            &self.data_sender,
            self.clock.get_time_ns(),
            self.provider
                .store()
                .list_all()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>(),
        );

        log::debug!("Published {total} Polymarket instruments to data engine");
        Ok(())
    }

    pub(super) fn spawn_instrument_refresh_task(&mut self) {
        let Some(interval_mins) = self.config.update_instruments_interval_mins else {
            return;
        };

        let filters = self.provider.filters();

        // A registered filter is a bootstrap scope in its own right, so a client carrying only
        // filters keeps refreshing instead of freezing at the universe it loaded on connect.
        // Filter source methods are deliberately not evaluated here: they are documented as
        // re-evaluated each load cycle, so probing them would consume a batch the refresh misses.
        if interval_mins == 0 || (self.config.instrument_config.is_none() && filters.is_empty()) {
            return;
        }

        let interval = Duration::from_secs(interval_mins.saturating_mul(60));
        let cancellation = self.cancellation_token.clone();
        let http_client = self.provider.http_client().clone();
        let instrument_config = self.config.instrument_config.clone();
        let instruments_cache = self.instruments.clone();
        let token_meta = self.token_meta.clone();
        let data_sender = self.data_sender.clone();
        let clock = self.clock;

        let handle = get_runtime().spawn(async move {
            log::debug!("Polymarket instrument refresh task started");

            loop {
                tokio::select! {
                    () = tokio::time::sleep(interval) => {}
                    () = cancellation.cancelled() => {
                        log::debug!("Polymarket instrument refresh task cancelled");
                        break;
                    }
                }

                match refresh_scoped_instruments(
                    http_client.clone(),
                    instrument_config.clone(),
                    filters.clone(),
                    &instruments_cache,
                    &token_meta,
                    &data_sender,
                    clock,
                )
                .await
                {
                    Ok(total) => {
                        if total > 0 {
                            log::debug!(
                                "Refreshed {total} Polymarket instruments into the live cache"
                            );
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to refresh Polymarket instruments: {e}");
                    }
                }
            }

            log::debug!("Polymarket instrument refresh task ended");
        });

        self.tasks.push(handle);
    }
}

#[cfg(test)]
mod tests {
    use nautilus_common::live::runner::replace_data_event_sender;
    use nautilus_core::{UnixNanos, time::get_atomic_clock_realtime};
    use nautilus_model::{
        enums::AssetClass,
        identifiers::{ClientId, Symbol},
        instruments::{BinaryOption, stubs::binary_option},
        types::{Currency, Price, Quantity},
    };
    use nautilus_network::{retry::RetryConfig, websocket::config::TransportBackend};
    use rstest::rstest;

    use super::*;
    use crate::{
        common::consts::WS_DEFAULT_SUBSCRIPTIONS,
        config::PolymarketDataClientConfig,
        filters::{PredicateFilter, TagFilter},
        http::{
            clob::PolymarketClobPublicClient, data_api::PolymarketDataApiHttpClient,
            gamma::PolymarketGammaHttpClient,
        },
        websocket::pool::PolymarketMarketConnectionPool,
    };

    fn make_client(config: PolymarketDataClientConfig) -> PolymarketDataClient {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        replace_data_event_sender(tx);

        let gamma = PolymarketGammaHttpClient::new(
            Some("http://localhost".to_string()),
            1,
            RetryConfig::default(),
        )
        .expect("gamma client");
        let clob = PolymarketClobPublicClient::new(Some("http://localhost".to_string()), 1)
            .expect("clob client");
        let data_api = PolymarketDataApiHttpClient::new(Some("http://localhost".to_string()), 1)
            .expect("data api client");
        let ws = PolymarketMarketConnectionPool::new(
            Some("ws://localhost/ws/market".to_string()),
            false,
            TransportBackend::default(),
            WS_DEFAULT_SUBSCRIPTIONS,
        );

        PolymarketDataClient::new(
            ClientId::from("POLY-TEST"),
            config,
            gamma,
            clob,
            data_api,
            ws,
        )
    }

    #[rstest]
    fn refresh_task_spawns_for_registered_filter_without_instrument_config() {
        let mut client = make_client(PolymarketDataClientConfig {
            update_instruments_interval_mins: Some(1),
            instrument_config: None,
            ..PolymarketDataClientConfig::default()
        });
        client.add_instrument_filter(Arc::new(TagFilter::from_tag_id(84)));

        client.spawn_instrument_refresh_task();

        // Without this the client would bootstrap from the filter and then freeze at its
        // connect-time universe, because the guard used to require an instrument_config.
        assert_eq!(client.tasks.len(), 1);
    }

    #[rstest]
    fn refresh_task_does_not_spawn_without_config_or_filters() {
        let mut client = make_client(PolymarketDataClientConfig {
            update_instruments_interval_mins: Some(1),
            instrument_config: None,
            ..PolymarketDataClientConfig::default()
        });

        client.spawn_instrument_refresh_task();

        assert_eq!(client.tasks.len(), 0);
    }

    #[rstest]
    fn refresh_task_spawns_for_accept_only_filter() {
        let mut client = make_client(PolymarketDataClientConfig {
            update_instruments_interval_mins: Some(1),
            instrument_config: None,
            ..PolymarketDataClientConfig::default()
        });
        client.add_instrument_filter(Arc::new(PredicateFilter::new("accept-all", |_| true)));

        client.spawn_instrument_refresh_task();

        // Accepted cost of not probing filter sources here: an accept-only filter spawns a timer
        // whose refresh issues no HTTP. A wakeup every N minutes is cheaper than consuming a
        // dynamic filter's batch at connect.
        assert_eq!(client.tasks.len(), 1);
    }

    fn stub_instrument(
        raw_symbol: &str,
        price_increment: Price,
        size_increment: Quantity,
    ) -> InstrumentAny {
        let price_precision = price_increment.precision;
        let size_precision = size_increment.precision;
        InstrumentAny::BinaryOption(BinaryOption::new(
            InstrumentId::from(format!("{raw_symbol}.POLYMARKET").as_str()),
            Symbol::new(raw_symbol),
            AssetClass::Alternative,
            Currency::pUSD(),
            UnixNanos::default(),
            UnixNanos::from(u64::MAX),
            price_precision,
            size_precision,
            price_increment,
            size_increment,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        ))
    }

    #[rstest]
    #[case::p3_s2("token-a", Price::from("0.001"), Quantity::from("0.01"))]
    #[case::p5_s4("token-b", Price::from("0.00001"), Quantity::from("0.0001"))]
    fn cache_instrument_writes_both_maps(
        #[case] raw_symbol: &str,
        #[case] price_increment: Price,
        #[case] size_increment: Quantity,
    ) {
        let instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>> = Arc::new(AtomicMap::new());
        let token_meta: Arc<DashMap<Ustr, TokenMeta>> = Arc::new(DashMap::new());
        let inst = stub_instrument(raw_symbol, price_increment, size_increment);
        let expected_id = inst.id();
        let expected_token = Ustr::from(raw_symbol);
        let expected_price_precision = price_increment.precision;
        let expected_size_precision = size_increment.precision;

        cache_instrument(&instruments, &token_meta, &inst);

        let loaded = instruments.load();
        let cached = loaded
            .get(&expected_id)
            .expect("instrument inserted into live cache");
        assert_eq!(cached.id(), expected_id);
        assert_eq!(cached.raw_symbol().as_str(), raw_symbol);

        let meta = token_meta
            .get(&expected_token)
            .expect("token_meta inserted for raw_symbol");
        assert_eq!(meta.instrument_id, expected_id);
        assert_eq!(meta.price_precision, expected_price_precision);
        assert_eq!(meta.size_precision, expected_size_precision);
    }

    #[rstest]
    fn cache_instrument_overwrites_precisions_on_second_call() {
        let instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>> = Arc::new(AtomicMap::new());
        let token_meta: Arc<DashMap<Ustr, TokenMeta>> = Arc::new(DashMap::new());
        let raw_symbol = "token-overwrite";

        let first = stub_instrument(raw_symbol, Price::from("0.01"), Quantity::from("0.1"));
        cache_instrument(&instruments, &token_meta, &first);

        let second = stub_instrument(raw_symbol, Price::from("0.0001"), Quantity::from("0.001"));
        cache_instrument(&instruments, &token_meta, &second);

        let meta = token_meta
            .get(&Ustr::from(raw_symbol))
            .expect("token_meta present after overwrite");
        assert_eq!(meta.price_precision, 4);
        assert_eq!(meta.size_precision, 3);
        assert_eq!(token_meta.len(), 1);
        assert_eq!(instruments.load().len(), 1);
    }

    #[rstest]
    fn cache_instrument_maintains_dual_cache_invariant() {
        let instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>> = Arc::new(AtomicMap::new());
        let token_meta: Arc<DashMap<Ustr, TokenMeta>> = Arc::new(DashMap::new());

        let samples = [
            stub_instrument("token-1", Price::from("0.001"), Quantity::from("0.01")),
            stub_instrument("token-2", Price::from("0.0001"), Quantity::from("0.01")),
            stub_instrument("token-3", Price::from("0.00001"), Quantity::from("0.001")),
        ];

        for inst in &samples {
            cache_instrument(&instruments, &token_meta, inst);
        }

        let loaded = instruments.load();
        assert_eq!(loaded.len(), samples.len());
        for inst in loaded.values() {
            let token_id = Ustr::from(inst.raw_symbol().as_str());
            let meta = token_meta
                .get(&token_id)
                .unwrap_or_else(|| panic!("missing token_meta for {token_id}"));
            assert_eq!(meta.instrument_id, inst.id());
        }
    }

    fn past_end_date_instrument(raw_symbol: &str, venue_closed: Option<bool>) -> InstrumentAny {
        let clock = get_atomic_clock_realtime();
        let mut binary = binary_option();
        binary.id = InstrumentId::from(format!("{raw_symbol}.POLYMARKET").as_str());
        binary.raw_symbol = Symbol::new(raw_symbol);
        binary.currency = Currency::pUSD();
        binary.activation_ns = UnixNanos::default();
        binary.expiration_ns =
            UnixNanos::from(clock.get_time_ns().as_u64().saturating_sub(1_000_000_000));

        let mut info = nautilus_core::Params::new();
        if let Some(closed) = venue_closed {
            info.insert("venue_closed".to_string(), serde_json::Value::Bool(closed));
        }
        binary.info = Some(info);

        InstrumentAny::BinaryOption(binary)
    }

    /// Every bootstrap and refresh scope converges on `cache_and_publish_instruments`, so this
    /// covers `load_all`, `load_ids`, `market_slugs`, `event_slugs`, `event_slug_builder`,
    /// `series_ids`, the `filters` map, and registered filters in one place.
    #[rstest]
    #[case::open_at_venue(Some(false), 1)]
    #[case::closed_at_venue(Some(true), 0)]
    #[case::no_venue_state(None, 0)]
    fn publish_sink_carries_past_end_date_markets_open_at_the_venue(
        #[case] venue_closed: Option<bool>,
        #[case] expected_published: usize,
    ) {
        let instruments_cache = Arc::new(AtomicMap::new());
        let token_meta = Arc::new(DashMap::new());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let instrument = past_end_date_instrument("0xTOKEN_SINK", venue_closed);
        let instrument_id = instrument.id();

        let published = cache_and_publish_instruments(
            &instruments_cache,
            &token_meta,
            &tx,
            get_atomic_clock_realtime().get_time_ns(),
            vec![instrument],
        );

        assert_eq!(published, expected_published);
        assert_eq!(
            instruments_cache.load().contains_key(&instrument_id),
            expected_published == 1,
        );
        assert_eq!(rx.try_recv().is_ok(), expected_published == 1);
    }

    #[rstest]
    fn delisted_condition_is_marked_closed_so_it_can_be_reclaimed() {
        // Backstop: `endDate` used to retire unconditionally. Once liveness drives retirement, a
        // market Gamma stops listing would otherwise keep its last "open" observation forever and
        // leak its subscription slot and runtime state.
        let instruments_cache = Arc::new(AtomicMap::new());
        let token_meta = Arc::new(DashMap::new());

        let condition_id = "0x00000000000000000000000000000000000000000000000000000000000000ab";
        let raw_symbol = "0xTOKEN_DELISTED";
        let mut instrument = past_end_date_instrument(raw_symbol, Some(false));
        if let InstrumentAny::BinaryOption(binary) = &mut instrument {
            binary.id =
                InstrumentId::from(format!("{condition_id}-{raw_symbol}.POLYMARKET").as_str());
        }
        cache_instrument(&instruments_cache, &token_meta, &instrument);
        let instrument_id = instrument.id();

        let now_ns = get_atomic_clock_realtime().get_time_ns();
        assert!(
            !crate::filters::is_retirable(
                instruments_cache
                    .load()
                    .get(&instrument_id)
                    .expect("cached"),
                now_ns,
            ),
            "precondition: the carried instrument is not retirable while last seen open",
        );

        let InstrumentAny::BinaryOption(mut binary) = instruments_cache
            .load()
            .get(&instrument_id)
            .expect("cached")
            .clone()
        else {
            panic!("expected BinaryOption");
        };
        crate::filters::set_venue_closed(&mut binary, true);
        cache_instrument(
            &instruments_cache,
            &token_meta,
            &InstrumentAny::BinaryOption(binary),
        );

        assert!(
            crate::filters::is_retirable(
                instruments_cache
                    .load()
                    .get(&instrument_id)
                    .expect("cached"),
                now_ns,
            ),
            "a delisted condition must become reclaimable by the expiry sweep",
        );
    }

    /// `request_instruments`, `request_instrument` and new-market WS discovery call
    /// `cache_instrument_if_active` directly rather than going through the publish helper, so the
    /// shared gate is asserted here on its own.
    #[rstest]
    #[case::open_at_venue(Some(false), true)]
    #[case::closed_at_venue(Some(true), false)]
    #[case::no_venue_state(None, false)]
    fn direct_cache_gate_carries_past_end_date_markets_open_at_the_venue(
        #[case] venue_closed: Option<bool>,
        #[case] expected_cached: bool,
    ) {
        let instruments_cache = Arc::new(AtomicMap::new());
        let token_meta = Arc::new(DashMap::new());

        let instrument = past_end_date_instrument("0xTOKEN_DIRECT", venue_closed);
        let instrument_id = instrument.id();

        let cached = cache_instrument_if_active(
            get_atomic_clock_realtime().get_time_ns(),
            &instruments_cache,
            &token_meta,
            &instrument,
        );

        assert_eq!(cached, expected_cached);
        assert_eq!(
            instruments_cache.load().contains_key(&instrument_id),
            expected_cached,
        );
        assert_eq!(
            token_meta.contains_key(&Ustr::from("0xTOKEN_DIRECT")),
            expected_cached,
            "routing metadata must follow the cache decision",
        );
    }

    #[rstest]
    fn a_wrongly_retired_market_is_readmitted_once_the_venue_reports_it_open() {
        // Retirement is recoverable, not terminal. If the venue ever reported a market closed in
        // error, the next observation that reports it open re-admits it. This is what bounds the
        // cost of trusting `closed`, and it is the behaviour the previous `endDate`-only gate
        // lacked: there, a refetched market stayed rejected forever.
        let instruments_cache = Arc::new(AtomicMap::new());
        let token_meta = Arc::new(DashMap::new());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let now_ns = get_atomic_clock_realtime().get_time_ns();

        // Venue reports closed, so the sweep retires it and it leaves the cache.
        let closed = past_end_date_instrument("0xTOKEN_RECOVER", Some(true));
        let instrument_id = closed.id();
        assert!(crate::filters::is_retirable(&closed, now_ns));
        assert_eq!(
            cache_and_publish_instruments(
                &instruments_cache,
                &token_meta,
                &tx,
                now_ns,
                vec![closed],
            ),
            0,
        );
        assert!(!instruments_cache.load().contains_key(&instrument_id));

        // A later fetch reports the same market open again.
        let reopened = past_end_date_instrument("0xTOKEN_RECOVER", Some(false));
        assert_eq!(
            cache_and_publish_instruments(
                &instruments_cache,
                &token_meta,
                &tx,
                now_ns,
                vec![reopened],
            ),
            1,
        );

        assert!(instruments_cache.load().contains_key(&instrument_id));
        assert!(token_meta.contains_key(&Ustr::from("0xTOKEN_RECOVER")));
        assert!(
            rx.try_recv().is_ok(),
            "re-admission must publish downstream"
        );
    }

    const LIVENESS_CONDITION_ID: &str = "0xCONDITION";
    const LIVENESS_TOKEN_IDS: [&str; 2] = ["111", "222"];

    /// Gamma market payload for the liveness tests, with `endDate` far in the past so every
    /// instrument parsed from it is carried rather than live.
    fn liveness_market_json(closed: bool) -> serde_json::Value {
        serde_json::json!({
            "id": "1",
            "conditionId": LIVENESS_CONDITION_ID,
            "question": "Liveness probe market",
            "slug": "liveness-probe-market",
            "endDate": "2020-01-01T00:00:00Z",
            "active": true,
            "closed": closed,
            "acceptingOrders": !closed,
            "enableOrderBook": true,
            "clobTokenIds": format!(
                "[\"{}\", \"{}\"]",
                LIVENESS_TOKEN_IDS[0], LIVENESS_TOKEN_IDS[1],
            ),
            "outcomes": "[\"Yes\", \"No\"]",
            "orderPriceMinTickSize": 0.01,
            "orderMinSize": 5,
        })
    }

    fn liveness_instruments(closed: bool) -> Vec<InstrumentAny> {
        let market: crate::http::models::GammaMarket =
            serde_json::from_value(liveness_market_json(closed)).expect("gamma market");
        let clock = get_atomic_clock_realtime();

        crate::http::parse::parse_gamma_market(&market)
            .expect("parse gamma market")
            .iter()
            .map(|def| {
                crate::http::parse::create_instrument_from_def(def, clock.get_time_ns())
                    .expect("create instrument")
            })
            .collect()
    }

    #[derive(Clone)]
    struct LivenessServerState {
        /// Served when the request carries no `closed` filter.
        open: serde_json::Value,
        /// Served when the request carries `closed=true`.
        closed: serde_json::Value,
        /// Every request fails while set, so a probe that never reaches Gamma can be exercised.
        fail: Arc<std::sync::atomic::AtomicBool>,
    }

    async fn handle_liveness_markets(
        axum::extract::State(state): axum::extract::State<LivenessServerState>,
        axum::extract::RawQuery(query): axum::extract::RawQuery,
    ) -> axum::response::Response {
        use axum::response::IntoResponse;

        if state.fail.load(std::sync::atomic::Ordering::SeqCst) {
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }

        let body = if query.unwrap_or_default().contains("closed=true") {
            state.closed
        } else {
            state.open
        };

        axum::Json(body).into_response()
    }

    async fn handle_liveness_markets_keyset(
        axum::extract::State(state): axum::extract::State<LivenessServerState>,
        axum::extract::RawQuery(query): axum::extract::RawQuery,
    ) -> axum::response::Response {
        use axum::response::IntoResponse;

        if state.fail.load(std::sync::atomic::Ordering::SeqCst) {
            return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }

        let markets = if query.unwrap_or_default().contains("closed=true") {
            state.closed
        } else {
            state.open
        };

        axum::Json(serde_json::json!({"markets": markets})).into_response()
    }

    async fn start_liveness_test_server(state: LivenessServerState) -> std::net::SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind failed");
        let addr = listener.local_addr().expect("local_addr");
        let router = axum::Router::new()
            .route("/markets", axum::routing::get(handle_liveness_markets))
            .route(
                "/markets/keyset",
                axum::routing::get(handle_liveness_markets_keyset),
            )
            .with_state(state);

        tokio::spawn(async move { axum::serve(listener, router).await.expect("serve failed") });
        addr
    }

    /// Seeds the cache with the carried instruments and returns everything the refresh needs.
    #[allow(clippy::type_complexity)]
    fn liveness_fixture() -> (
        Arc<AtomicMap<InstrumentId, InstrumentAny>>,
        tokio::sync::mpsc::UnboundedSender<DataEvent>,
        tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
        Vec<InstrumentId>,
    ) {
        let instruments_cache: Arc<AtomicMap<InstrumentId, InstrumentAny>> =
            Arc::new(AtomicMap::new());
        let token_meta: Arc<DashMap<Ustr, TokenMeta>> = Arc::new(DashMap::new());
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        let mut ids = Vec::new();
        for instrument in liveness_instruments(false) {
            ids.push(instrument.id());
            cache_instrument(&instruments_cache, &token_meta, &instrument);
        }

        (instruments_cache, tx, rx, ids)
    }

    fn liveness_client(addr: std::net::SocketAddr) -> PolymarketGammaHttpClient {
        PolymarketGammaHttpClient::new(Some(format!("http://{addr}")), 5, RetryConfig::default())
            .expect("gamma client")
    }

    fn venue_closed_in_cache(
        instruments_cache: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
        instrument_id: &InstrumentId,
    ) -> Option<bool> {
        instruments_cache
            .load()
            .get(instrument_id)
            .and_then(crate::filters::venue_reports_closed)
    }

    /// A close shows up only as absence from the plain `condition_ids` lookup, because Gamma omits
    /// closed markets unless asked for them. The follow-up `closed=true` lookup turns that into a
    /// positive observation, which retires on the first pass with no miss streak needed.
    #[rstest]
    #[tokio::test]
    async fn confirmed_closed_condition_retires_on_the_first_pass() {
        let addr = start_liveness_test_server(LivenessServerState {
            open: serde_json::json!([]),
            closed: serde_json::json!([liveness_market_json(true)]),
            fail: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        })
        .await;

        let (instruments_cache, tx, _rx, ids) = liveness_fixture();
        let mut misses: AHashMap<String, u32> = AHashMap::new();
        let now_ns = get_atomic_clock_realtime().get_time_ns();

        let refreshed = refresh_expired_instrument_liveness(
            &liveness_client(addr),
            &instruments_cache,
            &tx,
            &mut misses,
            now_ns,
        )
        .await;

        assert!(refreshed.probed);
        assert_eq!(refreshed.updated, ids.len());

        for instrument_id in &ids {
            assert_eq!(
                venue_closed_in_cache(&instruments_cache, instrument_id),
                Some(true),
            );
        }

        assert!(
            misses.is_empty(),
            "a positive observation must not leave a streak behind",
        );
    }

    /// Absence from BOTH lookups is a weak signal (a parse failure, a transient omission), so it
    /// has to repeat before it counts.
    #[rstest]
    #[tokio::test]
    async fn condition_absent_from_both_lookups_retires_only_after_the_threshold() {
        let addr = start_liveness_test_server(LivenessServerState {
            open: serde_json::json!([]),
            closed: serde_json::json!([]),
            fail: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        })
        .await;

        let client = liveness_client(addr);
        let (instruments_cache, tx, _rx, ids) = liveness_fixture();
        let mut misses: AHashMap<String, u32> = AHashMap::new();
        let now_ns = get_atomic_clock_realtime().get_time_ns();

        for pass in 1..LIVENESS_MISSES_BEFORE_DELISTED {
            let refreshed = refresh_expired_instrument_liveness(
                &client,
                &instruments_cache,
                &tx,
                &mut misses,
                now_ns,
            )
            .await;

            assert_eq!(refreshed.updated, 0, "must not retire on pass {pass}");
            assert_eq!(misses.get(LIVENESS_CONDITION_ID), Some(&pass));
            assert_eq!(
                venue_closed_in_cache(&instruments_cache, &ids[0]),
                Some(false),
            );
        }

        let refreshed = refresh_expired_instrument_liveness(
            &client,
            &instruments_cache,
            &tx,
            &mut misses,
            now_ns,
        )
        .await;

        assert_eq!(refreshed.updated, ids.len());
        assert_eq!(
            venue_closed_in_cache(&instruments_cache, &ids[0]),
            Some(true),
        );
        assert_eq!(
            misses.get(LIVENESS_CONDITION_ID),
            None,
            "the streak must be cleared at retirement, or a re-loaded condition would be retired \
             on its first absence with no confirmation",
        );
    }

    /// A market the venue still reports open stays carried, and an intermittent miss never
    /// accumulates into a retirement.
    #[rstest]
    #[tokio::test]
    async fn condition_returned_open_stays_carried_and_clears_its_streak() {
        let addr = start_liveness_test_server(LivenessServerState {
            open: serde_json::json!([liveness_market_json(false)]),
            closed: serde_json::json!([]),
            fail: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        })
        .await;

        let (instruments_cache, tx, _rx, ids) = liveness_fixture();
        let mut misses: AHashMap<String, u32> = AHashMap::new();
        misses.insert(LIVENESS_CONDITION_ID.to_string(), 2);
        let now_ns = get_atomic_clock_realtime().get_time_ns();

        let refreshed = refresh_expired_instrument_liveness(
            &liveness_client(addr),
            &instruments_cache,
            &tx,
            &mut misses,
            now_ns,
        )
        .await;

        assert!(refreshed.probed);
        assert_eq!(misses.get(LIVENESS_CONDITION_ID), None);

        for instrument_id in &ids {
            assert_eq!(
                venue_closed_in_cache(&instruments_cache, instrument_id),
                Some(false),
                "a market the venue still reports open must stay carried",
            );
        }
    }

    /// A probe that never reached Gamma must retire nothing and must not count as a miss, so the
    /// caller can retry on the next tick instead of deferring for a full interval.
    #[rstest]
    #[tokio::test]
    async fn failed_probe_retires_nothing_and_does_not_defer() {
        let addr = start_liveness_test_server(LivenessServerState {
            open: serde_json::json!([]),
            closed: serde_json::json!([]),
            fail: Arc::new(std::sync::atomic::AtomicBool::new(true)),
        })
        .await;

        let (instruments_cache, tx, _rx, ids) = liveness_fixture();
        let mut misses: AHashMap<String, u32> = AHashMap::new();
        let now_ns = get_atomic_clock_realtime().get_time_ns();

        let refreshed = refresh_expired_instrument_liveness(
            &liveness_client(addr),
            &instruments_cache,
            &tx,
            &mut misses,
            now_ns,
        )
        .await;

        assert!(
            !refreshed.probed,
            "a failed probe must not defer the next one"
        );
        assert_eq!(refreshed.updated, 0);
        assert!(misses.is_empty(), "a failed request is not an observation");
        assert_eq!(
            venue_closed_in_cache(&instruments_cache, &ids[0]),
            Some(false),
        );
    }

    /// The refresh takes only the venue state from Gamma. A `tick_size_change` rebuilds the cached
    /// definition, and swapping the Gamma copy in wholesale would revert that rebuild for a market
    /// the venue is still trading.
    #[rstest]
    #[tokio::test]
    async fn refresh_preserves_a_cached_tick_size_rebuild() {
        let addr = start_liveness_test_server(LivenessServerState {
            open: serde_json::json!([liveness_market_json(false)]),
            closed: serde_json::json!([]),
            fail: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        })
        .await;

        let (instruments_cache, tx, _rx, ids) = liveness_fixture();
        let instrument_id = ids[0];

        // Gamma reports a 0.01 tick; the venue then lowers it over the WebSocket.
        let rebuilt = {
            let loaded = instruments_cache.load();
            let cached = loaded.get(&instrument_id).expect("cached instrument");
            assert_eq!(cached.price_increment(), Price::from("0.01"));
            crate::http::parse::rebuild_instrument_with_tick_size(
                cached,
                "0.001",
                UnixNanos::default(),
                UnixNanos::default(),
            )
            .expect("rebuild instrument")
        };
        instruments_cache.insert(instrument_id, rebuilt);

        let mut misses: AHashMap<String, u32> = AHashMap::new();
        refresh_expired_instrument_liveness(
            &liveness_client(addr),
            &instruments_cache,
            &tx,
            &mut misses,
            get_atomic_clock_realtime().get_time_ns(),
        )
        .await;

        let loaded = instruments_cache.load();
        let cached = loaded.get(&instrument_id).expect("cached instrument");
        assert_eq!(
            cached.price_increment(),
            Price::from("0.001"),
            "the liveness refresh must not revert a tick size the venue lowered",
        );
    }
}
