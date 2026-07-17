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

//! Live-capture -> replay round-trip DIFFERENTIAL for the fill lifecycle.
//!
//! Runs a real `ExecutionEngine` with the bus capture tap installed (mirroring the
//! kernel's `EventStoreBusTap`) through the pre-fill-void maintainer trace PLUS an
//! open-position hedging flip, a pre-fill void allocated across flip fragments, a
//! closed->reopen-opposite lifecycle, and a spread fill; then replays the captured store
//! into a fresh cache and compares order, position, and portfolio state field-wise.
//! This pins "true live/replay parity" as a property instead of asserting it per-path.

use std::{
    any::Any,
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};

use indexmap::IndexMap;
use nautilus_common::{
    cache::Cache,
    clock::TestClock,
    msgbus::{self, BusTap, Endpoint, MStr, MessageBus, Topic as BusTopic, switchboard},
};
use nautilus_core::{UUID4, UnixNanos, time::get_atomic_clock_static};
use nautilus_event_store::{
    AppendEntry, BusCaptureAdapter, EventStore, EventStoreEntry, EventStoreError, EventStoreReader,
    EventStoreWriter, HaltCallback, Headers, IndexKind, MemoryBackend, RegisteredComponents,
    RunManifest, RunStatus, ScanDirection, Topic, WriterConfig, default_registry, noop_halt,
    replay_cache_snapshot_tail,
};
use nautilus_execution::engine::{ExecutionEngine, stubs::StubExecutionClient};
use nautilus_model::{
    accounts::CashAccount,
    enums::{LiquiditySide, OmsType, OrderSide, OrderType},
    events::{
        OrderEventAny, OrderFilled,
        order::spec::{OrderFillVoidedSpec, OrderFilledSpec},
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, PositionId, StrategyId, TradeId, TraderId, Venue,
        VenueOrderId,
    },
    instruments::{
        Instrument, InstrumentAny,
        stubs::{audusd_sim, futures_spread_es},
    },
    orders::{Order, OrderAny, OrderTestBuilder, stubs::TestOrderEventStubs},
    position::Position,
    stubs::TestDefault,
    types::{Money, Price, Quantity},
};
use nautilus_portfolio::Portfolio;
use rstest::rstest;

const INSTANCE_ID: &str = "trader-001";

fn manifest(run_id: &str) -> RunManifest {
    RunManifest {
        run_id: run_id.to_string(),
        parent_run_id: None,
        instance_id: INSTANCE_ID.to_string(),
        binary_hash: "deadbeef".to_string(),
        schema_version: 1,
        crate_versions: "feedface".to_string(),
        feature_flags: Vec::new(),
        adapter_versions: IndexMap::new(),
        config_hash: "cafebabe".to_string(),
        registered_components: RegisteredComponents::default(),
        seed: None,
        start_ts_init: UnixNanos::from(0),
        end_ts_init: None,
        high_watermark: 0,
        status: RunStatus::Running,
    }
}

/// Wraps `MemoryBackend` so the writer thread and the test thread can read the same
/// backend instance.
#[derive(Debug)]
struct SharedMemory(Arc<Mutex<MemoryBackend>>);

impl EventStore for SharedMemory {
    fn open_run(&mut self, _: RunManifest) -> Result<(), EventStoreError> {
        unreachable!("test wrapper does not forward open_run")
    }

    fn append_batch(&mut self, entries: &[AppendEntry]) -> Result<u64, EventStoreError> {
        self.0.lock().expect("shared").append_batch(entries)
    }

    fn scan_range(
        &self,
        from: u64,
        to: u64,
        direction: ScanDirection,
    ) -> Result<Vec<EventStoreEntry>, EventStoreError> {
        self.0
            .lock()
            .expect("shared")
            .scan_range(from, to, direction)
    }

    fn scan_seq(&self, seq: u64) -> Result<Option<EventStoreEntry>, EventStoreError> {
        self.0.lock().expect("shared").scan_seq(seq)
    }

    fn lookup(&self, kind: IndexKind, key: &str) -> Result<Option<u64>, EventStoreError> {
        self.0.lock().expect("shared").lookup(kind, key)
    }

