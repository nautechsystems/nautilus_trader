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

//! Verifies option chain replay and assembly during backtest (milestone M2).
//!
//! Loads per-instrument [`QuoteTick`] and [`OptionGreeks`] from a catalog, subscribes a strategy
//! to the option chain, and asserts that the `OptionChainManager` assembles [`OptionChainSlice`]s
//! during replay just as it does live:
//!
//! - Raw mode (`snapshot_interval_ms = None`) publishes one slice per active-instrument update.
//! - Thinned mode (`snapshot_interval_ms = Some(..)`) publishes only on the interval timer.
//! - Both modes behave identically under `run_oneshot` and `run_streaming`, including timers that
//!   straddle streaming chunk boundaries.
//!
//! Quotes reach the catalog through the standard `write_to_parquet` path (the `quotes` prefix the
//! backtest loader reads), not the Tardis replay writer, whose legacy `quote_tick` prefix diverges
//! from the catalog layout. A `Fixed` strike range is used so the manager bootstraps at creation;
//! ATM-relative and delta selection are milestone M3.

#![cfg(feature = "streaming")]

use std::{cell::RefCell, fmt::Debug, rc::Rc};

use nautilus_backtest::{
    config::{BacktestDataConfig, BacktestRunConfig, BacktestVenueConfig, NautilusDataType},
    node::BacktestNode,
};
use nautilus_common::actor::DataActor;
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{
        QuoteTick,
        greeks::OptionGreekValues,
        option_chain::{OptionChainSlice, OptionGreeks, StrikeRange},
    },
    enums::{AccountType, BookType, OmsType, OptionKind},
    identifiers::{InstrumentId, OptionSeriesId, StrategyId, Symbol, Venue},
    instruments::{CryptoOption, Instrument, InstrumentAny},
    types::{Currency, Money, Price, Quantity},
};
use nautilus_persistence::backend::catalog::ParquetDataCatalog;
use nautilus_trading::{StrategyConfig, StrategyCore, nautilus_strategy};
use rstest::*;
use tempfile::TempDir;
use ustr::Ustr;

const EXPIRATION_NS: u64 = 1_704_067_200_000_000_000;
const STRIKE: &str = "50000.000";
/// Aligned so the snapshot timer's first fire lands on a clean interval boundary.
const BASE_TS: u64 = 1_700_000_000_000_000_000;
/// Spacing between consecutive quotes (and between consecutive greeks); greeks are offset by half.
const QUOTE_STEP_NS: u64 = 250_000_000;
const GREEKS_OFFSET_NS: u64 = 125_000_000;
/// Number of (quote, greeks) updates fed per side-alternating index.
const STEPS: usize = 20;
const SNAPSHOT_INTERVAL_NS: u64 = 500_000_000;

fn make_btc_option(strike: &str, kind: OptionKind) -> InstrumentAny {
    let kind_char = match kind {
        OptionKind::Call => "C",
        OptionKind::Put => "P",
    };
    let symbol_str = format!("BTC-20240101-{strike}-{kind_char}.DERIBIT");
    let raw_symbol_str = symbol_str.split('.').next().unwrap();
    InstrumentAny::CryptoOption(CryptoOption::new(
        InstrumentId::from(symbol_str.as_str()),
        Symbol::from(raw_symbol_str),
        Currency::from("BTC"),
        Currency::USD(),
        Currency::from("BTC"),
        false,
        kind,
        Price::from(strike),
        UnixNanos::from(1_671_696_000_000_000_000u64),
        UnixNanos::from(EXPIRATION_NS),
        3,
        1,
        Price::from("0.001"),
        Quantity::from("0.1"),
        Some(Quantity::from(1)),
        Some(Quantity::from(1)),
        Some(Quantity::from("9000.0")),
        Some(Quantity::from("0.1")),
        None,
        Some(Money::new(10.00, Currency::USD())),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        0.into(),
        0.into(),
    ))
}

