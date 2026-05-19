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
//! `send_*_report` forwarding. The tracked-order path (`dispatch_ws_message`
//! -> `dispatch_parsed_order_event` -> `OrderAccepted`/`OrderFilled` event
//! construction) is `pub(crate)` and not exercised here.

mod common;

use std::hint::black_box;

use common::btc_usdt_swap;
use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce},
    identifiers::{ClientOrderId, TradeId, VenueOrderId},
    instruments::Instrument,
    reports::{FillReport, OrderStatusReport},
    types::{Money, Price, Quantity},
};
use nautilus_okx::{
    common::enums::{OKXAlgoOrderType, OKXOrderType, OKXSide, OKXTradeMode, OKXTriggerType},
    http::models::{OKXPlaceAlgoOrderRequest, OKXPlaceOrderRequest},
    websocket::{
        dispatch::{WsDispatchState, dispatch_execution_reports},
        enums::OKXWsOperation,
        messages::{
            ExecutionReport, OKXWsRequest, WsAmendOrderParams, WsAmendOrderParamsBuilder,
            WsCancelOrderParams, WsCancelOrderParamsBuilder, WsPostOrderParams,
            WsPostOrderParamsBuilder,
        },
    },
};

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
        attach_algo_ords: None,
        speed_bump: None,
        outcome: None,
        slippage_pct: None,
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
        attach_algo_ords: None,
        speed_bump: None,
        outcome: None,
        slippage_pct: None,
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

fn build_fill_report(cid: ClientOrderId, voi: VenueOrderId) -> FillReport {
    FillReport::new(
        common::account_id(),
        btc_usdt_swap().id(),
        voi,
        TradeId::new("TRADE-1"),
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

fn bench_dispatch_fill(c: &mut Criterion) {
    let (emitter, mut rx) = common::bench_emitter();
    let cid = ClientOrderId::from("O-BENCH-DFL");
    let voi = VenueOrderId::from("2497956918703120384");

    let mut group = c.benchmark_group("dispatch");
    group.throughput(Throughput::Elements(1));
    group.bench_function("fill", |b| {
        b.iter_batched(
            || {
                drain(&mut rx);
                let state = WsDispatchState::default();
                let report = build_fill_report(cid, voi);
                (state, vec![ExecutionReport::Fill(report)])
            },
            |(state, reports)| {
                dispatch_execution_reports(black_box(reports), &emitter, &state);
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_dispatch_status_accepted(c: &mut Criterion) {
    let (emitter, mut rx) = common::bench_emitter();
    let cid = ClientOrderId::from("O-BENCH-DAC");
    let voi = VenueOrderId::from("2497956918703120384");

    let mut group = c.benchmark_group("dispatch");
    group.throughput(Throughput::Elements(1));
    group.bench_function("status_accepted", |b| {
        b.iter_batched(
            || {
                drain(&mut rx);
                let state = WsDispatchState::default();
                let report = build_status_report(cid, voi, OrderStatus::Accepted);
                (state, vec![ExecutionReport::Order(report)])
            },
            |(state, reports)| {
                dispatch_execution_reports(black_box(reports), &emitter, &state);
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_dispatch_status_canceled(c: &mut Criterion) {
    let (emitter, mut rx) = common::bench_emitter();
    let cid = ClientOrderId::from("O-BENCH-DCX");
    let voi = VenueOrderId::from("2497956918703120384");

    let mut group = c.benchmark_group("dispatch");
    group.throughput(Throughput::Elements(1));
    group.bench_function("status_canceled", |b| {
        b.iter_batched(
            || {
                drain(&mut rx);
                let state = WsDispatchState::default();
                let report = build_status_report(cid, voi, OrderStatus::Canceled);
                (state, vec![ExecutionReport::Order(report)])
            },
            |(state, reports)| {
                dispatch_execution_reports(black_box(reports), &emitter, &state);
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_dispatch_status_filled(c: &mut Criterion) {
    let (emitter, mut rx) = common::bench_emitter();
    let cid = ClientOrderId::from("O-BENCH-DFD");
    let voi = VenueOrderId::from("2497956918703120384");

    let mut group = c.benchmark_group("dispatch");
    group.throughput(Throughput::Elements(1));
    group.bench_function("status_filled", |b| {
        b.iter_batched(
            || {
                drain(&mut rx);
                let state = WsDispatchState::default();
                let report = build_status_report(cid, voi, OrderStatus::Filled);
                (state, vec![ExecutionReport::Order(report)])
            },
            |(state, reports)| {
                dispatch_execution_reports(black_box(reports), &emitter, &state);
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
    bench_submit_ws_limit,
    bench_cancel,
    bench_modify,
    bench_dispatch_fill,
    bench_dispatch_status_accepted,
    bench_dispatch_status_canceled,
    bench_dispatch_status_filled,
);
criterion_main!(benches);
