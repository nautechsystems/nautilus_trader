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

//! Provides a generic `ExecutionEngine` for all environments.
//!
//! The execution engines primary responsibility is to orchestrate interactions
//! between the `ExecutionClient` instances, and the rest of the platform. This
//! includes sending commands to, and receiving events from, the trading venue
//! endpoints via its registered execution clients.

pub mod config;
pub mod stubs;

use std::{
    cell::{RefCell, RefMut},
    collections::{HashMap, HashSet},
    fmt::Debug,
    rc::Rc,
    time::SystemTime,
};

use ahash::AHashSet;
use config::ExecutionEngineConfig;
use futures::future::join_all;
use indexmap::{IndexMap, IndexSet};
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    clock::Clock,
    generators::position_id::PositionIdGenerator,
    logging::{CMD, EVT, RECV, SEND},
    messages::{
        ExecutionReport,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, ModifyOrder, QueryAccount, QueryOrder,
            SubmitOrder, SubmitOrderList, TradingCommand,
        },
    },
    msgbus::{
        self, MessagingSwitchboard, ShareableMessageHandler, TypedIntoHandler, get_message_bus,
        switchboard::{self},
    },
    runner::try_get_trading_cmd_sender,
    timer::{TimeEvent, TimeEventCallback},
};
use nautilus_core::{
    UUID4, UnixNanos, WeakCell,
    datetime::{mins_to_nanos, mins_to_secs},
};
use nautilus_model::{
    enums::{
        ContingencyType, OmsType, OrderStatus, OrderType, PositionSide, TimeInForce,
        TrailingOffsetType,
    },
    events::{
        OrderAccepted, OrderCanceled, OrderDenied, OrderEvent, OrderEventAny, OrderExpired,
        OrderFilled, OrderInitialized, PositionChanged, PositionClosed, PositionEvent,
        PositionOpened,
    },
    identifiers::{
        ClientId, ClientOrderId, InstrumentId, PositionId, StrategyId, Venue, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    orderbook::own::{OwnOrderBook, should_handle_own_book_order},
    orders::{Order, OrderAny, OrderError},
    position::Position,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{Money, Quantity},
};
use rust_decimal::Decimal;

use crate::{
    client::ExecutionClientAdapter,
    reconciliation::{
        check_position_reconciliation, create_incremental_inferred_fill,
        generate_external_order_status_events, generate_reconciliation_order_events,
        reconcile_fill_report as reconcile_fill,
    },
};

const TIMER_PURGE_CLOSED_ORDERS: &str = "ExecEngine_PURGE_CLOSED_ORDERS";
const TIMER_PURGE_CLOSED_POSITIONS: &str = "ExecEngine_PURGE_CLOSED_POSITIONS";
const TIMER_PURGE_ACCOUNT_EVENTS: &str = "ExecEngine_PURGE_ACCOUNT_EVENTS";

/// Central execution engine responsible for orchestrating order routing and execution.
///
/// The execution engine manages the entire order lifecycle from submission to completion,
/// handling routing to appropriate execution clients, position management, and event
/// processing. It supports multiple execution venues through registered clients and
/// provides sophisticated order management capabilities.
pub struct ExecutionEngine {
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    clients: IndexMap<ClientId, ExecutionClientAdapter>,
    default_client: Option<ExecutionClientAdapter>,
    routing_map: HashMap<Venue, ClientId>,
    oms_overrides: HashMap<StrategyId, OmsType>,
    external_order_claims: HashMap<InstrumentId, StrategyId>,
    external_clients: HashSet<ClientId>,
    pos_id_generator: PositionIdGenerator,
    config: ExecutionEngineConfig,
}

impl Debug for ExecutionEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(ExecutionEngine))
            .field("client_count", &self.clients.len())
            .finish()
    }
}

impl ExecutionEngine {
    /// Creates a new [`ExecutionEngine`] instance.
    pub fn new(
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
        config: Option<ExecutionEngineConfig>,
    ) -> Self {
        let trader_id = get_message_bus().borrow().trader_id;
        Self {
            clock: clock.clone(),
            cache,
            clients: IndexMap::new(),
            default_client: None,
            routing_map: HashMap::new(),
            oms_overrides: HashMap::new(),
            external_order_claims: HashMap::new(),
            external_clients: config
                .as_ref()
                .and_then(|c| c.external_clients.clone())
                .unwrap_or_default()
                .into_iter()
                .collect(),
            pos_id_generator: PositionIdGenerator::new(trader_id, clock),
            config: config.unwrap_or_default(),
        }
    }

    /// Registers all message bus handlers for the execution engine.
    pub fn register_msgbus_handlers(engine: &Rc<RefCell<Self>>) {
        let weak = WeakCell::from(Rc::downgrade(engine));

        let weak1 = weak.clone();
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::exec_engine_execute(),
            TypedIntoHandler::from(move |cmd: TradingCommand| {
                if let Some(rc) = weak1.upgrade() {
                    rc.borrow().execute(cmd);
                }
            }),
        );

        // Queued endpoint for deferred command execution (re-entrancy safe),
        // falls back to direct endpoint if no sender is initialized (e.g., backtest/test).
        msgbus::register_trading_command_endpoint(
            MessagingSwitchboard::exec_engine_queue_execute(),
            TypedIntoHandler::from(move |cmd: TradingCommand| {
                if let Some(sender) = try_get_trading_cmd_sender() {
                    sender.execute(cmd);
                } else {
                    let endpoint = MessagingSwitchboard::exec_engine_execute();
                    msgbus::send_trading_command(endpoint, cmd);
                }
            }),
        );

        let weak2 = weak.clone();
        msgbus::register_order_event_endpoint(
            MessagingSwitchboard::exec_engine_process(),
            TypedIntoHandler::from(move |event: OrderEventAny| {
                if let Some(rc) = weak2.upgrade() {
                    rc.borrow_mut().process(&event);
                }
            }),
        );

