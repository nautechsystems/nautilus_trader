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

//! Integration tests for `OKXExecutionClient`.

use std::{
    cell::RefCell, collections::HashMap, net::SocketAddr, rc::Rc, sync::Arc, time::Duration,
};

use ahash::AHashMap;
use axum::{Json, Router, extract::Query, http::HeaderMap, response::IntoResponse, routing::get};
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    live::runner::set_exec_event_sender,
    messages::{
        ExecutionEvent,
        execution::{
            BatchCancelOrders, CancelOrder, ExecutionReport as CommonExecutionReport, ModifyOrder,
            QueryAccount, SubmitOrder, SubmitOrderList,
            report::{GenerateFillReports, GenerateOrderStatusReports},
        },
    },
    testing::wait_until_async,
};
use nautilus_core::{AtomicMap, UUID4, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    enums::{AccountType, LiquiditySide, OmsType, OrderSide, OrderStatus, OrderType, TimeInForce},
    events::{OrderEventAny, OrderInitialized},
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, OrderListId, StrategyId, Symbol, TradeId, TraderId,
        VenueOrderId,
    },
    instruments::{CryptoFuturesSpread, InstrumentAny},
    orders::{Order, OrderAny, OrderList, OrderTestBuilder},
    reports::{FillReport, OrderStatusReport},
    types::{Currency, Money, Price, Quantity},
};
use nautilus_network::http::HttpClient;
use nautilus_okx::{
    common::{
        consts::{OKX_CLIENT_ID, OKX_POST_ONLY_CANCEL_SOURCE, OKX_VENUE},
        enums::{OKXInstrumentType, OKXOrderStatus, OKXOrderType, OKXSide},
    },
    config::OKXExecClientConfig,
    execution::OKXExecutionClient,
    http::models::{OKXCancelAlgoOrderResponse, OKXSpreadOrder},
    websocket::{
        dispatch::{
            AlgoCancelContext, OrderIdentity, WsDispatchState, dispatch_execution_reports,
            dispatch_ws_message, emit_algo_cancel_rejections, emit_batch_cancel_failure,
        },
        enums::OKXWsOperation,
        messages::{ExecutionReport, OKXWsMessage},
        parse::OrderStateSnapshot,
    },
};
use rstest::rstest;
use serde_json::json;
use ustr::Ustr;

fn test_emitter() -> (
    ExecutionEventEmitter,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
) {
    let clock = get_atomic_clock_realtime();
    let mut emitter = ExecutionEventEmitter::new(
        clock,
        TraderId::from("TESTER-001"),
        AccountId::from("OKX-001"),
        AccountType::Margin,
        None,
    );
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    emitter.set_sender(tx);
    (emitter, rx)
}

fn make_fill_report(cid: &str) -> FillReport {
    FillReport::new(
        AccountId::from("OKX-001"),
        InstrumentId::from("ETH-USDT-SWAP.OKX"),
        VenueOrderId::new("v-1"),
        TradeId::new("t-1"),
        OrderSide::Buy,
        Quantity::new(1.0, 0),
        Price::new(2000.0, 2),
        Money::new(0.01, Currency::USDT()),
        LiquiditySide::Taker,
        Some(ClientOrderId::new(cid)),
        None,
        UnixNanos::default(),
        UnixNanos::default(),
        None,
    )
}

fn make_order_status_report(cid: &str, status: OrderStatus) -> OrderStatusReport {
    OrderStatusReport::new(
        AccountId::from("OKX-001"),
        InstrumentId::from("ETH-USDT-SWAP.OKX"),
        Some(ClientOrderId::new(cid)),
        VenueOrderId::new("v-1"),
        OrderSide::Buy,
        OrderType::StopMarket,
        TimeInForce::Gtc,
        status,
        Quantity::new(1.0, 0),
        Quantity::zero(0),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        None,
    )
}

fn make_spread_instrument() -> InstrumentAny {
    let instrument = CryptoFuturesSpread::new(
        InstrumentId::from("BCH-USDT_BCH-USDT-SWAP.OKX"),
        Symbol::from("BCH-USDT_BCH-USDT-SWAP"),
        Currency::get_or_create_crypto("BCH"),
        Currency::USDT(),
        Currency::USDT(),
        false,
        Ustr::from("linear"),
        UnixNanos::default(),
        UnixNanos::default(),
        1,
        2,
        Price::from("0.1"),
        Quantity::from("0.01"),
        None,
        Some(Quantity::from("0.01")),
        None,
        Some(Quantity::from("0.01")),
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
        UnixNanos::default(),
        UnixNanos::default(),
    );

    InstrumentAny::CryptoFuturesSpread(instrument)
}

fn spread_instruments_cache() -> AtomicMap<Ustr, InstrumentAny> {
    let instruments = AtomicMap::new();
    instruments.insert(
        Ustr::from("BCH-USDT_BCH-USDT-SWAP"),
        make_spread_instrument(),
    );
    instruments
}

fn make_spread_order_msg(
    state: OKXOrderStatus,
    client_order_id: ClientOrderId,
    venue_order_id: &str,
) -> OKXSpreadOrder {
    OKXSpreadOrder {
        sprd_id: Ustr::from("BCH-USDT_BCH-USDT-SWAP"),
        ord_id: Ustr::from(venue_order_id),
        cl_ord_id: Ustr::from(client_order_id.as_str()),
        tag: String::new(),
        side: OKXSide::Buy,
        ord_type: OKXOrderType::Limit,
        sz: "0.01".to_string(),
        px: "1.0".to_string(),
        avg_px: String::new(),
        state,
        acc_fill_sz: "0".to_string(),
        pending_fill_sz: "0".to_string(),
        pending_settle_sz: "0".to_string(),
        canceled_sz: "0".to_string(),
        fill_sz: String::new(),
        fill_px: String::new(),
        trade_id: Ustr::default(),
        cancel_source: String::new(),
        req_id: String::new(),
        amend_result: String::new(),
        code: String::new(),
        msg: String::new(),
        c_time: Some(1_779_648_154_000),
        u_time: Some(1_779_648_155_000),
    }
}

fn dispatch_spread_message(
    message: OKXSpreadOrder,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    filled_qty_cache: &mut AHashMap<Ustr, Quantity>,
    order_state_cache: &mut AHashMap<ClientOrderId, OrderStateSnapshot>,
) {
    let mut fee_cache: AHashMap<Ustr, Money> = AHashMap::new();
    dispatch_ws_message(
        OKXWsMessage::SpreadOrders(vec![message]),
        emitter,
        state,
        AccountId::from("OKX-001"),
        instruments,
        &mut fee_cache,
        filled_qty_cache,
        order_state_cache,
        get_atomic_clock_realtime(),
    );
}

fn track_spread_order(state: &WsDispatchState, client_order_id: ClientOrderId) {
    state.order_identities.insert(
        client_order_id,
        OrderIdentity {
            instrument_id: InstrumentId::from("BCH-USDT_BCH-USDT-SWAP.OKX"),
            strategy_id: StrategyId::from("STRATEGY-001"),
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
        },
    );
}

fn drain_events(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
) -> Vec<ExecutionEvent> {
    let mut events = Vec::new();
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }
    events
}

#[rstest]
fn test_ambiguous_submit_send_failure_does_not_emit_order_rejected() {
    let cid = ClientOrderId::new("O-submit-send-failure");
    let (events, state) = dispatch_send_failed_response(OKXWsOperation::Order, cid);

    assert!(
        !contains_order_event(&events, |event| matches!(event, OrderEventAny::Rejected(_))),
        "ambiguous submit failure should not emit OrderRejected: {events:?}"
    );
    assert!(state.order_identities.contains_key(&cid));
}

#[rstest]
fn test_explicit_venue_submit_rejection_emits_order_rejected() {
    let cid = ClientOrderId::new("O-submit-explicit-reject");
    let events = dispatch_explicit_rejection_response(OKXWsOperation::Order, cid);

    assert!(
        contains_order_event(&events, |event| matches!(event, OrderEventAny::Rejected(_))),
        "explicit venue submit rejection should emit OrderRejected: {events:?}"
    );
}

