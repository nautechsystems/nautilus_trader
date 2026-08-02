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

//! Benchmarks for [`OrderMatchingEngine`].
//!
//! Each workload uses the standalone engine construction pattern from the
//! matching engine integration tests. Engine, cache, book, order, and command
//! setup is excluded from the timed region. Post-run assertions verify exact
//! event counts and final order states so a rejection or no-op cannot be timed
//! as successful work.

use std::{
    cell::RefCell,
    hint::black_box,
    rc::Rc,
    time::{Duration, Instant},
};

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_common::{
    cache::Cache,
    clock::TestClock,
    messages::execution::{CancelOrder, ModifyOrder},
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_execution::{
    matching_engine::{config::OrderMatchingEngineConfig, engine::OrderMatchingEngine},
    models::{fee::FeeModelAny, fill::FillModelHandle},
};
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, QuoteTick, TradeTick, stubs::OrderBookDeltaTestBuilder},
    enums::{
        AccountType, AggressorSide, BookAction, BookType, OmsType, OrderSide, OrderStatus,
        OrderType,
    },
    events::OrderEventAny,
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TradeId, TraderId,
    },
    instruments::{Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt},
    orders::{Order, OrderAny, OrderTestBuilder},
    stubs::TestDefault,
    types::{Price, Quantity},
};

const MARKET_DATA_COUNT: usize = 1_000;
const ORDER_COUNT: usize = 100;
const PASSIVE_ORDER_COUNT: usize = 1_000;
const ITERATION_COUNT: usize = 1_000;
const BASE_TS_NS: u64 = 1_735_689_600_000_000_000;
const ACCOUNT_ID: &str = "ACCOUNT-001";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct EventCounts {
    accepted: usize,
    filled: usize,
    rejected: usize,
    updated: usize,
    canceled: usize,
    unexpected: usize,
}

struct BenchEngine {
    engine: OrderMatchingEngine,
    cache: Rc<RefCell<Cache>>,
    events: Rc<RefCell<EventCounts>>,
}

struct OrderScenario {
    state: BenchEngine,
    orders: Vec<OrderAny>,
}

struct IterateScenario {
    state: BenchEngine,
    iterations: usize,
}

struct CommandScenario<T> {
    state: BenchEngine,
    commands: Vec<T>,
}

fn bench_market_data(c: &mut Criterion) {
    let instrument_id = crypto_perpetual_ethusdt().id();
    let quotes = generate_quotes(instrument_id, MARKET_DATA_COUNT);
    let trades = generate_trades(instrument_id, MARKET_DATA_COUNT);
    let deltas = generate_deltas(instrument_id, MARKET_DATA_COUNT);
    let mut group = c.benchmark_group("matching_engine/market_data");

    group.throughput(Throughput::Elements(MARKET_DATA_COUNT as u64));
    group.bench_function(BenchmarkId::new("quote_l1", MARKET_DATA_COUNT), |b| {
        b.iter_custom(|iters| {
            run_iterations(
                iters,
                || build_engine(BookType::L1_MBP),
                |state| {
                    for quote in &quotes {
                        state.engine.process_quote_tick(black_box(quote));
                    }
                },
                |state| {
                    assert_eq!(
                        state.engine.get_book().update_count,
                        MARKET_DATA_COUNT as u64,
                    );
                    assert_eq!(
                        state.engine.get_book().ts_last,
                        quotes.last().unwrap().ts_event
                    );
                    assert_events(state, EventCounts::default());
                },
            )
        });
    });

    group.bench_function(BenchmarkId::new("trade_l1", MARKET_DATA_COUNT), |b| {
        b.iter_custom(|iters| {
            run_iterations(
                iters,
                || build_engine(BookType::L1_MBP),
                |state| {
                    for trade in &trades {
                        state.engine.process_trade_tick(black_box(trade));
                    }
                },
                |state| {
                    assert_eq!(
                        state.engine.get_book().update_count,
                        MARKET_DATA_COUNT as u64,
                    );
                    assert_eq!(
                        state.engine.get_book().ts_last,
                        trades.last().unwrap().ts_event
                    );
                    assert_events(state, EventCounts::default());
                },
            )
        });
    });

    group.bench_function(BenchmarkId::new("delta_l2", MARKET_DATA_COUNT), |b| {
        b.iter_custom(|iters| {
            run_iterations(
                iters,
                || build_engine(BookType::L2_MBP),
                |state| {
                    for delta in &deltas {
                        state
                            .engine
                            .process_order_book_delta(black_box(delta))
                            .expect("L2 delta should be processed");
                    }
                },
                |state| {
                    assert_eq!(
                        state.engine.get_book().update_count,
                        MARKET_DATA_COUNT as u64,
                    );
                    assert_eq!(
                        state.engine.get_book().ts_last,
                        deltas.last().unwrap().ts_event
                    );
                    assert!(state.engine.get_book().has_ask());
                    assert_events(state, EventCounts::default());
                },
            )
        });
    });

    group.finish();
}