        let weak3 = weak;
        msgbus::register_execution_report_endpoint(
            MessagingSwitchboard::exec_engine_reconcile_execution_report(),
            TypedIntoHandler::from(move |report: ExecutionReport| {
                if let Some(rc) = weak3.upgrade() {
                    rc.borrow_mut().reconcile_execution_report(&report);
                }
            }),
        );
    }

    /// Subscribes to instrument updates for a venue via the message bus.
    ///
    /// When instruments are published by the `DataEngine`, the handler routes
    /// them to the execution client registered for that venue.
    pub fn subscribe_venue_instruments(engine: &Rc<RefCell<Self>>, venue: Venue) {
        let weak = WeakCell::from(Rc::downgrade(engine));
        let pattern = switchboard::get_instruments_pattern(venue);

        let handler = ShareableMessageHandler::from_typed(move |instrument: &InstrumentAny| {
            if let Some(rc) = weak.upgrade() {
                let venue = instrument.id().venue;
                let client_id = rc.borrow().routing_map.get(&venue).copied();
                if let Some(client_id) = client_id {
                    let mut engine = rc.borrow_mut();
                    if let Some(adapter) = engine.get_client_adapter_mut(&client_id) {
                        adapter.on_instrument(instrument.clone());
                    }
                }
            }
        });

        msgbus::subscribe_any(pattern, handler, None);
        log::info!("Subscribed to instrument updates for venue {venue}");
    }

    #[must_use]
    /// Returns the position ID count for the specified strategy.
    pub fn position_id_count(&self, strategy_id: StrategyId) -> usize {
        self.pos_id_generator.count(strategy_id)
    }

    #[must_use]
    /// Returns a reference to the cache.
    pub fn cache(&self) -> &Rc<RefCell<Cache>> {
        &self.cache
    }

    #[must_use]
    /// Returns a reference to the configuration.
    pub const fn config(&self) -> &ExecutionEngineConfig {
        &self.config
    }

    #[must_use]
    /// Checks the integrity of cached execution data.
    pub fn check_integrity(&self) -> bool {
        self.cache.borrow_mut().check_integrity()
    }

    #[must_use]
    /// Returns true if all registered execution clients are connected.
    pub fn check_connected(&self) -> bool {
        let clients_connected = self.clients.values().all(|c| c.is_connected());
        let default_connected = self
            .default_client
            .as_ref()
            .is_none_or(|c| c.is_connected());
        clients_connected && default_connected
    }

    #[must_use]
    /// Returns true if all registered execution clients are disconnected.
    pub fn check_disconnected(&self) -> bool {
        let clients_disconnected = self.clients.values().all(|c| !c.is_connected());
        let default_disconnected = self
            .default_client
            .as_ref()
            .is_none_or(|c| !c.is_connected());
        clients_disconnected && default_disconnected
    }

    /// Returns connection status for each registered client.
    #[must_use]
    pub fn client_connection_status(&self) -> Vec<(ClientId, bool)> {
        let mut status: Vec<_> = self
            .clients
            .values()
            .map(|c| (c.client_id(), c.is_connected()))
            .collect();

        if let Some(default) = &self.default_client {
            status.push((default.client_id(), default.is_connected()));
        }

        status
    }

    #[must_use]
    /// Checks for residual positions and orders in the cache.
    pub fn check_residuals(&self) -> bool {
        self.cache.borrow().check_residuals()
    }

    #[must_use]
    /// Returns the set of instruments that have external order claims.
    pub fn get_external_order_claims_instruments(&self) -> HashSet<InstrumentId> {
        self.external_order_claims.keys().copied().collect()
    }

    #[must_use]
    /// Returns the configured external client IDs.
    pub fn get_external_client_ids(&self) -> HashSet<ClientId> {
        self.external_clients.clone()
    }

    #[must_use]
    /// Returns any external order claim for the given instrument ID.
    pub fn get_external_order_claim(&self, instrument_id: &InstrumentId) -> Option<StrategyId> {
        self.external_order_claims.get(instrument_id).copied()
    }

    /// Registers a new execution client.
    ///
    /// # Errors
    ///
    /// Returns an error if a client with the same ID is already registered.
    pub fn register_client(&mut self, client: Box<dyn ExecutionClient>) -> anyhow::Result<()> {
        let client_id = client.client_id();
        let venue = client.venue();

        if self.clients.contains_key(&client_id) {
            anyhow::bail!("Client already registered with ID {client_id}");
        }

        let adapter = ExecutionClientAdapter::new(client);

        if let Some(existing_client_id) = self.routing_map.get(&venue) {
            anyhow::bail!(
                "Venue {venue} already routed to {existing_client_id}, \
                 cannot register {client_id} for the same venue"
            );
        }

        self.routing_map.insert(venue, client_id);
        log::debug!("Registered client {client_id}");
        self.clients.insert(client_id, adapter);
        Ok(())
    }

    /// Registers a default execution client for fallback routing.
    pub fn register_default_client(&mut self, client: Box<dyn ExecutionClient>) {
        let client_id = client.client_id();
        let adapter = ExecutionClientAdapter::new(client);

        log::debug!("Registered default client {client_id}");
        self.default_client = Some(adapter);
    }

    #[must_use]
    /// Returns a reference to the execution client registered with the given ID.
    pub fn get_client(&self, client_id: &ClientId) -> Option<&dyn ExecutionClient> {
        self.clients.get(client_id).map(|a| a.client.as_ref())
    }

    #[must_use]
    /// Returns a mutable reference to the execution client adapter registered with the given ID.
    pub fn get_client_adapter_mut(
        &mut self,
        client_id: &ClientId,
    ) -> Option<&mut ExecutionClientAdapter> {
        if let Some(default) = &self.default_client
            && &default.client_id == client_id
        {
            return self.default_client.as_mut();
        }
        self.clients.get_mut(client_id)
    }

    /// Generates mass status for the given client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client is not found or mass status generation fails.
    pub async fn generate_mass_status(
        &mut self,
        client_id: &ClientId,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        if let Some(client) = self.get_client_adapter_mut(client_id) {
            client.generate_mass_status(lookback_mins).await
        } else {
            anyhow::bail!("Client {client_id} not found")
        }
    }

    /// Registers an external order with the execution client for tracking.
    ///
    /// This is called after reconciliation creates an external order, allowing the
    /// execution client to track it for subsequent events (e.g., cancellations).
    pub fn register_external_order(
        &self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        instrument_id: InstrumentId,
        strategy_id: StrategyId,
        ts_init: UnixNanos,
    ) {
        let venue = instrument_id.venue;
        if let Some(client_id) = self.routing_map.get(&venue) {
            if let Some(client) = self.clients.get(client_id) {
                client.register_external_order(
                    client_order_id,
                    venue_order_id,
                    instrument_id,
                    strategy_id,
                    ts_init,
                );
            }
        } else if let Some(default) = &self.default_client {
            default.register_external_order(
                client_order_id,
                venue_order_id,
                instrument_id,
                strategy_id,
                ts_init,
            );
        }
    }

    #[must_use]
    /// Returns all registered execution client IDs.
    pub fn client_ids(&self) -> Vec<ClientId> {
        let mut ids: Vec<_> = self.clients.keys().copied().collect();

        if let Some(default) = &self.default_client {
            ids.push(default.client_id);
        }
        ids
    }

    #[must_use]
    /// Returns mutable access to all registered execution clients.
    pub fn get_clients_mut(&mut self) -> Vec<&mut ExecutionClientAdapter> {
        let mut adapters: Vec<_> = self.clients.values_mut().collect();

        if let Some(default) = &mut self.default_client {
            adapters.push(default);
        }
        adapters
    }

    /// Returns all registered execution clients.
    #[must_use]
    pub fn get_all_clients(&self) -> Vec<&dyn ExecutionClient> {
        let mut clients: Vec<&dyn ExecutionClient> =
            self.clients.values().map(|a| a.client.as_ref()).collect();

        if let Some(default) = &self.default_client {
            clients.push(default.client.as_ref());
        }

        clients
    }

    #[must_use]
    /// Returns execution clients that would handle the given orders.
    ///
    /// This method first attempts to resolve each order's originating client from the cache,
    /// then falls back to venue routing for any orders without a cached client.
    pub fn get_clients_for_orders(&self, orders: &[OrderAny]) -> Vec<&dyn ExecutionClient> {
        let mut client_ids: IndexSet<ClientId> = IndexSet::new();
        let mut venues: IndexSet<Venue> = IndexSet::new();

        // Collect client IDs from cache and venues for fallback
        for order in orders {
            venues.insert(order.instrument_id().venue);
            if let Some(client_id) = self.cache.borrow().client_id(&order.client_order_id()) {
                client_ids.insert(*client_id);
            }
        }

        let mut clients: Vec<&dyn ExecutionClient> = Vec::new();

        // Add clients for cached client IDs (orders go back to originating client)
        for client_id in &client_ids {
            if let Some(adapter) = self.clients.get(client_id)
                && !clients.iter().any(|c| c.client_id() == adapter.client_id)
            {
                clients.push(adapter.client.as_ref());
            }
        }

        // Add clients for venue routing (for orders not in cache)
        for venue in &venues {
            if let Some(client_id) = self.routing_map.get(venue) {
                if let Some(adapter) = self.clients.get(client_id)
                    && !clients.iter().any(|c| c.client_id() == adapter.client_id)
                {
                    clients.push(adapter.client.as_ref());
                }
            } else if let Some(adapter) = &self.default_client
                && !clients.iter().any(|c| c.client_id() == adapter.client_id)
            {
                clients.push(adapter.client.as_ref());
            }
        }

        clients
    }

    /// Sets routing for a specific venue to a given client ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the client ID is not registered.
    pub fn register_venue_routing(
        &mut self,
        client_id: ClientId,
        venue: Venue,
    ) -> anyhow::Result<()> {
        if !self.clients.contains_key(&client_id) {
            anyhow::bail!("No client registered with ID {client_id}");
        }

        if let Some(existing_client_id) = self.routing_map.get(&venue)
            && *existing_client_id != client_id
        {
            anyhow::bail!(
                "Venue {venue} already routed to {existing_client_id}, \
                 cannot re-route to {client_id}"
            );
        }

        self.routing_map.insert(venue, client_id);
        log::info!("Set client {client_id} routing for {venue}");
        Ok(())
    }

    /// Registers the OMS (Order Management System) type for a strategy.
    ///
    /// If an OMS type is already registered for this strategy, it will be overridden.
    pub fn register_oms_type(&mut self, strategy_id: StrategyId, oms_type: OmsType) {
        self.oms_overrides.insert(strategy_id, oms_type);
        log::info!("Registered OMS::{oms_type:?} for {strategy_id}");
    }

    /// Registers external order claims for a strategy.
    ///
    /// This operation is atomic: either all instruments are registered or none are.
    ///
    /// # Errors
    ///
    /// Returns an error if any instrument already has a registered claim.
    pub fn register_external_order_claims(
        &mut self,
        strategy_id: StrategyId,
        instrument_ids: &HashSet<InstrumentId>,
    ) -> anyhow::Result<()> {
        // Validate all instruments first
        for instrument_id in instrument_ids {
            if let Some(existing) = self.external_order_claims.get(instrument_id) {
                anyhow::bail!(
                    "External order claim for {instrument_id} already exists for {existing}"
                );
            }
        }

        // If validation passed, insert all claims
        for instrument_id in instrument_ids {
            self.external_order_claims
                .insert(*instrument_id, strategy_id);
        }

        if !instrument_ids.is_empty() {
            log::info!("Registered external order claims for {strategy_id}: {instrument_ids:?}");
        }

        Ok(())
    }

    /// # Errors
    ///
    /// Returns an error if no client is registered with the given ID.
    pub fn deregister_client(&mut self, client_id: ClientId) -> anyhow::Result<()> {
        if self.clients.shift_remove(&client_id).is_some() {
            // Remove from routing map if present
            self.routing_map
                .retain(|_, mapped_id| mapped_id != &client_id);
            log::info!("Deregistered client {client_id}");
            Ok(())
        } else {
            anyhow::bail!("No client registered with ID {client_id}")
        }
    }

    /// Connects all registered execution clients concurrently.
    ///
    /// Connection failures are logged but do not prevent the node from running.
    pub async fn connect(&mut self) {
        let futures: Vec<_> = self
            .get_clients_mut()
            .into_iter()
            .map(|client| client.connect())
            .collect();

        let results = join_all(futures).await;

        for error in results.into_iter().filter_map(Result::err) {
            log::error!("Failed to connect execution client: {error:#}");
        }
    }

    /// Disconnects all registered execution clients concurrently.
    ///
    /// # Errors
    ///
    /// Returns an error if any client fails to disconnect.
    pub async fn disconnect(&mut self) -> anyhow::Result<()> {
        let futures: Vec<_> = self
            .get_clients_mut()
            .into_iter()
            .map(|client| client.disconnect())
            .collect();

        let results = join_all(futures).await;
        let errors: Vec<_> = results.into_iter().filter_map(Result::err).collect();

        if errors.is_empty() {
            Ok(())
        } else {
            let error_msgs: Vec<_> = errors.iter().map(|e| e.to_string()).collect();
            anyhow::bail!(
                "Failed to disconnect execution clients: {}",
                error_msgs.join("; ")
            )
        }
    }

    /// Sets the `manage_own_order_books` configuration option.
    pub fn set_manage_own_order_books(&mut self, value: bool) {
        self.config.manage_own_order_books = value;
    }

    /// Starts the position snapshot timer if configured.
    ///
    /// Timer functionality requires a live execution context with an active clock.
    pub fn start_snapshot_timer(&mut self) {
        if let Some(interval_secs) = self.config.snapshot_positions_interval_secs {
            log::info!("Starting position snapshots timer at {interval_secs} second intervals");
        }
    }

    /// Stops the position snapshot timer if running.
    pub fn stop_snapshot_timer(&mut self) {
        if self.config.snapshot_positions_interval_secs.is_some() {
            log::info!("Canceling position snapshots timer");
        }
    }

    /// Starts the purge timers if configured.
    #[expect(
        clippy::missing_panics_doc,
        reason = "timer registration is not expected to fail"
    )]
    pub fn start_purge_timers(&mut self) {
        if let Some(interval_mins) = self
            .config
            .purge_closed_orders_interval_mins
            .filter(|&m| m > 0)
            && !self
                .clock
                .borrow()
                .timer_names()
                .contains(&TIMER_PURGE_CLOSED_ORDERS)
        {
            let interval_ns = mins_to_nanos(u64::from(interval_mins));
            let buffer_mins = self.config.purge_closed_orders_buffer_mins.unwrap_or(0);
            let buffer_secs = mins_to_secs(u64::from(buffer_mins));
            let cache = self.cache.clone();
            let clock = self.clock.clone();

            let callback_fn: Rc<dyn Fn(TimeEvent)> = Rc::new(move |_event| {
                let ts_now = clock.borrow().timestamp_ns();
                cache.borrow_mut().purge_closed_orders(ts_now, buffer_secs);
            });
            let callback = TimeEventCallback::from(callback_fn);

            log::info!("Starting purge closed orders timer at {interval_mins} minute intervals");
            self.clock
                .borrow_mut()
                .set_timer_ns(
                    TIMER_PURGE_CLOSED_ORDERS,
                    interval_ns,
                    None,
                    None,
                    Some(callback),
                    None,
                    None,
                )
                .expect("Failed to set purge closed orders timer");
        }

        if let Some(interval_mins) = self
            .config
            .purge_closed_positions_interval_mins
            .filter(|&m| m > 0)
            && !self
                .clock
                .borrow()
                .timer_names()
                .contains(&TIMER_PURGE_CLOSED_POSITIONS)
        {
            let interval_ns = mins_to_nanos(u64::from(interval_mins));
            let buffer_mins = self.config.purge_closed_positions_buffer_mins.unwrap_or(0);
            let buffer_secs = mins_to_secs(u64::from(buffer_mins));
            let cache = self.cache.clone();
            let clock = self.clock.clone();

            let callback_fn: Rc<dyn Fn(TimeEvent)> = Rc::new(move |_event| {
                let ts_now = clock.borrow().timestamp_ns();
                cache
                    .borrow_mut()
                    .purge_closed_positions(ts_now, buffer_secs);
            });
            let callback = TimeEventCallback::from(callback_fn);

            log::info!("Starting purge closed positions timer at {interval_mins} minute intervals");
            self.clock
                .borrow_mut()
                .set_timer_ns(
                    TIMER_PURGE_CLOSED_POSITIONS,
                    interval_ns,
                    None,
                    None,
                    Some(callback),
                    None,
                    None,
                )
                .expect("Failed to set purge closed positions timer");
        }

        if let Some(interval_mins) = self
            .config
            .purge_account_events_interval_mins
            .filter(|&m| m > 0)
            && !self
                .clock
                .borrow()
                .timer_names()
                .contains(&TIMER_PURGE_ACCOUNT_EVENTS)
        {
            let interval_ns = mins_to_nanos(u64::from(interval_mins));
            let lookback_mins = self.config.purge_account_events_lookback_mins.unwrap_or(0);
            let lookback_secs = mins_to_secs(u64::from(lookback_mins));
            let cache = self.cache.clone();
            let clock = self.clock.clone();

            let callback_fn: Rc<dyn Fn(TimeEvent)> = Rc::new(move |_event| {
                let ts_now = clock.borrow().timestamp_ns();
                cache
                    .borrow_mut()
                    .purge_account_events(ts_now, lookback_secs);
            });
            let callback = TimeEventCallback::from(callback_fn);

            log::info!("Starting purge account events timer at {interval_mins} minute intervals");
            self.clock
                .borrow_mut()
                .set_timer_ns(
                    TIMER_PURGE_ACCOUNT_EVENTS,
                    interval_ns,
                    None,
                    None,
                    Some(callback),
                    None,
                    None,
                )
                .expect("Failed to set purge account events timer");
        }
    }

    /// Stops the purge timers if running.
    pub fn stop_purge_timers(&mut self) {
        let timer_names: Vec<String> = self
            .clock
            .borrow()
            .timer_names()
            .into_iter()
            .map(String::from)
            .collect();

        if timer_names.iter().any(|n| n == TIMER_PURGE_CLOSED_ORDERS) {
            log::info!("Canceling purge closed orders timer");
            self.clock
                .borrow_mut()
                .cancel_timer(TIMER_PURGE_CLOSED_ORDERS);
        }

        if timer_names
            .iter()
            .any(|n| n == TIMER_PURGE_CLOSED_POSITIONS)
        {
            log::info!("Canceling purge closed positions timer");
            self.clock
                .borrow_mut()
                .cancel_timer(TIMER_PURGE_CLOSED_POSITIONS);
        }

        if timer_names.iter().any(|n| n == TIMER_PURGE_ACCOUNT_EVENTS) {
            log::info!("Canceling purge account events timer");
            self.clock
                .borrow_mut()
                .cancel_timer(TIMER_PURGE_ACCOUNT_EVENTS);
        }
    }

    /// Creates snapshots of all open positions.
    pub fn snapshot_open_position_states(&self) {
        let positions: Vec<Position> = self
            .cache
            .borrow()
            .positions_open(None, None, None, None, None)
            .into_iter()
            .cloned()
            .collect();

        for position in positions {
            self.create_position_state_snapshot(&position);
        }
    }

    #[expect(clippy::await_holding_refcell_ref)]
    /// Loads persistent state into cache and rebuilds indices.
    ///
    /// # Errors
    ///
    /// Returns an error if any cache operation fails.
    pub async fn load_cache(&mut self) -> anyhow::Result<()> {
        let ts = SystemTime::now(); // dst-ok: init-time log timing, not on DST state path

        {
            let mut cache = self.cache.borrow_mut();
            cache.clear_index();
            cache.cache_general()?;
            self.cache.borrow_mut().cache_all().await?;
            cache.build_index();
            let _ = cache.check_integrity();

            if self.config.manage_own_order_books {
                for order in cache.orders(None, None, None, None, None) {
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
            SystemTime::now() // dst-ok: init-time log timing, not on DST state path
                .duration_since(ts)
                .map_err(|e| anyhow::anyhow!("Failed to calculate duration: {e}"))?
                .as_millis()
        );

        Ok(())
    }

    /// Flushes the database to persist all cached data.
    pub fn flush_db(&self) {
        self.cache.borrow_mut().flush_db();
    }

    /// Reconciles an execution report.
    pub fn reconcile_execution_report(&mut self, report: &ExecutionReport) {
        match report {
            ExecutionReport::Order(order_report) => {
                self.reconcile_order_status_report(order_report);
            }
            ExecutionReport::Fill(fill_report) => {
                self.reconcile_fill_report(fill_report);
            }
            ExecutionReport::OrderWithFills(order_report, fills) => {
                self.reconcile_order_with_fills(order_report, fills);
            }
            ExecutionReport::Position(position_report) => {
                self.reconcile_position_report(position_report);
            }
            ExecutionReport::MassStatus(mass_status) => {
                self.reconcile_execution_mass_status(mass_status);
            }
        }
    }

    /// Reconciles an order status report received at runtime.
    ///
    /// Handles order status transitions by generating appropriate events when the venue
    /// reports a different status than our local state. Supports all order states including
    /// fills with inferred fill generation when instruments are available.
    ///
    /// When the order is not found in cache, creates an external order from the report.
    /// This handles exchange-generated orders (liquidation, ADL, settlement) that were
    /// not submitted locally.
    pub fn reconcile_order_status_report(&mut self, report: &OrderStatusReport) {
        let cache = self.cache.borrow();

        let order = report
            .client_order_id
            .and_then(|id| cache.order(&id).cloned())
            .or_else(|| {
                cache
                    .client_order_id(&report.venue_order_id)
                    .and_then(|cid| cache.order(cid).cloned())
            });

        let instrument = cache.instrument(&report.instrument_id).cloned();

        drop(cache);

        if let Some(order) = order {
            let ts_now = self.clock.borrow().timestamp_ns();
            let events =
                generate_reconciliation_order_events(&order, report, instrument.as_ref(), ts_now);

            for event in &events {
                self.handle_event(event);
            }
        } else {
            self.create_external_order(report, instrument.as_ref());
        }
    }

    fn create_external_order(
        &mut self,
        report: &OrderStatusReport,
        instrument: Option<&InstrumentAny>,
    ) {
        let Some(instrument) = instrument else {
            log::warn!(
                "Cannot create external order for venue_order_id={}: instrument {} not found",
                report.venue_order_id,
                report.instrument_id
            );
            return;
        };

        let Some(order) = self.materialize_external_order_from_status(report) else {
            return;
        };

        let ts_now = self.clock.borrow().timestamp_ns();
        let events = generate_external_order_status_events(
            &order,
            report,
            &report.account_id,
            instrument,
            ts_now,
        );

        for event in &events {
            self.handle_event(event);
        }
    }

    /// Builds and registers an external order from an [`OrderStatusReport`] without
    /// emitting status events. Returns the registered order.
    fn materialize_external_order_from_status(
        &self,
        report: &OrderStatusReport,
    ) -> Option<OrderAny> {
        let strategy_id = self.resolve_external_strategy(&report.instrument_id);

        let client_order_id = report
            .client_order_id
            .unwrap_or_else(|| ClientOrderId::from(report.venue_order_id.as_str()));

        let trader_id = get_message_bus().borrow().trader_id;
        let ts_now = self.clock.borrow().timestamp_ns();

        let initialized = OrderInitialized::new(
            trader_id,
            strategy_id,
            report.instrument_id,
            client_order_id,
            report.order_side,
            report.order_type,
            report.quantity,
            report.time_in_force,
            report.post_only,
            report.reduce_only,
            false, // quote_quantity
            true,  // reconciliation
            UUID4::new(),
            ts_now,
            ts_now,
            report.price,
            report.trigger_price,
            report.trigger_type,
            report.limit_offset,
            report.trailing_offset,
            Some(report.trailing_offset_type),
            report.expire_time,
            report.display_qty,
            None, // emulation_trigger
            None, // trigger_instrument_id
            Some(report.contingency_type),
            report.order_list_id,
            report.linked_order_ids.clone(),
            report.parent_order_id,
            None, // exec_algorithm_id
            None, // exec_algorithm_params
            None, // exec_spawn_id
            None, // tags
        );

        self.materialize_external_order(
            initialized,
            client_order_id,
            report.venue_order_id,
            report.instrument_id,
            strategy_id,
            ts_now,
            Some(report.order_status),
        )
    }

    /// Builds and registers an external order from a [`FillReport`] when no matching
    /// order exists in cache. The order is created with `OrderType::Market` and a
    /// quantity equal to the fill's `last_qty`, so the fill consumes the entire
    /// order on application.
    ///
    /// This handles venue-initiated fills (most commonly Hyperliquid liquidations)
    /// where the venue does not surface a user-level order on its order channel.
    fn materialize_external_order_from_fill(&self, report: &FillReport) -> Option<OrderAny> {
        let strategy_id = self.resolve_external_strategy(&report.instrument_id);

        let client_order_id = report
            .client_order_id
            .unwrap_or_else(|| ClientOrderId::from(report.venue_order_id.as_str()));

        let trader_id = get_message_bus().borrow().trader_id;
        let ts_now = self.clock.borrow().timestamp_ns();

        let initialized = OrderInitialized::new(
            trader_id,
            strategy_id,
            report.instrument_id,
            client_order_id,
            report.order_side,
            OrderType::Market,
            report.last_qty,
            TimeInForce::Ioc,
            false, // post_only
            true,  // reduce_only: venue-initiated closes always reduce
            false, // quote_quantity
            true,  // reconciliation
            UUID4::new(),
            ts_now,
            ts_now,
            None, // price
            None, // trigger_price
            None, // trigger_type
            None, // limit_offset
            None, // trailing_offset
            Some(TrailingOffsetType::NoTrailingOffset),
            None, // expire_time
            None, // display_qty
            None, // emulation_trigger
            None, // trigger_instrument_id
            Some(ContingencyType::NoContingency),
            None, // order_list_id
            None, // linked_order_ids
            None, // parent_order_id
            None, // exec_algorithm_id
            None, // exec_algorithm_params
            None, // exec_spawn_id
            None, // tags
        );

        self.materialize_external_order(
            initialized,
            client_order_id,
            report.venue_order_id,
            report.instrument_id,
            strategy_id,
            ts_now,
            None,
        )
    }

    fn resolve_external_strategy(&self, instrument_id: &InstrumentId) -> StrategyId {
        self.external_order_claims
            .get(instrument_id)
            .copied()
            .unwrap_or_else(|| StrategyId::from("EXTERNAL"))
    }

    /// Adds an external order to the cache and registers it for adapter routing.
    /// Returns the registered order on success.
    #[allow(
        clippy::too_many_arguments,
        reason = "external order materialisation threads several ids and a timestamp"
    )]
    fn materialize_external_order(
        &self,
        initialized: OrderInitialized,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        instrument_id: InstrumentId,
        strategy_id: StrategyId,
        ts_now: UnixNanos,
        order_status: Option<OrderStatus>,
    ) -> Option<OrderAny> {
        let order = match OrderAny::from_events(vec![OrderEventAny::Initialized(initialized)]) {
            Ok(order) => order,
            Err(e) => {
                log::error!("Failed to create external order from report: {e}");
                return None;
            }
        };

        {
            let mut cache = self.cache.borrow_mut();
            if let Err(e) = cache.add_order(order.clone(), None, None, false) {
                log::error!("Failed to add external order to cache: {e}");
                return None;
            }

            if let Err(e) = cache.add_venue_order_id(&client_order_id, &venue_order_id, false) {
                log::warn!("Failed to add venue order ID index: {e}");
            }
        }

        match order_status {
            Some(status) => log::info!(
                "Created external order {client_order_id} ({venue_order_id}) for {instrument_id} [{status}]",
            ),
            None => log::info!(
                "Created external order {client_order_id} ({venue_order_id}) for {instrument_id}",
            ),
        }

        self.register_external_order(
            client_order_id,
            venue_order_id,
            instrument_id,
            strategy_id,
            ts_now,
        );

        Some(order)
    }

    /// Reconciles a fill report received at runtime.
    ///
    /// Finds the associated order, validates the fill, and generates an OrderFilled event
    /// if the fill is not a duplicate and won't cause an overfill. When the order is not
    /// in cache, an external order is bootstrapped from the fill so that venue-initiated
    /// closures (e.g. Hyperliquid liquidations) that arrive without a companion order
    /// status report still update the local position.
    pub fn reconcile_fill_report(&mut self, report: &FillReport) {
        let cache = self.cache.borrow();

        let order = report
            .client_order_id
            .and_then(|id| cache.order(&id).cloned())
            .or_else(|| {
                cache
                    .client_order_id(&report.venue_order_id)
                    .and_then(|cid| cache.order(cid).cloned())
            });

        let instrument = cache.instrument(&report.instrument_id).cloned();

        drop(cache);

        let Some(instrument) = instrument else {
            log::debug!(
                "Cannot reconcile fill report for venue_order_id={}: instrument {} not found",
                report.venue_order_id,
                report.instrument_id
            );
            return;
        };

        let order = match order {
            Some(order) => order,
            None => {
                let Some(order) = self.materialize_external_order_from_fill(report) else {
                    return;
                };
                let ts_now = self.clock.borrow().timestamp_ns();
                let accepted = OrderAccepted::new(
                    order.trader_id(),
                    order.strategy_id(),
                    order.instrument_id(),
                    order.client_order_id(),
                    report.venue_order_id,
                    report.account_id,
                    UUID4::new(),
                    report.ts_event,
                    ts_now,
                    true, // reconciliation
                );
                self.handle_event(&OrderEventAny::Accepted(accepted));
                self.cache
                    .borrow()
                    .order(&order.client_order_id())
                    .cloned()
                    .unwrap_or(order)
            }
        };

        let ts_now = self.clock.borrow().timestamp_ns();

        if let Some(event) = reconcile_fill(
            &order,
            report,
            &instrument,
            ts_now,
            self.config.allow_overfills,
        ) {
            self.handle_event(&event);
        }
    }

    /// Reconciles an [`OrderStatusReport`] paired with companion [`FillReport`]s
    /// for the same venue event.
    ///
    /// Real fills supplied by the adapter are applied first so their `trade_id` and
    /// `commission` are preserved; any residual quantity not covered by the fills is
    /// then synthesised as an inferred fill from the status report's `avg_px`.
    /// Adapters use this to emit ADL / liquidation / settlement events without
    /// losing real fill metadata.
    pub fn reconcile_order_with_fills(&mut self, report: &OrderStatusReport, fills: &[FillReport]) {
        let cache = self.cache.borrow();
        let order = report
            .client_order_id
            .and_then(|id| cache.order(&id).cloned())
            .or_else(|| {
                cache
                    .client_order_id(&report.venue_order_id)
                    .and_then(|cid| cache.order(cid).cloned())
            });
        let instrument = cache.instrument(&report.instrument_id).cloned();
        drop(cache);

        let Some(instrument) = instrument else {
            log::debug!(
                "Cannot reconcile bundled report for venue_order_id={}: instrument {} not found",
                report.venue_order_id,
                report.instrument_id,
            );
            return;
        };

        // Bootstrap the external order with only OrderAccepted; defer fill events to
        // the per-fill loop so real fill metadata is preserved.
        let mut order = match order {
            Some(order) => order,
            None => {
                let Some(order) = self.materialize_external_order_from_status(report) else {
                    return;
                };
                let ts_now = self.clock.borrow().timestamp_ns();
                let accepted = OrderAccepted::new(
                    order.trader_id(),
                    order.strategy_id(),
                    order.instrument_id(),
                    order.client_order_id(),
                    report.venue_order_id,
                    report.account_id,
                    UUID4::new(),
                    report.ts_accepted,
                    ts_now,
                    true, // reconciliation
                );
                self.handle_event(&OrderEventAny::Accepted(accepted));
                order
            }
        };

        let client_order_id = order.client_order_id();

        for fill in fills {
            let ts_now = self.clock.borrow().timestamp_ns();

            if let Some(event) = reconcile_fill(
                &order,
                fill,
                &instrument,
                ts_now,
                self.config.allow_overfills,
            ) {
                self.handle_event(&event);
            }

            // Refresh order after fill to keep filled_qty accurate for the next iteration.
            if let Some(refreshed) = self.cache.borrow().order(&client_order_id).cloned() {
                order = refreshed;
            }
        }

        // Cover any quantity gap between the status report and the real fills with
        // an inferred fill so the order reaches the venue-reported terminal state.
        if matches!(
            report.order_status,
            OrderStatus::PartiallyFilled | OrderStatus::Filled,
        ) && report.filled_qty > order.filled_qty()
        {
            let ts_now = self.clock.borrow().timestamp_ns();

            if let Some(event) = create_incremental_inferred_fill(
                &order,
                report,
                &report.account_id,
                &instrument,
                ts_now,
                None,
            ) {
                self.handle_event(&event);

                if let Some(refreshed) = self.cache.borrow().order(&client_order_id).cloned() {
                    order = refreshed;
                }
            }
        }

        // Apply terminal events when the venue reports a non-fill closure.
        match report.order_status {
            OrderStatus::Canceled if !order.is_closed() => {
                let ts_now = self.clock.borrow().timestamp_ns();
                let canceled = OrderCanceled::new(
                    order.trader_id(),
                    order.strategy_id(),
                    order.instrument_id(),
                    order.client_order_id(),
                    UUID4::new(),
                    report.ts_last,
                    ts_now,
                    true,
                    Some(report.venue_order_id),
                    Some(report.account_id),
                );
                self.handle_event(&OrderEventAny::Canceled(canceled));
            }
            OrderStatus::Expired if !order.is_closed() => {
                let ts_now = self.clock.borrow().timestamp_ns();
                let expired = OrderExpired::new(
                    order.trader_id(),
                    order.strategy_id(),
                    order.instrument_id(),
                    order.client_order_id(),
                    UUID4::new(),
                    report.ts_last,
                    ts_now,
                    true,
                    Some(report.venue_order_id),
                    Some(report.account_id),
                );
                self.handle_event(&OrderEventAny::Expired(expired));
            }
            _ => {}
        }
    }

    /// Reconciles a position status report received at runtime.
    ///
    /// Compares the venue-reported position with cached positions and logs any discrepancies.
    /// Handles both hedging (with venue_position_id) and netting (without) modes.
    pub fn reconcile_position_report(&mut self, report: &PositionStatusReport) {
        let cache = self.cache.borrow();

        let size_precision = cache
            .instrument(&report.instrument_id)
            .map(|i| i.size_precision());

        if report.venue_position_id.is_some() {
            self.reconcile_position_report_hedging(report, &cache);
        } else {
            self.reconcile_position_report_netting(report, &cache, size_precision);
        }
    }

    fn reconcile_position_report_hedging(&self, report: &PositionStatusReport, cache: &Cache) {
        let venue_position_id = report.venue_position_id.as_ref().unwrap();

        log::debug!(
            "Reconciling HEDGE position for {}, venue_position_id={}",
            report.instrument_id,
            venue_position_id
        );

        let Some(position) = cache.position(venue_position_id) else {
            log::error!("Cannot reconcile position: {venue_position_id} not found in cache");
            return;
        };

        let cached_signed_qty = match position.side {
            PositionSide::Long => position.quantity.as_decimal(),
            PositionSide::Short => -position.quantity.as_decimal(),
            _ => Decimal::ZERO,
        };
        let venue_signed_qty = report.signed_decimal_qty;

        if cached_signed_qty != venue_signed_qty {
            log::error!(
                "Position mismatch for {} {}: cached={}, venue={}",
                report.instrument_id,
                venue_position_id,
                cached_signed_qty,
                venue_signed_qty
            );
        }
    }

    fn reconcile_position_report_netting(
        &self,
        report: &PositionStatusReport,
        cache: &Cache,
        size_precision: Option<u8>,
    ) {
        log::debug!("Reconciling NET position for {}", report.instrument_id);

        let positions_open =
            cache.positions_open(None, Some(&report.instrument_id), None, None, None);

        // Sum up cached position quantities using domain types to avoid f64 precision loss
        let cached_signed_qty: Decimal = positions_open
            .iter()
            .map(|p| match p.side {
                PositionSide::Long => p.quantity.as_decimal(),
                PositionSide::Short => -p.quantity.as_decimal(),
                _ => Decimal::ZERO,
            })
            .sum();

        log::debug!(
            "Position report: venue_signed_qty={}, cached_signed_qty={}",
            report.signed_decimal_qty,
            cached_signed_qty
        );

        let _ = check_position_reconciliation(report, cached_signed_qty, size_precision);
    }

    /// Reconciles an execution mass status report.
    ///
    /// Processes all order reports, fill reports, and position reports contained
    /// in the mass status. Orders created as external during this pass already receive
    /// inferred fills, so their companion fill reports are skipped to avoid double-fills.
    pub fn reconcile_execution_mass_status(&mut self, mass_status: &ExecutionMassStatus) {
        log::info!(
            "Reconciling mass status for client={}, account={}, venue={}",
            mass_status.client_id,
            mass_status.account_id,
            mass_status.venue
        );

        let mut external_venue_ids = AHashSet::new();

        for order_report in mass_status.order_reports().values() {
            let existed = {
                let cache = self.cache.borrow();
                order_report
                    .client_order_id
                    .and_then(|id| cache.order(&id).cloned())
                    .or_else(|| {
                        cache
                            .client_order_id(&order_report.venue_order_id)
                            .and_then(|cid| cache.order(cid).cloned())
                    })
                    .is_some()
            };

            self.reconcile_order_status_report(order_report);

            if !existed {
                external_venue_ids.insert(order_report.venue_order_id);
            }
        }

        for fill_reports in mass_status.fill_reports().values() {
            for fill_report in fill_reports {
                if external_venue_ids.contains(&fill_report.venue_order_id) {
                    log::debug!(
                        "Skipping fill report for external order {}: covered by inferred fill",
                        fill_report.venue_order_id
                    );
                    continue;
                }

                self.reconcile_fill_report(fill_report);
            }
        }

        for position_reports in mass_status.position_reports().values() {
            for position_report in position_reports {
                self.reconcile_position_report(position_report);
            }
        }

        log::info!(
            "Mass status reconciliation complete: {} orders, {} fills, {} positions",
            mass_status.order_reports().len(),
            mass_status
                .fill_reports()
                .values()
                .map(|v| v.len())
                .sum::<usize>(),
            mass_status
                .position_reports()
                .values()
                .map(|v| v.len())
                .sum::<usize>()
        );
    }

    /// Executes a trading command by routing it to the appropriate execution client.
    pub fn execute(&self, command: TradingCommand) {
        self.execute_command(command);
    }

    /// Processes an order event, updating internal state and routing as needed.
    pub fn process(&mut self, event: &OrderEventAny) {
        self.handle_event(event);
    }

    /// Starts the execution engine.
    pub fn start(&mut self) {
        self.start_snapshot_timer();
        self.start_purge_timers();

        log::info!("Started");
    }

    /// Stops the execution engine.
    pub fn stop(&mut self) {
        self.stop_snapshot_timer();
        self.stop_purge_timers();

        log::info!("Stopped");
    }

    /// Resets the execution engine to its initial state.
    pub fn reset(&mut self) {
        self.pos_id_generator.reset();

        log::info!("Reset");
    }

    /// Disposes of the execution engine, releasing resources.
    pub fn dispose(&mut self) {
        log::info!("Disposed");
    }

    fn execute_command(&self, command: TradingCommand) {
        if self.config.debug {
            log::debug!("{RECV}{CMD} {command:?}");
        }

        if let Some(cid) = command.client_id()
            && self.external_clients.contains(&cid)
        {
            if self.config.debug {
                log::debug!("Skipping execution command for external client {cid}: {command:?}");
            }
            return;
        }

        let client = if let Some(adapter) = command
            .client_id()
            .and_then(|cid| self.clients.get(&cid))
            .or_else(|| {
                self.routing_map
                    .get(&command.instrument_id().venue)
                    .and_then(|client_id| self.clients.get(client_id))
            })
            .or(self.default_client.as_ref())
        {
            adapter.client.as_ref()
        } else {
            log::error!(
                "No execution client found for command: client_id={:?}, venue={}, command={command:?}",
                command.client_id(),
                command.instrument_id().venue,
            );

            let reason = format!(
                "No execution client found for client_id={:?}, venue={}",
                command.client_id(),
                command.instrument_id().venue,
            );

            match command {
                TradingCommand::SubmitOrder(cmd) => {
                    let cache = self.cache.borrow();
                    if let Some(order) = cache.order(&cmd.client_order_id) {
                        let order = order.clone();
                        drop(cache);
                        self.deny_order(&order, &reason);
                    }
                }
                TradingCommand::SubmitOrderList(cmd) => {
                    let orders: Vec<OrderAny> = self
                        .cache
                        .borrow()
                        .orders_for_ids(&cmd.order_list.client_order_ids, &cmd);

                    for order in &orders {
                        self.deny_order(order, &reason);
                    }
                }
                _ => {}
            }

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
            TradingCommand::QueryAccount(cmd) => self.handle_query_account(client, cmd),
        }
    }

    fn handle_submit_order(&self, client: &dyn ExecutionClient, cmd: SubmitOrder) {
        let client_order_id = cmd.client_order_id;

        let order = {
            let cache = self.cache.borrow();
            match cache.order(&client_order_id) {
                Some(order) => order.clone(),
                None => {
                    log::error!(
                        "Cannot handle submit order: order not found in cache for {client_order_id}"
                    );
                    return;
                }
            }
        };

        let order_venue = order.instrument_id().venue;
        let client_venue = client.venue();
        if order_venue != client_venue {
            self.deny_order(
                &order,
                &format!("Order venue {order_venue} does not match client venue {client_venue}"),
            );
            return;
        }

        let instrument_id = order.instrument_id();

        if self.config.snapshot_orders {
            self.create_order_state_snapshot(&order);
        }

        {
            let cache = self.cache.borrow();
            if cache.instrument(&instrument_id).is_none() {
                log::error!(
                    "Cannot handle submit order: no instrument found for {instrument_id}, {cmd}",
                );
                return;
            }
        }

        if self.config.manage_own_order_books && should_handle_own_book_order(&order) {
            let mut own_book = self.get_or_init_own_order_book(&order.instrument_id());
            own_book.add(order.to_own_book_order());
        }

        if let Err(e) = client.submit_order(cmd) {
            self.deny_order(&order, &format!("failed-to-submit-order-to-client: {e}"));
        }
    }

    fn handle_submit_order_list(&self, client: &dyn ExecutionClient, cmd: SubmitOrderList) {
        let orders: Vec<OrderAny> = self
            .cache
            .borrow()
            .orders_for_ids(&cmd.order_list.client_order_ids, &cmd);

        if orders.len() != cmd.order_list.client_order_ids.len() {
            for order in &orders {
                self.deny_order(
                    order,
                    &format!("Incomplete order list: missing orders in cache for {cmd}"),
                );
            }
            return;
        }

        let order_list_venue = cmd.instrument_id.venue;
        let client_venue = client.venue();
        if order_list_venue != client_venue {
            for order in &orders {
                self.deny_order(
                    order,
                    &format!("Order list venue {order_list_venue} does not match client venue {client_venue}"),
                );
            }
            return;
        }

        if self.config.snapshot_orders {
            for order in &orders {
                self.create_order_state_snapshot(order);
            }
        }

        {
            let cache = self.cache.borrow();
            if cache.instrument(&cmd.instrument_id).is_none() {
                log::error!(
                    "Cannot handle submit order list: no instrument found for {}, {cmd}",
                    cmd.instrument_id,
                );
                return;
            }
        }

        if self.config.manage_own_order_books {
            let mut own_book = self.get_or_init_own_order_book(&cmd.instrument_id);

            for order in &orders {
                if should_handle_own_book_order(order) {
                    own_book.add(order.to_own_book_order());
                }
            }
        }

        if let Err(e) = client.submit_order_list(cmd) {
            log::error!("Error submitting order list to client: {e}");
            for order in &orders {
                self.deny_order(
                    order,
                    &format!("failed-to-submit-order-list-to-client: {e}"),
                );
            }
        }
    }

    fn handle_modify_order(&self, client: &dyn ExecutionClient, cmd: ModifyOrder) {
        if let Err(e) = client.modify_order(cmd) {
            log::error!("Error modifying order: {e}");
        }
    }

    fn handle_cancel_order(&self, client: &dyn ExecutionClient, cmd: CancelOrder) {
        if let Err(e) = client.cancel_order(cmd) {
            log::error!("Error canceling order: {e}");
        }
    }

    fn handle_cancel_all_orders(&self, client: &dyn ExecutionClient, cmd: CancelAllOrders) {
        if let Err(e) = client.cancel_all_orders(cmd) {
            log::error!("Error canceling all orders: {e}");
        }
    }

    fn handle_batch_cancel_orders(&self, client: &dyn ExecutionClient, cmd: BatchCancelOrders) {
        if let Err(e) = client.batch_cancel_orders(cmd) {
            log::error!("Error batch canceling orders: {e}");
        }
    }

    fn handle_query_account(&self, client: &dyn ExecutionClient, cmd: QueryAccount) {
        if let Err(e) = client.query_account(cmd) {
            log::error!("Error querying account: {e}");
        }
    }

    fn handle_query_order(&self, client: &dyn ExecutionClient, cmd: QueryOrder) {
        if let Err(e) = client.query_order(cmd) {
            log::error!("Error querying order: {e}");
        }
    }

    fn create_order_state_snapshot(&self, order: &OrderAny) {
        if self.config.debug {
            log::debug!("Creating order state snapshot for {order}");
        }

        if self.cache.borrow().has_backing()
            && let Err(e) = self.cache.borrow().snapshot_order_state(order)
        {
            log::error!("Failed to snapshot order state: {e}");
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
    }

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
            OrderEventAny::Filled(fill) => {
                let oms_type = self.determine_oms_type(fill);
                let position_id = self.determine_position_id(*fill, oms_type, Some(&order));

                let mut fill = *fill;
                fill.position_id = Some(position_id);

                if self.apply_fill_to_order(&mut order, fill).is_ok() {
                    self.handle_order_fill(&order, fill, oms_type);
                }
            }
            _ => {
                let _ = self.apply_event_to_order(&mut order, event);
            }
        }
    }

    fn determine_oms_type(&self, fill: &OrderFilled) -> OmsType {
        // Check for strategy OMS override
        if let Some(oms_type) = self.oms_overrides.get(&fill.strategy_id) {
            return *oms_type;
        }

        // Use native venue OMS
        if let Some(client_id) = self.routing_map.get(&fill.instrument_id.venue)
            && let Some(client) = self.clients.get(client_id)
        {
            return client.oms_type();
        }

        if let Some(client) = &self.default_client {
            return client.oms_type();
        }

        OmsType::Netting // Default fallback
    }

    fn determine_position_id(
        &mut self,
        fill: OrderFilled,
        oms_type: OmsType,
        order: Option<&OrderAny>,
    ) -> PositionId {
        let cache = self.cache.borrow();
        let cached_position_id = cache.position_id(&fill.client_order_id()).copied();
        drop(cache);

        if self.config.debug {
            log::debug!(
                "Determining position ID for {}, position_id={:?}",
                fill.client_order_id(),
                cached_position_id,
            );
        }

        if let Some(position_id) = cached_position_id {
            if let Some(fill_position_id) = fill.position_id
                && fill_position_id != position_id
            {
                log::warn!(
                    "Incorrect position ID assigned to fill: \
                     cached={position_id}, assigned={fill_position_id}; \
                     re-assigning from cache",
                );
            }

            if self.config.debug {
                log::debug!("Assigned {position_id} to {}", fill.client_order_id());
            }

            return position_id;
        }

        let position_id = match oms_type {
            OmsType::Hedging => self.determine_hedging_position_id(fill, order),
            OmsType::Netting => self.determine_netting_position_id(fill),
            _ => self.determine_netting_position_id(fill),
        };

        let order = if let Some(o) = order {
            o.clone()
        } else {
            let cache = self.cache.borrow();
            cache
                .order(&fill.client_order_id())
                .cloned()
                .unwrap_or_else(|| {
                    panic!(
                        "Order for {} not found to determine position ID",
                        fill.client_order_id()
                    )
                })
        };

        if order.exec_algorithm_id().is_some()
            && let Some(exec_spawn_id) = order.exec_spawn_id()
        {
            let cache = self.cache.borrow();
            let primary = if let Some(p) = cache.order(&exec_spawn_id) {
                p.clone()
            } else {
                log::warn!(
                    "Primary exec spawn order {exec_spawn_id} not found, \
                     skipping position ID propagation"
                );
                return position_id;
            };
            let primary_already_indexed = cache.position_id(&primary.client_order_id()).is_some();
            drop(cache);

            if primary.position_id().is_none() && !primary_already_indexed {
                let mut cache = self.cache.borrow_mut();
                if let Some(primary_mut) = cache.mut_order(&exec_spawn_id) {
                    primary_mut.set_position_id(Some(position_id));
                }
                let _ = cache.add_position_id(
                    &position_id,
                    &primary.instrument_id().venue,
                    &primary.client_order_id(),
                    &primary.strategy_id(),
                );
                log::debug!("Assigned primary order {position_id}");
            }
        }

        position_id
    }

    fn determine_hedging_position_id(
        &mut self,
        fill: OrderFilled,
        order: Option<&OrderAny>,
    ) -> PositionId {
        // Check if position ID already exists
        if let Some(position_id) = fill.position_id {
            if self.config.debug {
                log::debug!("Already had a position ID of: {position_id}");
            }
            return position_id;
        }

        let cache = self.cache.borrow();

        let order = if let Some(o) = order {
            o
        } else {
            match cache.order(&fill.client_order_id()) {
                Some(o) => o,
                None => {
                    panic!(
                        "Order for {} not found to determine position ID",
                        fill.client_order_id()
                    );
                }
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

    fn apply_fill_to_order(&self, order: &mut OrderAny, fill: OrderFilled) -> anyhow::Result<()> {
        if order.is_duplicate_fill(&fill) {
            log::warn!(
                "Duplicate fill: {} trade_id={} already applied, skipping",
                order.client_order_id(),
                fill.trade_id
            );
            anyhow::bail!("Duplicate fill");
        }

        self.check_overfill(order, &fill)?;
        let event = OrderEventAny::Filled(fill);
        self.apply_order_event(order, &event)
    }

    fn apply_event_to_order(
        &self,
        order: &mut OrderAny,
        event: &OrderEventAny,
    ) -> anyhow::Result<()> {
        self.apply_order_event(order, event)
    }

    fn apply_order_event(&self, order: &mut OrderAny, event: &OrderEventAny) -> anyhow::Result<()> {
        if let Err(e) = order.apply(event.clone()) {
            match e {
                OrderError::InvalidStateTransition => {
                    // Event already applied to order (e.g., from reconciliation or duplicate processing)
                    // Log warning and continue with downstream processing (cache update, publishing, etc.)
                    log::warn!("InvalidStateTrigger: {e}, did not apply {event}");
                }
                OrderError::DuplicateFill(trade_id) => {
                    // Duplicate fill detected at order level (secondary safety check)
                    log::warn!(
                        "Duplicate fill rejected at order level: trade_id={trade_id}, did not apply {event}"
                    );
                    anyhow::bail!("{e}");
                }
                _ => {
                    // Protection against invalid IDs and other invariants
                    log::error!("Error applying event: {e}, did not apply {event}");

                    if should_handle_own_book_order(order) {
                        self.cache.borrow_mut().update_own_order_book(order);
                    }
                    anyhow::bail!("{e}");
                }
            }
        }

        if let Err(e) = self.cache.borrow_mut().update_order(order) {
            log::error!("Error updating order in cache: {e}");
        }

        if self.config.debug {
            log::debug!("{SEND}{EVT} {event}");
        }

        let topic = switchboard::get_event_orders_topic(event.strategy_id());
        msgbus::publish_order_event(topic, event);

        if self.config.snapshot_orders {
            self.create_order_state_snapshot(order);
        }

        Ok(())
    }

    fn check_overfill(&self, order: &OrderAny, fill: &OrderFilled) -> anyhow::Result<()> {
        let potential_overfill = order.calculate_overfill(fill.last_qty);

        if potential_overfill.is_positive() {
            if self.config.allow_overfills {
                log::warn!(
                    "Order overfill detected: {} potential_overfill={}, current_filled={}, last_qty={}, quantity={}",
                    order.client_order_id(),
                    potential_overfill,
                    order.filled_qty(),
                    fill.last_qty,
                    order.quantity()
                );
            } else {
                let msg = format!(
                    "Order overfill rejected: {} potential_overfill={}, current_filled={}, last_qty={}, quantity={}. \
                Set `allow_overfills=true` in ExecutionEngineConfig to allow overfills.",
                    order.client_order_id(),
                    potential_overfill,
                    order.filled_qty(),
                    fill.last_qty,
                    order.quantity()
                );
                anyhow::bail!("{msg}");
            }
        }

        Ok(())
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

        // Skip portfolio position updates for combo fills (spread instruments)
        // Combo fills are only used for order management, not portfolio updates
        let position = if instrument.is_spread() {
            None
        } else {
            self.handle_position_update(&instrument, fill, oms_type);
            let position_id = fill.position_id.unwrap();
            self.cache.borrow().position(&position_id).cloned()
        };

        // Handle contingent orders for both spread and non-spread instruments
        // For spread instruments, contingent orders work without position linkage
        if matches!(order.contingency_type(), Some(ContingencyType::Oto)) {
            // For non-spread instruments, link to position if available
            if !instrument.is_spread()
                && let Some(ref pos) = position
                && pos.is_open()
            {
                let position_id = pos.id;

                for client_order_id in order.linked_order_ids().unwrap_or_default() {
                    let mut cache = self.cache.borrow_mut();
                    let contingent_order = cache.mut_order(client_order_id);
                    if let Some(contingent_order) = contingent_order
                        && contingent_order.position_id().is_none()
                    {
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
            // For spread instruments, contingent orders can still be triggered
            // but without position linkage (since no position is created for spreads)
        }
    }

    /// Handle position creation or update for a fill.
    ///
    /// This function mirrors the Python `_handle_position_update` method.
    fn handle_position_update(
        &mut self,
        instrument: &InstrumentAny,
        fill: OrderFilled,
        oms_type: OmsType,
    ) {
        let position_id = if let Some(position_id) = fill.position_id {
            position_id
        } else {
            log::error!("Cannot handle position update: no position ID found for fill {fill}");
            return;
        };

        let position_opt = self.cache.borrow().position(&position_id).cloned();

        match position_opt {
            None => {
                // Position is None - open new position
                if self.open_position(instrument, None, fill, oms_type).is_ok() {
                    // Position opened successfully
                }
            }
            Some(pos) if pos.is_closed() => {
                // Position is closed - open new position
                if self
                    .open_position(instrument, Some(&pos), fill, oms_type)
                    .is_ok()
                {
                    // Position opened successfully
                }
            }
            Some(mut pos) => {
                if self.will_flip_position(&pos, fill) {
                    // Position will flip
                    self.flip_position(instrument, &mut pos, fill, oms_type);
                } else {
                    // Update existing position
                    self.update_position(&mut pos, fill);
                }
            }
        }
    }

    fn open_position(
        &self,
        instrument: &InstrumentAny,
        position: Option<&Position>,
        fill: OrderFilled,
        oms_type: OmsType,
    ) -> anyhow::Result<()> {
        if let Some(position) = position {
            if Self::is_duplicate_closed_fill(position, &fill) {
                log::warn!(
                    "Ignoring duplicate fill {} for closed position {}; no position reopened (side={:?}, qty={}, px={})",
                    fill.trade_id,
                    position.id,
                    fill.order_side,
                    fill.last_qty,
                    fill.last_px
                );
                return Ok(());
            }
            self.reopen_position(position, oms_type)?;
        }

        let position = Position::new(instrument, fill);
        self.cache.borrow_mut().add_position(&position, oms_type)?;

        if self.config.snapshot_positions {
            self.create_position_state_snapshot(&position);
        }

        let ts_init = self.clock.borrow().timestamp_ns();
        let event = PositionOpened::create(&position, &fill, UUID4::new(), ts_init);
        let topic = switchboard::get_event_positions_topic(event.strategy_id);
        msgbus::publish_position_event(topic, &PositionEvent::PositionOpened(event));

        Ok(())
    }

    fn is_duplicate_closed_fill(position: &Position, fill: &OrderFilled) -> bool {
        position.events.iter().any(|event| {
            event.trade_id == fill.trade_id
                && event.order_side == fill.order_side
                && event.last_px == fill.last_px
                && event.last_qty == fill.last_qty
        })
    }

    fn reopen_position(&self, position: &Position, oms_type: OmsType) -> anyhow::Result<()> {
        if oms_type == OmsType::Netting {
            if position.is_open() {
                anyhow::bail!(
                    "Cannot reopen position {} (oms_type=NETTING): reopening is only valid for closed positions in NETTING mode",
                    position.id
                );
            }
            // Snapshot closed position if reopening (NETTING mode)
            self.cache.borrow_mut().snapshot_position(position)?;
        } else {
            // HEDGING mode
            log::warn!(
                "Received fill for closed position {} in HEDGING mode; creating new position and ignoring previous state",
                position.id
            );
        }
        Ok(())
    }

    fn update_position(&self, position: &mut Position, fill: OrderFilled) {
        // Apply the fill to the position
        position.apply(&fill);

        // Check if position is closed after applying the fill
        let is_closed = position.is_closed();

        // Update position in cache - this should handle the closed state tracking
        if let Err(e) = self.cache.borrow_mut().update_position(position) {
            log::error!("Failed to update position: {e:?}");
            return;
        }

        // Verify cache state after update
        let cache = self.cache.borrow();

        drop(cache);

        // Create position state snapshot if enabled
        if self.config.snapshot_positions {
            self.create_position_state_snapshot(position);
        }

        // Create and publish appropriate position event
        let topic = switchboard::get_event_positions_topic(position.strategy_id);
        let ts_init = self.clock.borrow().timestamp_ns();

        if is_closed {
            let event = PositionClosed::create(position, &fill, UUID4::new(), ts_init);
            msgbus::publish_position_event(topic, &PositionEvent::PositionClosed(event));
        } else {
            let event = PositionChanged::create(position, &fill, UUID4::new(), ts_init);
            msgbus::publish_position_event(topic, &PositionEvent::PositionChanged(event));
        }
    }

    fn will_flip_position(&self, position: &Position, fill: OrderFilled) -> bool {
        position.is_opposite_side(fill.order_side) && (fill.last_qty.raw > position.quantity.raw)
    }

    fn flip_position(
        &mut self,
        instrument: &InstrumentAny,
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
                position.quantity.raw.abs_diff(fill.last_qty.raw), // Equivalent to Python's abs(position.quantity - fill.last_qty)
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
            log::error!("Commission is not available");
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

            // Snapshot closed position before reusing ID (NETTING mode)
            if oms_type == OmsType::Netting
                && let Err(e) = self.cache.borrow_mut().snapshot_position(position)
            {
                log::error!("Failed to snapshot position during flip: {e:?}");
            }
        }

        // Guard against flipping a position with a zero fill size
        if difference.raw == 0 {
            log::warn!(
                "Zero fill size during position flip calculation, this could be caused by a mismatch between instrument `size_precision` and a quantity `size_precision`"
            );
            return;
        }

        let position_id_flip = if oms_type == OmsType::Hedging
            && let Some(position_id) = fill.position_id
            && position_id.is_virtual()
        {
            // Generate new position ID for flipped virtual position (Hedging OMS only)
            Some(self.pos_id_generator.generate(fill.strategy_id, true))
        } else {
            // Default: use the same position ID as the fill (Python behavior)
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

        if oms_type == OmsType::Hedging
            && let Some(position_id) = fill.position_id
            && position_id.is_virtual()
        {
            log::warn!("Closing position {fill_split1:?}");
            log::warn!("Flipping position {fill_split2:?}");
        }

        // Open flipped position
        if let Err(e) = self.open_position(instrument, None, fill_split2, oms_type) {
            log::error!("Failed to open flipped position: {e:?}");
        }
    }

    /// Sets the internal position ID generator counts based on existing cached positions.
    pub fn set_position_id_counts(&mut self) {
        let cache = self.cache.borrow();
        let positions = cache.positions(None, None, None, None, None);

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

    fn deny_order(&self, order: &OrderAny, reason: &str) {
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
        msgbus::publish_order_event(topic, &OrderEventAny::Denied(denied));

        if self.config.snapshot_orders {
            self.create_order_state_snapshot(&order);
        }
    }

    fn get_or_init_own_order_book(&self, instrument_id: &InstrumentId) -> RefMut<'_, OwnOrderBook> {
        let mut cache = self.cache.borrow_mut();
        if cache.own_order_book_mut(instrument_id).is_none() {
            let own_book = OwnOrderBook::new(*instrument_id);
            cache.add_own_order_book(own_book).unwrap();
        }

        RefMut::map(cache, |c| c.own_order_book_mut(instrument_id).unwrap())
    }
}
