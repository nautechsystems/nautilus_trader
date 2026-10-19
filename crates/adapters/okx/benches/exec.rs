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
//! `exec_pipeline`: strategy command (place/cancel/modify) -> wire bytes ready
//! to send. Each iteration both constructs the request struct and serializes
//! it to JSON, so the numbers reflect build + serialize together. OKX uses
//! WebSocket for live order ops with no per-message signature (auth is
//! established at login); HMAC signing is benched separately in `signing.rs`
//! for the HTTP path that production uses for instrument definitions and
//! algo orders.
//!
//! `dispatch`: venue execution report (`FillReport`, `OrderStatusReport`) ->
//! report forwarded via [`ExecutionEventEmitter`]. Covers the untracked
//! report-fallback path through `dispatch_execution_reports`: dedup plus
//! `send_*_report` forwarding.
//!
//! `dispatch_ws`: decoded private-stream message -> events forwarded via
//! [`ExecutionEventEmitter`] through `dispatch_ws_message`. Covers the
//! tracked-order path (identity lookup, order-event parse, dedup bookkeeping,
//! `OrderAccepted`/`OrderFilled` construction) and account-state updates.
//!
//! Every dispatch bench runs against one long-lived `WsDispatchState`, as in
//! production, and gives each iteration the next ID from a pool larger than the
//! dedup caches: the caches stay at capacity and no iteration hits a duplicate.

mod common;

use std::hint::black_box;

use ahash::AHashMap;
use common::{btc_usdt_swap, fixtures};
use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_common::messages::ExecutionEvent;
use nautilus_core::{AtomicMap, UUID4, UnixNanos};
use nautilus_live::execution::context::OrderIdentity;
use nautilus_model::{
    enums::{AccountType, LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce},
    events::OrderEventAny,
    identifiers::{ClientOrderId, StrategyId, TradeId, VenueOrderId},
    instruments::Instrument,
    reports::{FillReport, OrderStatusReport},
    types::{Money, Price, Quantity},
};
use nautilus_okx::{
    common::enums::{OKXAlgoOrderType, OKXOrderType, OKXSide, OKXTradeMode, OKXTriggerType},
    http::models::{OKXPlaceAlgoOrderRequest, OKXPlaceOrderRequest},
    websocket::{
        dispatch::{WsDispatchState, dispatch_execution_reports, dispatch_ws_message},
        enums::OKXWsOperation,
        messages::{
            ExecutionReport, OKXOrderMsg, OKXWsFrame, OKXWsMessage, OKXWsRequest,
            WsAmendOrderParams, WsAmendOrderParamsBuilder, WsCancelOrderParams,
            WsCancelOrderParamsBuilder, WsPostOrderParams, WsPostOrderParamsBuilder,
        },
        parse::{FeeCache, FilledQtyCache},
    },
};
use ustr::Ustr;

const BTC_INST_ID_CODE: u64 = 1234; // synthetic instIdCode used by WS order ops

fn build_place_limit() -> OKXPlaceOrderRequest {
    OKXPlaceOrderRequest {
        inst_id: "BTC-USDT-SWAP".to_string(),
        td_mode: OKXTradeMode::Cross,
        ccy: Some("USDT".to_string()),
        cl_ord_id: Some("O-BENCH-LIM".to_string()),
        tag: Some("nautilus".to_string()),
        side: OKXSide::Buy,
        pos_side: None,
        ord_type: OKXOrderType::Limit,
        sz: "0.001".to_string(),
        px: Some("92572.0".to_string()),
        px_usd: None,
        px_vol: None,
        reduce_only: Some(false),
        tgt_ccy: None,
        trade_quote_ccy: None,
        attach_algo_ords: None,
        outcome: None,
        slippage_pct: None,
        rpi_taker_access: None,
        rpi_px_round: None,
    }
}

fn build_place_market() -> OKXPlaceOrderRequest {
    OKXPlaceOrderRequest {
        inst_id: "BTC-USDT-SWAP".to_string(),
        td_mode: OKXTradeMode::Cross,
        ccy: Some("USDT".to_string()),
        cl_ord_id: Some("O-BENCH-MKT".to_string()),
        tag: Some("nautilus".to_string()),
        side: OKXSide::Buy,
        pos_side: None,
        ord_type: OKXOrderType::Market,
        sz: "0.001".to_string(),
        px: None,
        px_usd: None,
        px_vol: None,
        reduce_only: Some(false),
        tgt_ccy: None,
        trade_quote_ccy: None,
        attach_algo_ords: None,
        outcome: None,
        slippage_pct: None,
        rpi_taker_access: None,
        rpi_px_round: None,
    }
}