#[rstest]
fn test_ambiguous_cancel_send_failure_does_not_emit_order_cancel_rejected() {
    let cid = ClientOrderId::new("O-cancel-send-failure");
    let (events, state) = dispatch_send_failed_response(OKXWsOperation::CancelOrder, cid);

    assert!(
        !contains_order_event(&events, |event| matches!(
            event,
            OrderEventAny::CancelRejected(_)
        )),
        "ambiguous cancel failure should not emit OrderCancelRejected: {events:?}"
    );
    assert!(state.order_identities.contains_key(&cid));
}

#[rstest]
fn test_explicit_venue_cancel_rejection_emits_order_cancel_rejected() {
    let cid = ClientOrderId::new("O-cancel-explicit-reject");
    let events = dispatch_explicit_rejection_response(OKXWsOperation::CancelOrder, cid);

    assert!(
        contains_order_event(&events, |event| matches!(
            event,
            OrderEventAny::CancelRejected(_)
        )),
        "explicit venue cancel rejection should emit OrderCancelRejected: {events:?}"
    );
}

#[rstest]
fn test_ambiguous_modify_send_failure_does_not_emit_order_modify_rejected() {
    let cid = ClientOrderId::new("O-modify-send-failure");
    let (events, state) = dispatch_send_failed_response(OKXWsOperation::AmendOrder, cid);

    assert!(
        !contains_order_event(&events, |event| matches!(
            event,
            OrderEventAny::ModifyRejected(_)
        )),
        "ambiguous modify failure should not emit OrderModifyRejected: {events:?}"
    );
    assert!(state.order_identities.contains_key(&cid));
}

#[rstest]
fn test_explicit_venue_modify_rejection_emits_order_modify_rejected() {
    let cid = ClientOrderId::new("O-modify-explicit-reject");
    let events = dispatch_explicit_rejection_response(OKXWsOperation::AmendOrder, cid);

    assert!(
        contains_order_event(&events, |event| matches!(
            event,
            OrderEventAny::ModifyRejected(_)
        )),
        "explicit venue modify rejection should emit OrderModifyRejected: {events:?}"
    );
}

#[rstest]
#[tokio::test]
async fn test_local_submit_validation_failure_emits_order_rejected() {
    let addr = start_exec_test_server().await;
    let base_url = format!("http://{addr}");
    let (mut client, mut rx, cache) = create_test_execution_client(&base_url);

    client.start().unwrap();
    let _ = drain_events(&mut rx);

    let client_order_id = ClientOrderId::new("OLOCALSUBMITREJECT1");
    let order = cache_limit_order(&cache, client_order_id);
    let cmd = SubmitOrder::from_order(
        &order,
        TraderId::from("TESTER-001"),
        Some(*OKX_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
    );

    client.submit_order(cmd).unwrap();

    match recv_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::Rejected(rejected) if rejected.client_order_id == client_order_id
        )
    })
    .await
    {
        OrderEventAny::Rejected(rejected) => {
            assert_eq!(rejected.client_order_id, client_order_id);
            assert!(
                rejected.reason.as_str().contains("No instIdCode cached"),
                "reason was: {}",
                rejected.reason
            );
        }
        other => panic!("expected OrderRejected event, was {other:?}"),
    }
}

#[rstest]
#[tokio::test]
async fn test_local_cancel_validation_failure_does_not_emit_order_cancel_rejected() {
    let addr = start_exec_test_server().await;
    let base_url = format!("http://{addr}");
    let (mut client, mut rx, cache) = create_test_execution_client(&base_url);

    client.start().unwrap();
    let _ = drain_events(&mut rx);

    let client_order_id = ClientOrderId::new("OLOCALCANCELINVALID1");
    let order = cache_limit_order(&cache, client_order_id);
    let cmd = CancelOrder {
        trader_id: TraderId::from("TESTER-001"),
        client_id: Some(*OKX_CLIENT_ID),
        strategy_id: order.strategy_id(),
        instrument_id: order.instrument_id(),
        client_order_id,
        venue_order_id: Some(VenueOrderId::new("v-1")),
        command_id: UUID4::new(),
        ts_init: UnixNanos::default(),
        params: None,
        correlation_id: None,
        causation_id: None,
    };

    client.cancel_order(cmd).unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

    let events = drain_events(&mut rx);
    assert!(
        !contains_order_event(&events, |event| matches!(
            event,
            OrderEventAny::CancelRejected(rejected)
                if rejected.client_order_id == client_order_id
        )),
        "local cancel validation failure should not emit OrderCancelRejected: {events:?}"
    );
}