    fn iter_index_keys(&self, kind: IndexKind) -> Result<Vec<(String, u64)>, EventStoreError> {
        self.0.lock().expect("shared").iter_index_keys(kind)
    }

    fn seal(&mut self, status: RunStatus) -> Result<(), EventStoreError> {
        self.0.lock().expect("shared").seal(status)
    }

    fn manifest(&self) -> Result<RunManifest, EventStoreError> {
        self.0.lock().expect("shared").manifest()
    }

    fn high_watermark(&self) -> Result<u64, EventStoreError> {
        self.0.lock().expect("shared").high_watermark()
    }
}

fn writer_with_open_run(
    run_id: &str,
    halt: HaltCallback,
) -> (Arc<EventStoreWriter>, Arc<Mutex<MemoryBackend>>) {
    let backend_arc: Arc<Mutex<MemoryBackend>> = Arc::new(Mutex::new(MemoryBackend::new()));
    backend_arc
        .lock()
        .expect("inner")
        .open_run(manifest(run_id))
        .expect("open run");

    let wrapper = SharedMemory(Arc::clone(&backend_arc));
    let writer = EventStoreWriter::spawn(
        Box::new(wrapper),
        get_atomic_clock_static(),
        halt,
        WriterConfig::default(),
    )
    .expect("spawn");
    (Arc::new(writer), backend_arc)
}

// Waits (bounded) for the writer's I/O thread to drain: the high watermark must stay
// unchanged across two consecutive polls. The engine itself runs on virtual time; this
// only synchronizes with the writer's real background thread, as the existing capture
// integration tests do.
fn drain_stable(writer: &Arc<EventStoreWriter>) -> u64 {
    let deadline = Duration::from_secs(2);
    let mut waited = Duration::ZERO;
    let mut last = writer.high_watermark();

    loop {
        std::thread::sleep(Duration::from_millis(20));
        waited += Duration::from_millis(20);
        let current = writer.high_watermark();

        if current == last && current > 0 {
            return current;
        }
        assert!(
            waited < deadline,
            "writer did not drain within {deadline:?}"
        );
        last = current;
    }
}

/// Mirrors the kernel's `EventStoreBusTap`: forwards every publish AND send to the
/// capture adapter so the test captures exactly what a live run would.
struct CaptureTap {
    adapter: Arc<BusCaptureAdapter>,
}

impl BusTap for CaptureTap {
    fn on_publish(&self, topic: MStr<BusTopic>, message: &dyn Any) {
        let _ = self.adapter.capture_any(
            Topic::from(*topic),
            message,
            Headers::empty(),
            UnixNanos::from(0),
        );
    }

    fn on_send(&self, endpoint: MStr<Endpoint>, message: &dyn Any) {
        let _ = self.adapter.capture_any(
            Topic::from(*endpoint),
            message,
            Headers::empty(),
            UnixNanos::from(0),
        );
    }
}

fn accept_order(
    engine: &mut ExecutionEngine,
    instrument: &InstrumentAny,
    client_order_id: &str,
    side: OrderSide,
    quantity: Quantity,
    venue_order_id: &str,
    reduce_only: bool,
) -> OrderAny {
    let mut builder = OrderTestBuilder::new(OrderType::Market);
    builder
        .trader_id(TraderId::test_default())
        .strategy_id(StrategyId::test_default())
        .instrument_id(instrument.id())
        .client_order_id(ClientOrderId::from(client_order_id))
        .side(side)
        .quantity(quantity);

    if reduce_only {
        builder.reduce_only(true);
    }
    let order = builder.build();
    engine
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(ClientId::from("STUB")), true)
        .unwrap();

    // The engine never publishes OrderInitialized itself (strategies do in live runs);
    // publish it here so the capture tap records it and replay can create the order.
    let init_event = order.events()[0].clone();
    let topic = switchboard::get_event_order_topic(order.strategy_id());
    msgbus::publish_order_event(topic, &init_event);

    engine.process(&TestOrderEventStubs::submitted(
        &order,
        AccountId::test_default(),
    ));
    engine.process(&TestOrderEventStubs::accepted(
        &order,
        AccountId::test_default(),
        VenueOrderId::from(venue_order_id),
    ));
    order
}

