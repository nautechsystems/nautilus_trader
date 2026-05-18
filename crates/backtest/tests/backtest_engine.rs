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

use std::{
    cell::Cell,
    fmt::Debug,
    sync::atomic::{AtomicU32, Ordering},
};

use nautilus_backtest::{
    config::{BacktestEngineConfig, SimulatedVenueConfig},
    engine::BacktestEngine,
    modules::{ExchangeContext, SimulationModule},
};
use nautilus_common::{
    actor::{
        DataActor, DataActorCore, data_actor::DataActorConfig, registry::try_get_actor_unchecked,
    },
    component::Component,
    enums::ComponentState,
    msgbus, nautilus_actor,
    timer::TimeEvent,
};
use nautilus_core::UnixNanos;
use nautilus_indicators::{
    average::ema::ExponentialMovingAverage,
    indicator::{Indicator, MovingAverage},
};
use nautilus_model::{
    data::{Bar, BarSpecification, BarType, BookOrder, Data, OrderBookDelta, QuoteTick},
    enums::{
        AccountType, AggregationSource, BarAggregation, BookAction, BookType, OmsType, OrderSide,
        PriceType,
    },
    events::OrderFilled,
    identifiers::{ActorId, ExecAlgorithmId, InstrumentId, StrategyId, Venue},
    instruments::{CryptoPerpetual, Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt},
    orders::{Order, OrderAny},
    position::Position,
    types::{Money, Price, Quantity},
};
use nautilus_system::trader::Trader;
use nautilus_trading::{
    ExecutionAlgorithm as ExecutionAlgorithmTrait, ExecutionAlgorithmConfig,
    ExecutionAlgorithmCore, Strategy, StrategyConfig, StrategyCore, nautilus_strategy,
};
use rstest::*;
struct EmptyStrategy {
    core: StrategyCore,
}

impl EmptyStrategy {
    fn new() -> Self {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("EMPTY-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        Self {
            core: StrategyCore::new(config),
        }
    }
}

nautilus_strategy!(EmptyStrategy);

impl Debug for EmptyStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(EmptyStrategy)).finish()
    }
}

impl DataActor for EmptyStrategy {}

struct EmptyActor {
    core: DataActorCore,
}

impl EmptyActor {
    fn new() -> Self {
        let config = DataActorConfig {
            actor_id: Some(ActorId::from("EMPTY-ACTOR-001")),
            ..Default::default()
        };
        Self {
            core: DataActorCore::new(config),
        }
    }
}

nautilus_actor!(EmptyActor);

impl Debug for EmptyActor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(EmptyActor)).finish()
    }
}

impl DataActor for EmptyActor {}

struct EmptyExecAlgorithm {
    core: ExecutionAlgorithmCore,
}

impl EmptyExecAlgorithm {
    fn new() -> Self {
        let config = ExecutionAlgorithmConfig {
            exec_algorithm_id: Some(ExecAlgorithmId::from("EMPTY-EXEC-001")),
            ..Default::default()
        };
        Self {
            core: ExecutionAlgorithmCore::new(config),
        }
    }
}

nautilus_actor!(EmptyExecAlgorithm);

impl Debug for EmptyExecAlgorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(EmptyExecAlgorithm)).finish()
    }
}

impl DataActor for EmptyExecAlgorithm {}

impl ExecutionAlgorithmTrait for EmptyExecAlgorithm {
    fn core_mut(&mut self) -> &mut ExecutionAlgorithmCore {
        &mut self.core
    }

    fn on_order(&mut self, _order: OrderAny) -> anyhow::Result<()> {
        Ok(())
    }
}

struct EmaCross {
    core: StrategyCore,
    instrument_id: InstrumentId,
    trade_size: Quantity,
    ema_fast: ExponentialMovingAverage,
    ema_slow: ExponentialMovingAverage,
    prev_fast_above: Option<bool>,
}

impl EmaCross {
    fn new(
        instrument_id: InstrumentId,
        trade_size: Quantity,
        fast_period: usize,
        slow_period: usize,
    ) -> Self {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("EMA_CROSS-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        Self {
            core: StrategyCore::new(config),
            instrument_id,
            trade_size,
            ema_fast: ExponentialMovingAverage::new(fast_period, Some(PriceType::Mid)),
            ema_slow: ExponentialMovingAverage::new(slow_period, Some(PriceType::Mid)),
            prev_fast_above: None,
        }
    }

    fn enter(&mut self, side: OrderSide) -> anyhow::Result<()> {
        let order = self.core.order_factory().market(
            self.instrument_id,
            side,
            self.trade_size,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        self.submit_order(order, None, None, None)
    }
}

nautilus_strategy!(EmaCross);

impl Debug for EmaCross {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(EmaCross)).finish()
    }
}

impl DataActor for EmaCross {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.subscribe_quotes(self.instrument_id, None, None);
        Ok(())
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        self.ema_fast.handle_quote(quote);
        self.ema_slow.handle_quote(quote);

        if !self.ema_fast.initialized() || !self.ema_slow.initialized() {
            return Ok(());
        }

        let fast = self.ema_fast.value();
        let slow = self.ema_slow.value();
        let fast_above = fast > slow;

        if let Some(prev) = self.prev_fast_above {
            if fast_above && !prev {
                self.enter(OrderSide::Buy)?;
            } else if !fast_above && prev {
                self.enter(OrderSide::Sell)?;
            }
        }

        self.prev_fast_above = Some(fast_above);
        Ok(())
    }
}

struct SnapshotNettingFlip {
    core: StrategyCore,
    instrument_id: InstrumentId,
    trade_size: Quantity,
    tick_count: usize,
}

impl SnapshotNettingFlip {
    fn new(instrument_id: InstrumentId, trade_size: Quantity) -> Self {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("SNAPSHOT-FLIP-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        Self {
            core: StrategyCore::new(config),
            instrument_id,
            trade_size,
            tick_count: 0,
        }
    }

    fn submit_market(&mut self, side: OrderSide) -> anyhow::Result<()> {
        let order = self.core.order_factory().market(
            self.instrument_id,
            side,
            self.trade_size,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        self.submit_order(order, None, None, None)
    }
}

nautilus_strategy!(SnapshotNettingFlip);

impl Debug for SnapshotNettingFlip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(SnapshotNettingFlip)).finish()
    }
}

impl DataActor for SnapshotNettingFlip {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.subscribe_quotes(self.instrument_id, None, None);
        Ok(())
    }

    fn on_quote(&mut self, _quote: &QuoteTick) -> anyhow::Result<()> {
        self.tick_count += 1;

        match self.tick_count {
            2 => self.submit_market(OrderSide::Buy)?,
            4 => self.submit_market(OrderSide::Sell)?,
            6 => self.submit_market(OrderSide::Sell)?,
            8 => self.submit_market(OrderSide::Buy)?,
            _ => {}
        }

        Ok(())
    }
}

#[rstest]
fn test_add_actor_registers_actor_with_trader() {
    let mut engine = BacktestEngine::new(BacktestEngineConfig::default()).unwrap();
    let actor = EmptyActor::new();
    let actor_id = actor.actor_id();

    engine.add_actor(actor).unwrap();

    assert_eq!(engine.kernel().trader.borrow().actor_count(), 1);
    assert!(
        engine
            .kernel()
            .trader
            .borrow()
            .actor_ids()
            .contains(&actor_id)
    );
}

#[rstest]
fn test_add_exec_algorithm_registers_exec_algorithm_with_trader_and_endpoint() {
    let mut engine = BacktestEngine::new(BacktestEngineConfig::default()).unwrap();
    let exec_algorithm = EmptyExecAlgorithm::new();
    let exec_algorithm_id = ExecAlgorithmId::from(exec_algorithm.actor_id().inner().as_str());
    let endpoint = format!("{exec_algorithm_id}.execute");

    engine.add_exec_algorithm(exec_algorithm).unwrap();

    assert_eq!(engine.kernel().trader.borrow().exec_algorithm_count(), 1);
    assert!(
        engine
            .kernel()
            .trader
            .borrow()
            .exec_algorithm_ids()
            .contains(&exec_algorithm_id)
    );
    assert!(msgbus::has_endpoint(&endpoint));
}

#[rstest]
fn test_add_exec_algorithm_while_running_returns_error() {
    let mut engine = BacktestEngine::new(BacktestEngineConfig::default()).unwrap();

    engine
        .kernel_mut()
        .trader
        .borrow_mut()
        .initialize()
        .unwrap();
    engine.kernel_mut().trader.borrow_mut().start().unwrap();

    let result = engine.add_exec_algorithm(EmptyExecAlgorithm::new());
    assert!(result.is_err());
    assert_eq!(
        result.unwrap_err().to_string(),
        "Cannot add execution algorithms to running trader"
    );
    assert_eq!(engine.kernel().trader.borrow().exec_algorithm_count(), 0);
}

#[rstest]
fn test_add_actor_while_running_registers_actor_with_trader() {
    let mut engine = BacktestEngine::new(BacktestEngineConfig::default()).unwrap();
    let actor = EmptyActor::new();
    let actor_id = actor.actor_id();

    engine
        .kernel_mut()
        .trader
        .borrow_mut()
        .initialize()
        .unwrap();
    engine.kernel_mut().trader.borrow_mut().start().unwrap();

    engine.add_actor(actor).unwrap();

    assert_eq!(engine.kernel().trader.borrow().actor_count(), 1);
    assert!(
        engine
            .kernel()
            .trader
            .borrow()
            .actor_ids()
            .contains(&actor_id)
    );
}