fn deribit_venue_config() -> BacktestVenueConfig {
    BacktestVenueConfig::builder()
        .name(Ustr::from("DERIBIT"))
        .oms_type(OmsType::Netting)
        .account_type(AccountType::Margin)
        .book_type(BookType::L1_MBP)
        .starting_balances(vec!["10 BTC".to_string()])
        .build()
}

fn series_id() -> OptionSeriesId {
    OptionSeriesId::new(
        Venue::new("DERIBIT"),
        Ustr::from("BTC"),
        Ustr::from("BTC"),
        UnixNanos::from(EXPIRATION_NS),
    )
}

/// Quote `ts_event`/`ts_init` for the update at `index`.
fn quote_ts(index: usize) -> u64 {
    BASE_TS + index as u64 * QUOTE_STEP_NS
}

/// Greeks `ts_event`/`ts_init` for the update at `index` (offset half a step after the quote).
fn greeks_ts(index: usize) -> u64 {
    quote_ts(index) + GREEKS_OFFSET_NS
}

/// Timestamp of the final replayed event (the last greeks update).
fn last_data_ts() -> u64 {
    greeks_ts(STEPS - 1)
}

/// The instrument for update `index`: calls on even indices, puts on odd.
fn side_for(
    index: usize,
    call_id: InstrumentId,
    put_id: InstrumentId,
) -> (InstrumentId, OptionKind) {
    if index.is_multiple_of(2) {
        (call_id, OptionKind::Call)
    } else {
        (put_id, OptionKind::Put)
    }
}

fn make_quote(index: usize, instrument_id: InstrumentId, kind: OptionKind) -> QuoteTick {
    // Distinct, monotonically rising prices per side so the latest buffered quote is identifiable.
    let bid_base = match kind {
        OptionKind::Call => 0.300,
        OptionKind::Put => 0.200,
    };
    let bid = bid_base + index as f64 * 0.001;
    let ts = quote_ts(index);
    QuoteTick::new(
        instrument_id,
        Price::from(format!("{bid:.3}").as_str()),
        Price::from(format!("{:.3}", bid + 0.005).as_str()),
        Quantity::from("1.0"),
        Quantity::from("1.0"),
        UnixNanos::from(ts),
        UnixNanos::from(ts),
    )
}

fn make_greeks(index: usize, instrument_id: InstrumentId, kind: OptionKind) -> OptionGreeks {
    let delta = match kind {
        OptionKind::Call => 0.55,
        OptionKind::Put => -0.45,
    };
    let ts = greeks_ts(index);
    OptionGreeks {
        instrument_id,
        greeks: OptionGreekValues {
            delta,
            ..Default::default()
        },
        underlying_price: Some(50_000.0),
        ts_event: UnixNanos::from(ts),
        ts_init: UnixNanos::from(ts),
        ..Default::default()
    }
}

/// Builds a catalog with a call + put at one strike, plus interleaved quotes and greeks.
///
/// The first event (lowest `ts_init`) is a call quote, so the aggregator buffer is non-empty for
/// every later update; raw mode therefore publishes exactly one slice per update.
fn build_catalog() -> (
    TempDir,
    String,
    InstrumentId,
    InstrumentId,
    Vec<QuoteTick>,
    Vec<OptionGreeks>,
) {
    let temp_dir = TempDir::new().unwrap();
    let catalog_path = temp_dir.path().to_str().unwrap().to_string();
    let catalog = ParquetDataCatalog::new(temp_dir.path(), None, None, None, None);

    let call = make_btc_option(STRIKE, OptionKind::Call);
    let put = make_btc_option(STRIKE, OptionKind::Put);
    let call_id = call.id();
    let put_id = put.id();
    catalog.write_instruments(vec![call, put]).unwrap();

    let mut quotes = Vec::with_capacity(STEPS);
    let mut greeks = Vec::with_capacity(STEPS);
    for i in 0..STEPS {
        let (id, kind) = side_for(i, call_id, put_id);
        quotes.push(make_quote(i, id, kind));
        greeks.push(make_greeks(i, id, kind));
    }

    // `write_to_parquet` stamps every row of a batch with one instrument_id (from the schema
    // metadata), so each instrument must be written separately or the put rows would be relabelled
    // as calls. Per-instrument batches stay ascending in `ts_init` and land in disjoint directories.
    for id in [call_id, put_id] {
        catalog
            .write_to_parquet(
                quotes
                    .iter()
                    .filter(|q| q.instrument_id == id)
                    .copied()
                    .collect(),
                None,
                None,
                None,
            )
            .unwrap();
        catalog
            .write_to_parquet(
                greeks
                    .iter()
                    .filter(|g| g.instrument_id == id)
                    .copied()
                    .collect(),
                None,
                None,
                None,
            )
            .unwrap();
    }

    (temp_dir, catalog_path, call_id, put_id, quotes, greeks)
}

