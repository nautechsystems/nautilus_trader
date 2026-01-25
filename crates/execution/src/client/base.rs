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

//! Base execution client functionality.

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::{cell::RefCell, fmt::Debug, rc::Rc};

use nautilus_common::{
    cache::Cache, clock::Clock, messages::ExecutionReport, msgbus, msgbus::MessagingSwitchboard,
};
use nautilus_core::UUID4;
use nautilus_model::{
    accounts::AccountAny,
    enums::{AccountType, LiquiditySide, OmsType},
    events::{
        AccountState, OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDenied,
        OrderEventAny, OrderExpired, OrderFilled, OrderModifyRejected, OrderRejected,
        OrderSubmitted, OrderTriggered, OrderUpdated,
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, PositionId, TradeId, TraderId, Venue, VenueOrderId,
    },
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};

/// Base implementation for execution clients providing common functionality.
///
/// This struct provides the foundation for all execution clients, handling
/// account state generation, order event creation, and message routing.
/// Execution clients can inherit this base functionality and extend it
/// with venue-specific implementations.
#[derive(Clone)]
pub struct ExecutionClientCore {
    pub trader_id: TraderId,
    pub client_id: ClientId,
    pub venue: Venue,
    pub oms_type: OmsType,
    pub account_id: AccountId,
    pub account_type: AccountType,
    pub base_currency: Option<Currency>,
    pub is_connected: bool,
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
}

impl Debug for ExecutionClientCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ExecutionClientCore))
            .field("client_id", &self.client_id)
            .finish()
    }
}

