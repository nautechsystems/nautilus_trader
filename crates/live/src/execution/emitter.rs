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

//! Live execution event emitter for async event dispatch.
//!
//! This module provides [`ExecutionEventEmitter`], which combines event generation (via
//! [`OrderEventFactory`]) with async dispatch. Adapters use the `emit_*` convenience
//! methods to generate and send events in a single call.
//!
//! # Architecture
//!
//! ```text
//! Adapter
//! |-- core: ExecutionClientCore    (identity + connection state)
//! `-- emitter: ExecutionEventEmitter   (event generation + async dispatch)
//!     |-- factory: OrderEventFactory
//!     `-- target: ArcSwapOption<Legacy | Sourced>   (set in start())
//! ```

use std::sync::Arc;

use arc_swap::ArcSwapOption;
use nautilus_common::{
    factories::OrderEventFactory,
    messages::{ExecutionEvent, ExecutionReport},
};
use nautilus_core::{Params, UUID4, UnixNanos, time::AtomicTime};
use nautilus_model::{
    enums::{AccountType, LiquiditySide},
    events::{
        AccountState, OrderAcceptedBatch, OrderCancelRejected, OrderCanceledBatch, OrderEventAny,
        OrderModifyRejected, OrderRejected, OrderSubmittedBatch,
    },
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId, TraderId,
        VenueOrderId,
    },
    orders::OrderAny,
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};

use crate::runner::SourcedExecutionEventSink;

#[derive(Debug, Clone)]
enum ExecutionEventTarget {
    Legacy(tokio::sync::mpsc::UnboundedSender<ExecutionEvent>),
    Sourced(SourcedExecutionEventSink),
}

impl ExecutionEventTarget {
    fn send(
        &self,
        event: ExecutionEvent,
    ) -> Result<(), Box<tokio::sync::mpsc::error::SendError<ExecutionEvent>>> {
        match self {
            Self::Legacy(sender) => sender.send(event).map_err(Box::new),
            Self::Sourced(sink) => sink.send(event),
        }
    }

    fn send_bootstrap_account(
        &self,
        state: AccountState,
    ) -> Result<(), Box<tokio::sync::mpsc::error::SendError<ExecutionEvent>>> {
        match self {
            Self::Legacy(sender) => sender
                .send(ExecutionEvent::Account(state))
                .map_err(Box::new),
            Self::Sourced(sink) => sink.send_bootstrap_account(state),
        }
    }
}

/// Event emitter for live trading - combines event generation with async dispatch.
///
/// This struct wraps an [`OrderEventFactory`] for event construction and an unbounded
/// dispatch target. It provides `emit_*` convenience methods that
/// generate and send events in a single call.
///
/// The dispatch target is set during the adapter's `start()` phase via
/// [`set_sender`](Self::set_sender) or [`set_sourced_sink`](Self::set_sourced_sink).
/// Clones share the target slot and observe later target installations and replacements.
#[derive(Debug, Clone)]
pub struct ExecutionEventEmitter {
    clock: &'static AtomicTime,
    factory: OrderEventFactory,
    target: Arc<ArcSwapOption<ExecutionEventTarget>>,
}

impl ExecutionEventEmitter {
    /// Creates a new [`ExecutionEventEmitter`] with no sender.
    ///
    /// Call [`set_sender`](Self::set_sender) or [`set_sourced_sink`](Self::set_sourced_sink) in the
    /// adapter's `start()` method.
    #[must_use]
    pub fn new(
        clock: &'static AtomicTime,
        trader_id: TraderId,
        account_id: AccountId,
        account_type: AccountType,
        base_currency: Option<Currency>,
    ) -> Self {
        Self {
            clock,
            factory: OrderEventFactory::new(trader_id, account_id, account_type, base_currency),
            target: Arc::new(ArcSwapOption::empty()),
        }
    }

    fn ts_init(&self) -> UnixNanos {
        self.clock.get_time_ns()
    }

    /// Installs or replaces the sender for this emitter and all its clones.
    ///
    /// Call in the adapter's `start()` method.
    pub fn set_sender(&mut self, sender: tokio::sync::mpsc::UnboundedSender<ExecutionEvent>) {
        self.target
            .store(Some(Arc::new(ExecutionEventTarget::Legacy(sender))));
    }

    /// Sets a source-bound sink. Call in an opted-in adapter's `start()`.
    pub fn set_sourced_sink(&mut self, sink: SourcedExecutionEventSink) {
        self.target
            .store(Some(Arc::new(ExecutionEventTarget::Sourced(sink))));
    }