fn bench_submit(c: &mut Criterion) {
    let instrument_id = crypto_perpetual_ethusdt().id();
    let market_orders = generate_orders(instrument_id, OrderType::Market, None, ORDER_COUNT);
    let limit_orders = generate_orders(
        instrument_id,
        OrderType::Limit,
        Some(Price::from("1495.00")),
        ORDER_COUNT,
    );
    let quote = quote(
        instrument_id,
        Price::from("1490.00"),
        Price::from("1500.00"),
        1,
    );
    let ask = ask_delta(instrument_id, Quantity::from("100.000"), 1);
    let mut group = c.benchmark_group("matching_engine/submit");

    group.throughput(Throughput::Elements(ORDER_COUNT as u64));
    group.bench_function(BenchmarkId::new("market_l1", ORDER_COUNT), |b| {
        b.iter_custom(|iters| {
            run_iterations(
                iters,
                || {
                    let mut state = build_engine(BookType::L1_MBP);
                    state.engine.process_quote_tick(&quote);
                    let orders = market_orders.clone();
                    add_orders_to_cache(&state, &orders);
                    OrderScenario { state, orders }
                },
                |scenario| {
                    let account_id = AccountId::from(ACCOUNT_ID);

                    for order in &mut scenario.orders {
                        scenario
                            .state
                            .engine
                            .process_order(black_box(order), account_id);
                    }
                },
                |scenario| {
                    assert_events(
                        &scenario.state,
                        EventCounts {
                            filled: ORDER_COUNT,
                            ..Default::default()
                        },
                    );
                    assert_order_status(&scenario.state, OrderStatus::Filled, ORDER_COUNT);
                    assert_eq!(scenario.state.engine.get_open_orders().len(), 0);
                },
            )
        });
    });

    group.bench_function(BenchmarkId::new("limit_l2", ORDER_COUNT), |b| {
        b.iter_custom(|iters| {
            run_iterations(
                iters,
                || {
                    let mut state = build_engine(BookType::L2_MBP);
                    state
                        .engine
                        .process_order_book_delta(&ask)
                        .expect("L2 ask should be processed");
                    let orders = limit_orders.clone();
                    add_orders_to_cache(&state, &orders);
                    OrderScenario { state, orders }
                },
                |scenario| {
                    let account_id = AccountId::from(ACCOUNT_ID);

                    for order in &mut scenario.orders {
                        scenario
                            .state
                            .engine
                            .process_order(black_box(order), account_id);
                    }
                },
                |scenario| {
                    assert_events(
                        &scenario.state,
                        EventCounts {
                            accepted: ORDER_COUNT,
                            ..Default::default()
                        },
                    );
                    assert_order_status(&scenario.state, OrderStatus::Accepted, ORDER_COUNT);
                    assert_eq!(scenario.state.engine.get_open_orders().len(), ORDER_COUNT);
                },
            )
        });
    });

    group.finish();
}

fn bench_iterate(c: &mut Criterion) {
    let instrument_id = crypto_perpetual_ethusdt().id();
    let orders = generate_orders(
        instrument_id,
        OrderType::Limit,
        Some(Price::from("1495.00")),
        PASSIVE_ORDER_COUNT,
    );
    let ask = ask_delta(instrument_id, Quantity::from("100.000"), 1);
    let mut group = c.benchmark_group("matching_engine/iterate");

    group.throughput(Throughput::Elements(ITERATION_COUNT as u64));
    group.bench_function(
        BenchmarkId::new("passive_no_match_l2", PASSIVE_ORDER_COUNT),
        |b| {
            b.iter_custom(|iters| {
                run_iterations(
                    iters,
                    || {
                        let mut state = build_engine(BookType::L2_MBP);
                        state
                            .engine
                            .process_order_book_delta(&ask)
                            .expect("L2 ask should be processed");
                        let mut seeded_orders = orders.clone();
                        add_orders_to_cache(&state, &seeded_orders);
                        let account_id = AccountId::from(ACCOUNT_ID);
                        for order in &mut seeded_orders {
                            state.engine.process_order(order, account_id);
                        }
                        clear_events(&state);
                        IterateScenario {
                            state,
                            iterations: 0,
                        }
                    },
                    |scenario| {
                        for i in 0..ITERATION_COUNT {
                            scenario.state.engine.iterate(
                                UnixNanos::from(BASE_TS_NS + i as u64),
                                AggressorSide::NoAggressor,
                            );
                            scenario.iterations += 1;
                        }
                        black_box(scenario.iterations);
                    },
                    |scenario| {
                        assert_eq!(scenario.iterations, ITERATION_COUNT);
                        assert_events(&scenario.state, EventCounts::default());
                        assert_order_status(
                            &scenario.state,
                            OrderStatus::Accepted,
                            PASSIVE_ORDER_COUNT,
                        );
                        assert_eq!(
                            scenario.state.engine.get_open_orders().len(),
                            PASSIVE_ORDER_COUNT,
                        );
                    },
                )
            });
        },
    );

    group.finish();
}

