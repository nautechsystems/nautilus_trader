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

use super::{PolymarketDataClient, runtime::is_instrument_expired};
use crate::{
    common::consts::GAMMA_CONDITION_IDS_BATCH_SIZE,
    filters::{
        InstrumentFilter, binary_market_closed, is_expired, market_closed, set_market_closed,
    },
    http::{gamma::PolymarketGammaHttpClient, query::GetGammaMarketsParams},
    providers::extract_condition_id,
};

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
    if is_instrument_expired(instrument, now_ns) {
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
            log::debug!(
                "Skipping expired instrument {} during live cache publish",
                instrument.id()
            );
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

// Returns the condition IDs Gamma positively reports as `closed=true`.
//
// Condition IDs the unfiltered lookup does not return are re-queried with `closed=true`, so a
// market that lookup leaves out is still checked. A condition ID absent from both lookups is left
// out: closure was not observed.
async fn probe_closed_condition_ids(
    http: &PolymarketGammaHttpClient,
    condition_ids: &[String],
) -> anyhow::Result<Vec<String>> {
    let open = http
        .request_markets_by_params(GetGammaMarketsParams {
            condition_ids: Some(condition_ids.to_vec()),
            ..Default::default()
        })
        .await?;
    let returned = open
        .iter()
        .map(|market| market.condition_id.as_str())
        .collect::<AHashSet<_>>();
    let mut closed_ids = open
        .iter()
        .filter(|market| market.closed == Some(true))
        .map(|market| market.condition_id.clone())
        .collect::<Vec<_>>();
    let missing = condition_ids
        .iter()
        .filter(|condition_id| !returned.contains(condition_id.as_str()))
        .cloned()
        .collect::<Vec<_>>();

    if missing.is_empty() {
        return Ok(closed_ids);
    }

    let closed = http
        .request_markets_by_params(GetGammaMarketsParams {
            condition_ids: Some(missing),
            closed: Some(true),
            ..Default::default()
        })
        .await?;
    closed_ids.extend(
        closed
            .into_iter()
            .filter(|market| market.closed == Some(true))
            .map(|market| market.condition_id),
    );

    Ok(closed_ids)
}

pub(super) async fn refresh_expired_market_closure(
    http: &PolymarketGammaHttpClient,
    cache: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    now_ns: UnixNanos,
) -> anyhow::Result<usize> {
    let mut carried: AHashMap<String, Vec<InstrumentId>> = AHashMap::new();

    for (id, instrument) in cache.load().iter() {
        if is_expired(instrument, now_ns)
            && market_closed(instrument) == Some(false)
            && let Ok(condition_id) = extract_condition_id(id)
        {
            carried.entry(condition_id).or_default().push(*id);
        }
    }

    let condition_ids = carried.keys().cloned().collect::<Vec<_>>();
    let chunks = condition_ids.chunks(GAMMA_CONDITION_IDS_BATCH_SIZE);
    let total_chunks = chunks.len();
    let mut closed_ids = AHashSet::new();
    let mut failed_chunks = 0;

    // A failed chunk must not discard closures the other chunks already confirmed.
    for chunk in chunks {
        match probe_closed_condition_ids(http, chunk).await {
            Ok(chunk_closed_ids) => closed_ids.extend(chunk_closed_ids),
            Err(e) => {
                failed_chunks += 1;
                log::warn!(
                    "Failed to probe market closure for {} condition ID(s): {e}",
                    chunk.len()
                );
            }
        }
    }

    let closing_ids = closed_ids
        .iter()
        .filter_map(|condition_id| carried.get(condition_id))
        .flatten()
        .copied()
        .collect::<Vec<_>>();
    let mut updated = Vec::new();

    // Compose against the latest cached value, so a concurrent tick size change is not discarded.
    // Guarded because `rcu` clones the whole cache, and most ticks have nothing to close.
    if !closing_ids.is_empty() {
        cache.rcu(|map| {
            updated.clear();

            for instrument_id in &closing_ids {
                if let Some(InstrumentAny::BinaryOption(binary)) = map.get_mut(instrument_id)
                    && binary_market_closed(binary) != Some(true)
                {
                    set_market_closed(binary, true);
                    updated.push(InstrumentAny::BinaryOption(binary.clone()));
                }
            }
        });
    }

    for instrument in &updated {
        let instrument_id = instrument.id();

        // Retirement wins if the instrument was removed after the cache update
        if let Some(latest) = cache.get_cloned(&instrument_id)
            && let Err(e) = sender.send(DataEvent::Instrument(latest))
        {
            log::warn!("Failed to publish market closure update for {instrument_id}: {e}");
        }
    }

    if failed_chunks > 0 {
        anyhow::bail!(
            "Failed to probe market closure for {failed_chunks} of {total_chunks} condition ID chunk(s)"
        );
    }

    Ok(updated.len())
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
    use std::{
        net::SocketAddr,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use axum::{
        Json, Router,
        extract::{RawQuery, State},
        http::StatusCode,
        response::{IntoResponse, Response},
        routing::get,
    };
    use nautilus_common::live::runner::replace_data_event_sender;
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        enums::AssetClass,
        identifiers::{ClientId, Symbol},
        instruments::BinaryOption,
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

    fn past_end_open_market() -> serde_json::Value {
        serde_json::from_str(include_str!(
            "../../test_data/gamma_market_past_end_date_open.json"
        ))
        .unwrap()
    }

    // Mirrors the live Gamma funnel, which records closure state the historical loader must not
    // carry. See `parse_markets_with_transient`.
    fn past_end_open_instrument() -> BinaryOption {
        let market = serde_json::from_value(past_end_open_market()).unwrap();
        let definitions = crate::http::parse::parse_gamma_market(&market).unwrap();
        let instrument =
            crate::http::parse::create_instrument_from_def(&definitions[0], UnixNanos::default())
                .unwrap();
        let InstrumentAny::BinaryOption(mut binary) = instrument else {
            panic!("Expected BinaryOption, was {instrument:?}");
        };
        set_market_closed(&mut binary, false);
        binary
    }

    fn requested_condition_ids(query: Option<&str>) -> Vec<String> {
        query
            .unwrap_or_default()
            .split('&')
            .filter_map(|pair| pair.strip_prefix("condition_ids="))
            .map(ToString::to_string)
            .collect()
    }

    async fn market_closure_response(
        State((closed, fail)): State<(bool, bool)>,
        RawQuery(query): RawQuery,
    ) -> Response {
        if fail {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        let wants_closed = query.unwrap_or_default().contains("closed=true");
        let mut market = past_end_open_market();
        market["closed"] = wants_closed.into();
        market["orderPriceMinTickSize"] = serde_json::json!(0.01);
        let markets = (wants_closed == closed)
            .then_some(market)
            .into_iter()
            .collect::<Vec<_>>();
        Json(serde_json::json!({"markets": markets, "next_cursor": null})).into_response()
    }

    async fn market_closure_server(closed: bool, fail: bool) -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/markets/keyset", get(market_closure_response))
            .with_state((closed, fail));

        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        addr
    }

    #[rstest]
    #[case(false, false)]
    #[case(true, false)]
    #[case(false, true)]
    #[tokio::test]
    async fn market_closure_refresh_uses_positive_signal_without_replacing_definition(
        #[case] closed: bool,
        #[case] fail: bool,
    ) {
        let instrument = InstrumentAny::BinaryOption(past_end_open_instrument());
        let instruments = Arc::new(AtomicMap::new());
        instruments.insert(instrument.id(), instrument.clone());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let addr = market_closure_server(closed, fail).await;
        let client = PolymarketGammaHttpClient::new(
            Some(format!("http://{addr}")),
            1,
            RetryConfig::default(),
        )
        .unwrap();

        let result =
            refresh_expired_market_closure(&client, &instruments, &tx, UnixNanos::from(u64::MAX))
                .await;

        if fail {
            assert!(result.is_err());
            assert!(rx.try_recv().is_err());
            return;
        }

        assert_eq!(result.unwrap(), usize::from(closed));
        let cached = instruments.load();
        let cached = cached.get(&instrument.id()).unwrap();
        assert_eq!(market_closed(cached), Some(closed));
        // Gamma reports a 0.01 tick size; the cached definition keeps its own 0.001.
        assert_eq!(cached.price_increment(), Price::from("0.001"));

        if closed {
            let event = rx.try_recv().expect("closure instrument event");
            let DataEvent::Instrument(published) = event else {
                panic!("Expected instrument event, was {event:?}");
            };
            assert_eq!(published.id(), instrument.id());
            assert_eq!(market_closed(&published), Some(true));
            assert_eq!(published.price_increment(), Price::from("0.001"));
            assert!(rx.try_recv().is_err());
        } else {
            assert!(rx.try_recv().is_err());
        }
    }

    // Serves the first request only, echoing every requested condition ID back as closed.
    async fn first_chunk_only_response(
        State(requests): State<Arc<AtomicUsize>>,
        RawQuery(query): RawQuery,
    ) -> Response {
        if requests.fetch_add(1, Ordering::SeqCst) > 0 {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }

        let markets = requested_condition_ids(query.as_deref())
            .into_iter()
            .map(|condition_id| {
                let mut market = past_end_open_market();
                market["conditionId"] = condition_id.into();
                market["closed"] = true.into();
                market
            })
            .collect::<Vec<_>>();

        Json(serde_json::json!({"markets": markets, "next_cursor": null})).into_response()
    }

    #[rstest]
    #[tokio::test]
    async fn market_closure_refresh_keeps_earlier_chunk_results_when_a_later_chunk_fails() {
        let base = past_end_open_instrument();
        let instruments = Arc::new(AtomicMap::new());

        // One more candidate than a single Gamma request accepts, so the probe spans two chunks.
        for i in 0..=GAMMA_CONDITION_IDS_BATCH_SIZE {
            let mut binary = base.clone();
            binary.raw_symbol = Symbol::new(format!("0xCOND{i:04}-Yes"));
            binary.id = InstrumentId::new(binary.raw_symbol, base.id.venue);
            instruments.insert(binary.id, InstrumentAny::BinaryOption(binary));
        }

        let requests = Arc::new(AtomicUsize::new(0));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/markets/keyset", get(first_chunk_only_response))
            .with_state(requests);

        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let client = PolymarketGammaHttpClient::new(
            Some(format!("http://{addr}")),
            1,
            RetryConfig::default(),
        )
        .unwrap();

        let result =
            refresh_expired_market_closure(&client, &instruments, &tx, UnixNanos::from(u64::MAX))
                .await;

        assert!(result.is_err());

        let closed = instruments
            .load()
            .values()
            .filter(|instrument| market_closed(instrument) == Some(true))
            .count();

        assert_eq!(closed, GAMMA_CONDITION_IDS_BATCH_SIZE);
    }
}
