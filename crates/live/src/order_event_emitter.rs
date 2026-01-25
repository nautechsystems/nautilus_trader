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

//! Live order event emitter for sending pre-constructed events.
//!
//! This module provides [`OrderEventEmitter`], a struct for emitting events via a channel.
//! Adapters use the `emit_*` methods to construct and send order events.
//!
//! # Architecture
//!
//! ```text
//! Adapter
//! └── emitter: OrderEventEmitter      (event construction + async sending)
//!     ├── trader_id: TraderId
//!     ├── account_id: AccountId
//!     └── sender: Option<Sender>      (set in start())
//! ```

use nautilus_common::messages::{ExecutionEvent, ExecutionReport};
use nautilus_core::{UUID4, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{
    enums::LiquiditySide,
    events::{
        AccountState, OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied,
        OrderEventAny, OrderExpired, OrderFilled, OrderModifyRejected, OrderRejected,
        OrderSubmitted, OrderTriggered, OrderUpdated,
    },
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId, TraderId,
        VenueOrderId,
    },
    orders::Order,
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{Currency, Money, Price, Quantity},
};

/// Event emitter for live trading - handles async sending of events.
///
/// Can be cloned and moved into async tasks. The sender is set during
/// the adapter's `start()` phase.
#[derive(Debug, Clone)]
pub struct OrderEventEmitter {
    trader_id: TraderId,
    account_id: AccountId,
    sender: Option<tokio::sync::mpsc::UnboundedSender<ExecutionEvent>>,
}

impl OrderEventEmitter {
    /// Creates a new [`OrderEventEmitter`] with no sender.
    ///
    /// Call [`set_sender`](Self::set_sender) in the adapter's `start()` method.
    #[must_use]
    pub fn new(trader_id: TraderId, account_id: AccountId) -> Self {
        Self {
            trader_id,
            account_id,
            sender: None,
        }
    }

    /// Sets the sender. Call in adapter's `start()`.
    pub fn set_sender(&mut self, sender: tokio::sync::mpsc::UnboundedSender<ExecutionEvent>) {
        self.sender = Some(sender);
    }

    /// Returns the trader ID.
    #[must_use]
    pub fn trader_id(&self) -> TraderId {
        self.trader_id
    }

    /// Returns the account ID.
    #[must_use]
    pub fn account_id(&self) -> AccountId {
        self.account_id
    }

