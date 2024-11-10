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

pub mod config;

#[cfg(test)]
mod tests;

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
    events::order::{filled::OrderFilled, OrderEvent, OrderEventAny},
    identifiers::{ClientId, InstrumentId, PositionId, StrategyId, Venue},
    instruments::any::InstrumentAny,
    orders::any::OrderAny,
    position::Position,
    types::{price::Price, quantity::Quantity},
};

use crate::{
    client::ExecutionClient,
    messages::{
        cancel::CancelOrder, cancel_all::CancelAllOrders, cancel_batch::BatchCancelOrders,
        modify::ModifyOrder, query::QueryOrder, submit::SubmitOrder, submit_list::SubmitOrderList,
        TradingCommand,
    },
};

pub struct ExecutionEngine {
    clock: Rc<RefCell<dyn Clock>>,
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

impl ExecutionEngine {
    pub fn new(
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
        msgbus: Rc<RefCell<MessageBus>>,
        config: ExecutionEngineConfig,
    ) -> Self {
        let trader_id = msgbus.borrow().trader_id;
        Self {
            clock: clock.clone(),
            cache,
            msgbus,
            clients: HashMap::new(),
            default_client: None,
            routing_map: HashMap::new(),
            oms_overrides: HashMap::new(),
            external_order_claims: HashMap::new(),
            pos_id_generator: PositionIdGenerator::new(trader_id, clock),
            config,
        }
    }

    #[must_use]
    pub fn position_id_count(&self, strategy_id: StrategyId) -> usize {
        self.pos_id_generator.count(strategy_id)
    }

    #[must_use]
    pub fn check_integrity(&self) -> bool {
        self.cache.borrow_mut().check_integrity()
    }

    #[must_use]
    pub fn check_connected(&self) -> bool {
        self.clients.values().all(|c| c.is_connected)
    }

    #[must_use]
    pub fn check_disconnected(&self) -> bool {
        self.clients.values().all(|c| !c.is_connected)
    }

    #[must_use]
    pub fn check_residuals(&self) -> bool {
        self.cache.borrow().check_residuals()
    }

    #[must_use]
    pub fn get_external_order_claims_instruments(&self) -> HashSet<InstrumentId> {
        self.external_order_claims.keys().copied().collect()
    }

    // -- REGISTRATION --------------------------------------------------------

    pub fn register_client(&mut self, client: ExecutionClient) -> anyhow::Result<()> {
        if self.clients.contains_key(&client.client_id) {
            anyhow::bail!("Client already registered with ID {}", client.client_id);
        }

        // If client has venue, register routing
        self.routing_map.insert(client.venue, client.client_id);

        log::info!("Registered client {}", client.client_id);
        self.clients.insert(client.client_id, client);
        Ok(())
    }

    pub fn register_default_client(&mut self, client: ExecutionClient) {
        log::info!("Registered default client {}", client.client_id);
        self.default_client = Some(client);
    }

    pub fn register_venue_routing(
        &mut self,
        client_id: ClientId,
        venue: Venue,
    ) -> anyhow::Result<()> {
        if !self.clients.contains_key(&client_id) {
            anyhow::bail!("No client registered with ID {client_id}");
        }

        self.routing_map.insert(venue, client_id);
        log::info!("Set client {client_id} routing for {venue}");
        Ok(())
    }

    // TODO: Implement `Strategy`
    // pub fn register_external_order_claims(&mut self, strategy: Strategy) -> anyhow::Result<()> {
    //     todo!();
    // }

    pub fn deregister_client(&mut self, client_id: ClientId) -> anyhow::Result<()> {
        if self.clients.remove(&client_id).is_some() {
            // Remove from routing map if present
            self.routing_map
                .retain(|_, mapped_id| mapped_id != &client_id);
            log::info!("Deregistered client {client_id}");
            Ok(())
        } else {
            anyhow::bail!("No client registered with ID {client_id}")
        }
    }

    // -- COMMANDS ------------------------------------------------------------

