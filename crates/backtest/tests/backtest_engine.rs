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
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicU32, Ordering},
};

use ahash::AHashMap;
use nautilus_backtest::{config::BacktestEngineConfig, engine::BacktestEngine};
use nautilus_common::{
    actor::{DataActor, DataActorCore},
    timer::TimeEvent,
};
use nautilus_core::UnixNanos;
use nautilus_execution::models::{fee::FeeModelAny, fill::FillModelAny};
use nautilus_indicators::{
    average::ema::ExponentialMovingAverage,
    indicator::{Indicator, MovingAverage},
};
use nautilus_model::{
    data::{BarSpecification, BarType, Data, QuoteTick},
    enums::{
        AccountType, AggregationSource, BarAggregation, BookType, OmsType, OrderSide, PriceType,
    },
    events::OrderFilled,
    identifiers::{InstrumentId, StrategyId, Venue},
    instruments::{CryptoPerpetual, Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt},
    types::{Money, Price, Quantity},
};
use nautilus_trading::{Strategy, StrategyConfig, StrategyCore};
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

impl Deref for EmptyStrategy {
    type Target = DataActorCore;
    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl DerefMut for EmptyStrategy {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

impl Debug for EmptyStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(EmptyStrategy)).finish()
    }
}

impl DataActor for EmptyStrategy {}

impl Strategy for EmptyStrategy {
    fn core(&self) -> &StrategyCore {
        &self.core
    }

    fn core_mut(&mut self) -> &mut StrategyCore {
        &mut self.core
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
        self.submit_order(order, None, None)
    }
}

