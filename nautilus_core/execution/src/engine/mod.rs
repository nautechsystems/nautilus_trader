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
//!
//! The execution engines primary responsibility is to orchestrate interactions
//! between the `ExecutionClient` instances, and the rest of the platform. This
//! includes sending commands to, and receiving events from, the trading venue
//! endpoints via its registered execution clients.

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

pub mod config;

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
    time::SystemTime,
};

use config::ExecutionEngineConfig;
use nautilus_common::{
    cache::Cache,
    clock::Clock,
    generators::position_id::PositionIdGenerator,
    logging::{CMD, EVT, RECV},
    msgbus::MessageBus,
};
use nautilus_core::uuid::UUID4;
use nautilus_model::{
    enums::{OmsType, OrderSide, PositionSide},
    events::{OrderEvent, OrderEventAny, OrderFilled},
    identifiers::{ClientId, InstrumentId, PositionId, StrategyId, Venue},
    instruments::InstrumentAny,
    orders::OrderAny,
    position::Position,
    types::{Money, Price, Quantity},
};

use crate::{
    client::ExecutionClient,
    messages::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, ModifyOrder, QueryOrder, SubmitOrder,
        SubmitOrderList, TradingCommand,
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

    pub fn load_cache(&mut self) -> anyhow::Result<()> {
        let ts = SystemTime::now();

        self.cache
            .borrow_mut()
            .cache_general()
            .and_then(|()| self.cache.borrow_mut().cache_currencies())
            .and_then(|()| self.cache.borrow_mut().cache_instruments())
            .and_then(|()| self.cache.borrow_mut().cache_accounts())
            .and_then(|()| self.cache.borrow_mut().cache_orders())
            .and_then(|()| self.cache.borrow_mut().cache_positions())
            .map_err(|e| {
                log::error!("Failed to load cache: {}", e);
                e
            })?;

        self.cache.borrow_mut().build_index();
        let _ = self.cache.borrow_mut().check_integrity();
        self.set_position_id_counts();

        log::info!(
            "Loaded cache in {}ms",
            SystemTime::now()
                .duration_since(ts)
                .map_err(|e| anyhow::anyhow!("Failed to calculate duration: {}", e))?
                .as_millis()
        );

        Ok(())
    }

    pub fn flush_db(&self) {
        self.cache.borrow_mut().flush_db();
    }

    pub fn process(&mut self, event: &OrderEventAny) {
        self.handle_event(event);
    }

    pub fn execute(&self, command: TradingCommand) {
        self.execute_command(command);
    }

    // -- COMMAND HANDLERS ----------------------------------------------------

    fn execute_command(&self, command: TradingCommand) {
        if self.config.debug {
            log::debug!("{}{} {command:?}", RECV, CMD);
        }

        let client = if let Some(client) = self
            .clients
            .get(&command.client_id())
            .or_else(|| {
                self.routing_map
                    .get(&command.instrument_id().venue)
                    .and_then(|client_id| self.clients.get(client_id))
            })
            .or(self.default_client.as_ref())
        {
            client
        } else {
            log::error!(
                "No execution client found for command: client_id={:?}, venue={}, command={:?}",
                command.client_id(),
                command.instrument_id().venue,
                command
            );
            return;
        };

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
        let order = command.order.clone();

        // Check if the order exists in the cache
        if !self.cache.borrow().order_exists(&order.client_order_id()) {
            if let Err(e) = self.cache.borrow_mut().add_order(
                order.clone(),
                command.position_id,
                Some(command.client_id),
                true,
            ) {
                log::error!("Error adding order to cache: {}", e);
                return;
            }

            if self.config.snapshot_orders {
                self.create_order_state_snapshot(&order);
            }
        }

        let cache = self.cache.borrow();
        let instrument = if let Some(instrument) = cache.instrument(&order.instrument_id()) {
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
        if !instrument.is_inverse() && order.is_quote_quantity() {
            if let Some(last_px) =
                self.last_px_for_conversion(&order.instrument_id(), order.order_side())
            {
                let base_qty = instrument.get_base_quantity(order.quantity(), last_px);
                self.set_order_base_qty(&order, base_qty);
            } else {
                self.deny_order(
                    &order,
                    &format!("no-price-to-convert-quote-qty {}", order.instrument_id()),
                );
                return; // Denied
            }
        }

        // Send the order to the execution client
        if let Err(e) = client.submit_order(command) {
            log::error!("Error submitting order to client: {}", e);
            self.deny_order(&order, &format!("failed-to-submit-order-to-client: {e}"));
        }
    }

    fn handle_submit_order_list(&self, client: &ExecutionClient, command: SubmitOrderList) {
        let orders = command.order_list.orders.clone();

        // Cache orders
        let mut cache = self.cache.borrow_mut();
        for order in &orders {
            if !cache.order_exists(&order.client_order_id()) {
                if let Err(e) = cache.add_order(
                    order.clone(),
                    command.position_id,
                    Some(command.client_id),
                    true,
                ) {
                    log::error!("Error adding order to cache: {}", e);
                    return;
                }

                if self.config.snapshot_orders {
                    self.create_order_state_snapshot(order);
                }
            }
        }
        drop(cache);

        // Get instrument from cache
        let cache = self.cache.borrow();
        let instrument = if let Some(instrument) = cache.instrument(&command.instrument_id) {
            instrument
        } else {
            log::error!(
                "Cannot handle submit order list: no instrument found for {}, {}",
                command.instrument_id,
                &command
            );
            return;
        };

        // Check if converting quote quantity
        if !instrument.is_inverse() && command.order_list.orders[0].is_quote_quantity() {
            let mut quote_qty = None;
            let mut last_px = None;

            for order in &command.order_list.orders {
                if !order.is_quote_quantity() {
                    continue; // Base quantity already set
                }

                if Some(order.quantity()) != quote_qty {
                    last_px =
                        self.last_px_for_conversion(&order.instrument_id(), order.order_side());
                    quote_qty = Some(order.quantity());
                }

                if last_px.is_none() {
                    for order in &command.order_list.orders {
                        self.deny_order(
                            order,
                            &format!("no-price-to-convert-quote-qty {}", order.instrument_id()),
                        );
                    }
                    return; // Denied
                }

                let base_qty = instrument.get_base_quantity(order.quantity(), last_px.unwrap());
                self.set_order_base_qty(order, base_qty);
            }
        }

        // Send to execution client
        if let Err(e) = client.submit_order_list(command) {
            log::error!("Error submitting order list to client: {}", e);
            for order in &orders {
                self.deny_order(
                    order,
                    &format!("failed-to-submit-order-list-to-client: {e}"),
                );
            }
        }
    }

    fn handle_modify_order(&self, client: &ExecutionClient, command: ModifyOrder) {
        if let Err(e) = client.modify_order(command) {
            log::error!("Error modifying order: {}", e);
        }
    }

    fn handle_cancel_order(&self, client: &ExecutionClient, command: CancelOrder) {
        if let Err(e) = client.cancel_order(command) {
            log::error!("Error canceling order: {}", e);
        }
    }

    pub const fn handle_cancel_all_orders(
        &self,
        _client: &ExecutionClient,
        _command: CancelAllOrders,
    ) {
        // TODO
        // client.cancel_all_orders(command);
    }

    fn handle_batch_cancel_orders(&self, client: &ExecutionClient, command: BatchCancelOrders) {
        if let Err(e) = client.batch_cancel_orders(command) {
            log::error!("Error batch canceling orders: {}", e);
        }
    }

    fn handle_query_order(&self, client: &ExecutionClient, command: QueryOrder) {
        if let Err(e) = client.query_order(command) {
            log::error!("Error querying order: {}", e);
        }
    }

    fn create_order_state_snapshot(&self, order: &OrderAny) {
        let mut msgbus = self.msgbus.borrow_mut();
        let topic = msgbus
            .switchboard
            .get_order_snapshots_topic(order.client_order_id());
        msgbus.publish(&topic, order);
        todo!("??")
    }

    // -- EVENT HANDLERS ----------------------------------------------------

    fn handle_event(&mut self, event: &OrderEventAny) {
        if self.config.debug {
            log::debug!("{}{} {event:?}", RECV, EVT);
        }

        let client_order_id = event.client_order_id();
        let borrowed_cache = self.cache.borrow();
        let mut order = if let Some(order) = borrowed_cache.order(&client_order_id) {
            order.clone()
        } else {
            log::warn!(
                "Order with {} not found in the cache to apply {}",
                event.client_order_id(),
                event
            );

            // Try to find order by venue order ID if available
            let venue_order_id = if let Some(id) = event.venue_order_id() {
                id
            } else {
                log::error!(
                    "Cannot apply event to any order: {} not found in the cache with no VenueOrderId",
                    event.client_order_id()
                );
                return;
            };

            // Look up client order ID from venue order ID
            let client_order_id = if let Some(id) = borrowed_cache.client_order_id(&venue_order_id)
            {
                id
            } else {
                log::error!(
                    "Cannot apply event to any order: {} and {} not found in the cache",
                    event.client_order_id(),
                    venue_order_id
                );
                return;
            };

            // Get order using found client order ID
            if let Some(order) = borrowed_cache.order(client_order_id) {
                log::info!("Order with {} was found in the cache", client_order_id);
                order.clone()
            } else {
                log::error!(
                    "Cannot apply event to any order: {} and {} not found in cache",
                    client_order_id,
                    venue_order_id
                );
                return;
            }
        };
        drop(borrowed_cache); // Drop immutable borrow before mutable operations

        match event {
            OrderEventAny::Filled(order_filled) => {
                let oms_type = self.determine_oms_type(order_filled);
                let position_id = self.determine_position_id(*order_filled, oms_type);

                // Create a new fill with the determined position ID
                let mut order_filled = *order_filled;
                if order_filled.position_id.is_none() {
                    order_filled.position_id = Some(position_id);
                }

                self.apply_event_to_order(&mut order, OrderEventAny::Filled(order_filled));
                self.handle_order_fill(&order, order_filled, oms_type);
            }
            _ => {
                self.apply_event_to_order(&mut order, event.clone());
            }
        }
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
        let borrowed_cache = self.cache.borrow();
        let instrument = if let Some(instrument) = borrowed_cache.instrument(&fill.instrument_id) {
            instrument
        } else {
            log::error!(
                "Cannot handle order fill: no instrument found for {}, {}",
                fill.instrument_id,
                fill
            );
            return;
        };

        let account = if let Some(account_id) = borrowed_cache.account(&fill.account_id) {
            account_id
        } else {
            log::error!(
                "Cannot handle order fill: no account found for {}, {}",
                fill.instrument_id.venue,
                fill
            );
            return;
        };

        let position_id = if let Some(position_id) = fill.position_id {
            position_id
        } else {
            log::error!(
                "Cannot handle order fill: no position ID found for fill {}",
                fill
            );
            return;
        };

        // let position = borrowed_cache.position(&position_id);
        // if position.is_none() || position.unwrap().is_closed() {
        //     position = self.open_position(instrument, position_id, fill, oms_type)
        // }
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

    fn will_flip_position(&self, position: &Position, fill: OrderFilled) -> bool {
        position.is_opposite_side(fill.order_side) && (fill.last_qty.raw > position.quantity.raw)
    }

    fn flip_position(
        &mut self,
        instrument: InstrumentAny,
        position: &mut Position,
        fill: OrderFilled,
        oms_type: OmsType,
    ) {
        let difference = match position.side {
            PositionSide::Long => Quantity::from_raw(
                fill.last_qty.raw - position.quantity.raw,
                position.size_precision,
            ),
            PositionSide::Short => Quantity::from_raw(
                position.quantity.raw - fill.last_qty.raw,
                position.size_precision,
            ),
            _ => fill.last_qty,
        };

        // Split commission between two positions
        let fill_percent = position.quantity.as_f64() / fill.last_qty.as_f64();
        let (commission1, commission2) = if let Some(commission) = fill.commission {
            let commission_currency = commission.currency;
            let commission1 = Money::new(commission * fill_percent, commission_currency);
            let commission2 = commission - commission1;
            (Some(commission1), Some(commission2))
        } else {
            log::error!("Commission is not available.");
            (None, None)
        };

        let mut fill_split1: Option<OrderFilled> = None;
        if position.is_open() {
            fill_split1 = Some(OrderFilled {
                trader_id: fill.trader_id,
                strategy_id: fill.strategy_id,
                instrument_id: fill.instrument_id,
                client_order_id: fill.client_order_id,
                venue_order_id: fill.venue_order_id,
                account_id: fill.account_id,
                trade_id: fill.trade_id,
                position_id: fill.position_id,
                order_side: fill.order_side,
                order_type: fill.order_type,
                last_qty: position.quantity,
                last_px: fill.last_px,
                currency: fill.currency,
                commission: commission1,
                liquidity_side: fill.liquidity_side,
                event_id: fill.id(),
                ts_event: fill.ts_event,
                ts_init: fill.ts_init,
                reconciliation: fill.reconciliation,
            });

            // Close original position
            // todo: will not panic
            self.update_position(instrument, position, fill_split1.unwrap(), oms_type);
        }

        // Guard against flipping a position with a zero fill size
        if difference.raw == 0 {
            log::warn!(
                "Zero fill size during position flip calculation, this could be caused by a mismatch between instrument `size_precision` and a quantity `size_precision`"
            );
            return;
        }

        let position_id_flip = if oms_type == OmsType::Hedging {
            if let Some(position_id) = fill.position_id {
                if position_id.is_virtual() {
                    // Generate new position ID for flipped virtual position
                    Some(self.pos_id_generator.generate(fill.strategy_id, true))
                } else {
                    Some(position_id)
                }
            } else {
                None
            }
        } else {
            fill.position_id
        };

        // Generate order fill for flipped position
        let fill_split2 = OrderFilled {
            trader_id: fill.trader_id,
            strategy_id: fill.strategy_id,
            instrument_id: fill.instrument_id,
            client_order_id: fill.client_order_id,
            venue_order_id: fill.venue_order_id,
            account_id: fill.account_id,
            trade_id: fill.trade_id,
            position_id: position_id_flip,
            order_side: fill.order_side,
            order_type: fill.order_type,
            last_qty: difference, // Fill difference from original as above
            last_px: fill.last_px,
            currency: fill.currency,
            commission: commission2,
            liquidity_side: fill.liquidity_side,
            event_id: UUID4::new(), // New event ID
            ts_event: fill.ts_event,
            ts_init: fill.ts_init,
            reconciliation: fill.reconciliation,
        };

        if oms_type == OmsType::Hedging {
            if let Some(position_id) = fill.position_id {
                if position_id.is_virtual() {
                    log::warn!("Closing position {:?}", fill_split1);
                    log::warn!("Flipping position {:?}", fill_split2);
                }
            }
        }

        // Open flipped position
        // self.open_position(instrument, None, fill_split2, oms_type);
        todo!()
    }

    fn publish_order_snapshot(&self, order: &OrderAny) {
        if self.config.debug {
            log::debug!("Creating order state snapshot for {}", order);
        }

        // if self.cache.borrow().has_backing

        // if self.msgbus.borrow().has_backing && self.msgbus.borrow().serializer
        todo!();
    }

    fn publish_position_snapshot(&self, position: &Position) {
        if self.config.debug {
            log::debug!("Creating position state snapshot for {}", position);
        }

        // let unrealized_pnl = self.cache.borrow().pnl

        todo!();
    }

    // -- INTERNAL ------------------------------------------------------------

    fn set_position_id_counts(&mut self) {
        // For the internal position ID generator
        let borrowed_cache = self.cache.borrow();
        let positions = borrowed_cache.positions(None, None, None, None);

        // Count positions per instrument_id using a HashMap
        let mut counts: HashMap<StrategyId, usize> = HashMap::new();

        for position in positions {
            *counts.entry(position.strategy_id).or_insert(0) += 1;
        }

        self.pos_id_generator.reset();

        for (strategy_id, count) in counts {
            self.pos_id_generator.set_count(count, strategy_id);
            log::info!("Set PositionId count for {} to {}", strategy_id, count);
        }

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