    pub fn load_cache(&self) {
        todo!();
    }

    pub fn flush_db(&self) {
        todo!();
    }

    pub fn process(&self, event: &OrderEventAny) {
        todo!()
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
        let order = &command.order;

        if !self.cache.borrow().order_exists(&order.client_order_id()) {
            // Cache order
            self.cache
                .borrow_mut()
                .add_order(
                    order.clone(),
                    command.position_id,
                    Some(command.client_id),
                    true,
                )
                .unwrap();

            if self.config.snapshot_orders {
                self.create_order_state_snapshot(order);
            }
        }

        let instrument =
            if let Some(instrument) = self.cache.borrow().instrument(&order.instrument_id()) {
                instrument
            } else {
                log::error!(
                    "Cannot handle submit order: no instrument found for {}, {}",
                    order.instrument_id(),
                    &command
                );
                return;
            };

        // Handle quote quantity conversion
        // TODO: implemnent is_quote_quantity
        // if !instrument.is_inverse() && order.is_quote_quantity() {
        //     let last_px = self.last_px_for_conversion(&order.instrument_id(), order.order_side());

        //     if last_px.is_none() {
        //         self.deny_order(
        //             &order,
        //             &format!("no-price-to-convert-quote-qty {}", order.instrument_id()),
        //         );
        //         return;
        //     }

        //     // TODO: convert f64 to Price
        //     let base_qty = instrument.get_base_quantity(order.quantity(), last_px.unwrap().into());
        //     self.set_order_base_qty(&order, base_qty);
        // }

        // // Send to execution client
        // client.submit_order(command);
    }

    pub fn handle_submit_order_list(&self, client: &ExecutionClient, command: SubmitOrderList) {
        for order in &command.order_list.orders {
            if !self.cache.borrow().order_exists(&order.client_order_id()) {
                self.cache
                    .borrow_mut()
                    .add_order(
                        order.clone(),
                        command.position_id,
                        Some(command.client_id),
                        true,
                    )
                    .unwrap();

                if self.config.snapshot_orders {
                    self.create_order_state_snapshot(order);
                }
            }
        }

        // Send to execution client
        client.submit_order_list(command).unwrap();
    }

    fn handle_modify_order(&self, client: &ExecutionClient, command: ModifyOrder) {
        todo!();
    }

    fn handle_cancel_order(&self, client: &ExecutionClient, command: CancelOrder) {
        todo!();
    }

    pub const fn handle_cancel_all_orders(
        &self,
        client: &ExecutionClient,
        command: CancelAllOrders,
    ) {
        // TODO
        // client.cancel_all_orders(command);
    }
    fn handle_batch_cancel_orders(&self, client: &ExecutionClient, command: BatchCancelOrders) {
        todo!();
    }

    fn handle_query_order(&self, client: &ExecutionClient, command: QueryOrder) {
        todo!();
    }

    // TODO
    fn create_order_state_snapshot(&self, order: &OrderAny) {
        todo!()
        // let mut msgbus = self.msgbus.borrow_mut();
        // let topic = msgbus
        //     .switchboard
        //     .get_order_snapshot_topic(order.client_order_id());
        // msgbus.publish(&topic, order);
    }

    // -- EVENT HANDLERS ----------------------------------------------------

    fn handle_event(&self, event: OrderEventAny) {
        todo!()
    }

    fn determine_oms_type(&self, fill: &OrderFilled) -> OmsType {
        // Check for strategy OMS override
        if let Some(oms_type) = self.oms_overrides.get(&fill.strategy_id) {
            return *oms_type;
        }

        // Use native venue OMS
        if let Some(client_id) = self.routing_map.get(&fill.instrument_id.venue) {
            if let Some(client) = self.clients.get(client_id) {
                return client.oms_type;
            }
        }

        if let Some(client) = &self.default_client {
            return client.oms_type;
        }

        OmsType::Netting // Default fallback
    }