#[derive(Debug)]
struct ChainSubscriber {
    core: StrategyCore,
    series_id: OptionSeriesId,
    strike_range: StrikeRange,
    snapshot_interval_ms: Option<u64>,
    received: Rc<RefCell<Vec<OptionChainSlice>>>,
}

impl ChainSubscriber {
    fn new(
        series_id: OptionSeriesId,
        strike_range: StrikeRange,
        snapshot_interval_ms: Option<u64>,
        received: Rc<RefCell<Vec<OptionChainSlice>>>,
    ) -> Self {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("CHAIN-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        Self {
            core: StrategyCore::new(config),
            series_id,
            strike_range,
            snapshot_interval_ms,
            received,
        }
    }
}

nautilus_strategy!(ChainSubscriber);

impl DataActor for ChainSubscriber {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.subscribe_option_chain(
            self.series_id,
            self.strike_range.clone(),
            self.snapshot_interval_ms,
            None,
            None,
        );
        Ok(())
    }

    fn on_option_chain(&mut self, slice: &OptionChainSlice) -> anyhow::Result<()> {
        self.received.borrow_mut().push(slice.clone());
        Ok(())
    }
}

/// Runs a single backtest subscribing to the option chain and returns the slices received.
fn run_chain_backtest(
    catalog_path: &str,
    call_id: InstrumentId,
    put_id: InstrumentId,
    snapshot_interval_ms: Option<u64>,
    chunk_size: Option<usize>,
) -> Vec<OptionChainSlice> {
    let instrument_ids = vec![call_id, put_id];
    let quote_data = BacktestDataConfig::builder()
        .data_type(NautilusDataType::QuoteTick)
        .catalog_path(catalog_path.to_string())
        .instrument_ids(instrument_ids.clone())
        .build();
    let greeks_data = BacktestDataConfig::builder()
        .data_type(NautilusDataType::OptionGreeks)
        .catalog_path(catalog_path.to_string())
        .instrument_ids(instrument_ids)
        .build();

    let config = BacktestRunConfig::builder()
        .venues(vec![deribit_venue_config()])
        .data(vec![quote_data, greeks_data])
        .maybe_chunk_size(chunk_size)
        .build();
    let config_id = config.id().to_string();

    let mut node = BacktestNode::new(vec![config]).unwrap();
    node.build().unwrap();

    let received = Rc::new(RefCell::new(Vec::new()));
    let engine = node.get_engine_mut(&config_id).unwrap();
    engine
        .add_strategy(ChainSubscriber::new(
            series_id(),
            StrikeRange::Fixed(vec![Price::from(STRIKE)]),
            snapshot_interval_ms,
            received.clone(),
        ))
        .unwrap();

    node.run().unwrap();

    received.borrow().clone()
}

/// At most this many leading updates may be missed before the chain subscription goes live.
///
/// A subscribe issued from `on_start` reaches the `DataEngine` through the command pipeline, which
/// drains over the first couple of replayed ticks. The manager therefore misses a small, fixed
/// prefix of updates, mirroring live where data arriving before the subscription completes is also
/// not seen. This bounds that warmup so a regression that drops many updates still fails the test.
const STARTUP_DROP_MAX: usize = 2;