#[rstest]
#[tokio::test]
async fn test_local_modify_validation_failure_emits_order_modify_rejected() {
    let addr = start_exec_test_server().await;
    let base_url = format!("http://{addr}");
    let (mut client, mut rx, cache) = create_test_execution_client(&base_url);

    client.start().unwrap();
    let _ = drain_events(&mut rx);

    let client_order_id = ClientOrderId::new("OLOCALMODIFYREJECT1");
    let order = cache_limit_order(&cache, client_order_id);
    let cmd = ModifyOrder::new(
        TraderId::from("TESTER-001"),
        Some(*OKX_CLIENT_ID),
        order.strategy_id(),
        order.instrument_id(),
        client_order_id,
        Some(VenueOrderId::new("v-1")),
        Some(Quantity::from("2")),
        Some(Price::from("2001.00")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client.modify_order(cmd).unwrap();

    match recv_order_event_matching(&mut rx, |event| {
        matches!(
            event,
            OrderEventAny::ModifyRejected(rejected) if rejected.client_order_id == client_order_id
        )
    })
    .await
    {
        OrderEventAny::ModifyRejected(rejected) => {
            assert_eq!(rejected.client_order_id, client_order_id);
            assert!(
                rejected.reason.as_str().contains("No instIdCode cached"),
                "reason was: {}",
                rejected.reason
            );
        }
        other => panic!("expected OrderModifyRejected event, was {other:?}"),
    }
}

fn dispatch_send_failed_response(
    op: OKXWsOperation,
    client_order_id: ClientOrderId,
) -> (Vec<ExecutionEvent>, WsDispatchState) {
    let (emitter, mut rx) = test_emitter();
    let state = state_with_order_identity(client_order_id);

    dispatch_command_response(
        OKXWsMessage::SendFailed {
            request_id: "req-send-failure".to_string(),
            client_order_id: Some(client_order_id),
            op: Some(op),
            error: "send failed after retries".to_string(),
        },
        &emitter,
        &state,
    );

    (drain_events(&mut rx), state)
}

fn dispatch_explicit_rejection_response(
    op: OKXWsOperation,
    client_order_id: ClientOrderId,
) -> Vec<ExecutionEvent> {
    let (emitter, mut rx) = test_emitter();
    let state = state_with_order_identity(client_order_id);

    dispatch_command_response(
        OKXWsMessage::OrderResponse {
            id: Some("req-explicit-reject".to_string()),
            op,
            code: "1".to_string(),
            msg: "All operations failed".to_string(),
            data: vec![json!({
                "sCode": "51000",
                "sMsg": "Order rejected by venue",
                "clOrdId": client_order_id.as_str(),
                "ordId": "12345",
            })],
        },
        &emitter,
        &state,
    );

    drain_events(&mut rx)
}

fn dispatch_command_response(
    message: OKXWsMessage,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
) {
    let instruments = AtomicMap::new();
    let mut fee_cache: AHashMap<Ustr, Money> = AHashMap::new();
    let mut filled_qty_cache: AHashMap<Ustr, Quantity> = AHashMap::new();
    let mut order_state_cache: AHashMap<ClientOrderId, OrderStateSnapshot> = AHashMap::new();

    dispatch_ws_message(
        message,
        emitter,
        state,
        AccountId::from("OKX-001"),
        &instruments,
        &mut fee_cache,
        &mut filled_qty_cache,
        &mut order_state_cache,
        get_atomic_clock_realtime(),
    );
}

fn state_with_order_identity(client_order_id: ClientOrderId) -> WsDispatchState {
    let state = WsDispatchState::default();
    state.order_identities.insert(
        client_order_id,
        OrderIdentity {
            instrument_id: InstrumentId::from("ETH-USDT-SWAP.OKX"),
            strategy_id: StrategyId::from("STRATEGY-001"),
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
        },
    );
    state
}

fn contains_order_event<F>(events: &[ExecutionEvent], predicate: F) -> bool
where
    F: Fn(&OrderEventAny) -> bool,
{
    events.iter().any(|event| {
        matches!(
            event,
            ExecutionEvent::Order(order_event) if predicate(order_event)
        )
    })
}

async fn recv_order_event_matching<F>(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    predicate: F,
) -> OrderEventAny
where
    F: Fn(&OrderEventAny) -> bool,
{
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    let mut seen = Vec::new();

    loop {
        tokio::select! {
            event = rx.recv() => {
                let Some(event) = event else {
                    panic!("event stream closed before matching order event, seen: {seen:?}");
                };

                if let ExecutionEvent::Order(order_event) = event {
                    if predicate(&order_event) {
                        return order_event;
                    }

                    seen.push(format!("{order_event:?}"));
                }
            }
            () = tokio::time::sleep_until(deadline) => {
                panic!("timed out waiting for matching order event, seen: {seen:?}");
            }
        }
    }
}

fn cache_limit_order(cache: &Rc<RefCell<Cache>>, client_order_id: ClientOrderId) -> OrderAny {
    let order = build_test_limit_order(InstrumentId::from("ETH-USDT-SWAP.OKX"), client_order_id);
    cache
        .borrow_mut()
        .add_order(order.clone(), None, Some(*OKX_CLIENT_ID), false)
        .unwrap();

    order
}

#[rstest]
fn test_batch_cancel_orders_builds_payload() {
    let trader_id = TraderId::from("TRADER-001");
    let strategy_id = StrategyId::from("STRATEGY-001");
    let client_id = Some(*OKX_CLIENT_ID);
    let instrument_id = InstrumentId::from("BTC-USDT.OKX");
    let client_order_id1 = ClientOrderId::new("order1");
    let client_order_id2 = ClientOrderId::new("order2");
    let venue_order_id1 = VenueOrderId::new("venue1");
    let venue_order_id2 = VenueOrderId::new("venue2");

    let cmd = BatchCancelOrders {
        trader_id,
        client_id,
        strategy_id,
        instrument_id,
        cancels: vec![
            CancelOrder {
                trader_id,
                client_id,
                strategy_id,
                instrument_id,
                client_order_id: client_order_id1,
                venue_order_id: Some(venue_order_id1),
                command_id: UUID4::default(),
                ts_init: UnixNanos::default(),
                params: None,
                correlation_id: None,
                causation_id: None,
            },
            CancelOrder {
                trader_id,
                client_id,
                strategy_id,
                instrument_id,
                client_order_id: client_order_id2,
                venue_order_id: Some(venue_order_id2),
                command_id: UUID4::default(),
                ts_init: UnixNanos::default(),
                params: None,
                correlation_id: None,
                causation_id: None,
            },
        ],
        command_id: UUID4::default(),
        ts_init: UnixNanos::default(),
        params: None,
        correlation_id: None,
        causation_id: None,
    };

    let mut payload = Vec::with_capacity(cmd.cancels.len());
    for cancel in &cmd.cancels {
        payload.push((
            cancel.instrument_id,
            Some(cancel.client_order_id),
            cancel.venue_order_id,
        ));
    }

    assert_eq!(payload.len(), 2);
    assert_eq!(payload[0].0, instrument_id);
    assert_eq!(payload[0].1, Some(client_order_id1));
    assert_eq!(payload[0].2, Some(venue_order_id1));
    assert_eq!(payload[1].0, instrument_id);
    assert_eq!(payload[1].1, Some(client_order_id2));
    assert_eq!(payload[1].2, Some(venue_order_id2));
}

#[rstest]
fn test_batch_cancel_orders_with_empty_cancels() {
    let cmd = BatchCancelOrders {
        trader_id: TraderId::from("TRADER-001"),
        client_id: Some(*OKX_CLIENT_ID),
        strategy_id: StrategyId::from("STRATEGY-001"),
        instrument_id: InstrumentId::from("BTC-USDT.OKX"),
        cancels: vec![],
        command_id: UUID4::default(),
        ts_init: UnixNanos::default(),
        params: None,
        correlation_id: None,
        causation_id: None,
    };

    let payload: Vec<(InstrumentId, Option<ClientOrderId>, Option<VenueOrderId>)> =
        Vec::with_capacity(cmd.cancels.len());
    assert_eq!(payload.len(), 0);
}

#[rstest]
fn test_dispatch_order_accepted_passes_through() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    let reports = vec![ExecutionReport::Order(make_order_status_report(
        "O-001",
        OrderStatus::Accepted,
    ))];

    dispatch_execution_reports(reports, &emitter, &state);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], ExecutionEvent::Report(_)));
}

#[rstest]
fn test_dispatch_order_triggered_passes_through() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    let reports = vec![ExecutionReport::Order(make_order_status_report(
        "O-001",
        OrderStatus::Triggered,
    ))];

    dispatch_execution_reports(reports, &emitter, &state);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], ExecutionEvent::Report(_)));
    assert!(
        state
            .triggered_orders
            .contains(&ClientOrderId::new("O-001"))
    );
}

#[rstest]
fn test_dispatch_fill_report_passes_through() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    let reports = vec![ExecutionReport::Fill(make_fill_report("O-001"))];

    dispatch_execution_reports(reports, &emitter, &state);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], ExecutionEvent::Report(_)));
    assert!(state.filled_orders.contains(&ClientOrderId::new("O-001")));
}

#[rstest]
fn test_dispatch_order_status_report_accepted_passes_through() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    let reports = vec![ExecutionReport::Order(make_order_status_report(
        "O-001",
        OrderStatus::Accepted,
    ))];

    dispatch_execution_reports(reports, &emitter, &state);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
}

#[rstest]
fn test_dispatch_order_accepted_skipped_when_already_triggered() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    state.triggered_orders.insert(ClientOrderId::new("O-001"));

    let reports = vec![ExecutionReport::Order(make_order_status_report(
        "O-001",
        OrderStatus::Accepted,
    ))];
    dispatch_execution_reports(reports, &emitter, &state);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 0);
}

#[rstest]
fn test_dispatch_order_accepted_skipped_when_already_filled() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    state.filled_orders.insert(ClientOrderId::new("O-001"));

    let reports = vec![ExecutionReport::Order(make_order_status_report(
        "O-001",
        OrderStatus::Accepted,
    ))];
    dispatch_execution_reports(reports, &emitter, &state);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 0);
}

#[rstest]
fn test_dispatch_order_triggered_skipped_when_already_filled() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    state.filled_orders.insert(ClientOrderId::new("O-001"));

    let reports = vec![ExecutionReport::Order(make_order_status_report(
        "O-001",
        OrderStatus::Triggered,
    ))];
    dispatch_execution_reports(reports, &emitter, &state);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 0);
}

#[rstest]
fn test_dispatch_status_report_accepted_skipped_when_triggered() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    state.triggered_orders.insert(ClientOrderId::new("O-001"));

    let reports = vec![ExecutionReport::Order(make_order_status_report(
        "O-001",
        OrderStatus::Accepted,
    ))];
    dispatch_execution_reports(reports, &emitter, &state);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 0);
}

#[rstest]
fn test_dispatch_status_report_accepted_skipped_when_filled() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    state.filled_orders.insert(ClientOrderId::new("O-001"));

    let reports = vec![ExecutionReport::Order(make_order_status_report(
        "O-001",
        OrderStatus::Accepted,
    ))];
    dispatch_execution_reports(reports, &emitter, &state);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 0);
}

#[rstest]
fn test_dispatch_status_report_triggered_skipped_when_filled() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    state.filled_orders.insert(ClientOrderId::new("O-001"));

    let reports = vec![ExecutionReport::Order(make_order_status_report(
        "O-001",
        OrderStatus::Triggered,
    ))];
    dispatch_execution_reports(reports, &emitter, &state);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 0);
}

#[rstest]
fn test_dispatch_status_report_triggered_records_state() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();

    let reports = vec![ExecutionReport::Order(make_order_status_report(
        "O-001",
        OrderStatus::Triggered,
    ))];
    dispatch_execution_reports(reports, &emitter, &state);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert!(
        state
            .triggered_orders
            .contains(&ClientOrderId::new("O-001"))
    );
}