    fn determine_position_id(&mut self, fill: OrderFilled, oms_type: OmsType) -> PositionId {
        match oms_type {
            OmsType::Hedging => self.determine_hedging_position_id(fill),
            OmsType::Netting => self.determine_netting_position_id(fill),
            _ => self.determine_netting_position_id(fill), // Default to netting
        }
    }

    fn determine_hedging_position_id(&mut self, fill: OrderFilled) -> PositionId {
        // Check if position ID already exists
        if let Some(position_id) = fill.position_id {
            if self.config.debug {
                log::debug!("Already had a position ID of: {}", position_id);
            }
            return position_id;
        }

        // Check for order
        let cache = self.cache.borrow();
        let order = match cache.order(&fill.client_order_id()) {
            Some(o) => o,
            None => {
                panic!(
                    "Order for {} not found to determine position ID",
                    fill.client_order_id()
                );
            }
        };

        // Check execution spawn orders
        if let Some(spawn_id) = order.exec_spawn_id() {
            let cache = self.cache.borrow();
            let spawn_orders = cache.orders_for_exec_spawn(&spawn_id);
            for spawned_order in spawn_orders {
                if let Some(pos_id) = spawned_order.position_id() {
                    if self.config.debug {
                        log::debug!("Found spawned {} for {}", pos_id, fill.client_order_id());
                    }
                    return pos_id;
                }
            }
        }

        // Generate new position ID
        let position_id = self.pos_id_generator.generate(fill.strategy_id, false);
        if self.config.debug {
            log::debug!("Generated {} for {}", position_id, fill.client_order_id());
        }
        position_id
    }

    fn determine_netting_position_id(&self, fill: OrderFilled) -> PositionId {
        PositionId::new(format!("{}-{}", fill.instrument_id, fill.strategy_id))
    }

    fn apply_event_to_order(&self, order: &mut OrderAny, event: OrderEventAny) {
        match order.apply(event.clone()) {
            Ok(()) => {
                self.cache.borrow_mut().update_order(order).unwrap();
            }
            Err(e) => {
                log::error!("Error applying event: {}, did not apply {}", e, event);
            }
        }
    }

    fn handle_order_fill(&self, order: &OrderAny, fill: OrderFilled, oms_type: OmsType) {
        todo!();
    }

    fn open_position(
        &self,
        instrument: InstrumentAny,
        position_id: PositionId,
        fill: OrderFilled,
        oms_type: OmsType,
    ) -> anyhow::Result<()> {
        let position = Position::new(&instrument, fill);
        self.cache.borrow_mut().add_position(position, oms_type)
    }

    fn update_position(
        &self,
        instrument: InstrumentAny,
        position: &mut Position,
        fill: OrderFilled,
        oms_type: OmsType,
    ) {
        position.apply(&fill);
    }

    fn will_flip_position(&self, position: &Position, fill: OrderFilled) {
        todo!();
    }

    fn flip_position(
        &self,
        instrument: InstrumentAny,
        position: &mut Position,
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

    fn last_px_for_conversion(
        &self,
        instrument_id: &InstrumentId,
        side: OrderSide,
    ) -> Option<Price> {
        let cache = self.cache.borrow();

        // Try to get last trade price
        if let Some(trade) = cache.trade(instrument_id) {
            return Some(trade.price);
        }

        // Fall back to quote if available
        if let Some(quote) = cache.quote(instrument_id) {
            match side {
                OrderSide::Buy => Some(quote.ask_price),
                OrderSide::Sell => Some(quote.bid_price),
                OrderSide::NoOrderSide => None,
            }
        } else {
            None
        }
    }

    fn set_order_base_qty(&self, order: &OrderAny, quantity: Quantity) {
        // Implementation depends on your order type system
        // This is a placeholder for the actual implementation
        log::debug!(
            "Setting base quantity {} for order {}",
            quantity,
            order.client_order_id()
        );
    }

    fn deny_order(&self, order: &OrderAny, reason: &str) {
        log::error!(
            "Order denied: {reason}, order ID: {}",
            order.client_order_id()
        );
        todo!()
    }
}
