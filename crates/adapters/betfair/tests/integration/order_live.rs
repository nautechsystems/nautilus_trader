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

//! Live state-changing smoke against Betfair. Ignored: places, replaces, and cancels a real order.

use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Context;
use nautilus_betfair::{
    common::{
        consts::{
            BETFAIR_CLIENT_ID, METHOD_CANCEL_ORDERS, METHOD_GET_ACCOUNT_DETAILS,
            METHOD_LIST_CURRENT_ORDERS, METHOD_LIST_MARKET_CATALOGUE, METHOD_PLACE_ORDERS,
            METHOD_REPLACE_ORDERS,
        },
        credential::BetfairCredential,
        enums::{
            BetfairOrderStatus, BetfairOrderType, BetfairSide, ExecutionReportErrorCode,
            ExecutionReportStatus, InstructionReportErrorCode, InstructionReportStatus,
            MarketProjection, MarketSort, MarketStatus, OrderProjection, PersistenceType,
            RunnerStatus,
        },
        parse::{make_customer_order_ref, make_instrument_id},
        types::{BetId, Handicap, MarketId, SelectionId},
    },
    config::BetfairExecutionClientConfig,
    factories::BetfairExecutionClientFactory,
    http::{
        client::BetfairHttpClient,
        models::{
            AccountDetailsResponse, CancelExecutionReport, CancelInstruction, CancelOrdersParams,
            CurrentOrderSummary, CurrentOrderSummaryReport, LimitOrder, ListCurrentOrdersParams,
            ListMarketCatalogueParams, MarketCatalogue, MarketFilter, PlaceExecutionReport,
            PlaceInstruction, PlaceInstructionReport, PlaceOrdersParams, PriceSize,
            ReplaceExecutionReport, ReplaceInstruction, ReplaceOrdersParams,
        },
    },
    provider::{BetfairInstrumentProvider, NavigationFilter},
};
use nautilus_common::{
    actor::DataActor,
    enums::Environment,
    messages::system::{SocketState, SocketStateChanged},
    providers::InstrumentProvider,
    timer::TimeEvent,
};
use nautilus_core::UUID4;
use nautilus_live::{
    config::{LiveExecutionEngineConfig, LiveRiskEngineConfig},
    node::LiveNode,
};
use nautilus_model::{
    enums::{OrderSide, OrderStatus, OrderType, TimeInForce},
    events::{
        OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied, OrderFilled,
        OrderModifyRejected, OrderPendingCancel, OrderPendingUpdate, OrderRejected, OrderSubmitted,
        OrderUpdated,
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderTestBuilder},
    types::{Currency, Price, Quantity},
};
use nautilus_trading::{
    nautilus_strategy,
    strategy::{Strategy, StrategyConfig, StrategyCore},
};
use parking_lot::Mutex;
use rstest::rstest;
use rust_decimal::Decimal;
use serde::Deserialize;
use ustr::Ustr;