#[rstest]
fn test_add_strategy_while_running_registers_strategy_and_market_exit_control() {
    let mut engine = BacktestEngine::new(BacktestEngineConfig::default()).unwrap();
    let strategy = EmptyStrategy::new();
    let strategy_id = StrategyId::from(strategy.actor_id().inner().as_str());
    let strategy_registry_id = strategy_id.inner();

    engine
        .kernel_mut()
        .trader
        .borrow_mut()
        .initialize()
        .unwrap();
    engine.kernel_mut().trader.borrow_mut().start().unwrap();

    engine.add_strategy(strategy).unwrap();

    assert_eq!(engine.kernel().trader.borrow().strategy_count(), 1);
    assert!(
        engine
            .kernel()
            .trader
            .borrow()
            .strategy_ids()
            .contains(&strategy_id)
    );
    assert_eq!(
        try_get_actor_unchecked::<EmptyStrategy>(&strategy_registry_id)
            .unwrap()
            .state(),
        ComponentState::Ready
    );

    engine
        .kernel()
        .trader
        .borrow()
        .start_strategy(&strategy_id)
        .unwrap();
    Trader::market_exit_strategy(&engine.kernel().trader, &strategy_id).unwrap();

    assert!(
        try_get_actor_unchecked::<EmptyStrategy>(&strategy_registry_id)
            .unwrap()
            .is_exiting()
    );
}

fn create_engine() -> BacktestEngine {
    let config = BacktestEngineConfig::default();
    let mut engine = BacktestEngine::new(config).unwrap();
    let venue_config = SimulatedVenueConfig::builder()
        .venue(Venue::from("BINANCE"))
        .oms_type(OmsType::Netting)
        .account_type(AccountType::Margin)
        .book_type(BookType::L1_MBP)
        .starting_balances(vec![Money::from("1_000_000 USDT")])
        .build();
    engine.add_venue(venue_config).unwrap();
    engine
}

fn quote(instrument_id: InstrumentId, bid: &str, ask: &str, ts: u64) -> Data {
    Data::Quote(QuoteTick::new(
        instrument_id,
        Price::from(bid),
        Price::from(ask),
        Quantity::from("1.000"),
        Quantity::from("1.000"),
        ts.into(),
        ts.into(),
    ))
}

fn quote_with_size(instrument_id: InstrumentId, bid: &str, ask: &str, size: &str, ts: u64) -> Data {
    Data::Quote(QuoteTick::new(
        instrument_id,
        Price::from(bid),
        Price::from(ask),
        Quantity::from(size),
        Quantity::from(size),
        ts.into(),
        ts.into(),
    ))
}

fn bid_delta(instrument_id: InstrumentId, price: &str, sequence: u64, ts: u64) -> Data {
    Data::Delta(OrderBookDelta::new(
        instrument_id,
        BookAction::Add,
        BookOrder::new(
            OrderSide::Buy,
            Price::from(price),
            Quantity::from("1.000"),
            sequence,
        ),
        0,
        sequence,
        ts.into(),
        ts.into(),
    ))
}

fn bar_with_aggregation(
    instrument_id: InstrumentId,
    aggregation_source: AggregationSource,
    ts: u64,
) -> Data {
    let bar_type = BarType::new(
        instrument_id,
        BarSpecification::new(1, BarAggregation::Minute, PriceType::Mid),
        aggregation_source,
    );
    Data::Bar(Bar::new(
        bar_type,
        Price::from("1000.00"),
        Price::from("1001.00"),
        Price::from("999.00"),
        Price::from("1000.50"),
        Quantity::from("10.000"),
        ts.into(),
        ts.into(),
    ))
}

#[rstest]
fn test_run_with_empty_data(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    engine
        .add_instrument(&InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt))
        .unwrap();

    let result = engine.run(None, None, None, false);
    assert!(result.is_ok());

    let bt_result = engine.get_result();
    assert_eq!(bt_result.iterations, 0);
    assert_eq!(bt_result.total_orders, 0);
}

#[rstest]
fn test_add_data_rejects_empty(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    engine
        .add_instrument(&InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt))
        .unwrap();

    let err = engine.add_data(vec![], None, true, true).unwrap_err();
    assert!(err.to_string().contains("data was empty"), "got: {err}");
}

#[rstest]
fn test_add_data_rejects_unknown_instrument(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument_id = crypto_perpetual_ethusdt.id();
    // Note: instrument intentionally NOT added to engine.

    let quotes = vec![quote(instrument_id, "1000.00", "1000.10", 1)];
    let err = engine.add_data(quotes, None, true, true).unwrap_err();
    assert!(
        err.to_string().contains("not found in the cache"),
        "got: {err}"
    );
}

#[rstest]
fn test_run_rejects_unsorted_data(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    let quotes = vec![quote(instrument_id, "1000.00", "1000.10", 1_000_000_000)];
    engine.add_data(quotes, None, true, false).unwrap();

    let err = engine.run(None, None, None, false).unwrap_err();
    assert!(err.to_string().contains("not sorted"), "got: {err}");
}

#[rstest]
fn test_run_rejects_depth_book_without_book_data(crypto_perpetual_ethusdt: CryptoPerpetual) {
    // Build an engine with a venue requesting L2 depth, then add only quote
    // ticks (non-book data) for an instrument. `run` must refuse to start.
    let mut engine = BacktestEngine::new(BacktestEngineConfig::default()).unwrap();
    let venue_config = SimulatedVenueConfig::builder()
        .venue(Venue::from("BINANCE"))
        .oms_type(OmsType::Netting)
        .account_type(AccountType::Margin)
        .book_type(BookType::L2_MBP)
        .starting_balances(vec![Money::from("1_000_000 USDT")])
        .build();
    engine.add_venue(venue_config).unwrap();

    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    let quotes = vec![quote(instrument_id, "1000.00", "1000.10", 1_000_000_000)];
    engine.add_data(quotes, None, true, true).unwrap();

    let err = engine.run(None, None, None, false).unwrap_err();
    assert!(
        err.to_string().contains("No order book data found"),
        "got: {err}",
    );
}

#[rstest]
fn test_add_data_rejects_bar_internal_aggregation(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    let bars = vec![bar_with_aggregation(
        instrument_id,
        AggregationSource::Internal,
        1_000_000_000,
    )];
    let err = engine.add_data(bars, None, true, true).unwrap_err();
    assert!(
        err.to_string()
            .contains("aggregation_source must be External"),
        "got: {err}",
    );
}

#[rstest]
fn test_run_with_depth_venue_and_book_data_succeeds(crypto_perpetual_ethusdt: CryptoPerpetual) {
    // Mirror of test_run_rejects_depth_book_without_book_data with deltas added
    // so the venue's L2 book requirement is satisfied. Catches an inverted
    // depth-vs-data check that the negative test alone would not detect.
    let mut engine = BacktestEngine::new(BacktestEngineConfig::default()).unwrap();
    let venue_config = SimulatedVenueConfig::builder()
        .venue(Venue::from("BINANCE"))
        .oms_type(OmsType::Netting)
        .account_type(AccountType::Margin)
        .book_type(BookType::L2_MBP)
        .starting_balances(vec![Money::from("1_000_000 USDT")])
        .build();
    engine.add_venue(venue_config).unwrap();

    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    let deltas = vec![
        bid_delta(instrument_id, "1000.00", 1, 1_000_000_000),
        bid_delta(instrument_id, "1000.50", 2, 2_000_000_000),
    ];
    engine.add_data(deltas, None, true, true).unwrap();

    engine.run(None, None, None, false).unwrap();
    assert_eq!(engine.get_result().iterations, 2);
}

#[rstest]
fn test_run_depth_check_fires_on_validate_false_path(crypto_perpetual_ethusdt: CryptoPerpetual) {
    // The depth-vs-data check at run time must fire even when add_data is
    // called with validate=false (e.g. the catalog-loading path in node.rs).
    // This locks in the round-1 fix that hoisted has_data/has_book_data
    // bookkeeping out of the validate branch.
    let mut engine = BacktestEngine::new(BacktestEngineConfig::default()).unwrap();
    let venue_config = SimulatedVenueConfig::builder()
        .venue(Venue::from("BINANCE"))
        .oms_type(OmsType::Netting)
        .account_type(AccountType::Margin)
        .book_type(BookType::L2_MBP)
        .starting_balances(vec![Money::from("1_000_000 USDT")])
        .build();
    engine.add_venue(venue_config).unwrap();

    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    let quotes = vec![quote(instrument_id, "1000.00", "1000.10", 1_000_000_000)];
    engine.add_data(quotes, None, false, true).unwrap();

    let err = engine.run(None, None, None, false).unwrap_err();
    assert!(
        err.to_string().contains("No order book data found"),
        "got: {err}",
    );
}

