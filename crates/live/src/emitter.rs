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
//! ├── core: ExecutionClientCore    (identity + connection state)
//! └── emitter: ExecutionEventEmitter   (event generation + async dispatch)
//!     ├── factory: OrderEventFactory
//!     └── sender: Option<Sender>   (set in start())
//! ```

use nautilus_common::{
    factories::OrderEventFactory,
    messages::{ExecutionEvent, ExecutionReport},
};
use nautilus_core::{UUID4, UnixNanos, time::AtomicTime};
use nautilus_model::{
    enums::{AccountType, LiquiditySide},
    events::{
        AccountState, OrderCancelRejected, OrderEventAny, OrderModifyRejected, OrderRejected,
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
#[derive(Debug, Clone)]
pub struct ExecutionEventEmitter {
    clock: &'static AtomicTime,
    factory: OrderEventFactory,
    sender: Option<tokio::sync::mpsc::UnboundedSender<ExecutionEvent>>,
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
            sender: None,
        }
    }

    fn ts_init(&self) -> UnixNanos {
        self.clock.get_time_ns()
    }

    /// Sets the sender. Call in adapter's `start()`.
    pub fn set_sender(&mut self, sender: tokio::sync::mpsc::UnboundedSender<ExecutionEvent>) {
        self.sender = Some(sender);
    }

    /// Returns true if the sender is initialized.
    #[must_use]
    pub fn is_initialized(&self) -> bool {
        self.sender.is_some()
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

    /// Generates and emits an account state event.
    pub fn emit_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
    ) {
        let state = self.factory.generate_account_state(
            balances,
            margins,
            reported,
            ts_event,
            self.ts_init(),
        );
        self.send_account_state(state);
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
    #[allow(clippy::too_many_arguments)]
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
    #[allow(clippy::too_many_arguments)]
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
    #[allow(clippy::too_many_arguments)]
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
    #[allow(clippy::too_many_arguments)]
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
    #[allow(clippy::too_many_arguments)]
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
        if let Some(sender) = &self.sender {
            if let Err(e) = sender.send(ExecutionEvent::Order(event)) {
                log::warn!("Failed to send order event: {e}");
            }
        } else {
            log::warn!("Cannot send order event: sender not initialized");
        }
    }

    /// Emits an account state event.
    pub fn send_account_state(&self, state: AccountState) {
        if let Some(sender) = &self.sender {
            if let Err(e) = sender.send(ExecutionEvent::Account(state)) {
                log::warn!("Failed to send account state: {e}");
            }
        } else {
            log::warn!("Cannot send account state: sender not initialized");
        }
    }

    /// Emits an execution report.
    pub fn send_execution_report(&self, report: ExecutionReport) {
        if let Some(sender) = &self.sender {
            if let Err(e) = sender.send(ExecutionEvent::Report(report)) {
                log::warn!("Failed to send execution report: {e}");
            }
        } else {
            log::warn!("Cannot send execution report: sender not initialized");
        }
    }

    /// Emits an order status report.
    pub fn send_order_status_report(&self, report: OrderStatusReport) {
        self.send_execution_report(ExecutionReport::Order(Box::new(report)));
    }

    /// Emits a fill report.
    pub fn send_fill_report(&self, report: FillReport) {
        self.send_execution_report(ExecutionReport::Fill(Box::new(report)));
    }

    /// Emits a position status report.
    pub fn send_position_report(&self, report: PositionStatusReport) {
        self.send_execution_report(ExecutionReport::Position(Box::new(report)));
    }
}
