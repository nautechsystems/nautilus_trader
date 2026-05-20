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
//! `exec_pipeline`: strategy command (OrderAny / cancel / modify) → signed
//! wire bytes ready to POST. Covers normalize + serialize + sign.
//!
//! `dispatch`: venue report (FillReport, OrderStatusReport) → events emitted
//! via [`ExecutionEventEmitter`]. Covers dedup + identity lookup + event
//! construction.

mod common;

use std::hint::black_box;

use common::{btc_perp, trader_id};
use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_hyperliquid::{
    common::{credential::EvmPrivateKey, parse::order_to_hyperliquid_request_with_asset},
    http::models::{
        Cloid, HyperliquidExecAction, HyperliquidExecCancelByCloidRequest, HyperliquidExecGrouping,
        HyperliquidExecLimitParams, HyperliquidExecModifyOrderRequest, HyperliquidExecOrderKind,
        HyperliquidExecPlaceOrderRequest, HyperliquidExecTif,
    },
    signing::{HyperliquidActionType, HyperliquidEip712Signer, SignRequest, TimeNonce},
    websocket::dispatch::{
        OrderIdentity, WsDispatchState, dispatch_order_event, dispatch_order_fill,
    },
};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce, TriggerType},
    identifiers::{ClientOrderId, StrategyId, TradeId, VenueOrderId},
    instruments::Instrument,
    orders::{LimitOrder, MarketOrder, OrderAny, StopMarketOrder},
    reports::{FillReport, OrderStatusReport},
    types::{Money, Price, Quantity},
};
use rust_decimal::Decimal;

const TEST_KEY: &str = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
const BTC_ASSET_INDEX: u32 = 3;

fn signer() -> HyperliquidEip712Signer {
    let key = EvmPrivateKey::new(TEST_KEY).unwrap();
    HyperliquidEip712Signer::new(&key).unwrap()
}

fn strategy_id() -> StrategyId {
    StrategyId::from("S-BENCH")
}

fn client_order_id(suffix: &str) -> ClientOrderId {
    ClientOrderId::from(format!("O-BENCH-{suffix}").as_str())
}

