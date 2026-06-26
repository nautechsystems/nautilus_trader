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

//! End-to-end benchmarks for the v2 [`BacktestEngine`] run path.
//!
//! Each case builds a full engine with a simulated venue, instrument, market data, and optional
//! strategy outside the measured section. The timed section is `BacktestEngine::run`, while
//! teardown happens after timing so global message-bus cleanup does not pollute the run profile.
//!
//! Workloads:
//! - `market_data_replay`: interleaved quote and trade ticks with no strategy orders.
//! - `alternating_market_orders`: quote-driven strategy submitting market orders through the full
//!   strategy, risk, execution client, exchange, matching engine, cache, and portfolio path.
//! - `passive_limit_orders`: quote-driven strategy accumulating resting limit orders so
//!   `OrderMatchingCore` maintains passive order state while quote and trade ticks iterate.
//! - `data_routes`: bar, L2 delta, depth10, mark/index price, funding, status, and close event
//!   routing through the engine and simulated exchange.
//! - `order_type_sweep`: one strategy submits market, limit, stop, touched, and trailing orders
//!   while quote and trade ticks drive matching and trigger evaluation.
//!
//! Run with `cargo bench -p nautilus-backtest --bench engine`.

use std::{
    fmt::Debug,
    hint::black_box,
    time::{Duration, Instant},
};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_backtest::{
    config::{BacktestEngineConfig, SimulatedVenueConfig},
    engine::BacktestEngine,
};
use nautilus_common::{actor::DataActor, logging::logger::LoggerConfig};
use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{
        Bar, BarSpecification, BarType, BookOrder, Data, FundingRateUpdate, IndexPriceUpdate,
        InstrumentClose, InstrumentStatus, MarkPriceUpdate, OrderBookDelta, OrderBookDeltas,
        OrderBookDeltas_API, OrderBookDepth10, QuoteTick, TradeTick, depth::DEPTH10_LEN,
    },
    enums::{
        AccountType, AggregationSource, AggressorSide, BarAggregation, BookAction, BookType,
        InstrumentCloseType, MarketStatusAction, OmsType, OrderSide, PriceType, TimeInForce,
        TrailingOffsetType, TriggerType,
    },
    identifiers::{InstrumentId, StrategyId, TradeId, Venue},
    instruments::{Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt},
    types::{Money, Price, Quantity},
};
use nautilus_trading::{Strategy, StrategyConfig, StrategyCore, nautilus_strategy};
use rust_decimal::Decimal;

const QUOTE_COUNTS: &[usize] = &[1_000, 10_000];
const DATA_ROUTE_COUNT: usize = 1_000;
const ORDER_SWEEP_QUOTE_COUNT: usize = 1_000;
const ORDER_TYPE_SWEEP_ORDERS: usize = 9;
const BASE_TS_NS: u64 = 1_735_689_600_000_000_000;
const QUOTE_INTERVAL_NS: u64 = 1_000_000;
const TRADE_OFFSET_NS: u64 = 500_000;
const MARKET_ORDER_INTERVAL: usize = 10;
const PASSIVE_ORDER_INTERVAL: usize = 20;

