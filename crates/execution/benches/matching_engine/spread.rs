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

use std::{cell::RefCell, hint::black_box, rc::Rc};

use criterion::{BenchmarkId, Criterion, Throughput};
use nautilus_common::{cache::Cache, clock::VirtualClock};
use nautilus_core::UnixNanos;
use nautilus_execution::{
    matching_engine::{OrderMatchingEngine, config::OrderMatchingEngineConfig},
    models::{
        fee::{FeeModelHandle, PerContractFeeModel},
        fill::FillModelHandle,
    },
};
use nautilus_model::{
    accounts::MarginAccount,
    data::{BookOrder, QuoteTick, stubs::OrderBookDeltaTestBuilder},
    enums::{
        AccountType, BookAction, BookType, OmsType, OrderSide, OrderStatus, OrderType, TimeInForce,
    },
    events::{OrderEventAny, account::stubs::margin_account_state},
    identifiers::{AccountId, ClientOrderId, InstrumentId, Symbol, new_generic_spread_id},
    instruments::{
        CryptoFuture, CryptoFuturesSpread, Instrument, TickScheme, TieredTickScheme,
        register_tick_scheme,
    },
    orders::{Order, OrderAny, OrderTestBuilder},
    types::{Currency, Money, Price, Quantity},
};

use super::run_iterations;

pub(super) fn bench_spreads(c: &mut Criterion) {
    register_tick_scheme(
        "BENCH_SPREAD_TIERS",
        TickScheme::Tiered(
            TieredTickScheme::new(&[(0.01, 3.0, 0.01), (3.0, 10.0, 0.05)], 2, 1_000).unwrap(),
        ),
    )
    .unwrap();
    let mut group = c.benchmark_group("matching_engine/spread");
    for case in cases() {
        group.throughput(Throughput::Elements(case.orders as u64));
        group.bench_function(BenchmarkId::new(case.name, case.orders), |b| {
            let mut smoke = setup(&case);
            process_orders(&mut smoke);
            verify(&case, &smoke);
            b.iter_custom(|iters| {
                run_iterations(
                    iters,
                    || setup(&case),
                    process_orders,
                    |state| verify(&case, state),
                )
            });
        });
    }

    group.finish();
}

struct Case {
    name: &'static str,
    legs: Vec<Leg>,
    target: &'static str,
    expected: Vec<&'static str>,
    orders: usize,
    levels: usize,
    order_type: OrderType,
    time_in_force: TimeInForce,
}

struct Leg {
    ratio: i64,
    bid: &'static str,
    ask: &'static str,
    scheme: Option<&'static str>,
}

struct Scenario {
    engine: OrderMatchingEngine,
    cache: Rc<RefCell<Cache>>,
    events: Rc<RefCell<Vec<OrderEventAny>>>,
    orders: Vec<OrderAny>,
    legs: Vec<InstrumentId>,
}

fn cases() -> Vec<Case> {
    let anchor = || Case {
        name: "anchor",
        legs: vec![
            leg(1, "1.010", "1.020", None),
            leg(-1, "0.510", "0.520", None),
        ],
        target: "0.500",
        expected: vec!["1.020", "0.520"],
        orders: 100,
        levels: 1,
        order_type: OrderType::Market,
        time_in_force: TimeInForce::Gtc,
    };

    let mut mtl = anchor();
    mtl.name = "market_to_limit";
    mtl.order_type = OrderType::MarketToLimit;
    let mut fok = anchor();
    fok.name = "fok_single_execution";
    let mut levels = anchor();
    levels.name = "three_levels";
    fok.time_in_force = TimeInForce::Fok;
    levels.levels = 3;
    levels.expected = vec!["1.020", "0.520", "1.020", "0.510", "1.030", "0.510"];
    let mut fixed = anchor();
    fixed.name = "fixed_pair";
    fixed.legs = vec![
        leg(2, "100.000", "100.000", None),
        leg(5, "50.000", "50.000", None),
        leg(-3, "80.000", "80.000", None),
    ];
    fixed.target = "210.010";
    fixed.expected = vec!["99.990", "50.000", "79.990"];
    let mut four = anchor();
    four.name = "four_legs";
    four.legs = vec![
        leg(1, "10.010", "10.020", None),
        leg(-1, "5.010", "5.020", None),
        leg(2, "2.010", "2.020", None),
        leg(-2, "1.010", "1.020", None),
    ];
    four.target = "7.000";
    four.expected = vec!["10.020", "5.020", "2.020", "1.020"];
    let mut tiered = anchor();
    tiered.name = "tiered_pair";
    tiered.legs = vec![
        leg(1, "3.450", "3.550", Some("BENCH_SPREAD_TIERS")),
        leg(-2, "1.190", "1.210", Some("BENCH_SPREAD_TIERS")),
    ];
    tiered.target = "1.110";
    tiered.expected = vec!["3.550", "1.220"];
    let mut betfair = anchor();
    betfair.name = "betfair_no_solution";
    betfair.legs = vec![
        leg(1, "3.000", "3.000", Some("BETFAIR")),
        leg(-1, "3.000", "3.000", Some("BETFAIR")),
    ];
    betfair.target = "0.001";
    betfair.expected.clear();
    betfair.orders = 16;
    let mut topix = anchor();
    topix.name = "topix_no_solution";
    topix.legs = vec![
        leg(1, "3000.000", "3000.000", Some("TOPIX100")),
        leg(-1, "3000.000", "3000.000", Some("TOPIX100")),
    ];
    topix.target = "0.010";
    topix.expected.clear();
    topix.orders = 1;
    vec![
        anchor(),
        fixed,
        four,
        tiered,
        fok,
        levels,
        mtl,
        betfair,
        topix,
    ]
}

