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
    cell::RefCell,
    collections::HashMap,
    net::SocketAddr,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use ahash::AHashMap;
use axum::{
    Json, Router,
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use futures_util::StreamExt;
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    live::runner::set_exec_event_sender,
    messages::{
        ExecutionEvent,
        execution::{
            BatchCancelOrders, CancelOrder, ExecutionReport as CommonExecutionReport, ModifyOrder,
            QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
            report::{GenerateFillReports, GenerateOrderStatusReport, GenerateOrderStatusReports},
        },
    },
    testing::wait_until_async,
};
use nautilus_core::{AtomicMap, UUID4, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_live::{
    ExecutionClientCore, ExecutionEventEmitter, execution::context::OrderIdentity,
};
use nautilus_model::{
    enums::{
        AccountType, LiquiditySide, OmsType, OrderSide, OrderStatus, OrderType, PositionSide,
        TimeInForce, TriggerType,
    },
    events::{
        OrderEventAny, OrderInitialized,
        order::spec::{
            OrderAcceptedSpec, OrderSubmittedSpec, OrderTriggeredSpec, OrderUpdatedSpec,
        },
    },
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, OrderListId, PositionId, StrategyId, Symbol,
        TradeId, TraderId, VenueOrderId,
    },
    instruments::{
        CryptoFuturesSpread, Instrument, InstrumentAny,
        stubs::{
            crypto_option_btc_deribit, crypto_perpetual_ethusdt, currency_pair_btcusdt,
            currency_pair_ethusdt,
        },
    },
    orders::{Order, OrderAny, OrderList, OrderTestBuilder, stubs::TestOrderEventStubs},
    position::Position,
    reports::{FillReport, OrderStatusReport},
    types::{Currency, Money, Price, Quantity},
};
use nautilus_network::http::HttpClient;
use nautilus_okx::{
    common::{
        consts::{
            OKX_CLIENT_ID, OKX_POST_ONLY_CANCEL_REASON, OKX_POST_ONLY_CANCEL_SOURCE,
            OKX_RECONCILIATION_LOOKBACK_DEFAULT_MINS, OKX_RECONCILIATION_LOOKBACK_MAX_MINS,
            OKX_VENUE,
        },
        enums::{
            OKXEnvironment, OKXInstrumentType, OKXMarginMode, OKXOrderStatus, OKXOrderType,
            OKXSide, OKXTradeMode,
        },
        models::OKXInstrument,
        parse::parse_instrument_any,
    },
    config::OKXExecutionClientConfig,
    execution::OKXExecutionClient,
    http::{
        client::{OKXHttpClient, OKXResponse},
        error::OKXHttpError,
        models::{OKXCancelAlgoOrderResponse, OKXSpreadOrder},
    },
    websocket::{
        dispatch::{
            AlgoCancelContext, WsDispatchState, dispatch_execution_reports, dispatch_ws_message,
            emit_algo_cancel_rejections, emit_batch_cancel_failure,
        },
        enums::{OKXWsChannel, OKXWsOperation},
        error::OKXWsError,
        messages::{
            ExecutionReport, OKXLiquidationWarningMsg, OKXOrderMsg, OKXWebSocketArg, OKXWsFrame,
            OKXWsMessage,
        },
        parse::OrderStateSnapshot,
    },
};
use rstest::rstest;
use serde_json::json;
use ustr::Ustr;

const MARGIN_SPOT_PARENT_CLIENT_ORDER_ID: &str = "OEEADEMOSTOPLIMIT001";
const MARGIN_SPOT_PARENT_VENUE_ORDER_ID: &str = "2497956918703120500";
const MARGIN_SPOT_CHILD_VENUE_ORDER_ID: &str = "2497956918703120501";
const MARGIN_SPOT_TRADE_ID: &str = "1518905600";

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
        OrderSide::Buy.into(),
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
    let instrument = CryptoFuturesSpread::builder()
        .instrument_id(InstrumentId::from("BCH-USDT_BCH-USDT-SWAP.OKX"))
        .raw_symbol(Symbol::from("BCH-USDT_BCH-USDT-SWAP"))
        .underlying(Currency::get_or_create_crypto("BCH"))
        .quote_currency(Currency::USDT())
        .settlement_currency(Currency::USDT())
        .is_inverse(false)
        .strategy_type(Ustr::from("linear"))
        .activation_ns(UnixNanos::default())
        .expiration_ns(UnixNanos::default())
        .price_precision(1)
        .size_precision(2)
        .price_increment(Price::from("0.1"))
        .size_increment(Quantity::from("0.01"))
        .lot_size(Quantity::from("0.01"))
        .min_quantity(Quantity::from("0.01"))
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap();

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

fn load_order_messages(fixture: &str) -> (OKXWebSocketArg, Vec<OKXOrderMsg>) {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test_data")
        .join(fixture);
    let content = std::fs::read_to_string(path).unwrap();
    let frame: OKXWsFrame = serde_json::from_str(&content).unwrap();
    let OKXWsFrame::Data { arg, data } = frame else {
        panic!("Expected private order data frame");
    };
    let order_msgs = serde_json::from_value(data).unwrap();
    (arg, order_msgs)
}

fn make_spread_order_msg(
    state: OKXOrderStatus,
    client_order_id: ClientOrderId,
    venue_order_id: &str,
) -> OKXSpreadOrder {
    OKXSpreadOrder {
        sprd_id: Ustr::from("BCH-USDT_BCH-USDT-SWAP"),
        ord_id: Ustr::from(venue_order_id),
        cl_ord_id: client_order_id.inner(),
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
            client_order_id,
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

async fn recv_query_order_report(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    expected_client_order_id: Option<ClientOrderId>,
) -> OrderStatusReport {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);

    loop {
        tokio::select! {
            event = rx.recv() => {
                let Some(event) = event else {
                    panic!("event stream closed before query order report");
                };

                if let ExecutionEvent::Report(CommonExecutionReport::Order(report)) = event {
                    assert_eq!(report.client_order_id, expected_client_order_id);
                    return *report;
                }
            }
            () = tokio::time::sleep_until(deadline) => {
                panic!("timed out waiting for query order report for {expected_client_order_id:?}");
            }
        }
    }
}

#[rstest]
fn test_ambiguous_submit_send_failure_does_not_emit_order_rejected() {
    let cid = ClientOrderId::new("O-submit-send-failure");
    let (events, state) = dispatch_send_failed_response(
        OKXWsOperation::Order,
        cid,
        OKXWsError::SendFailed("send failed after retries".to_string()),
    );

    assert!(
        !contains_order_event(&events, |event| matches!(event, OrderEventAny::Rejected(_))),
        "ambiguous submit failure should not emit OrderRejected: {events:?}"
    );
    assert!(state.order_identities.contains_key(&cid));
}

#[rstest]
fn test_unsent_submit_send_failure_emits_order_rejected() {
    let cid = ClientOrderId::new("O-submit-handler-unavailable");
    let (events, state) = dispatch_send_failed_response(
        OKXWsOperation::Order,
        cid,
        OKXWsError::HandlerUnavailable("channel closed".to_string()),
    );

    assert!(
        contains_order_event(&events, |event| matches!(
            event,
            OrderEventAny::Rejected(rejected) if rejected.client_order_id == cid
        )),
        "unsent submit failure should emit OrderRejected: {events:?}"
    );
    assert!(!state.order_identities.contains_key(&cid));
}

#[rstest]
fn test_unsent_batch_submit_send_failure_resolves_every_order() {
    let cid_1 = ClientOrderId::new("O-batch-send-failure-1");
    let cid_2 = ClientOrderId::new("O-batch-send-failure-2");
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    for cid in [cid_1, cid_2] {
        state.order_identities.insert(
            cid,
            OrderIdentity {
                client_order_id: cid,
                instrument_id: InstrumentId::from("ETH-USDT-SWAP.OKX"),
                strategy_id: StrategyId::from("STRATEGY-001"),
                order_side: OrderSide::Buy,
                order_type: OrderType::Limit,
            },
        );
    }

    dispatch_command_response(
        OKXWsMessage::SendFailed {
            request_id: "req-batch-send-failure".to_string(),
            client_order_ids: vec![cid_1, cid_2],
            op: Some(OKXWsOperation::BatchOrders),
            error: OKXWsError::NoActiveClient,
        },
        &emitter,
        &state,
    );

    let events = drain_events(&mut rx);

    for cid in [cid_1, cid_2] {
        assert!(
            contains_order_event(&events, |event| matches!(
                event,
                OrderEventAny::Rejected(rejected) if rejected.client_order_id == cid
            )),
            "unsent batch submit should reject {cid}: {events:?}"
        );
        assert!(!state.order_identities.contains_key(&cid));
    }
}

#[rstest]
fn test_ambiguous_batch_submit_send_failure_emits_no_rejections() {
    let cid_1 = ClientOrderId::new("O-batch-ambiguous-1");
    let cid_2 = ClientOrderId::new("O-batch-ambiguous-2");
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    for cid in [cid_1, cid_2] {
        state.order_identities.insert(
            cid,
            OrderIdentity {
                client_order_id: cid,
                instrument_id: InstrumentId::from("ETH-USDT-SWAP.OKX"),
                strategy_id: StrategyId::from("STRATEGY-001"),
                order_side: OrderSide::Buy,
                order_type: OrderType::Limit,
            },
        );
    }

    dispatch_command_response(
        OKXWsMessage::SendFailed {
            request_id: "req-batch-ambiguous".to_string(),
            client_order_ids: vec![cid_1, cid_2],
            op: Some(OKXWsOperation::BatchOrders),
            error: OKXWsError::SendFailed("connection reset".to_string()),
        },
        &emitter,
        &state,
    );

    let events = drain_events(&mut rx);
    assert!(
        !contains_order_event(&events, |event| matches!(event, OrderEventAny::Rejected(_))),
        "ambiguous batch submit failure should not emit rejections: {events:?}"
    );

    for cid in [cid_1, cid_2] {
        assert!(state.order_identities.contains_key(&cid));
    }
}

#[rstest]
fn test_unsent_modify_send_failure_emits_order_modify_rejected() {
    let cid = ClientOrderId::new("O-modify-handler-unavailable");
    let (events, state) = dispatch_send_failed_response(
        OKXWsOperation::AmendOrder,
        cid,
        OKXWsError::HandlerUnavailable("channel closed".to_string()),
    );

    assert!(
        contains_order_event(&events, |event| matches!(
            event,
            OrderEventAny::ModifyRejected(rejected) if rejected.client_order_id == cid
        )),
        "unsent modify failure should emit OrderModifyRejected: {events:?}"
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
fn test_retryable_venue_submit_code_does_not_emit_order_rejected() {
    let cid = ClientOrderId::new("O-submit-system-busy");
    let events = dispatch_venue_code_response(OKXWsOperation::Order, cid, "50013", "System busy");

    assert!(
        !contains_order_event(&events, |event| matches!(
            event,
            OrderEventAny::Rejected(rejected) if rejected.client_order_id == cid
        )),
        "retryable venue submit code should not emit OrderRejected: {events:?}"
    );
}

#[rstest]
fn test_missing_venue_submit_code_does_not_emit_order_rejected() {
    let cid = ClientOrderId::new("O-submit-missing-scode");
    let events =
        dispatch_venue_code_response(OKXWsOperation::Order, cid, "", "All operations failed");

    assert!(
        !contains_order_event(&events, |event| matches!(
            event,
            OrderEventAny::Rejected(rejected) if rejected.client_order_id == cid
        )),
        "missing venue sCode should not emit OrderRejected: {events:?}"
    );
}

#[rstest]
fn test_ambiguous_cancel_send_failure_does_not_emit_order_cancel_rejected() {
    let cid = ClientOrderId::new("O-cancel-send-failure");
    let (events, state) = dispatch_send_failed_response(
        OKXWsOperation::CancelOrder,
        cid,
        OKXWsError::SendFailed("send failed after retries".to_string()),
    );

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
    let (events, state) = dispatch_send_failed_response(
        OKXWsOperation::AmendOrder,
        cid,
        OKXWsError::SendFailed("send failed after retries".to_string()),
    );

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
    error: OKXWsError,
) -> (Vec<ExecutionEvent>, WsDispatchState) {
    let (emitter, mut rx) = test_emitter();
    let state = state_with_order_identity(client_order_id, InstrumentId::from("ETH-USDT-SWAP.OKX"));

    dispatch_command_response(
        OKXWsMessage::SendFailed {
            request_id: "req-send-failure".to_string(),
            client_order_ids: vec![client_order_id],
            op: Some(op),
            error,
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
    dispatch_venue_code_response(op, client_order_id, "51000", "Order rejected by venue")
}

fn dispatch_venue_code_response(
    op: OKXWsOperation,
    client_order_id: ClientOrderId,
    s_code: &str,
    s_msg: &str,
) -> Vec<ExecutionEvent> {
    let (emitter, mut rx) = test_emitter();
    let state = state_with_order_identity(client_order_id, InstrumentId::from("ETH-USDT-SWAP.OKX"));

    dispatch_command_response(
        OKXWsMessage::OrderResponse {
            id: Some("req-explicit-reject".to_string()),
            op,
            code: "1".to_string(),
            msg: "All operations failed".to_string(),
            data: vec![json!({
                "sCode": s_code,
                "sMsg": s_msg,
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

fn state_with_order_identity(
    client_order_id: ClientOrderId,
    instrument_id: InstrumentId,
) -> WsDispatchState {
    let state = WsDispatchState::default();
    state.order_identities.insert(
        client_order_id,
        OrderIdentity {
            client_order_id,
            instrument_id,
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
    assert!(state.contains_accepted(&ClientOrderId::new("O-001")));

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
    assert!(state.contains_triggered(&ClientOrderId::new("O-001")));
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
    assert!(state.contains_filled(&ClientOrderId::new("O-001")));
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
    state.insert_triggered(ClientOrderId::new("O-001"));

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
    state.insert_filled(ClientOrderId::new("O-001"));

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
    state.insert_filled(ClientOrderId::new("O-001"));

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
    state.insert_triggered(ClientOrderId::new("O-001"));

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
    state.insert_filled(ClientOrderId::new("O-001"));

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
    state.insert_filled(ClientOrderId::new("O-001"));

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
    assert!(state.contains_triggered(&ClientOrderId::new("O-001")));
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
    assert!(state.contains_filled(&ClientOrderId::new("O-001")));
}

#[rstest]
fn test_dispatch_dedup_does_not_affect_different_orders() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    state.insert_filled(ClientOrderId::new("O-001"));

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
    assert!(state.contains_terminal(&ClientOrderId::new("O-001")));
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
    assert_eq!(
        state.order_identities.get(&cid).map(|entry| *entry),
        Some(OrderIdentity {
            client_order_id: cid,
            instrument_id: InstrumentId::from("BCH-USDT_BCH-USDT-SWAP.OKX"),
            strategy_id: StrategyId::from("STRATEGY-001"),
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
        })
    );

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
    assert!(!state.contains_accepted(&cid));
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
fn test_dispatch_spread_order_fill_fails_closed_without_fee() {
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
    assert!(
        events.is_empty(),
        "WS sprd-orders omit fee so the fill must stay unprocessed, was {events:?}"
    );
    assert!(state.order_identities.get(&cid).is_some());
    assert!(!state.contains_filled(&cid));
    assert!(!state.check_and_insert_trade(TradeId::new("TSPRD001")));

    dispatch_spread_message(
        fill,
        &emitter,
        &state,
        &instruments,
        &mut filled_qty_cache,
        &mut order_state_cache,
    );

    let replay_events = drain_events(&mut rx);
    assert!(replay_events.is_empty());
    assert!(state.order_identities.get(&cid).is_some());
}

#[rstest]
#[case::live_then_filled(true)]
#[case::filled_only(false)]
fn test_dispatch_tracked_algo_child_fill_with_empty_client_order_id(#[case] send_live: bool) {
    // OKX reports cross-margin child orders as SPOT with the parent algo client ID,
    // while the child client ID may be empty
    let (arg, order_msgs) = load_order_messages("ws_orders_algo_child_filled_empty_cl_ord_id.json");
    assert_eq!(order_msgs.len(), 1);
    let message = &order_msgs[0];
    let client_order_id = ClientOrderId::new("OEEADEMOSTOPLIMIT001");
    let instrument_id = InstrumentId::from("ETH-USDT.OKX");
    let venue_order_id = VenueOrderId::new("2497956918703120501");

    assert_eq!(arg.channel, OKXWsChannel::Orders);
    assert_eq!(arg.inst_type, Some(OKXInstrumentType::Spot));
    assert_eq!(message.inst_id, Ustr::from("ETH-USDT"));
    assert_eq!(message.inst_type, OKXInstrumentType::Spot);
    assert_eq!(message.td_mode, OKXTradeMode::Cross);
    assert_eq!(message.state, OKXOrderStatus::Filled);
    assert_eq!(message.cl_ord_id, "");
    assert_eq!(
        message.algo_cl_ord_id.as_deref(),
        Some(client_order_id.as_str())
    );
    assert_eq!(message.fill_sz, "0.003");
    assert_eq!(message.fill_px, "1886.00");
    assert_eq!(message.trade_id, "1518905600");
    assert_eq!(message.fee.as_deref(), Some("-0.000006"));
    assert_eq!(message.fee_ccy, Ustr::from("ETH"));

    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    state.order_identities.insert(
        client_order_id,
        OrderIdentity {
            client_order_id,
            instrument_id,
            strategy_id: StrategyId::from("STRATEGY-001"),
            order_side: OrderSide::Buy,
            order_type: OrderType::StopLimit,
        },
    );
    state.insert_accepted(client_order_id, VenueOrderId::new("2497956918703120500"));
    state.insert_triggered(client_order_id);

    let mut instrument = currency_pair_ethusdt();
    instrument.id = instrument_id;
    instrument.raw_symbol = Symbol::from("ETH-USDT");
    let instruments = AtomicMap::new();
    instruments.insert(
        Ustr::from("ETH-USDT"),
        InstrumentAny::CurrencyPair(instrument),
    );
    let mut fee_cache = AHashMap::new();
    let mut filled_qty_cache = AHashMap::new();
    let mut order_state_cache = AHashMap::new();

    let assert_venue_order_id_update = |event: &ExecutionEvent| match event {
        ExecutionEvent::Order(OrderEventAny::Updated(updated)) => {
            assert_eq!(updated.client_order_id, client_order_id);
            assert_eq!(updated.venue_order_id, Some(venue_order_id));
            assert_eq!(updated.quantity, Quantity::from("0.003"));
            assert_eq!(updated.price, Some(Price::from("1886.00")));
            assert_eq!(updated.trigger_price, None);
            assert_eq!(updated.account_id, Some(AccountId::from("OKX-001")));
        }
        other => panic!("Expected child venue order ID update, was {other:?}"),
    };

    if send_live {
        let mut live = order_msgs[0].clone();
        live.state = OKXOrderStatus::Live;
        live.acc_fill_sz = Some("0".to_string());
        live.fill_sz.clear();
        live.fill_px.clear();
        live.trade_id.clear();
        live.fee = None;
        live.fill_fee = None;

        dispatch_ws_message(
            OKXWsMessage::Orders(vec![live]),
            &emitter,
            &state,
            AccountId::from("OKX-001"),
            &instruments,
            &mut fee_cache,
            &mut filled_qty_cache,
            &mut order_state_cache,
            get_atomic_clock_realtime(),
        );

        let updated = drain_events(&mut rx);
        assert_eq!(updated.len(), 1);
        assert_venue_order_id_update(&updated[0]);
    }

    dispatch_ws_message(
        OKXWsMessage::Orders(order_msgs.clone()),
        &emitter,
        &state,
        AccountId::from("OKX-001"),
        &instruments,
        &mut fee_cache,
        &mut filled_qty_cache,
        &mut order_state_cache,
        get_atomic_clock_realtime(),
    );

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), if send_live { 1 } else { 2 });
    let filled_index = usize::from(!send_live);
    if !send_live {
        assert_venue_order_id_update(&events[0]);
    }

    match &events[filled_index] {
        ExecutionEvent::Order(OrderEventAny::Filled(filled)) => {
            assert_eq!(filled.trader_id, TraderId::from("TESTER-001"));
            assert_eq!(filled.strategy_id, StrategyId::from("STRATEGY-001"));
            assert_eq!(filled.instrument_id, instrument_id);
            assert_eq!(filled.client_order_id, client_order_id);
            assert_eq!(filled.venue_order_id, venue_order_id);
            assert_eq!(filled.account_id, AccountId::from("OKX-001"));
            assert_eq!(filled.trade_id, TradeId::new("1518905600"));
            assert_eq!(filled.order_side, OrderSide::Buy);
            assert_eq!(filled.order_type, OrderType::StopLimit);
            assert_eq!(filled.last_qty, Quantity::from("0.003"));
            assert_eq!(filled.last_px, Price::from("1886.00"));
            assert_eq!(filled.currency, Currency::USDT());
            assert_eq!(filled.liquidity_side, LiquiditySide::Taker);
            assert_eq!(filled.commission, Some(Money::from("0.000006 ETH")));
            assert!(!filled.reconciliation);
        }
        other => panic!("Expected tracked OrderFilled, was {other:?}"),
    }
    assert!(state.order_identities.get(&client_order_id).is_none());
    assert!(state.contains_filled(&client_order_id));
    assert!(state.contains_terminal(&client_order_id));
    assert!(!state.contains_triggered(&client_order_id));

    dispatch_ws_message(
        OKXWsMessage::Orders(order_msgs),
        &emitter,
        &state,
        AccountId::from("OKX-001"),
        &instruments,
        &mut fee_cache,
        &mut filled_qty_cache,
        &mut order_state_cache,
        get_atomic_clock_realtime(),
    );

    assert!(drain_events(&mut rx).is_empty());
}

#[rstest]
fn test_dispatch_untracked_algo_child_fill_with_empty_client_order_id_as_report() {
    let (_, order_msgs) = load_order_messages("ws_orders_algo_child_filled_empty_cl_ord_id.json");
    let client_order_id = ClientOrderId::new("OEEADEMOSTOPLIMIT001");
    let instrument_id = InstrumentId::from("ETH-USDT.OKX");
    let venue_order_id = VenueOrderId::new("2497956918703120501");
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    let mut instrument = currency_pair_ethusdt();
    instrument.id = instrument_id;
    instrument.raw_symbol = Symbol::from("ETH-USDT");
    let instruments = AtomicMap::new();
    instruments.insert(
        Ustr::from("ETH-USDT"),
        InstrumentAny::CurrencyPair(instrument),
    );
    let mut fee_cache = AHashMap::new();
    let mut filled_qty_cache = AHashMap::new();
    let mut order_state_cache = AHashMap::new();

    dispatch_ws_message(
        OKXWsMessage::Orders(order_msgs),
        &emitter,
        &state,
        AccountId::from("OKX-001"),
        &instruments,
        &mut fee_cache,
        &mut filled_qty_cache,
        &mut order_state_cache,
        get_atomic_clock_realtime(),
    );

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    match &events[0] {
        ExecutionEvent::Report(CommonExecutionReport::Fill(report)) => {
            assert_eq!(report.account_id, AccountId::from("OKX-001"));
            assert_eq!(report.instrument_id, instrument_id);
            assert_eq!(report.client_order_id, Some(client_order_id));
            assert_eq!(report.venue_order_id, venue_order_id);
            assert_eq!(report.trade_id, TradeId::new("1518905600"));
            assert_eq!(report.order_side, OrderSide::Buy);
            assert_eq!(report.last_qty, Quantity::from("0.003"));
            assert_eq!(report.last_px, Price::from("1886.00"));
            assert_eq!(report.commission, Money::from("0.000006 ETH"));
            assert_eq!(report.liquidity_side, LiquiditySide::Taker);
            assert_eq!(report.venue_position_id, None);
        }
        other => panic!("Expected untracked child fill report, was {other:?}"),
    }
    assert!(state.order_identities.is_empty());
    assert!(state.contains_filled(&client_order_id));
}

struct VenueFillCase {
    fixture: &'static str,
    raw_symbol: &'static str,
    instrument_id: InstrumentId,
    venue_order_id: &'static str,
    trade_id: &'static str,
    side: OrderSide,
    qty: &'static str,
    px: &'static str,
    fee: &'static str,
}

#[allow(
    clippy::too_many_arguments,
    reason = "case constructor mirrors the venue fill fields"
)]
fn venue_fill_case(
    fixture: &'static str,
    raw_symbol: &'static str,
    venue_order_id: &'static str,
    trade_id: &'static str,
    side: OrderSide,
    qty: &'static str,
    px: &'static str,
    fee: &'static str,
) -> VenueFillCase {
    VenueFillCase {
        fixture,
        raw_symbol,
        instrument_id: InstrumentId::from(format!("{raw_symbol}.OKX").as_str()),
        venue_order_id,
        trade_id,
        side,
        qty,
        px,
        fee,
    }
}

#[rstest]
#[case::liquidation(venue_fill_case(
    "ws_orders_liquidation.json",
    "BTC-USDT-SWAP",
    "2497956918703120999",
    "1518905999",
    OrderSide::Sell,
    "0.500",
    "40000.00",
    "20 USDT"
))]
#[case::adl(venue_fill_case(
    "ws_orders_adl.json",
    "ETH-USDT-SWAP",
    "2497956918703121000",
    "1518906000",
    OrderSide::Buy,
    "0.300",
    "41000.00",
    "12.3 USDT"
))]
fn test_dispatch_venue_initiated_order_fill_as_report(#[case] case: VenueFillCase) {
    let (_, order_msgs) = load_order_messages(case.fixture);
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    let instruments = AtomicMap::new();
    instruments.insert(
        Ustr::from(case.raw_symbol),
        order_instrument(OKXInstrumentType::Swap, case.instrument_id, case.raw_symbol),
    );
    let mut fee_cache = AHashMap::new();
    let mut filled_qty_cache = AHashMap::new();
    let mut order_state_cache = AHashMap::new();

    dispatch_ws_message(
        OKXWsMessage::Orders(order_msgs),
        &emitter,
        &state,
        AccountId::from("OKX-001"),
        &instruments,
        &mut fee_cache,
        &mut filled_qty_cache,
        &mut order_state_cache,
        get_atomic_clock_realtime(),
    );

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    match &events[0] {
        ExecutionEvent::Report(CommonExecutionReport::Fill(report)) => {
            // Venue-initiated flow (liquidation or ADL): no client order ID,
            // surfaced as a fill report
            assert_eq!(report.account_id, AccountId::from("OKX-001"));
            assert_eq!(report.instrument_id, case.instrument_id);
            assert_eq!(report.client_order_id, None);
            assert_eq!(
                report.venue_order_id,
                VenueOrderId::new(case.venue_order_id)
            );
            assert_eq!(report.trade_id, TradeId::new(case.trade_id));
            assert_eq!(report.order_side, case.side);
            assert_eq!(report.last_qty, Quantity::from(case.qty));
            assert_eq!(report.last_px, Price::from(case.px));
            assert_eq!(report.commission, Money::from(case.fee));
            assert_eq!(report.liquidity_side, LiquiditySide::Taker);
        }
        other => panic!("Expected venue-initiated fill report, was {other:?}"),
    }
}

#[rstest]
fn test_dispatch_positions_channel_emits_position_report() {
    let content = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_data")
            .join("ws_positions.json"),
    )
    .unwrap();
    let frame: OKXWsFrame = serde_json::from_str(&content).unwrap();
    let OKXWsFrame::Data { arg, data } = frame else {
        panic!("Expected data frame");
    };
    assert_eq!(arg.channel, OKXWsChannel::Positions);

    let instrument_id = InstrumentId::from("BTC-USDT-SWAP.OKX");
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    let instruments = AtomicMap::new();
    instruments.insert(
        Ustr::from("BTC-USDT-SWAP"),
        order_instrument(OKXInstrumentType::Swap, instrument_id, "BTC-USDT-SWAP"),
    );
    let mut fee_cache = AHashMap::new();
    let mut filled_qty_cache = AHashMap::new();
    let mut order_state_cache = AHashMap::new();

    dispatch_ws_message(
        OKXWsMessage::Positions(data),
        &emitter,
        &state,
        AccountId::from("OKX-001"),
        &instruments,
        &mut fee_cache,
        &mut filled_qty_cache,
        &mut order_state_cache,
        get_atomic_clock_realtime(),
    );

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    match &events[0] {
        ExecutionEvent::Report(CommonExecutionReport::Position(report)) => {
            assert_eq!(report.account_id, AccountId::from("OKX-001"));
            assert_eq!(report.instrument_id, instrument_id);
            assert_eq!(report.position_side, PositionSide::Long);
            assert_eq!(report.quantity, Quantity::from("0.500"));
            assert_eq!(
                report.venue_position_id,
                Some(PositionId::new("12345-LONG"))
            );
        }
        other => panic!("Expected position report, was {other:?}"),
    }
}

#[rstest]
fn test_dispatch_liquidation_warning_logs_only() {
    let content = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_data")
            .join("ws_liquidation_warning.json"),
    )
    .unwrap();
    let frame: OKXWsFrame = serde_json::from_str(&content).unwrap();
    let OKXWsFrame::Data { arg, data } = frame else {
        panic!("Expected data frame");
    };
    assert_eq!(arg.channel, OKXWsChannel::LiquidationWarning);

    let warnings: Vec<OKXLiquidationWarningMsg> = serde_json::from_value(data).unwrap();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].inst_id, Ustr::from("BTC-USDT-SWAP"));

    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    let instruments = AtomicMap::new();
    let mut fee_cache = AHashMap::new();
    let mut filled_qty_cache = AHashMap::new();
    let mut order_state_cache = AHashMap::new();

    dispatch_ws_message(
        OKXWsMessage::LiquidationWarnings(warnings),
        &emitter,
        &state,
        AccountId::from("OKX-001"),
        &instruments,
        &mut fee_cache,
        &mut filled_qty_cache,
        &mut order_state_cache,
        get_atomic_clock_realtime(),
    );

    // Risk warnings surface as logs only; no execution events
    assert!(drain_events(&mut rx).is_empty());
}