fn bench_resting_fill(c: &mut Criterion) {
    let instrument_id = crypto_perpetual_ethusdt().id();
    let orders = generate_orders(
        instrument_id,
        OrderType::Limit,
        Some(Price::from("1495.00")),
        ORDER_COUNT,
    );
    let initial_quote = quote(
        instrument_id,
        Price::from("1490.00"),
        Price::from("1500.00"),
        1,
    );
    let crossing_quote = quote(
        instrument_id,
        Price::from("1494.00"),
        Price::from("1495.00"),
        2,
    );
    let mut group = c.benchmark_group("matching_engine/resting_fill");

    group.throughput(Throughput::Elements(ORDER_COUNT as u64));
    group.bench_function(BenchmarkId::new("quote_l1", ORDER_COUNT), |b| {
        b.iter_custom(|iters| {
            run_iterations(
                iters,
                || {
                    let mut state = build_engine(BookType::L1_MBP);
                    state.engine.process_quote_tick(&initial_quote);
                    let mut seeded_orders = orders.clone();
                    add_orders_to_cache(&state, &seeded_orders);
                    let account_id = AccountId::from(ACCOUNT_ID);
                    for order in &mut seeded_orders {
                        state.engine.process_order(order, account_id);
                    }
                    clear_events(&state);
                    state
                },
                |state| state.engine.process_quote_tick(black_box(&crossing_quote)),
                |state| {
                    assert_events(
                        state,
                        EventCounts {
                            filled: ORDER_COUNT,
                            ..Default::default()
                        },
                    );
                    assert_order_status(state, OrderStatus::Filled, ORDER_COUNT);
                    assert_eq!(state.engine.get_open_orders().len(), 0);
                },
            )
        });
    });

    group.finish();
}

fn bench_commands(c: &mut Criterion) {
    let instrument_id = crypto_perpetual_ethusdt().id();
    let orders = generate_orders(
        instrument_id,
        OrderType::Limit,
        Some(Price::from("1495.00")),
        ORDER_COUNT,
    );
    let ask = ask_delta(instrument_id, Quantity::from("100.000"), 1);
    let modifies = modify_commands(instrument_id, &orders);
    let cancels = cancel_commands(instrument_id, &orders);
    let mut group = c.benchmark_group("matching_engine/commands");

    group.throughput(Throughput::Elements(ORDER_COUNT as u64));
    group.bench_function(BenchmarkId::new("modify_l2", ORDER_COUNT), |b| {
        b.iter_custom(|iters| {
            run_iterations(
                iters,
                || CommandScenario {
                    state: build_passive_engine(&orders, &ask),
                    commands: modifies.clone(),
                },
                |scenario| {
                    let account_id = AccountId::from(ACCOUNT_ID);

                    for command in &scenario.commands {
                        scenario
                            .state
                            .engine
                            .process_modify(black_box(command), account_id);
                    }
                },
                |scenario| {
                    assert_events(
                        &scenario.state,
                        EventCounts {
                            updated: ORDER_COUNT,
                            ..Default::default()
                        },
                    );
                    assert_order_status(&scenario.state, OrderStatus::Accepted, ORDER_COUNT);
                    assert_order_price(&scenario.state, Price::from("1494.00"), ORDER_COUNT);
                    assert_eq!(scenario.state.engine.get_open_orders().len(), ORDER_COUNT);
                },
            )
        });
    });

    group.bench_function(BenchmarkId::new("cancel_l2", ORDER_COUNT), |b| {
        b.iter_custom(|iters| {
            run_iterations(
                iters,
                || CommandScenario {
                    state: build_passive_engine(&orders, &ask),
                    commands: cancels.clone(),
                },
                |scenario| {
                    let account_id = AccountId::from(ACCOUNT_ID);

                    for command in &scenario.commands {
                        scenario
                            .state
                            .engine
                            .process_cancel(black_box(command), account_id);
                    }
                },
                |scenario| {
                    assert_events(
                        &scenario.state,
                        EventCounts {
                            canceled: ORDER_COUNT,
                            ..Default::default()
                        },
                    );
                    assert_order_status(&scenario.state, OrderStatus::Canceled, ORDER_COUNT);
                    assert_eq!(scenario.state.engine.get_open_orders().len(), 0);
                },
            )
        });
    });

    group.finish();
}