#[rstest]
fn test_add_data_tracks_global_ts_bounds_when_unsorted(crypto_perpetual_ethusdt: CryptoPerpetual) {
    // Two add_data calls with sort=false where neither first nor last element
    // is the global min/max. The engine must still pick the correct global
    // start/end as defaults so run() processes the full range.
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    let batch1 = vec![
        quote(instrument_id, "1000.00", "1000.10", 300),
        quote(instrument_id, "1000.50", "1000.60", 100),
        quote(instrument_id, "1001.00", "1001.10", 200),
    ];
    engine.add_data(batch1, None, true, false).unwrap();

    let batch2 = vec![
        quote(instrument_id, "1002.00", "1002.10", 400),
        quote(instrument_id, "1003.00", "1003.10", 50),
    ];
    engine.add_data(batch2, None, true, false).unwrap();

    engine.sort_data();
    engine.run(None, None, None, false).unwrap();

    assert_eq!(engine.backtest_start(), Some(UnixNanos::from(50)));
    assert_eq!(engine.backtest_end(), Some(UnixNanos::from(400)));
}

#[rstest]
fn test_sort_data_unblocks_run_after_unsorted_add(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    let quotes = vec![quote(instrument_id, "1000.00", "1000.10", 1_000_000_000)];
    engine.add_data(quotes, None, true, false).unwrap();

    // sort_data flips the sorted flag so run no longer rejects.
    engine.sort_data();
    engine.run(None, None, None, false).unwrap();
}

#[rstest]
fn test_clear_data_resets_sorted_flag(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    // Pollute sorted=false, then clear, then add a sorted batch and run.
    engine
        .add_data(
            vec![quote(instrument_id, "1000.00", "1000.10", 1_000_000_000)],
            None,
            true,
            false,
        )
        .unwrap();
    engine.clear_data();
    engine
        .add_data(
            vec![quote(instrument_id, "1000.00", "1000.10", 1_000_000_000)],
            None,
            true,
            true,
        )
        .unwrap();

    engine.run(None, None, None, false).unwrap();
}

#[rstest]
fn test_add_strategies_stops_at_first_error() {
    // Batch should fail when the second strategy duplicates the first's ID,
    // and the first strategy must remain registered (fail-fast semantics).
    let mut engine = BacktestEngine::new(BacktestEngineConfig::default()).unwrap();
    let venue_config = SimulatedVenueConfig::builder()
        .venue(Venue::from("BINANCE"))
        .oms_type(OmsType::Netting)
        .account_type(AccountType::Margin)
        .book_type(BookType::L1_MBP)
        .starting_balances(vec![Money::from("1_000_000 USDT")])
        .build();
    engine.add_venue(venue_config).unwrap();

    let s1 = EmptyStrategy::new();
    let s2 = EmptyStrategy::new(); // identical strategy_id
    let result = engine.add_strategies(vec![s1, s2]);
    assert!(result.is_err());
    assert_eq!(
        engine.kernel().trader.borrow().strategy_count(),
        1,
        "first strategy must remain registered after batch fail-fast",
    );
}

#[rstest]
fn test_run_processes_quote_ticks(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    let quotes = vec![
        quote(instrument_id, "1000.00", "1000.10", 1_000_000_000),
        quote(instrument_id, "1000.50", "1000.60", 2_000_000_000),
        quote(instrument_id, "1001.00", "1001.10", 3_000_000_000),
    ];
    engine.add_data(quotes, None, true, true).unwrap();

    let result = engine.run(None, None, None, false);
    assert!(result.is_ok());

    let bt_result = engine.get_result();
    assert_eq!(bt_result.iterations, 3);

    // Lifecycle getters must populate after a successful run + end.
    assert!(engine.run_id().is_some());
    let bt_start = engine.backtest_start().expect("backtest_start populated");
    let bt_end = engine.backtest_end().expect("backtest_end populated");
    assert!(bt_end >= bt_start);
}

#[rstest]
fn test_get_result_includes_snapshot_position_history(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    let strategy = SnapshotNettingFlip::new(instrument_id, Quantity::from("1.000"));
    engine.add_strategy(strategy).unwrap();

    let quotes = vec![
        quote(instrument_id, "1000.00", "1001.00", 1_000_000_000),
        quote(instrument_id, "1000.00", "1001.00", 2_000_000_000),
        quote(instrument_id, "1000.00", "1001.00", 3_000_000_000),
        quote(instrument_id, "998.00", "999.00", 4_000_000_000),
        quote(instrument_id, "998.00", "999.00", 5_000_000_000),
        quote(instrument_id, "997.00", "998.00", 6_000_000_000),
        quote(instrument_id, "997.00", "998.00", 7_000_000_000),
        quote(instrument_id, "999.00", "1000.00", 8_000_000_000),
        quote(instrument_id, "999.00", "1000.00", 9_000_000_000),
    ];
    engine.add_data(quotes, None, true, true).unwrap();
    engine.run(None, None, None, false).unwrap();

    let cache_rc = engine.kernel().cache();
    let (expected_total, cache_realized_count, snapshots_realized, snapshots_realized_count) = {
        let cache = cache_rc.borrow();
        let positions = cache.positions(None, None, None, None, None);

        let cache_realized: f64 = positions
            .iter()
            .filter_map(|p| p.realized_pnl.as_ref().map(|m| m.as_f64()))
            .sum();
        let cache_realized_count = positions
            .iter()
            .filter(|p| p.realized_pnl.is_some())
            .count() as f64;

        let snapshot_positions: Vec<Position> = positions
            .iter()
            .flat_map(|p| cache.position_snapshots(Some(&p.id), None))
            .collect();
        let snapshots_realized: f64 = snapshot_positions
            .iter()
            .filter_map(|p| p.realized_pnl.as_ref().map(|m| m.as_f64()))
            .sum();
        let snapshots_realized_count = snapshot_positions
            .iter()
            .filter(|p| p.realized_pnl.is_some())
            .count() as f64;

        assert!(
            snapshots_realized.abs() > 0.0,
            "expected non-zero snapshot realized history"
        );

        (
            cache_realized + snapshots_realized,
            cache_realized_count,
            snapshots_realized,
            snapshots_realized_count,
        )
    };

    let expected_expectancy = expected_total / (cache_realized_count + snapshots_realized_count);

    let bt_result = engine.get_result();
    let expectancy = bt_result
        .stats_pnls
        .values()
        .find_map(|pnls| pnls.get("Expectancy").copied())
        .expect("Expectancy stat must exist");

    assert!(
        (expectancy - expected_expectancy).abs() < 1e-9,
        "expected Expectancy={expected_expectancy} to include snapshot history {snapshots_realized}, found {expectancy}"
    );
}

#[rstest]
fn test_run_with_strategy(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    engine.add_strategy(EmptyStrategy::new()).unwrap();

    let quotes = vec![
        quote(instrument_id, "1000.00", "1000.10", 1_000_000_000),
        quote(instrument_id, "1000.50", "1000.60", 2_000_000_000),
        quote(instrument_id, "1001.00", "1001.10", 3_000_000_000),
    ];
    engine.add_data(quotes, None, true, true).unwrap();

    let result = engine.run(None, None, None, false);
    assert!(result.is_ok());

    let bt_result = engine.get_result();
    assert_eq!(bt_result.iterations, 3);
    assert_eq!(bt_result.total_orders, 0);
}

#[rstest]
fn test_run_with_start_end_bounds(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    let base: u64 = 1_000_000_000_000_000_000; // 1e18 ns
    let quotes = vec![
        quote(instrument_id, "1000.00", "1000.10", base),
        quote(instrument_id, "1000.50", "1000.60", base + 1_000_000_000),
        quote(instrument_id, "1001.00", "1001.10", base + 2_000_000_000),
        quote(instrument_id, "1001.50", "1001.60", base + 3_000_000_000),
    ];
    engine.add_data(quotes, None, true, true).unwrap();

    // Only process quotes at t=base+1s and t=base+2s (skip first and last)
    let result = engine.run(
        Some((base + 1_000_000_000).into()),
        Some((base + 2_000_000_000).into()),
        None,
        false,
    );
    assert!(result.is_ok());

    let bt_result = engine.get_result();
    assert_eq!(bt_result.iterations, 2);
}

#[rstest]
fn test_reset_preserves_data(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    let quotes = vec![
        quote(instrument_id, "1000.00", "1000.10", 1_000_000_000),
        quote(instrument_id, "1000.50", "1000.60", 2_000_000_000),
    ];
    engine.add_data(quotes, None, true, true).unwrap();

    // First run
    engine.run(None, None, None, false).unwrap();
    let result1 = engine.get_result();
    assert_eq!(result1.iterations, 2);

    // Reset and run again — data should persist
    engine.reset();

    engine.add_strategy(EmptyStrategy::new()).unwrap();
    engine.run(None, None, None, false).unwrap();
    let result2 = engine.get_result();
    assert_eq!(result2.iterations, 2);
}

#[rstest]
fn test_clear_data(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    let quotes = vec![quote(instrument_id, "1000.00", "1000.10", 1_000_000_000)];
    engine.add_data(quotes, None, true, true).unwrap();
    engine.clear_data();

    engine.run(None, None, None, false).unwrap();
    let result = engine.get_result();
    assert_eq!(result.iterations, 0);
}