impl Deref for EmaCross {
    type Target = DataActorCore;
    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl DerefMut for EmaCross {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

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

impl Strategy for EmaCross {
    fn core(&self) -> &StrategyCore {
        &self.core
    }

    fn core_mut(&mut self) -> &mut StrategyCore {
        &mut self.core
    }
}

fn create_engine() -> BacktestEngine {
    let config = BacktestEngineConfig::default();
    let mut engine = BacktestEngine::new(config).unwrap();
    engine
        .add_venue(
            Venue::from("BINANCE"),
            OmsType::Netting,
            AccountType::Margin,
            BookType::L1_MBP,
            vec![Money::from("1_000_000 USDT")],
            None,
            None,
            AHashMap::new(),
            vec![],
            FillModelAny::default(),
            FeeModelAny::default(),
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
            None,
            None,
            None,
        )
        .unwrap();
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

#[rstest]
fn test_run_with_empty_data(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    engine
        .add_instrument(InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt))
        .unwrap();

    let result = engine.run(None, None, None, false);
    assert!(result.is_ok());

    let bt_result = engine.get_result();
    assert_eq!(bt_result.iterations, 0);
    assert_eq!(bt_result.total_orders, 0);
}

#[rstest]
fn test_run_processes_quote_ticks(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(instrument).unwrap();

    let quotes = vec![
        quote(instrument_id, "1000.00", "1000.10", 1_000_000_000),
        quote(instrument_id, "1000.50", "1000.60", 2_000_000_000),
        quote(instrument_id, "1001.00", "1001.10", 3_000_000_000),
    ];
    engine.add_data(quotes, None, true, true);

    let result = engine.run(None, None, None, false);
    assert!(result.is_ok());

    let bt_result = engine.get_result();
    assert_eq!(bt_result.iterations, 3);
}

#[rstest]
fn test_run_with_strategy(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(instrument).unwrap();

    engine.add_strategy(EmptyStrategy::new()).unwrap();

    let quotes = vec![
        quote(instrument_id, "1000.00", "1000.10", 1_000_000_000),
        quote(instrument_id, "1000.50", "1000.60", 2_000_000_000),
        quote(instrument_id, "1001.00", "1001.10", 3_000_000_000),
    ];
    engine.add_data(quotes, None, true, true);

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
    engine.add_instrument(instrument).unwrap();

    let base: u64 = 1_000_000_000_000_000_000; // 1e18 ns
    let quotes = vec![
        quote(instrument_id, "1000.00", "1000.10", base),
        quote(instrument_id, "1000.50", "1000.60", base + 1_000_000_000),
        quote(instrument_id, "1001.00", "1001.10", base + 2_000_000_000),
        quote(instrument_id, "1001.50", "1001.60", base + 3_000_000_000),
    ];
    engine.add_data(quotes, None, true, true);

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
    engine.add_instrument(instrument).unwrap();

    let quotes = vec![
        quote(instrument_id, "1000.00", "1000.10", 1_000_000_000),
        quote(instrument_id, "1000.50", "1000.60", 2_000_000_000),
    ];
    engine.add_data(quotes, None, true, true);

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
    engine.add_instrument(instrument).unwrap();

    let quotes = vec![quote(instrument_id, "1000.00", "1000.10", 1_000_000_000)];
    engine.add_data(quotes, None, true, true);
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
    engine.add_instrument(instrument).unwrap();

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
    engine.add_data(quotes, None, true, true);

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

#[rstest]
fn test_streaming_mode_processes_data_in_batches(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(instrument).unwrap();
    engine.add_strategy(EmptyStrategy::new()).unwrap();

    // Batch 1: first 3 quotes
    let batch1 = vec![
        quote(instrument_id, "1000.00", "1000.10", 1_000_000_000),
        quote(instrument_id, "1001.00", "1001.10", 2_000_000_000),
        quote(instrument_id, "1002.00", "1002.10", 3_000_000_000),
    ];
    engine.add_data(batch1, None, true, true);
    engine.run(None, None, None, true).unwrap(); // streaming=true

    let result1 = engine.get_result();
    assert_eq!(result1.iterations, 3);

    // Batch 2: next 2 quotes, clear old data first
    engine.clear_data();
    let batch2 = vec![
        quote(instrument_id, "1003.00", "1003.10", 4_000_000_000),
        quote(instrument_id, "1004.00", "1004.10", 5_000_000_000),
    ];
    engine.add_data(batch2, None, true, true);
    engine.run(None, None, None, false).unwrap(); // streaming=false, finalizes

    let result2 = engine.get_result();
    assert_eq!(result2.iterations, 5); // Total across both batches
}

#[rstest]
fn test_multiple_add_data_batches_merged(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(instrument).unwrap();

    // Add data in two separate batches (the P1 fix scenario)
    let batch1 = vec![
        quote(instrument_id, "1000.00", "1000.10", 1_000_000_000),
        quote(instrument_id, "1002.00", "1002.10", 3_000_000_000),
    ];
    let batch2 = vec![
        quote(instrument_id, "1001.00", "1001.10", 2_000_000_000),
        quote(instrument_id, "1003.00", "1003.10", 4_000_000_000),
    ];
    engine.add_data(batch1, None, true, true);
    engine.add_data(batch2, None, true, true);

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

    // Add BINANCE venue
    engine
        .add_venue(
            Venue::from("BINANCE"),
            OmsType::Netting,
            AccountType::Margin,
            BookType::L1_MBP,
            vec![Money::from("1_000_000 USDT")],
            None,
            None,
            AHashMap::new(),
            vec![],
            FillModelAny::default(),
            FeeModelAny::default(),
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
            None,
            None,
            None,
        )
        .unwrap();

    // Add BITMEX venue
    engine
        .add_venue(
            Venue::from("BITMEX"),
            OmsType::Netting,
            AccountType::Margin,
            BookType::L1_MBP,
            vec![Money::from("1_000_000 USD")],
            None,
            None,
            AHashMap::new(),
            vec![],
            FillModelAny::default(),
            FeeModelAny::default(),
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
            None,
            None,
            None,
        )
        .unwrap();

    let eth = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let eth_id = eth.id();
    engine.add_instrument(eth).unwrap();

    let btc = InstrumentAny::CryptoPerpetual(nautilus_model::instruments::stubs::xbtusd_bitmex());
    let btc_id = btc.id();
    engine.add_instrument(btc).unwrap();

    // Interleave quotes from both venues (respecting instrument precision)
    // ETHUSDT-PERP.BINANCE: price_prec=2, size_prec=3
    // BTCUSDT.BITMEX: price_prec=1, size_prec=0
    let quotes = vec![
        quote(eth_id, "1000.00", "1000.10", 1_000_000_000),
        quote_with_size(btc_id, "50000.5", "50001.0", "1", 2_000_000_000),
        quote(eth_id, "1001.00", "1001.10", 3_000_000_000),
        quote_with_size(btc_id, "50100.5", "50101.0", "1", 4_000_000_000),
    ];
    engine.add_data(quotes, None, true, true);

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
    engine.add_instrument(instrument).unwrap();

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

    engine.add_data(quotes, None, true, true);
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
    engine.add_instrument(instrument).unwrap();

    let quotes = vec![
        quote(instrument_id, "1000.00", "1000.10", 1_000_000_000),
        quote(instrument_id, "1001.00", "1001.10", 2_000_000_000),
        quote(instrument_id, "1002.00", "1002.10", 3_000_000_000),
    ];
    engine.add_data(quotes, None, true, true);

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
    engine.add_instrument(instrument).unwrap();

    let quotes = vec![
        quote(instrument_id, "1000.00", "1000.10", 1_000_000_000),
        quote(instrument_id, "1001.00", "1001.10", 2_000_000_000),
        quote(instrument_id, "1002.00", "1002.10", 3_000_000_000),
        quote(instrument_id, "1003.00", "1003.10", 4_000_000_000),
        quote(instrument_id, "1004.00", "1004.10", 5_000_000_000),
    ];
    engine.add_data(quotes, None, true, true);

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
    engine.add_instrument(instrument).unwrap();

    let quotes = vec![
        quote(instrument_id, "1000.00", "1000.10", 1_000_000_000),
        quote(instrument_id, "1001.00", "1001.10", 2_000_000_000),
        quote(instrument_id, "1002.00", "1002.10", 3_000_000_000),
        quote(instrument_id, "1003.00", "1003.10", 4_000_000_000),
        quote(instrument_id, "1004.00", "1004.10", 5_000_000_000),
    ];
    engine.add_data(quotes, None, true, true);

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
    engine.add_instrument(instrument).unwrap();

    engine
        .add_strategy(EmaCross::new(instrument_id, Quantity::from("0.100"), 3, 5))
        .unwrap();

    let base_ts: u64 = 1_000_000_000;
    let interval: u64 = 1_000_000_000;

    // Add flat data in one batch
    let flat: Vec<Data> = (0..10u64)
        .map(|i| quote(instrument_id, "1000.00", "1000.10", base_ts + i * interval))
        .collect();
    engine.add_data(flat, None, true, true);

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
    engine.add_data(ramp_up, None, true, true);

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

impl Deref for CascadingStopStrategy {
    type Target = DataActorCore;
    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl DerefMut for CascadingStopStrategy {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

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
            self.submit_order(order, None, None)?;
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
            self.submit_order(order, None, None)?;
        }
        Ok(())
    }
}

impl Strategy for CascadingStopStrategy {
    fn core(&self) -> &StrategyCore {
        &self.core
    }

    fn core_mut(&mut self) -> &mut StrategyCore {
        &mut self.core
    }
}

#[rstest]
fn test_cascading_stop_loss_on_fill_settled_same_tick(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(instrument).unwrap();

    let strategy = CascadingStopStrategy::new(instrument_id, Quantity::from("1.000"));
    engine.add_strategy(strategy).unwrap();

    let quotes = vec![
        quote(instrument_id, "1000.00", "1001.00", 1_000_000_000),
        quote(instrument_id, "1000.50", "1001.50", 2_000_000_000),
    ];
    engine.add_data(quotes, None, true, true);

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
            order_id_tag: Some("002".to_string()),
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

impl Deref for DualTimerStrategy {
    type Target = DataActorCore;
    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl DerefMut for DualTimerStrategy {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

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
        self.submit_order(order, None, None)?;
        Ok(())
    }
}

impl Strategy for DualTimerStrategy {
    fn core(&self) -> &StrategyCore {
        &self.core
    }

    fn core_mut(&mut self) -> &mut StrategyCore {
        &mut self.core
    }
}

#[rstest]
fn test_all_same_timestamp_timer_commands_settled(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(instrument).unwrap();

    // Timer fires at 30s, between data points at 0s and 60s
    let timer_ts: u64 = 30_000_000_000;
    let strategy = DualTimerStrategy::new(instrument_id, Quantity::from("1.000"), timer_ts);
    engine.add_strategy(strategy).unwrap();

    let quotes = vec![
        quote(instrument_id, "1000.00", "1001.00", 0),
        quote(instrument_id, "1000.50", "1001.50", 60_000_000_000),
    ];
    engine.add_data(quotes, None, true, true);

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

impl Deref for BarSubscriberStrategy {
    type Target = DataActorCore;
    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl DerefMut for BarSubscriberStrategy {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

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

impl Strategy for BarSubscriberStrategy {
    fn core(&self) -> &StrategyCore {
        &self.core
    }

    fn core_mut(&mut self) -> &mut StrategyCore {
        &mut self.core
    }
}

#[rstest]
fn test_streaming_no_dummy_bars_past_batch_data(crypto_perpetual_ethusdt: CryptoPerpetual) {
    let mut engine = create_engine();
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);
    let instrument_id = instrument.id();
    engine.add_instrument(instrument).unwrap();

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
    engine.add_data(batch1, None, true, true);

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
    engine.add_data(batch2, None, true, true);
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
    engine.add_instrument(instrument).unwrap();

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
    engine.add_data(batch, None, true, true);

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