#[rstest]
#[case(
    "ws_orders_post_only_canceled_first.json",
    OKXInstrumentType::Swap,
    OKXOrderType::PostOnly,
    "ETH-USDT-SWAP",
    "ETH-USDT-SWAP.OKX",
    "OPOSTONLYCANCEL001",
    "2497956918703120600"
)]
#[case(
    "ws_orders_mmp_and_post_only_canceled_first.json",
    OKXInstrumentType::Option,
    OKXOrderType::MmpAndPostOnly,
    "BTC-USD-260828-100000-C",
    "BTC-USD-260828-100000-C.OKX",
    "OMMPPOSTONLYCANCEL001",
    "2497956918703120601"
)]
fn test_dispatch_tracked_post_only_cancel_from_fixture(
    #[case] fixture: &str,
    #[case] expected_instrument_type: OKXInstrumentType,
    #[case] expected_order_type: OKXOrderType,
    #[case] raw_symbol: &str,
    #[case] instrument_id: InstrumentId,
    #[case] client_order_id: &str,
    #[case] venue_order_id: &str,
) {
    let client_order_id = ClientOrderId::new(client_order_id);
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test_data")
        .join(fixture);
    let content = std::fs::read_to_string(path).unwrap();
    let frame: OKXWsFrame = serde_json::from_str(&content).unwrap();
    let OKXWsFrame::Data { arg, data } = frame else {
        panic!("Expected private order data frame");
    };
    assert_eq!(arg.channel, OKXWsChannel::Orders);
    assert_eq!(arg.inst_type, Some(expected_instrument_type));

    let order_msgs: Vec<OKXOrderMsg> = serde_json::from_value(data).unwrap();
    assert_eq!(order_msgs.len(), 1);
    let message = order_msgs.into_iter().next().unwrap();
    assert_eq!(message.inst_type, expected_instrument_type);
    assert_eq!(message.ord_type, expected_order_type);
    assert_eq!(message.state, OKXOrderStatus::Canceled);
    assert_eq!(message.acc_fill_sz.as_deref(), Some("0"));
    assert_eq!(message.fill_sz, "0");
    assert_eq!(
        message.cancel_source.as_deref(),
        Some(OKX_POST_ONLY_CANCEL_SOURCE),
    );
    assert_eq!(
        message.cancel_source_reason.as_deref(),
        Some(OKX_POST_ONLY_CANCEL_REASON),
    );
    assert_eq!(message.cl_ord_id, client_order_id.as_str());
    assert_eq!(message.ord_id, Ustr::from(venue_order_id));

    let (emitter, mut rx) = test_emitter();
    let state = state_with_order_identity(client_order_id, instrument_id);
    let instruments = AtomicMap::new();
    instruments.insert(
        Ustr::from(raw_symbol),
        order_instrument(expected_instrument_type, instrument_id, raw_symbol),
    );

    let (untracked_emitter, mut untracked_rx) = test_emitter();
    let untracked_state = WsDispatchState::default();
    let mut untracked_fee_cache = AHashMap::new();
    let mut untracked_filled_qty_cache = AHashMap::new();
    let mut untracked_order_state_cache = AHashMap::new();

    dispatch_ws_message(
        OKXWsMessage::Orders(vec![message.clone()]),
        &untracked_emitter,
        &untracked_state,
        AccountId::from("OKX-001"),
        &instruments,
        &mut untracked_fee_cache,
        &mut untracked_filled_qty_cache,
        &mut untracked_order_state_cache,
        get_atomic_clock_realtime(),
    );

    let untracked_events = drain_events(&mut untracked_rx);
    assert_eq!(untracked_events.len(), 1);
    match &untracked_events[0] {
        ExecutionEvent::Report(CommonExecutionReport::Order(report)) => {
            assert_eq!(report.account_id, AccountId::from("OKX-001"));
            assert_eq!(report.instrument_id, instrument_id);
            assert_eq!(report.client_order_id, Some(client_order_id));
            assert_eq!(report.venue_order_id, VenueOrderId::new(venue_order_id));
            assert_eq!(report.order_side, Some(OrderSide::Buy));
            assert_eq!(report.order_type, OrderType::Limit);
            assert_eq!(report.time_in_force, TimeInForce::Gtc);
            assert_eq!(report.order_status, OrderStatus::Canceled);
            assert!(report.post_only);
            assert_eq!(
                report.cancel_reason.as_deref(),
                Some(OKX_POST_ONLY_CANCEL_REASON),
            );
        }
        other => panic!("Expected first-seen untracked order status report, was {other:?}"),
    }

    let venue_order_id_key = Ustr::from(venue_order_id);
    let mut fee_cache = AHashMap::new();
    fee_cache.insert(venue_order_id_key, Money::new(1.25, Currency::from("USDT")));
    let mut filled_qty_cache = AHashMap::new();
    filled_qty_cache.insert(venue_order_id_key, Quantity::from("0.5"));
    let mut order_state_cache = AHashMap::new();
    order_state_cache.insert(
        client_order_id,
        OrderStateSnapshot {
            venue_order_id: VenueOrderId::new(venue_order_id),
            quantity: Quantity::from("2"),
            price: Some(Price::from("1.25")),
        },
    );

    dispatch_ws_message(
        OKXWsMessage::Orders(vec![message.clone()]),
        &emitter,
        &state,
        AccountId::from("OKX-001"),
        &instruments,
        &mut fee_cache,
        &mut filled_qty_cache,
        &mut order_state_cache,
        get_atomic_clock_realtime(),
    );

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    match events[0] {
        ExecutionEvent::Order(OrderEventAny::Rejected(rejected)) => {
            assert_eq!(rejected.trader_id, TraderId::from("TESTER-001"));
            assert_eq!(rejected.strategy_id, StrategyId::from("STRATEGY-001"));
            assert_eq!(rejected.instrument_id, instrument_id);
            assert_eq!(rejected.client_order_id, client_order_id);
            assert_eq!(rejected.account_id, AccountId::from("OKX-001"));
            assert_eq!(
                rejected.reason,
                Ustr::from("Post-only order would have taken liquidity"),
            );
            assert!(!rejected.reconciliation);
            assert!(rejected.due_post_only);
        }
        ref other => panic!("Expected one OrderRejected, was {other:?}"),
    }

    assert!(!state.order_identities.contains_key(&client_order_id));
    assert!(!order_state_cache.contains_key(&client_order_id));
    assert!(!fee_cache.contains_key(&venue_order_id_key));
    assert!(!filled_qty_cache.contains_key(&venue_order_id_key));

    dispatch_ws_message(
        OKXWsMessage::Orders(vec![message]),
        &emitter,
        &state,
        AccountId::from("OKX-001"),
        &instruments,
        &mut fee_cache,
        &mut filled_qty_cache,
        &mut order_state_cache,
        get_atomic_clock_realtime(),
    );

    let replay_events = drain_events(&mut rx);
    assert_eq!(
        replay_events.len(),
        0,
        "replay should not emit another execution event: {replay_events:?}",
    );
}