#[rstest]
fn test_ema_cross_strategy_generates_orders(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    engine
        .add_strategy(EmaCross::new(
            instrument_id,
            Quantity::from("0.100"),
            10,
            20,
        ))
        .unwrap();

    // Generate price series with clear trend changes to trigger EMA crossovers.
    // Phase 1: Flat at 1000 (25 ticks) — both EMAs initialize and converge
    // Phase 2: Ramp up to 1200 (40 ticks) — fast EMA crosses above slow → BUY
    // Phase 3: Ramp down to 800 (80 ticks) — fast EMA crosses below slow → SELL
    // Phase 4: Ramp up to 1000 (40 ticks) — fast crosses above again → BUY
    let spread = 0.10;
    let mut quotes = Vec::new();
    let base_ts: u64 = 1_000_000_000;
    let interval: u64 = 1_000_000_000;
    let mut tick: u64 = 0;

    let add_quote = |quotes: &mut Vec<Data>, mid: f64, tick: &mut u64| {
        let bid = format!("{:.2}", mid - spread / 2.0);
        let ask = format!("{:.2}", mid + spread / 2.0);
        quotes.push(quote(instrument_id, &bid, &ask, base_ts + *tick * interval));
        *tick += 1;
    };

    // Phase 1: Flat
    for _ in 0..25 {
        add_quote(&mut quotes, 1000.0, &mut tick);
    }

    // Phase 2: Ramp up
    for i in 0..40 {
        add_quote(&mut quotes, 1000.0 + (i as f64 * 5.0), &mut tick);
    }

    // Phase 3: Ramp down
    for i in 0..80 {
        add_quote(&mut quotes, 1195.0 - (i as f64 * 5.0), &mut tick);
    }

    // Phase 4: Ramp up
    for i in 0..40 {
        add_quote(&mut quotes, 800.0 + (i as f64 * 5.0), &mut tick);
    }

    let total_quotes = quotes.len();
    engine.add_data(quotes, None, true, true).unwrap();

    engine.run(None, None, None, false).unwrap();

    let bt_result = engine.get_result();
    assert_eq!(bt_result.iterations, total_quotes);
    assert!(
        bt_result.total_orders >= 2,
        "Expected at least 2 orders (buy + sell crossovers), was {}",
        bt_result.total_orders
    );
    assert!(
        bt_result.total_positions > 0,
        "Expected positions from filled orders"
    );
}

struct ShutdownOnTick {
    core: StrategyCore,
    instrument_id: InstrumentId,
    shutdown_after: usize,
    tick_count: usize,
}

impl ShutdownOnTick {
    fn new(instrument_id: InstrumentId, shutdown_after: usize) -> Self {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("SHUTDOWN-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        Self {
            core: StrategyCore::new(config),
            instrument_id,
            shutdown_after,
            tick_count: 0,
        }
    }
}

nautilus_strategy!(ShutdownOnTick);

impl Debug for ShutdownOnTick {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ShutdownOnTick)).finish()
    }
}

impl DataActor for ShutdownOnTick {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.subscribe_quotes(self.instrument_id, None, None);
        Ok(())
    }

    fn on_quote(&mut self, _quote: &QuoteTick) -> anyhow::Result<()> {
        self.tick_count += 1;
        if self.tick_count == self.shutdown_after {
            self.shutdown_system(Some("shutdown on tick".to_string()));
        }
        Ok(())
    }
}

struct ShutdownBeforeFutureTimer {
    core: StrategyCore,
    instrument_id: InstrumentId,
    shutdown_after: usize,
    tick_count: usize,
    timer_ts: u64,
    timer_count: std::rc::Rc<Cell<u32>>,
}

impl ShutdownBeforeFutureTimer {
    fn new(
        instrument_id: InstrumentId,
        shutdown_after: usize,
        timer_ts: u64,
        timer_count: std::rc::Rc<Cell<u32>>,
    ) -> Self {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("SHUTDOWN-TIMER-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        Self {
            core: StrategyCore::new(config),
            instrument_id,
            shutdown_after,
            tick_count: 0,
            timer_ts,
            timer_count,
        }
    }
}

nautilus_strategy!(ShutdownBeforeFutureTimer);

impl Debug for ShutdownBeforeFutureTimer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ShutdownBeforeFutureTimer))
            .finish()
    }
}

impl DataActor for ShutdownBeforeFutureTimer {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.subscribe_quotes(self.instrument_id, None, None);
        let timer_ts = self.timer_ts;
        self.clock()
            .set_time_alert_ns("future_timer", timer_ts.into(), None, None)?;
        Ok(())
    }

    fn on_quote(&mut self, _quote: &QuoteTick) -> anyhow::Result<()> {
        self.tick_count += 1;
        if self.tick_count == self.shutdown_after {
            self.shutdown_system(Some("shutdown before future timer".to_string()));
        }
        Ok(())
    }

    fn on_time_event(&mut self, _event: &TimeEvent) -> anyhow::Result<()> {
        self.timer_count.set(self.timer_count.get() + 1);
        Ok(())
    }
}

#[rstest]
fn test_non_streaming_shutdown_does_not_fire_future_timers(
    crypto_perpetual_ethusdt: CryptoPerpetual,
) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    let timer_count = std::rc::Rc::new(Cell::new(0));
    engine
        .add_strategy(ShutdownBeforeFutureTimer::new(
            instrument_id,
            2,
            2_500_000_000,
            timer_count.clone(),
        ))
        .unwrap();

    let batch = vec![
        quote(instrument_id, "1000.00", "1000.10", 1_000_000_000),
        quote(instrument_id, "1001.00", "1001.10", 2_000_000_000),
        quote(instrument_id, "1002.00", "1002.10", 3_000_000_000),
    ];
    engine.add_data(batch, None, true, true).unwrap();

    engine.run(None, None, None, false).unwrap();

    assert_eq!(
        engine.get_result().iterations,
        2,
        "Run must stop on the shutdown tick",
    );
    assert_eq!(
        timer_count.get(),
        0,
        "Future timer must not fire after shutdown in a non-streaming run",
    );
}

struct ShutdownFromTimer {
    core: StrategyCore,
    instrument_id: InstrumentId,
    shutdown_ts: u64,
    later_ts: u64,
    shutdown_fired: std::rc::Rc<Cell<u32>>,
    later_fired: std::rc::Rc<Cell<u32>>,
    quote_count: std::rc::Rc<Cell<u32>>,
}

impl ShutdownFromTimer {
    fn new(
        instrument_id: InstrumentId,
        shutdown_ts: u64,
        later_ts: u64,
        shutdown_fired: std::rc::Rc<Cell<u32>>,
        later_fired: std::rc::Rc<Cell<u32>>,
        quote_count: std::rc::Rc<Cell<u32>>,
    ) -> Self {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("SHUTDOWN-FROM-TIMER-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        Self {
            core: StrategyCore::new(config),
            instrument_id,
            shutdown_ts,
            later_ts,
            shutdown_fired,
            later_fired,
            quote_count,
        }
    }
}

nautilus_strategy!(ShutdownFromTimer);

impl Debug for ShutdownFromTimer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ShutdownFromTimer)).finish()
    }
}

impl DataActor for ShutdownFromTimer {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.subscribe_quotes(self.instrument_id, None, None);
        let shutdown_ts = self.shutdown_ts;
        let later_ts = self.later_ts;
        self.clock()
            .set_time_alert_ns("shutdown_timer", shutdown_ts.into(), None, None)?;
        self.clock()
            .set_time_alert_ns("later_timer", later_ts.into(), None, None)?;
        Ok(())
    }

    fn on_quote(&mut self, _quote: &QuoteTick) -> anyhow::Result<()> {
        self.quote_count.set(self.quote_count.get() + 1);
        Ok(())
    }

    fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
        if event.name.as_str() == "shutdown_timer" {
            self.shutdown_fired.set(self.shutdown_fired.get() + 1);
            self.shutdown_system(Some("shutdown from timer".to_string()));
        } else if event.name.as_str() == "later_timer" {
            self.later_fired.set(self.later_fired.get() + 1);
        }
        Ok(())
    }
}

struct ShutdownAndScheduleNewAlert {
    core: StrategyCore,
    instrument_id: InstrumentId,
    shutdown_ts: u64,
    new_alert_ts: u64,
    shutdown_fired: std::rc::Rc<Cell<u32>>,
    new_alert_fired: std::rc::Rc<Cell<u32>>,
}

impl ShutdownAndScheduleNewAlert {
    fn new(
        instrument_id: InstrumentId,
        shutdown_ts: u64,
        new_alert_ts: u64,
        shutdown_fired: std::rc::Rc<Cell<u32>>,
        new_alert_fired: std::rc::Rc<Cell<u32>>,
    ) -> Self {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("SHUTDOWN-RESCHEDULE-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        Self {
            core: StrategyCore::new(config),
            instrument_id,
            shutdown_ts,
            new_alert_ts,
            shutdown_fired,
            new_alert_fired,
        }
    }
}

nautilus_strategy!(ShutdownAndScheduleNewAlert);

impl Debug for ShutdownAndScheduleNewAlert {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ShutdownAndScheduleNewAlert))
            .finish()
    }
}

impl DataActor for ShutdownAndScheduleNewAlert {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.subscribe_quotes(self.instrument_id, None, None);
        let shutdown_ts = self.shutdown_ts;
        self.clock()
            .set_time_alert_ns("shutdown_timer", shutdown_ts.into(), None, None)?;
        Ok(())
    }

    fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
        if event.name.as_str() == "shutdown_timer" {
            self.shutdown_fired.set(self.shutdown_fired.get() + 1);
            let new_alert_ts = self.new_alert_ts;
            self.clock().set_time_alert_ns(
                "post_shutdown_alert",
                new_alert_ts.into(),
                None,
                None,
            )?;
            self.shutdown_system(Some("shutdown and reschedule".to_string()));
        } else if event.name.as_str() == "post_shutdown_alert" {
            self.new_alert_fired.set(self.new_alert_fired.get() + 1);
        }
        Ok(())
    }
}

