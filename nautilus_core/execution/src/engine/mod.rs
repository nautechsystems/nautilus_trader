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

//! Provides a generic `ExecutionEngine` for all environments.

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use config::ExecutionEngineConfig;
use nautilus_common::{
    cache::Cache, clock::Clock, generators::position_id::PositionIdGenerator, msgbus::MessageBus,
};
use nautilus_model::{
    enums::{OmsType, OrderSide},
    events::order::{filled::OrderFilled, OrderEventAny},
    identifiers::{ClientId, InstrumentId, StrategyId, Venue},
    instruments::any::InstrumentAny,
    orders::any::OrderAny,
    position::Position,
    types::quantity::Quantity,
};

use crate::{
    client::ExecutionClient,
    messages::{
        cancel::CancelOrder, cancel_all::CancelAllOrders, cancel_batch::BatchCancelOrders,
        modify::ModifyOrder, query::QueryOrder, submit::SubmitOrder, submit_list::SubmitOrderList,
        TradingCommand,
    },
};

pub mod config;

pub struct ExecutionEngine<C>
where
    C: Clock,
{
    clock: C,
    cache: Rc<RefCell<Cache>>,
    msgbus: Rc<RefCell<MessageBus>>,
    clients: HashMap<ClientId, ExecutionClient>,
    default_client: Option<ExecutionClient>,
    routing_map: HashMap<Venue, ClientId>,
    oms_overrides: HashMap<StrategyId, OmsType>,
    external_order_claims: HashMap<InstrumentId, StrategyId>,
    pos_id_generator: PositionIdGenerator,
    config: ExecutionEngineConfig,
}

impl<C> ExecutionEngine<C>
where
    C: Clock,
{
    #[must_use]
    pub fn position_id_count(&self, strategy_id: StrategyId) -> u64 {
        todo!();
    }

    #[must_use]
    pub fn check_integrity(&self) -> bool {
        todo!();
    }

    #[must_use]
    pub fn check_connected(&self) -> bool {
        todo!();
    }

    #[must_use]
    pub fn check_disconnected(&self) -> bool {
        todo!();
    }

    #[must_use]
    pub fn check_residuals(&self) -> bool {
        todo!();
    }

    #[must_use]
    pub fn get_external_order_claims_instruments(&self) -> HashSet<InstrumentId> {
        todo!();
    }

    // -- REGISTRATION --------------------------------------------------------

    pub fn register_client(&mut self, client: ExecutionClient) -> anyhow::Result<()> {
        todo!();
    }

    pub fn register_default_client(&mut self, client: ExecutionClient) -> anyhow::Result<()> {
        todo!();
    }

    pub fn register_venue_routing(
        &mut self,
        client_id: ClientId,
        venue: Venue,
    ) -> anyhow::Result<()> {
        todo!();
    }

    // TODO: Implement `Strategy`
    // pub fn register_external_order_claims(&mut self, strategy: Strategy) -> anyhow::Result<()> {
    //     todo!();
    // }

    pub fn deregister_client(&mut self, client_id: ClientId) -> anyhow::Result<()> {
        todo!();
    }

    // -- COMMANDS ------------------------------------------------------------

    pub fn load_cache(&self) {
        todo!();
    }

    pub fn flush_db(&self) {
        todo!();
    }

    pub fn execute(&mut self, command: TradingCommand) {
        self.execute_command(command);
    }

    pub fn process(&self, event: &OrderEventAny) {
        todo!();
    }

    // -- COMMAND HANDLERS ----------------------------------------------------

    fn execute_command(&self, command: TradingCommand) {
        log::debug!("<--[CMD] {command:?}"); // TODO: Log constants

        let client = self
            .clients
            .get(&command.client_id())
            .or_else(|| {
                self.routing_map
                    .get(&command.instrument_id().venue)
                    .and_then(|client_id| self.clients.get(client_id))
            })
            .or(self.default_client.as_ref())
            .expect("No client found");

        match command {
            TradingCommand::SubmitOrder(cmd) => self.handle_submit_order(client, cmd),
            TradingCommand::SubmitOrderList(cmd) => self.handle_submit_order_list(client, cmd),
            TradingCommand::ModifyOrder(cmd) => self.handle_modify_order(client, cmd),
            TradingCommand::CancelOrder(cmd) => self.handle_cancel_order(client, cmd),
            TradingCommand::CancelAllOrders(cmd) => self.handle_cancel_all_orders(client, cmd),
            TradingCommand::BatchCancelOrders(cmd) => self.handle_batch_cancel_orders(client, cmd),
            TradingCommand::QueryOrder(cmd) => self.handle_query_order(client, cmd),
        }
    }

    fn handle_submit_order(&self, client: &ExecutionClient, command: SubmitOrder) {
        todo!();
    }

    fn handle_submit_order_list(&self, client: &ExecutionClient, command: SubmitOrderList) {
        todo!();
    }

    fn handle_modify_order(&self, client: &ExecutionClient, command: ModifyOrder) {
        todo!();
    }

    fn handle_cancel_order(&self, client: &ExecutionClient, command: CancelOrder) {
        todo!();
    }

    fn handle_cancel_all_orders(&self, client: &ExecutionClient, command: CancelAllOrders) {
        todo!();
    }

    fn handle_batch_cancel_orders(&self, client: &ExecutionClient, command: BatchCancelOrders) {
        todo!();
    }

    fn handle_query_order(&self, client: &ExecutionClient, command: QueryOrder) {
        todo!();
    }

    // -- EVENT HANDLERS ----------------------------------------------------

    fn handle_event(&self, event: OrderEventAny) {
        todo!();
    }

    fn determine_oms_type(&self, fill: OrderFilled) {
        todo!();
    }

    fn determine_position_id(&self, fill: OrderFilled, oms_type: OmsType) {
        todo!();
    }

    fn determine_hedging_position_id(&self, fill: OrderFilled) {
        todo!();
    }

    fn determine_netting_position_id(&self, fill: OrderFilled) {
        todo!();
    }

    fn apply_event_to_order(&self, order: &OrderAny, event: OrderEventAny) {
        todo!();
    }

    fn handle_order_fill(&self, order: &OrderAny, fill: OrderFilled, oms_type: OmsType) {
        todo!();
    }

    fn open_position(
        &self,
        instrument: InstrumentAny,
        position: &Position,
        fill: OrderFilled,
        oms_type: OmsType,
    ) {
        todo!();
    }

    fn update_position(
        &self,
        instrument: InstrumentAny,
        position: &Position,
        fill: OrderFilled,
        oms_type: OmsType,
    ) {
        todo!();
    }

    fn will_flip_position(&self, position: &Position, fill: OrderFilled) {
        todo!();
    }

    fn flip_position(
        &self,
        instrument: InstrumentAny,
        position: &Position,
        fill: OrderFilled,
        oms_type: OmsType,
    ) {
        todo!();
    }

    fn publish_order_snapshot(&self, order: &OrderAny) {
        todo!();
    }

    fn publish_position_snapshot(&self, position: &Position) {
        todo!();
    }

    // -- INTERNAL ------------------------------------------------------------

    fn set_position_id_counts(&self) {
        todo!();
    }

    fn last_px_for_conversion(&self, instrument_id: InstrumentId, side: OrderSide) {
        todo!();
    }

    fn set_order_base_qty(&self, order: &OrderAny, base_qty: Quantity) {
        todo!();
    }

    fn deny_order(&self, order: &OrderAny, reason: &str) {
        todo!();
    }
}
