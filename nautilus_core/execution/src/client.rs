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

use nautilus_common::cache::Cache;
use nautilus_core::nanos::UnixNanos;
use nautilus_model::{
    enums::{AccountType, LiquiditySide, OmsType, OrderSide, OrderType},
    events::{account::state::AccountState, order::event::OrderEventAny},
    identifiers::{
        account_id::AccountId, client_order_id::ClientOrderId, instrument_id::InstrumentId,
        position_id::PositionId, strategy_id::StrategyId, trade_id::TradeId, venue::Venue,
        venue_order_id::VenueOrderId,
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
    pub venue: Venue,
    pub oms_type: OmsType,
    pub account_id: AccountId,
    pub account_type: AccountType,
    pub base_currency: Option<Currency>,
    pub is_connected: bool,
    cache: &'static Cache,
}

impl ExecutionClient {
    // TODO: Polymorphism for `Account` TBD?
    // pub fn get_account(&self) -> Box<dyn Account> {
    //     todo!();
    // }

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
        todo!();
    }

    pub fn generate_order_submitted(
        &self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        ts_event: UnixNanos,
    ) {
        todo!();
    }

    pub fn generate_order_rejected(
        &self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        reason: &str,
        ts_event: UnixNanos,
    ) {
        todo!();
    }

    pub fn generate_order_accepted(
        &self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        reason: &str,
        ts_event: UnixNanos,
    ) {
        todo!();
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
        todo!();
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
        todo!();
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
        todo!();
    }

    pub fn generate_order_canceled(
        &self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        ts_event: UnixNanos,
    ) {
        todo!();
    }

    pub fn generate_order_triggered(
        &self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        ts_event: UnixNanos,
    ) {
        todo!();
    }

    pub fn generate_order_expired(
        &self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        ts_event: UnixNanos,
    ) {
        todo!();
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
        todo!();
    }

    fn send_account_state(&self, account_state: AccountState) {
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