#[rstest]
fn test_shutdown_handler_scheduling_new_alert_does_not_fire_it(
    crypto_perpetual_ethusdt: CryptoPerpetual,
) {
    // Alerts scheduled by a shutdown handler must not fire on later flushes
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    let shutdown_fired = std::rc::Rc::new(Cell::new(0));
    let new_alert_fired = std::rc::Rc::new(Cell::new(0));
    engine
        .add_strategy(ShutdownAndScheduleNewAlert::new(
            instrument_id,
            2_500_000_000,
            2_600_000_000,
            shutdown_fired.clone(),
            new_alert_fired.clone(),
        ))
        .unwrap();

    let batch = vec![
        quote(instrument_id, "1000.00", "1000.10", 1_000_000_000),
        quote(instrument_id, "1001.00", "1001.10", 2_000_000_000),
        quote(instrument_id, "1002.00", "1002.10", 3_000_000_000),
    ];
    engine.add_data(batch, None, true, true).unwrap();

    engine.run(None, None, None, false).unwrap();

    assert_eq!(
        shutdown_fired.get(),
        1,
        "Shutdown timer must fire once before requesting shutdown",
    );
    assert_eq!(
        new_alert_fired.get(),
        0,
        "Alert scheduled by the shutdown handler must not fire after the stop",
    );
}

#[rstest]
fn test_shutdown_from_timer_during_flush_does_not_fire_later_timers(
    crypto_perpetual_ethusdt: CryptoPerpetual,
) {
    // A timer-triggered shutdown must drop later alerts queued for the same flush
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    let shutdown_fired = std::rc::Rc::new(Cell::new(0));
    let later_fired = std::rc::Rc::new(Cell::new(0));
    let quote_count = std::rc::Rc::new(Cell::new(0));
    engine
        .add_strategy(ShutdownFromTimer::new(
            instrument_id,
            2_500_000_000,
            2_800_000_000,
            shutdown_fired.clone(),
            later_fired.clone(),
            quote_count.clone(),
        ))
        .unwrap();

    let batch = vec![
        quote(instrument_id, "1000.00", "1000.10", 1_000_000_000),
        quote(instrument_id, "1001.00", "1001.10", 2_000_000_000),
        quote(instrument_id, "1002.00", "1002.10", 3_000_000_000),
    ];
    engine.add_data(batch, None, true, true).unwrap();

    engine.run(None, None, None, false).unwrap();

    assert_eq!(
        shutdown_fired.get(),
        1,
        "Shutdown timer must fire once before requesting shutdown",
    );
    assert_eq!(
        later_fired.get(),
        0,
        "Later timer must not fire after a timer-initiated shutdown",
    );
    assert_eq!(
        quote_count.get(),
        2,
        "Quote arriving after a timer-initiated shutdown must not be delivered",
    );
    assert_eq!(
        engine.kernel().clock.borrow().timestamp_ns().as_u64(),
        2_500_000_000,
        "Engine clock must anchor at the shutdown timer ts, not the skipped data ts",
    );
}

#[rstest]
fn test_streaming_shutdown_finalizes_engine(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();
    engine
        .add_strategy(ShutdownOnTick::new(instrument_id, 2))
        .unwrap();

    let batch = vec![
        quote(instrument_id, "1000.00", "1000.10", 1_000_000_000),
        quote(instrument_id, "1001.00", "1001.10", 2_000_000_000),
        quote(instrument_id, "1002.00", "1002.10", 3_000_000_000),
        quote(instrument_id, "1003.00", "1003.10", 4_000_000_000),
    ];
    engine.add_data(batch, None, true, true).unwrap();

    engine.run(None, None, None, true).unwrap();

    let result = engine.get_result();
    assert_eq!(
        result.iterations, 2,
        "Run must stop after the shutdown tick"
    );
    assert!(
        !engine.kernel().trader.borrow().is_running(),
        "Trader must be stopped after streaming shutdown finalization",
    );
}

#[rstest]
fn test_streaming_shutdown_on_last_tick_finalizes_engine(
    crypto_perpetual_ethusdt: CryptoPerpetual,
) {
    // Regression: shutdown published on the last quote leaves the loop via
    // streaming data-exhaustion rather than the top-of-loop force_stop check.
    // The finalize branch in run() must still observe the shutdown flag.
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();
    engine
        .add_strategy(ShutdownOnTick::new(instrument_id, 3))
        .unwrap();

    let batch = vec![
        quote(instrument_id, "1000.00", "1000.10", 1_000_000_000),
        quote(instrument_id, "1001.00", "1001.10", 2_000_000_000),
        quote(instrument_id, "1002.00", "1002.10", 3_000_000_000),
    ];
    engine.add_data(batch, None, true, true).unwrap();

    engine.run(None, None, None, true).unwrap();

    assert!(
        !engine.kernel().trader.borrow().is_running(),
        "Trader must be stopped when shutdown fires on the last streaming tick",
    );
}

#[rstest]
fn test_streaming_mode_processes_data_in_batches(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();
    engine.add_strategy(EmptyStrategy::new()).unwrap();

    // Batch 1: first 3 quotes
    let batch1 = vec![
        quote(instrument_id, "1000.00", "1000.10", 1_000_000_000),
        quote(instrument_id, "1001.00", "1001.10", 2_000_000_000),
        quote(instrument_id, "1002.00", "1002.10", 3_000_000_000),
    ];
    engine.add_data(batch1, None, true, true).unwrap();
    engine.run(None, None, None, true).unwrap(); // streaming=true

    let result1 = engine.get_result();
    assert_eq!(result1.iterations, 3);

    // Batch 2: next 2 quotes, clear old data first
    engine.clear_data();
    let batch2 = vec![
        quote(instrument_id, "1003.00", "1003.10", 4_000_000_000),
        quote(instrument_id, "1004.00", "1004.10", 5_000_000_000),
    ];
    engine.add_data(batch2, None, true, true).unwrap();
    engine.run(None, None, None, false).unwrap(); // streaming=false, finalizes

    let result2 = engine.get_result();
    assert_eq!(result2.iterations, 5); // Total across both batches
}

#[rstest]
fn test_multiple_add_data_batches_merged(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    // Add data in two separate batches (the P1 fix scenario)
    let batch1 = vec![
        quote(instrument_id, "1000.00", "1000.10", 1_000_000_000),
        quote(instrument_id, "1002.00", "1002.10", 3_000_000_000),
    ];
    let batch2 = vec![
        quote(instrument_id, "1001.00", "1001.10", 2_000_000_000),
        quote(instrument_id, "1003.00", "1003.10", 4_000_000_000),
    ];
    engine.add_data(batch1, None, true, true).unwrap();
    engine.add_data(batch2, None, true, true).unwrap();

    engine.run(None, None, None, false).unwrap();

    let bt_result = engine.get_result();
    assert_eq!(
        bt_result.iterations, 4,
        "All 4 quotes from both batches should be processed"
    );
}

#[rstest]
fn test_multi_venue_data_routing(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let config = BacktestEngineConfig::default();
    let mut engine = BacktestEngine::new(config).unwrap();

    engine
        .add_venue(
            SimulatedVenueConfig::builder()
                .venue(Venue::from("BINANCE"))
                .oms_type(OmsType::Netting)
                .account_type(AccountType::Margin)
                .book_type(BookType::L1_MBP)
                .starting_balances(vec![Money::from("1_000_000 USDT")])
                .build(),
        )
        .unwrap();

    engine
        .add_venue(
            SimulatedVenueConfig::builder()
                .venue(Venue::from("BITMEX"))
                .oms_type(OmsType::Netting)
                .account_type(AccountType::Margin)
                .book_type(BookType::L1_MBP)
                .starting_balances(vec![Money::from("1_000_000 USD")])
                .build(),
        )
        .unwrap();

    let eth = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let eth_id = eth.id();
    engine.add_instrument(&eth).unwrap();

    let btc = InstrumentAny::CryptoPerpetual(nautilus_model::instruments::stubs::xbtusd_bitmex());
    let btc_id = btc.id();
    engine.add_instrument(&btc).unwrap();

    // Interleave quotes from both venues (respecting instrument precision)
    // ETHUSDT-PERP.BINANCE: price_prec=2, size_prec=3
    // BTCUSDT.BITMEX: price_prec=1, size_prec=0
    let quotes = vec![
        quote(eth_id, "1000.00", "1000.10", 1_000_000_000),
        quote_with_size(btc_id, "50000.5", "50001.0", "1", 2_000_000_000),
        quote(eth_id, "1001.00", "1001.10", 3_000_000_000),
        quote_with_size(btc_id, "50100.5", "50101.0", "1", 4_000_000_000),
    ];
    engine.add_data(quotes, None, true, true).unwrap();

    engine.run(None, None, None, false).unwrap();

    let bt_result = engine.get_result();
    assert_eq!(
        bt_result.iterations, 4,
        "All quotes from both venues should be processed"
    );
}