fn limit_order(side: OrderSide) -> OrderAny {
    OrderAny::Limit(LimitOrder::new(
        trader_id(),
        strategy_id(),
        btc_perp().id(),
        client_order_id("LIM"),
        side,
        Quantity::from("0.001"),
        Price::from("92572.0"),
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
        btc_perp().id(),
        client_order_id("MKT"),
        side,
        Quantity::from("0.001"),
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

fn stop_market_order(side: OrderSide) -> OrderAny {
    OrderAny::StopMarket(StopMarketOrder::new(
        trader_id(),
        strategy_id(),
        btc_perp().id(),
        client_order_id("STP"),
        side,
        Quantity::from("0.001"),
        Price::from("90000.0"),
        TriggerType::LastPrice,
        TimeInForce::Gtc,
        None,
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

// Builds a signed L1 request body from a HyperliquidExecAction, exactly as the
// HTTP client does it on the order-submit path (skip the to_value step that
// the perf patch removed).
fn sign_action(signer: &HyperliquidEip712Signer, action: &HyperliquidExecAction) -> Vec<u8> {
    let action_bytes = rmp_serde::to_vec_named(action).unwrap();
    let sign_request = SignRequest {
        action: None,
        action_bytes: Some(action_bytes.clone()),
        time_nonce: TimeNonce::from_millis(1_733_833_200_000),
        action_type: HyperliquidActionType::L1,
        is_testnet: false,
        vault_address: None,
        expires_after: None,
    };
    let _sig = signer.sign(&sign_request).unwrap();
    action_bytes
}

fn bench_submit_market(c: &mut Criterion) {
    let signer = signer();
    let order = market_order(OrderSide::Buy);

    let mut group = c.benchmark_group("exec_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("submit_market", |b| {
        b.iter(|| {
            let req = order_to_hyperliquid_request_with_asset(
                black_box(&order),
                BTC_ASSET_INDEX,
                2,
                true,
                50,
            )
            .unwrap();
            let action = HyperliquidExecAction::Order {
                orders: vec![req],
                grouping: HyperliquidExecGrouping::Na,
                builder: None,
            };
            let bytes = sign_action(&signer, &action);
            black_box(bytes);
        });
    });
    group.finish();
}

fn bench_submit_limit(c: &mut Criterion) {
    let signer = signer();
    let order = limit_order(OrderSide::Buy);

    let mut group = c.benchmark_group("exec_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("submit_limit", |b| {
        b.iter(|| {
            let req = order_to_hyperliquid_request_with_asset(
                black_box(&order),
                BTC_ASSET_INDEX,
                2,
                true,
                50,
            )
            .unwrap();
            let action = HyperliquidExecAction::Order {
                orders: vec![req],
                grouping: HyperliquidExecGrouping::Na,
                builder: None,
            };
            let bytes = sign_action(&signer, &action);
            black_box(bytes);
        });
    });
    group.finish();
}

fn bench_submit_stop_market(c: &mut Criterion) {
    let signer = signer();
    let order = stop_market_order(OrderSide::Sell);

    let mut group = c.benchmark_group("exec_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("submit_stop_market", |b| {
        b.iter(|| {
            let req = order_to_hyperliquid_request_with_asset(
                black_box(&order),
                BTC_ASSET_INDEX,
                2,
                true,
                50,
            )
            .unwrap();
            let action = HyperliquidExecAction::Order {
                orders: vec![req],
                grouping: HyperliquidExecGrouping::Na,
                builder: None,
            };
            let bytes = sign_action(&signer, &action);
            black_box(bytes);
        });
    });
    group.finish();
}

fn bench_cancel(c: &mut Criterion) {
    let signer = signer();
    let cloid = Cloid::from_client_order_id(client_order_id("CXL"));

    let mut group = c.benchmark_group("exec_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("cancel", |b| {
        b.iter(|| {
            let action = HyperliquidExecAction::CancelByCloid {
                cancels: vec![HyperliquidExecCancelByCloidRequest {
                    asset: BTC_ASSET_INDEX,
                    cloid: cloid.clone(),
                }],
            };
            let bytes = sign_action(&signer, black_box(&action));
            black_box(bytes);
        });
    });
    group.finish();
}

fn bench_modify(c: &mut Criterion) {
    let signer = signer();
    let cloid = Cloid::from_client_order_id(client_order_id("MOD"));
    let replacement = HyperliquidExecPlaceOrderRequest {
        asset: BTC_ASSET_INDEX,
        is_buy: true,
        price: Decimal::from(92573),
        size: Decimal::new(1, 3),
        reduce_only: false,
        kind: HyperliquidExecOrderKind::Limit {
            limit: HyperliquidExecLimitParams {
                tif: HyperliquidExecTif::Gtc,
            },
        },
        cloid: Some(cloid),
    };

    let mut group = c.benchmark_group("exec_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("modify", |b| {
        b.iter(|| {
            let action = HyperliquidExecAction::Modify {
                modify: HyperliquidExecModifyOrderRequest {
                    oid: 430_481_837_807,
                    order: replacement.clone(),
                },
            };
            let bytes = sign_action(&signer, black_box(&action));
            black_box(bytes);
        });
    });
    group.finish();
}

// Dispatch path -----------------------------------------------------------

// Empties the emitter's unbounded receiver between iter_batched setups so the
// queue does not grow across criterion samples and skew measurement variance.
fn drain<T>(rx: &mut tokio::sync::mpsc::UnboundedReceiver<T>) {
    while rx.try_recv().is_ok() {}
}

fn order_identity() -> OrderIdentity {
    OrderIdentity {
        strategy_id: strategy_id(),
        instrument_id: btc_perp().id(),
        order_side: OrderSide::Buy,
        order_type: OrderType::Limit,
        quantity: Quantity::from("0.001"),
        price: Some(Price::from("92572.0")),
    }
}

fn build_fill_report(cid: ClientOrderId, voi: VenueOrderId) -> FillReport {
    FillReport::new(
        common::account_id(),
        btc_perp().id(),
        voi,
        TradeId::new("TRADE-1"),
        OrderSide::Buy,
        Quantity::from("0.001"),
        Price::from("92572.0"),
        Money::from("0.05 USDC"),
        LiquiditySide::Taker,
        Some(cid),
        None,
        UnixNanos::from(1),
        UnixNanos::from(2),
        Some(UUID4::new()),
    )
}

fn build_status_report(
    cid: ClientOrderId,
    voi: VenueOrderId,
    status: OrderStatus,
) -> OrderStatusReport {
    OrderStatusReport::new(
        common::account_id(),
        btc_perp().id(),
        Some(cid),
        voi,
        OrderSide::Buy,
        OrderType::Limit,
        TimeInForce::Gtc,
        status,
        Quantity::from("0.001"),
        Quantity::from("0"),
        UnixNanos::from(1),
        UnixNanos::from(2),
        UnixNanos::from(3),
        Some(UUID4::new()),
    )
    .with_price(Price::from("92572.0"))
}

fn primed_state(cid: ClientOrderId, voi: VenueOrderId) -> WsDispatchState {
    let state = WsDispatchState::new();
    state.register_identity(cid, order_identity());
    state.record_venue_order_id(cid, voi);
    state.insert_accepted(cid);
    state
}

fn bench_dispatch_fill(c: &mut Criterion) {
    let (emitter, mut rx) = common::bench_emitter();
    let cid = client_order_id("DFL");
    let voi = VenueOrderId::from("430481837807");
    let report = build_fill_report(cid, voi);

    let mut group = c.benchmark_group("dispatch");
    group.throughput(Throughput::Elements(1));
    group.bench_function("fill", |b| {
        b.iter_batched(
            || {
                drain(&mut rx);
                primed_state(cid, voi)
            },
            |state| {
                let outcome =
                    dispatch_order_fill(black_box(&report), &state, &emitter, UnixNanos::default());
                black_box(outcome);
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_dispatch_status_accepted(c: &mut Criterion) {
    let (emitter, mut rx) = common::bench_emitter();
    let cid = client_order_id("DAC");
    let voi = VenueOrderId::from("430481837807");
    let report = build_status_report(cid, voi, OrderStatus::Accepted);

    let mut group = c.benchmark_group("dispatch");
    group.throughput(Throughput::Elements(1));
    group.bench_function("status_accepted", |b| {
        b.iter_batched(
            || {
                drain(&mut rx);
                let state = WsDispatchState::new();
                state.register_identity(cid, order_identity());
                state
            },
            |state| {
                let outcome = dispatch_order_event(
                    black_box(&report),
                    &state,
                    &emitter,
                    UnixNanos::default(),
                );
                black_box(outcome);
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_dispatch_status_canceled(c: &mut Criterion) {
    let (emitter, mut rx) = common::bench_emitter();
    let cid = client_order_id("DCX");
    let voi = VenueOrderId::from("430481837807");
    let report = build_status_report(cid, voi, OrderStatus::Canceled);

    let mut group = c.benchmark_group("dispatch");
    group.throughput(Throughput::Elements(1));
    group.bench_function("status_canceled", |b| {
        b.iter_batched(
            || {
                drain(&mut rx);
                primed_state(cid, voi)
            },
            |state| {
                let outcome = dispatch_order_event(
                    black_box(&report),
                    &state,
                    &emitter,
                    UnixNanos::default(),
                );
                black_box(outcome);
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_dispatch_status_modified(c: &mut Criterion) {
    // Cancel-replace promotion: the cached voi differs from the new accepted voi
    // under the same cid, triggering the OrderUpdated path.
    let (emitter, mut rx) = common::bench_emitter();
    let cid = client_order_id("DMD");
    let old_voi = VenueOrderId::from("430481837807");
    let new_voi = VenueOrderId::from("430481999467");
    let report = build_status_report(cid, new_voi, OrderStatus::Accepted);

    let mut group = c.benchmark_group("dispatch");
    group.throughput(Throughput::Elements(1));
    group.bench_function("status_modified", |b| {
        b.iter_batched(
            || {
                drain(&mut rx);
                let state = WsDispatchState::new();
                state.register_identity(cid, order_identity());
                state.record_venue_order_id(cid, old_voi);
                state.insert_accepted(cid);
                state.mark_pending_modify(cid, old_voi, Quantity::from("0.001"));
                state
            },
            |state| {
                let outcome = dispatch_order_event(
                    black_box(&report),
                    &state,
                    &emitter,
                    UnixNanos::default(),
                );
                black_box(outcome);
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_submit_market,
    bench_submit_limit,
    bench_submit_stop_market,
    bench_cancel,
    bench_modify,
    bench_dispatch_fill,
    bench_dispatch_status_accepted,
    bench_dispatch_status_canceled,
    bench_dispatch_status_modified,
);
criterion_main!(benches);
