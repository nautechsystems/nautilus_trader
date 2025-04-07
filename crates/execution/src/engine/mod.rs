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

//! Provides a generic `ExecutionEngine` for all environments.
//!
//! The execution engines primary responsibility is to orchestrate interactions
//! between the `ExecutionClient` instances, and the rest of the platform. This
//! includes sending commands to, and receiving events from, the trading venue
//! endpoints via its registered execution clients.

pub mod config;

use std::{
    cell::{RefCell, RefMut},
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
    msgbus::{
        self, get_message_bus,
        switchboard::{self},
    },
};
use nautilus_core::UUID4;
use nautilus_model::{
    enums::{ContingencyType, OmsType, OrderSide, PositionSide},
    events::{
        OrderDenied, OrderEvent, OrderEventAny, OrderFilled, PositionChanged, PositionClosed,
        PositionOpened,
    },
    identifiers::{ClientId, InstrumentId, PositionId, StrategyId, Venue},
    instruments::{Instrument, InstrumentAny},
    orderbook::own::{OwnOrderBook, should_handle_own_book_order},
    orders::{Order, OrderAny, OrderError},
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
    clients: HashMap<ClientId, Rc<dyn ExecutionClient>>,
    default_client: Option<Rc<dyn ExecutionClient>>,
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
        config: Option<ExecutionEngineConfig>,
    ) -> Self {
        let trader_id = get_message_bus().borrow().trader_id;
        Self {
            clock: clock.clone(),
            cache,
            clients: HashMap::new(),
            default_client: None,
            routing_map: HashMap::new(),
            oms_overrides: HashMap::new(),
            external_order_claims: HashMap::new(),
            pos_id_generator: PositionIdGenerator::new(trader_id, clock),
            config: config.unwrap_or_default(),
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
        self.clients.values().all(|c| c.is_connected())
    }

    #[must_use]
    pub fn check_disconnected(&self) -> bool {
        self.clients.values().all(|c| !c.is_connected())
    }

    #[must_use]
    pub fn check_residuals(&self) -> bool {
        self.cache.borrow().check_residuals()
    }

    #[must_use]
    pub fn get_external_order_claims_instruments(&self) -> HashSet<InstrumentId> {
        self.external_order_claims.keys().copied().collect()
    }

    // -- REGISTRATION ----------------------------------------------------------------------------

    pub fn register_client(&mut self, client: Rc<dyn ExecutionClient>) -> anyhow::Result<()> {
        if self.clients.contains_key(&client.client_id()) {
            anyhow::bail!("Client already registered with ID {}", client.client_id());
        }

        // If client has venue, register routing
        self.routing_map.insert(client.venue(), client.client_id());

        log::info!("Registered client {}", client.client_id());
        self.clients.insert(client.client_id(), client);
        Ok(())
    }

    pub fn register_default_client(&mut self, client: Rc<dyn ExecutionClient>) {
        log::info!("Registered default client {}", client.client_id());
        self.default_client = Some(client);
    }

    #[must_use]
    pub fn get_client(&self, client_id: &ClientId) -> Option<Rc<dyn ExecutionClient>> {
        self.clients.get(client_id).cloned()
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

    // -- COMMANDS --------------------------------------------------------------------------------

    #[allow(clippy::await_holding_refcell_ref)]
    pub async fn load_cache(&mut self) -> anyhow::Result<()> {
        let ts = SystemTime::now();

        {
            let mut cache = self.cache.borrow_mut();
            cache.clear_index();
            cache.cache_general()?;
            self.cache.borrow_mut().cache_all().await?;
            cache.build_index();
            let _ = cache.check_integrity();

            if self.config.manage_own_order_books {
                for order in cache.orders(None, None, None, None) {
                    if order.is_closed() || !should_handle_own_book_order(order) {
                        continue;
                    }
                    let mut own_book = self.get_or_init_own_order_book(&order.instrument_id());
                    own_book.add(order.to_own_book_order());
                }
            }
        }

        self.set_position_id_counts();

        log::info!(
            "Loaded cache in {}ms",
            SystemTime::now()
                .duration_since(ts)
                .map_err(|e| anyhow::anyhow!("Failed to calculate duration: {e}"))?
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

    // -- COMMAND HANDLERS ------------------------------------------------------------------------

    fn execute_command(&self, command: TradingCommand) {
        if self.config.debug {
            log::debug!("{RECV}{CMD} {command:?}");
        }

        let client: Rc<dyn ExecutionClient> = if let Some(client) = self
            .clients
            .get(&command.client_id())
            .or_else(|| {
                self.routing_map
                    .get(&command.instrument_id().venue)
                    .and_then(|client_id| self.clients.get(client_id))
            })
            .or(self.default_client.as_ref())
        {
            client.clone()
        } else {
            log::error!(
                "No execution client found for command: client_id={:?}, venue={}, command={command:?}",
                command.client_id(),
                command.instrument_id().venue,
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

    fn handle_submit_order(&self, client: Rc<dyn ExecutionClient>, command: SubmitOrder) {
        let mut order = command.order.clone();
        let client_order_id = order.client_order_id();
        let instrument_id = order.instrument_id();

        // Check if the order exists in the cache
        if !self.cache.borrow().order_exists(&client_order_id) {
            // Add order to cache in a separate scope to drop the mutable borrow
            {
                let mut cache = self.cache.borrow_mut();
                if let Err(e) = cache.add_order(
                    order.clone(),
                    command.position_id,
                    Some(command.client_id),
                    true,
                ) {
                    log::error!("Error adding order to cache: {e}");
                    return;
                }
            }

            if self.config.snapshot_orders {
                self.create_order_state_snapshot(&order);
            }
        }

        // Get instrument in a separate scope to manage borrows
        let instrument = {
            let cache = self.cache.borrow();
            if let Some(instrument) = cache.instrument(&instrument_id) {
                instrument.clone()
            } else {
                log::error!(
                    "Cannot handle submit order: no instrument found for {instrument_id}, {command}",
                );
                return;
            }
        };

        // Handle quote quantity conversion
        if !instrument.is_inverse() && order.is_quote_quantity() {
            let last_px = self.last_px_for_conversion(&instrument_id, order.order_side());

            if let Some(price) = last_px {
                let base_qty = instrument.get_base_quantity(order.quantity(), price);
                self.set_order_base_qty(&mut order, base_qty);
            } else {
                self.deny_order(
                    &order,
                    &format!("no-price-to-convert-quote-qty {instrument_id}"),
                );
                return;
            }
        }

        if self.config.manage_own_order_books && should_handle_own_book_order(&order) {
            let mut own_book = self.get_or_init_own_order_book(&order.instrument_id());
            own_book.add(order.to_own_book_order());
        }

        // Send the order to the execution client
        // TODO: Avoid this clone
        if let Err(e) = client.submit_order(command.clone()) {
            log::error!("Error submitting order to client: {e}");
            self.deny_order(
                &command.order,
                &format!("failed-to-submit-order-to-client: {e}"),
            );
        }
    }

    fn handle_submit_order_list(
        &self,
        client: Rc<dyn ExecutionClient>,
        mut command: SubmitOrderList,
    ) {
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
                    log::error!("Error adding order to cache: {e}");
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
                "Cannot handle submit order list: no instrument found for {}, {command}",
                command.instrument_id,
            );
            return;
        };

        // Check if converting quote quantity
        if !instrument.is_inverse() && command.order_list.orders[0].is_quote_quantity() {
            let mut quote_qty = None;
            let mut last_px = None;

            for order in &mut command.order_list.orders {
                if !order.is_quote_quantity() {
                    continue; // Base quantity already set
                }

                if Some(order.quantity()) != quote_qty {
                    last_px =
                        self.last_px_for_conversion(&order.instrument_id(), order.order_side());
                    quote_qty = Some(order.quantity());
                }

                if let Some(px) = last_px {
                    let base_qty = instrument.get_base_quantity(order.quantity(), px);
                    self.set_order_base_qty(order, base_qty);
                } else {
                    for order in &command.order_list.orders {
                        self.deny_order(
                            order,
                            &format!("no-price-to-convert-quote-qty {}", order.instrument_id()),
                        );
                    }
                    return; // Denied
                }
            }
        }

        if self.config.manage_own_order_books {
            let mut own_book = self.get_or_init_own_order_book(&command.instrument_id);
            for order in &command.order_list.orders {
                if should_handle_own_book_order(order) {
                    own_book.add(order.to_own_book_order());
                }
            }
        }

        // Send to execution client
        if let Err(e) = client.submit_order_list(command) {
            log::error!("Error submitting order list to client: {e}");
            for order in &orders {
                self.deny_order(
                    order,
                    &format!("failed-to-submit-order-list-to-client: {e}"),
                );
            }
        }
    }

    fn handle_modify_order(&self, client: Rc<dyn ExecutionClient>, command: ModifyOrder) {
        if let Err(e) = client.modify_order(command) {
            log::error!("Error modifying order: {e}");
        }
    }

    fn handle_cancel_order(&self, client: Rc<dyn ExecutionClient>, command: CancelOrder) {
        if let Err(e) = client.cancel_order(command) {
            log::error!("Error canceling order: {e}");
        }
    }

    fn handle_cancel_all_orders(&self, client: Rc<dyn ExecutionClient>, command: CancelAllOrders) {
        if let Err(e) = client.cancel_all_orders(command) {
            log::error!("Error canceling all orders: {e}");
        }
    }

    fn handle_batch_cancel_orders(
        &self,
        client: Rc<dyn ExecutionClient>,
        command: BatchCancelOrders,
    ) {
        if let Err(e) = client.batch_cancel_orders(command) {
            log::error!("Error batch canceling orders: {e}");
        }
    }

    fn handle_query_order(&self, client: Rc<dyn ExecutionClient>, command: QueryOrder) {
        if let Err(e) = client.query_order(command) {
            log::error!("Error querying order: {e}");
        }
    }

    fn create_order_state_snapshot(&self, order: &OrderAny) {
        if self.config.debug {
            log::debug!("Creating order state snapshot for {order}");
        }

        if self.cache.borrow().has_backing() {
            if let Err(e) = self.cache.borrow().snapshot_order_state(order) {
                log::error!("Failed to snapshot order state: {e}");
                return;
            }
        }

        if get_message_bus().borrow().has_backing {
            let topic = switchboard::get_order_snapshots_topic(order.client_order_id());
            msgbus::publish(&topic, order);
        }
    }

    fn create_position_state_snapshot(&self, position: &Position) {
        if self.config.debug {
            log::debug!("Creating position state snapshot for {position}");
        }

        // let mut position: Position = position.clone();
        // if let Some(pnl) = self.cache.borrow().calculate_unrealized_pnl(&position) {
        //     position.unrealized_pnl(last)
        // }

        let topic = switchboard::get_positions_snapshots_topic(position.id);
        msgbus::publish(&topic, position);
    }

    // -- EVENT HANDLERS --------------------------------------------------------------------------

    fn handle_event(&mut self, event: &OrderEventAny) {
        if self.config.debug {
            log::debug!("{RECV}{EVT} {event:?}");
        }

        let client_order_id = event.client_order_id();
        let cache = self.cache.borrow();
        let mut order = if let Some(order) = cache.order(&client_order_id) {
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
            let client_order_id = if let Some(id) = cache.client_order_id(&venue_order_id) {
                id
            } else {
                log::error!(
                    "Cannot apply event to any order: {} and {venue_order_id} not found in the cache",
                    event.client_order_id(),
                );
                return;
            };

            // Get order using found client order ID
            if let Some(order) = cache.order(client_order_id) {
                log::info!("Order with {client_order_id} was found in the cache");
                order.clone()
            } else {
                log::error!(
                    "Cannot apply event to any order: {client_order_id} and {venue_order_id} not found in cache",
                );
                return;
            }
        };

        drop(cache);
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
                return client.oms_type();
            }
        }

        if let Some(client) = &self.default_client {
            return client.oms_type();
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
                log::debug!("Already had a position ID of: {position_id}");
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
        if let Err(e) = order.apply(event.clone()) {
            match e {
                OrderError::InvalidStateTransition => {
                    log::warn!("InvalidStateTrigger: {e}, did not apply {event}");
                }
                _ => {
                    log::error!("Error applying event: {e}, did not apply {event}");
                }
            }
            return;
        }

        if let Err(e) = self.cache.borrow_mut().update_order(order) {
            log::error!("Error updating order in cache: {e}");
        }

        let topic = switchboard::get_event_orders_topic(event.strategy_id());
        msgbus::publish(&topic, order);

        if self.config.snapshot_orders {
            self.create_order_state_snapshot(order);
        }
    }

    fn handle_order_fill(&mut self, order: &OrderAny, fill: OrderFilled, oms_type: OmsType) {
        let instrument =
            if let Some(instrument) = self.cache.borrow().instrument(&fill.instrument_id) {
                instrument.clone()
            } else {
                log::error!(
                    "Cannot handle order fill: no instrument found for {}, {fill}",
                    fill.instrument_id,
                );
                return;
            };

        if self.cache.borrow().account(&fill.account_id).is_none() {
            log::error!(
                "Cannot handle order fill: no account found for {}, {fill}",
                fill.instrument_id.venue,
            );
            return;
        }

        let position_id = if let Some(position_id) = fill.position_id {
            position_id
        } else {
            log::error!("Cannot handle order fill: no position ID found for fill {fill}");
            return;
        };

        let mut position = match self.cache.borrow().position(&position_id) {
            Some(pos) if !pos.is_closed() => pos.clone(),
            _ => self
                .open_position(instrument.clone(), None, fill, oms_type)
                .unwrap(),
        };

        if self.will_flip_position(&position, fill) {
            self.flip_position(instrument, &mut position, fill, oms_type);
        } else {
            self.update_position(&mut position, fill);
        }

        if matches!(order.contingency_type(), Some(ContingencyType::Oto)) && position.is_open() {
            for client_order_id in order.linked_order_ids().unwrap_or_default() {
                let mut cache = self.cache.borrow_mut();
                let contingent_order = cache.mut_order(client_order_id);
                if let Some(contingent_order) = contingent_order {
                    if contingent_order.position_id().is_none() {
                        contingent_order.set_position_id(Some(position_id));

                        if let Err(e) = self.cache.borrow_mut().add_position_id(
                            &position_id,
                            &contingent_order.instrument_id().venue,
                            &contingent_order.client_order_id(),
                            &contingent_order.strategy_id(),
                        ) {
                            log::error!("Failed to add position ID: {e}");
                        }
                    }
                }
            }
        }
    }

    fn open_position(
        &self,
        instrument: InstrumentAny,
        position: Option<&Position>,
        fill: OrderFilled,
        oms_type: OmsType,
    ) -> anyhow::Result<Position> {
        let position = if let Some(position) = position {
            // Always snapshot opening positions to handle NETTING OMS
            self.cache.borrow_mut().snapshot_position(position)?;
            let mut position = position.clone();
            position.apply(&fill);
            self.cache.borrow_mut().update_position(&position)?;
            position
        } else {
            let position = Position::new(&instrument, fill);
            self.cache
                .borrow_mut()
                .add_position(position.clone(), oms_type)?;
            if self.config.snapshot_positions {
                self.create_position_state_snapshot(&position);
            }
            position
        };

        let ts_init = self.clock.borrow().timestamp_ns();
        let event = PositionOpened::create(&position, &fill, UUID4::new(), ts_init);
        let topic = switchboard::get_event_positions_topic(event.strategy_id);
        msgbus::publish(&topic, &event);

        Ok(position)
    }

    fn update_position(&self, position: &mut Position, fill: OrderFilled) {
        position.apply(&fill);

        if let Err(e) = self.cache.borrow_mut().update_position(position) {
            log::error!("Failed to update position: {e:?}");
            return;
        }

        if self.config.snapshot_positions {
            self.create_position_state_snapshot(position);
        }

        let topic = switchboard::get_event_positions_topic(position.strategy_id);
        let ts_init = self.clock.borrow().timestamp_ns();

        if position.is_closed() {
            let event = PositionClosed::create(position, &fill, UUID4::new(), ts_init);
            msgbus::publish(&topic, &event);
        } else {
            let event = PositionChanged::create(position, &fill, UUID4::new(), ts_init);
            msgbus::publish(&topic, &event);
        }
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
            fill_split1 = Some(OrderFilled::new(
                fill.trader_id,
                fill.strategy_id,
                fill.instrument_id,
                fill.client_order_id,
                fill.venue_order_id,
                fill.account_id,
                fill.trade_id,
                fill.order_side,
                fill.order_type,
                position.quantity,
                fill.last_px,
                fill.currency,
                fill.liquidity_side,
                UUID4::new(),
                fill.ts_event,
                fill.ts_init,
                fill.reconciliation,
                fill.position_id,
                commission1,
            ));

            self.update_position(position, fill_split1.unwrap());
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

        let fill_split2 = OrderFilled::new(
            fill.trader_id,
            fill.strategy_id,
            fill.instrument_id,
            fill.client_order_id,
            fill.venue_order_id,
            fill.account_id,
            fill.trade_id,
            fill.order_side,
            fill.order_type,
            difference,
            fill.last_px,
            fill.currency,
            fill.liquidity_side,
            UUID4::new(),
            fill.ts_event,
            fill.ts_init,
            fill.reconciliation,
            position_id_flip,
            commission2,
        );

        if oms_type == OmsType::Hedging {
            if let Some(position_id) = fill.position_id {
                if position_id.is_virtual() {
                    log::warn!("Closing position {fill_split1:?}");
                    log::warn!("Flipping position {fill_split2:?}");
                }
            }
        }

        // Open flipped position
        if let Err(e) = self.open_position(instrument, None, fill_split2, oms_type) {
            log::error!("Failed to open flipped position: {e:?}");
        }
    }

    // -- INTERNAL --------------------------------------------------------------------------------

    fn set_position_id_counts(&mut self) {
        // For the internal position ID generator
        let cache = self.cache.borrow();
        let positions = cache.positions(None, None, None, None);

        // Count positions per instrument_id using a HashMap
        let mut counts: HashMap<StrategyId, usize> = HashMap::new();

        for position in positions {
            *counts.entry(position.strategy_id).or_insert(0) += 1;
        }

        self.pos_id_generator.reset();

        for (strategy_id, count) in counts {
            self.pos_id_generator.set_count(count, strategy_id);
            log::info!("Set PositionId count for {strategy_id} to {count}");
        }
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

    fn set_order_base_qty(&self, order: &mut OrderAny, base_qty: Quantity) {
        log::info!(
            "Setting {} order quote quantity {} to base quantity {}",
            order.instrument_id(),
            order.quantity(),
            base_qty
        );

        let original_qty = order.quantity();
        order.set_quantity(base_qty);
        order.set_leaves_qty(base_qty);
        order.set_is_quote_quantity(false);

        if matches!(order.contingency_type(), Some(ContingencyType::Oto)) {
            return;
        }

        if let Some(linked_order_ids) = order.linked_order_ids() {
            for client_order_id in linked_order_ids {
                match self.cache.borrow_mut().mut_order(client_order_id) {
                    Some(contingent_order) => {
                        if !contingent_order.is_quote_quantity() {
                            continue; // Already base quantity
                        }

                        if contingent_order.quantity() != original_qty {
                            log::warn!(
                                "Contingent order quantity {} was not equal to the OTO parent original quantity {} when setting to base quantity of {}",
                                contingent_order.quantity(),
                                original_qty,
                                base_qty
                            );
                        }

                        log::info!(
                            "Setting {} order quote quantity {} to base quantity {}",
                            contingent_order.instrument_id(),
                            contingent_order.quantity(),
                            base_qty
                        );

                        contingent_order.set_quantity(base_qty);
                        contingent_order.set_leaves_qty(base_qty);
                        contingent_order.set_is_quote_quantity(false);
                    }
                    None => {
                        log::error!("Contingency order {client_order_id} not found");
                    }
                }
            }
        } else {
            log::warn!(
                "No linked order IDs found for order {}",
                order.client_order_id()
            );
        }
    }

    fn deny_order(&self, order: &OrderAny, reason: &str) {
        log::error!(
            "Order denied: {reason}, order ID: {}",
            order.client_order_id()
        );

        let denied = OrderDenied::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            reason.into(),
            UUID4::new(),
            self.clock.borrow().timestamp_ns(),
            self.clock.borrow().timestamp_ns(),
        );

        let mut order = order.clone();

        if let Err(e) = order.apply(OrderEventAny::Denied(denied)) {
            log::error!("Failed to apply denied event to order: {e}");
            return;
        }

        if let Err(e) = self.cache.borrow_mut().update_order(&order) {
            log::error!("Failed to update order in cache: {e}");
            return;
        }

        let topic = switchboard::get_event_orders_topic(order.strategy_id());
        msgbus::publish(&topic, &denied);

        if self.config.snapshot_orders {
            self.create_order_state_snapshot(&order);
        }
    }

    fn get_or_init_own_order_book(&self, instrument_id: &InstrumentId) -> RefMut<OwnOrderBook> {
        let mut cache = self.cache.borrow_mut();
        if cache.own_order_book_mut(instrument_id).is_none() {
            let own_book = OwnOrderBook::new(*instrument_id);
            cache.add_own_order_book(own_book).unwrap();
        }

        RefMut::map(cache, |c| c.own_order_book_mut(instrument_id).unwrap())
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{cache::Cache, clock::TestClock, msgbus::MessageBus};
    use rstest::fixture;

    use super::*;

    #[fixture]
    fn msgbus() -> MessageBus {
        MessageBus::default()
    }

    #[fixture]
    fn simple_cache() -> Cache {
        Cache::new(None, None)
    }

    #[fixture]
    fn clock() -> TestClock {
        TestClock::new()
    }

    // Helpers
    fn _get_exec_engine(
        cache: Rc<RefCell<Cache>>,
        clock: Rc<RefCell<TestClock>>,
        config: Option<ExecutionEngineConfig>,
    ) -> ExecutionEngine {
        ExecutionEngine::new(clock, cache, config)
    }

    // TODO: After Implementing ExecutionClient & Strategy
}