#[rstest]
fn test_strategy_receives_only_subscribed_quotes(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    // Use EMA cross with fast periods so it triggers quickly
    engine
        .add_strategy(EmaCross::new(instrument_id, Quantity::from("0.100"), 3, 5))
        .unwrap();

    // 10 quotes ramping up then 10 down — with 3/5 periods, should trigger quickly
    let mut quotes = Vec::new();
    let base_ts: u64 = 1_000_000_000;
    let interval: u64 = 1_000_000_000;

    // Ramp up
    for i in 0..10u64 {
        let mid = 1000.0 + (i as f64 * 10.0);
        quotes.push(quote(
            instrument_id,
            &format!("{:.2}", mid - 0.05),
            &format!("{:.2}", mid + 0.05),
            base_ts + i * interval,
        ));
    }

    // Ramp down
    for i in 0..10u64 {
        let mid = 1090.0 - (i as f64 * 10.0);
        quotes.push(quote(
            instrument_id,
            &format!("{:.2}", mid - 0.05),
            &format!("{:.2}", mid + 0.05),
            base_ts + (10 + i) * interval,
        ));
    }

    engine.add_data(quotes, None, true, true).unwrap();
    engine.run(None, None, None, false).unwrap();

    let bt_result = engine.get_result();
    assert_eq!(bt_result.iterations, 20);
    assert!(
        bt_result.total_orders >= 1,
        "Expected at least 1 order from EMA crossover, was {}",
        bt_result.total_orders
    );
}

#[rstest]
fn test_reset_run_produces_same_results(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    let quotes = vec![
        quote(instrument_id, "1000.00", "1000.10", 1_000_000_000),
        quote(instrument_id, "1001.00", "1001.10", 2_000_000_000),
        quote(instrument_id, "1002.00", "1002.10", 3_000_000_000),
    ];
    engine.add_data(quotes, None, true, true).unwrap();

    // First run
    engine.run(None, None, None, false).unwrap();
    let result1_iterations = engine.get_result().iterations;
    let result1_orders = engine.get_result().total_orders;

    // Reset and run again with same data
    engine.reset();
    engine.run(None, None, None, false).unwrap();
    let result2_iterations = engine.get_result().iterations;
    let result2_orders = engine.get_result().total_orders;

    assert_eq!(result1_iterations, result2_iterations);
    assert_eq!(result1_orders, result2_orders);
    assert_eq!(result1_iterations, 3);
}

#[rstest]
fn test_start_boundary_skips_earlier_data(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    let quotes = vec![
        quote(instrument_id, "1000.00", "1000.10", 1_000_000_000),
        quote(instrument_id, "1001.00", "1001.10", 2_000_000_000),
        quote(instrument_id, "1002.00", "1002.10", 3_000_000_000),
        quote(instrument_id, "1003.00", "1003.10", 4_000_000_000),
        quote(instrument_id, "1004.00", "1004.10", 5_000_000_000),
    ];
    engine.add_data(quotes, None, true, true).unwrap();

    // Start at t=3, should skip first 2 quotes
    engine
        .run(Some(3_000_000_000u64.into()), None, None, false)
        .unwrap();

    let bt_result = engine.get_result();
    assert_eq!(
        bt_result.iterations, 3,
        "Should process only quotes at t=3,4,5"
    );
}

#[rstest]
fn test_end_boundary_stops_before_later_data(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    let quotes = vec![
        quote(instrument_id, "1000.00", "1000.10", 1_000_000_000),
        quote(instrument_id, "1001.00", "1001.10", 2_000_000_000),
        quote(instrument_id, "1002.00", "1002.10", 3_000_000_000),
        quote(instrument_id, "1003.00", "1003.10", 4_000_000_000),
        quote(instrument_id, "1004.00", "1004.10", 5_000_000_000),
    ];
    engine.add_data(quotes, None, true, true).unwrap();

    // End at t=3, should process only first 3
    engine
        .run(None, Some(3_000_000_000u64.into()), None, false)
        .unwrap();

    let bt_result = engine.get_result();
    assert_eq!(
        bt_result.iterations, 3,
        "Should process only quotes at t=1,2,3"
    );
}

#[rstest]
fn test_ema_cross_with_batched_data(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    engine
        .add_strategy(EmaCross::new(instrument_id, Quantity::from("0.100"), 3, 5))
        .unwrap();

    let base_ts: u64 = 1_000_000_000;
    let interval: u64 = 1_000_000_000;

    // Add flat data in one batch
    let flat: Vec<Data> = (0..10u64)
        .map(|i| quote(instrument_id, "1000.00", "1000.10", base_ts + i * interval))
        .collect();
    engine.add_data(flat, None, true, true).unwrap();

    // Add ramp-up in a separate batch
    let ramp_up: Vec<Data> = (0..15u64)
        .map(|i| {
            let mid = 1000.0 + (i as f64 * 10.0);
            quote(
                instrument_id,
                &format!("{:.2}", mid - 0.05),
                &format!("{:.2}", mid + 0.05),
                base_ts + (10 + i) * interval,
            )
        })
        .collect();
    engine.add_data(ramp_up, None, true, true).unwrap();

    engine.run(None, None, None, false).unwrap();

    let bt_result = engine.get_result();
    assert_eq!(bt_result.iterations, 25);
    assert!(
        bt_result.total_orders >= 1,
        "Expected at least 1 order from batched data crossover, was {}",
        bt_result.total_orders
    );
}

// Strategy that submits a stop-loss when its market order fills,
// exercising the engine's settle loop for cascading commands.
struct CascadingStopStrategy {
    core: StrategyCore,
    instrument_id: InstrumentId,
    trade_size: Quantity,
    entry_submitted: Cell<bool>,
    stop_submitted: Cell<bool>,
}

impl CascadingStopStrategy {
    fn new(instrument_id: InstrumentId, trade_size: Quantity) -> Self {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("CASCADE-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        Self {
            core: StrategyCore::new(config),
            instrument_id,
            trade_size,
            entry_submitted: Cell::new(false),
            stop_submitted: Cell::new(false),
        }
    }
}

nautilus_strategy!(CascadingStopStrategy);

impl Debug for CascadingStopStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(CascadingStopStrategy)).finish()
    }
}

impl DataActor for CascadingStopStrategy {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.subscribe_quotes(self.instrument_id, None, None);
        Ok(())
    }

    fn on_quote(&mut self, _quote: &QuoteTick) -> anyhow::Result<()> {
        if !self.entry_submitted.get() {
            self.entry_submitted.set(true);
            let order = self.core.order_factory().market(
                self.instrument_id,
                OrderSide::Buy,
                self.trade_size,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            );
            self.submit_order(order, None, None, None)?;
        }
        Ok(())
    }

    fn on_order_filled(&mut self, _event: &OrderFilled) -> anyhow::Result<()> {
        // Submit stop-loss in response to fill (cascading command)
        if !self.stop_submitted.get() {
            self.stop_submitted.set(true);
            let order = self.core.order_factory().stop_market(
                self.instrument_id,
                OrderSide::Sell,
                self.trade_size,
                Price::from("900.00"),
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
            );
            self.submit_order(order, None, None, None)?;
        }
        Ok(())
    }
}

#[rstest]
fn test_cascading_stop_loss_on_fill_settled_same_tick(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    let strategy = CascadingStopStrategy::new(instrument_id, Quantity::from("1.000"));
    engine.add_strategy(strategy).unwrap();

    let quotes = vec![
        quote(instrument_id, "1000.00", "1001.00", 1_000_000_000),
        quote(instrument_id, "1000.50", "1001.50", 2_000_000_000),
    ];
    engine.add_data(quotes, None, true, true).unwrap();

    engine.run(None, None, None, false).unwrap();

    let bt_result = engine.get_result();

    // Entry market order + cascading stop-loss = 2 orders
    assert_eq!(
        bt_result.total_orders, 2,
        "Expected 2 orders (entry + cascading stop-loss), was {}",
        bt_result.total_orders
    );
}

// Strategy that sets two timers at the same timestamp, each submitting
// a market order. Tests that all same-timestamp timer commands are settled.
struct DualTimerStrategy {
    core: StrategyCore,
    instrument_id: InstrumentId,
    trade_size: Quantity,
    timer_ts: u64,
    timer_count: AtomicU32,
}

impl DualTimerStrategy {
    fn new(instrument_id: InstrumentId, trade_size: Quantity, timer_ts: u64) -> Self {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("DUAL-TIMER-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        Self {
            core: StrategyCore::new(config),
            instrument_id,
            trade_size,
            timer_ts,
            timer_count: AtomicU32::new(0),
        }
    }
}

nautilus_strategy!(DualTimerStrategy);

impl Debug for DualTimerStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(DualTimerStrategy)).finish()
    }
}

impl DataActor for DualTimerStrategy {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.subscribe_quotes(self.instrument_id, None, None);
        let timer_ns = self.timer_ts.into();
        self.clock()
            .set_time_alert_ns("timer_a", timer_ns, None, None)?;
        self.clock()
            .set_time_alert_ns("timer_b", timer_ns, None, None)?;
        Ok(())
    }

    fn on_time_event(&mut self, _event: &TimeEvent) -> anyhow::Result<()> {
        let count = self.timer_count.fetch_add(1, Ordering::Relaxed);
        let side = if count == 0 {
            OrderSide::Buy
        } else {
            OrderSide::Sell
        };
        let order = self.core.order_factory().market(
            self.instrument_id,
            side,
            self.trade_size,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        self.submit_order(order, None, None, None)?;
        Ok(())
    }
}