#[rstest]
fn test_dispatch_status_report_filled_records_state() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();

    let reports = vec![ExecutionReport::Order(make_order_status_report(
        "O-001",
        OrderStatus::Filled,
    ))];
    dispatch_execution_reports(reports, &emitter, &state);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert!(state.filled_orders.contains(&ClientOrderId::new("O-001")));
}

#[rstest]
fn test_dispatch_dedup_does_not_affect_different_orders() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    state.filled_orders.insert(ClientOrderId::new("O-001"));

    let reports = vec![ExecutionReport::Order(make_order_status_report(
        "O-002",
        OrderStatus::Accepted,
    ))];
    dispatch_execution_reports(reports, &emitter, &state);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
}

#[rstest]
fn test_dispatch_full_lifecycle_stale_accepted_skipped() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();

    // 1. Triggered arrives first (from business WS)
    dispatch_execution_reports(
        vec![ExecutionReport::Order(make_order_status_report(
            "O-001",
            OrderStatus::Triggered,
        ))],
        &emitter,
        &state,
    );

    // 2. Fill arrives (from private WS)
    dispatch_execution_reports(
        vec![ExecutionReport::Fill(make_fill_report("O-001"))],
        &emitter,
        &state,
    );

    // 3. Stale Accepted arrives late (from private WS)
    dispatch_execution_reports(
        vec![ExecutionReport::Order(make_order_status_report(
            "O-001",
            OrderStatus::Accepted,
        ))],
        &emitter,
        &state,
    );

    // 4. Stale Triggered arrives late (from private WS)
    dispatch_execution_reports(
        vec![ExecutionReport::Order(make_order_status_report(
            "O-001",
            OrderStatus::Triggered,
        ))],
        &emitter,
        &state,
    );

    let events = drain_events(&mut rx);
    // Only the first Triggered report and the Fill should have been emitted
    assert_eq!(events.len(), 2);
}

#[rstest]
fn test_dispatch_status_report_accepted_skipped_when_canceled() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();

    dispatch_execution_reports(
        vec![ExecutionReport::Order(make_order_status_report(
            "O-001",
            OrderStatus::Canceled,
        ))],
        &emitter,
        &state,
    );

    // Stale Accepted replayed after cancel must be dropped, not forwarded.
    dispatch_execution_reports(
        vec![ExecutionReport::Order(make_order_status_report(
            "O-001",
            OrderStatus::Accepted,
        ))],
        &emitter,
        &state,
    );

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert!(state.terminal_orders.contains(&ClientOrderId::new("O-001")));
}

#[rstest]
fn test_dispatch_spread_order_accept_then_cancel() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    let instruments = spread_instruments_cache();
    let mut filled_qty_cache = AHashMap::new();
    let mut order_state_cache = AHashMap::new();
    let cid = ClientOrderId::new("OSPRD001");
    let venue_order_id = "3386544889978159104";
    track_spread_order(&state, cid);

    dispatch_spread_message(
        make_spread_order_msg(OKXOrderStatus::Live, cid, venue_order_id),
        &emitter,
        &state,
        &instruments,
        &mut filled_qty_cache,
        &mut order_state_cache,
    );

    let accepted = drain_events(&mut rx);
    assert_eq!(accepted.len(), 1);
    match &accepted[0] {
        ExecutionEvent::Order(OrderEventAny::Accepted(event)) => {
            assert_eq!(event.client_order_id, cid);
            assert_eq!(event.venue_order_id, VenueOrderId::new(venue_order_id));
            assert_eq!(
                event.instrument_id,
                InstrumentId::from("BCH-USDT_BCH-USDT-SWAP.OKX")
            );
        }
        other => panic!("Expected Accepted spread order event, was {other:?}"),
    }

    dispatch_spread_message(
        make_spread_order_msg(OKXOrderStatus::Canceled, cid, venue_order_id),
        &emitter,
        &state,
        &instruments,
        &mut filled_qty_cache,
        &mut order_state_cache,
    );

    let canceled = drain_events(&mut rx);
    assert_eq!(canceled.len(), 1);
    match &canceled[0] {
        ExecutionEvent::Order(OrderEventAny::Canceled(event)) => {
            assert_eq!(event.client_order_id, cid);
            assert_eq!(
                event.venue_order_id,
                Some(VenueOrderId::new(venue_order_id))
            );
        }
        other => panic!("Expected Canceled spread order event, was {other:?}"),
    }

    assert!(state.order_identities.get(&cid).is_none());
    assert!(!state.emitted_accepted.contains(&cid));
}

#[rstest]
fn test_dispatch_spread_order_cancel_synthesizes_accepted() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    let instruments = spread_instruments_cache();
    let mut filled_qty_cache = AHashMap::new();
    let mut order_state_cache = AHashMap::new();
    let cid = ClientOrderId::new("OSPRD002");
    let venue_order_id = "3386544889978159105";
    track_spread_order(&state, cid);

    dispatch_spread_message(
        make_spread_order_msg(OKXOrderStatus::Canceled, cid, venue_order_id),
        &emitter,
        &state,
        &instruments,
        &mut filled_qty_cache,
        &mut order_state_cache,
    );

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 2);
    match (&events[0], &events[1]) {
        (
            ExecutionEvent::Order(OrderEventAny::Accepted(accepted)),
            ExecutionEvent::Order(OrderEventAny::Canceled(canceled)),
        ) => {
            assert_eq!(accepted.client_order_id, cid);
            assert_eq!(accepted.venue_order_id, VenueOrderId::new(venue_order_id));
            assert_eq!(canceled.client_order_id, cid);
            assert_eq!(
                canceled.venue_order_id,
                Some(VenueOrderId::new(venue_order_id))
            );
        }
        other => panic!("Expected Accepted then Canceled spread events, was {other:?}"),
    }

    assert!(state.order_identities.get(&cid).is_none());
}

#[rstest]
fn test_dispatch_spread_order_live_update_emits_updated() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    let instruments = spread_instruments_cache();
    let mut filled_qty_cache = AHashMap::new();
    let mut order_state_cache = AHashMap::new();
    let cid = ClientOrderId::new("OSPRD003");
    let venue_order_id = "3386544889978159106";
    track_spread_order(&state, cid);

    dispatch_spread_message(
        make_spread_order_msg(OKXOrderStatus::Live, cid, venue_order_id),
        &emitter,
        &state,
        &instruments,
        &mut filled_qty_cache,
        &mut order_state_cache,
    );

    let accepted = drain_events(&mut rx);
    assert_eq!(accepted.len(), 1);
    assert!(matches!(
        &accepted[0],
        ExecutionEvent::Order(OrderEventAny::Accepted(_))
    ));

    let mut updated = make_spread_order_msg(OKXOrderStatus::Live, cid, venue_order_id);
    updated.px = "1.1".to_string();
    updated.sz = "0.02".to_string();

    dispatch_spread_message(
        updated,
        &emitter,
        &state,
        &instruments,
        &mut filled_qty_cache,
        &mut order_state_cache,
    );

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    match &events[0] {
        ExecutionEvent::Order(OrderEventAny::Updated(event)) => {
            assert_eq!(event.client_order_id, cid);
            assert_eq!(
                event.venue_order_id,
                Some(VenueOrderId::new(venue_order_id))
            );
            assert_eq!(event.quantity, Quantity::from("0.02"));
            assert_eq!(event.price, Some(Price::from("1.1")));
        }
        other => panic!("Expected Updated spread order event, was {other:?}"),
    }

    assert!(state.order_identities.get(&cid).is_some());
}

