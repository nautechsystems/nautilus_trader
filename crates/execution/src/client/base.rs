// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{any::Any, cell::RefCell, rc::Rc};

use nautilus_common::{
    cache::Cache,
    clock::Clock,
    msgbus::{self},
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    accounts::AccountAny,
    enums::{AccountType, LiquiditySide, OmsType, OrderSide, OrderType},
    events::{
        AccountState, OrderAccepted, OrderCancelRejected, OrderCanceled, OrderEventAny,
        OrderExpired, OrderFilled, OrderModifyRejected, OrderRejected, OrderSubmitted,
        OrderTriggered, OrderUpdated,
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId,
        TraderId, Venue, VenueOrderId,
    },
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};
use ustr::Ustr;

use crate::reports::{
    fill::FillReport, mass_status::ExecutionMassStatus, order::OrderStatusReport,
    position::PositionStatusReport,
};

pub struct BaseExecutionClient {
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

impl BaseExecutionClient {
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
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

    pub const fn set_connected(&mut self, is_connected: bool) {
        self.is_connected = is_connected;
    }

    pub const fn set_account_id(&mut self, account_id: AccountId) {
        self.account_id = account_id;
    }

    #[must_use]
    pub fn get_account(&self) -> Option<AccountAny> {
        self.cache.borrow().account(&self.account_id).cloned()
    }

    pub fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
        // info:  TODO: Need to double check the use case here
    ) -> anyhow::Result<()> {
        let account_state = AccountState::new(
            self.account_id,
            self.account_type,
            balances,
            margins,
            reported,
            UUID4::new(),
            ts_event,
            self.clock.borrow().timestamp_ns(),
            self.base_currency,
        );
        self.send_account_state(account_state);
        Ok(())
    }

    pub fn generate_order_submitted(
        &self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        ts_event: UnixNanos,
    ) {
        let event = OrderSubmitted::new(
            self.trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            self.account_id,
            UUID4::new(),
            ts_event,
            self.clock.borrow().timestamp_ns(),
        );
        self.send_order_event(OrderEventAny::Submitted(event));
    }

    pub fn generate_order_rejected(
        &self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        reason: &str,
        ts_event: UnixNanos,
    ) {
        let event = OrderRejected::new(
            self.trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            self.account_id,
            reason.into(),
            UUID4::new(),
            ts_event,
            self.clock.borrow().timestamp_ns(),
            false,
        );
        self.send_order_event(OrderEventAny::Rejected(event));
    }

    pub fn generate_order_accepted(
        &self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        ts_event: UnixNanos,
    ) {
        let event = OrderAccepted::new(
            self.trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            self.account_id,
            UUID4::new(),
            ts_event,
            self.clock.borrow().timestamp_ns(),
            false,
        );
        self.send_order_event(OrderEventAny::Accepted(event));
    }

    pub fn generate_order_modify_rejected(
        &self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        reason: &str,
        ts_event: UnixNanos,
    ) {
        let event = OrderModifyRejected::new(
            self.trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            reason.into(),
            UUID4::new(),
            ts_event,
            self.clock.borrow().timestamp_ns(),
            false,
            Some(venue_order_id),
            Some(self.account_id),
        );
        self.send_order_event(OrderEventAny::ModifyRejected(event));
    }

    pub fn generate_order_cancel_rejected(
        &self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        reason: &str,
        ts_event: UnixNanos,
    ) {
        let event = OrderCancelRejected::new(
            self.trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            reason.into(),
            UUID4::new(),
            ts_event,
            self.clock.borrow().timestamp_ns(),
            false,
            Some(venue_order_id),
            Some(self.account_id),
        );
        self.send_order_event(OrderEventAny::CancelRejected(event));
    }