#[rstest]
fn test_dispatch_rpi_canceled_first_emits_rejection_without_acceptance() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test_data")
        .join("ws_orders_rpi_canceled_first.json");
    let content = std::fs::read_to_string(path).unwrap();
    let frame: OKXWsFrame = serde_json::from_str(&content).unwrap();
    let OKXWsFrame::Data { arg, data } = frame else {
        panic!("Expected private RPI order data frame");
    };
    let order_msgs: Vec<OKXOrderMsg> = serde_json::from_value(data).unwrap();
    let message = &order_msgs[0];
    let client_order_id = ClientOrderId::new("ORPICANCEL001");
    let instrument_id = InstrumentId::from("OMI-USD.OKX");

    assert_eq!(arg.channel, OKXWsChannel::Orders);
    assert_eq!(arg.inst_type, Some(OKXInstrumentType::Spot));
    assert_eq!(order_msgs.len(), 1);
    assert_eq!(message.inst_id, Ustr::from("OMI-USD"));
    assert_eq!(message.inst_type, OKXInstrumentType::Spot);
    assert_eq!(message.ord_type, OKXOrderType::Rpi);
    assert_eq!(message.state, OKXOrderStatus::Canceled);
    assert_eq!(message.acc_fill_sz.as_deref(), Some("0"));
    assert_eq!(message.fill_sz, "0");
    assert_eq!(message.cancel_source.as_deref(), Some(""));
    assert_eq!(message.cancel_source_reason.as_deref(), Some(""));
    assert_eq!(message.cl_ord_id, client_order_id.as_str());
    assert_eq!(message.ord_id, Ustr::from("2500000000000000001"));

    let (emitter, mut rx) = test_emitter();
    let state = state_with_order_identity(client_order_id, instrument_id);
    let instruments = AtomicMap::new();
    instruments.insert(
        Ustr::from("OMI-USD"),
        order_instrument(OKXInstrumentType::Spot, instrument_id, "OMI-USD"),
    );
    let mut fee_cache = AHashMap::new();
    let mut filled_qty_cache = AHashMap::new();
    let mut order_state_cache = AHashMap::new();

    dispatch_ws_message(
        OKXWsMessage::Orders(order_msgs),
        &emitter,
        &state,
        AccountId::from("OKX-001"),
        &instruments,
        &mut fee_cache,
        &mut filled_qty_cache,
        &mut order_state_cache,
        get_atomic_clock_realtime(),
    );

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    match &events[0] {
        ExecutionEvent::Order(OrderEventAny::Rejected(rejected)) => {
            assert_eq!(rejected.trader_id, TraderId::from("TESTER-001"));
            assert_eq!(rejected.strategy_id, StrategyId::from("STRATEGY-001"));
            assert_eq!(rejected.instrument_id, instrument_id);
            assert_eq!(rejected.client_order_id, client_order_id);
            assert_eq!(rejected.account_id, AccountId::from("OKX-001"));
            assert_eq!(
                rejected.reason,
                Ustr::from("RPI order canceled before acceptance")
            );
            assert!(!rejected.reconciliation);
            assert!(rejected.due_post_only);
        }
        other => panic!("Expected one OrderRejected, was {other:?}"),
    }
    assert!(!events.iter().any(|event| matches!(
        event,
        ExecutionEvent::Order(OrderEventAny::Accepted(_) | OrderEventAny::Canceled(_))
    )));
    assert!(!state.contains_accepted(&client_order_id));
    assert!(!state.order_identities.contains_key(&client_order_id));
}

fn order_instrument(
    instrument_type: OKXInstrumentType,
    instrument_id: InstrumentId,
    raw_symbol: &str,
) -> InstrumentAny {
    match instrument_type {
        OKXInstrumentType::Spot => {
            let mut instrument = currency_pair_btcusdt();
            instrument.id = instrument_id;
            instrument.raw_symbol = Symbol::from(raw_symbol);
            InstrumentAny::CurrencyPair(instrument)
        }
        OKXInstrumentType::Swap => {
            let mut instrument = crypto_perpetual_ethusdt();
            instrument.id = instrument_id;
            instrument.raw_symbol = Symbol::from(raw_symbol);
            InstrumentAny::CryptoPerpetual(instrument)
        }
        OKXInstrumentType::Option => {
            let mut instrument =
                crypto_option_btc_deribit(3, 1, Price::from("0.001"), Quantity::from("0.1"));
            instrument.id = instrument_id;
            instrument.raw_symbol = Symbol::from(raw_symbol);
            InstrumentAny::CryptoOption(instrument)
        }
        other => panic!("Unsupported test instrument type: {other}"),
    }
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

    assert!(state.contains_filled(&cid));

    dispatch_execution_reports(vec![ExecutionReport::Fill(fill)], &emitter, &state);

    assert!(state.contains_filled(&cid));
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

#[rstest]
#[tokio::test]
async fn test_dispatch_fill_reports_claim_trade_id_across_tasks() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::default());
    let fill = make_fill_report_with_trade_id("O-001", "t-race-dispatch");
    let mut handles = Vec::new();

    for _ in 0..8 {
        let emitter = emitter.clone();
        let state = Arc::clone(&state);
        let fill = fill.clone();
        handles.push(tokio::spawn(async move {
            dispatch_execution_reports(vec![ExecutionReport::Fill(fill)], &emitter, &state);
        }));
    }

    for handle in handles {
        handle.await.unwrap();
    }
    let events = drain_events(&mut rx);

    assert_eq!(events.len(), 1, "exactly one fill should be emitted");
    assert!(state.contains_trade(&TradeId::new("t-race-dispatch")));
}

fn load_test_data(filename: &str) -> serde_json::Value {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test_data")
        .join(filename);
    let content = std::fs::read_to_string(path).unwrap();
    serde_json::from_str(&content).unwrap()
}

fn query_order_instrument() -> InstrumentAny {
    let response: OKXResponse<OKXInstrument> =
        serde_json::from_value(load_test_data("http_get_instruments_swap.json")).unwrap();
    let raw = response
        .data
        .iter()
        .find(|instrument| instrument.inst_id == Ustr::from("ETH-USDT-SWAP"))
        .expect("expected ETH-USDT-SWAP fixture");

    parse_instrument_any(raw, None, None, None, None, UnixNanos::default())
        .unwrap()
        .expect("expected parsed ETH-USDT-SWAP instrument")
}

fn btc_usdt_swap_instrument() -> InstrumentAny {
    let response: OKXResponse<OKXInstrument> =
        serde_json::from_value(load_test_data("http_get_instruments_swap.json")).unwrap();
    let raw = response
        .data
        .iter()
        .find(|instrument| instrument.inst_id == Ustr::from("BTC-USDT-SWAP"))
        .expect("expected BTC-USDT-SWAP fixture");

    parse_instrument_any(raw, None, None, None, None, UnixNanos::default())
        .unwrap()
        .expect("expected parsed BTC-USDT-SWAP instrument")
}

fn regular_order_detail_response(params: &HashMap<String, String>) -> serde_json::Value {
    let mut response = load_test_data("http_get_orders_history.json");
    let order = &mut response["data"][0];
    let query_id = params
        .get("clOrdId")
        .or_else(|| params.get("ordId"))
        .map(String::as_str)
        .expect("expected clOrdId or ordId query parameter");
    let (algo_client_order_id, client_order_id, venue_order_id, order_type, state, price) =
        match query_id {
            "regular-venue-id" => (
                "",
                "OQUERYREGULAR1",
                "regular-venue-id",
                "limit",
                "live",
                "2000.00",
            ),
            "OQUERYREGULAR1" => (
                "",
                "OQUERYREGULAR1",
                "reused-regular-venue-id",
                "limit",
                "live",
                "2000.00",
            ),
            "market-child-venue-id" => (
                "OQUERYMARKETCHILD1",
                "",
                "market-child-venue-id",
                "market",
                "filled",
                "1000.00",
            ),
            "triggered-child-venue-id" => (
                "OQUERYTRIGGERED1",
                "",
                "triggered-child-venue-id",
                "limit",
                "filled",
                "900.00",
            ),
            "missed-child-venue-id" => (
                "OQUERYMISSEDCHILD1",
                "",
                "missed-child-venue-id",
                "limit",
                "filled",
                "850.00",
            ),
            "missed-accept-child-venue-id" => (
                "OQUERYMISSEDACCEPT1",
                "",
                "missed-accept-child-venue-id",
                "limit",
                "filled",
                "825.00",
            ),
            "single-child-venue-id" => (
                "OQUERYSINGLECHILD1",
                "",
                "single-child-venue-id",
                "limit",
                "filled",
                "800.00",
            ),
            "mass-triggered-child-venue-id" => (
                "",
                "",
                "mass-triggered-child-venue-id",
                "limit",
                "filled",
                "850.00",
            ),
            other if other.starts_with("mass-cap-child-") => {
                ("", "", other, "limit", "filled", "850.00")
            }
            "external-venue-id" => ("", "", "external-venue-id", "limit", "live", "2000.00"),
            other => panic!("unexpected regular order detail query: {other}"),
        };
    let is_filled = state == "filled";

    order["accFillSz"] = json!(if is_filled { "1" } else { "0" });
    order["algoClOrdId"] = json!(algo_client_order_id);
    order["avgPx"] = json!(if is_filled { price } else { "" });
    order["clOrdId"] = json!(client_order_id);
    order["fillPx"] = json!(if is_filled { price } else { "" });
    order["fillSz"] = json!(if is_filled { "1" } else { "" });
    order["instId"] = json!("ETH-USDT-SWAP");
    order["ordId"] = json!(venue_order_id);
    order["ordType"] = json!(order_type);
    order["posSide"] = json!("net");
    order["px"] = json!(if order_type == "market" { "" } else { price });
    order["state"] = json!(state);
    order["sz"] = json!("1");
    order["tdMode"] = json!("cross");
    response
}

fn algo_order_detail_response(params: &HashMap<String, String>) -> serde_json::Value {
    let mut response = load_test_data("http_get_orders_algo_pending_close_fraction.json");
    let order = &mut response["data"][0];
    order["algoId"] = json!(
        params
            .get("algoId")
            .map_or("unknown-algo-id", String::as_str)
    );
    order["algoClOrdId"] = json!(
        params
            .get("algoClOrdId")
            .map_or("unknown-algo-client-id", String::as_str)
    );
    order["instId"] = json!("ETH-USDT-SWAP");

    match params.get("algoClOrdId").map(String::as_str) {
        Some("OQUERYALGO1") => {
            order["algoId"] = json!("algo-venue-id");
            order["ordId"] = json!("");
            order["state"] = json!("live");
        }
        Some("OQUERYMARKETCHILD1") => {
            order["algoId"] = json!("parent-market-algo-id");
            order["ordId"] = json!("market-child-venue-id");
            order["state"] = json!("effective");
        }
        Some("OQUERYTRIGGERED1") => {
            order["algoId"] = json!("parent-algo-id");
            order["ordId"] = json!("triggered-child-venue-id");
            order["ordIdList"] = json!(["triggered-child-venue-id"]);
            order["slOrdPx"] = json!("900.00");
            order["state"] = json!("effective");
        }
        Some("OQUERYMISSEDCHILD1") => {
            order["algoId"] = json!("parent-missed-algo-id");
            order["ordId"] = json!("missed-child-venue-id");
            order["ordIdList"] = json!(["missed-child-venue-id"]);
            order["slOrdPx"] = json!("850.00");
            order["state"] = json!("effective");
        }
        Some("OQUERYMISSEDACCEPT1") => {
            order["algoId"] = json!("parent-missed-accept-algo-id");
            order["ordId"] = json!("missed-accept-child-venue-id");
            order["ordIdList"] = json!(["missed-accept-child-venue-id"]);
            order["slOrdPx"] = json!("825.00");
            order["state"] = json!("effective");
        }
        Some("OQUERYSINGLECHILD1") => {
            order["algoId"] = json!("parent-single-algo-id");
            order["ordId"] = json!("single-child-venue-id");
            order["ordIdList"] = json!(["single-child-venue-id"]);
            order["slOrdPx"] = json!("800.00");
            order["state"] = json!("effective");
        }
        None if params.get("algoId").map(String::as_str) == Some("algo-venue-id") => {
            order["algoClOrdId"] = json!("OQUERYALGO1");
            order["ordId"] = json!("");
            order["state"] = json!("live");
        }
        _ => {}
    }

    response
}

#[derive(Default)]
struct QueryOrderRouteState {
    regular_queries: tokio::sync::Mutex<Vec<HashMap<String, String>>>,
    algo_queries: tokio::sync::Mutex<Vec<HashMap<String, String>>>,
    sequence: tokio::sync::Mutex<Vec<String>>,
}

async fn start_exec_query_order_test_server(state: Arc<QueryOrderRouteState>) -> SocketAddr {
    let regular_state = Arc::clone(&state);
    let algo_state = state;
    let router = create_exec_test_router()
        .route(
            "/api/v5/public/instruments",
            get(|| async { Json(load_test_data("http_get_instruments_swap.json")) }),
        )
        .route(
            "/api/v5/trade/order",
            get(move |Query(params): Query<HashMap<String, String>>| {
                let state = Arc::clone(&regular_state);
                async move {
                    let query_id = params
                        .get("clOrdId")
                        .or_else(|| params.get("ordId"))
                        .cloned()
                        .unwrap_or_default();
                    state
                        .sequence
                        .lock()
                        .await
                        .push(format!("regular:{query_id}"));

                    if matches!(query_id.as_str(), "OQUERYUNKNOWN1" | "algo-venue-id") {
                        state.regular_queries.lock().await.push(params);
                        return Json(json!({
                            "code": "51603",
                            "msg": "Order does not exist",
                            "data": [],
                        }))
                        .into_response();
                    }

                    let response = regular_order_detail_response(&params);
                    state.regular_queries.lock().await.push(params);
                    Json(response).into_response()
                }
            }),
        )
        .route(
            "/api/v5/trade/order-algo",
            get(move |Query(params): Query<HashMap<String, String>>| {
                let state = Arc::clone(&algo_state);
                async move {
                    let query_id = params
                        .get("algoClOrdId")
                        .or_else(|| params.get("algoId"))
                        .cloned()
                        .unwrap_or_default();
                    state.sequence.lock().await.push(format!("algo:{query_id}"));
                    state.algo_queries.lock().await.push(params.clone());

                    Json(algo_order_detail_response(&params))
                }
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });

    addr
}

async fn create_query_order_test_client() -> (
    OKXExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    Rc<RefCell<Cache>>,
    Arc<QueryOrderRouteState>,
) {
    let state = Arc::new(QueryOrderRouteState::default());
    let addr = start_exec_query_order_test_server(Arc::clone(&state)).await;
    let base_url = format!("http://{addr}");
    let (mut client, mut rx, cache) =
        create_test_execution_client_configured(&base_url, |config| {
            config.instrument_types = vec![OKXInstrumentType::Swap];
        });
    client.on_instrument(query_order_instrument());
    client.start().unwrap();
    let _ = drain_events(&mut rx);

    (client, rx, cache, state)
}

fn create_exec_test_router() -> Router {
    Router::new().route(
        "/api/v5/account/balance",
        get(|_headers: HeaderMap| async {
            axum::Json(load_test_data("http_get_account_balance.json")).into_response()
        }),
    )
}

#[derive(Clone, Default)]
struct WsTeardownState {
    opened: Arc<AtomicUsize>,
    closed: Arc<AtomicUsize>,
}

async fn handle_exec_ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<Arc<WsTeardownState>>,
) -> Response {
    ws.on_upgrade(move |socket| handle_exec_ws_socket(socket, state))
}

async fn handle_exec_ws_socket(mut socket: WebSocket, state: Arc<WsTeardownState>) {
    state.opened.fetch_add(1, Ordering::Relaxed);

    while let Some(message) = socket.next().await {
        let Ok(message) = message else { break };
        if let Message::Text(text) = message
            && text.contains("\"op\":\"login\"")
            && socket
                .send(Message::Text(
                    "{\"event\":\"login\",\"code\":\"0\",\"msg\":\"\",\"connId\":\"test\"}"
                        .to_string()
                        .into(),
                ))
                .await
                .is_err()
        {
            break;
        }
    }
    state.closed.fetch_add(1, Ordering::Relaxed);
}

/// Serves instrument fixtures, a failing account balance endpoint, and
/// WebSocket endpoints that stay open until the client closes them.
async fn start_exec_session_failure_server() -> (SocketAddr, Arc<WsTeardownState>) {
    let ws_state = Arc::new(WsTeardownState::default());
    let router = Router::new()
        .route(
            "/ws/v5/private",
            get(handle_exec_ws_upgrade).with_state(Arc::clone(&ws_state)),
        )
        .route(
            "/ws/v5/business",
            get(handle_exec_ws_upgrade).with_state(Arc::clone(&ws_state)),
        )
        .route(
            "/api/v5/public/instruments",
            get(|| async { Json(load_test_data("http_get_instruments_swap.json")) }),
        )
        .route(
            "/api/v5/account/balance",
            get(|| async { StatusCode::INTERNAL_SERVER_ERROR }),
        );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });
    (addr, ws_state)
}

#[derive(Default)]
struct ReportRouteState {
    regular_order_pending_queries: tokio::sync::Mutex<Vec<HashMap<String, String>>>,
    regular_order_history_queries: tokio::sync::Mutex<Vec<HashMap<String, String>>>,
    algo_order_pending_queries: tokio::sync::Mutex<Vec<HashMap<String, String>>>,
    algo_order_history_queries: tokio::sync::Mutex<Vec<HashMap<String, String>>>,
    spread_order_pending_queries: tokio::sync::Mutex<Vec<HashMap<String, String>>>,
    spread_order_history_queries: tokio::sync::Mutex<Vec<HashMap<String, String>>>,
    regular_fill_queries: tokio::sync::Mutex<Vec<HashMap<String, String>>>,
    regular_fill_history_queries: tokio::sync::Mutex<Vec<HashMap<String, String>>>,
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
    let http_client = HttpClient::builder().build().unwrap();
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
    let http_client = HttpClient::builder().build().unwrap();
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
    let algo_pending_state = Arc::clone(&state);
    let algo_history_state = Arc::clone(&state);
    let spread_pending_state = Arc::clone(&state);
    let spread_history_state = Arc::clone(&state);
    let regular_fill_state = Arc::clone(&state);
    let regular_fill_history_state = Arc::clone(&state);
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
                    let is_spot = params.get("instType").is_some_and(|value| value == "SPOT");
                    state
                        .regular_order_history_queries
                        .lock()
                        .await
                        .push(params);

                    if !is_spot {
                        return Json(json!({"code": "0", "msg": "", "data": []})).into_response();
                    }