#[expect(clippy::too_many_arguments)]
fn build_fill(
    order: &OrderAny,
    instrument: &InstrumentAny,
    trade_id: &str,
    venue_order_id: &str,
    last_qty: Quantity,
    last_px: &str,
    position_id: PositionId,
    commission: Option<Money>,
) -> OrderFilled {
    OrderFilledSpec::builder()
        .trader_id(order.trader_id())
        .strategy_id(order.strategy_id())
        .instrument_id(instrument.id())
        .client_order_id(order.client_order_id())
        .venue_order_id(VenueOrderId::from(venue_order_id))
        .account_id(AccountId::test_default())
        .trade_id(TradeId::from(trade_id))
        .order_side(order.order_side())
        .order_type(order.order_type())
        .last_qty(last_qty)
        .last_px(Price::from(last_px))
        .currency(instrument.quote_currency())
        .liquidity_side(LiquiditySide::Maker)
        .position_id(position_id)
        .event_id(UUID4::new())
        .maybe_commission(commission)
        .build()
}

fn build_pre_fill_void(
    order: &OrderAny,
    instrument: &InstrumentAny,
    trade_id: &str,
    venue_order_id: &str,
    voided_qty: Quantity,
    last_px: &str,
    is_reopened: bool,
) -> OrderEventAny {
    OrderEventAny::FillVoided(
        OrderFillVoidedSpec::builder()
            .trader_id(order.trader_id())
            .strategy_id(order.strategy_id())
            .instrument_id(instrument.id())
            .client_order_id(order.client_order_id())
            .venue_order_id(VenueOrderId::from(venue_order_id))
            .account_id(AccountId::test_default())
            .trade_id(TradeId::from(trade_id))
            .voided_qty(voided_qty)
            .order_side(order.order_side())
            .order_type(order.order_type())
            .last_px(Price::from(last_px))
            .currency(instrument.quote_currency())
            .liquidity_side(LiquiditySide::Maker)
            .is_reopened(is_reopened)
            .build(),
    )
}

fn assert_positions_match(live: &Position, replayed: &Position) {
    let id = live.id;
    assert_eq!(live.quantity, replayed.quantity, "quantity for {id}");
    assert_eq!(live.side, replayed.side, "side for {id}");
    assert_eq!(live.signed_qty, replayed.signed_qty, "signed_qty for {id}");
    assert_eq!(live.entry, replayed.entry, "entry for {id}");
    assert_eq!(
        live.avg_px_open, replayed.avg_px_open,
        "avg_px_open for {id}"
    );
    assert_eq!(
        live.avg_px_close, replayed.avg_px_close,
        "avg_px_close for {id}"
    );
    assert_eq!(
        live.realized_return, replayed.realized_return,
        "realized_return for {id}"
    );
    assert_eq!(
        live.realized_pnl, replayed.realized_pnl,
        "realized_pnl for {id}"
    );
    assert_eq!(live.peak_qty, replayed.peak_qty, "peak_qty for {id}");
    assert_eq!(live.ts_opened, replayed.ts_opened, "ts_opened for {id}");
    assert_eq!(live.ts_closed, replayed.ts_closed, "ts_closed for {id}");
    assert_eq!(
        live.opening_order_id, replayed.opening_order_id,
        "opening_order_id for {id}"
    );
    assert_eq!(
        live.closing_order_id, replayed.closing_order_id,
        "closing_order_id for {id}"
    );
    assert_eq!(live.events, replayed.events, "events for {id}");
    assert_eq!(
        format!("{:?}", live.replay_events),
        format!("{:?}", replayed.replay_events),
        "replay_events for {id}"
    );
    assert_eq!(
        format!("{:?}", live.fill_voids),
        format!("{:?}", replayed.fill_voids),
        "fill_voids for {id}"
    );
    assert_eq!(live.trade_ids(), replayed.trade_ids(), "trade_ids for {id}");
    assert_eq!(
        live.commissions(),
        replayed.commissions(),
        "commissions for {id}"
    );
}

