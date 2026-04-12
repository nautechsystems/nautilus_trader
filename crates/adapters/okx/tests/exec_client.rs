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

use std::{cell::RefCell, collections::HashMap, net::SocketAddr, rc::Rc, time::Duration};

use axum::{Router, http::HeaderMap, response::IntoResponse, routing::get};
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    live::runner::set_exec_event_sender,
    messages::{
        ExecutionEvent,
        execution::{BatchCancelOrders, CancelOrder, QueryAccount, SubmitOrder, SubmitOrderList},
    },
    testing::wait_until_async,
};
use nautilus_core::{UUID4, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    enums::{AccountType, LiquiditySide, OmsType, OrderSide, OrderStatus, OrderType, TimeInForce},
    events::OrderInitialized,
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, OrderListId, StrategyId, TradeId,
        TraderId, Venue, VenueOrderId,
    },
    orders::OrderList,
    reports::{FillReport, OrderStatusReport},
    types::{Currency, Money, Price, Quantity},
};
use nautilus_network::http::HttpClient;
use nautilus_okx::{
    config::OKXExecClientConfig,
    execution::OKXExecutionClient,
    http::models::OKXCancelAlgoOrderResponse,
    websocket::{
        dispatch::{
            AlgoCancelContext, WsDispatchState, dispatch_execution_reports,
            emit_algo_cancel_rejections, emit_batch_cancel_failure,
        },
        messages::ExecutionReport,
    },
};
use rstest::rstest;

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
fn test_batch_cancel_orders_builds_payload() {
    let trader_id = TraderId::from("TRADER-001");
    let strategy_id = StrategyId::from("STRATEGY-001");
    let client_id = Some(ClientId::from("OKX"));
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
            },
        ],
        command_id: UUID4::default(),
        ts_init: UnixNanos::default(),
        params: None,
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
        client_id: Some(ClientId::from("OKX")),
        strategy_id: StrategyId::from("STRATEGY-001"),
        instrument_id: InstrumentId::from("BTC-USDT.OKX"),
        cancels: vec![],
        command_id: UUID4::default(),
        ts_init: UnixNanos::default(),
        params: None,
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
    let client_id = Some(ClientId::from("OKX"));
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
        Some(ClientId::from("OKX")),
        strategy_id,
        order_list,
        order_inits,
        None,
        None,
        None,
        UUID4::default(),
        UnixNanos::default(),
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
fn test_batch_cancel_failure_emits_for_all_orders() {
    let (emitter, mut rx) = test_emitter();
    let clock = get_atomic_clock_realtime();

    let contexts = vec![
        make_algo_cancel_context("O-001"),
        make_algo_cancel_context("O-002"),
        make_algo_cancel_context("O-003"),
    ];

    emit_batch_cancel_failure(&contexts, "network timeout", &emitter, clock);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 3, "each order should get a rejection");
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
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

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

fn create_test_execution_client(
    base_url: &str,
) -> (
    OKXExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
) {
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("OKX-001");
    let client_id = ClientId::from("OKX");

    let cache = Rc::new(RefCell::new(Cache::default()));

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        Venue::from("OKX"),
        OmsType::Hedging,
        account_id,
        AccountType::Margin,
        None,
        cache,
    );

    let config = OKXExecClientConfig {
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

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let client = OKXExecutionClient::new(core, config).unwrap();

    (client, rx)
}

#[rstest]
#[tokio::test]
async fn test_query_account_does_not_block_within_runtime() {
    let addr = start_exec_test_server().await;
    let base_url = format!("http://{addr}");

    let (mut client, mut rx) = create_test_execution_client(&base_url);

    client.start().unwrap();

    let cmd = QueryAccount::new(
        TraderId::from("TESTER-001"),
        Some(ClientId::from("OKX")),
        AccountId::from("OKX-001"),
        UUID4::new(),
        UnixNanos::default(),
        None,
    );

    let result = client.query_account(cmd);
    assert!(result.is_ok());

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