#[rstest]
fn test_all_same_timestamp_timer_commands_settled(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    // Timer fires at 30s, between data points at 0s and 60s
    let timer_ts: u64 = 30_000_000_000;
    let strategy = DualTimerStrategy::new(instrument_id, Quantity::from("1.000"), timer_ts);
    engine.add_strategy(strategy).unwrap();

    let quotes = vec![
        quote(instrument_id, "1000.00", "1001.00", 0),
        quote(instrument_id, "1000.50", "1001.50", 60_000_000_000),
    ];
    engine.add_data(quotes, None, true, true).unwrap();

    engine.run(None, None, None, false).unwrap();

    let bt_result = engine.get_result();
    assert_eq!(
        bt_result.total_orders, 2,
        "Expected 2 orders from dual timer callbacks, was {}",
        bt_result.total_orders
    );
}

struct BarSubscriberStrategy {
    core: StrategyCore,
    instrument_id: InstrumentId,
    bar_type: BarType,
}

impl BarSubscriberStrategy {
    fn new(instrument_id: InstrumentId, bar_type: BarType) -> Self {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("BAR-SUB-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        Self {
            core: StrategyCore::new(config),
            instrument_id,
            bar_type,
        }
    }
}

nautilus_strategy!(BarSubscriberStrategy);

impl Debug for BarSubscriberStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BarSubscriberStrategy)).finish()
    }
}

impl DataActor for BarSubscriberStrategy {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.subscribe_quotes(self.instrument_id, None, None);
        self.subscribe_bars(self.bar_type, None, None);
        Ok(())
    }
}

#[rstest]
fn test_streaming_no_dummy_bars_past_batch_data(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    let bar_type = BarType::new(
        instrument_id,
        BarSpecification::new(5, BarAggregation::Second, PriceType::Mid),
        AggregationSource::Internal,
    );
    engine
        .add_strategy(BarSubscriberStrategy::new(instrument_id, bar_type))
        .unwrap();

    let batch1: Vec<Data> = (1..=10u64)
        .map(|i| quote(instrument_id, "1000.00", "1000.10", i * 1_000_000_000))
        .collect();
    engine.add_data(batch1, None, true, true).unwrap();

    // Run with end far past data (100s), streaming=true.
    // Without the fix, timers fire from 10s to 100s producing ~18 dummy bars.
    // With the fix, only bars from the actual data period are built.
    let end = Some(UnixNanos::from(100_000_000_000u64));
    engine.run(None, end, None, true).unwrap();

    let cache = engine.kernel().cache.borrow();
    let bars = cache.bars(&bar_type).unwrap_or_default();
    assert!(
        bars.len() <= 2,
        "Expected at most 2 bars from 10s of data with 5s bars, found {}",
        bars.len(),
    );
    drop(cache);

    // Batch 2: continues from where batch 1 left off (20s to 30s).
    // Gap bars (10-20s) fire naturally when time advances to batch 2 data.
    engine.clear_data();
    let batch2: Vec<Data> = (20..=30u64)
        .map(|i| quote(instrument_id, "1001.00", "1001.10", i * 1_000_000_000))
        .collect();
    engine.add_data(batch2, None, true, true).unwrap();
    engine
        .run(None, Some(UnixNanos::from(30_000_000_000u64)), None, false)
        .unwrap();

    // Batch 1 produced ~1 bar, batch 2 adds gap bars (10-20s) + data bars (20-30s)
    let cache = engine.kernel().cache.borrow();
    let bars = cache.bars(&bar_type).unwrap_or_default();
    assert!(
        bars.len() <= 6,
        "Expected at most 6 bars across both batches, found {}",
        bars.len(),
    );
}

#[rstest]
fn test_streaming_end_flushes_tail_timers(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    let bar_type = BarType::new(
        instrument_id,
        BarSpecification::new(5, BarAggregation::Second, PriceType::Mid),
        AggregationSource::Internal,
    );
    engine
        .add_strategy(BarSubscriberStrategy::new(instrument_id, bar_type))
        .unwrap();

    let batch: Vec<Data> = (1..=10u64)
        .map(|i| quote(instrument_id, "1000.00", "1000.10", i * 1_000_000_000))
        .collect();
    engine.add_data(batch, None, true, true).unwrap();

    // Node-style workflow: all batches use streaming=true, finalize with end()
    let end = Some(UnixNanos::from(20_000_000_000u64));
    engine.run(None, end, None, true).unwrap();

    let cache = engine.kernel().cache.borrow();
    let bars_before_end = cache.bars(&bar_type).unwrap_or_default().len();
    assert!(
        bars_before_end <= 2,
        "Expected at most 2 bars before end(), found {bars_before_end}",
    );
    drop(cache);

    // end() should flush tail timers up to end_ns (20s),
    // producing gap bars between 10s and 20s
    engine.end();

    let cache = engine.kernel().cache.borrow();
    let bars_after_end = cache.bars(&bar_type).unwrap_or_default().len();
    assert!(
        bars_after_end > bars_before_end,
        "end() should have flushed tail timers, but bar count unchanged: {bars_after_end}",
    );
    assert!(
        bars_after_end <= 4,
        "Expected at most 4 bars after end() flush to 20s, found {bars_after_end}",
    );
}

#[rstest]
fn test_engine_properties() {
    let config = BacktestEngineConfig::default();
    let engine = BacktestEngine::new(config).unwrap();

    assert_eq!(engine.trader_id().to_string(), "TRADER-001");
    assert!(!engine.instance_id().to_string().is_empty());
    assert_eq!(engine.iteration(), 0);
}

#[rstest]
fn test_list_venues_empty() {
    let engine = BacktestEngine::new(BacktestEngineConfig::default()).unwrap();
    assert!(engine.list_venues().is_empty());
}

#[rstest]
fn test_list_venues_single() {
    let engine = create_engine();
    let venues = engine.list_venues();

    assert_eq!(venues.len(), 1);
    assert_eq!(venues[0], Venue::from("BINANCE"));
}

#[rstest]
fn test_list_venues_multiple() {
    let config = BacktestEngineConfig::default();
    let mut engine = BacktestEngine::new(config).unwrap();

    engine
        .add_venue(
            SimulatedVenueConfig::builder()
                .venue(Venue::from("BINANCE"))
                .oms_type(OmsType::Netting)
                .account_type(AccountType::Margin)
                .book_type(BookType::L1_MBP)
                .starting_balances(vec![Money::from("1_000_000 USDT")])
                .build(),
        )
        .unwrap();

    engine
        .add_venue(
            SimulatedVenueConfig::builder()
                .venue(Venue::from("BITMEX"))
                .oms_type(OmsType::Netting)
                .account_type(AccountType::Margin)
                .book_type(BookType::L1_MBP)
                .starting_balances(vec![Money::from("1_000_000 USD")])
                .build(),
        )
        .unwrap();

    let mut venues = engine.list_venues();
    venues.sort_by_key(|v| v.to_string());
    assert_eq!(venues.len(), 2);
    assert_eq!(venues[0], Venue::from("BINANCE"));
    assert_eq!(venues[1], Venue::from("BITMEX"));
}

#[rstest]
fn test_iteration_advances_with_data(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    assert_eq!(engine.iteration(), 0);

    let quotes = vec![
        quote(instrument_id, "1000.00", "1000.10", 1_000_000_000),
        quote(instrument_id, "1000.50", "1000.60", 2_000_000_000),
        quote(instrument_id, "1001.00", "1001.10", 3_000_000_000),
    ];
    engine.add_data(quotes, None, true, true).unwrap();
    engine.run(None, None, None, false).unwrap();

    assert_eq!(engine.iteration(), 3);
}

#[rstest]
fn test_add_venue_with_queue_position(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let config = BacktestEngineConfig::default();
    let mut engine = BacktestEngine::new(config).unwrap();

    let result = engine.add_venue(
        SimulatedVenueConfig::builder()
            .venue(Venue::from("BINANCE"))
            .oms_type(OmsType::Netting)
            .account_type(AccountType::Margin)
            .book_type(BookType::L1_MBP)
            .starting_balances(vec![Money::from("1_000_000 USDT")])
            .queue_position(true)
            .build(),
    );
    assert!(result.is_ok());

    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    let quotes = vec![quote(instrument_id, "1000.00", "1000.10", 1_000_000_000)];
    engine.add_data(quotes, None, true, true).unwrap();
    engine.run(None, None, None, false).unwrap();
    assert_eq!(engine.get_result().iterations, 1);
}

struct CloseOnStop {
    core: StrategyCore,
    instrument_id: InstrumentId,
    trade_size: Quantity,
    opened: bool,
}

impl CloseOnStop {
    fn new(instrument_id: InstrumentId, trade_size: Quantity) -> Self {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("CLOSE-ON-STOP-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        Self {
            core: StrategyCore::new(config),
            instrument_id,
            trade_size,
            opened: false,
        }
    }
}

nautilus_strategy!(CloseOnStop);

impl Debug for CloseOnStop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(CloseOnStop)).finish()
    }
}

impl DataActor for CloseOnStop {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.subscribe_quotes(self.instrument_id, None, None);
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        self.close_all_positions(self.instrument_id, None, None, None, None, None, None)
    }

    fn on_quote(&mut self, _quote: &QuoteTick) -> anyhow::Result<()> {
        if self.opened {
            return Ok(());
        }
        self.opened = true;
        let order = self.core.order_factory().market(
            self.instrument_id,
            OrderSide::Buy,
            self.trade_size,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        );
        self.submit_order(order, None, None, None)
    }
}