/// All quote and greeks update timestamps fed to the catalog, sorted ascending (the order replay
/// delivers them).
fn all_update_timestamps() -> Vec<u64> {
    let mut ts: Vec<u64> = (0..STEPS)
        .flat_map(|i| [quote_ts(i), greeks_ts(i)])
        .collect();
    ts.sort_unstable();
    ts
}

/// The interval boundaries at which the snapshot timer fires during the run.
///
/// The manager schedules the timer at the next interval boundary after the (aligned) start, and
/// with `fire_immediately = false` the first event lands one interval later. Fires then recur every
/// `SNAPSHOT_INTERVAL_NS` up to and including the last data timestamp.
fn expected_timer_boundaries() -> Vec<u64> {
    let start_aligned = BASE_TS - (BASE_TS % SNAPSHOT_INTERVAL_NS) + SNAPSHOT_INTERVAL_NS;
    let mut boundaries = Vec::new();
    let mut fire = start_aligned + SNAPSHOT_INTERVAL_NS;
    while fire <= last_data_ts() {
        boundaries.push(fire);
        fire += SNAPSHOT_INTERVAL_NS;
    }
    boundaries
}

/// Asserts the raw-mode invariant: exactly one slice per observed update, with no timer-driven
/// slices and no gaps. The received timestamps must be the contiguous suffix of all update
/// timestamps starting at the first one the live manager saw.
fn assert_one_slice_per_update(slices: &[OptionChainSlice]) {
    assert!(!slices.is_empty(), "expected at least one slice");

    let received: Vec<u64> = slices.iter().map(|s| s.ts_init.as_u64()).collect();
    let all = all_update_timestamps();
    let first_seen = received[0];
    let expected: Vec<u64> = all.iter().copied().filter(|t| *t >= first_seen).collect();

    assert_eq!(
        received, expected,
        "raw mode must publish exactly one slice per update from the first observed onward",
    );

    let dropped = all.len() - received.len();
    assert!(
        dropped <= STARTUP_DROP_MAX,
        "dropped {dropped} startup updates, expected at most {STARTUP_DROP_MAX}",
    );
}

/// Asserts that assembled slice contents trace back to replayed inputs: every quote and attached
/// greeks payload in every slice is one of the inputs, and the final slice carries both a call and
/// a put at the strike with the correct quote and greeks on each side.
fn assert_contents_trace_inputs(
    slices: &[OptionChainSlice],
    quotes: &[QuoteTick],
    greeks: &[OptionGreeks],
    call_id: InstrumentId,
    put_id: InstrumentId,
) {
    let strike = Price::from(STRIKE);

    for slice in slices {
        for (_, data) in slice.calls.iter().chain(slice.puts.iter()) {
            assert!(
                quotes.contains(&data.quote),
                "every slice quote must be a replayed input, found {:?}",
                data.quote,
            );

            if let Some(option_greeks) = data.greeks {
                assert!(
                    greeks.contains(&option_greeks),
                    "every slice greeks must be a replayed input, found {option_greeks:?}",
                );
            }
        }
    }

    let last = slices.last().expect("expected at least one slice");
    let call = last.get_call(&strike).expect("expected a call entry");
    let put = last.get_put(&strike).expect("expected a put entry");
    assert_eq!(call.quote.instrument_id, call_id);
    assert_eq!(put.quote.instrument_id, put_id);

    let call_greeks = call.greeks.expect("expected call greeks");
    assert!(
        greeks.contains(&call_greeks),
        "final call greeks must be a replayed input, found {call_greeks:?}",
    );
    assert_eq!(call_greeks.instrument_id, call_id);
    assert_eq!(call_greeks.delta, 0.55);

    let put_greeks = put.greeks.expect("expected put greeks");
    assert!(
        greeks.contains(&put_greeks),
        "final put greeks must be a replayed input, found {put_greeks:?}",
    );
    assert_eq!(put_greeks.instrument_id, put_id);
    assert_eq!(put_greeks.delta, -0.45);
}