                    let mut response =
                        load_test_data("ws_orders_algo_child_filled_empty_cl_ord_id.json");
                    response["code"] = json!("0");
                    response["msg"] = json!("");
                    Json(response).into_response()
                }
            }),
        )
        .route(
            "/api/v5/trade/orders-algo-pending",
            get(move |Query(params): Query<HashMap<String, String>>| {
                let state = Arc::clone(&algo_pending_state);
                async move {
                    state.algo_order_pending_queries.lock().await.push(params);
                    Json(json!({"code": "0", "msg": "", "data": []})).into_response()
                }
            }),
        )
        .route(
            "/api/v5/trade/orders-algo-history",
            get(move |Query(params): Query<HashMap<String, String>>| {
                let state = Arc::clone(&algo_history_state);
                async move {
                    let returns_parent =
                        params.get("instType").is_some_and(|value| value == "SPOT")
                            && params
                                .get("ordType")
                                .is_some_and(|value| value == "trigger")
                            && params
                                .get("state")
                                .is_some_and(|value| value == "effective");
                    state.algo_order_history_queries.lock().await.push(params);

                    if !returns_parent {
                        return Json(json!({"code": "0", "msg": "", "data": []})).into_response();
                    }

                    let mut response = load_test_data("http_get_orders_algo_history.json");
                    let order = &mut response["data"][0];
                    order["actualPx"] = json!("1886.00");
                    order["actualSide"] = json!("buy");
                    order["actualSz"] = json!("0.003");
                    order["algoClOrdId"] = json!(MARGIN_SPOT_PARENT_CLIENT_ORDER_ID);
                    order["algoId"] = json!(MARGIN_SPOT_PARENT_VENUE_ORDER_ID);
                    order["clOrdId"] = json!("");
                    order["instId"] = json!("ETH-USDT");
                    order["instType"] = json!("SPOT");
                    order["ordId"] = json!(MARGIN_SPOT_CHILD_VENUE_ORDER_ID);
                    order["ordPx"] = json!("1886.00");
                    order["posSide"] = json!("net");
                    order["side"] = json!("buy");
                    order["sz"] = json!("0.003");
                    order["tdMode"] = json!("cross");
                    order["triggerPx"] = json!("1887.00");
                    Json(response).into_response()
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
                    let is_spot = params.get("instType").is_some_and(|value| value == "SPOT");
                    state.regular_fill_queries.lock().await.push(params);

                    if !is_spot {
                        return Json(json!({"code": "0", "msg": "", "data": []})).into_response();
                    }

                    let mut response =
                        load_test_data("ws_orders_algo_child_filled_empty_cl_ord_id.json");
                    response["code"] = json!("0");
                    response["msg"] = json!("");
                    response["data"][0]["billId"] = json!("bill-margin-spot-1");
                    response["data"][0]["ts"] = json!("1786550400100");
                    Json(response).into_response()
                }
            }),
        )
        .route(
            "/api/v5/trade/fills-history",
            get(move |Query(params): Query<HashMap<String, String>>| {
                let state = Arc::clone(&regular_fill_history_state);
                async move {
                    state.regular_fill_history_queries.lock().await.push(params);
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
        .route(
            "/api/v5/account/positions",
            get(|| async { Json(json!({"code": "0", "msg": "", "data": []})).into_response() }),
        )
        .route(
            "/api/v5/account/balance",
            get(|| async { Json(load_test_data("http_get_account_balance.json")).into_response() }),
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
    configure: impl FnOnce(&mut OKXExecutionClientConfig),
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

    let mut config = OKXExecutionClientConfig {
        account_id,
        base_url_http: Some(base_url.to_string()),
        base_url_ws_private: Some("ws://127.0.0.1:19999/ws/v5/private".to_string()),
        base_url_ws_business: Some("ws://127.0.0.1:19999/ws/v5/business".to_string()),
        api_key: Some("test_key".into()),
        api_secret: Some("test_secret".into()),
        api_passphrase: Some("test_passphrase".into()),
        ..Default::default()
    };
    configure(&mut config);

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let client = OKXExecutionClient::new(core, config).unwrap();

    (client, rx, cache)
}

fn make_margin_spot_report_instrument() -> InstrumentAny {
    let mut instrument = currency_pair_ethusdt();
    instrument.id = InstrumentId::from("ETH-USDT.OKX");
    instrument.raw_symbol = Symbol::from("ETH-USDT");
    InstrumentAny::CurrencyPair(instrument)
}

fn assert_margin_spot_queries(queries: &[HashMap<String, String>]) {
    let margin_count = queries
        .iter()
        .filter(|query| query.get("instType").is_some_and(|value| value == "MARGIN"))
        .count();
    let spot_count = queries
        .iter()
        .filter(|query| query.get("instType").is_some_and(|value| value == "SPOT"))
        .count();

    assert!(margin_count > 0);
    assert_eq!(margin_count, spot_count);
    assert_eq!(queries.len(), margin_count + spot_count);
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
async fn test_margin_only_restart_recovers_spot_order_status_reports() {
    let state = Arc::new(ReportRouteState::default());
    let addr = start_exec_report_test_server(Arc::clone(&state)).await;
    let base_url = format!("http://{addr}");
    let (mut client, _rx, _cache) = create_test_execution_client_configured(&base_url, |config| {
        config.instrument_types = vec![OKXInstrumentType::Margin];
        config.margin_mode = Some(OKXMarginMode::Cross);
        config.use_spot_margin = true;
    });
    client.on_instrument(make_margin_spot_report_instrument());

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
    let algo_pending_queries = state.algo_order_pending_queries.lock().await;
    let algo_history_queries = state.algo_order_history_queries.lock().await;

    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].instrument_id, InstrumentId::from("ETH-USDT.OKX"));
    assert_eq!(
        reports[0].client_order_id,
        Some(ClientOrderId::from(MARGIN_SPOT_PARENT_CLIENT_ORDER_ID))
    );
    assert_eq!(
        reports[0].venue_order_id,
        VenueOrderId::from(MARGIN_SPOT_CHILD_VENUE_ORDER_ID)
    );
    assert_eq!(reports[0].order_status, OrderStatus::Filled);
    assert_eq!(reports[0].filled_qty, Quantity::from("0.003"));
    assert_margin_spot_queries(&regular_pending_queries);
    assert_margin_spot_queries(&regular_history_queries);
    assert_margin_spot_queries(&algo_pending_queries);
    assert_margin_spot_queries(&algo_history_queries);
}

#[rstest]
#[tokio::test]
async fn test_margin_only_restart_recovers_spot_fill_reports() {
    let state = Arc::new(ReportRouteState::default());
    let addr = start_exec_report_test_server(Arc::clone(&state)).await;
    let base_url = format!("http://{addr}");
    let (mut client, _rx, _cache) = create_test_execution_client_configured(&base_url, |config| {
        config.instrument_types = vec![OKXInstrumentType::Margin];
        config.margin_mode = Some(OKXMarginMode::Cross);
        config.use_spot_margin = true;
    });
    client.on_instrument(make_margin_spot_report_instrument());

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

    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].instrument_id, InstrumentId::from("ETH-USDT.OKX"));
    assert_eq!(reports[0].client_order_id, None);
    assert_eq!(
        reports[0].venue_order_id,
        VenueOrderId::from(MARGIN_SPOT_CHILD_VENUE_ORDER_ID)
    );
    assert_eq!(reports[0].trade_id, TradeId::new(MARGIN_SPOT_TRADE_ID));
    assert_eq!(reports[0].last_qty, Quantity::from("0.003"));
    assert_eq!(reports[0].last_px, Price::from("1886.00"));
    assert_margin_spot_queries(&regular_fill_queries);
}

#[rstest]
#[tokio::test]
async fn test_query_order_routes_cached_regular_pretrigger_algo_and_unknown_orders() {
    let (client, mut rx, cache, state) = create_query_order_test_client().await;

    let regular_client_order_id = ClientOrderId::from("OQUERYREGULAR1");
    let regular_venue_order_id = VenueOrderId::from("regular-venue-id");
    let regular_order =
        build_test_regular_order_with_venue_id(regular_client_order_id, regular_venue_order_id);
    cache
        .borrow_mut()
        .add_order(regular_order, None, Some(*OKX_CLIENT_ID), false)
        .unwrap();
    client
        .query_order(query_order_command(
            regular_client_order_id,
            Some(VenueOrderId::from("stale-regular-venue-id")),
        ))
        .unwrap();
    let regular_report = recv_query_order_report(&mut rx, Some(regular_client_order_id)).await;
    assert_eq!(regular_report.order_type, OrderType::Limit);
    assert_eq!(regular_report.venue_order_id, regular_venue_order_id);

    let algo_client_order_id = ClientOrderId::from("OQUERYALGO1");
    let algo_venue_order_id = VenueOrderId::from("algo-venue-id");
    let algo_order = build_test_conditional_order_with_single_venue_id(
        algo_client_order_id,
        algo_venue_order_id,
    );
    cache
        .borrow_mut()
        .add_order(algo_order, None, Some(*OKX_CLIENT_ID), false)
        .unwrap();
    client
        .query_order(query_order_command(
            algo_client_order_id,
            Some(algo_venue_order_id),
        ))
        .unwrap();
    let algo_report = recv_query_order_report(&mut rx, Some(algo_client_order_id)).await;
    assert_eq!(algo_report.order_type, OrderType::StopMarket);

    let unknown_client_order_id = ClientOrderId::from("OQUERYUNKNOWN1");
    client
        .query_order(query_order_command(unknown_client_order_id, None))
        .unwrap();
    let unknown_report = recv_query_order_report(&mut rx, Some(unknown_client_order_id)).await;
    assert_eq!(unknown_report.order_type, OrderType::StopMarket);

    let unexpected_reports: Vec<_> = drain_events(&mut rx)
        .into_iter()
        .filter(|event| {
            matches!(
                event,
                ExecutionEvent::Report(CommonExecutionReport::Order(_))
            )
        })
        .collect();
    assert!(
        unexpected_reports.is_empty(),
        "query_order emitted duplicate reports: {unexpected_reports:?}"
    );

    let regular_queries = state.regular_queries.lock().await;
    let algo_queries = state.algo_queries.lock().await;
    let sequence = state.sequence.lock().await;

    assert_eq!(regular_queries.len(), 2);
    assert_eq!(
        regular_queries[0].get("ordId").map(String::as_str),
        Some("regular-venue-id")
    );
    assert!(!regular_queries[0].contains_key("clOrdId"));
    assert_eq!(
        regular_queries[1].get("clOrdId").map(String::as_str),
        Some("OQUERYUNKNOWN1")
    );
    assert!(!regular_queries[1].contains_key("ordId"));
    assert_eq!(algo_queries.len(), 2);
    assert_eq!(
        algo_queries[0].get("algoClOrdId").map(String::as_str),
        Some("OQUERYALGO1")
    );
    assert!(!algo_queries[0].contains_key("algoId"));
    assert_eq!(
        algo_queries[1].get("algoClOrdId").map(String::as_str),
        Some("OQUERYUNKNOWN1")
    );
    assert!(!algo_queries[1].contains_key("algoId"));
    assert_eq!(
        sequence.as_slice(),
        [
            "regular:regular-venue-id",
            "algo:OQUERYALGO1",
            "regular:OQUERYUNKNOWN1",
            "algo:OQUERYUNKNOWN1",
        ]
    );
}

#[rstest]
#[tokio::test]
async fn test_query_order_market_conditional_with_child_id_queries_child_and_parent() {
    let (client, mut rx, cache, state) = create_query_order_test_client().await;

    // Market-style conditional orders can receive a regular child venue ID
    // without transitioning their cached triggered flag.
    let client_order_id = ClientOrderId::from("OQUERYMARKETCHILD1");
    let child_venue_order_id = VenueOrderId::from("market-child-venue-id");
    let order =
        build_test_market_conditional_order_with_child(client_order_id, child_venue_order_id);
    cache
        .borrow_mut()
        .add_order(order, None, Some(*OKX_CLIENT_ID), false)
        .unwrap();

    client
        .query_order(query_order_command(
            client_order_id,
            Some(child_venue_order_id),
        ))
        .unwrap();
    let report = recv_query_order_report(&mut rx, Some(client_order_id)).await;

    assert_eq!(report.order_type, OrderType::Market);
    assert_eq!(report.order_status, OrderStatus::Filled);
    assert_eq!(report.filled_qty, Quantity::from("1"));
    assert_eq!(report.venue_order_id, child_venue_order_id);

    let regular_queries = state.regular_queries.lock().await;
    let algo_queries = state.algo_queries.lock().await;
    let sequence = state.sequence.lock().await;
    assert_eq!(regular_queries.len(), 1);
    assert_eq!(
        regular_queries[0].get("ordId").map(String::as_str),
        Some("market-child-venue-id")
    );
    assert!(!regular_queries[0].contains_key("clOrdId"));
    assert_eq!(algo_queries.len(), 1);
    assert_eq!(
        algo_queries[0].get("algoClOrdId").map(String::as_str),
        Some("OQUERYMARKETCHILD1")
    );
    assert!(!algo_queries[0].contains_key("algoId"));
    assert_eq!(
        sequence.as_slice(),
        ["algo:OQUERYMARKETCHILD1", "regular:market-child-venue-id",]
    );
}

#[rstest]
#[tokio::test]
async fn test_query_order_triggered_conditional_recovers_missed_child_fill() {
    let (client, mut rx, cache, state) = create_query_order_test_client().await;

    let client_order_id = ClientOrderId::from("OQUERYTRIGGERED1");
    let child_venue_order_id = VenueOrderId::from("triggered-child-venue-id");
    let order = build_test_triggered_conditional_order(client_order_id, child_venue_order_id);
    cache
        .borrow_mut()
        .add_order(order, None, Some(*OKX_CLIENT_ID), false)
        .unwrap();

    client
        .query_order(query_order_command(
            client_order_id,
            Some(child_venue_order_id),
        ))
        .unwrap();
    let report = recv_query_order_report(&mut rx, Some(client_order_id)).await;

    // The algo parent is only Triggered, while the regular child is Filled.
    assert_eq!(report.order_type, OrderType::Limit);
    assert_eq!(report.order_status, OrderStatus::Filled);
    assert_eq!(report.filled_qty, Quantity::from("1"));
    assert_eq!(report.venue_order_id, child_venue_order_id);

    let regular_queries = state.regular_queries.lock().await;
    let algo_queries = state.algo_queries.lock().await;
    let sequence = state.sequence.lock().await;
    assert_eq!(regular_queries.len(), 1);
    assert_eq!(
        regular_queries[0].get("ordId").map(String::as_str),
        Some("triggered-child-venue-id")
    );
    assert!(!regular_queries[0].contains_key("clOrdId"));
    assert_eq!(algo_queries.len(), 1);
    assert_eq!(
        algo_queries[0].get("algoClOrdId").map(String::as_str),
        Some("OQUERYTRIGGERED1")
    );
    assert!(!algo_queries[0].contains_key("algoId"));
    assert_eq!(
        sequence.as_slice(),
        ["algo:OQUERYTRIGGERED1", "regular:triggered-child-venue-id",]
    );
}

#[rstest]
#[tokio::test]
async fn test_query_order_recovers_fill_when_child_event_was_completely_missed() {
    let (client, mut rx, cache, state) = create_query_order_test_client().await;

    let client_order_id = ClientOrderId::from("OQUERYMISSEDCHILD1");
    let parent_venue_order_id = VenueOrderId::from("parent-missed-algo-id");
    let child_venue_order_id = VenueOrderId::from("missed-child-venue-id");
    let order =
        build_test_conditional_order_with_single_venue_id(client_order_id, parent_venue_order_id);
    cache
        .borrow_mut()
        .add_order(order, None, Some(*OKX_CLIENT_ID), false)
        .unwrap();

    client
        .query_order(query_order_command(
            client_order_id,
            Some(parent_venue_order_id),
        ))
        .unwrap();
    let report = recv_query_order_report(&mut rx, Some(client_order_id)).await;

    assert_eq!(report.order_type, OrderType::Limit);
    assert_eq!(report.order_status, OrderStatus::Filled);
    assert_eq!(report.filled_qty, Quantity::from("1"));
    assert_eq!(report.venue_order_id, child_venue_order_id);

    let regular_queries = state.regular_queries.lock().await;
    let algo_queries = state.algo_queries.lock().await;
    let sequence = state.sequence.lock().await;
    assert_eq!(regular_queries.len(), 1);
    assert_eq!(
        regular_queries[0].get("ordId").map(String::as_str),
        Some("missed-child-venue-id")
    );
    assert!(!regular_queries[0].contains_key("clOrdId"));
    assert_eq!(algo_queries.len(), 1);
    assert_eq!(
        algo_queries[0].get("algoClOrdId").map(String::as_str),
        Some("OQUERYMISSEDCHILD1")
    );
    assert!(!algo_queries[0].contains_key("algoId"));
    assert_eq!(
        sequence.as_slice(),
        ["algo:OQUERYMISSEDCHILD1", "regular:missed-child-venue-id",]
    );
}

#[rstest]
#[tokio::test]
async fn test_query_order_recovers_fill_when_acceptance_and_child_events_were_missed() {
    let (client, mut rx, cache, state) = create_query_order_test_client().await;

    let client_order_id = ClientOrderId::from("OQUERYMISSEDACCEPT1");
    let child_venue_order_id = VenueOrderId::from("missed-accept-child-venue-id");
    let order = build_test_submitted_conditional_order(client_order_id);
    cache
        .borrow_mut()
        .add_order(order, None, Some(*OKX_CLIENT_ID), false)
        .unwrap();

    client
        .query_order(query_order_command(client_order_id, None))
        .unwrap();
    let report = recv_query_order_report(&mut rx, Some(client_order_id)).await;

    assert_eq!(report.order_type, OrderType::Limit);
    assert_eq!(report.order_status, OrderStatus::Filled);
    assert_eq!(report.filled_qty, Quantity::from("1"));
    assert_eq!(report.venue_order_id, child_venue_order_id);

    let regular_queries = state.regular_queries.lock().await;
    let algo_queries = state.algo_queries.lock().await;
    let sequence = state.sequence.lock().await;
    assert_eq!(regular_queries.len(), 1);
    assert_eq!(
        regular_queries[0].get("ordId").map(String::as_str),
        Some("missed-accept-child-venue-id")
    );
    assert!(!regular_queries[0].contains_key("clOrdId"));
    assert_eq!(algo_queries.len(), 1);
    assert_eq!(
        algo_queries[0].get("algoClOrdId").map(String::as_str),
        Some("OQUERYMISSEDACCEPT1")
    );
    assert!(!algo_queries[0].contains_key("algoId"));
    assert_eq!(
        sequence.as_slice(),
        [
            "algo:OQUERYMISSEDACCEPT1",
            "regular:missed-accept-child-venue-id",
        ]
    );
}

#[rstest]
#[tokio::test]
async fn test_query_order_recovers_fill_when_only_cached_id_is_child() {
    let (client, mut rx, cache, state) = create_query_order_test_client().await;

    let client_order_id = ClientOrderId::from("OQUERYSINGLECHILD1");
    let child_venue_order_id = VenueOrderId::from("single-child-venue-id");
    let order =
        build_test_conditional_order_with_single_venue_id(client_order_id, child_venue_order_id);
    cache
        .borrow_mut()
        .add_order(order, None, Some(*OKX_CLIENT_ID), false)
        .unwrap();

    client
        .query_order(query_order_command(
            client_order_id,
            Some(child_venue_order_id),
        ))
        .unwrap();
    let report = recv_query_order_report(&mut rx, Some(client_order_id)).await;

    assert_eq!(report.order_type, OrderType::Limit);
    assert_eq!(report.order_status, OrderStatus::Filled);
    assert_eq!(report.filled_qty, Quantity::from("1"));
    assert_eq!(report.venue_order_id, child_venue_order_id);

    let regular_queries = state.regular_queries.lock().await;
    let algo_queries = state.algo_queries.lock().await;
    let sequence = state.sequence.lock().await;
    assert_eq!(regular_queries.len(), 1);
    assert_eq!(
        regular_queries[0].get("ordId").map(String::as_str),
        Some("single-child-venue-id")
    );
    assert!(!regular_queries[0].contains_key("clOrdId"));
    assert_eq!(algo_queries.len(), 1);
    assert_eq!(
        algo_queries[0].get("algoClOrdId").map(String::as_str),
        Some("OQUERYSINGLECHILD1")
    );
    assert!(!algo_queries[0].contains_key("algoId"));
    assert_eq!(
        sequence.as_slice(),
        ["algo:OQUERYSINGLECHILD1", "regular:single-child-venue-id",]
    );
}

#[rstest]
#[case(None)]
#[case(Some("stale-external-venue-id"))]
#[tokio::test]
async fn test_query_order_adopted_external_regular_uses_cached_venue_order_id(
    #[case] command_venue_order_id: Option<&str>,
) {
    let (client, mut rx, cache, state) = create_query_order_test_client().await;

    // Adopted external regular orders synthesize their local client ID from
    // ordId when OKX reports no clOrdId.
    let venue_order_id = VenueOrderId::from("external-venue-id");
    let client_order_id = ClientOrderId::from(venue_order_id.as_str());
    let order = build_test_regular_order_with_venue_id(client_order_id, venue_order_id);
    cache
        .borrow_mut()
        .add_order(order, None, Some(*OKX_CLIENT_ID), false)
        .unwrap();

    client
        .query_order(query_order_command(
            client_order_id,
            command_venue_order_id.map(VenueOrderId::from),
        ))
        .unwrap();
    let report = recv_query_order_report(&mut rx, None).await;

    assert_eq!(report.order_type, OrderType::Limit);
    assert_eq!(report.order_status, OrderStatus::Accepted);
    assert_eq!(report.venue_order_id, venue_order_id);

    let regular_queries = state.regular_queries.lock().await;
    let algo_queries = state.algo_queries.lock().await;
    let sequence = state.sequence.lock().await;
    assert_eq!(regular_queries.len(), 1);
    assert_eq!(
        regular_queries[0].get("ordId").map(String::as_str),
        Some("external-venue-id")
    );
    assert!(!regular_queries[0].contains_key("clOrdId"));
    assert!(algo_queries.is_empty());
    assert_eq!(sequence.as_slice(), ["regular:external-venue-id"]);
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
    let instrument = CryptoFuturesSpread::builder()
        .instrument_id(InstrumentId::from("ETH-USD-SWAP_ETH-USD-231229.OKX"))
        .raw_symbol(Symbol::from("ETH-USD-SWAP_ETH-USD-231229"))
        .underlying(Currency::get_or_create_crypto("ETH"))
        .quote_currency(Currency::USD())
        .settlement_currency(Currency::USD())
        .is_inverse(false)
        .strategy_type(Ustr::from("inverse"))
        .activation_ns(UnixNanos::default())
        .expiration_ns(UnixNanos::default())
        .price_precision(2)
        .size_precision(0)
        .price_increment(Price::from("0.01"))
        .size_increment(Quantity::from("1"))
        .lot_size(Quantity::from("1"))
        .min_quantity(Quantity::from("1"))
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap();

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

fn build_test_stop_order(client_order_id: ClientOrderId) -> OrderAny {
    OrderTestBuilder::new(OrderType::StopMarket)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("STRATEGY-001"))
        .instrument_id(InstrumentId::from("ETH-USDT-SWAP.OKX"))
        .client_order_id(client_order_id)
        .side(OrderSide::Sell)
        .quantity(Quantity::from("1"))
        .trigger_price(Price::from("1000.00"))
        .trigger_type(TriggerType::MarkPrice)
        .time_in_force(TimeInForce::Gtc)
        .build()
}

fn build_test_conditional_order_with_single_venue_id(
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
) -> OrderAny {
    let trader_id = TraderId::from("TESTER-001");
    let strategy_id = StrategyId::from("STRATEGY-001");
    let instrument_id = InstrumentId::from("ETH-USDT-SWAP.OKX");
    let account_id = AccountId::from("OKX-001");
    let mut order = build_test_submitted_conditional_order(client_order_id);
    let accepted = OrderAcceptedSpec::builder()
        .trader_id(trader_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .venue_order_id(venue_order_id)
        .account_id(account_id)
        .build();

    order.apply(OrderEventAny::Accepted(accepted)).unwrap();

    assert_eq!(order.is_triggered(), Some(false));
    assert_eq!(order.venue_order_ids(), vec![&venue_order_id]);
    assert_eq!(order.venue_order_id(), Some(venue_order_id));
    order
}

fn build_test_submitted_conditional_order(client_order_id: ClientOrderId) -> OrderAny {
    let trader_id = TraderId::from("TESTER-001");
    let strategy_id = StrategyId::from("STRATEGY-001");
    let instrument_id = InstrumentId::from("ETH-USDT-SWAP.OKX");
    let account_id = AccountId::from("OKX-001");
    let mut order = OrderTestBuilder::new(OrderType::StopLimit)
        .trader_id(trader_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(OrderSide::Sell)
        .price(Price::from("850.00"))
        .quantity(Quantity::from("1"))
        .trigger_price(Price::from("1000.00"))
        .trigger_type(TriggerType::MarkPrice)
        .time_in_force(TimeInForce::Gtc)
        .build();
    let submitted = OrderSubmittedSpec::builder()
        .trader_id(trader_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .account_id(account_id)
        .build();

    order.apply(OrderEventAny::Submitted(submitted)).unwrap();

    assert_eq!(order.is_triggered(), Some(false));
    assert_eq!(order.status(), OrderStatus::Submitted);
    assert_eq!(order.venue_order_id(), None);
    order
}

fn build_test_triggered_conditional_order(
    client_order_id: ClientOrderId,
    child_venue_order_id: VenueOrderId,
) -> OrderAny {
    let trader_id = TraderId::from("TESTER-001");
    let strategy_id = StrategyId::from("STRATEGY-001");
    let instrument_id = InstrumentId::from("ETH-USDT-SWAP.OKX");
    let account_id = AccountId::from("OKX-001");
    let mut order = OrderTestBuilder::new(OrderType::StopLimit)
        .trader_id(trader_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(OrderSide::Sell)
        .price(Price::from("900.00"))
        .quantity(Quantity::from("1"))
        .trigger_price(Price::from("1000.00"))
        .trigger_type(TriggerType::MarkPrice)
        .time_in_force(TimeInForce::Gtc)
        .build();
    let submitted = OrderSubmittedSpec::builder()
        .trader_id(trader_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .account_id(account_id)
        .build();
    let accepted = OrderAcceptedSpec::builder()
        .trader_id(trader_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .venue_order_id(VenueOrderId::from("parent-algo-id"))
        .account_id(account_id)
        .build();
    let triggered = OrderTriggeredSpec::builder()
        .trader_id(trader_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .venue_order_id(child_venue_order_id)
        .account_id(account_id)
        .build();
    let updated = OrderUpdatedSpec::builder()
        .trader_id(trader_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .venue_order_id(child_venue_order_id)
        .account_id(account_id)
        .quantity(Quantity::from("1"))
        .build();

    order.apply(OrderEventAny::Submitted(submitted)).unwrap();
    order.apply(OrderEventAny::Accepted(accepted)).unwrap();
    order.apply(OrderEventAny::Triggered(triggered)).unwrap();
    order.apply(OrderEventAny::Updated(updated)).unwrap();

    assert_eq!(order.is_triggered(), Some(true));
    assert_eq!(order.venue_order_id(), Some(child_venue_order_id));
    order
}

fn build_test_market_conditional_order_with_child(
    client_order_id: ClientOrderId,
    child_venue_order_id: VenueOrderId,
) -> OrderAny {
    let trader_id = TraderId::from("TESTER-001");
    let strategy_id = StrategyId::from("STRATEGY-001");
    let instrument_id = InstrumentId::from("ETH-USDT-SWAP.OKX");
    let account_id = AccountId::from("OKX-001");
    let mut order = build_test_stop_order(client_order_id);
    let submitted = OrderSubmittedSpec::builder()
        .trader_id(trader_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .account_id(account_id)
        .build();
    let accepted = OrderAcceptedSpec::builder()
        .trader_id(trader_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .venue_order_id(VenueOrderId::from("parent-market-algo-id"))
        .account_id(account_id)
        .build();
    let updated = OrderUpdatedSpec::builder()
        .trader_id(trader_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .venue_order_id(child_venue_order_id)
        .account_id(account_id)
        .quantity(Quantity::from("1"))
        .build();

    order.apply(OrderEventAny::Submitted(submitted)).unwrap();
    order.apply(OrderEventAny::Accepted(accepted)).unwrap();
    order.apply(OrderEventAny::Updated(updated)).unwrap();

    assert_eq!(order.is_triggered(), Some(false));
    assert_eq!(order.venue_order_ids().len(), 2);
    assert_eq!(order.venue_order_id(), Some(child_venue_order_id));
    order
}

fn build_test_regular_order_with_venue_id(
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
) -> OrderAny {
    let trader_id = TraderId::from("TESTER-001");
    let strategy_id = StrategyId::from("STRATEGY-001");
    let instrument_id = InstrumentId::from("ETH-USDT-SWAP.OKX");
    let account_id = AccountId::from("OKX-001");
    let mut order = build_test_limit_order(instrument_id, client_order_id);
    let submitted = OrderSubmittedSpec::builder()
        .trader_id(trader_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .account_id(account_id)
        .build();
    let accepted = OrderAcceptedSpec::builder()
        .trader_id(trader_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .venue_order_id(venue_order_id)
        .account_id(account_id)
        .build();

    order.apply(OrderEventAny::Submitted(submitted)).unwrap();
    order.apply(OrderEventAny::Accepted(accepted)).unwrap();

    assert_eq!(order.venue_order_id(), Some(venue_order_id));
    order
}

fn query_order_command(
    client_order_id: ClientOrderId,
    venue_order_id: Option<VenueOrderId>,
) -> QueryOrder {
    QueryOrder::new(
        TraderId::from("TESTER-001"),
        Some(*OKX_CLIENT_ID),
        StrategyId::from("STRATEGY-001"),
        InstrumentId::from("ETH-USDT-SWAP.OKX"),
        client_order_id,
        venue_order_id,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    )
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
async fn test_submit_spread_order_denies_reduce_only() {
    let addr = start_exec_test_server().await;
    let base_url = format!("http://{addr}");
    let (mut client, mut rx, cache) = create_test_execution_client(&base_url);
    client.start().unwrap();
    let _ = drain_events(&mut rx);
    let instrument_id = InstrumentId::from("BCH-USDT_BCH-USDT-SWAP.OKX");
    let client_order_id = ClientOrderId::from("OREDUCESPREAD1");
    let order = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("STRATEGY-001"))
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(OrderSide::Sell)
        .price(Price::from("2000.00"))
        .quantity(Quantity::from("1"))
        .time_in_force(TimeInForce::Gtc)
        .reduce_only(true)
        .build();
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

    client.submit_order(cmd).unwrap();

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1, "events: {events:?}");
    let ExecutionEvent::Order(OrderEventAny::Denied(denied)) = &events[0] else {
        panic!("Expected OrderDenied, was {:?}", events[0]);
    };
    assert_eq!(denied.client_order_id, client_order_id);
    assert_eq!(denied.reason.as_str(), "UNSUPPORTED_REDUCE_ONLY");
}

#[rstest]
#[tokio::test]
async fn test_submit_cash_order_denies_reduce_only() {
    let addr = start_exec_test_server().await;
    let base_url = format!("http://{addr}");
    let (mut client, mut rx, cache) = create_test_execution_client(&base_url);
    client.start().unwrap();
    let _ = drain_events(&mut rx);
    let instrument_id = InstrumentId::from("BTC-USDT.OKX");
    let client_order_id = ClientOrderId::from("OREDUCECASH1");
    let order = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("STRATEGY-001"))
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(OrderSide::Sell)
        .price(Price::from("2000.00"))
        .quantity(Quantity::from("1"))
        .time_in_force(TimeInForce::Gtc)
        .reduce_only(true)
        .build();
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

    client.submit_order(cmd).unwrap();

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1, "events: {events:?}");
    let ExecutionEvent::Order(OrderEventAny::Denied(denied)) = &events[0] else {
        panic!("Expected OrderDenied, was {:?}", events[0]);
    };
    assert_eq!(denied.client_order_id, client_order_id);
    assert_eq!(denied.reason.as_str(), "UNSUPPORTED_REDUCE_ONLY");
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
async fn test_submit_order_list_errors_before_events_when_order_is_missing() {
    let addr = start_exec_test_server().await;
    let base_url = format!("http://{addr}");
    let (mut client, mut rx, cache) = create_test_execution_client(&base_url);

    client.start().unwrap();
    let _ = drain_events(&mut rx);

    let trader_id = TraderId::from("TESTER-001");
    let strategy_id = StrategyId::from("STRATEGY-001");
    let instrument_id = InstrumentId::from("ETH-USDT-SWAP.OKX");
    let cached_id = ClientOrderId::from("OCACHED1");
    let missing_id = ClientOrderId::from("OMISSING1");
    let cached_order = build_test_limit_order(instrument_id, cached_id);
    let missing_order = build_test_limit_order(instrument_id, missing_id);

    cache
        .borrow_mut()
        .add_order(cached_order.clone(), None, Some(*OKX_CLIENT_ID), false)
        .unwrap();

    let order_list = OrderList::new(
        OrderListId::new("OL-MISSING"),
        instrument_id,
        strategy_id,
        vec![cached_id, missing_id],
        UnixNanos::default(),
    );
    let cmd = SubmitOrderList::new(
        trader_id,
        Some(*OKX_CLIENT_ID),
        strategy_id,
        order_list,
        vec![
            OrderInitialized::from(&cached_order),
            OrderInitialized::from(&missing_order),
        ],
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    let error = client
        .submit_order_list(cmd)
        .expect_err("missing order should fail the list submission");

    assert!(error.to_string().contains(missing_id.as_str()));
    assert!(drain_events(&mut rx).is_empty());
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

fn generate_order_status_report_cmd(
    instrument_id: Option<InstrumentId>,
    client_order_id: Option<ClientOrderId>,
    venue_order_id: Option<VenueOrderId>,
) -> GenerateOrderStatusReport {
    GenerateOrderStatusReport::new(
        UUID4::new(),
        UnixNanos::default(),
        instrument_id,
        client_order_id,
        venue_order_id,
        None,
        None,
    )
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_report_requires_instrument_id() {
    let (client, _rx, _cache, _state) = create_query_order_test_client().await;
    let error = client
        .generate_order_status_report(&generate_order_status_report_cmd(
            None,
            Some(ClientOrderId::from("OQUERYREGULAR1")),
            None,
        ))
        .await
        .unwrap_err();

    assert!(error.to_string().contains("requires instrument_id"));
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_report_uses_targeted_lookup() {
    let (client, _rx, _cache, state) = create_query_order_test_client().await;
    let client_order_id = ClientOrderId::from("OQUERYREGULAR1");
    let report = client
        .generate_order_status_report(&generate_order_status_report_cmd(
            Some(InstrumentId::from("ETH-USDT-SWAP.OKX")),
            Some(client_order_id),
            None,
        ))
        .await
        .unwrap()
        .unwrap();
    let regular_queries = state.regular_queries.lock().await;
    let sequence = state.sequence.lock().await;

    assert_eq!(report.client_order_id, Some(client_order_id));
    assert!(
        regular_queries
            .iter()
            .any(|query| query.get("clOrdId").map(String::as_str) == Some("OQUERYREGULAR1"))
    );
    assert!(sequence.iter().any(|entry| entry.starts_with("regular:")));
    assert!(
        !sequence
            .iter()
            .any(|entry| entry.contains("orders-history"))
    );
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_report_venue_only_queries_algo_after_regular_miss() {
    let (client, _rx, _cache, state) = create_query_order_test_client().await;
    let venue_order_id = VenueOrderId::from("algo-venue-id");
    let report = client
        .generate_order_status_report(&generate_order_status_report_cmd(
            Some(InstrumentId::from("ETH-USDT-SWAP.OKX")),
            None,
            Some(venue_order_id),
        ))
        .await
        .unwrap()
        .unwrap();
    let regular_queries = state.regular_queries.lock().await;
    let algo_queries = state.algo_queries.lock().await;
    let sequence = state.sequence.lock().await;

    assert_eq!(report.venue_order_id, venue_order_id);
    assert_eq!(
        report.client_order_id,
        Some(ClientOrderId::from("OQUERYALGO1"))
    );
    assert_eq!(report.order_status, OrderStatus::Accepted);
    assert_eq!(
        regular_queries[0].get("ordId").map(String::as_str),
        Some("algo-venue-id")
    );
    assert!(!regular_queries[0].contains_key("clOrdId"));
    assert_eq!(
        algo_queries[0].get("algoId").map(String::as_str),
        Some("algo-venue-id")
    );
    assert!(!algo_queries[0].contains_key("algoClOrdId"));
    assert_eq!(
        sequence.as_slice(),
        ["regular:algo-venue-id", "algo:algo-venue-id"]
    );
}

#[rstest]
#[case(Some(60), 60)]
#[case(None, OKX_RECONCILIATION_LOOKBACK_DEFAULT_MINS)]
#[case(Some(5 * 24 * 60), 5 * 24 * 60)]
#[case(
    Some(8 * 24 * 60),
    OKX_RECONCILIATION_LOOKBACK_MAX_MINS
)]
#[tokio::test]
async fn test_generate_mass_status_sets_report_window(
    #[case] lookback_mins: Option<u64>,
    #[case] expected_mins: u64,
) {
    let state = Arc::new(ReportRouteState::default());
    let addr = start_exec_report_test_server(Arc::clone(&state)).await;
    let base_url = format!("http://{addr}");
    let (mut client, _rx, _cache) = create_test_execution_client_configured(&base_url, |config| {
        config.instrument_types = vec![OKXInstrumentType::Swap];
    });
    client.on_instrument(query_order_instrument());

    let mass_status = client
        .generate_mass_status(lookback_mins)
        .await
        .unwrap()
        .unwrap();
    let expected_start = UnixNanos::from(
        mass_status
            .ts_init
            .as_u64()
            .saturating_sub(expected_mins * 60 * 1_000_000_000),
    );
    let recent_fill_queries = state.regular_fill_queries.lock().await.clone();
    let extended_fill_queries = state.regular_fill_history_queries.lock().await.clone();
    let expected_begin = (expected_start.as_u64() / 1_000_000).to_string();

    assert_eq!(mass_status.lookback_start(), Some(expected_start));
    assert!(mass_status.reports_complete());
    let fill_query = if expected_mins <= OKX_RECONCILIATION_LOOKBACK_DEFAULT_MINS {
        assert_eq!(recent_fill_queries.len(), 1);
        assert!(extended_fill_queries.is_empty());
        &recent_fill_queries[0]
    } else {
        assert!(recent_fill_queries.is_empty());
        assert_eq!(extended_fill_queries.len(), 1);
        &extended_fill_queries[0]
    };
    assert_eq!(
        fill_query.get("begin").map(String::as_str),
        Some(expected_begin.as_str())
    );
    assert!(!fill_query.contains_key("end"));
}

#[rstest]
#[case::service_unavailable(StatusCode::SERVICE_UNAVAILABLE, 2)]
#[case::not_found(StatusCode::NOT_FOUND, 1)]
#[tokio::test]
async fn test_generate_mass_status_fails_when_pending_algo_orders_are_unavailable(
    #[case] failure_status: StatusCode,
    #[case] expected_attempts: usize,
) {
    let pending_attempts = Arc::new(AtomicUsize::new(0));
    let handler_attempts = Arc::clone(&pending_attempts);
    let empty = get(|| async { Json(json!({"code": "0", "msg": "", "data": []})).into_response() });
    let router = Router::new()
        .route("/api/v5/trade/orders-pending", empty.clone())
        .route("/api/v5/trade/orders-history", empty.clone())
        .route(
            "/api/v5/trade/orders-algo-pending",
            get(move || {
                let attempts = Arc::clone(&handler_attempts);
                async move {
                    attempts.fetch_add(1, Ordering::Relaxed);
                    failure_status.into_response()
                }
            }),
        )
        .route("/api/v5/trade/orders-algo-history", empty.clone())
        .route("/api/v5/trade/fills", empty.clone())
        .route("/api/v5/account/positions", empty.clone())
        .route(
            "/api/v5/account/balance",
            get(|| async { Json(load_test_data("http_get_account_balance.json")).into_response() }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });
    let (client, _rx, _cache) =
        create_test_execution_client_configured(&format!("http://{addr}"), |config| {
            config.instrument_types = vec![OKXInstrumentType::Swap];
            config.max_retries = 1;
            config.retry_delay_initial_ms = 1;
            config.retry_delay_max_ms = 1;
        });

    let error = client.generate_mass_status(None).await.unwrap_err();

    assert!(
        format!("{error:#}").contains("Failed to fetch pending algo order reports"),
        "was {error:#}"
    );
    assert_eq!(pending_attempts.load(Ordering::Relaxed), expected_attempts);
}

#[rstest]
#[tokio::test]
async fn test_generate_mass_status_fails_when_pending_algo_pagination_is_incomplete() {
    let pending_attempts = Arc::new(AtomicUsize::new(0));
    let handler_attempts = Arc::clone(&pending_attempts);
    let pending_order = load_test_data("http_get_orders_algo_pending.json")["data"][0].clone();
    let pending_page = json!({
        "code": "0",
        "msg": "",
        "data": vec![pending_order; 100],
    });
    let empty = get(|| async { Json(json!({"code": "0", "msg": "", "data": []})).into_response() });
    let router = Router::new()
        .route("/api/v5/trade/orders-pending", empty.clone())
        .route("/api/v5/trade/orders-history", empty.clone())
        .route(
            "/api/v5/trade/orders-algo-pending",
            get(move || {
                let attempts = Arc::clone(&handler_attempts);
                let page = pending_page.clone();
                async move {
                    attempts.fetch_add(1, Ordering::Relaxed);
                    Json(page).into_response()
                }
            }),
        )
        .route("/api/v5/trade/orders-algo-history", empty.clone())
        .route("/api/v5/trade/fills", empty.clone())
        .route("/api/v5/account/positions", empty.clone())
        .route(
            "/api/v5/account/balance",
            get(|| async { Json(load_test_data("http_get_account_balance.json")).into_response() }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });
    let (client, _rx, _cache) =
        create_test_execution_client_configured(&format!("http://{addr}"), |config| {
            config.instrument_types = vec![OKXInstrumentType::Swap];
        });

    let error = client.generate_mass_status(None).await.unwrap_err();

    assert!(
        format!("{error:#}").contains("did not establish complete coverage"),
        "was {error:#}"
    );
    assert_eq!(pending_attempts.load(Ordering::Relaxed), 50);
}

#[rstest]
#[tokio::test]
async fn test_generate_mass_status_fails_when_pending_algo_report_conversion_is_incomplete() {
    let empty = get(|| async { Json(json!({"code": "0", "msg": "", "data": []})).into_response() });
    let router = Router::new()
        .route("/api/v5/trade/orders-pending", empty.clone())
        .route("/api/v5/trade/orders-history", empty.clone())
        .route(
            "/api/v5/trade/orders-algo-pending",
            get(|Query(params): Query<HashMap<String, String>>| async move {
                if params
                    .get("ordType")
                    .is_some_and(|value| value == "trigger")
                {
                    let mut response = load_test_data("http_get_orders_algo_pending.json");
                    response["data"][0]["sz"] = json!("invalid");
                    Json(response).into_response()
                } else {
                    Json(json!({"code": "0", "msg": "", "data": []})).into_response()
                }
            }),
        )
        .route("/api/v5/trade/orders-algo-history", empty.clone())
        .route("/api/v5/trade/fills", empty.clone())
        .route("/api/v5/account/positions", empty.clone())
        .route(
            "/api/v5/account/balance",
            get(|| async { Json(load_test_data("http_get_account_balance.json")).into_response() }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });
    let (mut client, _rx, _cache) =
        create_test_execution_client_configured(&format!("http://{addr}"), |config| {
            config.instrument_types = vec![OKXInstrumentType::Swap];
        });
    client.on_instrument(btc_usdt_swap_instrument());

    let error = client.generate_mass_status(None).await.unwrap_err();

    assert!(
        format!("{error:#}").contains("could not be completely converted"),
        "was {error:#}"
    );
}

#[rstest]
#[tokio::test]
async fn test_generate_mass_status_fails_when_old_pending_algo_state_is_unknown() {
    let empty = get(|| async { Json(json!({"code": "0", "msg": "", "data": []})).into_response() });
    let router = Router::new()
        .route("/api/v5/trade/orders-pending", empty.clone())
        .route("/api/v5/trade/orders-history", empty.clone())
        .route(
            "/api/v5/trade/orders-algo-pending",
            get(|Query(params): Query<HashMap<String, String>>| async move {
                if params
                    .get("ordType")
                    .is_some_and(|value| value == "trigger")
                {
                    let mut response = load_test_data("http_get_orders_algo_pending.json");
                    response["data"][0]["state"] = json!("future_state");
                    response["data"][0]["uTime"] = json!("1");
                    Json(response).into_response()
                } else {
                    Json(json!({"code": "0", "msg": "", "data": []})).into_response()
                }
            }),
        )
        .route("/api/v5/trade/orders-algo-history", empty.clone())
        .route("/api/v5/trade/fills", empty.clone())
        .route("/api/v5/account/positions", empty.clone())
        .route(
            "/api/v5/account/balance",
            get(|| async { Json(load_test_data("http_get_account_balance.json")).into_response() }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });
    let (mut client, _rx, _cache) =
        create_test_execution_client_configured(&format!("http://{addr}"), |config| {
            config.instrument_types = vec![OKXInstrumentType::Swap];
        });
    client.on_instrument(btc_usdt_swap_instrument());

    let error = client.generate_mass_status(None).await.unwrap_err();

    assert!(
        format!("{error:#}").contains("could not be completely converted"),
        "was {error:#}"
    );
}

#[rstest]
#[tokio::test]
async fn test_public_algo_report_request_preserves_http_error_identity() {
    let router = Router::new().route(
        "/api/v5/trade/orders-algo-pending",
        get(|| async { StatusCode::FORBIDDEN.into_response() }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });
    let client = OKXHttpClient::with_credentials(
        Some("api_key".to_string()),
        Some("api_secret".to_string()),
        Some("passphrase".to_string()),
        Some(format!("http://{addr}")),
        60,
        0,
        1,
        1,
        OKXEnvironment::Live,
        None,
    )
    .unwrap();

    let error = client
        .request_algo_order_status_reports(
            AccountId::from("OKX-001"),
            Some(OKXInstrumentType::Swap),
            None,
            None,
            None,
            None,
            None,
        )
        .await
        .unwrap_err();

    assert!(
        error.downcast_ref::<OKXHttpError>().is_some(),
        "was {error:#}"
    );
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_reports_preserves_history_after_pending_not_found() {
    let empty = get(|| async { Json(json!({"code": "0", "msg": "", "data": []})).into_response() });
    let router = Router::new()
        .route("/api/v5/trade/orders-pending", empty.clone())
        .route("/api/v5/trade/orders-history", empty.clone())
        .route(
            "/api/v5/trade/orders-algo-pending",
            get(|| async { StatusCode::NOT_FOUND.into_response() }),
        )
        .route(
            "/api/v5/trade/orders-algo-history",
            get(|Query(params): Query<HashMap<String, String>>| async move {
                let is_target = params
                    .get("ordType")
                    .is_some_and(|value| value == "trigger")
                    && params
                        .get("state")
                        .is_some_and(|value| value == "effective");

                if is_target {
                    Json(load_test_data("http_get_orders_algo_history.json")).into_response()
                } else {
                    Json(json!({"code": "0", "msg": "", "data": []})).into_response()
                }
            }),
        )
        .route(
            "/api/v5/account/balance",
            get(|| async { Json(load_test_data("http_get_account_balance.json")).into_response() }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });
    let (mut client, _rx, _cache) =
        create_test_execution_client_configured(&format!("http://{addr}"), |config| {
            config.instrument_types = vec![OKXInstrumentType::Swap];
        });
    client.on_instrument(query_order_instrument());
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

    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].venue_order_id, VenueOrderId::from("ord_456"));
    assert_eq!(reports[0].order_status, OrderStatus::Triggered);
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_reports_preserves_pending_after_history_not_found() {
    let empty = get(|| async { Json(json!({"code": "0", "msg": "", "data": []})).into_response() });
    let router = Router::new()
        .route("/api/v5/trade/orders-pending", empty.clone())
        .route("/api/v5/trade/orders-history", empty.clone())
        .route(
            "/api/v5/trade/orders-algo-pending",
            get(|Query(params): Query<HashMap<String, String>>| async move {
                if params
                    .get("ordType")
                    .is_some_and(|value| value == "trigger")
                {
                    Json(load_test_data("http_get_orders_algo_pending.json")).into_response()
                } else {
                    Json(json!({"code": "0", "msg": "", "data": []})).into_response()
                }
            }),
        )
        .route(
            "/api/v5/trade/orders-algo-history",
            get(|Query(params): Query<HashMap<String, String>>| async move {
                let is_target = params
                    .get("ordType")
                    .is_some_and(|value| value == "trigger")
                    && params
                        .get("state")
                        .is_some_and(|value| value == "effective");

                if is_target {
                    StatusCode::NOT_FOUND.into_response()
                } else {
                    Json(json!({"code": "0", "msg": "", "data": []})).into_response()
                }
            }),
        )
        .route(
            "/api/v5/account/balance",
            get(|| async { Json(load_test_data("http_get_account_balance.json")).into_response() }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });
    let (mut client, _rx, _cache) =
        create_test_execution_client_configured(&format!("http://{addr}"), |config| {
            config.instrument_types = vec![OKXInstrumentType::Swap];
        });
    client.on_instrument(btc_usdt_swap_instrument());
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

    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].venue_order_id, VenueOrderId::from("123456789"));
    assert_eq!(reports[0].order_status, OrderStatus::Accepted);
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_reports_excludes_old_terminal_pending_record() {
    let empty = get(|| async { Json(json!({"code": "0", "msg": "", "data": []})).into_response() });
    let router = Router::new()
        .route("/api/v5/trade/orders-pending", empty.clone())
        .route("/api/v5/trade/orders-history", empty.clone())
        .route(
            "/api/v5/trade/orders-algo-pending",
            get(|Query(params): Query<HashMap<String, String>>| async move {
                if params
                    .get("ordType")
                    .is_some_and(|value| value == "trigger")
                {
                    let mut response = load_test_data("http_get_orders_algo_pending.json");
                    response["data"][0]["state"] = json!("effective");
                    response["data"][0]["uTime"] = json!("1");
                    Json(response).into_response()
                } else {
                    Json(json!({"code": "0", "msg": "", "data": []})).into_response()
                }
            }),
        )
        .route("/api/v5/trade/orders-algo-history", empty.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });
    let (mut client, _rx, _cache) =
        create_test_execution_client_configured(&format!("http://{addr}"), |config| {
            config.instrument_types = vec![OKXInstrumentType::Swap];
        });
    client.on_instrument(btc_usdt_swap_instrument());
    let cmd = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        false,
        None,
        Some(UnixNanos::from(2_000_000_u64)),
        None,
        None,
        None,
    );

    let reports = client.generate_order_status_reports(&cmd).await.unwrap();

    assert!(reports.is_empty());
}

#[rstest]
#[case::service_unavailable(StatusCode::SERVICE_UNAVAILABLE, 2)]
#[case::not_found(StatusCode::NOT_FOUND, 1)]
#[tokio::test]
async fn test_generate_mass_status_preserves_reports_when_algo_history_is_unavailable(
    #[case] failure_status: StatusCode,
    #[case] expected_attempts: usize,
) {
    let pending_attempts = Arc::new(AtomicUsize::new(0));
    let pending_handler_attempts = Arc::clone(&pending_attempts);
    let history_attempts = Arc::new(AtomicUsize::new(0));
    let history_handler_attempts = Arc::clone(&history_attempts);
    let empty = get(|| async { Json(json!({"code": "0", "msg": "", "data": []})).into_response() });
    let router = Router::new()
        .route("/api/v5/trade/orders-pending", empty.clone())
        .route("/api/v5/trade/orders-history", empty.clone())
        .route(
            "/api/v5/trade/orders-algo-pending",
            get(move |Query(params): Query<HashMap<String, String>>| {
                let attempts = Arc::clone(&pending_handler_attempts);
                async move {
                    attempts.fetch_add(1, Ordering::Relaxed);

                    if params
                        .get("ordType")
                        .is_some_and(|value| value == "trigger")
                    {
                        Json(load_test_data("http_get_orders_algo_pending.json")).into_response()
                    } else {
                        Json(json!({"code": "0", "msg": "", "data": []})).into_response()
                    }
                }
            }),
        )
        .route(
            "/api/v5/trade/orders-algo-history",
            get(move |Query(params): Query<HashMap<String, String>>| {
                let attempts = Arc::clone(&history_handler_attempts);
                async move {
                    let is_target = params
                        .get("ordType")
                        .is_some_and(|value| value == "trigger")
                        && params
                            .get("state")
                            .is_some_and(|value| value == "effective");

                    if is_target {
                        attempts.fetch_add(1, Ordering::Relaxed);
                        failure_status.into_response()
                    } else {
                        Json(json!({"code": "0", "msg": "", "data": []})).into_response()
                    }
                }
            }),
        )
        .route("/api/v5/trade/fills", empty.clone())
        .route("/api/v5/account/positions", empty.clone())
        .route(
            "/api/v5/account/balance",
            get(|| async { Json(load_test_data("http_get_account_balance.json")).into_response() }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });
    let (mut client, _rx, _cache) =
        create_test_execution_client_configured(&format!("http://{addr}"), |config| {
            config.instrument_types = vec![OKXInstrumentType::Swap];
            config.max_retries = 1;
            config.retry_delay_initial_ms = 1;
            config.retry_delay_max_ms = 1;
        });
    client.on_instrument(btc_usdt_swap_instrument());

    let mass_status = client.generate_mass_status(None).await.unwrap().unwrap();

    assert!(!mass_status.reports_complete());
    assert!(
        mass_status
            .order_reports()
            .contains_key(&VenueOrderId::from("123456789"))
    );
    assert_eq!(pending_attempts.load(Ordering::Relaxed), 4);
    assert_eq!(history_attempts.load(Ordering::Relaxed), expected_attempts);
}

#[rstest]
#[tokio::test]
async fn test_generate_mass_status_does_not_invent_net_flat_for_cached_hedge_position() {
    let state = Arc::new(ReportRouteState::default());
    let addr = start_exec_report_test_server(Arc::clone(&state)).await;
    let base_url = format!("http://{addr}");
    let (mut client, _rx, cache) = create_test_execution_client_configured(&base_url, |config| {
        config.instrument_types = vec![OKXInstrumentType::Swap];
    });
    let instrument = query_order_instrument();
    client.on_instrument(instrument.clone());

    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(instrument.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from("1"))
        .build();
    let fill = TestOrderEventStubs::filled(
        &order,
        &instrument,
        Some(TradeId::from("T-HEDGE-001")),
        Some(PositionId::from("P-HEDGE-001")),
        Some(Price::from("2000")),
        Some(Quantity::from("1")),
        Some(LiquiditySide::Taker),
        Some(Money::from("0.01 USDT")),
        None,
        Some(AccountId::from("OKX-001")),
    );
    let position = Position::new(&instrument, fill.into());
    cache
        .borrow_mut()
        .add_position(&position, OmsType::Hedging)
        .unwrap();

    let mass_status = client.generate_mass_status(None).await.unwrap().unwrap();

    assert!(mass_status.reports_complete());
    assert!(mass_status.position_reports().is_empty());
}

#[rstest]
#[tokio::test]
async fn test_generate_mass_status_fails_on_malformed_fill_quantity() {
    let mut fill = load_test_data("http_transaction_detail.json");
    fill["fillSz"] = json!("invalid");
    fill["instId"] = json!("ETH-USDT-SWAP");
    fill["instType"] = json!("SWAP");
    fill["ts"] = json!(jiff::Timestamp::now().as_millisecond().to_string());
    let empty = get(|| async { Json(json!({"code": "0", "msg": "", "data": []})).into_response() });
    let router = Router::new()
        .route("/api/v5/trade/orders-pending", empty.clone())
        .route("/api/v5/trade/orders-history", empty.clone())
        .route("/api/v5/trade/orders-algo-pending", empty.clone())
        .route("/api/v5/trade/orders-algo-history", empty.clone())
        .route(
            "/api/v5/trade/fills",
            get(move || {
                let fill = fill.clone();
                async move { Json(json!({"code": "0", "msg": "", "data": [fill]})) }
            }),
        )
        .route("/api/v5/account/positions", empty.clone())
        .route(
            "/api/v5/account/balance",
            get(|| async { Json(load_test_data("http_get_account_balance.json")).into_response() }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });
    let (mut client, _rx, _cache) =
        create_test_execution_client_configured(&format!("http://{addr}"), |config| {
            config.instrument_types = vec![OKXInstrumentType::Swap];
        });
    client.on_instrument(query_order_instrument());

    let error = client.generate_mass_status(None).await.unwrap_err();

    assert!(error.to_string().contains("failed to parse fill quantity"));
}

#[rstest]
#[tokio::test]
async fn test_generate_mass_status_recovers_historical_triggered_child_status() {
    let detail_queries = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let addr = start_mass_status_triggered_child_server(
        Arc::clone(&detail_queries),
        "effective",
        Some("filled"),
        "1",
        "OMASSTRIGGERED1",
        "",
        "ETH-USDT-SWAP",
        None,
        1,
        false,
        &["mass-triggered-child-venue-id"],
    )
    .await;
    let base_url = format!("http://{addr}");
    let (mut client, _rx, _cache) = create_test_execution_client_configured(&base_url, |config| {
        config.instrument_types = vec![OKXInstrumentType::Swap];
    });
    client.on_instrument(query_order_instrument());

    let mass_status = client.generate_mass_status(None).await.unwrap().unwrap();
    let reports = mass_status.order_reports();
    let report = reports
        .get(&VenueOrderId::from("mass-triggered-child-venue-id"))
        .expect("expected recovered triggered child report");
    let detail_queries = detail_queries.lock().await;

    assert!(mass_status.reports_complete());
    assert!(mass_status.lookback_start().is_some());
    assert_eq!(reports.len(), 1);
    let position_reports = mass_status.position_reports();
    let position_report = position_reports
        .get(&InstrumentId::from("ETH-USDT-SWAP.OKX"))
        .and_then(|reports| reports.first())
        .expect("expected explicit flat position report");
    assert_eq!(detail_queries.len(), 1);
    assert_eq!(
        detail_queries[0].get("ordId").map(String::as_str),
        Some("mass-triggered-child-venue-id")
    );
    assert_eq!(
        detail_queries[0].get("instId").map(String::as_str),
        Some("ETH-USDT-SWAP")
    );
    assert_eq!(
        report.client_order_id,
        Some(ClientOrderId::from("OMASSTRIGGERED1"))
    );
    assert_eq!(report.order_status, OrderStatus::Filled);
    assert_eq!(report.quantity, Quantity::from("1"));
    assert_eq!(report.filled_qty, Quantity::from("1"));
    assert_eq!(position_reports.len(), 1);
    assert_eq!(position_report.position_side, PositionSide::Flat);
    assert_eq!(position_report.quantity, Quantity::from("0"));
}

#[rstest]
#[case("live", "0", OrderStatus::Triggered, Quantity::from("0"))]
#[case(
    "partially_filled",
    "0.5",
    OrderStatus::PartiallyFilled,
    Quantity::from("0.5")
)]
#[case("canceled", "0", OrderStatus::Canceled, Quantity::from("0"))]
#[case("post_only_rejected", "0", OrderStatus::Rejected, Quantity::from("0"))]
#[tokio::test]
async fn test_generate_mass_status_preserves_triggered_child_status(
    #[case] child_state: &'static str,
    #[case] filled_qty: &'static str,
    #[case] expected_status: OrderStatus,
    #[case] expected_filled_qty: Quantity,
) {
    let detail_queries = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let addr = start_mass_status_triggered_child_server(
        Arc::clone(&detail_queries),
        "effective",
        Some(child_state),
        filled_qty,
        "OMASSTRIGGERED1",
        "",
        "ETH-USDT-SWAP",
        None,
        1,
        false,
        &["mass-triggered-child-venue-id"],
    )
    .await;
    let base_url = format!("http://{addr}");
    let (mut client, _rx, _cache) = create_test_execution_client_configured(&base_url, |config| {
        config.instrument_types = vec![OKXInstrumentType::Swap];
    });
    client.on_instrument(query_order_instrument());

    let mass_status = client.generate_mass_status(None).await.unwrap().unwrap();
    let reports = mass_status.order_reports();
    let report = reports
        .get(&VenueOrderId::from("mass-triggered-child-venue-id"))
        .expect("expected recovered triggered child report");
    let detail_queries = detail_queries.lock().await;

    assert!(mass_status.reports_complete());
    assert_eq!(reports.len(), 1);
    assert_eq!(detail_queries.len(), 1);
    assert_eq!(report.order_status, expected_status);
    assert_eq!(report.filled_qty, expected_filled_qty);
}

#[rstest]
#[tokio::test]
async fn test_generate_mass_status_uses_live_child_quantity_for_close_fraction_parent() {
    let detail_queries = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let addr = start_mass_status_triggered_child_server(
        Arc::clone(&detail_queries),
        "effective",
        Some("live"),
        "0",
        "OMASSCLOSEFRACTION1",
        "",
        "ETH-USDT-SWAP",
        None,
        1,
        false,
        &["mass-triggered-child-venue-id"],
    )
    .await;
    let base_url = format!("http://{addr}");
    let (mut client, _rx, _cache) = create_test_execution_client_configured(&base_url, |config| {
        config.instrument_types = vec![OKXInstrumentType::Swap];
    });
    client.on_instrument(query_order_instrument());

    let mass_status = client.generate_mass_status(None).await.unwrap().unwrap();
    let reports = mass_status.order_reports();
    let report = reports
        .get(&VenueOrderId::from("mass-triggered-child-venue-id"))
        .expect("expected recovered close-fraction child report");

    assert_eq!(detail_queries.lock().await.len(), 1);
    assert_eq!(report.order_status, OrderStatus::Triggered);
    assert_eq!(report.quantity, Quantity::from("1"));
    assert_eq!(report.filled_qty, Quantity::from("0"));
}

#[rstest]
#[tokio::test]
async fn test_generate_mass_status_preserves_external_triggered_child_identity() {
    let detail_queries = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let addr = start_mass_status_triggered_child_server(
        Arc::clone(&detail_queries),
        "effective",
        Some("filled"),
        "1",
        "",
        "OCHILDGENERATED1",
        "ETH-USDT-SWAP",
        None,
        1,
        false,
        &["mass-triggered-child-venue-id"],
    )
    .await;
    let base_url = format!("http://{addr}");
    let (mut client, _rx, _cache) = create_test_execution_client_configured(&base_url, |config| {
        config.instrument_types = vec![OKXInstrumentType::Swap];
    });
    client.on_instrument(query_order_instrument());

    let mass_status = client.generate_mass_status(None).await.unwrap().unwrap();
    let reports = mass_status.order_reports();
    let report = reports
        .get(&VenueOrderId::from("mass-triggered-child-venue-id"))
        .expect("expected recovered external child report");

    assert!(mass_status.reports_complete());
    assert_eq!(report.client_order_id, None);
    assert_eq!(report.order_status, OrderStatus::Filled);
}

#[rstest]
#[case::missing_child(
    "effective",
    None,
    "ETH-USDT-SWAP",
    None,
    1,
    &["mass-triggered-child-venue-id"],
    1
)]
#[case::multiple_children(
    "effective",
    Some("filled"),
    "ETH-USDT-SWAP",
    None,
    1,
    &["mass-triggered-child-venue-id", "other-child-venue-id"],
    0
)]
#[case::mismatched_instrument(
    "effective",
    Some("filled"),
    "BTC-USDT-SWAP",
    None,
    1,
    &["mass-triggered-child-venue-id"],
    1
)]
#[case::mismatched_venue_order_id(
    "effective",
    Some("filled"),
    "ETH-USDT-SWAP",
    Some("other-child-venue-id"),
    1,
    &["mass-triggered-child-venue-id"],
    1
)]
#[case::inconsistent_child_order_id(
    "effective",
    Some("filled"),
    "ETH-USDT-SWAP",
    None,
    1,
    &["other-child-venue-id"],
    0
)]
#[case::child_algo_order(
    "effective",
    Some("filled"),
    "ETH-USDT-SWAP",
    None,
    1,
    &["sub-algo-id"],
    0
)]
#[case::excess_order_details(
    "effective",
    Some("filled"),
    "ETH-USDT-SWAP",
    None,
    2,
    &["mass-triggered-child-venue-id"],
    1
)]
#[tokio::test]
async fn test_generate_mass_status_omits_unresolved_triggered_child(
    #[case] parent_state: &'static str,
    #[case] child_state: Option<&'static str>,
    #[case] response_instrument_id: &'static str,
    #[case] response_venue_order_id: Option<&'static str>,
    #[case] detail_record_count: usize,
    #[case] child_order_ids: &'static [&'static str],
    #[case] expected_detail_queries: usize,
) {
    let detail_queries = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let addr = start_mass_status_triggered_child_server(
        Arc::clone(&detail_queries),
        parent_state,
        child_state,
        "0",
        "OMASSTRIGGERED1",
        "",
        response_instrument_id,
        response_venue_order_id,
        detail_record_count,
        false,
        child_order_ids,
    )
    .await;
    let base_url = format!("http://{addr}");
    let (mut client, _rx, _cache) = create_test_execution_client_configured(&base_url, |config| {
        config.instrument_types = vec![OKXInstrumentType::Swap];
    });
    client.on_instrument(query_order_instrument());

    let mass_status = client.generate_mass_status(None).await.unwrap().unwrap();
    let detail_queries = detail_queries.lock().await;

    assert!(!mass_status.reports_complete());
    assert!(mass_status.order_reports().is_empty());
    assert_eq!(detail_queries.len(), expected_detail_queries);
}

