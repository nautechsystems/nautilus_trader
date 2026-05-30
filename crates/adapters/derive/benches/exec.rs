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

//! Canonical exec + dispatch benches.
//!
//! `exec_pipeline`: strategy command (`OrderAny` / cancel) -> wire bytes ready
//! to send. `submit_limit`, `submit_market`, and `modify` cover the signed
//! `private/order` and `private/replace` path (normalize + ABI encode +
//! EIP-712 sign + JSON serialize). `cancel` covers the unsigned
//! `private/cancel` path (build + serialize). Derive supports only Limit and
//! Market orders, so there is no stop-order row.
//!
//! `dispatch`: venue WS payload (`DeriveOrdersSubscriptionData`,
//! `DeriveTradesSubscriptionData`) -> events emitted via
//! [`ExecutionEventEmitter`]. Covers parse + dedup + identity lookup + event
//! construction through `dispatch_orders_payload` / `dispatch_trades_payload`.
//! The untracked row forwards a raw status report; the tracked rows resolve a
//! registered identity and emit `OrderAccepted` / `OrderFilled` events.

mod common;

use std::hint::black_box;

use alloy::signers::local::PrivateKeySigner;
use alloy_primitives::{Address, B256};
use common::{account_id, clock, fixtures, trader_id};
use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_derive::{
    common::{
        consts::{ACTION_TYPEHASH, DERIVE_VENUE, domain_separator_for, trade_module_address_for},
        enums::DeriveEnvironment,
    },
    execution::{dispatch_orders_payload, dispatch_trades_payload},
    http::{
        models::DeriveInstrument,
        query::{DeriveCancelParams, order_replace_to_derive_payload, order_to_derive_payload},
    },
    signing::encoding::{parse_address_const, parse_b256_const},
    websocket::{
        dispatch::{OrderIdentity, WsDispatchState},
        messages::{DeriveOrdersSubscriptionData, DeriveTradesSubscriptionData},
    },
};
use nautilus_model::{
    enums::{OrderSide, OrderType, TimeInForce},
    identifiers::{ClientOrderId, InstrumentId, StrategyId, Symbol},
    orders::{LimitOrder, MarketOrder, OrderAny},
    types::{Price, Quantity},
};
use rust_decimal_macros::dec;

const SESSION_KEY: &str = "0x2ae8be44db8a590d20bffbe3b6872df9b569147d3bf6801a35a28281a4816bbd";
const WALLET: &str = "0x000000000000000000000000000000000000aaaa";
const SUBACCOUNT_ID: u64 = 30769;
const NONCE: u64 = 1_700_000_000_000_000;
const EXPIRY_SEC: i64 = 1_900_000_000; // far-future so the EIP-712 expiry guard passes

fn instrument_id() -> InstrumentId {
    InstrumentId::new(Symbol::new("ETH-PERP"), *DERIVE_VENUE)
}

fn strategy_id() -> StrategyId {
    StrategyId::from("S-BENCH")
}

fn signer() -> PrivateKeySigner {
    SESSION_KEY.parse().unwrap()
}

fn wallet() -> Address {
    WALLET.parse().unwrap()
}

fn instrument() -> DeriveInstrument {
    serde_json::from_str(fixtures::INSTRUMENT_PERP).unwrap()
}

fn domain() -> B256 {
    parse_b256_const(
        domain_separator_for(DeriveEnvironment::Mainnet),
        "domain_separator",
    )
    .unwrap()
}

fn typehash() -> B256 {
    parse_b256_const(ACTION_TYPEHASH, "action_typehash").unwrap()
}

fn module() -> Address {
    parse_address_const(
        trade_module_address_for(DeriveEnvironment::Mainnet),
        "trade_module_address",
    )
    .unwrap()
}

fn limit_order(side: OrderSide) -> OrderAny {
    OrderAny::Limit(LimitOrder::new(
        trader_id(),
        strategy_id(),
        instrument_id(),
        ClientOrderId::from("O-BENCH-LIM"),
        side,
        Quantity::from("1"),
        Price::from("3500.0"),
        TimeInForce::Gtc,
        None,
        false,
        false,
        false,
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
        UUID4::new(),
        UnixNanos::default(),
    ))
}

fn market_order(side: OrderSide) -> OrderAny {
    OrderAny::Market(MarketOrder::new(
        trader_id(),
        strategy_id(),
        instrument_id(),
        ClientOrderId::from("O-BENCH-MKT"),
        side,
        Quantity::from("1"),
        TimeInForce::Ioc,
        UUID4::new(),
        UnixNanos::default(),
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    ))
}

fn bench_submit_limit(c: &mut Criterion) {
    let signer = signer();
    let instrument = instrument();
    let wallet = wallet();
    let (module, domain, typehash) = (module(), domain(), typehash());
    let order = limit_order(OrderSide::Buy);
    let max_fee = dec!(1000);

    let mut group = c.benchmark_group("exec_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("submit_limit", |b| {
        b.iter(|| {
            let params = order_to_derive_payload(
                black_box(&order),
                &instrument,
                SUBACCOUNT_ID,
                wallet,
                &signer,
                NONCE,
                EXPIRY_SEC,
                module,
                domain,
                typehash,
                max_fee,
                None,
            )
            .unwrap();
            let bytes = serde_json::to_vec(&params).unwrap();
            black_box(bytes);
        });
    });
    group.finish();
}