const RECONNECT_REPLACE_TIMER: &str = "betfair-live-reconnect-replace";
const USER_STREAM_ENDPOINT: &str = "betfair-user-streams";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LiveMarketBook {
    market_id: MarketId,
    status: MarketStatus,
    inplay: bool,
    runners: Vec<LiveRunnerBook>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LiveRunnerBook {
    selection_id: SelectionId,
    handicap: Handicap,
    status: RunnerStatus,
    ex: LiveExchangePrices,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LiveExchangePrices {
    available_to_back: Vec<PriceSize>,
}

#[derive(Debug)]
struct LiveTarget {
    market_id: MarketId,
    selection_id: SelectionId,
    handicap: Handicap,
}

#[derive(Debug)]
struct LiveExecutionFixture {
    credential: BetfairCredential,
    currency_code: String,
    stake: Decimal,
    market_id: MarketId,
    instrument_id: InstrumentId,
    instrument: InstrumentAny,
}

#[derive(Debug)]
struct InvalidReplaceObservation {
    report: ReplaceExecutionReport,
    market_id: MarketId,
    selection_id: SelectionId,
    old_bet_id: BetId,
    stake: Decimal,
    customer_ref: String,
}

#[derive(Debug, Clone, Default)]
struct LiveExecutionState {
    accepted: usize,
    updated: usize,
    canceled: usize,
    filled: usize,
    socket_connected: usize,
    socket_disconnected: usize,
    accepted_bet_id: Option<BetId>,
    updated_bet_id: Option<BetId>,
    bet_ids: HashSet<BetId>,
    failure: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct LiveExecutionProbe {
    state: Arc<Mutex<LiveExecutionState>>,
    completion: Arc<tokio::sync::Notify>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiveExecutionScenario {
    ReplaceCancel,
    InvalidReplace,
    ReconnectReplaceCancel,
    ReplaceFill,
    ReconnectReplaceDuringRecovery,
}

impl LiveExecutionScenario {
    fn reconnects(self) -> bool {
        matches!(
            self,
            Self::ReconnectReplaceCancel | Self::ReconnectReplaceDuringRecovery
        )
    }

    fn expects_fill(self) -> bool {
        self == Self::ReplaceFill
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiveStressEvent {
    Submitted,
    Accepted,
    PendingUpdate,
    Updated,
    PendingCancel,
    Canceled,
}

#[derive(Debug, Default)]
struct LiveStressOrder {
    events: Vec<LiveStressEvent>,
    accepted_bet_id: Option<BetId>,
    updated_bet_id: Option<BetId>,
    canceled_bet_id: Option<BetId>,
}

#[derive(Debug, Default)]
struct LiveStressState {
    orders: HashMap<ClientOrderId, LiveStressOrder>,
    completed: HashSet<ClientOrderId>,
    failure: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct LiveStressProbe {
    state: Arc<Mutex<LiveStressState>>,
}

impl LiveStressProbe {
    fn register(&self, client_order_id: ClientOrderId) {
        let previous = self
            .state
            .lock()
            .orders
            .insert(client_order_id, LiveStressOrder::default());
        assert!(previous.is_none(), "duplicate live stress client order ID");
    }

    fn record(
        &self,
        client_order_id: ClientOrderId,
        event: LiveStressEvent,
        bet_id: Option<BetId>,
    ) -> bool {
        let mut state = self.state.lock();
        let Some(order) = state.orders.get_mut(&client_order_id) else {
            state.failure = Some(format!(
                "event for unknown order {client_order_id}: {event:?}"
            ));
            return false;
        };
        let first = !order.events.contains(&event);
        order.events.push(event);

        if first {
            match event {
                LiveStressEvent::Accepted => order.accepted_bet_id = bet_id,
                LiveStressEvent::Updated => order.updated_bet_id = bet_id,
                LiveStressEvent::Canceled => order.canceled_bet_id = bet_id,
                LiveStressEvent::Submitted
                | LiveStressEvent::PendingUpdate
                | LiveStressEvent::PendingCancel => {}
            }
        } else if state.failure.is_none() {
            state.failure = Some(format!(
                "duplicate {event:?} event for order {client_order_id}"
            ));
        }

        first
    }

    fn mark_completed(&self, client_order_id: ClientOrderId) -> bool {
        self.state.lock().completed.insert(client_order_id)
    }

    fn fail(&self, reason: impl Into<String>) {
        let mut state = self.state.lock();
        if state.failure.is_none() {
            state.failure = Some(reason.into());
        }
    }

    fn finished(&self, expected: usize) -> bool {
        let state = self.state.lock();
        state.failure.is_some() || state.completed.len() == expected
    }

    fn failed(&self) -> bool {
        self.state.lock().failure.is_some()
    }
}

impl LiveExecutionProbe {
    fn record_socket_state(&self, socket_state: SocketState) -> bool {
        let mut state = self.state.lock();
        let count = match socket_state {
            SocketState::Connected => &mut state.socket_connected,
            SocketState::Disconnected => &mut state.socket_disconnected,
        };
        *count += 1;
        *count == 1
    }

    fn record_accepted(&self, bet_id: BetId) -> bool {
        let mut state = self.state.lock();
        state.record_bet_id(Some(bet_id.clone()));
        state.accepted_bet_id = Some(bet_id);
        state.accepted += 1;
        state.accepted == 1
    }

    fn record_updated(&self, bet_id: Option<BetId>) -> bool {
        let mut state = self.state.lock();
        state.record_bet_id(bet_id.clone());
        state.updated_bet_id = bet_id;
        state.updated += 1;
        state.updated == 1
    }

    fn record_canceled(&self, bet_id: Option<BetId>) {
        {
            let mut state = self.state.lock();
            state.record_bet_id(bet_id);
            state.canceled += 1;
        }
        self.completion.notify_one();
    }

    fn record_filled(&self, bet_id: BetId) {
        {
            let mut state = self.state.lock();
            state.record_bet_id(Some(bet_id));
            state.filled += 1;
            let failure = if state.updated != 1 {
                Some(format!(
                    "replacement fill arrived after {} order updates",
                    state.updated,
                ))
            } else if state.filled != 1 {
                Some("duplicate replacement fill event".to_string())
            } else {
                None
            };

            if state.failure.is_none() {
                state.failure = failure;
            }
        }
        self.completion.notify_one();
    }

    fn fail(&self, reason: impl Into<String>) {
        {
            let mut state = self.state.lock();
            if state.failure.is_none() {
                state.failure = Some(reason.into());
            }
        }
        self.completion.notify_one();
    }

    fn finished(&self) -> bool {
        let state = self.state.lock();
        state.canceled == 1 || state.filled == 1 || state.failure.is_some()
    }

    async fn wait_finished(&self) {
        loop {
            let notified = self.completion.notified();

            if self.finished() {
                return;
            }
            notified.await;
        }
    }

    fn snapshot(&self) -> LiveExecutionState {
        self.state.lock().clone()
    }
}

impl LiveExecutionState {
    fn record_bet_id(&mut self, bet_id: Option<BetId>) {
        if let Some(bet_id) = bet_id {
            self.bet_ids.insert(bet_id);
        }
    }
}

#[derive(Debug)]
struct LiveExecutionLifecycle {
    core: StrategyCore,
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    quantity: Quantity,
    replace_price: Price,
    scenario: LiveExecutionScenario,
    probe: LiveExecutionProbe,
    reconnect_requested: bool,
    replace_requested: bool,
}

impl LiveExecutionLifecycle {
    fn new(
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        quantity: Quantity,
        scenario: LiveExecutionScenario,
        probe: LiveExecutionProbe,
    ) -> Self {
        let replace_price = match scenario {
            LiveExecutionScenario::InvalidReplace => Price::from("2.57"),
            LiveExecutionScenario::ReplaceFill => Price::from("1.01"),
            LiveExecutionScenario::ReplaceCancel
            | LiveExecutionScenario::ReconnectReplaceCancel
            | LiveExecutionScenario::ReconnectReplaceDuringRecovery => Price::from("980"),
        };
        Self {
            core: StrategyCore::new(StrategyConfig {
                strategy_id: Some(StrategyId::from("BETFAIR-LIVE-SMOKE")),
                ..Default::default()
            }),
            instrument_id,
            client_order_id,
            quantity,
            replace_price,
            scenario,
            probe,
            reconnect_requested: false,
            replace_requested: false,
        }
    }

    fn request_replace(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(!self.replace_requested, "replace already requested");
        self.replace_requested = true;
        self.modify_order(
            self.client_order_id,
            None,
            Some(self.replace_price),
            None,
            Some(*BETFAIR_CLIENT_ID),
            None,
        )
    }

    fn request_reconnect(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(!self.reconnect_requested, "reconnect already requested");
        self.reconnect_requested = true;
        self.reconnect_socket(ClientId::from("BETFAIR"), USER_STREAM_ENDPOINT)
    }

    fn is_requested_user_stream_state(&self, event: &SocketStateChanged) -> bool {
        self.reconnect_requested
            && event.client_id == *BETFAIR_CLIENT_ID
            && event.endpoint.as_str() == USER_STREAM_ENDPOINT
    }

    fn should_replace_during_recovery(
        &self,
        socket_state: SocketState,
        first_observation: bool,
    ) -> bool {
        self.scenario == LiveExecutionScenario::ReconnectReplaceDuringRecovery
            && socket_state == SocketState::Disconnected
            && first_observation
    }
}

impl DataActor for LiveExecutionLifecycle {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.subscribe_socket_state(None);
        let order = OrderTestBuilder::new(OrderType::Limit)
            .trader_id(TraderId::from("BETFAIR-LIVE-TESTER"))
            .strategy_id(StrategyId::from("BETFAIR-LIVE-SMOKE"))
            .instrument_id(self.instrument_id)
            .client_order_id(self.client_order_id)
            .side(OrderSide::Sell)
            .price(Price::from("990"))
            .quantity(self.quantity)
            .time_in_force(TimeInForce::Day)
            .build();
        self.submit_order(order, None, Some(*BETFAIR_CLIENT_ID), None)
    }

    fn on_time_event(&mut self, event: &TimeEvent) -> anyhow::Result<()> {
        if event.name == RECONNECT_REPLACE_TIMER {
            self.request_replace()?;
        }
        Ok(())
    }

    fn on_socket_state(&mut self, event: &SocketStateChanged) -> anyhow::Result<()> {
        if !self.is_requested_user_stream_state(event) {
            return Ok(());
        }

        let first = self.probe.record_socket_state(event.state);
        if self.should_replace_during_recovery(event.state, first)
            && let Err(e) = self.request_replace()
        {
            self.probe
                .fail(format!("replace during recovery failed: {e}"));
        }
        Ok(())
    }
}

nautilus_strategy!(LiveExecutionLifecycle, {
    fn on_order_accepted(&mut self, event: OrderAccepted) {
        let bet_id = event.venue_order_id.to_string();

        if self.probe.record_accepted(bet_id) {
            let result = match self.scenario {
                LiveExecutionScenario::ReconnectReplaceCancel => {
                    self.request_reconnect().and_then(|()| {
                        let replace_at = self.clock().timestamp_ns() + 5_000_000_000;
                        self.clock().set_time_alert_ns(
                            RECONNECT_REPLACE_TIMER,
                            replace_at,
                            None,
                            None,
                        )
                    })
                }
                LiveExecutionScenario::ReconnectReplaceDuringRecovery => self.request_reconnect(),
                LiveExecutionScenario::ReplaceCancel
                | LiveExecutionScenario::InvalidReplace
                | LiveExecutionScenario::ReplaceFill => self.request_replace(),
            };

            if let Err(e) = result {
                self.probe.fail(format!("modify setup failed: {e}"));
            }
        }
    }

    fn on_order_updated(&mut self, event: OrderUpdated) {
        let bet_id = event.venue_order_id.map(|id| id.to_string());
        if self.scenario == LiveExecutionScenario::InvalidReplace {
            self.probe.fail(format!(
                "unexpected order update: {}",
                event.client_order_id
            ));
        } else if self.probe.record_updated(bet_id)
            && !self.scenario.expects_fill()
            && let Err(e) = self.cancel_order(event.client_order_id, Some(*BETFAIR_CLIENT_ID), None)
        {
            self.probe.fail(format!("cancel_order failed: {e}"));
        }
    }

    fn on_order_canceled(&mut self, event: &OrderCanceled) {
        self.probe
            .record_canceled(event.venue_order_id.map(|id| id.to_string()));
    }

    fn on_order_rejected(&mut self, event: OrderRejected) {
        self.probe.fail(format!("order rejected: {}", event.reason));
    }

    fn on_order_denied(&mut self, event: OrderDenied) {
        self.probe.fail(format!("order denied: {}", event.reason));
    }

    fn on_order_modify_rejected(&mut self, event: OrderModifyRejected) {
        self.probe
            .fail(format!("modify rejected: {}", event.reason));
    }

    fn on_order_cancel_rejected(&mut self, event: OrderCancelRejected) {
        self.probe
            .fail(format!("cancel rejected: {}", event.reason));
    }

    fn on_order_filled(&mut self, event: &OrderFilled) {
        if self.scenario.expects_fill() {
            self.probe.record_filled(event.venue_order_id.to_string());
        } else {
            self.probe.fail(format!(
                "live validation order matched unexpectedly: {} @ {}",
                event.last_qty, event.last_px,
            ));
        }
    }
});

#[derive(Debug)]
struct LiveExecutionStress {
    core: StrategyCore,
    instrument_id: InstrumentId,
    quantity: Quantity,
    replace_price: Price,
    run_token: String,
    total_orders: usize,
    max_active: usize,
    submitted: usize,
    active: usize,
    probe: LiveStressProbe,
}

impl LiveExecutionStress {
    fn new(
        instrument_id: InstrumentId,
        quantity: Quantity,
        replace_price: Price,
        total_orders: usize,
        max_active: usize,
        probe: LiveStressProbe,
    ) -> Self {
        Self {
            core: StrategyCore::new(StrategyConfig {
                strategy_id: Some(StrategyId::from("BETFAIR-LIVE-STRESS")),
                ..Default::default()
            }),
            instrument_id,
            quantity,
            replace_price,
            run_token: live_ref(),
            total_orders,
            max_active,
            submitted: 0,
            active: 0,
            probe,
        }
    }

    fn submit_until_full(&mut self) -> anyhow::Result<()> {
        while !self.probe.failed()
            && self.active < self.max_active
            && self.submitted < self.total_orders
        {
            let client_order_id =
                ClientOrderId::from(format!("S{}{:05}", &self.run_token[..24], self.submitted,));
            let order = OrderTestBuilder::new(OrderType::Limit)
                .trader_id(TraderId::from("BETFAIR-LIVE-TESTER"))
                .strategy_id(StrategyId::from("BETFAIR-LIVE-STRESS"))
                .instrument_id(self.instrument_id)
                .client_order_id(client_order_id)
                .side(OrderSide::Sell)
                .price(Price::from("990"))
                .quantity(self.quantity)
                .time_in_force(TimeInForce::Day)
                .build();
            self.probe.register(client_order_id);
            self.submitted += 1;
            self.active += 1;
            self.submit_order(order, None, Some(*BETFAIR_CLIENT_ID), None)?;
        }

        Ok(())
    }
}

impl DataActor for LiveExecutionStress {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.submit_until_full()
    }
}

nautilus_strategy!(LiveExecutionStress, {
    fn on_order_submitted(&mut self, event: OrderSubmitted) {
        self.probe
            .record(event.client_order_id, LiveStressEvent::Submitted, None);
    }

    fn on_order_accepted(&mut self, event: OrderAccepted) {
        if self.probe.record(
            event.client_order_id,
            LiveStressEvent::Accepted,
            Some(event.venue_order_id.to_string()),
        ) && let Err(e) = self.modify_order(
            event.client_order_id,
            None,
            Some(self.replace_price),
            None,
            Some(*BETFAIR_CLIENT_ID),
            None,
        ) {
            self.probe.fail(format!(
                "modify_order failed for {}: {e}",
                event.client_order_id,
            ));
        }
    }

    fn on_order_pending_update(&mut self, event: OrderPendingUpdate) {
        self.probe.record(
            event.client_order_id,
            LiveStressEvent::PendingUpdate,
            event.venue_order_id.map(|id| id.to_string()),
        );
    }

    fn on_order_updated(&mut self, event: OrderUpdated) {
        if self.probe.record(
            event.client_order_id,
            LiveStressEvent::Updated,
            event.venue_order_id.map(|id| id.to_string()),
        ) && let Err(e) =
            self.cancel_order(event.client_order_id, Some(*BETFAIR_CLIENT_ID), None)
        {
            self.probe.fail(format!(
                "cancel_order failed for {}: {e}",
                event.client_order_id,
            ));
        }
    }

    fn on_order_pending_cancel(&mut self, event: OrderPendingCancel) {
        self.probe.record(
            event.client_order_id,
            LiveStressEvent::PendingCancel,
            event.venue_order_id.map(|id| id.to_string()),
        );
    }

    fn on_order_canceled(&mut self, event: &OrderCanceled) {
        self.probe.record(
            event.client_order_id,
            LiveStressEvent::Canceled,
            event.venue_order_id.map(|id| id.to_string()),
        );

        if self.probe.mark_completed(event.client_order_id) {
            self.active = self.active.saturating_sub(1);

            if let Err(e) = self.submit_until_full() {
                self.probe.fail(format!("submit_order failed: {e}"));
            }
        }
    }

    fn on_order_rejected(&mut self, event: OrderRejected) {
        self.probe.fail(format!(
            "order rejected for {}: {}",
            event.client_order_id, event.reason,
        ));
    }

    fn on_order_denied(&mut self, event: OrderDenied) {
        self.probe.fail(format!(
            "order denied for {}: {}",
            event.client_order_id, event.reason,
        ));
    }

    fn on_order_modify_rejected(&mut self, event: OrderModifyRejected) {
        self.probe.fail(format!(
            "modify rejected for {}: {}",
            event.client_order_id, event.reason,
        ));
    }

    fn on_order_cancel_rejected(&mut self, event: OrderCancelRejected) {
        self.probe.fail(format!(
            "cancel rejected for {}: {}",
            event.client_order_id, event.reason,
        ));
    }

    fn on_order_filled(&mut self, event: &OrderFilled) {
        self.probe.fail(format!(
            "passive stress order {} matched unexpectedly: {} @ {}",
            event.client_order_id, event.last_qty, event.last_px,
        ));
    }
});

#[rstest]
#[tokio::test]
#[ignore = "places, replaces, and cancels an order on the configured live Betfair account"]
async fn live_limit_order_place_replace_cancel() {
    let credential = BetfairCredential::from_env()
        .expect("BETFAIR_USERNAME, BETFAIR_PASSWORD, and BETFAIR_APP_KEY must be set");
    let client = BetfairHttpClient::new(credential, None, None, None, None, Some(5), Some(20))
        .expect("live HTTP client");
    client.connect().await.expect("Betfair login");

    let customer_order_ref = live_ref();
    let mut known_bet_ids = HashSet::new();
    let smoke_result =
        exercise_order_lifecycle(&client, &customer_order_ref, &mut known_bet_ids).await;
    let cleanup_result = cleanup_orders(
        &client,
        &customer_order_ref,
        &known_bet_ids,
        smoke_result.is_err(),
        Decimal::ZERO,
    )
    .await;
    client.disconnect().await;

    cleanup_result.expect("live smoke cleanup and exposure verification failed");
    smoke_result.expect("live place-replace-cancel smoke failed");
}

#[rstest]
#[tokio::test]
#[ignore = "places an order and attempts an invalid-price replace on the configured live Betfair account"]
async fn live_replace_cancelled_not_placed() {
    let credential = BetfairCredential::from_env()
        .expect("BETFAIR_USERNAME, BETFAIR_PASSWORD, and BETFAIR_APP_KEY must be set");
    let client = BetfairHttpClient::new(credential, None, None, None, None, Some(5), Some(20))
        .expect("live HTTP client");
    client.connect().await.expect("Betfair login");

    let customer_order_ref = live_ref();
    let mut known_bet_ids = HashSet::new();
    let probe_result =
        exercise_invalid_replace(&client, &customer_order_ref, &mut known_bet_ids).await;
    let cleanup_result = cleanup_orders(
        &client,
        &customer_order_ref,
        &known_bet_ids,
        probe_result.is_err(),
        Decimal::ZERO,
    )
    .await;
    client.disconnect().await;

    cleanup_result.expect("live invalid-replace cleanup and exposure verification failed");
    let observation = probe_result.expect("live invalid-price replace request failed");
    let report = observation.report;
    assert_eq!(
        report.customer_ref.as_deref(),
        Some(observation.customer_ref.as_str())
    );
    assert_eq!(report.status, ExecutionReportStatus::Failure);
    assert_eq!(
        report.error_code,
        Some(ExecutionReportErrorCode::BetActionError)
    );
    assert_eq!(report.error_message, None);
    assert_eq!(report.market_id.as_ref(), Some(&observation.market_id));
    let reports = report
        .instruction_reports
        .expect("replace instruction reports");
    assert_eq!(reports.len(), 1);
    let instruction = &reports[0];
    assert_eq!(instruction.status, InstructionReportStatus::Failure);
    assert_eq!(
        instruction.error_code,
        Some(InstructionReportErrorCode::CancelledNotPlaced)
    );
    assert_eq!(instruction.error_message, None);
    let cancel = instruction
        .cancel_instruction_report
        .as_ref()
        .expect("replace cancel report");
    assert_eq!(cancel.status, InstructionReportStatus::Success);
    assert_eq!(cancel.error_code, None);
    assert_eq!(cancel.error_message, None);
    assert_eq!(
        cancel.instruction.as_ref().map(|value| &value.bet_id),
        Some(&observation.old_bet_id)
    );
    assert_eq!(cancel.size_cancelled, Some(observation.stake));
    assert!(cancel.cancelled_date.is_some());
    let place = instruction
        .place_instruction_report
        .as_ref()
        .expect("replace place report");
    assert_eq!(place.status, InstructionReportStatus::Failure);
    assert_eq!(
        place.error_code,
        Some(InstructionReportErrorCode::InvalidOdds)
    );
    assert_eq!(place.error_message, None);
    assert_eq!(place.order_status, None);
    let placed_instruction = place
        .instruction
        .as_ref()
        .expect("replace place instruction");
    assert_eq!(placed_instruction.order_type, BetfairOrderType::Limit);
    assert_eq!(placed_instruction.selection_id, observation.selection_id);
    assert_eq!(placed_instruction.handicap, None);
    assert_eq!(placed_instruction.side, BetfairSide::Back);
    let limit = placed_instruction
        .limit_order
        .as_ref()
        .expect("replace limit order");
    assert_eq!(limit.size, observation.stake);
    assert_eq!(limit.price, Decimal::new(257, 2));
    assert_eq!(limit.persistence_type, Some(PersistenceType::Lapse));
    assert_eq!(limit.time_in_force, None);
    assert_eq!(limit.min_fill_size, None);
    assert_eq!(limit.bet_target_type, None);
    assert_eq!(limit.bet_target_size, None);
    assert!(placed_instruction.limit_on_close_order.is_none());
    assert!(placed_instruction.market_on_close_order.is_none());
    assert_eq!(placed_instruction.customer_order_ref, None);
    assert_eq!(place.bet_id, None);
    assert_eq!(place.placed_date, None);
    assert_eq!(place.average_price_matched, None);
    assert_eq!(place.size_matched, None);
}

#[rstest]
#[case(LiveExecutionScenario::ReplaceCancel)]
#[case(LiveExecutionScenario::InvalidReplace)]
#[case(LiveExecutionScenario::ReconnectReplaceCancel)]
#[case(LiveExecutionScenario::ReplaceFill)]
#[case(LiveExecutionScenario::ReconnectReplaceDuringRecovery)]
#[tokio::test]
#[ignore = "runs a production LiveNode and mutates orders on the configured live Betfair account"]
async fn live_execution_client_replace_via_stream(#[case] scenario: LiveExecutionScenario) {
    let LiveExecutionFixture {
        credential,
        currency_code,
        stake,
        market_id,
        instrument_id,
        instrument,
    } = prepare_live_execution()
        .await
        .expect("prepare live execution fixture");

    let trader_id = TraderId::from("BETFAIR-LIVE-TESTER");
    let account_id = AccountId::from("BETFAIR-001");
    let exec_config = BetfairExecutionClientConfig {
        account_id,
        account_currency: currency_code.clone(),
        stream_market_ids_filter: Some(vec![market_id.clone()]),
        ignore_external_orders: true,
        calculate_account_state: false,
        reconcile_market_ids_only: true,
        reconcile_market_ids: Some(vec![market_id]),
        ..Default::default()
    };
    let mut node = build_live_execution_node(
        "BetfairLiveExecutionSmoke",
        trader_id,
        exec_config,
        instrument,
    )
    .expect("build live node");

    let unique = live_ref();
    let client_order_id = ClientOrderId::from(format!("L{}", &unique[..31]));
    let customer_order_ref = make_customer_order_ref(client_order_id.as_str());
    let probe = LiveExecutionProbe::default();
    node.add_strategy(LiveExecutionLifecycle::new(
        instrument_id,
        client_order_id,
        Quantity::from(stake.to_string()),
        scenario,
        probe.clone(),
    ))
    .expect("add live execution strategy");

    let handle = node.handle();
    let stop_handle = handle.clone();
    let monitor_probe = probe.clone();

    let monitor = tokio::spawn(async move {
        if tokio::time::timeout(Duration::from_secs(60), monitor_probe.wait_finished())
            .await
            .is_err()
        {
            monitor_probe.fail("live execution lifecycle timed out");
        }
        stop_handle.stop();
    });

    let run_result = tokio::time::timeout(Duration::from_secs(75), node.run()).await;
    let _ = monitor.await;

    let state = probe.snapshot();
    let known_bet_ids = state.bet_ids.clone();
    let cleanup_client =
        BetfairHttpClient::new(credential, None, None, None, None, Some(5), Some(20))
            .expect("live cleanup client");
    cleanup_client
        .connect()
        .await
        .expect("Betfair cleanup login");
    let cleanup_result = cleanup_orders(
        &cleanup_client,
        &customer_order_ref,
        &known_bet_ids,
        live_run_failed(&run_result) || state.failure.is_some(),
        if scenario.expects_fill() {
            stake
        } else {
            Decimal::ZERO
        },
    )
    .await;
    cleanup_client.disconnect().await;

    cleanup_result.expect("live execution smoke cleanup and exposure verification failed");
    let node_result = run_result.expect("live node did not stop within 75 seconds");
    node_result.expect("live node run failed");
    if let Some(failure) = state.failure.clone() {
        panic!("live execution lifecycle failed: {failure}");
    }
    assert_eq!(state.accepted, 1);
    if scenario.reconnects() {
        assert_eq!(state.socket_disconnected, 1);
        assert_eq!(state.socket_connected, 1);
    } else {
        assert_eq!(state.socket_disconnected, 0);
        assert_eq!(state.socket_connected, 0);
    }

    match scenario {
        LiveExecutionScenario::InvalidReplace => {
            assert_eq!(state.updated, 0);
            assert_eq!(state.canceled, 1);
            assert_eq!(state.filled, 0);
            assert_eq!(known_bet_ids.len(), 1);
        }
        LiveExecutionScenario::ReplaceCancel
        | LiveExecutionScenario::ReconnectReplaceCancel
        | LiveExecutionScenario::ReplaceFill
        | LiveExecutionScenario::ReconnectReplaceDuringRecovery => {
            assert_eq!(state.updated, 1);
            assert_eq!(state.canceled, usize::from(!scenario.expects_fill()),);
            assert_eq!(state.filled, usize::from(scenario.expects_fill()),);
            assert_eq!(
                known_bet_ids.len(),
                2,
                "place and replace Bet IDs must differ"
            );

            let old_bet_id = state
                .accepted_bet_id
                .clone()
                .expect("accepted event must carry the original Bet ID");
            let new_bet_id = state
                .updated_bet_id
                .clone()
                .expect("updated event must carry the replacement Bet ID");
            assert_ne!(old_bet_id, new_bet_id);

            let cache = node.kernel().cache.borrow();
            let order = cache
                .order(&client_order_id)
                .expect("live execution order must remain cached");
            let old_venue_order_id = VenueOrderId::from(old_bet_id.as_str());
            let new_venue_order_id = VenueOrderId::from(new_bet_id.as_str());
            assert_eq!(order.venue_order_id(), Some(new_venue_order_id));
            assert_eq!(
                cache.client_order_id(&old_venue_order_id),
                Some(&client_order_id),
            );
            assert_eq!(
                cache.client_order_id(&new_venue_order_id),
                Some(&client_order_id),
            );

            if scenario.expects_fill() {
                assert_eq!(order.status(), OrderStatus::Filled);
                assert_eq!(order.filled_qty(), Quantity::from(stake.to_string()));
            } else {
                assert_eq!(order.status(), OrderStatus::Canceled);
                assert_eq!(
                    order.filled_qty(),
                    Quantity::zero(order.quantity().precision),
                );
            }
        }
    }
}

#[tokio::test]
#[ignore = "runs concurrent production LiveNode order lifecycles on the configured live Betfair account"]
async fn live_execution_concurrent_replace_stress() {
    let total_orders = live_stress_setting("BETFAIR_LIVE_STRESS_ORDERS", 20, 1, 200);
    let max_active = live_stress_setting("BETFAIR_LIVE_STRESS_MAX_ACTIVE", 1, 1, 20);
    let order_rate = live_stress_setting("BETFAIR_LIVE_STRESS_ORDER_RATE", 5, 1, 20) as u32;

    run_live_execution_stress(total_orders, max_active, order_rate)
        .await
        .expect("live concurrent replacement stress failed");
}

async fn run_live_execution_stress(
    total_orders: usize,
    max_active: usize,
    order_rate: u32,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        max_active <= total_orders,
        "max active orders {max_active} exceeds total orders {total_orders}",
    );
    let LiveExecutionFixture {
        credential,
        currency_code,
        stake,
        market_id,
        instrument_id,
        instrument,
    } = prepare_live_execution().await?;

    let trader_id = TraderId::from("BETFAIR-LIVE-TESTER");
    let account_id = AccountId::from("BETFAIR-001");
    let exec_config = BetfairExecutionClientConfig {
        account_id,
        account_currency: currency_code,
        order_request_rate_per_second: order_rate,
        stream_market_ids_filter: Some(vec![market_id.clone()]),
        ignore_external_orders: true,
        calculate_account_state: false,
        reconcile_market_ids_only: true,
        reconcile_market_ids: Some(vec![market_id]),
        ..Default::default()
    };
    let mut node = build_live_execution_node(
        "BetfairLiveExecutionStress",
        trader_id,
        exec_config,
        instrument,
    )?;

    let probe = LiveStressProbe::default();
    node.add_strategy(LiveExecutionStress::new(
        instrument_id,
        Quantity::from(stake.to_string()),
        Price::from("980"),
        total_orders,
        max_active,
        probe.clone(),
    ))?;

    let handle = node.handle();
    let stop_handle = handle.clone();
    let monitor_probe = probe.clone();
    let expected_request_secs = (total_orders as u64 * 3).div_ceil(u64::from(order_rate));
    let deadline = Duration::from_secs(expected_request_secs + 90);
    let monitor = tokio::spawn(async move {
        let deadline_at = Instant::now() + deadline;
        while !monitor_probe.finished(total_orders) && Instant::now() < deadline_at {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        if !monitor_probe.finished(total_orders) {
            monitor_probe.fail(format!(
                "live stress timed out after {} seconds",
                deadline.as_secs(),
            ));
        }
        stop_handle.stop();
    });

    let node_timeout = deadline + Duration::from_secs(15);
    let run_result = tokio::time::timeout(node_timeout, node.run()).await;
    let _ = monitor.await;

    let (customer_order_refs, known_bet_ids, failure) = {
        let state = probe.state.lock();
        let customer_order_refs = state
            .orders
            .keys()
            .map(|client_order_id| make_customer_order_ref(client_order_id.as_str()))
            .collect::<Vec<_>>();
        let known_bet_ids = state
            .orders
            .values()
            .flat_map(|order| {
                [
                    order.accepted_bet_id.clone(),
                    order.updated_bet_id.clone(),
                    order.canceled_bet_id.clone(),
                ]
                .into_iter()
                .flatten()
            })
            .collect::<HashSet<_>>();
        (customer_order_refs, known_bet_ids, state.failure.clone())
    };

    let cleanup_client =
        BetfairHttpClient::new(credential, None, None, None, None, Some(5), Some(20))?;
    cleanup_client.connect().await?;
    let cleanup_result = cleanup_order_refs(
        &cleanup_client,
        &customer_order_refs,
        &known_bet_ids,
        live_run_failed(&run_result) || failure.is_some(),
        Decimal::ZERO,
    )
    .await;
    cleanup_client.disconnect().await;
    cleanup_result?;

    let node_result = run_result.context("live stress node timed out")?;
    node_result?;
    anyhow::ensure!(failure.is_none(), "{}", failure.unwrap_or_default());

    let expected_events = [
        LiveStressEvent::Submitted,
        LiveStressEvent::Accepted,
        LiveStressEvent::PendingUpdate,
        LiveStressEvent::Updated,
        LiveStressEvent::PendingCancel,
        LiveStressEvent::Canceled,
    ];
    let state = probe.state.lock();
    anyhow::ensure!(
        state.orders.len() == total_orders,
        "live stress registered {} orders, expected {total_orders}",
        state.orders.len(),
    );
    anyhow::ensure!(
        state.completed.len() == total_orders,
        "live stress completed {} orders, expected {total_orders}",
        state.completed.len(),
    );
    let cache = node.kernel().cache.borrow();

    for (client_order_id, record) in &state.orders {
        anyhow::ensure!(
            record.events == expected_events,
            "order {client_order_id} event sequence was {:?}",
            record.events,
        );
        let old_bet_id = record
            .accepted_bet_id
            .as_deref()
            .context("accepted event omitted Bet ID")?;
        let new_bet_id = record
            .updated_bet_id
            .as_deref()
            .context("updated event omitted Bet ID")?;
        let canceled_bet_id = record
            .canceled_bet_id
            .as_deref()
            .context("canceled event omitted Bet ID")?;
        anyhow::ensure!(
            old_bet_id != new_bet_id,
            "order {client_order_id} retained old Bet ID after replacement",
        );
        anyhow::ensure!(
            new_bet_id == canceled_bet_id,
            "order {client_order_id} canceled Bet ID {canceled_bet_id}, expected {new_bet_id}",
        );

        let order = cache
            .order(client_order_id)
            .context("completed live stress order missing from cache")?;
        anyhow::ensure!(
            order.status() == OrderStatus::Canceled,
            "order {client_order_id} cache status was {}",
            order.status(),
        );
        anyhow::ensure!(
            order.quantity() == Quantity::from(stake.to_string()),
            "order {client_order_id} cache quantity was {}",
            order.quantity(),
        );
        anyhow::ensure!(
            order.filled_qty() == Quantity::zero(order.quantity().precision),
            "order {client_order_id} cache filled quantity was {}",
            order.filled_qty(),
        );
        let old_venue_order_id = VenueOrderId::from(old_bet_id);
        let new_venue_order_id = VenueOrderId::from(new_bet_id);
        anyhow::ensure!(
            order.venue_order_id() == Some(new_venue_order_id),
            "order {client_order_id} current Bet ID was {:?}",
            order.venue_order_id(),
        );
        anyhow::ensure!(
            cache.client_order_id(&old_venue_order_id) == Some(client_order_id),
            "old Bet ID {old_bet_id} did not route to {client_order_id}",
        );
        anyhow::ensure!(
            cache.client_order_id(&new_venue_order_id) == Some(client_order_id),
            "new Bet ID {new_bet_id} did not route to {client_order_id}",
        );
    }

    Ok(())
}

fn live_run_failed<T, E>(result: &Result<anyhow::Result<T>, E>) -> bool {
    !matches!(result, Ok(Ok(_)))
}

fn build_live_execution_node(
    name: &str,
    trader_id: TraderId,
    exec_config: BetfairExecutionClientConfig,
    instrument: InstrumentAny,
) -> anyhow::Result<LiveNode> {
    let exec_engine_config = LiveExecutionEngineConfig {
        open_check_interval_secs: Some(5.0),
        position_check_interval_secs: None,
        ..Default::default()
    };
    let node = LiveNode::builder(trader_id, Environment::Live)?
        .with_name(name.to_string())
        .with_exec_engine_config(exec_engine_config)
        .with_risk_engine_config(LiveRiskEngineConfig {
            bypass: true,
            ..Default::default()
        })
        .add_exec_client(
            None,
            Box::new(BetfairExecutionClientFactory::new()),
            Box::new(exec_config),
        )?
        .with_reconciliation(false)
        .with_delay_post_stop_secs(2)
        .build()?;
    node.kernel()
        .cache
        .borrow_mut()
        .add_instrument(instrument)?;
    Ok(node)
}

async fn exercise_order_lifecycle(
    client: &BetfairHttpClient,
    customer_order_ref: &str,
    known_bet_ids: &mut HashSet<BetId>,
) -> anyhow::Result<()> {
    let account: AccountDetailsResponse = client
        .send_accounts(METHOD_GET_ACCOUNT_DETAILS, serde_json::json!({}))
        .await
        .context("getAccountDetails")?;
    let currency = account
        .currency_code
        .context("account details omitted currencyCode")?;
    let stake = minimum_stake(currency.as_str())?;
    let target = find_unmatched_target(client, stake).await?;

    let place_params = PlaceOrdersParams {
        market_id: target.market_id.clone(),
        instructions: vec![PlaceInstruction {
            order_type: BetfairOrderType::Limit,
            selection_id: target.selection_id,
            handicap: Some(target.handicap),
            side: BetfairSide::Back,
            limit_order: Some(LimitOrder {
                size: stake,
                price: price_passive(),
                persistence_type: Some(PersistenceType::Lapse),
                time_in_force: None,
                min_fill_size: None,
                bet_target_type: None,
                bet_target_size: None,
            }),
            limit_on_close_order: None,
            market_on_close_order: None,
            customer_order_ref: Some(customer_order_ref.to_string()),
        }],
        customer_ref: Some(live_ref()),
        market_version: None,
        customer_strategy_ref: None,
    };
    let place: PlaceExecutionReport = client
        .send_betting_order(METHOD_PLACE_ORDERS, &place_params)
        .await
        .context("placeOrders")?;
    anyhow::ensure!(
        place.status == ExecutionReportStatus::Success,
        "placeOrders returned {:?}: {:?}",
        place.status,
        place.error_code,
    );
    let place_instruction = one_place_instruction(&place)?;
    let old_bet_id = place_instruction
        .bet_id
        .clone()
        .context("successful placeOrders omitted betId")?;
    known_bet_ids.insert(old_bet_id.clone());
    anyhow::ensure!(
        place_instruction.size_matched.unwrap_or(Decimal::ZERO) == Decimal::ZERO,
        "live passive order matched during placement",
    );

    let replace_params = ReplaceOrdersParams {
        market_id: target.market_id.clone(),
        instructions: vec![ReplaceInstruction {
            bet_id: old_bet_id,
            new_price: price_replace(),
        }],
        customer_ref: Some(live_ref()),
        market_version: None,
    };
    let replace: ReplaceExecutionReport = client
        .send_betting_order(METHOD_REPLACE_ORDERS, &replace_params)
        .await
        .context("replaceOrders")?;
    anyhow::ensure!(
        replace.status == ExecutionReportStatus::Success,
        "replaceOrders returned {:?}: {:?}",
        replace.status,
        replace.error_code,
    );
    let replace_instruction = replace
        .instruction_reports
        .as_deref()
        .filter(|reports| reports.len() == 1)
        .and_then(|reports| reports.first())
        .context("replaceOrders did not return one instruction report")?;
    anyhow::ensure!(
        replace_instruction.status == InstructionReportStatus::Success,
        "replace instruction returned {:?}: {:?}",
        replace_instruction.status,
        replace_instruction.error_code,
    );
    anyhow::ensure!(
        replace_instruction
            .cancel_instruction_report
            .as_ref()
            .is_some_and(|report| report.status == InstructionReportStatus::Success),
        "replace cancel phase did not succeed",
    );
    let replacement = replace_instruction
        .place_instruction_report
        .as_ref()
        .context("replace report omitted place phase")?;
    anyhow::ensure!(
        replacement.status == InstructionReportStatus::Success,
        "replace place phase returned {:?}: {:?}",
        replacement.status,
        replacement.error_code,
    );
    let new_bet_id = replacement
        .bet_id
        .clone()
        .context("replace place phase omitted betId")?;
    known_bet_ids.insert(new_bet_id.clone());
    anyhow::ensure!(
        replacement.size_matched.unwrap_or(Decimal::ZERO) == Decimal::ZERO,
        "live passive order matched during replacement",
    );

    let cancel_params = CancelOrdersParams {
        market_id: Some(target.market_id),
        instructions: Some(vec![CancelInstruction {
            bet_id: new_bet_id,
            size_reduction: None,
        }]),
        customer_ref: Some(live_ref()),
    };
    let cancel: CancelExecutionReport = client
        .send_betting_order(METHOD_CANCEL_ORDERS, &cancel_params)
        .await
        .context("cancelOrders")?;
    anyhow::ensure!(
        cancel.status == ExecutionReportStatus::Success,
        "cancelOrders returned {:?}: {:?}",
        cancel.status,
        cancel.error_code,
    );
    let cancel_instruction = cancel
        .instruction_reports
        .as_deref()
        .filter(|reports| reports.len() == 1)
        .and_then(|reports| reports.first())
        .context("cancelOrders did not return one instruction report")?;
    anyhow::ensure!(
        cancel_instruction.status == InstructionReportStatus::Success
            || cancel_instruction.error_code == Some(InstructionReportErrorCode::BetTakenOrLapsed),
        "cancel instruction returned {:?}: {:?}",
        cancel_instruction.status,
        cancel_instruction.error_code,
    );

    Ok(())
}

async fn exercise_invalid_replace(
    client: &BetfairHttpClient,
    customer_order_ref: &str,
    known_bet_ids: &mut HashSet<BetId>,
) -> anyhow::Result<InvalidReplaceObservation> {
    let account: AccountDetailsResponse = client
        .send_accounts(METHOD_GET_ACCOUNT_DETAILS, serde_json::json!({}))
        .await
        .context("getAccountDetails")?;
    let currency = account
        .currency_code
        .context("account details omitted currencyCode")?;
    let stake = minimum_stake(currency.as_str())?;
    let target = find_unmatched_target(client, stake).await?;

    let place_params = PlaceOrdersParams {
        market_id: target.market_id.clone(),
        instructions: vec![PlaceInstruction {
            order_type: BetfairOrderType::Limit,
            selection_id: target.selection_id,
            handicap: Some(target.handicap),
            side: BetfairSide::Back,
            limit_order: Some(LimitOrder {
                size: stake,
                price: price_passive(),
                persistence_type: Some(PersistenceType::Lapse),
                time_in_force: None,
                min_fill_size: None,
                bet_target_type: None,
                bet_target_size: None,
            }),
            limit_on_close_order: None,
            market_on_close_order: None,
            customer_order_ref: Some(customer_order_ref.to_string()),
        }],
        customer_ref: Some(live_ref()),
        market_version: None,
        customer_strategy_ref: None,
    };
    let place: PlaceExecutionReport = client
        .send_betting_order(METHOD_PLACE_ORDERS, &place_params)
        .await
        .context("placeOrders")?;
    let old_bet_id = one_place_instruction(&place)?
        .bet_id
        .clone()
        .context("successful placeOrders omitted betId")?;
    known_bet_ids.insert(old_bet_id.clone());

    let customer_ref = live_ref();
    let replace_params = ReplaceOrdersParams {
        market_id: target.market_id.clone(),
        instructions: vec![ReplaceInstruction {
            bet_id: old_bet_id.clone(),
            new_price: Decimal::new(257, 2),
        }],
        customer_ref: Some(customer_ref.clone()),
        market_version: None,
    };
    let report = client
        .send_betting_order(METHOD_REPLACE_ORDERS, &replace_params)
        .await
        .context("replaceOrders")?;

    Ok(InvalidReplaceObservation {
        report,
        market_id: target.market_id,
        selection_id: target.selection_id,
        old_bet_id,
        stake,
        customer_ref,
    })
}

fn one_place_instruction(report: &PlaceExecutionReport) -> anyhow::Result<&PlaceInstructionReport> {
    let instruction = report
        .instruction_reports
        .as_deref()
        .filter(|reports| reports.len() == 1)
        .and_then(|reports| reports.first())
        .context("placeOrders did not return one instruction report")?;
    anyhow::ensure!(
        instruction.status == InstructionReportStatus::Success,
        "place instruction returned {:?}: {:?}",
        instruction.status,
        instruction.error_code,
    );
    Ok(instruction)
}

async fn prepare_live_execution() -> anyhow::Result<LiveExecutionFixture> {
    let credential = BetfairCredential::from_env()
        .context("BETFAIR_USERNAME, BETFAIR_PASSWORD, and BETFAIR_APP_KEY must be set")?;
    let discovery = Arc::new(BetfairHttpClient::new(
        credential.clone(),
        None,
        None,
        None,
        None,
        Some(5),
        Some(20),
    )?);
    discovery
        .connect()
        .await
        .context("Betfair discovery login")?;

    let result = async {
        let account: AccountDetailsResponse = discovery
            .send_accounts(METHOD_GET_ACCOUNT_DETAILS, serde_json::json!({}))
            .await
            .context("getAccountDetails")?;
        let currency_code = account
            .currency_code
            .context("account details omitted currencyCode")?;
        let currency = currency_code
            .parse::<Currency>()
            .context("account currency must be supported")?;
        let stake = minimum_stake(currency_code.as_str())?;
        let target = find_unmatched_target(discovery.as_ref(), stake).await?;
        let market_id = target.market_id;
        let instrument_id =
            make_instrument_id(market_id.as_str(), target.selection_id, target.handicap);
        let mut provider = BetfairInstrumentProvider::new(
            Arc::clone(&discovery),
            NavigationFilter {
                market_ids: Some(vec![market_id.clone()]),
                ..Default::default()
            },
            currency,
            None,
        );
        provider
            .load_all(None)
            .await
            .context("load live Betfair instruments")?;
        let instrument = provider
            .store()
            .list_all()
            .into_iter()
            .find(|instrument| instrument.id() == instrument_id)
            .cloned()
            .context("selected live instrument must be loaded")?;

        Ok(LiveExecutionFixture {
            credential,
            currency_code: currency_code.to_string(),
            stake,
            market_id,
            instrument_id,
            instrument,
        })
    }
    .await;

    discovery.disconnect().await;
    result
}

async fn find_unmatched_target(
    client: &BetfairHttpClient,
    minimum_liquidity: Decimal,
) -> anyhow::Result<LiveTarget> {
    let params = ListMarketCatalogueParams {
        filter: MarketFilter {
            in_play_only: Some(false),
            market_type_codes: Some(vec![Ustr::from("MATCH_ODDS")]),
            turn_in_play_enabled: Some(true),
            ..Default::default()
        },
        market_projection: Some(vec![
            MarketProjection::MarketStartTime,
            MarketProjection::RunnerDescription,
        ]),
        sort: Some(MarketSort::MaximumTraded),
        max_results: Some(20),
        locale: None,
    };
    let catalogues: Vec<MarketCatalogue> = client
        .send_betting(METHOD_LIST_MARKET_CATALOGUE, &params)
        .await
        .context("listMarketCatalogue")?;
    anyhow::ensure!(!catalogues.is_empty(), "no candidate markets available");

    let market_ids: Vec<_> = catalogues
        .into_iter()
        .map(|catalogue| catalogue.market_id)
        .collect();
    let books: Vec<LiveMarketBook> = client
        .send_betting(
            "SportsAPING/v1.0/listMarketBook",
            serde_json::json!({
                "marketIds": market_ids,
                "priceProjection": {"priceData": ["EX_BEST_OFFERS"]},
            }),
        )
        .await
        .context("listMarketBook")?;

    books
        .into_iter()
        .filter(|book| book.status == MarketStatus::Open && !book.inplay)
        .find_map(|book| {
            book.runners
                .into_iter()
                .filter(|runner| runner.status == RunnerStatus::Active)
                .find(|runner| {
                    runner.ex.available_to_back.first().is_some_and(|price| {
                        price.price < price_replace() && price.size >= minimum_liquidity
                    })
                })
                .map(|runner| LiveTarget {
                    market_id: book.market_id,
                    selection_id: runner.selection_id,
                    handicap: runner.handicap,
                })
        })
        .context("no open non-in-play runner has a safely separated liquid back price")
}

async fn cleanup_orders(
    client: &BetfairHttpClient,
    customer_order_ref: &str,
    known_bet_ids: &HashSet<BetId>,
    await_unknown_order: bool,
    expected_matched: Decimal,
) -> anyhow::Result<()> {
    cleanup_order_refs(
        client,
        &[customer_order_ref.to_string()],
        known_bet_ids,
        await_unknown_order,
        expected_matched,
    )
    .await
}

async fn cleanup_order_refs(
    client: &BetfairHttpClient,
    customer_order_refs: &[String],
    known_bet_ids: &HashSet<BetId>,
    await_unknown_order: bool,
    expected_matched: Decimal,
) -> anyhow::Result<()> {
    let mut last_error = None;
    let mut observed_matched = Decimal::ZERO;
    let mut queried_known_bet_ids = false;
    let wait = if await_unknown_order || expected_matched != Decimal::ZERO {
        Duration::from_secs(15)
    } else {
        Duration::from_secs(2)
    };
    let deadline = Instant::now() + wait;

    loop {
        let orders =
            current_orders_for_refs(client, customer_order_refs, OrderProjection::All).await;
        let executable =
            match orders {
                Ok(orders) => {
                    observed_matched = observed_matched.max(
                        orders
                            .iter()
                            .map(|order| order.size_matched.unwrap_or(Decimal::ZERO))
                            .sum(),
                    );
                    orders
                        .into_iter()
                        .filter(|order| order.status == BetfairOrderStatus::Executable)
                        .collect()
                }
                Err(e) => {
                    last_error = Some(e);

                    if queried_known_bet_ids {
                        Vec::new()
                    } else {
                        queried_known_bet_ids = true;
                        let mut orders = Vec::new();

                        for bet_id in known_bet_ids {
                            match current_orders_for_bet_id(client, bet_id).await {
                                Ok(found) => orders.extend(found.into_iter().filter(|order| {
                                    order.status == BetfairOrderStatus::Executable
                                })),
                                Err(e) => last_error = Some(e),
                            }
                        }
                        orders
                    }
                }
            };

        for order in executable {
            if let Err(e) = cancel_live_order(client, &order).await {
                last_error = Some(e);
            }
        }

        if Instant::now() >= deadline {
            break;
        }

        tokio::time::sleep(Duration::from_secs(1)).await;
    }

    let orders =
        match current_orders_for_refs(client, customer_order_refs, OrderProjection::All).await {
            Ok(orders) => orders,
            Err(e) => return Err(last_error.unwrap_or(e)),
        };
    observed_matched = observed_matched.max(
        orders
            .iter()
            .map(|order| order.size_matched.unwrap_or(Decimal::ZERO))
            .sum(),
    );
    let executable: Vec<_> = orders
        .iter()
        .filter(|order| order.status == BetfairOrderStatus::Executable)
        .map(|order| &order.bet_id)
        .collect();
    anyhow::ensure!(
        executable.is_empty(),
        "task-created executable orders remain after cleanup: {executable:?}; last cleanup error: {last_error:?}",
    );
    anyhow::ensure!(
        observed_matched == expected_matched,
        "task-created live orders matched {observed_matched}, expected {expected_matched}",
    );
    Ok(())
}

async fn cancel_live_order(
    client: &BetfairHttpClient,
    order: &CurrentOrderSummary,
) -> anyhow::Result<()> {
    let params = CancelOrdersParams {
        market_id: Some(order.market_id.clone()),
        instructions: Some(vec![CancelInstruction {
            bet_id: order.bet_id.clone(),
            size_reduction: None,
        }]),
        customer_ref: Some(live_ref()),
    };
    let report: CancelExecutionReport = client
        .send_betting_order(METHOD_CANCEL_ORDERS, &params)
        .await
        .context("cancelOrders during cleanup")?;
    let instruction = report
        .instruction_reports
        .as_deref()
        .filter(|reports| reports.len() == 1)
        .and_then(|reports| reports.first())
        .context("cleanup cancelOrders did not return one instruction report")?;
    anyhow::ensure!(
        instruction.status == InstructionReportStatus::Success
            || instruction.error_code == Some(InstructionReportErrorCode::BetTakenOrLapsed),
        "cleanup cancel instruction returned {:?}: {:?}",
        instruction.status,
        instruction.error_code,
    );
    Ok(())
}

async fn current_orders_for_refs(
    client: &BetfairHttpClient,
    customer_order_refs: &[String],
    projection: OrderProjection,
) -> anyhow::Result<Vec<CurrentOrderSummary>> {
    let mut orders = Vec::new();

    for customer_order_ref in customer_order_refs {
        let report = current_orders(client, customer_order_ref, projection).await?;
        orders.extend(report.current_orders);
    }

    Ok(orders)
}

async fn current_orders_for_bet_id(
    client: &BetfairHttpClient,
    bet_id: &BetId,
) -> anyhow::Result<Vec<CurrentOrderSummary>> {
    let params = ListCurrentOrdersParams {
        bet_ids: Some(vec![bet_id.clone()]),
        market_ids: None,
        order_projection: Some(OrderProjection::All),
        customer_order_refs: None,
        customer_strategy_refs: None,
        date_range: None,
        order_by: None,
        sort_dir: None,
        from_record: None,
        record_count: Some(100),
    };
    let report: CurrentOrderSummaryReport = client
        .send_betting(METHOD_LIST_CURRENT_ORDERS, &params)
        .await
        .context("listCurrentOrders by Bet ID")?;
    Ok(report.current_orders)
}

async fn current_orders(
    client: &BetfairHttpClient,
    customer_order_ref: &str,
    projection: OrderProjection,
) -> anyhow::Result<CurrentOrderSummaryReport> {
    let params = ListCurrentOrdersParams {
        bet_ids: None,
        market_ids: None,
        order_projection: Some(projection),
        customer_order_refs: Some(vec![customer_order_ref.to_string()]),
        customer_strategy_refs: None,
        date_range: None,
        order_by: None,
        sort_dir: None,
        from_record: None,
        record_count: Some(100),
    };
    client
        .send_betting(METHOD_LIST_CURRENT_ORDERS, &params)
        .await
        .context("listCurrentOrders")
}

fn minimum_stake(currency: &str) -> anyhow::Result<Decimal> {
    let units = match currency {
        "GBP" => 1,
        "EUR" | "NZD" => 2,
        "USD" => 3,
        "AUD" => 5,
        "CAD" | "SGD" => 6,
        "BRL" | "GEL" | "PEN" | "RON" => 10,
        "HKD" => 25,
        "DKK" | "NOK" | "SEK" => 30,
        "MXN" => 60,
        "ARS" => 100,
        "ISK" => 350,
        "HUF" => 800,
        other => anyhow::bail!("unsupported live-test account currency {other}"),
    };
    Ok(Decimal::from(units))
}

fn live_stress_setting(name: &str, default: usize, minimum: usize, maximum: usize) -> usize {
    let value = std::env::var(name).map_or(default, |value| {
        value
            .parse::<usize>()
            .unwrap_or_else(|e| panic!("invalid {name} value {value:?}: {e}"))
    });
    assert!(
        (minimum..=maximum).contains(&value),
        "{name} must be in {minimum}..={maximum}, was {value}",
    );
    value
}

fn live_ref() -> String {
    UUID4::new().to_string().replace('-', "")
}

fn price_passive() -> Decimal {
    Decimal::from(990)
}

fn price_replace() -> Decimal {
    Decimal::from(980)
}