#[rstest]
#[tokio::test]
async fn test_generate_mass_status_preserves_rejected_algo_parent() {
    let detail_queries = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let addr = start_mass_status_triggered_child_server(
        Arc::clone(&detail_queries),
        "order_failed",
        Some("filled"),
        "0",
        "OMASSREJECTED1",
        "",
        "ETH-USDT-SWAP",
        None,
        1,
        false,
        &["mass-triggered-child-venue-id"],
    )
    .await;
    let base_url = format!("http://{addr}");
    let (mut client, _rx, _cache) = create_test_execution_client_configured(&base_url, |config| {
        config.instrument_types = vec![OKXInstrumentType::Swap];
    });
    client.on_instrument(query_order_instrument());

    let mass_status = client.generate_mass_status(None).await.unwrap().unwrap();
    let reports = mass_status.order_reports();
    let report = reports
        .get(&VenueOrderId::from("mass-triggered-child-venue-id"))
        .expect("expected rejected algo report");
    let detail_queries = detail_queries.lock().await;

    assert!(mass_status.reports_complete());
    assert_eq!(reports.len(), 1);
    assert!(detail_queries.is_empty());
    assert_eq!(report.order_status, OrderStatus::Rejected);
}

#[rstest]
#[tokio::test]
async fn test_generate_mass_status_preserves_regular_child_for_ambiguous_algo_parent() {
    let detail_queries = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let addr = start_mass_status_triggered_child_server(
        Arc::clone(&detail_queries),
        "effective",
        Some("live"),
        "0",
        "OMASSTRIGGERED1",
        "",
        "ETH-USDT-SWAP",
        None,
        1,
        true,
        &["mass-triggered-child-venue-id", "other-child-venue-id"],
    )
    .await;
    let base_url = format!("http://{addr}");
    let (mut client, _rx, _cache) = create_test_execution_client_configured(&base_url, |config| {
        config.instrument_types = vec![OKXInstrumentType::Swap];
    });
    client.on_instrument(query_order_instrument());

    let mass_status = client.generate_mass_status(None).await.unwrap().unwrap();
    let reports = mass_status.order_reports();
    let report = reports
        .get(&VenueOrderId::from("mass-triggered-child-venue-id"))
        .expect("expected authoritative regular child report");
    let detail_queries = detail_queries.lock().await;

    assert!(!mass_status.reports_complete());
    assert_eq!(reports.len(), 1);
    assert!(detail_queries.is_empty());
    assert_eq!(report.order_status, OrderStatus::Accepted);
    assert_eq!(report.filled_qty, Quantity::from("0"));
}