fn run_iterations<S, Setup, Workload, Verify>(
    iters: u64,
    mut setup: Setup,
    mut workload: Workload,
    mut verify: Verify,
) -> Duration
where
    Setup: FnMut() -> S,
    Workload: FnMut(&mut S),
    Verify: FnMut(&S),
{
    let mut elapsed = Duration::ZERO;

    for _ in 0..iters {
        let mut state = setup();
        let started = Instant::now();
        workload(&mut state);
        elapsed += started.elapsed();
        verify(&state);
    }

    elapsed
}

fn build_engine(book_type: BookType) -> BenchEngine {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let cache = Rc::new(RefCell::new(Cache::default()));
    let events = Rc::new(RefCell::new(EventCounts::default()));
    let event_cache = cache.clone();
    let event_counts = events.clone();
    let mut engine = OrderMatchingEngine::new(
        instrument,
        1,
        FillModelHandle::default(),
        FeeModelAny::default().into(),
        book_type,
        OmsType::Netting,
        AccountType::Margin,
        Rc::new(RefCell::new(TestClock::new())),
        cache.clone(),
        OrderMatchingEngineConfig::default(),
    );
    engine.set_event_handler(Rc::new(move |event| {
        event_cache
            .borrow_mut()
            .update_order(&event)
            .expect("benchmark order event should update the cache");
        let mut counts = event_counts.borrow_mut();
        match event {
            OrderEventAny::Accepted(_) => counts.accepted += 1,
            OrderEventAny::Filled(_) => counts.filled += 1,
            OrderEventAny::Rejected(_)
            | OrderEventAny::ModifyRejected(_)
            | OrderEventAny::CancelRejected(_) => counts.rejected += 1,
            OrderEventAny::Updated(_) => counts.updated += 1,
            OrderEventAny::Canceled(_) => counts.canceled += 1,
            _ => counts.unexpected += 1,
        }
    }));

    BenchEngine {
        engine,
        cache,
        events,
    }
}

fn build_passive_engine(orders: &[OrderAny], ask: &OrderBookDelta) -> BenchEngine {
    let mut state = build_engine(BookType::L2_MBP);
    state
        .engine
        .process_order_book_delta(ask)
        .expect("L2 ask should be processed");
    let mut seeded_orders = orders.to_vec();
    add_orders_to_cache(&state, &seeded_orders);
    let account_id = AccountId::from(ACCOUNT_ID);
    for order in &mut seeded_orders {
        state.engine.process_order(order, account_id);
    }
    clear_events(&state);
    state
}

fn add_orders_to_cache(state: &BenchEngine, orders: &[OrderAny]) {
    let mut cache = state.cache.borrow_mut();
    for order in orders {
        cache
            .add_order(order.clone(), None, None, false)
            .expect("benchmark order should be added to the cache");
    }
}

fn clear_events(state: &BenchEngine) {
    *state.events.borrow_mut() = EventCounts::default();
}

fn assert_events(state: &BenchEngine, expected: EventCounts) {
    assert_eq!(*state.events.borrow(), expected);
}

fn assert_order_status(state: &BenchEngine, status: OrderStatus, expected: usize) {
    let cache = state.cache.borrow();
    let orders = cache.orders(None, None, None, None, None);
    assert_eq!(orders.len(), expected);
    assert_eq!(
        orders
            .iter()
            .filter(|order| order.status() == status)
            .count(),
        expected,
    );
}

fn assert_order_price(state: &BenchEngine, price: Price, expected: usize) {
    let cache = state.cache.borrow();
    let orders = cache.orders(None, None, None, None, None);
    assert_eq!(orders.len(), expected);
    for order in orders {
        assert_eq!(order.price(), Some(price));
    }
}