fn leg(ratio: i64, bid: &'static str, ask: &'static str, scheme: Option<&'static str>) -> Leg {
    Leg {
        ratio,
        bid,
        ask,
        scheme,
    }
}

fn setup(case: &Case) -> Scenario {
    let account_id = AccountId::from("ACCOUNT-001");
    let cache = Rc::new(RefCell::new(Cache::default()));
    let mut account = margin_account_state();
    account.account_id = account_id;
    cache
        .borrow_mut()
        .add_account(MarginAccount::new(account, false).into())
        .unwrap();

    let legs: Vec<_> = case
        .legs
        .iter()
        .enumerate()
        .map(|(index, spec)| {
            let id = InstrumentId::from(format!("L{index}.SIM"));
            let mut instrument = CryptoFuture::builder()
                .instrument_id(id)
                .raw_symbol(Symbol::from(format!("L{index}")))
                .underlying(Currency::BTC())
                .quote_currency(Currency::USD())
                .settlement_currency(Currency::USD())
                .is_inverse(false)
                .activation_ns(0.into())
                .expiration_ns(100_000_000_000.into())
                .price_precision(3)
                .size_precision(0)
                .price_increment(Price::from("0.010"))
                .size_increment(Quantity::from(1))
                .ts_event(0.into())
                .ts_init(0.into())
                .build()
                .unwrap();
            instrument.tick_scheme = spec.scheme.map(Into::into);
            cache
                .borrow_mut()
                .add_instrument(instrument.into())
                .unwrap();
            cache
                .borrow_mut()
                .add_quote(quote(
                    id,
                    Price::from(spec.bid),
                    Price::from(spec.ask),
                    Quantity::from(100),
                ))
                .unwrap();
            id
        })
        .collect();

    let tuples: Vec<_> = legs
        .iter()
        .copied()
        .zip(case.legs.iter().map(|leg| leg.ratio))
        .collect();
    let spread = CryptoFuturesSpread::builder()
        .instrument_id(new_generic_spread_id(&tuples).unwrap())
        .raw_symbol(Symbol::from("SPREAD"))
        .underlying(Currency::BTC())
        .quote_currency(Currency::USD())
        .settlement_currency(Currency::USD())
        .is_inverse(false)
        .strategy_type("SPREAD".into())
        .activation_ns(0.into())
        .expiration_ns(100_000_000_000.into())
        .price_precision(3)
        .size_precision(0)
        .price_increment(Price::from("0.001"))
        .size_increment(Quantity::from(1))
        .ts_event(0.into())
        .ts_init(0.into())
        .build()
        .unwrap();
    let id = spread.id();
    cache
        .borrow_mut()
        .add_instrument(spread.clone().into())
        .unwrap();

    let events = Rc::new(RefCell::new(Vec::with_capacity(
        case.orders * case.levels * (legs.len() + 2),
    )));

    let saved = events.clone();
    let event_cache = cache.clone();

    let mut engine = OrderMatchingEngine::new(
        spread.into(),
        1,
        FillModelHandle::default(),
        FeeModelHandle::new(PerContractFeeModel::new(Money::from("1 USD")).unwrap()),
        if case.levels == 1 {
            BookType::L1_MBP
        } else {
            BookType::L2_MBP
        },
        OmsType::Netting,
        AccountType::Margin,
        Rc::new(RefCell::new(VirtualClock::new())),
        cache.clone(),
        OrderMatchingEngineConfig::default(),
    );

    engine.set_event_handler(Rc::new(move |event| {
        if event.instrument_id() == id {
            event_cache.borrow_mut().update_order(&event).unwrap();
        }

        saved.borrow_mut().push(event);
    }));

    if case.levels == 1 {
        engine.process_quote_tick(&quote(
            id,
            Price::from(case.target),
            Price::from(case.target),
            Quantity::from(100),
        ));
    } else {
        for level in 0..case.levels {
            let price = Price::from_decimal_dp(
                Price::from(case.target).as_decimal() + rust_decimal::Decimal::new(level as i64, 2),
                3,
            )
            .unwrap();
            let delta = OrderBookDeltaTestBuilder::new(id)
                .book_action(BookAction::Add)
                .book_order(BookOrder::new(
                    OrderSide::Sell,
                    price,
                    Quantity::from(1),
                    level as u64 + 1,
                ))
                .build();
            engine.process_order_book_delta(&delta).unwrap();
        }
    }

    let orders = (0..case.orders)
        .map(|index| {
            let order = OrderTestBuilder::new(case.order_type)
                .instrument_id(id)
                .client_order_id(ClientOrderId::from(format!("O-{index}")))
                .side(OrderSide::Buy)
                .quantity(Quantity::from(case.levels as u64))
                .time_in_force(case.time_in_force)
                .submit(true)
                .build();
            cache
                .borrow_mut()
                .add_order(order.clone(), None, None, false)
                .unwrap();
            order
        })
        .collect();

    Scenario {
        engine,
        cache,
        events,
        orders,
        legs,
    }
}