fn build_place_algo_stop() -> OKXPlaceAlgoOrderRequest {
    OKXPlaceAlgoOrderRequest {
        inst_id: "BTC-USDT-SWAP".to_string(),
        inst_id_code: None,
        td_mode: OKXTradeMode::Cross,
        side: OKXSide::Sell,
        ord_type: OKXAlgoOrderType::Trigger,
        sz: Some("0.001".to_string()),
        algo_cl_ord_id: Some("O-BENCH-STP".to_string()),
        trigger_px: Some("90000.0".to_string()),
        order_px: Some("-1".to_string()), // market-on-trigger sentinel
        trigger_px_type: Some(OKXTriggerType::Last),
        sl_trigger_px: None,
        sl_ord_px: None,
        sl_trigger_px_type: None,
        tp_trigger_px: None,
        tp_ord_px: None,
        tp_trigger_px_type: None,
        tgt_ccy: None,
        pos_side: None,
        close_position: None,
        tag: Some("nautilus".to_string()),
        reduce_only: Some(false),
        close_fraction: None,
        callback_ratio: None,
        callback_spread: None,
        active_px: None,
    }
}

fn build_ws_post_request() -> OKXWsRequest<WsPostOrderParams> {
    let params = WsPostOrderParamsBuilder::default()
        .inst_id_code(BTC_INST_ID_CODE)
        .td_mode(OKXTradeMode::Cross)
        .ccy("USDT")
        .cl_ord_id("O-BENCH-LIM")
        .side(OKXSide::Buy)
        .ord_type(OKXOrderType::Limit)
        .sz("0.001")
        .px("92572.0")
        .tag("nautilus")
        .build()
        .unwrap();
    OKXWsRequest {
        id: Some("req-1".to_string()),
        op: OKXWsOperation::Order,
        exp_time: None,
        args: vec![params],
    }
}

fn build_ws_cancel_request() -> OKXWsRequest<WsCancelOrderParams> {
    let params = WsCancelOrderParamsBuilder::default()
        .inst_id_code(BTC_INST_ID_CODE)
        .cl_ord_id("O-BENCH-CXL")
        .build()
        .unwrap();
    OKXWsRequest {
        id: Some("req-1".to_string()),
        op: OKXWsOperation::CancelOrder,
        exp_time: None,
        args: vec![params],
    }
}

fn build_ws_amend_request() -> OKXWsRequest<WsAmendOrderParams> {
    let params = WsAmendOrderParamsBuilder::default()
        .inst_id_code(BTC_INST_ID_CODE)
        .cl_ord_id("O-BENCH-MOD")
        .new_px("92573.0")
        .new_sz("0.001")
        .build()
        .unwrap();
    OKXWsRequest {
        id: Some("req-1".to_string()),
        op: OKXWsOperation::AmendOrder,
        exp_time: None,
        args: vec![params],
    }
}

fn bench_submit_market(c: &mut Criterion) {
    let mut group = c.benchmark_group("exec_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("submit_market", |b| {
        b.iter(|| {
            let req = build_place_market();
            let bytes = serde_json::to_vec(black_box(&req)).unwrap();
            black_box(bytes);
        });
    });
    group.finish();
}

fn bench_submit_limit(c: &mut Criterion) {
    let mut group = c.benchmark_group("exec_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("submit_limit", |b| {
        b.iter(|| {
            let req = build_place_limit();
            let bytes = serde_json::to_vec(black_box(&req)).unwrap();
            black_box(bytes);
        });
    });
    group.finish();
}

fn bench_submit_stop_market(c: &mut Criterion) {
    let mut group = c.benchmark_group("exec_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("submit_stop_market", |b| {
        b.iter(|| {
            let req = build_place_algo_stop();
            let bytes = serde_json::to_vec(black_box(&req)).unwrap();
            black_box(bytes);
        });
    });
    group.finish();
}

fn bench_submit_ws_limit(c: &mut Criterion) {
    let mut group = c.benchmark_group("exec_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("submit_ws_limit", |b| {
        b.iter(|| {
            let req = build_ws_post_request();
            let bytes = serde_json::to_string(black_box(&req)).unwrap();
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
            let req = build_ws_cancel_request();
            let bytes = serde_json::to_string(black_box(&req)).unwrap();
            black_box(bytes);
        });
    });
    group.finish();
}

fn bench_modify(c: &mut Criterion) {
    let mut group = c.benchmark_group("exec_pipeline");
    group.throughput(Throughput::Elements(1));
    group.bench_function("modify", |b| {
        b.iter(|| {
            let req = build_ws_amend_request();
            let bytes = serde_json::to_string(black_box(&req)).unwrap();
            black_box(bytes);
        });
    });
    group.finish();
}

// Drains the emitter's unbounded receiver between iter_batched setups so the
// queue does not grow across criterion samples and skew measurement variance.
fn drain<T>(rx: &mut tokio::sync::mpsc::UnboundedReceiver<T>) {
    while rx.try_recv().is_ok() {}
}

