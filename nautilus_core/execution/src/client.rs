// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{cell::RefCell, rc::Rc};

use nautilus_common::{cache::Cache, msgbus::MessageBus};
use nautilus_core::{nanos::UnixNanos, time::AtomicTime, uuid::UUID4};
use nautilus_model::{
    accounts::any::AccountAny,
    enums::{AccountType, LiquiditySide, OmsType, OrderSide, OrderType},
    events::{
        account::state::AccountState,
        order::{
            OrderAccepted, OrderCancelRejected, OrderCanceled, OrderEventAny, OrderExpired,
            OrderFilled, OrderModifyRejected, OrderRejected, OrderSubmitted, OrderTriggered,
            OrderUpdated,
        },
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, PositionId, StrategyId, TradeId,
        TraderId, Venue, VenueOrderId,
    },
    types::{
        balance::{AccountBalance, MarginBalance},
        currency::Currency,
        money::Money,
        price::Price,
        quantity::Quantity,
    },
};

use crate::messages::{
    cancel::CancelOrder, cancel_batch::BatchCancelOrders, modify::ModifyOrder, query::QueryOrder,
    submit::SubmitOrder, submit_list::SubmitOrderList,
};

pub struct ExecutionClient {
    pub trader_id: TraderId,
    pub client_id: ClientId,
    pub venue: Venue,
    pub oms_type: OmsType,
    pub account_id: AccountId,
    pub account_type: AccountType,
    pub base_currency: Option<Currency>,
    pub is_connected: bool,
    clock: &'static AtomicTime,
    cache: Rc<RefCell<Cache>>,
    msgbus: Rc<RefCell<MessageBus>>,
}

impl ExecutionClient {
    #[must_use]
    pub fn get_account(&self) -> AccountAny {
        let cache = self.cache.as_ref().borrow();
        cache.account(&self.account_id).unwrap().clone()
    }

    // -- COMMAND HANDLERS ----------------------------------------------------

    pub fn submit_order(&self, command: SubmitOrder) -> anyhow::Result<()> {
        todo!();
    }

    pub fn submit_order_list(&self, command: SubmitOrderList) -> anyhow::Result<()> {
        todo!();
    }

    pub fn modify_order(&self, command: ModifyOrder) -> anyhow::Result<()> {
        todo!();
    }

    pub fn cancel_order(&self, command: CancelOrder) -> anyhow::Result<()> {
        todo!();
    }

    pub fn batch_cancel_orders(&self, command: BatchCancelOrders) -> anyhow::Result<()> {
        todo!();
    }

    pub fn query_order(&self, command: QueryOrder) -> anyhow::Result<()> {
        todo!();
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
            self.clock.get_time_ns(),
            self.base_currency,
        );
        self.send_account_state(account_state)
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
            self.clock.get_time_ns(),
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
            self.clock.get_time_ns(),
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
            self.clock.get_time_ns(),
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
            self.clock.get_time_ns(),
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
            self.clock.get_time_ns(),
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
        reason: &str,
        ts_event: UnixNanos,
        venue_order_id_modified: bool,
    ) {
        if !venue_order_id_modified {
            let cache = self.cache.as_ref().borrow();
            let existing_order_result = cache.venue_order_id(&client_order_id);
            if let Some(existing_order) = existing_order_result {
                if *existing_order != venue_order_id {
                    log::error!(
                        "Existing venue order id {} does not match provided venue order id {}",
                        existing_order,
                        venue_order_id
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
            self.clock.get_time_ns(),
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
            self.clock.get_time_ns(),
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
            self.clock.get_time_ns(),
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
            self.clock.get_time_ns(),
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
            self.clock.get_time_ns(),
            false,
            Some(venue_position_id),
            Some(commission),
        );

        self.send_order_event(OrderEventAny::Filled(event));
    }

    fn send_account_state(&self, account_state: AccountState) -> anyhow::Result<()> {
        todo!()
    }

    fn send_order_event(&self, event: OrderEventAny) {
        todo!()
    }

    // TODO: Implement execution reports
    // fn send_mass_status_report(&self, report)

    // TODO: Implement execution reports
    // fn send_order_status_report(&self, report)

    // TODO: Implement execution reports
    // fn send_fill_report(&self, report)
}