#[rstest]
fn test_raw_mode_oneshot_publishes_one_slice_per_update() {
    let (_temp_dir, catalog_path, call_id, put_id, quotes, _greeks) = build_catalog();

    let slices = run_chain_backtest(&catalog_path, call_id, put_id, None, None);

    assert_one_slice_per_update(&slices);

    // The final slice reflects the latest buffered quote and greeks for both sides
    let strike = Price::from(STRIKE);
    let last = slices.last().unwrap();
    assert_eq!(last.series_id, series_id());
    assert_eq!(last.atm_strike, Some(strike)); // greeks carried underlying_price = 50000

    let last_call_quote = quotes
        .iter()
        .rev()
        .find(|q| q.instrument_id == call_id)
        .unwrap();
    let last_put_quote = quotes
        .iter()
        .rev()
        .find(|q| q.instrument_id == put_id)
        .unwrap();

    let call = last.get_call(&strike).expect("expected a call entry");
    assert_eq!(&call.quote, last_call_quote);
    assert_eq!(call.greeks.expect("expected call greeks").delta, 0.55);

    let put = last.get_put(&strike).expect("expected a put entry");
    assert_eq!(&put.quote, last_put_quote);
    assert_eq!(put.greeks.expect("expected put greeks").delta, -0.45);
}

#[rstest]
fn test_raw_mode_streaming_publishes_one_slice_per_update() {
    let (_temp_dir, catalog_path, call_id, put_id, _quotes, _greeks) = build_catalog();

    // chunk_size = 7 does not divide the 40 events evenly, forcing mid-stream chunk boundaries
    let slices = run_chain_backtest(&catalog_path, call_id, put_id, None, Some(7));

    // Raw publishing is event-driven, so chunking must not change which slices are produced: the
    // same one-per-update cadence as oneshot.
    assert_one_slice_per_update(&slices);
}

#[rstest]
fn test_thinned_mode_oneshot_publishes_on_timer() {
    let (_temp_dir, catalog_path, call_id, put_id, quotes, greeks) = build_catalog();

    let slices = run_chain_backtest(&catalog_path, call_id, put_id, Some(500), None);

    let boundaries = expected_timer_boundaries();
    assert!(!boundaries.is_empty());
    // Thinning publishes far fewer slices than raw mode (one per timer fire, not per update)
    assert!(slices.len() < all_update_timestamps().len());

    let ts_inits: Vec<u64> = slices.iter().map(|s| s.ts_init.as_u64()).collect();
    assert_eq!(ts_inits, boundaries);

    // Every published snapshot carries data, not an empty timer tick
    for slice in &slices {
        assert_eq!(slice.series_id, series_id());
        assert!(!slice.is_empty());
    }

    // The thinned snapshots assemble from the same replayed quotes and greeks as raw mode
    assert_contents_trace_inputs(&slices, &quotes, &greeks, call_id, put_id);
}

#[rstest]
fn test_thinned_mode_streaming_matches_oneshot_across_chunk_boundaries() {
    let (_temp_dir, catalog_path, call_id, put_id, quotes, greeks) = build_catalog();

    // chunk_size = 7 places several interval boundaries inside later chunks, so the recurring
    // timer must fire correctly after each chunk's clock is carried forward.
    let slices = run_chain_backtest(&catalog_path, call_id, put_id, Some(500), Some(7));

    let ts_inits: Vec<u64> = slices.iter().map(|s| s.ts_init.as_u64()).collect();
    // Identical to the oneshot cadence: chunk boundaries do not drop, duplicate, or shift fires
    assert_eq!(ts_inits, expected_timer_boundaries());

    // Chunked replay assembles the same input-derived quotes and greeks as oneshot
    assert_contents_trace_inputs(&slices, &quotes, &greeks, call_id, put_id);
}