    /// Returns true if the sender is initialized for this emitter and its clones.
    #[must_use]
    pub fn is_initialized(&self) -> bool {
        self.target.load().is_some()
    }

    /// Returns the trader ID.
    #[must_use]
    pub fn trader_id(&self) -> TraderId {
        self.factory.trader_id()
    }

    /// Returns the account ID.
    #[must_use]
    pub fn account_id(&self) -> AccountId {
        self.factory.account_id()
    }

    /// Sets the account ID for generated events.
    pub fn set_account_id(&mut self, account_id: AccountId) {
        self.factory.set_account_id(account_id);
    }

    /// Generates and emits an account state event.
    pub fn emit_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
        info: Option<Params>,
    ) {
        if let Err(e) = self.try_emit_account_state(balances, margins, reported, ts_event, info) {
            log::warn!("{e}");
        }
    }

    /// Generates and emits an account state event, reporting dispatch failures.
    ///
    /// # Errors
    ///
    /// Returns an error if the sender is uninitialized or its receiver is closed.
    pub fn try_emit_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
        info: Option<Params>,
    ) -> anyhow::Result<()> {
        let state = self.factory.generate_account_state(
            balances,
            margins,
            reported,
            ts_event,
            self.ts_init(),
            info,
        );
        self.try_send_account_state(state)
    }

    /// Generates and emits an order denied event.
    pub fn emit_order_denied(&self, order: &OrderAny, reason: &str) {
        let event = self
            .factory
            .generate_order_denied(order, reason, self.ts_init());
        self.send_order_event(event);
    }

    /// Generates and emits an order submitted event.
    pub fn emit_order_submitted(&self, order: &OrderAny) {
        let event = self.factory.generate_order_submitted(order, self.ts_init());
        self.send_order_event(event);
    }

    /// Generates and emits an order rejected event.
    pub fn emit_order_rejected(
        &self,
        order: &OrderAny,
        reason: &str,
        ts_event: UnixNanos,
        due_post_only: bool,
    ) {
        let event = self.factory.generate_order_rejected(
            order,
            reason,
            ts_event,
            self.ts_init(),
            due_post_only,
        );
        self.send_order_event(event);
    }

    /// Generates and emits an order accepted event.
    pub fn emit_order_accepted(
        &self,
        order: &OrderAny,
        venue_order_id: VenueOrderId,
        ts_event: UnixNanos,
    ) {
        let event =
            self.factory
                .generate_order_accepted(order, venue_order_id, ts_event, self.ts_init());
        self.send_order_event(event);
    }

    /// Generates and emits an order modify rejected event.
    pub fn emit_order_modify_rejected(
        &self,
        order: &OrderAny,
        venue_order_id: Option<VenueOrderId>,
        reason: &str,
        ts_event: UnixNanos,
    ) {
        let event = self.factory.generate_order_modify_rejected(
            order,
            venue_order_id,
            reason,
            ts_event,
            self.ts_init(),
        );
        self.send_order_event(event);
    }

    /// Generates and emits an order cancel rejected event.
    pub fn emit_order_cancel_rejected(
        &self,
        order: &OrderAny,
        venue_order_id: Option<VenueOrderId>,
        reason: &str,
        ts_event: UnixNanos,
    ) {
        let event = self.factory.generate_order_cancel_rejected(
            order,
            venue_order_id,
            reason,
            ts_event,
            self.ts_init(),
        );
        self.send_order_event(event);
    }

    /// Generates and emits an order updated event.
    #[expect(clippy::too_many_arguments)]
    pub fn emit_order_updated(
        &self,
        order: &OrderAny,
        venue_order_id: VenueOrderId,
        quantity: Quantity,
        price: Option<Price>,
        trigger_price: Option<Price>,
        protection_price: Option<Price>,
        ts_event: UnixNanos,
    ) {
        let event = self.factory.generate_order_updated(
            order,
            venue_order_id,
            quantity,
            price,
            trigger_price,
            protection_price,
            ts_event,
            self.ts_init(),
        );
        self.send_order_event(event);
    }

    /// Generates and emits an order canceled event.
    pub fn emit_order_canceled(
        &self,
        order: &OrderAny,
        venue_order_id: Option<VenueOrderId>,
        ts_event: UnixNanos,
    ) {
        let event =
            self.factory
                .generate_order_canceled(order, venue_order_id, ts_event, self.ts_init());
        self.send_order_event(event);
    }

    /// Generates and emits an order triggered event.
    pub fn emit_order_triggered(
        &self,
        order: &OrderAny,
        venue_order_id: Option<VenueOrderId>,
        ts_event: UnixNanos,
    ) {
        let event =
            self.factory
                .generate_order_triggered(order, venue_order_id, ts_event, self.ts_init());
        self.send_order_event(event);
    }

    /// Generates and emits an order expired event.
    pub fn emit_order_expired(
        &self,
        order: &OrderAny,
        venue_order_id: Option<VenueOrderId>,
        ts_event: UnixNanos,
    ) {
        let event =
            self.factory
                .generate_order_expired(order, venue_order_id, ts_event, self.ts_init());
        self.send_order_event(event);
    }

    /// Generates and emits an order filled event.
    #[expect(clippy::too_many_arguments)]
    pub fn emit_order_filled(
        &self,
        order: &OrderAny,
        venue_order_id: VenueOrderId,
        venue_position_id: Option<PositionId>,
        trade_id: TradeId,
        last_qty: Quantity,
        last_px: Price,
        quote_currency: Currency,
        commission: Option<Money>,
        liquidity_side: LiquiditySide,
        ts_event: UnixNanos,
    ) {
        let event = self.factory.generate_order_filled(
            order,
            venue_order_id,
            venue_position_id,
            trade_id,
            last_qty,
            last_px,
            quote_currency,
            commission,
            liquidity_side,
            ts_event,
            self.ts_init(),
        );
        self.send_order_event(event);
    }

    /// Constructs and emits an order rejected event from raw fields.
    pub fn emit_order_rejected_event(
        &self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        reason: &str,
        ts_event: UnixNanos,
        due_post_only: bool,
    ) {
        let event = OrderRejected::new(
            self.factory.trader_id(),
            strategy_id,
            instrument_id,
            client_order_id,
            self.factory.account_id(),
            reason.into(),
            UUID4::new(),
            ts_event,
            self.ts_init(),
            false,
            due_post_only,
        );
        self.send_order_event(OrderEventAny::Rejected(event));
    }

    /// Constructs and emits an order modify rejected event from raw fields.
    pub fn emit_order_modify_rejected_event(
        &self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        reason: &str,
        ts_event: UnixNanos,
    ) {
        let event = OrderModifyRejected::new(
            self.factory.trader_id(),
            strategy_id,
            instrument_id,
            client_order_id,
            reason.into(),
            UUID4::new(),
            ts_event,
            self.ts_init(),
            false,
            venue_order_id,
            Some(self.factory.account_id()),
        );
        self.send_order_event(OrderEventAny::ModifyRejected(event));
    }

    /// Constructs and emits an order cancel rejected event from raw fields.
    pub fn emit_order_cancel_rejected_event(
        &self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        reason: &str,
        ts_event: UnixNanos,
    ) {
        let event = OrderCancelRejected::new(
            self.factory.trader_id(),
            strategy_id,
            instrument_id,
            client_order_id,
            reason.into(),
            UUID4::new(),
            ts_event,
            self.ts_init(),
            false,
            venue_order_id,
            Some(self.factory.account_id()),
        );
        self.send_order_event(OrderEventAny::CancelRejected(event));
    }

    /// Emits an order event.
    pub fn send_order_event(&self, event: OrderEventAny) {
        if let Err(e) = self.try_send_order_event(event) {
            log::warn!("{e}");
        }
    }

    /// Emits an order event and returns any channel error to the caller.
    ///
    /// # Errors
    ///
    /// Returns an error if the sender is uninitialized or its receiver is closed.
    pub fn try_send_order_event(&self, event: OrderEventAny) -> anyhow::Result<()> {
        let target = self.target.load();
        let target = target
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Cannot send order event: sender not initialized"))?;
        target
            .send(ExecutionEvent::Order(event))
            .map_err(|e| anyhow::anyhow!("Failed to send order event: {e}"))
    }

    /// Emits a batch of order submitted events as a single channel message.
    pub fn send_order_submitted_batch(&self, batch: OrderSubmittedBatch) {
        let target = self.target.load();
        if let Some(target) = target.as_ref() {
            if let Err(e) = target.send(ExecutionEvent::OrderSubmittedBatch(batch)) {
                log::warn!("Failed to send order submitted batch: {e}");
            }
        } else {
            log::warn!("Cannot send order submitted batch: sender not initialized");
        }
    }

    /// Emits a batch of order accepted events as a single channel message.
    pub fn send_order_accepted_batch(&self, batch: OrderAcceptedBatch) {
        let target = self.target.load();
        if let Some(target) = target.as_ref() {
            if let Err(e) = target.send(ExecutionEvent::OrderAcceptedBatch(batch)) {
                log::warn!("Failed to send order accepted batch: {e}");
            }
        } else {
            log::warn!("Cannot send order accepted batch: sender not initialized");
        }
    }

    /// Emits a batch of order canceled events as a single channel message.
    pub fn send_order_canceled_batch(&self, batch: OrderCanceledBatch) {
        let target = self.target.load();
        if let Some(target) = target.as_ref() {
            if let Err(e) = target.send(ExecutionEvent::OrderCanceledBatch(batch)) {
                log::warn!("Failed to send order canceled batch: {e}");
            }
        } else {
            log::warn!("Cannot send order canceled batch: sender not initialized");
        }
    }

    /// Emits an account state event.
    pub fn send_account_state(&self, state: AccountState) {
        if let Err(e) = self.try_send_account_state(state) {
            log::warn!("{e}");
        }
    }

    fn try_send_account_state(&self, state: AccountState) -> anyhow::Result<()> {
        let target = self.target.load();
        let target = target
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Cannot send account state: sender not initialized"))?;
        target
            .send(ExecutionEvent::Account(state))
            .map_err(|e| anyhow::anyhow!("Failed to send account state: {e}"))
    }

    /// Emits the source-bound account barrier required while an execution client connects.
    ///
    /// Legacy targets receive an ordinary account event. Sourced targets retain the account's
    /// bootstrap purpose until the live node applies the barrier.
    ///
    /// # Errors
    ///
    /// Returns an error if the sender is uninitialized or its receiver is closed.
    #[doc(hidden)]
    pub fn try_send_bootstrap_account_state(&self, state: AccountState) -> anyhow::Result<()> {
        let target = self.target.load();
        let target = target.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Cannot send bootstrap account state: sender not initialized")
        })?;
        target
            .send_bootstrap_account(state)
            .map_err(|e| anyhow::anyhow!("Failed to send bootstrap account state: {e}"))
    }

    /// Emits an execution report.
    pub fn send_execution_report(&self, report: ExecutionReport) {
        if let Err(e) = self.try_send_execution_report(report) {
            log::warn!("{e}");
        }
    }

    /// Emits an execution report and returns any channel error to the caller.
    ///
    /// # Errors
    ///
    /// Returns an error if the sender is not initialized or the receiving channel is closed.
    pub fn try_send_execution_report(&self, report: ExecutionReport) -> anyhow::Result<()> {
        let target = self.target.load();
        let target = target.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Cannot send execution report: sender not initialized")
        })?;
        target
            .send(ExecutionEvent::Report(report))
            .map_err(|e| anyhow::anyhow!("Failed to send execution report: {e}"))
    }

    /// Emits an order status report.
    pub fn send_order_status_report(&self, report: OrderStatusReport) {
        self.send_execution_report(ExecutionReport::Order(Box::new(report)));
    }

    /// Emits a fill report.
    pub fn send_fill_report(&self, report: FillReport) {
        self.send_execution_report(ExecutionReport::Fill(Box::new(report)));
    }

    /// Emits an order status report bundled with the fills that produced it.
    pub fn send_order_with_fills(&self, report: OrderStatusReport, fills: Vec<FillReport>) {
        self.send_execution_report(ExecutionReport::OrderWithFills(Box::new(report), fills));
    }

    /// Emits a position status report.
    pub fn send_position_report(&self, report: PositionStatusReport) {
        self.send_execution_report(ExecutionReport::Position(Box::new(report)));
    }
}