#[rstest]
fn test_dispatch_spread_order_fill_synthesizes_accepted_and_dedups_replay() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    let instruments = spread_instruments_cache();
    let mut filled_qty_cache = AHashMap::new();
    let mut order_state_cache = AHashMap::new();
    let cid = ClientOrderId::new("OSPRD004");
    let venue_order_id = "3386544889978159107";
    track_spread_order(&state, cid);

    let mut fill = make_spread_order_msg(OKXOrderStatus::Filled, cid, venue_order_id);
    fill.fill_sz = "0.01".to_string();
    fill.fill_px = "1.0".to_string();
    fill.acc_fill_sz = "0.01".to_string();
    fill.trade_id = Ustr::from("TSPRD001");

    dispatch_spread_message(
        fill.clone(),
        &emitter,
        &state,
        &instruments,
        &mut filled_qty_cache,
        &mut order_state_cache,
    );

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 2);
    match (&events[0], &events[1]) {
        (
            ExecutionEvent::Order(OrderEventAny::Accepted(accepted)),
            ExecutionEvent::Order(OrderEventAny::Filled(filled)),
        ) => {
            assert_eq!(accepted.client_order_id, cid);
            assert_eq!(filled.client_order_id, cid);
            assert_eq!(filled.trade_id, TradeId::new("TSPRD001"));
            assert_eq!(filled.last_qty, Quantity::from("0.01"));
            assert_eq!(filled.last_px, Price::from("1.0"));
        }
        other => panic!("Expected Accepted then Filled spread events, was {other:?}"),
    }

    assert!(state.order_identities.get(&cid).is_none());
    assert!(state.filled_orders.contains(&cid));

    dispatch_spread_message(
        fill,
        &emitter,
        &state,
        &instruments,
        &mut filled_qty_cache,
        &mut order_state_cache,
    );

    let replay_events = drain_events(&mut rx);
    assert_eq!(replay_events.len(), 0);
}

#[rstest]
fn test_dispatch_spread_post_only_cancel_emits_rejected() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    let instruments = spread_instruments_cache();
    let mut filled_qty_cache = AHashMap::new();
    let mut order_state_cache = AHashMap::new();
    let cid = ClientOrderId::new("OSPRD005");
    let venue_order_id = "3386544889978159108";
    track_spread_order(&state, cid);

    let mut canceled = make_spread_order_msg(OKXOrderStatus::Canceled, cid, venue_order_id);
    canceled.cancel_source = OKX_POST_ONLY_CANCEL_SOURCE.to_string();

    dispatch_spread_message(
        canceled,
        &emitter,
        &state,
        &instruments,
        &mut filled_qty_cache,
        &mut order_state_cache,
    );

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    match &events[0] {
        ExecutionEvent::Order(OrderEventAny::Rejected(event)) => {
            assert_eq!(event.client_order_id, cid);
            assert!(event.due_post_only);
        }
        other => panic!("Expected Rejected spread order event, was {other:?}"),
    }

    assert!(state.order_identities.get(&cid).is_none());
}

#[rstest]
fn test_dispatch_untracked_spread_order_emits_status_report() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    let instruments = spread_instruments_cache();
    let mut filled_qty_cache = AHashMap::new();
    let mut order_state_cache = AHashMap::new();
    let cid = ClientOrderId::new("OSPRD006");
    let venue_order_id = "3386544889978159109";

    dispatch_spread_message(
        make_spread_order_msg(OKXOrderStatus::Live, cid, venue_order_id),
        &emitter,
        &state,
        &instruments,
        &mut filled_qty_cache,
        &mut order_state_cache,
    );

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    match &events[0] {
        ExecutionEvent::Report(CommonExecutionReport::Order(report)) => {
            assert_eq!(report.client_order_id, Some(cid));
            assert_eq!(report.venue_order_id, VenueOrderId::new(venue_order_id));
            assert_eq!(
                report.instrument_id,
                InstrumentId::from("BCH-USDT_BCH-USDT-SWAP.OKX")
            );
            assert_eq!(report.order_status, OrderStatus::Accepted);
        }
        other => panic!("Expected untracked spread order status report, was {other:?}"),
    }
}

fn make_order_init(
    client_order_id: ClientOrderId,
    instrument_id: InstrumentId,
) -> OrderInitialized {
    OrderInitialized {
        client_order_id,
        instrument_id,
        ..Default::default()
    }
}

#[rstest]
fn test_submit_order_list_builds_individual_commands() {
    let trader_id = TraderId::from("TESTER-001");
    let strategy_id = StrategyId::from("STRATEGY-001");
    let client_id = Some(*OKX_CLIENT_ID);
    let instrument_id = InstrumentId::from("ETH-USDT-SWAP.OKX");

    let cid1 = ClientOrderId::new("order1");
    let cid2 = ClientOrderId::new("order2");
    let cid3 = ClientOrderId::new("order3");

    let order_list = OrderList::new(
        OrderListId::new("OL-001"),
        instrument_id,
        strategy_id,
        vec![cid1, cid2, cid3],
        UnixNanos::default(),
    );

    let order_inits = vec![
        make_order_init(cid1, instrument_id),
        make_order_init(cid2, instrument_id),
        make_order_init(cid3, instrument_id),
    ];

    let cmd = SubmitOrderList::new(
        trader_id,
        client_id,
        strategy_id,
        order_list,
        order_inits,
        None,
        None,
        None,
        UUID4::default(),
        UnixNanos::default(),
        None, // correlation_id
    );

    // Verify each SubmitOrder can be constructed from the list
    let submits: Vec<SubmitOrder> = cmd
        .order_list
        .client_order_ids
        .iter()
        .zip(cmd.order_inits.iter())
        .map(|(cid, init)| SubmitOrder {
            trader_id: cmd.trader_id,
            client_id: cmd.client_id,
            strategy_id: cmd.strategy_id,
            instrument_id: cmd.instrument_id,
            client_order_id: *cid,
            order_init: init.clone(),
            exec_algorithm_id: cmd.exec_algorithm_id,
            position_id: cmd.position_id,
            params: cmd.params.clone(),
            command_id: cmd.command_id,
            ts_init: cmd.ts_init,
            correlation_id: None,
            causation_id: None,
        })
        .collect();

    assert_eq!(submits.len(), 3);
    assert_eq!(submits[0].client_order_id, cid1);
    assert_eq!(submits[1].client_order_id, cid2);
    assert_eq!(submits[2].client_order_id, cid3);

    for submit in &submits {
        assert_eq!(submit.trader_id, trader_id);
        assert_eq!(submit.strategy_id, strategy_id);
        assert_eq!(submit.client_id, client_id);
        assert_eq!(submit.instrument_id, instrument_id);
    }
}

#[rstest]
fn test_submit_order_list_single_order() {
    let trader_id = TraderId::from("TESTER-001");
    let strategy_id = StrategyId::from("STRATEGY-001");
    let instrument_id = InstrumentId::from("ETH-USDT-SWAP.OKX");
    let cid = ClientOrderId::new("order1");

    let order_list = OrderList::new(
        OrderListId::new("OL-001"),
        instrument_id,
        strategy_id,
        vec![cid],
        UnixNanos::default(),
    );

    let order_inits = vec![make_order_init(cid, instrument_id)];

    let cmd = SubmitOrderList::new(
        trader_id,
        Some(*OKX_CLIENT_ID),
        strategy_id,
        order_list,
        order_inits,
        None,
        None,
        None,
        UUID4::default(),
        UnixNanos::default(),
        None, // correlation_id
    );

    let submits: Vec<SubmitOrder> = cmd
        .order_list
        .client_order_ids
        .iter()
        .zip(cmd.order_inits.iter())
        .map(|(cid, init)| SubmitOrder {
            trader_id: cmd.trader_id,
            client_id: cmd.client_id,
            strategy_id: cmd.strategy_id,
            instrument_id: cmd.instrument_id,
            client_order_id: *cid,
            order_init: init.clone(),
            exec_algorithm_id: cmd.exec_algorithm_id,
            position_id: cmd.position_id,
            params: cmd.params.clone(),
            command_id: cmd.command_id,
            ts_init: cmd.ts_init,
            correlation_id: None,
            causation_id: None,
        })
        .collect();

    assert_eq!(submits.len(), 1);
    assert_eq!(submits[0].client_order_id, cid);
}

fn make_algo_cancel_response(
    algo_id: &str,
    s_code: &str,
    s_msg: &str,
) -> OKXCancelAlgoOrderResponse {
    OKXCancelAlgoOrderResponse {
        algo_id: algo_id.to_string(),
        s_code: Some(s_code.to_string()),
        s_msg: Some(s_msg.to_string()),
    }
}

fn make_algo_cancel_context(cid: &str) -> AlgoCancelContext {
    AlgoCancelContext {
        client_order_id: ClientOrderId::new(cid),
        instrument_id: InstrumentId::from("ETH-USDT-SWAP.OKX"),
        strategy_id: StrategyId::from("STRATEGY-001"),
        venue_order_id: Some(VenueOrderId::new("v-algo-1")),
    }
}