fn assert_orders_match(live: &OrderAny, replayed: &OrderAny) {
    let id = live.client_order_id();
    assert_eq!(live.status(), replayed.status(), "status for {id}");
    assert_eq!(
        live.filled_qty(),
        replayed.filled_qty(),
        "filled_qty for {id}"
    );
    assert_eq!(
        live.voided_qty(),
        replayed.voided_qty(),
        "voided_qty for {id}"
    );
    assert_eq!(
        live.leaves_qty(),
        replayed.leaves_qty(),
        "leaves_qty for {id}"
    );
    assert_eq!(live.avg_px(), replayed.avg_px(), "avg_px for {id}");
    assert_eq!(
        format!("{:?}", live.events()),
        format!("{:?}", replayed.events()),
        "events for {id}"
    );
}

#[rstest]
fn live_capture_replay_round_trip_preserves_order_position_and_portfolio_state() {
    *msgbus::get_message_bus().borrow_mut() = MessageBus::default();
    let clock = Rc::new(RefCell::new(TestClock::new()));
    let live_cache = Rc::new(RefCell::new(Cache::default()));
    let mut engine = ExecutionEngine::new(clock, Rc::clone(&live_cache), None);
    engine
        .register_client(Box::new(StubExecutionClient::new(
            ClientId::from("STUB"),
            AccountId::test_default(),
            Venue::test_default(),
            OmsType::Hedging,
            None,
        )))
        .unwrap();

    let audusd = InstrumentAny::from(audusd_sim());
    let spread = InstrumentAny::FuturesSpread(futures_spread_es());
    {
        let mut cache = engine.cache().borrow_mut();
        cache.add_instrument(audusd.clone()).unwrap();
        cache.add_instrument(spread.clone()).unwrap();
        cache.add_account(CashAccount::default().into()).unwrap();
    }
    let live_portfolio = Portfolio::new(
        Rc::new(RefCell::new(TestClock::new())),
        engine.cache().clone(),
        None,
    );

    let (writer, backend_arc) = writer_with_open_run("run-live-replay-parity", noop_halt());
    let registry = Arc::new(default_registry());
    let adapter = Arc::new(BusCaptureAdapter::new(
        Arc::clone(&writer),
        registry,
        noop_halt(),
    ));
    msgbus::set_bus_tap(Rc::new(CaptureTap {
        adapter: Arc::clone(&adapter),
    }));

    // Scenario A - the maintainer trace: pre-fill void, then the late fill catches up.
    let order_a = accept_order(
        &mut engine,
        &audusd,
        "O-CAP-A",
        OrderSide::Buy,
        Quantity::from(100_000),
        "V-CAP-A",
        false,
    );
    engine.process(&build_pre_fill_void(
        &order_a,
        &audusd,
        "T-CAP-A",
        "V-CAP-A",
        Quantity::from(60_000),
        "1.00000",
        true,
    ));
    engine.process(&OrderEventAny::Filled(build_fill(
        &order_a,
        &audusd,
        "T-CAP-A",
        "V-CAP-A",
        Quantity::from(40_000),
        "1.00000",
        PositionId::from("P-CAP-A"),
        None,
    )));

    // Scenario B - an OPEN-position hedging flip: the generated position ID must be
    // claimed from the captured PositionOpened on replay.
    let order_b1 = accept_order(
        &mut engine,
        &audusd,
        "O-CAP-B1",
        OrderSide::Buy,
        Quantity::from(10),
        "V-CAP-B1",
        false,
    );
    engine.process(&OrderEventAny::Filled(build_fill(
        &order_b1,
        &audusd,
        "T-CAP-B1",
        "V-CAP-B1",
        Quantity::from(10),
        "1.00000",
        PositionId::from("P-CAP-B"),
        Some(Money::from("1.00 USD")),
    )));
    let order_b2 = accept_order(
        &mut engine,
        &audusd,
        "O-CAP-B2",
        OrderSide::Sell,
        Quantity::from(15),
        "V-CAP-B2",
        false,
    );
    engine.process(&OrderEventAny::Filled(build_fill(
        &order_b2,
        &audusd,
        "T-CAP-B2",
        "V-CAP-B2",
        Quantity::from(15),
        "1.00010",
        PositionId::from("P-CAP-B"),
        Some(Money::from("1.50 USD")),
    )));

    // Scenario C - close then reopen OPPOSITE under the same ID: live reopens via
    // `open_position`, never the flip path; replay must mirror that dispatch.
    let order_c1 = accept_order(
        &mut engine,
        &audusd,
        "O-CAP-C1",
        OrderSide::Buy,
        Quantity::from(10),
        "V-CAP-C1",
        false,
    );
    engine.process(&OrderEventAny::Filled(build_fill(
        &order_c1,
        &audusd,
        "T-CAP-C1",
        "V-CAP-C1",
        Quantity::from(10),
        "1.00000",
        PositionId::from("P-CAP-C"),
        None,
    )));
    let order_c2 = accept_order(
        &mut engine,
        &audusd,
        "O-CAP-C2",
        OrderSide::Sell,
        Quantity::from(10),
        "V-CAP-C2",
        false,
    );
    engine.process(&OrderEventAny::Filled(build_fill(
        &order_c2,
        &audusd,
        "T-CAP-C2",
        "V-CAP-C2",
        Quantity::from(10),
        "1.00020",
        PositionId::from("P-CAP-C"),
        None,
    )));
    let order_c3 = accept_order(
        &mut engine,
        &audusd,
        "O-CAP-C3",
        OrderSide::Sell,
        Quantity::from(5),
        "V-CAP-C3",
        false,
    );
    engine.process(&OrderEventAny::Filled(build_fill(
        &order_c3,
        &audusd,
        "T-CAP-C3",
        "V-CAP-C3",
        Quantity::from(5),
        "1.00030",
        PositionId::from("P-CAP-C"),
        None,
    )));

    // Scenario D - a spread fill: no position live, no position on replay.
    let order_d = accept_order(
        &mut engine,
        &spread,
        "O-CAP-D",
        OrderSide::Buy,
        Quantity::from(1),
        "V-CAP-D",
        false,
    );
    engine.process(&OrderEventAny::Filled(build_fill(
        &order_d,
        &spread,
        "T-CAP-D",
        "V-CAP-D",
        Quantity::from(1),
        "10.25",
        PositionId::from("P-CAP-D"),
        None,
    )));

    // Scenario E - a pre-fill void on a FLIP fill: the void allocates across both flip
    // fragments; live and replay must allocate identically.
    let order_e1 = accept_order(
        &mut engine,
        &audusd,
        "O-CAP-E1",
        OrderSide::Buy,
        Quantity::from(10),
        "V-CAP-E1",
        false,
    );
    engine.process(&OrderEventAny::Filled(build_fill(
        &order_e1,
        &audusd,
        "T-CAP-E1",
        "V-CAP-E1",
        Quantity::from(10),
        "1.00000",
        PositionId::from("P-CAP-E"),
        None,
    )));
    let order_e2 = accept_order(
        &mut engine,
        &audusd,
        "O-CAP-E2",
        OrderSide::Sell,
        Quantity::from(15),
        "V-CAP-E2",
        false,
    );
    engine.process(&build_pre_fill_void(
        &order_e2,
        &audusd,
        "T-CAP-E2",
        "V-CAP-E2",
        Quantity::from(8),
        "1.00040",
        true,
    ));
    engine.process(&OrderEventAny::Filled(build_fill(
        &order_e2,
        &audusd,
        "T-CAP-E2",
        "V-CAP-E2",
        Quantity::from(15),
        "1.00040",
        PositionId::from("P-CAP-E"),
        None,
    )));

    // Scenario F - a non-reopened mismatched pre-fill void is audit-only once its
    // fill arrives. The live order and position retain the full fill, as must replay.
    let order_f = accept_order(
        &mut engine,
        &audusd,
        "O-CAP-F",
        OrderSide::Buy,
        Quantity::from(100_000),
        "V-CAP-F",
        false,
    );
    engine.process(&build_pre_fill_void(
        &order_f,
        &audusd,
        "T-CAP-F",
        "V-CAP-F",
        Quantity::from(10_000),
        "9.99999",
        false,
    ));
    engine.process(&OrderEventAny::Filled(build_fill(
        &order_f,
        &audusd,
        "T-CAP-F",
        "V-CAP-F",
        Quantity::from(40_000),
        "1.00000",
        PositionId::from("P-CAP-F"),
        None,
    )));

    msgbus::clear_bus_tap();
    drain_stable(&writer);

    // Replay the captured store into a fresh cache (instruments and account arrive from
    // the cache snapshot in a real restore; seed them the same way here).
    let replay_cache = Rc::new(RefCell::new(Cache::default()));
    {
        let mut cache = replay_cache.borrow_mut();
        cache.add_instrument(audusd.clone()).unwrap();
        cache.add_instrument(spread.clone()).unwrap();
        cache.add_account(CashAccount::default().into()).unwrap();
    }
    let reader = EventStoreReader::new(SharedMemory(Arc::clone(&backend_arc)));
    replay_cache_snapshot_tail(&mut replay_cache.borrow_mut(), &reader)
        .expect("captured live run must replay cleanly");

    let live = live_cache.borrow();
    let replayed = replay_cache.borrow();

    // Orders: every order must round-trip event-for-event.
    for order in [
        &order_a, &order_b1, &order_b2, &order_c1, &order_c2, &order_c3, &order_d, &order_e1,
        &order_e2, &order_f,
    ] {
        let client_order_id = order.client_order_id();
        let live_order = live.order(&client_order_id).expect("live order");
        let replayed_order = replayed
            .order(&client_order_id)
            .expect("order must replay into the cache");
        assert_orders_match(&live_order, &replayed_order);
    }

    // Positions: the ID sets must agree (including flip-generated hedging IDs), and
    // every position must match field-for-field.
    let mut live_position_ids: Vec<PositionId> = live
        .positions(None, None, None, None, None)
        .iter()
        .map(|position| position.id)
        .collect();
    let mut replayed_position_ids: Vec<PositionId> = replayed
        .positions(None, None, None, None, None)
        .iter()
        .map(|position| position.id)
        .collect();
    live_position_ids.sort_by_key(|id| id.to_string());
    replayed_position_ids.sort_by_key(|id| id.to_string());
    assert_eq!(live_position_ids, replayed_position_ids);
    assert!(!live_position_ids.is_empty());
    assert!(live.position(&PositionId::from("P-CAP-D")).is_none());
    {
        let live_mismatch_position = live
            .position(&PositionId::from("P-CAP-F"))
            .expect("mismatched void fill must retain live position exposure");
        assert_eq!(live_mismatch_position.quantity, Quantity::from(40_000));
        let live_mismatch_order = live
            .order(&order_f.client_order_id())
            .expect("mismatched void order");
        assert_eq!(live_mismatch_order.filled_qty(), Quantity::from(40_000));
        assert_eq!(live_mismatch_order.voided_qty(), Quantity::from(0));
    }

    for position_id in &live_position_ids {
        let live_position = live.position(position_id).expect("live position");
        let replayed_position = replayed.position(position_id).expect("replayed position");
        assert_positions_match(&live_position, &replayed_position);
    }

    // Portfolio: net exposure derived from the replayed cache must equal the net
    // exposure the live portfolio accumulated from the published position events.
    drop(live);
    drop(replayed);
    let clock = Rc::new(RefCell::new(TestClock::new()));
    let mut replay_portfolio = Portfolio::new(clock, Rc::clone(&replay_cache), None);
    replay_portfolio.initialize_positions();

    for instrument_id in [audusd.id(), spread.id()] {
        assert_eq!(
            live_portfolio.net_position(&instrument_id),
            replay_portfolio.net_position(&instrument_id),
            "net position for {instrument_id}"
        );
    }
}