#[cfg(test)]
mod tests {
    use nautilus_common::live::runner::get_exec_event_sender;
    use nautilus_core::time::get_atomic_clock_static;
    use nautilus_model::{
        enums::PositionSide, events::order::spec::OrderSubmittedSpec, identifiers::ClientId,
    };
    use rstest::rstest;

    use super::*;
    use crate::runner::{AsyncRunner, ExecutionEventIngress, get_sourced_exec_event_sink};

    fn test_emitter() -> ExecutionEventEmitter {
        ExecutionEventEmitter::new(
            get_atomic_clock_static(),
            TraderId::from("TESTER-001"),
            AccountId::from("BYBIT-001"),
            AccountType::Margin,
            None,
        )
    }

    fn test_order_event() -> OrderEventAny {
        OrderEventAny::Submitted(
            OrderSubmittedSpec::builder()
                .client_order_id(ClientOrderId::from("O-001"))
                .build(),
        )
    }

    fn test_position_report() -> PositionStatusReport {
        PositionStatusReport::new(
            AccountId::from("BYBIT-001"),
            InstrumentId::from("BTCUSDT-LINEAR.BYBIT"),
            PositionSide::Long,
            Quantity::from(1),
            UnixNanos::from(1),
            UnixNanos::from(2),
            None,
            Some(PositionId::from("P-BYBIT-001")),
            None,
        )
    }