#[rstest]
#[tokio::test]
async fn test_generate_mass_status_preserves_external_regular_child_when_detail_missing() {
    let detail_queries = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let addr = start_mass_status_triggered_child_server(
        Arc::clone(&detail_queries),
        "effective",
        None,
        "0",
        "OMASSTRIGGERED1",
        "",
        "ETH-USDT-SWAP",
        None,
        1,
        true,
        &["mass-triggered-child-venue-id"],
    )
    .await;
    let base_url = format!("http://{addr}");
    let (mut client, _rx, _cache) = create_test_execution_client_configured(&base_url, |config| {
        config.instrument_types = vec![OKXInstrumentType::Swap];
    });
    client.on_instrument(query_order_instrument());

    let mass_status = client.generate_mass_status(None).await.unwrap().unwrap();
    let reports = mass_status.order_reports();
    let report = reports
        .get(&VenueOrderId::from("mass-triggered-child-venue-id"))
        .expect("expected regular child fallback report");
    let detail_queries = detail_queries.lock().await;

    assert!(!mass_status.reports_complete());
    assert_eq!(reports.len(), 1);
    assert_eq!(detail_queries.len(), 1);
    assert_eq!(report.order_status, OrderStatus::Accepted);
    assert_eq!(report.filled_qty, Quantity::from("0"));
}