    /// Returns true if the sender is initialized.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.sender.is_some()
    }

    // ---- Order Event Convenience Methods ----

    /// Constructs and emits an [`OrderDenied`] event.
    pub fn emit_order_denied_event(
        &self,
        order: &dyn Order,
        reason: &str,
        ts_event: impl Into<UnixNanos>,
    ) {
        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let event = OrderDenied::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            reason.into(),
            UUID4::new(),
            ts_event.into(),
            ts_init,
        );
        self.emit_execution_order_event(OrderEventAny::Denied(event));
    }

    /// Constructs and emits an [`OrderSubmitted`] event.
    pub fn emit_order_submitted_event(&self, order: &dyn Order, ts_event: impl Into<UnixNanos>) {
        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let event = OrderSubmitted::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            self.account_id,
            UUID4::new(),
            ts_event.into(),
            ts_init,
        );
        self.emit_execution_order_event(OrderEventAny::Submitted(event));
    }

    /// Constructs and emits an [`OrderRejected`] event.
    pub fn emit_order_rejected_event(
        &self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        reason: &str,
        ts_event: impl Into<UnixNanos>,
        due_post_only: bool,
    ) {
        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let event = OrderRejected::new(
            self.trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            self.account_id,
            reason.into(),
            UUID4::new(),
            ts_event.into(),
            ts_init,
            false,
            due_post_only,
        );
        self.emit_execution_order_event(OrderEventAny::Rejected(event));
    }

    /// Constructs and emits an [`OrderAccepted`] event.
    pub fn emit_order_accepted_event(
        &self,
        order: &dyn Order,
        venue_order_id: VenueOrderId,
        ts_event: impl Into<UnixNanos>,
    ) {
        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let event = OrderAccepted::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            venue_order_id,
            self.account_id,
            UUID4::new(),
            ts_event.into(),
            ts_init,
            false,
        );
        self.emit_execution_order_event(OrderEventAny::Accepted(event));
    }

    /// Constructs and emits an [`OrderModifyRejected`] event.
    pub fn emit_order_modify_rejected_event(
        &self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        reason: &str,
        ts_event: impl Into<UnixNanos>,
    ) {
        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let event = OrderModifyRejected::new(
            self.trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            reason.into(),
            UUID4::new(),
            ts_init,
            ts_event.into(),
            false,
            venue_order_id,
            Some(self.account_id),
        );
        self.emit_execution_order_event(OrderEventAny::ModifyRejected(event));
    }

    /// Constructs and emits an [`OrderCancelRejected`] event.
    pub fn emit_order_cancel_rejected_event(
        &self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        reason: &str,
        ts_event: impl Into<UnixNanos>,
    ) {
        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let event = OrderCancelRejected::new(
            self.trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            reason.into(),
            UUID4::new(),
            ts_init,
            ts_event.into(),
            false,
            venue_order_id,
            Some(self.account_id),
        );
        self.emit_execution_order_event(OrderEventAny::CancelRejected(event));
    }

    /// Constructs and emits an [`OrderUpdated`] event.
    #[allow(clippy::too_many_arguments)]
    pub fn emit_order_updated_event(
        &self,
        order: &dyn Order,
        venue_order_id: VenueOrderId,
        quantity: Quantity,
        price: Option<Price>,
        trigger_price: Option<Price>,
        ts_event: impl Into<UnixNanos>,
    ) {
        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let event = OrderUpdated::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            quantity,
            UUID4::new(),
            ts_event.into(),
            ts_init,
            false,
            Some(venue_order_id),
            Some(self.account_id),
            price,
            trigger_price,
            None, // protection_price
        );
        self.emit_execution_order_event(OrderEventAny::Updated(event));
    }

    /// Constructs and emits an [`OrderCanceled`] event.
    pub fn emit_order_canceled_event(
        &self,
        order: &dyn Order,
        venue_order_id: Option<VenueOrderId>,
        ts_event: impl Into<UnixNanos>,
    ) {
        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let event = OrderCanceled::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            UUID4::new(),
            ts_event.into(),
            ts_init,
            false,
            venue_order_id,
            Some(self.account_id),
        );
        self.emit_execution_order_event(OrderEventAny::Canceled(event));
    }

    /// Constructs and emits an [`OrderTriggered`] event.
    pub fn emit_order_triggered_event(
        &self,
        order: &dyn Order,
        venue_order_id: Option<VenueOrderId>,
        ts_event: impl Into<UnixNanos>,
    ) {
        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let event = OrderTriggered::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            UUID4::new(),
            ts_event.into(),
            ts_init,
            false,
            venue_order_id,
            Some(self.account_id),
        );
        self.emit_execution_order_event(OrderEventAny::Triggered(event));
    }

    /// Constructs and emits an [`OrderExpired`] event.
    pub fn emit_order_expired_event(
        &self,
        order: &dyn Order,
        venue_order_id: Option<VenueOrderId>,
        ts_event: impl Into<UnixNanos>,
    ) {
        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let event = OrderExpired::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            UUID4::new(),
            ts_event.into(),
            ts_init,
            false,
            venue_order_id,
            Some(self.account_id),
        );
        self.emit_execution_order_event(OrderEventAny::Expired(event));
    }

    /// Constructs and emits an [`OrderFilled`] event.
    #[allow(clippy::too_many_arguments)]
    pub fn emit_order_filled_event(
        &self,
        order: &dyn Order,
        venue_order_id: VenueOrderId,
        venue_position_id: Option<PositionId>,
        trade_id: TradeId,
        last_qty: Quantity,
        last_px: Price,
        quote_currency: Currency,
        commission: Option<Money>,
        liquidity_side: LiquiditySide,
        ts_event: impl Into<UnixNanos>,
    ) {
        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let event = OrderFilled::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            venue_order_id,
            self.account_id,
            trade_id,
            order.order_side(),
            order.order_type(),
            last_qty,
            last_px,
            quote_currency,
            liquidity_side,
            UUID4::new(),
            ts_event.into(),
            ts_init,
            false,
            venue_position_id,
            commission,
        );
        self.emit_execution_order_event(OrderEventAny::Filled(event));
    }

    /// Emits a pre-constructed order event.
    pub fn emit_execution_order_event(&self, event: OrderEventAny) {
        if let Some(sender) = &self.sender {
            if let Err(e) = sender.send(ExecutionEvent::Order(event)) {
                log::warn!("Failed to send order event: {e}");
            }
        } else {
            log::warn!("Cannot send order event: sender not initialized");
        }
    }

    /// Emits an account state event.
    pub fn emit_account_state_event(&self, state: AccountState) {
        if let Some(sender) = &self.sender {
            if let Err(e) = sender.send(ExecutionEvent::Account(state)) {
                log::warn!("Failed to send account state: {e}");
            }
        } else {
            log::warn!("Cannot send account state: sender not initialized");
        }
    }

    /// Emits an execution report.
    pub fn emit_execution_report(&self, report: ExecutionReport) {
        if let Some(sender) = &self.sender {
            if let Err(e) = sender.send(ExecutionEvent::Report(report)) {
                log::warn!("Failed to send execution report: {e}");
            }
        } else {
            log::warn!("Cannot send execution report: sender not initialized");
        }
    }

    /// Emits an order status report.
    pub fn emit_order_status_report(&self, report: OrderStatusReport) {
        let exec_report = ExecutionReport::Order(Box::new(report));
        if let Some(sender) = &self.sender {
            if let Err(e) = sender.send(ExecutionEvent::Report(exec_report)) {
                log::warn!("Failed to send order status report: {e}");
            }
        } else {
            log::warn!("Cannot send order status report: sender not initialized");
        }
    }

    /// Emits a fill report.
    pub fn emit_fill_report(&self, report: FillReport) {
        let exec_report = ExecutionReport::Fill(Box::new(report));
        if let Some(sender) = &self.sender {
            if let Err(e) = sender.send(ExecutionEvent::Report(exec_report)) {
                log::warn!("Failed to send fill report: {e}");
            }
        } else {
            log::warn!("Cannot send fill report: sender not initialized");
        }
    }

    /// Emits a position status report.
    pub fn emit_position_report(&self, report: PositionStatusReport) {
        let exec_report = ExecutionReport::Position(Box::new(report));
        if let Some(sender) = &self.sender {
            if let Err(e) = sender.send(ExecutionEvent::Report(exec_report)) {
                log::warn!("Failed to send position report: {e}");
            }
        } else {
            log::warn!("Cannot send position report: sender not initialized");
        }
    }
}