#[rstest]
fn test_close_all_positions_in_on_stop_is_processed(crypto_perpetual_ethusdt: CryptoPerpetual) {
    // Regression test for: closing orders emitted in on_stop() must be dispatched,
    // matched, and filled before the engine returns. Without the fix, the SubmitOrder
    // sits in the trading command queue and the position remains open at run end.
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    engine
        .add_strategy(CloseOnStop::new(instrument_id, Quantity::from("1.000")))
        .unwrap();

    let quotes = vec![
        quote(instrument_id, "1000.00", "1001.00", 1_000_000_000),
        quote(instrument_id, "1000.00", "1001.00", 2_000_000_000),
        quote(instrument_id, "1000.00", "1001.00", 3_000_000_000),
    ];
    engine.add_data(quotes, None, true, true).unwrap();

    engine.run(None, None, None, false).unwrap();

    let cache_rc = engine.kernel().cache();
    let cache = cache_rc.borrow();

    let open = cache.positions_open(None, Some(&instrument_id), None, None, None);
    assert!(
        open.is_empty(),
        "expected no open positions after on_stop close_all_positions, found {}",
        open.len(),
    );

    let closed = cache.positions_closed(None, Some(&instrument_id), None, None, None);
    assert_eq!(
        closed.len(),
        1,
        "expected one closed position after on_stop close_all_positions",
    );
    assert!(
        closed[0].is_closed(),
        "position must report is_closed() after run end",
    );

    let bt_result = engine.get_result();
    assert_eq!(
        bt_result.total_orders, 2,
        "expected opening and closing orders to both be tracked",
    );
}

#[derive(Debug, Default)]
struct ProcessCallTracker {
    total_calls: Cell<u32>,
    last_ts: Cell<Option<UnixNanos>>,
    duplicate_ts_seen: Cell<bool>,
}

#[derive(Debug)]
struct CountingSimulationModule {
    tracker: std::rc::Rc<ProcessCallTracker>,
}

impl SimulationModule for CountingSimulationModule {
    fn pre_process(&self, _data: &Data) {}

    fn process(&self, ts_now: UnixNanos, _ctx: &ExchangeContext) -> Vec<Money> {
        let prev = self.tracker.last_ts.get();
        if prev == Some(ts_now) {
            self.tracker.duplicate_ts_seen.set(true);
        }
        self.tracker.last_ts.set(Some(ts_now));
        self.tracker
            .total_calls
            .set(self.tracker.total_calls.get() + 1);
        Vec::new()
    }

    fn log_diagnostics(&self) {}

    fn reset(&self) {}
}

#[rstest]
fn test_end_does_not_double_run_modules_at_same_timestamp(
    crypto_perpetual_ethusdt: CryptoPerpetual,
) {
    // Regression guard: end() must not invoke run_venue_modules a second time at the
    // final timestamp after run_impl already ran them. SimulationModule::process is
    // documented as once-per-time-step; double-calling can double-apply Money
    // adjustments (FX rollover and user-defined modules).
    let tracker = std::rc::Rc::new(ProcessCallTracker::default());
    let module = CountingSimulationModule {
        tracker: tracker.clone(),
    };

    let config = BacktestEngineConfig::default();
    let mut engine = BacktestEngine::new(config).unwrap();
    let venue_config = SimulatedVenueConfig::builder()
        .venue(Venue::from("BINANCE"))
        .oms_type(OmsType::Netting)
        .account_type(AccountType::Margin)
        .book_type(BookType::L1_MBP)
        .starting_balances(vec![Money::from("1_000_000 USDT")])
        .modules(vec![Box::new(module)])
        .build();
    engine.add_venue(venue_config).unwrap();

    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    engine
        .add_strategy(CloseOnStop::new(instrument_id, Quantity::from("1.000")))
        .unwrap();

    let quotes = vec![
        quote(instrument_id, "1000.00", "1001.00", 1_000_000_000),
        quote(instrument_id, "1000.00", "1001.00", 2_000_000_000),
        quote(instrument_id, "1000.00", "1001.00", 3_000_000_000),
    ];
    engine.add_data(quotes, None, true, true).unwrap();

    engine.run(None, None, None, false).unwrap();

    assert!(
        !tracker.duplicate_ts_seen.get(),
        "SimulationModule::process invoked twice at the same timestamp; \
         end() must preserve the once-per-time-step contract",
    );
    assert!(
        tracker.total_calls.get() > 0,
        "expected the module to run at least once during the backtest",
    );
}

struct CancelOnStop {
    core: StrategyCore,
    instrument_id: InstrumentId,
    trade_size: Quantity,
    limit_price: Price,
    placed: bool,
}

impl CancelOnStop {
    fn new(instrument_id: InstrumentId, trade_size: Quantity, limit_price: Price) -> Self {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("CANCEL-ON-STOP-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        Self {
            core: StrategyCore::new(config),
            instrument_id,
            trade_size,
            limit_price,
            placed: false,
        }
    }
}

nautilus_strategy!(CancelOnStop);

impl Debug for CancelOnStop {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(CancelOnStop)).finish()
    }
}

impl DataActor for CancelOnStop {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.subscribe_quotes(self.instrument_id, None, None);
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        self.cancel_all_orders(self.instrument_id, None, None, None)
    }

    fn on_quote(&mut self, _quote: &QuoteTick) -> anyhow::Result<()> {
        if self.placed {
            return Ok(());
        }
        self.placed = true;
        let order = self.core.order_factory().limit(
            self.instrument_id,
            OrderSide::Buy,
            self.trade_size,
            self.limit_price,
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
        );
        self.submit_order(order, None, None, None)
    }
}

#[rstest]
fn test_cancel_all_orders_in_on_stop_is_processed(crypto_perpetual_ethusdt: CryptoPerpetual) {
    // Sibling regression to the close_all_positions case: cancel commands emitted in
    // on_stop must reach the venue and resolve before end() returns.
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    // Limit price well below market so the order rests rather than fills immediately.
    engine
        .add_strategy(CancelOnStop::new(
            instrument_id,
            Quantity::from("1.000"),
            Price::from("900.00"),
        ))
        .unwrap();

    let quotes = vec![
        quote(instrument_id, "1000.00", "1001.00", 1_000_000_000),
        quote(instrument_id, "1000.00", "1001.00", 2_000_000_000),
        quote(instrument_id, "1000.00", "1001.00", 3_000_000_000),
    ];
    engine.add_data(quotes, None, true, true).unwrap();

    engine.run(None, None, None, false).unwrap();

    let cache_rc = engine.kernel().cache();
    let cache = cache_rc.borrow();

    let open = cache.orders_open(None, Some(&instrument_id), None, None, None);
    assert!(
        open.is_empty(),
        "expected no open orders after on_stop cancel_all_orders, found {}",
        open.len(),
    );

    let closed = cache.orders_closed(None, Some(&instrument_id), None, None, None);
    assert_eq!(
        closed.len(),
        1,
        "expected the limit order to be closed (canceled) after on_stop",
    );
    assert!(
        closed[0].is_canceled(),
        "expected the closed order to be in CANCELED status",
    );

    let bt_result = engine.get_result();
    assert_eq!(
        bt_result.total_orders, 1,
        "expected only the resting limit order to be tracked",
    );
}

#[rstest]
fn test_close_all_positions_in_on_stop_is_processed_streaming(
    crypto_perpetual_ethusdt: CryptoPerpetual,
) {
    // Streaming-mode counterpart: engine.run(streaming=true) does not call end()
    // internally; the BacktestNode-style caller invokes end() explicitly. The fix
    // must hold on this call path too, otherwise streaming consumers see the bug.
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    engine
        .add_strategy(CloseOnStop::new(instrument_id, Quantity::from("1.000")))
        .unwrap();

    let quotes = vec![
        quote(instrument_id, "1000.00", "1001.00", 1_000_000_000),
        quote(instrument_id, "1000.00", "1001.00", 2_000_000_000),
        quote(instrument_id, "1000.00", "1001.00", 3_000_000_000),
    ];
    engine.add_data(quotes, None, true, true).unwrap();

    engine.run(None, None, None, true).unwrap();
    engine.end();

    let cache_rc = engine.kernel().cache();
    let cache = cache_rc.borrow();

    let open = cache.positions_open(None, Some(&instrument_id), None, None, None);
    assert!(
        open.is_empty(),
        "expected no open positions after streaming run + end(), found {}",
        open.len(),
    );

    let closed = cache.positions_closed(None, Some(&instrument_id), None, None, None);
    assert_eq!(
        closed.len(),
        1,
        "expected one closed position after streaming run + end()",
    );

    let bt_result = engine.get_result();
    assert_eq!(
        bt_result.total_orders, 2,
        "expected opening and closing orders in streaming mode",
    );
}

#[rstest]
fn test_add_venue_with_oto_full_trigger(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let config = BacktestEngineConfig::default();
    let mut engine = BacktestEngine::new(config).unwrap();

    let result = engine.add_venue(
        SimulatedVenueConfig::builder()
            .venue(Venue::from("BINANCE"))
            .oms_type(OmsType::Netting)
            .account_type(AccountType::Margin)
            .book_type(BookType::L1_MBP)
            .starting_balances(vec![Money::from("1_000_000 USDT")])
            .oto_full_trigger(true)
            .build(),
    );
    assert!(result.is_ok());

    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(&instrument).unwrap();

    let quotes = vec![quote(instrument_id, "1000.00", "1000.10", 1_000_000_000)];
    engine.add_data(quotes, None, true, true).unwrap();
    engine.run(None, None, None, false).unwrap();
    assert_eq!(engine.get_result().iterations, 1);
}