#[rstest]
#[tokio::test]
async fn test_generate_mass_status_recovers_sole_listed_child_order_id() {
    let detail_queries = Arc::new(tokio::sync::Mutex::new(Vec::new()));
    let addr = start_mass_status_triggered_child_server(
        Arc::clone(&detail_queries),
        "effective",
        Some("filled"),
        "1",
        "OMASSLISTONLY1",
        "",
        "ETH-USDT-SWAP",
        None,
        1,
        false,
        &["mass-triggered-child-venue-id"],
    )
    .await;
    let base_url = format!("http://{addr}");
    let (mut client, _rx, _cache) = create_test_execution_client_configured(&base_url, |config| {
        config.instrument_types = vec![OKXInstrumentType::Swap];
    });
    client.on_instrument(query_order_instrument());

    let mass_status = client.generate_mass_status(None).await.unwrap().unwrap();
    let reports = mass_status.order_reports();
    let report = reports
        .get(&VenueOrderId::from("mass-triggered-child-venue-id"))
        .expect("expected recovered listed child report");
    let detail_queries = detail_queries.lock().await;

    assert!(mass_status.reports_complete());
    assert_eq!(reports.len(), 1);
    assert_eq!(detail_queries.len(), 1);
    assert_eq!(report.order_status, OrderStatus::Filled);
    assert_eq!(report.filled_qty, Quantity::from("1"));
}

#[rstest]
#[tokio::test]
async fn test_generate_mass_status_bounds_triggered_child_recovery() {
    let detail_query_count = Arc::new(AtomicUsize::new(0));
    let addr = start_mass_status_recovery_cap_server(Arc::clone(&detail_query_count)).await;
    let base_url = format!("http://{addr}");
    let (mut client, _rx, _cache) = create_test_execution_client_configured(&base_url, |config| {
        config.instrument_types = vec![OKXInstrumentType::Swap];
    });
    client.on_instrument(query_order_instrument());

    let mass_status = client.generate_mass_status(None).await.unwrap().unwrap();
    let reports = mass_status.order_reports();

    assert!(!mass_status.reports_complete());
    assert_eq!(reports.len(), 100);
    assert_eq!(detail_query_count.load(Ordering::Relaxed), 100);
    assert!(reports.values().all(|report| {
        report.order_status == OrderStatus::Filled && report.filled_qty == Quantity::from("1")
    }));
    assert!(reports.contains_key(&VenueOrderId::from("mass-cap-child-99")));
    assert!(!reports.contains_key(&VenueOrderId::from("mass-cap-child-100")));
}

#[expect(clippy::too_many_arguments)]
async fn start_mass_status_triggered_child_server(
    detail_queries: Arc<tokio::sync::Mutex<Vec<HashMap<String, String>>>>,
    parent_state: &'static str,
    child_state: Option<&'static str>,
    filled_qty: &'static str,
    parent_client_order_id: &'static str,
    child_client_order_id: &'static str,
    response_instrument_id: &'static str,
    response_venue_order_id: Option<&'static str>,
    detail_record_count: usize,
    bulk_regular_child: bool,
    child_order_ids: &'static [&'static str],
) -> SocketAddr {
    let algo_history = get(
        move |Query(params): Query<HashMap<String, String>>| async move {
            let returns_parent = params
                .get("ordType")
                .is_some_and(|value| value == "trigger")
                && params
                    .get("state")
                    .is_some_and(|value| value == parent_state);

            if !returns_parent {
                return Json(json!({"code": "0", "msg": "", "data": []})).into_response();
            }

            let mut response = load_test_data("http_get_orders_algo_history.json");
            let order = &mut response["data"][0];
            order["actualPx"] = json!("850.00");
            order["actualSide"] = json!("buy");
            order["actualSz"] = json!(filled_qty);
            order["algoClOrdId"] = json!(parent_client_order_id);
            order["algoId"] = json!("mass-triggered-parent-venue-id");
            order["clOrdId"] = json!("");
            order["instId"] = json!("ETH-USDT-SWAP");
            order["instType"] = json!("SWAP");
            order["ordId"] = json!(if parent_client_order_id == "OMASSLISTONLY1" {
                ""
            } else {
                "mass-triggered-child-venue-id"
            });

            if child_order_ids
                .first()
                .is_some_and(|order_id| order_id.starts_with("sub-algo-"))
            {
                order["ordIdList"] = json!([]);
                order["subAlgoIdList"] = json!(child_order_ids);
            } else {
                order["ordIdList"] = json!(child_order_ids);
            }
            order["ordPx"] = json!("850.00");
            order["posSide"] = json!("net");
            order["side"] = json!("buy");
            order["state"] = json!(parent_state);
            if parent_client_order_id == "OMASSCLOSEFRACTION1" {
                order["closeFraction"] = json!("1");
                order["sz"] = json!("");
            } else {
                order["sz"] = json!("1");
            }
            order["tdMode"] = json!("cross");
            order["triggerPx"] = json!("900.00");
            let now_ms = jiff::Timestamp::now().as_millisecond().to_string();
            order["cTime"] = json!(&now_ms);
            order["triggerTime"] = json!(&now_ms);
            order["uTime"] = json!(now_ms);
            Json(response).into_response()
        },
    );
    let order_detail = get(move |Query(params): Query<HashMap<String, String>>| {
        let detail_queries = Arc::clone(&detail_queries);
        async move {
            detail_queries.lock().await.push(params.clone());
            let Some(child_state) = child_state else {
                return Json(json!({"code": "0", "msg": "", "data": []})).into_response();
            };
            let mut response = regular_order_detail_response(&params);
            let order = &mut response["data"][0];
            order["accFillSz"] = json!(filled_qty);
            order["algoClOrdId"] = json!("");
            order["avgPx"] = json!(if filled_qty == "0" { "" } else { "850.00" });
            order["clOrdId"] = json!(child_client_order_id);
            order["fillPx"] = json!(if filled_qty == "0" { "" } else { "850.00" });
            order["fillSz"] = json!(filled_qty);
            order["instId"] = json!(response_instrument_id);
            let state = if child_state == "post_only_rejected" {
                order["cancelSource"] = json!(OKX_POST_ONLY_CANCEL_SOURCE);
                order["cancelSourceReason"] = json!(OKX_POST_ONLY_CANCEL_REASON);
                order["ordType"] = json!("post_only");
                "canceled"
            } else {
                child_state
            };

            if let Some(response_venue_order_id) = response_venue_order_id {
                order["ordId"] = json!(response_venue_order_id);
            }
            order["state"] = json!(state);
            let now_ms = jiff::Timestamp::now().as_millisecond().to_string();
            order["cTime"] = json!(&now_ms);
            order["uTime"] = json!(now_ms);
            if detail_record_count > 1 {
                let duplicate = order.clone();
                response["data"]
                    .as_array_mut()
                    .unwrap()
                    .resize(detail_record_count, duplicate);
            }
            Json(response).into_response()
        }
    });
    let regular_history = get(move || async move {
        if !bulk_regular_child {
            return Json(json!({"code": "0", "msg": "", "data": []})).into_response();
        }

        let mut response = load_test_data("http_get_orders_history.json");
        let order = &mut response["data"][0];
        order["accFillSz"] = json!("0");
        order["algoClOrdId"] = json!(parent_client_order_id);
        order["avgPx"] = json!("");
        order["clOrdId"] = json!("");
        order["fillPx"] = json!("");
        order["fillSz"] = json!("");
        order["instId"] = json!("ETH-USDT-SWAP");
        order["ordId"] = json!("mass-triggered-child-venue-id");
        order["ordType"] = json!("limit");
        order["posSide"] = json!("net");
        order["px"] = json!("850.00");
        order["side"] = json!("buy");
        order["state"] = json!("live");
        order["sz"] = json!("1");
        order["tdMode"] = json!("cross");
        let now_ms = jiff::Timestamp::now().as_millisecond().to_string();
        order["cTime"] = json!(&now_ms);
        order["uTime"] = json!(now_ms);
        Json(response).into_response()
    });
    let empty = get(|| async { Json(json!({"code": "0", "msg": "", "data": []})).into_response() });
    let router = Router::new()
        .route("/api/v5/trade/order", order_detail)
        .route("/api/v5/trade/orders-pending", empty.clone())
        .route("/api/v5/trade/orders-history", regular_history)
        .route("/api/v5/trade/orders-algo-pending", empty.clone())
        .route("/api/v5/trade/orders-algo-history", algo_history)
        .route("/api/v5/trade/fills", empty.clone())
        .route("/api/v5/trade/fills-history", empty.clone())
        .route("/api/v5/account/positions", empty)
        .route(
            "/api/v5/account/balance",
            get(|| async { Json(load_test_data("http_get_account_balance.json")).into_response() }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });
    addr
}