    #[allow(clippy::too_many_arguments)]
    pub fn generate_order_updated(
        &self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        quantity: Quantity,
        price: Price,
        trigger_price: Option<Price>,
        ts_event: UnixNanos,
        venue_order_id_modified: bool,
    ) {
        if !venue_order_id_modified {
            let cache = self.cache.as_ref().borrow();
            let existing_order_result = cache.venue_order_id(&client_order_id);
            if let Some(existing_order) = existing_order_result {
                if *existing_order != venue_order_id {
                    log::error!(
                        "Existing venue order id {existing_order} does not match provided venue order id {venue_order_id}"
                    );
                }
            }
        }

        let event = OrderUpdated::new(
            self.trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            quantity,
            UUID4::new(),
            ts_event,
            self.clock.borrow().timestamp_ns(),
            false,
            Some(venue_order_id),
            Some(self.account_id),
            Some(price),
            trigger_price,
        );

        self.send_order_event(OrderEventAny::Updated(event));
    }

    pub fn generate_order_canceled(
        &self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        ts_event: UnixNanos,
    ) {
        let event = OrderCanceled::new(
            self.trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            UUID4::new(),
            ts_event,
            self.clock.borrow().timestamp_ns(),
            false,
            Some(venue_order_id),
            Some(self.account_id),
        );

        self.send_order_event(OrderEventAny::Canceled(event));
    }

    pub fn generate_order_triggered(
        &self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        ts_event: UnixNanos,
    ) {
        let event = OrderTriggered::new(
            self.trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            UUID4::new(),
            ts_event,
            self.clock.borrow().timestamp_ns(),
            false,
            Some(venue_order_id),
            Some(self.account_id),
        );

        self.send_order_event(OrderEventAny::Triggered(event));
    }

    pub fn generate_order_expired(
        &self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        ts_event: UnixNanos,
    ) {
        let event = OrderExpired::new(
            self.trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            UUID4::new(),
            ts_event,
            self.clock.borrow().timestamp_ns(),
            false,
            Some(venue_order_id),
            Some(self.account_id),
        );

        self.send_order_event(OrderEventAny::Expired(event));
    }

    #[allow(clippy::too_many_arguments)]
    pub fn generate_order_filled(
        &self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        venue_position_id: PositionId,
        trade_id: TradeId,
        order_side: OrderSide,
        order_type: OrderType,
        last_qty: Quantity,
        last_px: Price,
        quote_currency: Currency,
        commission: Money,
        liquidity_side: LiquiditySide,
        ts_event: UnixNanos,
    ) {
        let event = OrderFilled::new(
            self.trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            self.account_id,
            trade_id,
            order_side,
            order_type,
            last_qty,
            last_px,
            quote_currency,
            liquidity_side,
            UUID4::new(),
            ts_event,
            self.clock.borrow().timestamp_ns(),
            false,
            Some(venue_position_id),
            Some(commission),
        );

        self.send_order_event(OrderEventAny::Filled(event));
    }

    fn send_account_state(&self, account_state: AccountState) {
        let endpoint = Ustr::from("Portfolio.update_account");
        msgbus::send(&endpoint, &account_state as &dyn Any);
    }

    fn send_order_event(&self, event: OrderEventAny) {
        let endpoint = Ustr::from("ExecEngine.process");
        msgbus::send(&endpoint, &event as &dyn Any);
    }

    fn send_mass_status_report(&self, report: ExecutionMassStatus) {
        let endpoint = Ustr::from("ExecEngine.reconcile_mass_status");
        msgbus::send(&endpoint, &report as &dyn Any);
    }

    fn send_order_status_report(&self, report: OrderStatusReport) {
        let endpoint = Ustr::from("ExecEngine.reconcile_report");
        msgbus::send(&endpoint, &report as &dyn Any);
    }

    fn send_fill_report(&self, report: FillReport) {
        let endpoint = Ustr::from("ExecEngine.reconcile_report");
        msgbus::send(&endpoint, &report as &dyn Any);
    }

    fn send_position_report(&self, report: PositionStatusReport) {
        let endpoint = Ustr::from("ExecEngine.reconcile_report");
        msgbus::send(&endpoint, &report as &dyn Any);
    }
}