    fn test_account_state(event_id: UUID4) -> AccountState {
        AccountState::new(
            AccountId::from("BYBIT-001"),
            AccountType::Margin,
            vec![],
            vec![],
            true,
            event_id,
            UnixNanos::from(1),
            UnixNanos::from(2),
            None,
        )
    }

    #[rstest]
    fn test_clone_before_set_sender_observes_sender() {
        let mut emitter = test_emitter();
        let cloned = emitter.clone();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        emitter.set_sender(tx);

        assert!(cloned.is_initialized());
        cloned.send_order_event(test_order_event());
        assert!(matches!(
            rx.try_recv(),
            Ok(ExecutionEvent::Order(OrderEventAny::Submitted(_)))
        ));
    }

    #[rstest]
    fn test_clone_before_set_sourced_sink_observes_sink() {
        let runner = AsyncRunner::new();
        runner.bind_senders();
        let source_client_id = ClientId::from("BYBIT");
        let mut emitter = test_emitter();
        let cloned = emitter.clone();

        emitter.set_sourced_sink(get_sourced_exec_event_sink(source_client_id));

        assert!(cloned.is_initialized());
        cloned.send_position_report(test_position_report());
        let (mut channels, mut sourced_rx) = runner.take_channels_with_sourced();
        assert!(channels.exec_evt_rx.try_recv().is_err());
        let Some(ExecutionEventIngress::Sourced(sourced)) =
            sourced_rx.try_recv(&mut channels.exec_evt_rx)
        else {
            panic!("expected sourced execution event");
        };
        let (client_id, event) = sourced.into_parts();
        assert_eq!(client_id, source_client_id);
        assert!(matches!(
            event,
            ExecutionEvent::Report(ExecutionReport::Position(_))
        ));
    }