async fn start_mass_status_recovery_cap_server(detail_query_count: Arc<AtomicUsize>) -> SocketAddr {
    let algo_history = get(
        move |Query(params): Query<HashMap<String, String>>| async move {
            let returns_parents = params
                .get("ordType")
                .is_some_and(|value| value == "trigger")
                && params
                    .get("state")
                    .is_some_and(|value| value == "effective")
                && !params.contains_key("after");

            if !returns_parents {
                return Json(json!({"code": "0", "msg": "", "data": []})).into_response();
            }

            let template = load_test_data("http_get_orders_algo_history.json")["data"][0].clone();
            let now_ms = jiff::Timestamp::now().as_millisecond().to_string();
            let data: Vec<_> = (0..101)
                .map(|index| {
                    let mut order = template.clone();
                    let child_order_id = format!("mass-cap-child-{index}");
                    order["actualPx"] = json!("850.00");
                    order["actualSide"] = json!("buy");
                    order["actualSz"] = json!("1");
                    order["algoClOrdId"] = json!(format!("OMASSCAP{index}"));
                    order["algoId"] = json!(format!("mass-cap-parent-{index}"));
                    order["clOrdId"] = json!("");
                    order["instId"] = json!("ETH-USDT-SWAP");
                    order["instType"] = json!("SWAP");
                    order["ordId"] = json!(&child_order_id);
                    order["ordIdList"] = json!([child_order_id]);
                    order["ordPx"] = json!("850.00");
                    order["posSide"] = json!("net");
                    order["side"] = json!("buy");
                    order["sz"] = json!("1");
                    order["tdMode"] = json!("cross");
                    order["triggerPx"] = json!("900.00");
                    order["cTime"] = json!(&now_ms);
                    order["triggerTime"] = json!(&now_ms);
                    order["uTime"] = json!(&now_ms);
                    order
                })
                .collect();

            Json(json!({"code": "0", "msg": "", "data": data})).into_response()
        },
    );
    let order_detail = get(move |Query(params): Query<HashMap<String, String>>| {
        let detail_query_count = Arc::clone(&detail_query_count);
        async move {
            detail_query_count.fetch_add(1, Ordering::Relaxed);
            Json(regular_order_detail_response(&params)).into_response()
        }
    });
    let empty = get(|| async { Json(json!({"code": "0", "msg": "", "data": []})).into_response() });
    let router = Router::new()
        .route("/api/v5/trade/order", order_detail)
        .route("/api/v5/trade/orders-pending", empty.clone())
        .route("/api/v5/trade/orders-history", empty.clone())
        .route("/api/v5/trade/orders-algo-pending", empty.clone())
        .route("/api/v5/trade/orders-algo-history", algo_history)
        .route("/api/v5/trade/fills", empty.clone())
        .route("/api/v5/trade/fills-history", empty.clone())
        .route("/api/v5/account/positions", empty)
        .route(
            "/api/v5/account/balance",
            get(|| async { Json(load_test_data("http_get_account_balance.json")).into_response() }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });
    addr
}

async fn start_live_order_report_server(inst_type: &'static str) -> SocketAddr {
    let router = Router::new()
        .route("/health", get(|| async { Json(json!({"ok": true})) }))
        .route(
            "/api/v5/trade/orders-pending",
            get(move || async move {
                let mut response = load_test_data("http_get_orders_pending.json");
                response["data"][0]["instType"] = json!(inst_type);
                Json(response).into_response()
            }),
        )
        .route(
            "/api/v5/trade/orders-history",
            get(|| async { Json(json!({"code": "0", "msg": "", "data": []})).into_response() }),
        )
        .route(
            "/api/v5/trade/orders-algo-pending",
            get(|| async { Json(json!({"code": "0", "msg": "", "data": []})).into_response() }),
        )
        .route(
            "/api/v5/trade/orders-algo-history",
            get(|| async { Json(json!({"code": "0", "msg": "", "data": []})).into_response() }),
        )
        .route(
            "/api/v5/account/balance",
            get(|| async { Json(load_test_data("http_get_account_balance.json")).into_response() }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });
    addr
}

#[rstest]
#[tokio::test]
async fn test_in_scope_open_order_cache_miss_fails_report_request() {
    let addr = start_live_order_report_server("SWAP").await;
    let base_url = format!("http://{addr}");
    let (client, _rx, _cache) = create_test_execution_client_configured(&base_url, |config| {
        config.instrument_types = vec![OKXInstrumentType::Swap];
    });
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

    let error = client
        .generate_order_status_reports(&cmd)
        .await
        .unwrap_err();

    assert!(error.to_string().contains("missing from cache"));
}

#[rstest]
#[tokio::test]
async fn test_margin_only_open_spot_cache_miss_fails_report_request() {
    let addr = start_live_order_report_server("SPOT").await;
    let base_url = format!("http://{addr}");
    let (client, _rx, _cache) = create_test_execution_client_configured(&base_url, |config| {
        config.instrument_types = vec![OKXInstrumentType::Margin];
        config.margin_mode = Some(OKXMarginMode::Cross);
        config.use_spot_margin = true;
    });
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

    let error = client
        .generate_order_status_reports(&cmd)
        .await
        .unwrap_err();

    assert!(error.to_string().contains("missing from cache"));
}

async fn start_stale_pending_order_report_server() -> SocketAddr {
    let router = Router::new()
        .route("/health", get(|| async { Json(json!({"ok": true})) }))
        .route(
            "/api/v5/trade/orders-pending",
            get(|| async move {
                let mut response = load_test_data("http_get_orders_pending.json");
                response["data"][0]["uTime"] = json!("1600000000000");
                Json(response).into_response()
            }),
        )
        .route(
            "/api/v5/trade/orders-history",
            get(|| async move {
                // Closed history outside the report window must stay excluded.
                let mut response = load_test_data("http_get_orders_pending.json");
                response["data"][0]["uTime"] = json!("1600000000000");
                response["data"][0]["cTime"] = json!("1600000000000");
                response["data"][0]["state"] = json!("canceled");
                response["data"][0]["ordId"] = json!("9999999999999999999");
                response["data"][0]["clOrdId"] = json!("Ostaleclosedhistory0");
                Json(response).into_response()
            }),
        )
        .route(
            "/api/v5/trade/orders-algo-pending",
            get(|| async {
                Json(load_test_data("http_get_orders_algo_pending.json")).into_response()
            }),
        )
        .route(
            "/api/v5/trade/orders-algo-history",
            get(|| async {
                Json(load_test_data("http_get_orders_algo_history.json")).into_response()
            }),
        )
        .route(
            "/api/v5/account/balance",
            get(|| async { Json(load_test_data("http_get_account_balance.json")).into_response() }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });
    addr
}

#[tokio::test]
async fn test_failed_connect_tears_down_websockets_before_retry() {
    let (addr, ws_state) = start_exec_session_failure_server().await;
    let ws_base = format!("ws://{addr}");
    let (mut client, _rx, _cache) =
        create_test_execution_client_configured(&format!("http://{addr}"), |config| {
            config.instrument_types = vec![OKXInstrumentType::Swap];
            config.base_url_ws_private = Some(format!("{ws_base}/ws/v5/private"));
            config.base_url_ws_business = Some(format!("{ws_base}/ws/v5/business"));
        });

    for attempt in 1..=2 {
        let error = client.connect().await.unwrap_err();
        assert!(
            error.to_string().contains("account state"),
            "expected account state failure after transports started: {error}"
        );

        let expected_connections = attempt * 2;
        wait_until_async(
            || {
                let ws_state = Arc::clone(&ws_state);
                async move { ws_state.closed.load(Ordering::Relaxed) >= expected_connections }
            },
            Duration::from_secs(5),
        )
        .await;

        assert_eq!(
            ws_state.opened.load(Ordering::Relaxed),
            expected_connections,
            "each connect attempt must open fresh private and business websockets"
        );
        assert_eq!(
            ws_state.closed.load(Ordering::Relaxed),
            expected_connections,
            "failed connect must close both started websockets before retry"
        );
    }
}

#[rstest]
#[case::older_than_start(Some(1_700_000_000_000_000_000), None)]
#[case::newer_than_end(None, Some(1_500_000_000_000_000_000))]
#[tokio::test]
async fn test_open_order_outside_report_window_is_reported(
    #[case] start_ns: Option<u64>,
    #[case] end_ns: Option<u64>,
) {
    let addr = start_stale_pending_order_report_server().await;
    let base_url = format!("http://{addr}");
    let (mut client, _rx, _cache) = create_test_execution_client_configured(&base_url, |config| {
        config.instrument_types = vec![OKXInstrumentType::Swap];
    });
    client.on_instrument(btc_usdt_swap_instrument());

    let cmd = GenerateOrderStatusReports::new(
        UUID4::new(),
        UnixNanos::default(),
        false,
        None,
        start_ns.map(UnixNanos::from),
        end_ns.map(UnixNanos::from),
        None,
        None,
    );

    let reports = client.generate_order_status_reports(&cmd).await.unwrap();

    assert!(
        reports
            .iter()
            .any(|r| r.venue_order_id == VenueOrderId::from("1234567890123456789")),
        "a live order outside the report window must still be reported"
    );
    assert!(
        reports
            .iter()
            .any(|r| r.venue_order_id == VenueOrderId::from("123456789")),
        "a live algo order outside the report window must still be reported"
    );
    assert!(
        !reports
            .iter()
            .any(|r| r.venue_order_id == VenueOrderId::from("9999999999999999999")),
        "a closed order outside the report window must stay excluded"
    );
    assert!(
        !reports
            .iter()
            .any(|r| r.venue_order_id == VenueOrderId::from("987654321")),
        "a triggered algo parent outside the report window must stay excluded"
    );
}

#[rstest]
#[tokio::test]
async fn test_out_of_scope_open_order_cache_miss_is_dropped() {
    let addr = start_live_order_report_server("SWAP").await;
    let base_url = format!("http://{addr}");
    let (client, _rx, _cache) = create_test_execution_client_configured(&base_url, |config| {
        config.instrument_types = vec![OKXInstrumentType::Spot];
    });
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

    assert!(reports.is_empty());
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_report_http_failure_is_error() {
    let router = Router::new()
        .route(
            "/api/v5/account/balance",
            get(|| async { Json(load_test_data("http_get_account_balance.json")).into_response() }),
        )
        .route(
            "/api/v5/trade/order",
            get(|| async {
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"code": "1", "msg": "internal", "data": []})),
                )
                    .into_response()
            }),
        )
        .route(
            "/api/v5/trade/order-algo",
            get(|| async {
                (
                    axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"code": "1", "msg": "internal", "data": []})),
                )
                    .into_response()
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });
    let (mut client, _rx, _cache) =
        create_test_execution_client_configured(&format!("http://{addr}"), |config| {
            config.instrument_types = vec![OKXInstrumentType::Swap];
        });
    client.on_instrument(query_order_instrument());

    let error = client
        .generate_order_status_report(&generate_order_status_report_cmd(
            Some(InstrumentId::from("ETH-USDT-SWAP.OKX")),
            Some(ClientOrderId::from("OQUERYREGULAR1")),
            None,
        ))
        .await
        .unwrap_err();

    assert!(
        !error.to_string().is_empty(),
        "failed lookup must not become Ok(None)"
    );
}

#[rstest]
#[tokio::test]
async fn test_generate_mass_status_marks_historical_cache_miss_incomplete() {
    let router = Router::new()
        .route("/health", get(|| async { Json(json!({"ok": true})) }))
        .route(
            "/api/v5/trade/orders-pending",
            get(|| async { Json(json!({"code": "0", "msg": "", "data": []})).into_response() }),
        )
        .route(
            "/api/v5/trade/orders-history",
            get(|| async {
                let mut response = load_test_data("http_get_orders_history.json");
                response["data"][0]["uTime"] =
                    json!(jiff::Timestamp::now().as_millisecond().to_string());
                Json(response).into_response()
            }),
        )
        .route(
            "/api/v5/trade/orders-algo-pending",
            get(|| async { Json(json!({"code": "0", "msg": "", "data": []})).into_response() }),
        )
        .route(
            "/api/v5/trade/orders-algo-history",
            get(|| async { Json(json!({"code": "0", "msg": "", "data": []})).into_response() }),
        )
        .route(
            "/api/v5/trade/fills",
            get(|| async { Json(json!({"code": "0", "msg": "", "data": []})).into_response() }),
        )
        .route(
            "/api/v5/trade/fills-history",
            get(|| async { Json(json!({"code": "0", "msg": "", "data": []})).into_response() }),
        )
        .route(
            "/api/v5/account/positions",
            get(|| async { Json(json!({"code": "0", "msg": "", "data": []})).into_response() }),
        )
        .route(
            "/api/v5/account/balance",
            get(|| async { Json(load_test_data("http_get_account_balance.json")).into_response() }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });
    let (client, _rx, _cache) =
        create_test_execution_client_configured(&format!("http://{addr}"), |config| {
            config.instrument_types = vec![OKXInstrumentType::Swap];
        });

    let mass_status = client
        .generate_mass_status(Some(60))
        .await
        .unwrap()
        .unwrap();

    assert!(mass_status.lookback_start().is_some());
    assert!(!mass_status.reports_complete());
    assert!(mass_status.order_reports().is_empty());
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_reports_fails_on_open_algo_cache_miss() {
    let router = Router::new()
        .route(
            "/api/v5/trade/orders-pending",
            get(|| async { Json(json!({"code": "0", "msg": "", "data": []})).into_response() }),
        )
        .route(
            "/api/v5/trade/orders-history",
            get(|| async { Json(json!({"code": "0", "msg": "", "data": []})).into_response() }),
        )
        .route(
            "/api/v5/trade/orders-algo-pending",
            get(|| async {
                Json(load_test_data("http_get_orders_algo_pending.json")).into_response()
            }),
        )
        .route(
            "/api/v5/trade/orders-algo-history",
            get(|| async { Json(json!({"code": "0", "msg": "", "data": []})).into_response() }),
        )
        .route(
            "/api/v5/account/balance",
            get(|| async { Json(load_test_data("http_get_account_balance.json")).into_response() }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });
    let (client, _rx, _cache) =
        create_test_execution_client_configured(&format!("http://{addr}"), |config| {
            config.instrument_types = vec![OKXInstrumentType::Swap];
        });
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

    let error = client
        .generate_order_status_reports(&cmd)
        .await
        .unwrap_err();

    assert!(error.to_string().contains("missing from cache"));
}

#[rstest]
#[tokio::test]
async fn test_generate_fill_reports_fails_on_spread_missing_fee() {
    let mut fill = load_test_data("http_get_spread_trades.json");
    fill["data"][0]["fee"] = json!("");
    fill["data"][0]["sprdId"] = json!("BCH-USDT_BCH-USDT-SWAP");
    let router = Router::new()
        .route(
            "/api/v5/trade/fills",
            get(|| async { Json(json!({"code": "0", "msg": "", "data": []})).into_response() }),
        )
        .route(
            "/api/v5/sprd/trades",
            get(move || {
                let fill = fill.clone();
                async move { Json(fill).into_response() }
            }),
        )
        .route(
            "/api/v5/account/balance",
            get(|| async { Json(load_test_data("http_get_account_balance.json")).into_response() }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .unwrap();
    });
    let (mut client, _rx, _cache) =
        create_test_execution_client_configured(&format!("http://{addr}"), |config| {
            config.instrument_types = vec![OKXInstrumentType::Swap];
            config.load_spreads = true;
        });
    client.on_instrument(make_spread_instrument());
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

    let error = client.generate_fill_reports(cmd).await.unwrap_err();

    assert!(
        format!("{error:#}").contains("missing fee"),
        "was {error:#}"
    );
}

#[rstest]
fn test_dispatch_fill_claims_trade_id() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    let fill = make_fill_report("O-ROUTE-1");
    let trade_id = fill.trade_id;

    assert!(!state.contains_trade(&trade_id));
    dispatch_execution_reports(vec![ExecutionReport::Fill(fill)], &emitter, &state);
    let events = drain_events(&mut rx);

    assert_eq!(events.len(), 1);
    assert!(state.contains_trade(&trade_id));
}