fn bench_submit_market(c: &mut Criterion) {
    let signer = signer();
    let instrument = instrument();
    let wallet = wallet();
    let (module, domain, typehash) = (module(), domain(), typehash());
    let order = market_order(OrderSide::Buy);
    let max_fee = dec!(1000);

    let mut group = c.benchmark_group("exec_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("submit_market", |b| {
        b.iter(|| {
            let params = order_to_derive_payload(
                black_box(&order),
                &instrument,
                SUBACCOUNT_ID,
                wallet,
                &signer,
                NONCE,
                EXPIRY_SEC,
                module,
                domain,
                typehash,
                max_fee,
                Some(dec!(3500)),
            )
            .unwrap();
            let bytes = serde_json::to_vec(&params).unwrap();
            black_box(bytes);
        });
    });
    group.finish();
}

fn bench_cancel(c: &mut Criterion) {
    let mut group = c.benchmark_group("exec_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("cancel", |b| {
        b.iter(|| {
            let params = DeriveCancelParams::new(SUBACCOUNT_ID, "ETH-PERP", "order-abc");
            let bytes = serde_json::to_vec(black_box(&params)).unwrap();
            black_box(bytes);
        });
    });
    group.finish();
}

fn bench_modify(c: &mut Criterion) {
    let signer = signer();
    let instrument = instrument();
    let wallet = wallet();
    let (module, domain, typehash) = (module(), domain(), typehash());
    let order = limit_order(OrderSide::Buy);
    let max_fee = dec!(1000);

    let mut group = c.benchmark_group("exec_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("modify", |b| {
        b.iter(|| {
            let params = order_replace_to_derive_payload(
                black_box(&order),
                &instrument,
                SUBACCOUNT_ID,
                wallet,
                &signer,
                NONCE,
                EXPIRY_SEC,
                module,
                domain,
                typehash,
                max_fee,
                None,
                None,
                "order-abc",
            )
            .unwrap();
            let bytes = serde_json::to_vec(&params).unwrap();
            black_box(bytes);
        });
    });
    group.finish();
}

// Empties the emitter's unbounded receiver between iter_batched setups so the
// queue does not grow across criterion samples and skew measurement variance.
fn drain<T>(rx: &mut tokio::sync::mpsc::UnboundedReceiver<T>) {
    while rx.try_recv().is_ok() {}
}

fn order_identity() -> OrderIdentity {
    OrderIdentity {
        instrument_id: instrument_id(),
        strategy_id: strategy_id(),
        order_side: OrderSide::Buy,
        order_type: OrderType::Limit,
    }
}

fn orders_data() -> DeriveOrdersSubscriptionData {
    serde_json::from_str(fixtures::ORDER).unwrap()
}

fn trades_data() -> DeriveTradesSubscriptionData {
    serde_json::from_str(fixtures::TRADE_PRIVATE).unwrap()
}

fn bench_dispatch_orders_untracked(c: &mut Criterion) {
    let (emitter, mut rx) = common::bench_emitter();
    let clock = clock();
    let account_id = account_id();

    let mut group = c.benchmark_group("dispatch");
    group.throughput(Throughput::Elements(1));
    group.bench_function("orders_untracked", |b| {
        b.iter_batched(
            || {
                drain(&mut rx);
                (WsDispatchState::new(), orders_data())
            },
            |(state, data)| {
                dispatch_orders_payload(black_box(data), &emitter, account_id, clock, &state);
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_dispatch_orders_tracked(c: &mut Criterion) {
    let (emitter, mut rx) = common::bench_emitter();
    let clock = clock();
    let account_id = account_id();

    let mut group = c.benchmark_group("dispatch");
    group.throughput(Throughput::Elements(1));
    group.bench_function("orders_tracked", |b| {
        b.iter_batched(
            || {
                drain(&mut rx);
                let state = WsDispatchState::new();
                state.register_identity(
                    ClientOrderId::from(fixtures::TRACKED_LABEL),
                    order_identity(),
                );
                (state, orders_data())
            },
            |(state, data)| {
                dispatch_orders_payload(black_box(data), &emitter, account_id, clock, &state);
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_dispatch_trades_fill(c: &mut Criterion) {
    let (emitter, mut rx) = common::bench_emitter();
    let clock = clock();
    let account_id = account_id();

    let mut group = c.benchmark_group("dispatch");
    group.throughput(Throughput::Elements(1));
    group.bench_function("trades_fill", |b| {
        b.iter_batched(
            || {
                drain(&mut rx);
                let state = WsDispatchState::new();
                state.register_identity(
                    ClientOrderId::from(fixtures::TRACKED_LABEL),
                    order_identity(),
                );
                (state, trades_data())
            },
            |(state, data)| {
                dispatch_trades_payload(black_box(data), &emitter, account_id, clock, &state);
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_submit_limit,
    bench_submit_market,
    bench_cancel,
    bench_modify,
    bench_dispatch_orders_untracked,
    bench_dispatch_orders_tracked,
    bench_dispatch_trades_fill,
);
criterion_main!(benches);