    #[rstest]
    fn test_sourced_target_replaces_legacy_target() {
        let runner = AsyncRunner::new();
        runner.bind_senders();
        let source_client_id = ClientId::from("BYBIT");
        let mut emitter = test_emitter();
        emitter.set_sender(get_exec_event_sender());
        emitter.set_sourced_sink(get_sourced_exec_event_sink(source_client_id));

        emitter.send_position_report(test_position_report());

        let (mut channels, mut sourced_rx) = runner.take_channels_with_sourced();
        assert!(channels.exec_evt_rx.try_recv().is_err());
        let Some(ExecutionEventIngress::Sourced(sourced)) =
            sourced_rx.try_recv(&mut channels.exec_evt_rx)
        else {
            panic!("expected sourced execution event");
        };
        let (client_id, event) = sourced.into_parts();
        assert_eq!(client_id, source_client_id);
        assert!(matches!(
            event,
            ExecutionEvent::Report(ExecutionReport::Position(_))
        ));
    }

    #[rstest]
    fn test_set_sender_replaces_existing_sender() {
        let mut emitter = test_emitter();
        let (tx_a, mut rx_a) = tokio::sync::mpsc::unbounded_channel();
        let (tx_b, mut rx_b) = tokio::sync::mpsc::unbounded_channel();

        emitter.set_sender(tx_a);
        emitter.set_sender(tx_b);
        emitter.send_order_event(test_order_event());

        assert!(matches!(
            rx_a.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected)
        ));
        assert!(matches!(
            rx_b.try_recv(),
            Ok(ExecutionEvent::Order(OrderEventAny::Submitted(_)))
        ));
    }

    #[rstest]
    fn test_never_initialized_sender_drops_or_errors() {
        let emitter = test_emitter();

        emitter.send_order_event(test_order_event());
        let error = emitter
            .try_send_order_event(test_order_event())
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Cannot send order event: sender not initialized"
        );
    }

    #[rstest]
    fn test_closed_sourced_target_returns_report_error_without_legacy_fallback() {
        let runner = AsyncRunner::new();
        runner.bind_senders();
        let mut emitter = test_emitter();
        emitter.set_sender(get_exec_event_sender());
        emitter.set_sourced_sink(get_sourced_exec_event_sink(ClientId::from("BYBIT")));
        let (mut channels, sourced_rx) = runner.take_channels_with_sourced();
        drop(sourced_rx);

        let result = emitter
            .try_send_execution_report(ExecutionReport::Position(Box::new(test_position_report())));

        assert!(result.is_err());
        assert!(channels.exec_evt_rx.try_recv().is_err());
    }

    #[rstest]
    fn test_closed_sourced_target_returns_order_error_without_legacy_fallback() {
        let runner = AsyncRunner::new();
        runner.bind_senders();
        let mut emitter = test_emitter();
        emitter.set_sender(get_exec_event_sender());
        emitter.set_sourced_sink(get_sourced_exec_event_sink(ClientId::from("BYBIT")));
        let (mut channels, sourced_rx) = runner.take_channels_with_sourced();
        drop(sourced_rx);

        let result = emitter.try_send_order_event(OrderEventAny::Submitted(
            OrderSubmittedSpec::builder().build(),
        ));

        assert!(result.is_err());
        assert!(channels.exec_evt_rx.try_recv().is_err());
    }

    #[rstest]
    fn test_sourced_bootstrap_account_retains_its_purpose() {
        let runner = AsyncRunner::new();
        runner.bind_senders();
        let source_client_id = ClientId::from("BYBIT");
        let event_id = UUID4::new();
        let mut emitter = test_emitter();
        emitter.set_sender(get_exec_event_sender());
        emitter.set_sourced_sink(get_sourced_exec_event_sink(source_client_id));

        emitter
            .try_send_bootstrap_account_state(test_account_state(event_id))
            .unwrap();

        let (mut channels, mut sourced_rx) = runner.take_channels_with_sourced();
        assert!(channels.exec_evt_rx.try_recv().is_err());
        let Some(ExecutionEventIngress::Sourced(
            crate::runner::SourcedExecutionEvent::BootstrapAccount { client_id, state },
        )) = sourced_rx.try_recv(&mut channels.exec_evt_rx)
        else {
            panic!("expected sourced bootstrap account event");
        };
        assert_eq!(client_id, source_client_id);
        assert_eq!(state.event_id, event_id);
    }

    #[rstest]
    fn test_legacy_bootstrap_account_is_an_ordinary_account_event() {
        let runner = AsyncRunner::new();
        runner.bind_senders();
        let event_id = UUID4::new();
        let mut emitter = test_emitter();
        emitter.set_sender(get_exec_event_sender());

        emitter
            .try_send_bootstrap_account_state(test_account_state(event_id))
            .unwrap();

        let (mut channels, mut sourced_rx) = runner.take_channels_with_sourced();
        let ExecutionEvent::Account(state) = channels.exec_evt_rx.try_recv().unwrap() else {
            panic!("expected ordinary account event");
        };
        assert_eq!(state.event_id, event_id);
        assert!(sourced_rx.try_recv(&mut channels.exec_evt_rx).is_none());
    }

    #[rstest]
    fn test_closed_sourced_target_rejects_bootstrap_without_legacy_fallback() {
        let runner = AsyncRunner::new();
        runner.bind_senders();
        let mut emitter = test_emitter();
        emitter.set_sender(get_exec_event_sender());
        emitter.set_sourced_sink(get_sourced_exec_event_sink(ClientId::from("BYBIT")));
        let (mut channels, sourced_rx) = runner.take_channels_with_sourced();
        drop(sourced_rx);

        let result = emitter.try_send_bootstrap_account_state(test_account_state(UUID4::new()));

        assert!(result.is_err());
        assert!(channels.exec_evt_rx.try_recv().is_err());
    }

    #[rstest]
    fn test_legacy_target_remains_unchanged() {
        let runner = AsyncRunner::new();
        runner.bind_senders();
        let mut emitter = test_emitter();
        emitter.set_sender(get_exec_event_sender());

        emitter.send_position_report(test_position_report());

        let (mut channels, mut sourced_rx) = runner.take_channels_with_sourced();
        assert!(matches!(
            channels.exec_evt_rx.try_recv().unwrap(),
            ExecutionEvent::Report(ExecutionReport::Position(_))
        ));
        assert!(sourced_rx.try_recv(&mut channels.exec_evt_rx).is_none());
    }
}