fn generate_quotes(instrument_id: InstrumentId, count: usize) -> Vec<QuoteTick> {
    (0..count)
        .map(|i| {
            let bid_cents = 149_000 + i as i64 % 100;
            quote(
                instrument_id,
                price_from_cents(bid_cents),
                price_from_cents(bid_cents + 10),
                i as u64 + 1,
            )
        })
        .collect()
}

fn generate_trades(instrument_id: InstrumentId, count: usize) -> Vec<TradeTick> {
    (0..count)
        .map(|i| {
            let ts = UnixNanos::from(BASE_TS_NS + i as u64);
            TradeTick::new(
                instrument_id,
                price_from_cents(149_005 + i as i64 % 100),
                Quantity::from("1.000"),
                if i.is_multiple_of(2) {
                    AggressorSide::Buyer
                } else {
                    AggressorSide::Seller
                },
                TradeId::from(format!("T-{i}").as_str()),
                ts,
                ts,
            )
        })
        .collect()
}

fn generate_deltas(instrument_id: InstrumentId, count: usize) -> Vec<OrderBookDelta> {
    (0..count)
        .map(|i| {
            let ts = UnixNanos::from(BASE_TS_NS + i as u64);
            OrderBookDeltaTestBuilder::new(instrument_id)
                .book_action(BookAction::Add)
                .book_order(BookOrder::new(
                    OrderSide::Sell,
                    price_from_cents(150_000 + i as i64),
                    Quantity::from("1.000"),
                    i as u64 + 1,
                ))
                .sequence(i as u64 + 1)
                .ts_event(ts)
                .ts_init(ts)
                .build()
        })
        .collect()
}

fn generate_orders(
    instrument_id: InstrumentId,
    order_type: OrderType,
    price: Option<Price>,
    count: usize,
) -> Vec<OrderAny> {
    (0..count)
        .map(|i| {
            let mut builder = OrderTestBuilder::new(order_type);
            builder
                .instrument_id(instrument_id)
                .side(OrderSide::Buy)
                .quantity(Quantity::from("1.000"))
                .client_order_id(ClientOrderId::from(format!("O-BENCH-{i:06}").as_str()))
                .submit(true);

            if let Some(price) = price {
                builder.price(price);
            }
            builder.build()
        })
        .collect()
}

fn modify_commands(instrument_id: InstrumentId, orders: &[OrderAny]) -> Vec<ModifyOrder> {
    orders
        .iter()
        .map(|order| {
            ModifyOrder::new(
                TraderId::test_default(),
                Some(ClientId::from("CLIENT-001")),
                StrategyId::test_default(),
                instrument_id,
                order.client_order_id(),
                None,
                None,
                Some(Price::from("1494.00")),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            )
        })
        .collect()
}

fn cancel_commands(instrument_id: InstrumentId, orders: &[OrderAny]) -> Vec<CancelOrder> {
    orders
        .iter()
        .map(|order| {
            CancelOrder::new(
                TraderId::test_default(),
                Some(ClientId::from("CLIENT-001")),
                StrategyId::test_default(),
                instrument_id,
                order.client_order_id(),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            )
        })
        .collect()
}

fn quote(instrument_id: InstrumentId, bid: Price, ask: Price, sequence: u64) -> QuoteTick {
    let ts = UnixNanos::from(BASE_TS_NS + sequence);
    QuoteTick::new(
        instrument_id,
        bid,
        ask,
        Quantity::from("100.000"),
        Quantity::from("100.000"),
        ts,
        ts,
    )
}

fn ask_delta(instrument_id: InstrumentId, size: Quantity, sequence: u64) -> OrderBookDelta {
    let ts = UnixNanos::from(BASE_TS_NS + sequence);
    OrderBookDeltaTestBuilder::new(instrument_id)
        .book_action(BookAction::Add)
        .book_order(BookOrder::new(
            OrderSide::Sell,
            Price::from("1500.00"),
            size,
            sequence,
        ))
        .sequence(sequence)
        .ts_event(ts)
        .ts_init(ts)
        .build()
}

fn price_from_cents(cents: i64) -> Price {
    Price::from(format!("{}.{:02}", cents / 100, cents % 100).as_str())
}

criterion_group!(
    benches,
    bench_market_data,
    bench_submit,
    bench_iterate,
    bench_resting_fill,
    bench_commands,
);
criterion_main!(benches);