impl ExecutionClientCore {
    /// Creates a new [`ExecutionClientCore`] instance.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        trader_id: TraderId,
        client_id: ClientId,
        venue: Venue,
        oms_type: OmsType,
        account_id: AccountId,
        account_type: AccountType,
        base_currency: Option<Currency>,
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
    ) -> Self {
        Self {
            trader_id,
            client_id,
            venue,
            oms_type,
            account_id,
            account_type,
            base_currency,
            is_connected: false,
            clock,
            cache,
        }
    }

    /// Sets the connection status of the execution client.
    pub const fn set_connected(&mut self, is_connected: bool) {
        self.is_connected = is_connected;
    }

    /// Sets the account identifier for the execution client.
    pub const fn set_account_id(&mut self, account_id: AccountId) {
        self.account_id = account_id;
    }

    /// Returns a reference to the clock.
    #[must_use]
    pub const fn clock(&self) -> &Rc<RefCell<dyn Clock>> {
        &self.clock
    }

    /// Returns a reference to the cache.
    #[must_use]
    pub const fn cache(&self) -> &Rc<RefCell<Cache>> {
        &self.cache
    }

    /// Returns the account associated with this execution client.
    #[must_use]
    pub fn get_account(&self) -> Option<AccountAny> {
        self.cache.borrow().account(&self.account_id).cloned()
    }

    /// Returns the order for the given client order ID from the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if the order is not found in the cache.
    pub fn get_order(&self, client_order_id: &ClientOrderId) -> anyhow::Result<OrderAny> {
        self.cache
            .borrow()
            .order(client_order_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Order not found in cache for {client_order_id}"))
    }

    /// Generates an account state event.
    #[must_use]
    pub fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        // info:  TODO: Need to double check the use case here
    ) -> AccountState {
        let ts = self.clock.borrow().timestamp_ns();
        AccountState::new(
            self.account_id,
            self.account_type,
            balances,
            margins,
            reported,
            UUID4::new(),
            ts,
            ts,
            self.base_currency,
        )
    }

    /// Generates an order denied event.
    #[must_use]
    pub fn generate_order_denied(&self, order: &dyn Order, reason: &str) -> OrderEventAny {
        let ts = self.clock.borrow().timestamp_ns();
        let event = OrderDenied::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            reason.into(),
            UUID4::new(),
            ts,
            ts,
        );
        OrderEventAny::Denied(event)
    }

    /// Generates an order submitted event.
    #[must_use]
    pub fn generate_order_submitted(&self, order: &dyn Order) -> OrderEventAny {
        let ts = self.clock.borrow().timestamp_ns();
        let event = OrderSubmitted::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            self.account_id,
            UUID4::new(),
            ts,
            ts,
        );
        OrderEventAny::Submitted(event)
    }

    /// Generates an order rejected event.
    #[must_use]
    pub fn generate_order_rejected(
        &self,
        order: &dyn Order,
        reason: &str,
        due_post_only: bool,
    ) -> OrderEventAny {
        let ts = self.clock.borrow().timestamp_ns();
        let event = OrderRejected::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            self.account_id,
            reason.into(),
            UUID4::new(),
            ts,
            ts,
            false,
            due_post_only,
        );
        OrderEventAny::Rejected(event)
    }

    /// Generates an order accepted event.
    #[must_use]
    pub fn generate_order_accepted(
        &self,
        order: &dyn Order,
        venue_order_id: VenueOrderId,
    ) -> OrderEventAny {
        let ts = self.clock.borrow().timestamp_ns();
        let event = OrderAccepted::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            venue_order_id,
            self.account_id,
            UUID4::new(),
            ts,
            ts,
            false,
        );
        OrderEventAny::Accepted(event)
    }

    /// Generates an order modify rejected event.
    #[must_use]
    pub fn generate_order_modify_rejected(
        &self,
        order: &dyn Order,
        venue_order_id: Option<VenueOrderId>,
        reason: &str,
    ) -> OrderEventAny {
        let ts = self.clock.borrow().timestamp_ns();
        let event = OrderModifyRejected::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            reason.into(),
            UUID4::new(),
            ts,
            ts,
            false,
            venue_order_id,
            Some(self.account_id),
        );
        OrderEventAny::ModifyRejected(event)
    }

    /// Generates an order cancel rejected event.
    #[must_use]
    pub fn generate_order_cancel_rejected(
        &self,
        order: &dyn Order,
        venue_order_id: Option<VenueOrderId>,
        reason: &str,
    ) -> OrderEventAny {
        let ts = self.clock.borrow().timestamp_ns();
        let event = OrderCancelRejected::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            reason.into(),
            UUID4::new(),
            ts,
            ts,
            false,
            venue_order_id,
            Some(self.account_id),
        );
        OrderEventAny::CancelRejected(event)
    }

    /// Generates an order updated event.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn generate_order_updated(
        &self,
        order: &dyn Order,
        venue_order_id: VenueOrderId,
        quantity: Quantity,
        price: Option<Price>,
        trigger_price: Option<Price>,
        protection_price: Option<Price>,
        venue_order_id_modified: bool,
    ) -> OrderEventAny {
        if !venue_order_id_modified {
            let cache = self.cache.as_ref().borrow();
            let existing_order_result = cache.venue_order_id(&order.client_order_id());
            if let Some(existing_order) = existing_order_result
                && *existing_order != venue_order_id
            {
                log::error!(
                    "Existing venue order id {existing_order} does not match provided venue order id {venue_order_id}"
                );
            }
        }

        let ts = self.clock.borrow().timestamp_ns();
        let event = OrderUpdated::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            quantity,
            UUID4::new(),
            ts,
            ts,
            false,
            Some(venue_order_id),
            Some(self.account_id),
            price,
            trigger_price,
            protection_price,
        );

        OrderEventAny::Updated(event)
    }

    /// Generates an order canceled event.
    #[must_use]
    pub fn generate_order_canceled(
        &self,
        order: &dyn Order,
        venue_order_id: Option<VenueOrderId>,
    ) -> OrderEventAny {
        let ts = self.clock.borrow().timestamp_ns();
        let event = OrderCanceled::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            UUID4::new(),
            ts,
            ts,
            false,
            venue_order_id,
            Some(self.account_id),
        );

        OrderEventAny::Canceled(event)
    }

    /// Generates an order triggered event.
    #[must_use]
    pub fn generate_order_triggered(
        &self,
        order: &dyn Order,
        venue_order_id: Option<VenueOrderId>,
    ) -> OrderEventAny {
        let ts = self.clock.borrow().timestamp_ns();
        let event = OrderTriggered::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            UUID4::new(),
            ts,
            ts,
            false,
            venue_order_id,
            Some(self.account_id),
        );

        OrderEventAny::Triggered(event)
    }

    /// Generates an order expired event.
    #[must_use]
    pub fn generate_order_expired(
        &self,
        order: &dyn Order,
        venue_order_id: Option<VenueOrderId>,
    ) -> OrderEventAny {
        let ts = self.clock.borrow().timestamp_ns();
        let event = OrderExpired::new(
            self.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            UUID4::new(),
            ts,
            ts,
            false,
            venue_order_id,
            Some(self.account_id),
        );

        OrderEventAny::Expired(event)
    }

    /// Generates an order filled event.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn generate_order_filled(
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
    ) -> OrderEventAny {
        let ts = self.clock.borrow().timestamp_ns();
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
            ts,
            ts,
            false,
            venue_position_id,
            commission,
        );

        OrderEventAny::Filled(event)
    }

    /// Sends an account state event via msgbus.
    pub fn send_account_state(&self, account_state: &AccountState) {
        let endpoint = MessagingSwitchboard::portfolio_update_account();
        msgbus::send_account_state(endpoint, account_state);
    }

    /// Sends an order event via msgbus.
    pub fn send_order_event(&self, event: OrderEventAny) {
        let endpoint = MessagingSwitchboard::exec_engine_process();
        msgbus::send_order_event(endpoint, event);
    }

    fn send_mass_status_report(&self, report: ExecutionMassStatus) {
        let endpoint = MessagingSwitchboard::exec_engine_reconcile_execution_report();
        let report = ExecutionReport::MassStatus(Box::new(report));
        msgbus::send_execution_report(endpoint, report);
    }

    fn send_order_status_report(&self, report: OrderStatusReport) {
        let endpoint = MessagingSwitchboard::exec_engine_reconcile_execution_report();
        let report = ExecutionReport::Order(Box::new(report));
        msgbus::send_execution_report(endpoint, report);
    }

    fn send_fill_report(&self, report: FillReport) {
        let endpoint = MessagingSwitchboard::exec_engine_reconcile_execution_report();
        let report = ExecutionReport::Fill(Box::new(report));
        msgbus::send_execution_report(endpoint, report);
    }

    fn send_position_report(&self, report: PositionStatusReport) {
        let endpoint = MessagingSwitchboard::exec_engine_reconcile_execution_report();
        let report = ExecutionReport::Position(Box::new(report));
        msgbus::send_execution_report(endpoint, report);
    }
}