// Cycles more distinct IDs than the dispatch dedup caches retain (10,000), so a
// long-lived state stays at capacity and no ID is still cached when it recurs
const ID_POOL_SIZE: usize = 16_384;

// Keeps each batch's events, up to two per dispatch, within one emitter channel
// block (32 slots). The channel drains only between batches, so a larger batch
// grows and trims the heap inside the timed region; a live consumer never does.
const DISPATCH_BATCH_SIZE: u64 = 16;

fn id_pool(prefix: &str) -> Vec<String> {
    (0..ID_POOL_SIZE).map(|i| format!("{prefix}-{i}")).collect()
}

fn build_fill_report(cid: ClientOrderId, voi: VenueOrderId, trade_id: TradeId) -> FillReport {
    FillReport::new(
        common::account_id(),
        btc_usdt_swap().id(),
        voi,
        trade_id,
        OrderSide::Buy,
        Quantity::from("0.001"),
        Price::from("92572.0"),
        Money::from("0.05 USDT"),
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
        btc_usdt_swap().id(),
        Some(cid),
        voi,
        OrderSide::Buy.into(),
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

fn bench_dispatch_reports<F>(c: &mut Criterion, name: &str, build: F)
where
    F: Fn(ClientOrderId, VenueOrderId, TradeId) -> ExecutionReport,
{
    let (emitter, mut rx) = common::bench_emitter();
    let state = WsDispatchState::default();
    let client_order_ids: Vec<ClientOrderId> =
        id_pool("O-BENCH").iter().map(ClientOrderId::new).collect();
    let trade_ids: Vec<TradeId> = id_pool("T-BENCH").iter().map(TradeId::new).collect();
    let voi = VenueOrderId::from("2497956918703120384");
    let mut next = 0;

    let mut group = c.benchmark_group("dispatch");
    group.throughput(Throughput::Elements(1));
    group.bench_function(name, |b| {
        b.iter_batched(
            || {
                drain(&mut rx);
                next = (next + 1) % ID_POOL_SIZE;
                vec![build(client_order_ids[next], voi, trade_ids[next])]
            },
            |reports| dispatch_execution_reports(black_box(reports), &emitter, &state),
            BatchSize::NumIterations(DISPATCH_BATCH_SIZE),
        );
    });
    group.finish();
}

fn bench_dispatch_fill(c: &mut Criterion) {
    bench_dispatch_reports(c, "fill", |cid, voi, trade_id| {
        ExecutionReport::Fill(build_fill_report(cid, voi, trade_id))
    });
}

fn bench_dispatch_status_accepted(c: &mut Criterion) {
    bench_dispatch_reports(c, "status_accepted", |cid, voi, _| {
        ExecutionReport::Order(build_status_report(cid, voi, OrderStatus::Accepted))
    });
}

fn bench_dispatch_status_canceled(c: &mut Criterion) {
    bench_dispatch_reports(c, "status_canceled", |cid, voi, _| {
        ExecutionReport::Order(build_status_report(cid, voi, OrderStatus::Canceled))
    });
}

fn bench_dispatch_status_filled(c: &mut Criterion) {
    bench_dispatch_reports(c, "status_filled", |cid, voi, _| {
        ExecutionReport::Order(build_status_report(cid, voi, OrderStatus::Filled))
    });
}

fn decode_data(frame: &str) -> serde_json::Value {
    let frame: OKXWsFrame = serde_json::from_str(frame).unwrap();

    let OKXWsFrame::Data { data, .. } = frame else {
        unreachable!()
    };

    data
}

fn decode_order_msgs(frame: &str) -> Vec<OKXOrderMsg> {
    serde_json::from_value(decode_data(frame)).unwrap()
}

// Gives each pooled message its own client order, venue order, and trade IDs so
// the long-lived fee, fill, and dedup caches treat every iteration as a new order
fn tracked_order_msgs(frame: &str) -> Vec<OKXOrderMsg> {
    let template = decode_order_msgs(frame).remove(0);
    let client_order_ids = id_pool("O-BENCH");
    let venue_order_ids = id_pool(template.ord_id.as_str());
    let trade_ids = id_pool("T-BENCH");

    (0..ID_POOL_SIZE)
        .map(|i| {
            let mut msg = template.clone();
            msg.cl_ord_id.clone_from(&client_order_ids[i]);
            msg.ord_id = Ustr::from(&venue_order_ids[i]);

            if !msg.trade_id.is_empty() {
                msg.trade_id.clone_from(&trade_ids[i]);
            }

            msg
        })
        .collect()
}

fn order_event_kinds(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
) -> Vec<&'static str> {
    std::iter::from_fn(|| rx.try_recv().ok())
        .map(|event| match event {
            ExecutionEvent::Order(OrderEventAny::Accepted(_)) => "accepted",
            ExecutionEvent::Order(OrderEventAny::Filled(_)) => "filled",
            _ => "other",
        })
        .collect()
}

fn bench_dispatch_ws_orders(
    c: &mut Criterion,
    name: &str,
    frame: &str,
    order_type: OrderType,
    expected_events: &[&str],
) {
    let (emitter, mut rx) = common::bench_emitter();
    let account_id = common::account_id();
    let instruments = AtomicMap::from(common::instrument_cache());
    let state = WsDispatchState::default();
    let mut fee_cache = FeeCache::new();
    let mut filled_qty_cache = FilledQtyCache::new();
    let mut order_state_cache = AHashMap::new();
    let msgs = tracked_order_msgs(frame);

    let identities: Vec<OrderIdentity> = msgs
        .iter()
        .map(|msg| OrderIdentity {
            client_order_id: ClientOrderId::new(&msg.cl_ord_id),
            strategy_id: StrategyId::from("S-BENCH"),
            instrument_id: btc_usdt_swap().id(),
            order_side: OrderSide::Buy,
            order_type,
        })
        .collect();

    let mut dispatch = |message| {
        dispatch_ws_message(
            message,
            &emitter,
            &state,
            account_id,
            AccountType::Cash,
            &instruments,
            &mut fee_cache,
            &mut filled_qty_cache,
            &mut order_state_cache,
            common::clock(),
        );
    };

    // Checks the steady state the pool relies on: once every pooled ID has been
    // seen, a recurring ID still emits the events of a first venue update
    for index in (0..ID_POOL_SIZE).chain([0]) {
        drain(&mut rx);
        let identity = identities[index];
        state
            .order_identities
            .insert(identity.client_order_id, identity);
        dispatch(OKXWsMessage::Orders(vec![msgs[index].clone()]));
    }

    assert_eq!(order_event_kinds(&mut rx), expected_events);
    let mut next = 0;

    let mut group = c.benchmark_group("dispatch_ws");
    group.throughput(Throughput::Elements(1));
    group.bench_function(name, |b| {
        b.iter_batched(
            || {
                drain(&mut rx);
                next = (next + 1) % ID_POOL_SIZE;
                let identity = identities[next];
                state
                    .order_identities
                    .insert(identity.client_order_id, identity);
                OKXWsMessage::Orders(vec![msgs[next].clone()])
            },
            |message| dispatch(black_box(message)),
            BatchSize::NumIterations(DISPATCH_BATCH_SIZE),
        );
    });
    group.finish();
}

fn bench_dispatch_ws_order_accepted(c: &mut Criterion) {
    bench_dispatch_ws_orders(
        c,
        "order_accepted",
        fixtures::ORDER_LIVE,
        OrderType::Limit,
        &["accepted"],
    );
}

fn bench_dispatch_ws_order_filled(c: &mut Criterion) {
    bench_dispatch_ws_orders(
        c,
        "order_filled",
        fixtures::ORDERS,
        OrderType::Market,
        &["accepted", "filled"],
    );
}

fn bench_dispatch_ws_account(c: &mut Criterion) {
    let (emitter, mut rx) = common::bench_emitter();
    let account_id = common::account_id();
    let instruments = AtomicMap::from(common::instrument_cache());
    let state = WsDispatchState::default();
    let mut fee_cache = FeeCache::new();
    let mut filled_qty_cache = FilledQtyCache::new();
    let mut order_state_cache = AHashMap::new();
    let data = decode_data(fixtures::ACCOUNT);

    let mut group = c.benchmark_group("dispatch_ws");
    group.throughput(Throughput::Elements(1));
    group.bench_function("account", |b| {
        b.iter_batched(
            || {
                drain(&mut rx);
                OKXWsMessage::Account(data.clone())
            },
            |message| {
                dispatch_ws_message(
                    black_box(message),
                    &emitter,
                    &state,
                    account_id,
                    AccountType::Cash,
                    &instruments,
                    &mut fee_cache,
                    &mut filled_qty_cache,
                    &mut order_state_cache,
                    common::clock(),
                );
            },
            BatchSize::NumIterations(DISPATCH_BATCH_SIZE),
        );
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_submit_market,
    bench_submit_limit,
    bench_submit_stop_market,
    bench_submit_ws_limit,
    bench_cancel,
    bench_modify,
    bench_dispatch_fill,
    bench_dispatch_status_accepted,
    bench_dispatch_status_canceled,
    bench_dispatch_status_filled,
    bench_dispatch_ws_order_accepted,
    bench_dispatch_ws_order_filled,
    bench_dispatch_ws_account,
);
criterion_main!(benches);