fn bench_run(c: &mut Criterion) {
    let mut group = c.benchmark_group("backtest_engine/run");
    let instrument_id = crypto_perpetual_ethusdt().id();

    for &quote_count in QUOTE_COUNTS {
        let data = generate_market_data(instrument_id, quote_count);
        let data_count = data.len();
        group.throughput(Throughput::Elements(data_count as u64));

        group.bench_with_input(
            BenchmarkId::new("market_data_replay", data_count),
            &data,
            |b, data| {
                b.iter_custom(|iters| {
                    run_engine_iterations(iters, data_count, 0, || {
                        build_market_data_replay(data.clone())
                    })
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("alternating_market_orders", data_count),
            &data,
            |b, data| {
                let expected_orders = quote_count / MARKET_ORDER_INTERVAL;
                b.iter_custom(|iters| {
                    run_engine_iterations(iters, data_count, expected_orders, || {
                        build_alternating_market_orders(data.clone(), quote_count)
                    })
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("passive_limit_orders", data_count),
            &data,
            |b, data| {
                let expected_orders = quote_count / PASSIVE_ORDER_INTERVAL;
                b.iter_custom(|iters| {
                    run_engine_iterations(iters, data_count, expected_orders, || {
                        build_passive_limit_orders(data.clone(), quote_count)
                    })
                });
            },
        );
    }

    group.finish();
}

fn bench_data_routes(c: &mut Criterion) {
    let mut group = c.benchmark_group("backtest_engine/data_routes");
    let instrument_id = crypto_perpetual_ethusdt().id();
    let cases = vec![
        (
            "bar_last",
            generate_last_bar_data(instrument_id, DATA_ROUTE_COUNT),
            EngineBuildConfig::default(),
        ),
        (
            "bar_bid_ask",
            generate_bid_ask_bar_data(instrument_id, DATA_ROUTE_COUNT),
            EngineBuildConfig::default(),
        ),
        (
            "l2_deltas",
            generate_l2_delta_data(instrument_id, DATA_ROUTE_COUNT),
            EngineBuildConfig {
                book_type: BookType::L2_MBP,
                ..Default::default()
            },
        ),
        (
            "depth10",
            generate_depth10_data(instrument_id, DATA_ROUTE_COUNT),
            EngineBuildConfig {
                book_type: BookType::L2_MBP,
                ..Default::default()
            },
        ),
        (
            "price_status_funding",
            generate_price_status_funding_data(instrument_id, DATA_ROUTE_COUNT),
            EngineBuildConfig::default(),
        ),
    ];

    for (name, data, config) in cases {
        let data_count = data.len();
        group.throughput(Throughput::Elements(data_count as u64));
        group.bench_with_input(BenchmarkId::new(name, data_count), &data, |b, data| {
            b.iter_custom(|iters| {
                run_engine_iterations(iters, data_count, 0, || {
                    build_engine_with_config(data.clone(), None, config)
                })
            });
        });
    }

    group.finish();
}

fn bench_order_types(c: &mut Criterion) {
    let mut group = c.benchmark_group("backtest_engine/order_types");
    let instrument_id = crypto_perpetual_ethusdt().id();
    let data = generate_order_trigger_data(instrument_id, ORDER_SWEEP_QUOTE_COUNT);
    let data_count = data.len();

    group.throughput(Throughput::Elements(data_count as u64));
    group.bench_with_input(
        BenchmarkId::new("order_type_sweep", data_count),
        &data,
        |b, data| {
            b.iter_custom(|iters| {
                run_engine_iterations(iters, data_count, ORDER_TYPE_SWEEP_ORDERS, || {
                    build_engine_with_config(
                        data.clone(),
                        Some(StrategyWorkload::OrderSweep(OrderTypeSweep::new(
                            instrument_id,
                        ))),
                        EngineBuildConfig {
                            reject_stop_orders: false,
                            ..Default::default()
                        },
                    )
                })
            });
        },
    );

    group.finish();
}

fn run_engine_iterations<F>(
    iters: u64,
    expected_iterations: usize,
    expected_orders: usize,
    mut build_engine: F,
) -> Duration
where
    F: FnMut() -> BacktestEngine,
{
    let mut elapsed = Duration::ZERO;

    for _ in 0..iters {
        let mut engine = build_engine();
        let started = Instant::now();
        engine
            .run(None, None, None, false)
            .expect("backtest run should succeed");
        elapsed += started.elapsed();

        black_box(engine.iteration());
        assert_eq!(engine.iteration(), expected_iterations);
        assert_eq!(engine.get_result().total_orders, expected_orders);
        engine.dispose();
    }

    elapsed
}

fn build_market_data_replay(data: Vec<Data>) -> BacktestEngine {
    build_engine_with_config(data, None, EngineBuildConfig::default())
}

fn build_alternating_market_orders(data: Vec<Data>, quote_count: usize) -> BacktestEngine {
    let instrument_id = crypto_perpetual_ethusdt().id();
    build_engine_with_config(
        data,
        Some(StrategyWorkload::Market(AlternatingMarketOrders::new(
            instrument_id,
            quote_count / MARKET_ORDER_INTERVAL,
        ))),
        EngineBuildConfig::default(),
    )
}

fn build_passive_limit_orders(data: Vec<Data>, quote_count: usize) -> BacktestEngine {
    let instrument_id = crypto_perpetual_ethusdt().id();
    build_engine_with_config(
        data,
        Some(StrategyWorkload::Passive(PassiveLimitOrders::new(
            instrument_id,
            quote_count / PASSIVE_ORDER_INTERVAL,
        ))),
        EngineBuildConfig::default(),
    )
}

#[derive(Clone, Copy)]
struct EngineBuildConfig {
    book_type: BookType,
    reject_stop_orders: bool,
}

impl Default for EngineBuildConfig {
    fn default() -> Self {
        Self {
            book_type: BookType::L1_MBP,
            reject_stop_orders: true,
        }
    }
}

fn build_engine_with_config(
    data: Vec<Data>,
    strategy: Option<StrategyWorkload>,
    build_config: EngineBuildConfig,
) -> BacktestEngine {
    let config = BacktestEngineConfig {
        logging: LoggerConfig::from_spec("bypass_logging")
            .expect("benchmark logger config should be valid"),
        bypass_logging: true,
        run_analysis: false,
        ..Default::default()
    };
    let mut engine = BacktestEngine::new(config).expect("engine config should be valid");
    engine
        .add_venue(
            SimulatedVenueConfig::builder()
                .venue(Venue::from("BINANCE"))
                .oms_type(OmsType::Netting)
                .account_type(AccountType::Margin)
                .book_type(build_config.book_type)
                .starting_balances(vec![Money::from("1_000_000 USDT")])
                .reject_stop_orders(build_config.reject_stop_orders)
                .queue_position(true)
                .build()
                .expect("venue config should be valid"),
        )
        .expect("venue should be added");

    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    engine
        .add_instrument(&instrument)
        .expect("instrument should be added");

    match strategy {
        Some(StrategyWorkload::Market(strategy)) => engine
            .add_strategy(strategy)
            .expect("market order strategy should be added"),
        Some(StrategyWorkload::Passive(strategy)) => engine
            .add_strategy(strategy)
            .expect("passive limit strategy should be added"),
        Some(StrategyWorkload::OrderSweep(strategy)) => engine
            .add_strategy(strategy)
            .expect("order type sweep strategy should be added"),
        None => {}
    }

    engine
        .add_data(data, None, true, true)
        .expect("market data should be added");
    engine
}

fn generate_market_data(instrument_id: InstrumentId, quote_count: usize) -> Vec<Data> {
    let mut data = Vec::with_capacity(quote_count * 2);

    for i in 0..quote_count {
        let quote_ts = BASE_TS_NS + i as u64 * QUOTE_INTERVAL_NS;
        let mid_cents = 100_000 + i as i64 % 200 - 100;
        let bid = price_from_cents(mid_cents - 5);
        let ask = price_from_cents(mid_cents + 5);
        let trade_price = price_from_cents(mid_cents);
        let aggressor_side = if i % 2 == 0 {
            AggressorSide::Buyer
        } else {
            AggressorSide::Seller
        };

        data.push(Data::Quote(QuoteTick::new(
            instrument_id,
            Price::from(bid.as_str()),
            Price::from(ask.as_str()),
            Quantity::from("100.000"),
            Quantity::from("100.000"),
            quote_ts.into(),
            quote_ts.into(),
        )));

        let trade_ts = quote_ts + TRADE_OFFSET_NS;
        data.push(Data::Trade(TradeTick::new(
            instrument_id,
            Price::from(trade_price.as_str()),
            Quantity::from("1.000"),
            aggressor_side,
            TradeId::from(format!("T-{i}").as_str()),
            trade_ts.into(),
            trade_ts.into(),
        )));
    }

    data
}

fn price_from_cents(cents: i64) -> String {
    format!("{}.{:02}", cents / 100, cents % 100)
}

fn generate_last_bar_data(instrument_id: InstrumentId, bar_count: usize) -> Vec<Data> {
    let bar_type = BarType::new(
        instrument_id,
        BarSpecification::new(1, BarAggregation::Minute, PriceType::Last),
        AggregationSource::External,
    );

    (0..bar_count)
        .map(|i| {
            let ts = BASE_TS_NS + i as u64 * QUOTE_INTERVAL_NS;
            let open = 100_000 + i as i64 % 200;
            Data::Bar(bar(bar_type, open, open + 10, open - 10, open + 5, ts))
        })
        .collect()
}

fn generate_bid_ask_bar_data(instrument_id: InstrumentId, bar_count: usize) -> Vec<Data> {
    let bid_bar_type = BarType::new(
        instrument_id,
        BarSpecification::new(1, BarAggregation::Minute, PriceType::Bid),
        AggregationSource::External,
    );
    let ask_bar_type = BarType::new(
        instrument_id,
        BarSpecification::new(1, BarAggregation::Minute, PriceType::Ask),
        AggregationSource::External,
    );
    let mut data = Vec::with_capacity(bar_count * 2);

    for i in 0..bar_count {
        let ts = BASE_TS_NS + i as u64 * QUOTE_INTERVAL_NS;
        let bid_open = 100_000 + i as i64 % 200;
        let ask_open = bid_open + 10;
        data.push(Data::Bar(bar(
            bid_bar_type,
            bid_open,
            bid_open + 8,
            bid_open - 8,
            bid_open + 4,
            ts,
        )));
        data.push(Data::Bar(bar(
            ask_bar_type,
            ask_open,
            ask_open + 8,
            ask_open - 8,
            ask_open + 4,
            ts,
        )));
    }

    data
}

fn bar(bar_type: BarType, open: i64, high: i64, low: i64, close: i64, ts: u64) -> Bar {
    Bar::new(
        bar_type,
        Price::from(price_from_cents(open).as_str()),
        Price::from(price_from_cents(high).as_str()),
        Price::from(price_from_cents(low).as_str()),
        Price::from(price_from_cents(close).as_str()),
        Quantity::from("100.000"),
        ts.into(),
        ts.into(),
    )
}

fn generate_l2_delta_data(instrument_id: InstrumentId, event_count: usize) -> Vec<Data> {
    let mut data = Vec::with_capacity(event_count);

    for i in 0..event_count {
        let sequence = u64::try_from(i + 1).expect("sequence should fit in u64");
        let ts = BASE_TS_NS + sequence * QUOTE_INTERVAL_NS;
        let base = 100_000 + i as i64 % 100;
        let bid = order_book_delta(instrument_id, OrderSide::Buy, base - 10, sequence, ts);
        let ask = order_book_delta(instrument_id, OrderSide::Sell, base + 10, sequence + 1, ts);

        if i.is_multiple_of(2) {
            data.push(Data::Delta(bid));
        } else {
            data.push(Data::Deltas(OrderBookDeltas_API::new(
                OrderBookDeltas::new(instrument_id, vec![bid, ask]),
            )));
        }
    }

    data
}

fn order_book_delta(
    instrument_id: InstrumentId,
    side: OrderSide,
    price: i64,
    sequence: u64,
    ts: u64,
) -> OrderBookDelta {
    OrderBookDelta::new(
        instrument_id,
        BookAction::Add,
        BookOrder::new(
            side,
            Price::from(price_from_cents(price).as_str()),
            Quantity::from("1.000"),
            sequence,
        ),
        0,
        sequence,
        ts.into(),
        ts.into(),
    )
}

fn generate_depth10_data(instrument_id: InstrumentId, depth_count: usize) -> Vec<Data> {
    (0..depth_count)
        .map(|i| {
            let sequence = u64::try_from(i + 1).expect("sequence should fit in u64");
            let ts = BASE_TS_NS + sequence * QUOTE_INTERVAL_NS;
            let base = 100_000 + i as i64 % 100;
            let mut bids = [BookOrder::default(); DEPTH10_LEN];
            let mut asks = [BookOrder::default(); DEPTH10_LEN];

            for level in 0..DEPTH10_LEN {
                let level_id = u64::try_from(level).expect("depth level should fit in u64");
                let level_offset = i64::try_from(level).expect("depth level should fit in i64");
                bids[level] = BookOrder::new(
                    OrderSide::Buy,
                    Price::from(price_from_cents(base - 10 - level_offset).as_str()),
                    Quantity::from("1.000"),
                    sequence * 100 + level_id,
                );
                asks[level] = BookOrder::new(
                    OrderSide::Sell,
                    Price::from(price_from_cents(base + 10 + level_offset).as_str()),
                    Quantity::from("1.000"),
                    sequence * 100 + 50 + level_id,
                );
            }

            Data::Depth10(Box::new(OrderBookDepth10::new(
                instrument_id,
                bids,
                asks,
                [1; DEPTH10_LEN],
                [1; DEPTH10_LEN],
                0,
                sequence,
                ts.into(),
                ts.into(),
            )))
        })
        .collect()
}

fn generate_price_status_funding_data(
    instrument_id: InstrumentId,
    cycle_count: usize,
) -> Vec<Data> {
    let mut data = Vec::with_capacity(cycle_count * 5);

    for i in 0..cycle_count {
        let ts = BASE_TS_NS + i as u64 * QUOTE_INTERVAL_NS * 5;
        let price = Price::from(price_from_cents(100_000 + i as i64 % 100).as_str());
        let status_action = if i.is_multiple_of(2) {
            MarketStatusAction::Pause
        } else {
            MarketStatusAction::Trading
        };

        data.push(Data::MarkPriceUpdate(MarkPriceUpdate::new(
            instrument_id,
            price,
            UnixNanos::from(ts),
            UnixNanos::from(ts),
        )));
        data.push(Data::IndexPriceUpdate(IndexPriceUpdate::new(
            instrument_id,
            price,
            UnixNanos::from(ts + 1),
            UnixNanos::from(ts + 1),
        )));
        data.push(Data::FundingRateUpdate(FundingRateUpdate::new(
            instrument_id,
            Decimal::new(1, 4),
            None,
            None,
            UnixNanos::from(ts + 2),
            UnixNanos::from(ts + 2),
        )));
        data.push(Data::InstrumentStatus(InstrumentStatus::new(
            instrument_id,
            status_action,
            UnixNanos::from(ts + 3),
            UnixNanos::from(ts + 3),
            None,
            None,
            Some(matches!(status_action, MarketStatusAction::Trading)),
            Some(true),
            None,
        )));
        data.push(Data::InstrumentClose(InstrumentClose::new(
            instrument_id,
            price,
            InstrumentCloseType::EndOfSession,
            UnixNanos::from(ts + 4),
            UnixNanos::from(ts + 4),
        )));
    }

    data
}

fn generate_order_trigger_data(instrument_id: InstrumentId, quote_count: usize) -> Vec<Data> {
    let mut data = Vec::with_capacity(quote_count * 2);

    for i in 0..quote_count {
        let quote_ts = BASE_TS_NS + i as u64 * QUOTE_INTERVAL_NS;
        let mid = 100_000 + i as i64;
        let bid = price_from_cents(mid - 5);
        let ask = price_from_cents(mid + 5);
        let trade_price = price_from_cents(mid);

        data.push(Data::Quote(QuoteTick::new(
            instrument_id,
            Price::from(bid.as_str()),
            Price::from(ask.as_str()),
            Quantity::from("100.000"),
            Quantity::from("100.000"),
            quote_ts.into(),
            quote_ts.into(),
        )));

        let trade_ts = quote_ts + TRADE_OFFSET_NS;
        data.push(Data::Trade(TradeTick::new(
            instrument_id,
            Price::from(trade_price.as_str()),
            Quantity::from("1.000"),
            AggressorSide::Buyer,
            TradeId::from(format!("OT-{i}").as_str()),
            trade_ts.into(),
            trade_ts.into(),
        )));
    }

    data
}

enum StrategyWorkload {
    Market(AlternatingMarketOrders),
    Passive(PassiveLimitOrders),
    OrderSweep(OrderTypeSweep),
}

struct AlternatingMarketOrders {
    core: StrategyCore,
    instrument_id: InstrumentId,
    trade_size: Quantity,
    max_orders: usize,
    quote_count: usize,
    orders_submitted: usize,
}

impl AlternatingMarketOrders {
    fn new(instrument_id: InstrumentId, max_orders: usize) -> Self {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("BENCH-MARKET-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        Self {
            core: StrategyCore::new(config),
            instrument_id,
            trade_size: Quantity::from("0.010"),
            max_orders,
            quote_count: 0,
            orders_submitted: 0,
        }
    }

    fn submit_market_order(&mut self) -> anyhow::Result<()> {
        let side = if self.orders_submitted.is_multiple_of(2) {
            OrderSide::Buy
        } else {
            OrderSide::Sell
        };
        let order = self.order().market(
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
        self.orders_submitted += 1;
        self.submit_order(order, None, None, None)
    }
}

nautilus_strategy!(AlternatingMarketOrders);

impl Debug for AlternatingMarketOrders {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(AlternatingMarketOrders)).finish()
    }
}

impl DataActor for AlternatingMarketOrders {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.subscribe_quotes(self.instrument_id, None, None);
        Ok(())
    }

    fn on_quote(&mut self, _quote: &QuoteTick) -> anyhow::Result<()> {
        self.quote_count += 1;
        if self.quote_count.is_multiple_of(MARKET_ORDER_INTERVAL)
            && self.orders_submitted < self.max_orders
        {
            self.submit_market_order()?;
        }
        Ok(())
    }
}

struct PassiveLimitOrders {
    core: StrategyCore,
    instrument_id: InstrumentId,
    trade_size: Quantity,
    max_orders: usize,
    quote_count: usize,
    orders_submitted: usize,
}

impl PassiveLimitOrders {
    fn new(instrument_id: InstrumentId, max_orders: usize) -> Self {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("BENCH-LIMIT-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        Self {
            core: StrategyCore::new(config),
            instrument_id,
            trade_size: Quantity::from("0.010"),
            max_orders,
            quote_count: 0,
            orders_submitted: 0,
        }
    }

    fn submit_passive_limit_order(&mut self) -> anyhow::Result<()> {
        let side = if self.orders_submitted.is_multiple_of(2) {
            OrderSide::Buy
        } else {
            OrderSide::Sell
        };
        let limit_price = passive_limit_price(side, self.orders_submitted);
        let order = self.order().limit(
            self.instrument_id,
            side,
            self.trade_size,
            limit_price,
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
        self.orders_submitted += 1;
        self.submit_order(order, None, None, None)
    }
}

nautilus_strategy!(PassiveLimitOrders);

impl Debug for PassiveLimitOrders {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PassiveLimitOrders)).finish()
    }
}

impl DataActor for PassiveLimitOrders {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.subscribe_quotes(self.instrument_id, None, None);
        Ok(())
    }

    fn on_quote(&mut self, _quote: &QuoteTick) -> anyhow::Result<()> {
        self.quote_count += 1;
        if self.quote_count.is_multiple_of(PASSIVE_ORDER_INTERVAL)
            && self.orders_submitted < self.max_orders
        {
            self.submit_passive_limit_order()?;
        }
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        self.cancel_all_orders(self.instrument_id, None, None, None)
    }
}

struct OrderTypeSweep {
    core: StrategyCore,
    instrument_id: InstrumentId,
    trade_size: Quantity,
    submitted: bool,
}

impl OrderTypeSweep {
    fn new(instrument_id: InstrumentId) -> Self {
        let config = StrategyConfig {
            strategy_id: Some(StrategyId::from("BENCH-ORDER-SWEEP-001")),
            order_id_tag: Some("001".to_string()),
            ..Default::default()
        };
        Self {
            core: StrategyCore::new(config),
            instrument_id,
            trade_size: Quantity::from("0.010"),
            submitted: false,
        }
    }

    fn submit_order_type_sweep(&mut self) -> anyhow::Result<()> {
        self.submit_order(
            self.order().market(
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
            ),
            None,
            None,
            None,
        )?;
        self.submit_order(
            self.order().limit(
                self.instrument_id,
                OrderSide::Buy,
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
            ),
            None,
            None,
            None,
        )?;
        self.submit_order(
            self.order().market_to_limit(
                self.instrument_id,
                OrderSide::Sell,
                self.trade_size,
                Some(TimeInForce::Gtc),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            ),
            None,
            None,
            None,
        )?;
        self.submit_order(
            self.order().stop_market(
                self.instrument_id,
                OrderSide::Buy,
                self.trade_size,
                Price::from("1001.00"),
                Some(TriggerType::LastPrice),
                Some(TimeInForce::Gtc),
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
            ),
            None,
            None,
            None,
        )?;
        self.submit_order(
            self.order().stop_limit(
                self.instrument_id,
                OrderSide::Buy,
                self.trade_size,
                Price::from("1002.50"),
                Price::from("1002.00"),
                Some(TriggerType::LastPrice),
                Some(TimeInForce::Gtc),
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
            ),
            None,
            None,
            None,
        )?;
        self.submit_order(
            self.order().market_if_touched(
                self.instrument_id,
                OrderSide::Sell,
                self.trade_size,
                Price::from("1002.00"),
                Some(TriggerType::LastPrice),
                Some(TimeInForce::Gtc),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
            ),
            None,
            None,
            None,
        )?;
        self.submit_order(
            self.order().limit_if_touched(
                self.instrument_id,
                OrderSide::Sell,
                self.trade_size,
                Price::from("1001.50"),
                Price::from("1001.50"),
                Some(TriggerType::LastPrice),
                Some(TimeInForce::Gtc),
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
            ),
            None,
            None,
            None,
        )?;
        self.submit_order(
            self.order().trailing_stop_market(
                self.instrument_id,
                OrderSide::Buy,
                self.trade_size,
                Decimal::new(50, 2),
                Some(TrailingOffsetType::Price),
                None,
                Some(Price::from("1001.00")),
                Some(TriggerType::BidAsk),
                Some(TimeInForce::Gtc),
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
            ),
            None,
            None,
            None,
        )?;
        self.submit_order(
            self.order().trailing_stop_limit(
                self.instrument_id,
                OrderSide::Buy,
                self.trade_size,
                Price::from("1002.00"),
                Decimal::new(50, 2),
                Decimal::new(50, 2),
                Some(TrailingOffsetType::Price),
                None,
                Some(Price::from("1001.50")),
                Some(TriggerType::BidAsk),
                Some(TimeInForce::Gtc),
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
            ),
            None,
            None,
            None,
        )
    }
}

nautilus_strategy!(OrderTypeSweep);

impl Debug for OrderTypeSweep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(OrderTypeSweep)).finish()
    }
}

impl DataActor for OrderTypeSweep {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.subscribe_quotes(self.instrument_id, None, None);
        Ok(())
    }

    fn on_quote(&mut self, _quote: &QuoteTick) -> anyhow::Result<()> {
        if !self.submitted {
            self.submitted = true;
            self.submit_order_type_sweep()?;
        }
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        self.cancel_all_orders(self.instrument_id, None, None, None)
    }
}

fn passive_limit_price(side: OrderSide, order_index: usize) -> Price {
    let offset = i64::try_from(order_index % 100).expect("offset should fit in i64");
    let cents = match side {
        OrderSide::Buy => 90_000 - offset,
        OrderSide::Sell => 110_000 + offset,
        _ => unreachable!(),
    };
    Price::from(price_from_cents(cents).as_str())
}

criterion_group!(benches, bench_run, bench_data_routes, bench_order_types);
criterion_main!(benches);
