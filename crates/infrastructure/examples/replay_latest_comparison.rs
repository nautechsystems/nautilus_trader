// -------------------------------------------------------------------------------------------------
// Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
// https://nautechsystems.io
// Licensed under the GNU Lesser General Public License Version 3.0.
// https://www.gnu.org/licenses/lgpl-3.0.en.html
// -------------------------------------------------------------------------------------------------

//! Measures offline continuing fills or repeated flips through the real execution engine.
//!
//! Redis mode requires a disposable loopback Redis instance supplied by the runner.
//! Timings include adapter submission, not Redis acknowledgement or durable persistence.
use std::{
    cell::RefCell,
    hint::black_box,
    rc::Rc,
    time::{Duration, Instant},
};

use nautilus_common::{
    cache::{Cache, CacheConfig, database::CacheDatabaseAdapter},
    clock::VirtualClock,
    live::get_runtime,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_execution::engine::{
    ExecutionEngine, config::ExecutionEngineConfig, stubs::StubExecutionClient,
};
use nautilus_infrastructure::redis::cache::{
    RedisCacheConfig, RedisCacheDatabase, RedisCacheDatabaseAdapter,
};
use nautilus_model::{
    accounts::AccountAny,
    enums::{LiquiditySide, OmsType, OrderSide, OrderType, PositionSide},
    events::{OrderEventAny, OrderFilled, order::spec::OrderFilledSpec},
    identifiers::{AccountId, ClientId, ClientOrderId, PositionId, TradeId, TraderId},
    instruments::{Instrument, InstrumentAny, stubs::audusd_sim},
    orders::{Order, builder::OrderTestBuilder, stubs::TestOrderEventStubs},
    position::Position,
    types::{Currency, Price, Quantity},
};

fn adapter(port: u16, trader: TraderId) -> RedisCacheDatabaseAdapter {
    assert!(
        port >= 20000,
        "only the runner's isolated loopback Redis is allowed"
    );
    let database = get_runtime()
        .block_on(RedisCacheDatabase::new(
            trader,
            UUID4::new(),
            CacheConfig::default(),
            RedisCacheConfig {
                host: Some("127.0.0.1".into()),
                port: Some(port),
                number_of_retries: 0,
                connection_timeout: 2,
                response_timeout: 10,
                ..Default::default()
            },
        ))
        .unwrap();
    RedisCacheDatabaseAdapter { database }
}
fn seed_fill(i: usize, phase: &str, side: OrderSide, qty: usize, id: PositionId) -> OrderFilled {
    OrderFilledSpec::builder()
        .instrument_id(audusd_sim().id())
        .client_order_id(ClientOrderId::from(format!("O-{phase}-{i}")))
        .trade_id(TradeId::from(format!("T-{phase}-{i}")))
        .order_side(side)
        .last_qty(Quantity::from(qty as u64))
        .last_px(Price::from("1.00000"))
        .currency(Currency::USD())
        .liquidity_side(LiquiditySide::Maker)
        .position_id(id)
        .build()
}
fn seed(n: usize, id: PositionId, instrument: &InstrumentAny) -> Position {
    let mut p = Position::new(instrument, seed_fill(0, "OPEN", OrderSide::Buy, 1, id));
    for i in 1..n {
        p.apply(&seed_fill(i, "OPEN", OrderSide::Buy, 1, id));
    }
    p.apply(&seed_fill(n, "CLOSE", OrderSide::Sell, n, id));
    p.apply(&seed_fill(n + 1, "REOPEN", OrderSide::Buy, 1, id));
    assert_eq!(p.replay_events.len(), n + 2);
    assert_eq!(p.events.len(), 1);
    p
}
struct CandidateBuilder<'a> {
    engine: &'a mut ExecutionEngine,
    cache: &'a Rc<RefCell<Cache>>,
    instrument: &'a InstrumentAny,
    account: AccountId,
    client: ClientId,
    position: PositionId,
}
impl CandidateBuilder<'_> {
    fn event(&mut self, name: &str, trade: TradeId, side: OrderSide, qty: usize) -> OrderEventAny {
        let mut order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(self.instrument.id())
            .client_order_id(ClientOrderId::from(format!("O-{name}")))
            .side(side)
            .quantity(Quantity::from(qty as u64))
            .build();
        self.cache
            .borrow_mut()
            .add_order(order.clone(), Some(self.position), Some(self.client), false)
            .unwrap();
        let submitted = TestOrderEventStubs::submitted(&order, self.account);
        self.engine.process(&submitted);
        order.apply(submitted).unwrap();
        let accepted =
            TestOrderEventStubs::accepted(&order, self.account, format!("V-{name}").into());
        self.engine.process(&accepted);
        order.apply(accepted).unwrap();
        TestOrderEventStubs::filled(
            &order,
            self.instrument,
            Some(trade),
            Some(self.position),
            Some(Price::from("1.00000")),
            Some(Quantity::from(qty as u64)),
            Some(LiquiditySide::Maker),
            None,
            None,
            Some(self.account),
        )
    }
}
struct Step {
    flip: OrderEventAny,
    unique: OrderEventAny,
    oldest: OrderEventAny,
    latest: OrderEventAny,
}
fn check(cache: &Cache, id: PositionId, step: usize, n: usize, unique: bool) {
    let p = cache.position(&id).unwrap();
    assert_eq!(
        p.side,
        if step.is_multiple_of(2) {
            PositionSide::Short
        } else {
            PositionSide::Long
        }
    );
    assert_eq!(p.quantity, Quantity::from(if unique { 2 } else { 1 }));
    assert_eq!(p.events.len(), if unique { 2 } else { 1 });
    assert_eq!(p.trade_ids.len(), if unique { 2 } else { 1 });
    assert_eq!(
        p.replay_events.len(),
        n + 2 + step * 3 + 2 + usize::from(unique)
    );
    assert_eq!(cache.position_snapshot_count(&id), step + 1);
}
fn state(cache: &Cache, id: PositionId, full: bool) -> serde_json::Value {
    let p = cache.position(&id).unwrap();
    if full {
        serde_json::to_value(&*p).unwrap()
    } else {
        serde_json::to_value(p.clone_without_events()).unwrap()
    }
}
fn process(engine: &mut ExecutionEngine, event: &OrderEventAny) -> u128 {
    let t = Instant::now();
    engine.process(black_box(event));
    t.elapsed().as_nanos()
}
fn writer_barrier(cache: &Cache, port: u16, trader_key: &str) {
    cache.heartbeat(UnixNanos::from(123456u64)).unwrap();
    let client = redis::Client::open(format!("redis://127.0.0.1:{port}/")).unwrap();
    let mut conn = client.get_connection().unwrap();
    let key = format!("{trader_key}:health:heartbeat");
    for _ in 0..2000 {
        let exists: bool = redis::cmd("EXISTS").arg(&key).query(&mut conn).unwrap();
        if exists {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("private Redis writer did not acknowledge fixture setup");
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert!(
        (8..=9).contains(&args.len()),
        "usage: replay_latest_comparison HISTORY BACKEND PORT cold|warm \
         unique|duplicate|unique-only COUNT full|economics [flip|continue]"
    );
    let n: usize = args[1].parse().unwrap();
    assert!(n > 0);
    let backend = &args[2];
    assert!(["memory", "redis-nonvirtual", "redis-virtual"].contains(&backend.as_str()));
    let port: u16 = args[3].parse().unwrap();
    let start = &args[4];
    assert!(["cold", "warm"].contains(&start.as_str()));
    let first = &args[5];
    assert!(["unique", "duplicate", "unique-only"].contains(&first.as_str()));
    let count: usize = args[6].parse().unwrap();
    assert!(count > 0);
    assert!(["full", "economics"].contains(&args[7].as_str()));
    let full = args[7] == "full";
    let mode = args.get(8).map_or("flip", String::as_str);
    assert!(["flip", "continue"].contains(&mode));
    let continuing = mode == "continue";
    assert!(!continuing || first == "unique-only");
    let virtual_id = backend == "redis-virtual";
    let id = PositionId::from(if virtual_id {
        "P-REPEAT-FLIP"
    } else {
        "AUDUSD-REPEAT-FLIP"
    });
    let instrument = InstrumentAny::CurrencyPair(audusd_sim());
    let account = AccountAny::default();
    let account_id = account.id();
    let client_id = ClientId::from("REPEAT-FLIP");
    let trader = TraderId::from(format!("REPEAT-{}", UUID4::new()));
    let position = seed(n, id, &instrument);
    let cache = Rc::new(RefCell::new(Cache::default()));
    {
        let mut c = cache.borrow_mut();
        c.add_instrument(instrument.clone()).unwrap();
        c.add_account(account).unwrap();
        c.add_position_without_order(&position, OmsType::Netting)
            .unwrap();
    }
    let trader_key = if backend == "memory" {
        None
    } else {
        let a = adapter(port, trader);
        // Seed persistence separately: its predicate may warm this source, never the cache clone.
        a.add_instrument(&instrument).unwrap();
        a.add_position(&position).unwrap();
        let key = a.database.get_trader_key().to_string();
        cache.borrow_mut().set_database(Box::new(a));
        Some(key)
    };
    let config = ExecutionEngineConfig::builder()
        .carry_replay_events_on_reopen(true)
        .build()
        .unwrap();
    let clock = Rc::new(RefCell::new(VirtualClock::new()));
    let mut engine = ExecutionEngine::new(clock.clone(), Rc::clone(&cache), Some(config));
    engine
        .register_client(Box::new(StubExecutionClient::new(
            client_id,
            account_id,
            instrument.id().venue,
            OmsType::Netting,
            Some(clock),
        )))
        .unwrap();
    let mut builder = CandidateBuilder {
        engine: &mut engine,
        cache: &cache,
        instrument: &instrument,
        account: account_id,
        client: client_id,
        position: id,
    };
    let mut steps = vec![];
    let mut continuing_events = vec![];

    if continuing {
        for step in 0..count {
            continuing_events.push(builder.event(
                &format!("CONTINUE-{step}"),
                TradeId::from(format!("T-CONTINUE-{step}")),
                OrderSide::Buy,
                1,
            ));
        }
    }

    for step in 0..if continuing { 0 } else { count } {
        let side = if step.is_multiple_of(2) {
            OrderSide::Sell
        } else {
            OrderSide::Buy
        };
        steps.push(Step {
            flip: builder.event(
                &format!("FLIP-{step}"),
                TradeId::from(format!("T-FLIP-{step}")),
                side,
                if step == 0 { 2 } else { 3 },
            ),
            unique: builder.event(
                &format!("UNIQUE-{step}"),
                TradeId::from(format!("T-UNIQUE-{step}")),
                side,
                1,
            ),
            oldest: builder.event(
                &format!("OLDEST-{step}"),
                TradeId::from("T-OPEN-0"),
                side,
                1,
            ),
            latest: builder.event(
                &format!("LATEST-{step}"),
                TradeId::from(if step == 0 {
                    format!("T-OPEN-{}", n - 1)
                } else {
                    format!("T-UNIQUE-{}", step - 1)
                }),
                side,
                1,
            ),
        });
    }

    if let Some(key) = &trader_key {
        writer_barrier(&cache.borrow(), port, key);
    }

    if start == "warm" {
        let c = cache.borrow();
        let p = c.position(&id).unwrap();
        black_box(p.has_replay_trade_id(TradeId::from("MISSING")));
        black_box(p.requires_replay_state());
    }
    // Cold means a genuine cold canonical index at the first fill, not a forced reset each step.
    // Subsequent flips inherit exactly the state left by the previous engine/adapter operations.
    println!("backend,start,first,history_fills,step,operation,elapsed_ns");
    for (step, event) in continuing_events.iter().enumerate() {
        let ns = process(&mut engine, event);
        println!("{backend},{start},{first},{n},{step},continuing_fill,{ns}");
        let c = cache.borrow();
        let p = c.position(&id).unwrap();
        assert_eq!(p.side, PositionSide::Long);
        assert_eq!(p.quantity, Quantity::from((step + 2) as u64));
        assert_eq!(p.events.len(), step + 2);
        assert_eq!(p.replay_events.len(), n + step + 3);
        assert_eq!(c.position_snapshot_count(&id), 0);
        drop(p);
        drop(c);

        if full {
            let before = state(&cache.borrow(), id, true);
            engine.process(event);
            assert_eq!(state(&cache.borrow(), id, true), before);
        }
    }

    for (step, events) in steps.iter().enumerate() {
        let ns = process(&mut engine, &events.flip);
        println!("{backend},{start},{first},{n},{step},flip,{ns}");
        check(&cache.borrow(), id, step, n, false);
        let mut unique_applied = false;
        let mut ordered = if first == "unique-only" {
            vec![("unique_first", &events.unique, true)]
        } else if first == "unique" {
            vec![
                ("unique_first", &events.unique, true),
                ("oldest_after_unique", &events.oldest, false),
            ]
        } else {
            vec![
                ("oldest_first", &events.oldest, false),
                ("unique_after_duplicate", &events.unique, true),
            ]
        };

        if first != "unique-only" {
            ordered.push(("latest_duplicate_warm", &events.latest, false));
        }

        for (op, event, is_unique) in ordered {
            let before = (!is_unique).then(|| state(&cache.borrow(), id, full));
            let ns = process(&mut engine, event);
            println!("{backend},{start},{first},{n},{step},{op},{ns}");

            if is_unique {
                unique_applied = true;
            }
            check(&cache.borrow(), id, step, n, unique_applied);

            if let Some(before) = before {
                assert_eq!(
                    state(&cache.borrow(), id, full),
                    before,
                    "historical duplicate changed position"
                );
            }
        }

        if full {
            let before = state(&cache.borrow(), id, true);
            engine.process(&events.flip);
            assert_eq!(
                state(&cache.borrow(), id, true),
                before,
                "exact flip duplicate changed position"
            );
        }
    }
    let expected_economics = state(&cache.borrow(), id, false);
    let expected_full = if virtual_id {
        Some(state(&cache.borrow(), id, true))
    } else {
        None
    };
    let t = Instant::now();
    cache.borrow_mut().dispose();
    let drain = t.elapsed().as_nanos();
    println!("{backend},{start},{first},{n},{count},dispose_and_drain,{drain}");
    if backend != "memory" {
        let mut a = adapter(port, trader);
        let t = Instant::now();
        let restored_result = get_runtime().block_on(a.load_position(&id));
        let reload = t.elapsed().as_nanos();
        let restored = match restored_result {
            Ok(Some(position)) => position,
            result => {
                a.close().unwrap();
                println!("{backend},{start},{first},{n},{count},reload_failed,{reload}");
                eprintln!("RELOAD_ERROR {result:?}");
                eprintln!(
                    "PASS in-memory economic and duplicate checks before reload; backend={backend} start={start} first={first} n={n} mode={mode} steps={count} full_json_checks={full}"
                );
                std::process::exit(2);
            }
        };
        assert_eq!(
            serde_json::to_value(restored.clone_without_events()).unwrap(),
            expected_economics,
            "Redis reload changed economics"
        );

        if let Some(expected) = expected_full {
            assert_eq!(
                serde_json::to_value(&restored).unwrap(),
                expected,
                "virtual Redis lost complete replay state"
            );
        }
        a.close().unwrap();
        println!("{backend},{start},{first},{n},{count},reload,{reload}");
        eprintln!("PASS Redis reload economics; full replay equality checked={virtual_id}");
    }
    eprintln!(
        "PASS actual_engine=true backend={backend} start={start} first={first} n={n} mode={mode} steps={count} full_json_checks={full}; fresh-order historical duplicates protect Position, not order status"
    );
}