// Keep this boundary visible to allocation profiles, which also observe untimed setup
#[inline(never)]
fn process_orders(state: &mut Scenario) {
    for order in &mut state.orders {
        state
            .engine
            .process_order(black_box(order), AccountId::from("ACCOUNT-001"));
    }
}

fn verify(case: &Case, state: &Scenario) {
    let events = state.events.borrow();

    let fills: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill),
            _ => None,
        })
        .collect();

    let cache = state.cache.borrow();

    if case.expected.is_empty() {
        assert_eq!(fills.len(), 0);
        assert_eq!(events.len(), case.orders);
        assert!(
            events
                .iter()
                .all(|event| matches!(event, OrderEventAny::Canceled(_)))
        );

        for order in &state.orders {
            let order = cache.order(&order.client_order_id()).unwrap();
            assert_eq!(order.status(), OrderStatus::Canceled);
            assert_eq!(order.filled_qty(), Quantity::from(0));
        }

        return;
    }

    assert_eq!(
        fills.len(),
        case.orders * case.levels * (case.legs.len() + 1)
    );
    let updates = usize::from(case.order_type == OrderType::MarketToLimit) * case.orders;
    assert_eq!(events.len(), fills.len() + updates);

    for (order_index, batch) in fills
        .chunks_exact(case.levels * (case.legs.len() + 1))
        .enumerate()
    {
        for (level, chunk) in batch.chunks_exact(case.legs.len() + 1).enumerate() {
            let mut commission = Money::from("0 USD");

            for (index, fill) in chunk[..case.legs.len()].iter().enumerate() {
                let spec = &case.legs[index];
                assert_eq!(fill.instrument_id, state.legs[index]);
                assert_eq!(
                    fill.last_px,
                    Price::from(case.expected[level * case.legs.len() + index])
                );
                assert_eq!(fill.last_qty, Quantity::from(spec.ratio.unsigned_abs()));
                assert_eq!(
                    fill.order_side,
                    if spec.ratio > 0 {
                        OrderSide::Buy
                    } else {
                        OrderSide::Sell
                    }
                );
                let expected = Money::from_decimal(
                    rust_decimal::Decimal::from(spec.ratio.unsigned_abs()),
                    Currency::USD(),
                )
                .unwrap();
                assert_eq!(fill.commission, Some(expected));
                commission = commission.checked_add(expected).unwrap();
            }

            let parent = chunk.last().unwrap();
            assert_eq!(
                parent.client_order_id,
                state.orders[order_index].client_order_id()
            );
            assert_eq!(parent.last_qty, Quantity::from(1));
            assert_eq!(
                parent.last_px.as_decimal(),
                Price::from(case.target).as_decimal() + rust_decimal::Decimal::new(level as i64, 2)
            );
            assert_eq!(parent.commission, Some(commission));
        }

        let order = cache
            .order(&state.orders[order_index].client_order_id())
            .unwrap();
        assert_eq!(order.status(), OrderStatus::Filled);
        assert_eq!(order.filled_qty(), Quantity::from(case.levels as u64));

        if case.order_type == OrderType::MarketToLimit {
            assert_eq!(order.price(), Some(Price::from(case.target)));
        }
    }
}

fn quote(id: InstrumentId, bid: Price, ask: Price, size: Quantity) -> QuoteTick {
    QuoteTick::new(
        id,
        bid,
        ask,
        size,
        size,
        UnixNanos::default(),
        UnixNanos::default(),
    )
}
