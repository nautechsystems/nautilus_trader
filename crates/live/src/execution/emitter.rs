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
//!     `-- sender: ArcSwapOption<Sender>   (set in start())
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

/// Event emitter for live trading - combines event generation with async dispatch.
///
/// This struct wraps an [`OrderEventFactory`] for event construction and an unbounded
/// channel sender for async dispatch. It provides `emit_*` convenience methods that
/// generate and send events in a single call.
///
/// The sender is set during the adapter's `start()` phase via [`set_sender`](Self::set_sender).
/// Clones share the sender slot and observe later sender installations and replacements.
#[derive(Debug, Clone)]
pub struct ExecutionEventEmitter {
    clock: &'static AtomicTime,
    factory: OrderEventFactory,
    sender: Arc<ArcSwapOption<tokio::sync::mpsc::UnboundedSender<ExecutionEvent>>>,
}

impl ExecutionEventEmitter {
    /// Creates a new [`ExecutionEventEmitter`] with no sender.
    ///
    /// Call [`set_sender`](Self::set_sender) in the adapter's `start()` method.
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
            sender: Arc::new(ArcSwapOption::empty()),
        }
    }

    fn ts_init(&self) -> UnixNanos {
        self.clock.get_time_ns()
    }

    /// Installs or replaces the sender for this emitter and all its clones.
    ///
    /// Call in the adapter's `start()` method.
    pub fn set_sender(&mut self, sender: tokio::sync::mpsc::UnboundedSender<ExecutionEvent>) {
        self.sender.store(Some(Arc::new(sender)));
    }

    /// Returns true if the sender is initialized for this emitter and its clones.
    #[must_use]
    pub fn is_initialized(&self) -> bool {
        self.sender.load().is_some()
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
        let sender = self.sender.load();
        let sender = sender
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Cannot send order event: sender not initialized"))?;
        sender
            .send(ExecutionEvent::Order(event))
            .map_err(|e| anyhow::anyhow!("Failed to send order event: {e}"))
    }

    /// Emits a batch of order submitted events as a single channel message.
    pub fn send_order_submitted_batch(&self, batch: OrderSubmittedBatch) {
        let sender = self.sender.load();
        if let Some(sender) = sender.as_ref() {
            if let Err(e) = sender.send(ExecutionEvent::OrderSubmittedBatch(batch)) {
                log::warn!("Failed to send order submitted batch: {e}");
            }
        } else {
            log::warn!("Cannot send order submitted batch: sender not initialized");
        }
    }

    /// Emits a batch of order accepted events as a single channel message.
    pub fn send_order_accepted_batch(&self, batch: OrderAcceptedBatch) {
        let sender = self.sender.load();
        if let Some(sender) = sender.as_ref() {
            if let Err(e) = sender.send(ExecutionEvent::OrderAcceptedBatch(batch)) {
                log::warn!("Failed to send order accepted batch: {e}");
            }
        } else {
            log::warn!("Cannot send order accepted batch: sender not initialized");
        }
    }

    /// Emits a batch of order canceled events as a single channel message.
    pub fn send_order_canceled_batch(&self, batch: OrderCanceledBatch) {
        let sender = self.sender.load();
        if let Some(sender) = sender.as_ref() {
            if let Err(e) = sender.send(ExecutionEvent::OrderCanceledBatch(batch)) {
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

    /// Emits an account state event and returns any channel error to the caller.
    ///
    /// # Errors
    ///
    /// Returns an error if the sender is uninitialized or its receiver is closed.
    pub fn try_send_account_state(&self, state: AccountState) -> anyhow::Result<()> {
        let sender = self.sender.load();
        let sender = sender
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Cannot send account state: sender not initialized"))?;
        sender
            .send(ExecutionEvent::Account(state))
            .map_err(|e| anyhow::anyhow!("Failed to send account state: {e}"))
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
        let sender = self.sender.load();
        let sender = sender.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Cannot send execution report: sender not initialized")
        })?;
        sender
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
    use nautilus_core::time::get_atomic_clock_static;
    use nautilus_model::events::order::spec::OrderSubmittedSpec;
    use rstest::rstest;

    use super::*;

    fn create_emitter() -> ExecutionEventEmitter {
        ExecutionEventEmitter::new(
            get_atomic_clock_static(),
            TraderId::from("TRADER-001"),
            AccountId::from("SIM-001"),
            AccountType::Cash,
            None,
        )
    }

    fn create_order_event() -> OrderEventAny {
        OrderEventAny::Submitted(
            OrderSubmittedSpec::builder()
                .client_order_id(ClientOrderId::from("O-001"))
                .build(),
        )
    }

    #[rstest]
    fn test_clone_before_set_sender_observes_sender() {
        let mut emitter = create_emitter();
        let cloned = emitter.clone();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        emitter.set_sender(tx);

        assert!(cloned.is_initialized());
        cloned.send_order_event(create_order_event());
        assert!(matches!(
            rx.try_recv(),
            Ok(ExecutionEvent::Order(OrderEventAny::Submitted(_)))
        ));
    }

    #[rstest]
    fn test_set_sender_replaces_existing_sender() {
        let mut emitter = create_emitter();
        let (tx_a, mut rx_a) = tokio::sync::mpsc::unbounded_channel();
        let (tx_b, mut rx_b) = tokio::sync::mpsc::unbounded_channel();

        emitter.set_sender(tx_a);
        emitter.set_sender(tx_b);
        emitter.send_order_event(create_order_event());

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
        let emitter = create_emitter();

        emitter.send_order_event(create_order_event());
        let error = emitter
            .try_send_order_event(create_order_event())
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Cannot send order event: sender not initialized"
        );
    }
}