fn make_fill_report_with_trade_id(cid: &str, trade_id: &str) -> FillReport {
    FillReport::new(
        AccountId::from("OKX-001"),
        InstrumentId::from("ETH-USDT-SWAP.OKX"),
        VenueOrderId::new("v-1"),
        TradeId::new(trade_id),
        OrderSide::Buy,
        Quantity::new(1.0, 0),
        Price::new(2000.0, 2),
        Money::new(0.01, Currency::USDT()),
        LiquiditySide::Taker,
        Some(ClientOrderId::new(cid)),
        None,
        UnixNanos::default(),
        UnixNanos::default(),
        None,
    )
}

#[rstest]
fn test_trade_dedup_first_insert_returns_false() {
    let state = WsDispatchState::default();
    let trade_id = TradeId::new("t-100");

    assert!(!state.check_and_insert_trade(trade_id));
}

#[rstest]
fn test_trade_dedup_second_insert_returns_true() {
    let state = WsDispatchState::default();
    let trade_id = TradeId::new("t-100");

    state.check_and_insert_trade(trade_id);

    assert!(state.check_and_insert_trade(trade_id));
}

#[rstest]
fn test_trade_dedup_different_trade_ids_are_independent() {
    let state = WsDispatchState::default();

    state.check_and_insert_trade(TradeId::new("t-100"));

    assert!(!state.check_and_insert_trade(TradeId::new("t-200")));
}

#[rstest]
fn test_dispatch_duplicate_fill_report_is_suppressed() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();

    let fill = make_fill_report_with_trade_id("O-001", "t-dup-1");
    dispatch_execution_reports(vec![ExecutionReport::Fill(fill.clone())], &emitter, &state);
    dispatch_execution_reports(vec![ExecutionReport::Fill(fill)], &emitter, &state);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1, "duplicate fill should be suppressed");
}

#[rstest]
fn test_dispatch_fills_with_different_trade_ids_both_emitted() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();

    let fill1 = make_fill_report_with_trade_id("O-001", "t-1");
    let fill2 = make_fill_report_with_trade_id("O-001", "t-2");
    dispatch_execution_reports(
        vec![ExecutionReport::Fill(fill1), ExecutionReport::Fill(fill2)],
        &emitter,
        &state,
    );

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 2, "different trade_ids should both emit");
}

#[rstest]
fn test_dispatch_duplicate_fill_still_updates_filled_state() {
    let (emitter, _rx) = test_emitter();
    let state = WsDispatchState::default();
    let cid = ClientOrderId::new("O-001");

    let fill = make_fill_report_with_trade_id("O-001", "t-dup-2");
    dispatch_execution_reports(vec![ExecutionReport::Fill(fill.clone())], &emitter, &state);

    assert!(state.filled_orders.contains(&cid));

    dispatch_execution_reports(vec![ExecutionReport::Fill(fill)], &emitter, &state);

    assert!(state.filled_orders.contains(&cid));
}

#[rstest]
fn test_algo_cancel_rejection_emits_for_nonzero_scode() {
    let (emitter, mut rx) = test_emitter();
    let clock = get_atomic_clock_realtime();

    let responses = vec![make_algo_cancel_response(
        "algo-1",
        "51000",
        "Order not found",
    )];
    let contexts = vec![make_algo_cancel_context("O-001")];

    emit_algo_cancel_rejections(&responses, &contexts, &emitter, clock);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);

    if let ExecutionEvent::Order(event) = &events[0] {
        assert!(
            format!("{event:?}").contains("CancelRejected"),
            "expected CancelRejected event, was {event:?}"
        );
    } else {
        panic!("expected ExecutionEvent::Order, was {:?}", events[0]);
    }
}

#[rstest]
fn test_algo_cancel_rejection_skips_success_scode() {
    let (emitter, mut rx) = test_emitter();
    let clock = get_atomic_clock_realtime();

    let responses = vec![make_algo_cancel_response("algo-1", "0", "")];
    let contexts = vec![make_algo_cancel_context("O-001")];

    emit_algo_cancel_rejections(&responses, &contexts, &emitter, clock);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 0, "sCode=0 should not emit rejection");
}

#[rstest]
fn test_algo_cancel_rejection_mixed_batch() {
    let (emitter, mut rx) = test_emitter();
    let clock = get_atomic_clock_realtime();

    let responses = vec![
        make_algo_cancel_response("algo-1", "0", ""),
        make_algo_cancel_response("algo-2", "51000", "Not found"),
        make_algo_cancel_response("algo-3", "0", ""),
    ];
    let contexts = vec![
        make_algo_cancel_context("O-001"),
        make_algo_cancel_context("O-002"),
        make_algo_cancel_context("O-003"),
    ];

    emit_algo_cancel_rejections(&responses, &contexts, &emitter, clock);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1, "only one rejection in the batch");
}

#[rstest]
fn test_algo_cancel_rejection_missing_context_does_not_panic() {
    let (emitter, mut rx) = test_emitter();
    let clock = get_atomic_clock_realtime();

    // More responses than contexts
    let responses = vec![
        make_algo_cancel_response("algo-1", "51000", "error"),
        make_algo_cancel_response("algo-2", "51000", "error"),
    ];
    let contexts = vec![make_algo_cancel_context("O-001")];

    emit_algo_cancel_rejections(&responses, &contexts, &emitter, clock);

    let events = drain_events(&mut rx);
    // First item has context -> emits rejection; second has no context -> logs warning
    assert_eq!(events.len(), 1);
}

#[rstest]
fn test_batch_cancel_failure_does_not_emit_rejections() {
    let (emitter, mut rx) = test_emitter();
    let clock = get_atomic_clock_realtime();

    let contexts = vec![
        make_algo_cancel_context("O-001"),
        make_algo_cancel_context("O-002"),
        make_algo_cancel_context("O-003"),
    ];

    emit_batch_cancel_failure(&contexts, "network timeout", &emitter, clock);

    let events = drain_events(&mut rx);
    assert_eq!(
        events.len(),
        0,
        "whole batch failure should not emit per-order rejection"
    );
}

#[rstest]
fn test_batch_cancel_failure_empty_contexts() {
    let (emitter, mut rx) = test_emitter();
    let clock = get_atomic_clock_realtime();

    emit_batch_cancel_failure(&[], "error", &emitter, clock);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 0);
}

#[rstest]
#[tokio::test]
async fn test_trade_dedup_concurrent_inserts_only_one_wins() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let state = Arc::new(WsDispatchState::default());
    let trade_id = TradeId::new("t-race");
    let new_count = Arc::new(AtomicUsize::new(0));

    let mut handles = Vec::new();

    for _ in 0..10 {
        let state = Arc::clone(&state);
        let counter = Arc::clone(&new_count);

        handles.push(tokio::spawn(async move {
            if !state.check_and_insert_trade(trade_id) {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        }));
    }

    for h in handles {
        h.await.unwrap();
    }

    assert_eq!(
        new_count.load(Ordering::SeqCst),
        1,
        "exactly one task should see the trade as new"
    );
}

fn load_test_data(filename: &str) -> serde_json::Value {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test_data")
        .join(filename);
    let content = std::fs::read_to_string(path).unwrap();
    serde_json::from_str(&content).unwrap()
}

fn create_exec_test_router() -> Router {
    Router::new().route(
        "/api/v5/account/balance",
        get(|_headers: HeaderMap| async {
            axum::Json(load_test_data("http_get_account_balance.json")).into_response()
        }),
    )
}

#[derive(Default)]
struct ReportRouteState {
    regular_order_pending_queries: tokio::sync::Mutex<Vec<HashMap<String, String>>>,
    regular_order_history_queries: tokio::sync::Mutex<Vec<HashMap<String, String>>>,
    spread_order_pending_queries: tokio::sync::Mutex<Vec<HashMap<String, String>>>,
    spread_order_history_queries: tokio::sync::Mutex<Vec<HashMap<String, String>>>,
    regular_fill_queries: tokio::sync::Mutex<Vec<HashMap<String, String>>>,
    spread_trade_queries: tokio::sync::Mutex<Vec<HashMap<String, String>>>,
}

