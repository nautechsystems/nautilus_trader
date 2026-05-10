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

use nautilus_common::messages::{
    ExecutionEvent,
    execution::{BatchCancelOrders, CancelOrder},
};
use nautilus_core::{UUID4, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::{AccountType, LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce},
    events::{OrderAccepted, OrderTriggered},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TradeId, TraderId,
        VenueOrderId,
    },
    reports::{FillReport, OrderStatusReport},
    types::{Currency, Money, Price, Quantity},
};
use nautilus_okx::{
    execution::{WsDispatchState, dispatch_ws_message},
    websocket::messages::{ExecutionReport, NautilusWsMessage},
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

fn make_order_accepted(cid: &str) -> OrderAccepted {
    OrderAccepted::new(
        TraderId::from("TESTER-001"),
        StrategyId::from("S-001"),
        InstrumentId::from("ETH-USDT-SWAP.OKX"),
        ClientOrderId::new(cid),
        VenueOrderId::new("v-1"),
        AccountId::from("OKX-001"),
        UUID4::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
    )
}

fn make_order_triggered(cid: &str) -> OrderTriggered {
    OrderTriggered::new(
        TraderId::from("TESTER-001"),
        StrategyId::from("S-001"),
        InstrumentId::from("ETH-USDT-SWAP.OKX"),
        ClientOrderId::new(cid),
        UUID4::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        false,
        Some(VenueOrderId::new("v-1")),
        Some(AccountId::from("OKX-001")),
    )
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
    let msg = NautilusWsMessage::OrderAccepted(make_order_accepted("O-001"));

    dispatch_ws_message(msg, &emitter, &state);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], ExecutionEvent::Order(_)));
}

#[rstest]
fn test_dispatch_order_triggered_passes_through() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    let msg = NautilusWsMessage::OrderTriggered(make_order_triggered("O-001"));

    dispatch_ws_message(msg, &emitter, &state);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], ExecutionEvent::Order(_)));
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
    let msg =
        NautilusWsMessage::ExecutionReports(vec![ExecutionReport::Fill(make_fill_report("O-001"))]);

    dispatch_ws_message(msg, &emitter, &state);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], ExecutionEvent::Report(_)));
    assert!(state.filled_orders.contains(&ClientOrderId::new("O-001")));
}

#[rstest]
fn test_dispatch_order_status_report_accepted_passes_through() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    let msg = NautilusWsMessage::ExecutionReports(vec![ExecutionReport::Order(
        make_order_status_report("O-001", OrderStatus::Accepted),
    )]);

    dispatch_ws_message(msg, &emitter, &state);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
}

#[rstest]
fn test_dispatch_order_accepted_skipped_when_already_triggered() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    state.triggered_orders.insert(ClientOrderId::new("O-001"));

    let msg = NautilusWsMessage::OrderAccepted(make_order_accepted("O-001"));
    dispatch_ws_message(msg, &emitter, &state);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 0);
}

#[rstest]
fn test_dispatch_order_accepted_skipped_when_already_filled() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    state.filled_orders.insert(ClientOrderId::new("O-001"));

    let msg = NautilusWsMessage::OrderAccepted(make_order_accepted("O-001"));
    dispatch_ws_message(msg, &emitter, &state);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 0);
}

#[rstest]
fn test_dispatch_order_triggered_skipped_when_already_filled() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    state.filled_orders.insert(ClientOrderId::new("O-001"));

    let msg = NautilusWsMessage::OrderTriggered(make_order_triggered("O-001"));
    dispatch_ws_message(msg, &emitter, &state);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 0);
}

#[rstest]
fn test_dispatch_status_report_accepted_skipped_when_triggered() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    state.triggered_orders.insert(ClientOrderId::new("O-001"));

    let msg = NautilusWsMessage::ExecutionReports(vec![ExecutionReport::Order(
        make_order_status_report("O-001", OrderStatus::Accepted),
    )]);
    dispatch_ws_message(msg, &emitter, &state);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 0);
}

#[rstest]
fn test_dispatch_status_report_accepted_skipped_when_filled() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    state.filled_orders.insert(ClientOrderId::new("O-001"));

    let msg = NautilusWsMessage::ExecutionReports(vec![ExecutionReport::Order(
        make_order_status_report("O-001", OrderStatus::Accepted),
    )]);
    dispatch_ws_message(msg, &emitter, &state);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 0);
}

#[rstest]
fn test_dispatch_status_report_triggered_skipped_when_filled() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    state.filled_orders.insert(ClientOrderId::new("O-001"));

    let msg = NautilusWsMessage::ExecutionReports(vec![ExecutionReport::Order(
        make_order_status_report("O-001", OrderStatus::Triggered),
    )]);
    dispatch_ws_message(msg, &emitter, &state);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 0);
}

#[rstest]
fn test_dispatch_status_report_triggered_records_state() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();

    let msg = NautilusWsMessage::ExecutionReports(vec![ExecutionReport::Order(
        make_order_status_report("O-001", OrderStatus::Triggered),
    )]);
    dispatch_ws_message(msg, &emitter, &state);

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

    let msg = NautilusWsMessage::ExecutionReports(vec![ExecutionReport::Order(
        make_order_status_report("O-001", OrderStatus::Filled),
    )]);
    dispatch_ws_message(msg, &emitter, &state);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert!(state.filled_orders.contains(&ClientOrderId::new("O-001")));
}

#[rstest]
fn test_dispatch_dedup_does_not_affect_different_orders() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();
    state.filled_orders.insert(ClientOrderId::new("O-001"));

    let msg = NautilusWsMessage::OrderAccepted(make_order_accepted("O-002"));
    dispatch_ws_message(msg, &emitter, &state);

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
}

#[rstest]
fn test_dispatch_full_lifecycle_stale_accepted_skipped() {
    let (emitter, mut rx) = test_emitter();
    let state = WsDispatchState::default();

    // 1. Triggered arrives first (from business WS)
    dispatch_ws_message(
        NautilusWsMessage::ExecutionReports(vec![ExecutionReport::Order(
            make_order_status_report("O-001", OrderStatus::Triggered),
        )]),
        &emitter,
        &state,
    );

    // 2. Fill arrives (from private WS)
    dispatch_ws_message(
        NautilusWsMessage::ExecutionReports(vec![ExecutionReport::Fill(make_fill_report("O-001"))]),
        &emitter,
        &state,
    );

    // 3. Stale Accepted arrives late (from private WS)
    dispatch_ws_message(
        NautilusWsMessage::OrderAccepted(make_order_accepted("O-001")),
        &emitter,
        &state,
    );

    // 4. Stale Triggered arrives late (from private WS)
    dispatch_ws_message(
        NautilusWsMessage::OrderTriggered(make_order_triggered("O-001")),
        &emitter,
        &state,
    );

    let events = drain_events(&mut rx);
    // Only the first Triggered report and the Fill should have been emitted
    assert_eq!(events.len(), 2);
}