async fn start_exec_test_server() -> SocketAddr {
    let router = create_exec_test_router();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    let health_url = format!("http://{addr}/api/v5/account/balance");
    let http_client =
        HttpClient::new(HashMap::new(), Vec::new(), Vec::new(), None, None, None).unwrap();
    wait_until_async(
        || {
            let url = health_url.clone();
            let client = http_client.clone();
            async move { client.get(url, None, None, Some(1), None).await.is_ok() }
        },
        Duration::from_secs(5),
    )
    .await;

    addr
}

async fn start_exec_report_test_server(state: Arc<ReportRouteState>) -> SocketAddr {
    let router = create_exec_report_test_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    let health_url = format!("http://{addr}/health");
    let http_client =
        HttpClient::new(HashMap::new(), Vec::new(), Vec::new(), None, None, None).unwrap();
    wait_until_async(
        || {
            let url = health_url.clone();
            let client = http_client.clone();
            async move { client.get(url, None, None, Some(1), None).await.is_ok() }
        },
        Duration::from_secs(5),
    )
    .await;

    addr
}

fn create_exec_report_test_router(state: Arc<ReportRouteState>) -> Router {
    let regular_pending_state = Arc::clone(&state);
    let regular_history_state = Arc::clone(&state);
    let spread_pending_state = Arc::clone(&state);
    let spread_history_state = Arc::clone(&state);
    let regular_fill_state = Arc::clone(&state);
    let spread_trade_state = state;

    Router::new()
        .route("/health", get(|| async { Json(json!({"ok": true})) }))
        .route(
            "/api/v5/trade/orders-pending",
            get(move |Query(params): Query<HashMap<String, String>>| {
                let state = Arc::clone(&regular_pending_state);
                async move {
                    state
                        .regular_order_pending_queries
                        .lock()
                        .await
                        .push(params);
                    Json(json!({"code": "0", "msg": "", "data": []})).into_response()
                }
            }),
        )
        .route(
            "/api/v5/trade/orders-history",
            get(move |Query(params): Query<HashMap<String, String>>| {
                let state = Arc::clone(&regular_history_state);
                async move {
                    state
                        .regular_order_history_queries
                        .lock()
                        .await
                        .push(params);
                    Json(json!({"code": "0", "msg": "", "data": []})).into_response()
                }
            }),
        )
        .route(
            "/api/v5/sprd/orders-pending",
            get(move |Query(params): Query<HashMap<String, String>>| {
                let state = Arc::clone(&spread_pending_state);
                async move {
                    state.spread_order_pending_queries.lock().await.push(params);
                    Json(load_test_data("http_get_spread_orders.json")).into_response()
                }
            }),
        )
        .route(
            "/api/v5/sprd/orders-history",
            get(move |Query(params): Query<HashMap<String, String>>| {
                let state = Arc::clone(&spread_history_state);
                async move {
                    state.spread_order_history_queries.lock().await.push(params);
                    Json(load_test_data("http_get_spread_orders.json")).into_response()
                }
            }),
        )
        .route(
            "/api/v5/trade/fills",
            get(move |Query(params): Query<HashMap<String, String>>| {
                let state = Arc::clone(&regular_fill_state);
                async move {
                    state.regular_fill_queries.lock().await.push(params);
                    Json(json!({"code": "0", "msg": "", "data": []})).into_response()
                }
            }),
        )
        .route(
            "/api/v5/sprd/trades",
            get(move |Query(params): Query<HashMap<String, String>>| {
                let state = Arc::clone(&spread_trade_state);
                async move {
                    state.spread_trade_queries.lock().await.push(params);
                    Json(load_test_data("http_get_spread_trades.json")).into_response()
                }
            }),
        )
}

fn create_test_execution_client(
    base_url: &str,
) -> (
    OKXExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    create_test_execution_client_configured(base_url, |_| {})
}

fn create_test_execution_client_configured(
    base_url: &str,
    configure: impl FnOnce(&mut OKXExecClientConfig),
) -> (
    OKXExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
) {
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("OKX-001");
    let client_id = *OKX_CLIENT_ID;

    let cache = Rc::new(RefCell::new(Cache::default()));

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *OKX_VENUE,
        OmsType::Hedging,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );

    let mut config = OKXExecClientConfig {
        trader_id,
        account_id,
        base_url_http: Some(base_url.to_string()),
        base_url_ws_private: Some("ws://127.0.0.1:19999/ws/v5/private".to_string()),
        base_url_ws_business: Some("ws://127.0.0.1:19999/ws/v5/business".to_string()),
        api_key: Some("test_key".to_string()),
        api_secret: Some("test_secret".to_string()),
        api_passphrase: Some("test_passphrase".to_string()),
        ..Default::default()
    };
    configure(&mut config);

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let client = OKXExecutionClient::new(core, config).unwrap();

    (client, rx, cache)
}

#[rstest]
#[tokio::test]
async fn test_query_account_does_not_block_within_runtime() {
    let addr = start_exec_test_server().await;
    let base_url = format!("http://{addr}");

    let (mut client, mut rx, _cache) = create_test_execution_client(&base_url);

    client.start().unwrap();

    let cmd = QueryAccount::new(
        TraderId::from("TESTER-001"),
        Some(*OKX_CLIENT_ID),
        AccountId::from("OKX-001"),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    let result = client.query_account(cmd);
    result.unwrap();

    wait_until_async(
        || {
            let found = rx
                .try_recv()
                .is_ok_and(|e| matches!(e, ExecutionEvent::Account(_)));
            async move { found }
        },
        Duration::from_secs(5),
    )
    .await;
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_reports_includes_spreads_when_enabled() {
    let state = Arc::new(ReportRouteState::default());
    let addr = start_exec_report_test_server(Arc::clone(&state)).await;
    let base_url = format!("http://{addr}");
    let (mut client, _rx, _cache) = create_test_execution_client_configured(&base_url, |config| {
        config.instrument_types = vec![OKXInstrumentType::Swap];
        config.load_spreads = true;
    });
    client.on_instrument(make_report_spread_instrument());

    let cmd = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        false,
        None,
        None,
        None,
        None,
        None,
    );

    let reports = client.generate_order_status_reports(&cmd).await.unwrap();
    let regular_pending_queries = state.regular_order_pending_queries.lock().await;
    let regular_history_queries = state.regular_order_history_queries.lock().await;
    let spread_pending_queries = state.spread_order_pending_queries.lock().await;
    let spread_history_queries = state.spread_order_history_queries.lock().await;

    assert_eq!(reports.len(), 1);
    assert_eq!(
        reports[0].instrument_id,
        InstrumentId::from("ETH-USD-SWAP_ETH-USD-231229.OKX")
    );
    assert_eq!(
        reports[0].client_order_id,
        Some(ClientOrderId::from("O-spread-entry"))
    );
    assert_eq!(regular_pending_queries.len(), 1);
    assert_eq!(regular_history_queries.len(), 1);
    assert_eq!(spread_pending_queries.len(), 1);
    assert_eq!(spread_history_queries.len(), 1);
    assert!(!spread_pending_queries[0].contains_key("sprdId"));
    assert!(!spread_history_queries[0].contains_key("sprdId"));
}

#[rstest]
#[tokio::test]
async fn test_generate_fill_reports_includes_spreads_when_enabled() {
    let state = Arc::new(ReportRouteState::default());
    let addr = start_exec_report_test_server(Arc::clone(&state)).await;
    let base_url = format!("http://{addr}");
    let (mut client, _rx, _cache) = create_test_execution_client_configured(&base_url, |config| {
        config.instrument_types = vec![OKXInstrumentType::Swap];
        config.load_spreads = true;
    });
    client.on_instrument(make_report_spread_instrument());

    let cmd = GenerateFillReports::new(
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
        None,
        None,
        None,
        None,
    );

    let reports = client.generate_fill_reports(cmd).await.unwrap();
    let regular_fill_queries = state.regular_fill_queries.lock().await;
    let spread_trade_queries = state.spread_trade_queries.lock().await;

    assert_eq!(reports.len(), 1);
    assert_eq!(
        reports[0].instrument_id,
        InstrumentId::from("ETH-USD-SWAP_ETH-USD-231229.OKX")
    );
    assert_eq!(
        reports[0].client_order_id,
        Some(ClientOrderId::from("O-spread-entry"))
    );
    assert_eq!(reports[0].trade_id, TradeId::new("9001"));
    assert_eq!(regular_fill_queries.len(), 1);
    assert_eq!(spread_trade_queries.len(), 1);
    assert!(!spread_trade_queries[0].contains_key("sprdId"));
}

fn make_report_spread_instrument() -> InstrumentAny {
    let instrument = CryptoFuturesSpread::new(
        InstrumentId::from("ETH-USD-SWAP_ETH-USD-231229.OKX"),
        Symbol::from("ETH-USD-SWAP_ETH-USD-231229"),
        Currency::get_or_create_crypto("ETH"),
        Currency::USD(),
        Currency::USD(),
        false,
        Ustr::from("inverse"),
        UnixNanos::default(),
        UnixNanos::default(),
        2,
        0,
        Price::from("0.01"),
        Quantity::from("1"),
        None,
        Some(Quantity::from("1")),
        None,
        Some(Quantity::from("1")),
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
        UnixNanos::default(),
        UnixNanos::default(),
    );

    InstrumentAny::CryptoFuturesSpread(instrument)
}

fn build_test_limit_order(instrument_id: InstrumentId, client_order_id: ClientOrderId) -> OrderAny {
    OrderTestBuilder::new(OrderType::Limit)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("STRATEGY-001"))
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .price(Price::from("2000.00"))
        .quantity(Quantity::from("1"))
        .time_in_force(TimeInForce::Gtc)
        .build()
}

fn collect_order_denied_events(events: Vec<ExecutionEvent>) -> HashMap<ClientOrderId, String> {
    let mut by_cid = HashMap::new();

    for event in events {
        if let ExecutionEvent::Order(OrderEventAny::Denied(denied)) = event {
            by_cid.insert(denied.client_order_id, denied.reason.to_string());
        }
    }
    by_cid
}

#[rstest]
#[tokio::test]
async fn test_submit_order_denies_when_clord_id_exceeds_32_chars() {
    let addr = start_exec_test_server().await;
    let base_url = format!("http://{addr}");
    let (mut client, mut rx, cache) = create_test_execution_client(&base_url);

    client.start().unwrap();
    // Clear any startup events emitted by the background bootstrap task.
    let _ = drain_events(&mut rx);

    let instrument_id = InstrumentId::from("ETH-USDT-SWAP.OKX");
    // 35-char compact ID matching the shape from the original bug report.
    let invalid_cid = ClientOrderId::from("O20260522145501532392555aceLTCUSDT5");
    let order = build_test_limit_order(instrument_id, invalid_cid);

    cache
        .borrow_mut()
        .add_order(order.clone(), None, Some(*OKX_CLIENT_ID), false)
        .unwrap();

    let cmd = SubmitOrder::from_order(
        &order,
        TraderId::from("TESTER-001"),
        Some(*OKX_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
    );

    client
        .submit_order(cmd)
        .expect("submit_order should not error");

    let denied = collect_order_denied_events(drain_events(&mut rx));
    assert_eq!(denied.len(), 1, "denied: {denied:?}");
    let reason = denied.get(&invalid_cid).expect("missing denied event");
    assert!(
        reason.contains("INVALID_CLIENT_ORDER_ID"),
        "reason was: {reason}"
    );
    assert!(reason.contains("at most 32"), "reason was: {reason}");
    assert!(reason.contains("was 35"), "reason was: {reason}");
    assert!(
        reason.contains("use_uuid_client_order_ids"),
        "reason was: {reason}"
    );
}

#[rstest]
#[tokio::test]
async fn test_submit_order_list_denies_every_leg_when_any_clord_id_invalid() {
    let addr = start_exec_test_server().await;
    let base_url = format!("http://{addr}");
    let (mut client, mut rx, cache) = create_test_execution_client(&base_url);

    client.start().unwrap();
    let _ = drain_events(&mut rx);

    let trader_id = TraderId::from("TESTER-001");
    let strategy_id = StrategyId::from("STRATEGY-001");
    let instrument_id = InstrumentId::from("ETH-USDT-SWAP.OKX");

    let cid_valid_a = ClientOrderId::from("O20260522145501ABCDEF1");
    let cid_invalid = ClientOrderId::from("O20260522145501532392555aceLTCUSDT5"); // 35 chars
    let cid_valid_b = ClientOrderId::from("O20260522145501ABCDEF3");

    let order_a = build_test_limit_order(instrument_id, cid_valid_a);
    let order_invalid = build_test_limit_order(instrument_id, cid_invalid);
    let order_b = build_test_limit_order(instrument_id, cid_valid_b);

    for order in [&order_a, &order_invalid, &order_b] {
        cache
            .borrow_mut()
            .add_order(order.clone(), None, Some(*OKX_CLIENT_ID), false)
            .unwrap();
    }

    let order_list = OrderList::new(
        OrderListId::new("OL-001"),
        instrument_id,
        strategy_id,
        vec![cid_valid_a, cid_invalid, cid_valid_b],
        UnixNanos::default(),
    );
    let order_inits = vec![
        OrderInitialized::from(&order_a),
        OrderInitialized::from(&order_invalid),
        OrderInitialized::from(&order_b),
    ];
    let cmd = SubmitOrderList::new(
        trader_id,
        Some(*OKX_CLIENT_ID),
        strategy_id,
        order_list,
        order_inits,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    client
        .submit_order_list(cmd)
        .expect("submit_order_list should not error");

    let mut denied = collect_order_denied_events(drain_events(&mut rx));
    assert_eq!(denied.len(), 3, "denied: {denied:?}");

    let reason_invalid = denied.remove(&cid_invalid).expect("missing invalid leg");
    assert!(
        reason_invalid.contains("INVALID_CLIENT_ORDER_ID")
            && reason_invalid.contains("at most 32")
            && reason_invalid.contains("was 35"),
        "invalid-leg reason was: {reason_invalid}"
    );

    // Sibling legs are denied as part of the list; the offending leg carries the specific reason.
    let reason_a = denied.remove(&cid_valid_a).expect("missing valid leg A");
    assert!(
        reason_a.contains("ORDER_LIST_DENIED") && reason_a.contains("OL-001"),
        "sibling A reason was: {reason_a}"
    );

    let reason_b = denied.remove(&cid_valid_b).expect("missing valid leg B");
    assert!(
        reason_b.contains("ORDER_LIST_DENIED") && reason_b.contains("OL-001"),
        "sibling B reason was: {reason_b}"
    );
}

#[rstest]
#[tokio::test]
async fn test_submit_order_list_denies_spread_instrument() {
    let addr = start_exec_test_server().await;
    let base_url = format!("http://{addr}");
    let (mut client, mut rx, cache) = create_test_execution_client(&base_url);

    client.start().unwrap();
    let _ = drain_events(&mut rx);

    let trader_id = TraderId::from("TESTER-001");
    let strategy_id = StrategyId::from("STRATEGY-001");
    // Spread symbols deny the whole list regardless of clOrdId validity, so use valid IDs.
    let instrument_id = InstrumentId::from("BCH-USDT_BCH-USDT-SWAP.OKX");

    let cid_a = ClientOrderId::from("O20260522145501ABCDEF1");
    let cid_b = ClientOrderId::from("O20260522145501ABCDEF3");

    let order_a = build_test_limit_order(instrument_id, cid_a);
    let order_b = build_test_limit_order(instrument_id, cid_b);

    for order in [&order_a, &order_b] {
        cache
            .borrow_mut()
            .add_order(order.clone(), None, Some(*OKX_CLIENT_ID), false)
            .unwrap();
    }

    let order_list = OrderList::new(
        OrderListId::new("OL-002"),
        instrument_id,
        strategy_id,
        vec![cid_a, cid_b],
        UnixNanos::default(),
    );
    let order_inits = vec![
        OrderInitialized::from(&order_a),
        OrderInitialized::from(&order_b),
    ];
    let cmd = SubmitOrderList::new(
        trader_id,
        Some(*OKX_CLIENT_ID),
        strategy_id,
        order_list,
        order_inits,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    client
        .submit_order_list(cmd)
        .expect("submit_order_list should not error");

    let denied = collect_order_denied_events(drain_events(&mut rx));
    assert_eq!(denied.len(), 2, "denied: {denied:?}");
    for cid in [&cid_a, &cid_b] {
        let reason = denied.get(cid).expect("missing denied leg");
        assert!(
            reason.contains("UNSUPPORTED_ORDER_LIST"),
            "reason was: {reason}"
        );
    }
}
