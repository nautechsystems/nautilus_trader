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

//! Provides a high-performance `DataEngine` for all environments.
//!
//! The `DataEngine` is the central component of the entire data stack.
//! The data engines primary responsibility is to orchestrate interactions between
//! the `DataClient` instances, and the rest of the platform. This includes sending
//! requests to, and receiving responses from, data endpoints via its registered
//! data clients.
//!
//! The engine employs a simple fan-in fan-out messaging pattern to execute
//! `DataCommand` type messages, and process `DataResponse` messages or market data
//! objects.
//!
//! Alternative implementations can be written on top of the generic engine - which
//! just need to override the `execute`, `process`, `send` and `receive` methods.

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_assignments)]

pub mod book;
pub mod config;
pub mod runner;

#[cfg(test)]
mod tests;

use std::{
    any::Any,
    cell::{Ref, RefCell},
    collections::{HashMap, HashSet, VecDeque},
    num::NonZeroU64,
    rc::Rc,
    sync::Arc,
};

use book::{BookSnapshotInfo, BookSnapshotter, BookUpdater};
use config::DataEngineConfig;
use indexmap::IndexMap;
use nautilus_common::{
    cache::Cache,
    clock::Clock,
    logging::{RECV, RES},
    messages::data::{Action, DataRequest, DataResponse, SubscriptionCommand},
    msgbus::{
        handler::{MessageHandler, ShareableMessageHandler},
        MessageBus,
    },
    timer::TimeEventCallback,
};
use nautilus_core::{
    correctness::{check_key_in_index_map, check_key_not_in_index_map, FAILED},
    datetime::{millis_to_nanos, NANOSECONDS_IN_MILLISECOND, NANOSECONDS_IN_SECOND},
};
use nautilus_model::{
    data::{
        bar::{Bar, BarType},
        delta::OrderBookDelta,
        deltas::OrderBookDeltas,
        depth::OrderBookDepth10,
        quote::QuoteTick,
        trade::TradeTick,
        Data, DataType,
    },
    enums::{BookType, RecordFlag},
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{any::InstrumentAny, synthetic::SyntheticInstrument},
    orderbook::book::OrderBook,
};
use ustr::Ustr;

use crate::{aggregation::BarAggregator, client::DataClientAdapter};

/// Provides a high-performance `DataEngine` for all environments.
pub struct DataEngine {
    clock: Box<dyn Clock>,
    cache: Rc<RefCell<Cache>>,
    msgbus: Rc<RefCell<MessageBus>>,
    clients: IndexMap<ClientId, DataClientAdapter>,
    default_client: Option<DataClientAdapter>,
    external_clients: HashSet<ClientId>,
    routing_map: IndexMap<Venue, ClientId>,
    book_intervals: HashMap<NonZeroU64, HashSet<InstrumentId>>,
    book_updaters: HashMap<InstrumentId, Rc<BookUpdater>>,
    book_snapshotters: HashMap<InstrumentId, Rc<BookSnapshotter>>,
    bar_aggregators: Vec<Box<dyn BarAggregator>>, // TODO: dyn for now
    synthetic_quote_feeds: HashMap<InstrumentId, Vec<SyntheticInstrument>>,
    synthetic_trade_feeds: HashMap<InstrumentId, Vec<SyntheticInstrument>>,
    buffered_deltas_map: HashMap<InstrumentId, Vec<OrderBookDelta>>, // TODO: Use OrderBookDeltas?
    msgbus_priority: u8,
    command_queue: VecDeque<SubscriptionCommand>,
    config: DataEngineConfig,
}

impl DataEngine {
    /// Creates a new [`DataEngine`] instance.
    #[must_use]
    pub fn new(
        clock: Box<dyn Clock>,
        cache: Rc<RefCell<Cache>>,
        msgbus: Rc<RefCell<MessageBus>>,
        config: Option<DataEngineConfig>,
    ) -> Self {
        Self {
            clock,
            cache,
            msgbus,
            clients: IndexMap::new(),
            default_client: None,
            external_clients: HashSet::new(),
            routing_map: IndexMap::new(),
            book_intervals: HashMap::new(),
            book_updaters: HashMap::new(),
            book_snapshotters: HashMap::new(),
            bar_aggregators: Vec::new(),
            synthetic_quote_feeds: HashMap::new(),
            synthetic_trade_feeds: HashMap::new(),
            buffered_deltas_map: HashMap::new(),
            msgbus_priority: 10, // High-priority for built-in component
            command_queue: VecDeque::new(),
            config: config.unwrap_or_default(),
        }
    }

    /// Provides read-only access to the cache.
    #[must_use]
    pub fn get_cache(&self) -> Ref<'_, Cache> {
        self.cache.borrow()
    }

    // pub fn register_catalog(&mut self, catalog: ParquetDataCatalog) {}  TODO: Implement catalog

    /// Registers the given data `client` with the engine as the default routing client.
    ///
    /// When a specific venue routing cannot be found, this client will receive messages.
    ///
    /// # Warnings
    ///
    /// Any existing default routing client will be overwritten.
    /// TODO: change this to suit message bus behaviour
    pub fn register_default_client(&mut self, client: DataClientAdapter) {
        log::info!("Registered default client {}", client.client_id());
        self.default_client = Some(client);
    }

    pub fn start(self) {
        self.clients.values().for_each(|client| client.start());
    }

    pub fn stop(self) {
        self.clients.values().for_each(|client| client.stop());
    }

    pub fn reset(self) {
        self.clients.values().for_each(|client| client.reset());
    }

    pub fn dispose(mut self) {
        self.clients.values().for_each(|client| client.dispose());
        self.clock.cancel_timers();
    }

    pub fn connect(&self) {
        todo!() //  Implement actual client connections for a live/sandbox context
    }

    pub fn disconnect(&self) {
        todo!() // Implement actual client connections for a live/sandbox context
    }

    #[must_use]
    pub fn check_connected(&self) -> bool {
        self.clients.values().all(|client| client.is_connected())
    }

    #[must_use]
    pub fn check_disconnected(&self) -> bool {
        self.clients.values().all(|client| !client.is_connected())
    }

    #[must_use]
    pub fn registed_clients(&self) -> Vec<ClientId> {
        self.clients.keys().copied().collect()
    }

    // -- SUBSCRIPTIONS ---------------------------------------------------------------------------

    fn collect_subscriptions<F, T>(&self, get_subs: F) -> Vec<T>
    where
        F: Fn(&DataClientAdapter) -> &HashSet<T>,
        T: Clone,
    {
        let mut subs = Vec::new();
        for client in self.clients.values() {
            subs.extend(get_subs(client).iter().cloned());
        }
        subs
    }

    fn get_client(&self, client_id: &ClientId, venue: &Venue) -> Option<&DataClientAdapter> {
        match self.clients.get(client_id) {
            Some(client) => Some(client),
            None => self
                .routing_map
                .get(venue)
                .and_then(|client_id: &ClientId| self.clients.get(client_id)),
        }
    }

    fn get_client_mut(
        &mut self,
        client_id: &ClientId,
        venue: &Venue,
    ) -> Option<&mut DataClientAdapter> {
        // Try to get client directly from clients map
        if self.clients.contains_key(client_id) {
            return self.clients.get_mut(client_id);
        }

        // If not found, try to get client_id from routing map
        if let Some(mapped_client_id) = self.routing_map.get(venue) {
            return self.clients.get_mut(mapped_client_id);
        }

        None
    }

    #[must_use]
    pub fn subscribed_custom_data(&self) -> Vec<DataType> {
        self.collect_subscriptions(|client| &client.subscriptions_generic)
    }

    #[must_use]
    pub fn subscribed_instruments(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_instrument)
    }

    #[must_use]
    pub fn subscribed_order_book_deltas(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_order_book_delta)
    }

    #[must_use]
    pub fn subscribed_order_book_snapshots(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_order_book_snapshot)
    }

    #[must_use]
    pub fn subscribed_quote_ticks(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_quote_tick)
    }

    #[must_use]
    pub fn subscribed_trade_ticks(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_trade_tick)
    }

    #[must_use]
    pub fn subscribed_bars(&self) -> Vec<BarType> {
        self.collect_subscriptions(|client| &client.subscriptions_bar)
    }

    #[must_use]
    pub fn subscribed_instrument_status(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_instrument_status)
    }

    #[must_use]
    pub fn subscribed_instrument_close(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_instrument_close)
    }

    pub fn on_start(self) {
        todo!()
    }

    pub fn on_stop(self) {
        todo!()
    }

    /// Registers a new [`DataClientAdapter`]
    ///
    /// # Panics
    ///
    /// This function panics:
    /// - If a client with the same client ID has already been registered.
    pub fn register_client(&mut self, client: DataClientAdapter, routing: Option<Venue>) {
        check_key_not_in_index_map(&client.client_id, &self.clients, "client_id", "clients")
            .expect(FAILED);

        if let Some(routing) = routing {
            self.routing_map.insert(routing, client.client_id());
            log::info!("Set client {} routing for {routing}", client.client_id());
        }

        log::info!("Registered client {}", client.client_id());
        self.clients.insert(client.client_id, client);
    }

    /// Deregisters a [`DataClientAdapter`]
    ///
    /// # Panics
    ///
    /// This function panics:
    /// - If a client with the same client ID has not been registered.
    pub fn deregister_client(&mut self, client_id: &ClientId) {
        check_key_in_index_map(client_id, &self.clients, "client_id", "clients").expect(FAILED);

        self.clients.shift_remove(client_id);
        log::info!("Deregistered client {client_id}");
    }

    pub fn run(&mut self) {
        let commands: Vec<_> = self.command_queue.drain(..).collect();
        for cmd in commands {
            self.execute(cmd);
        }
    }

    pub fn enqueue(&mut self, cmd: &dyn Any) {
        if let Some(cmd) = cmd.downcast_ref::<SubscriptionCommand>() {
            self.command_queue.push_back(cmd.clone());
        } else {
            log::error!("Invalid message type received: {cmd:?}");
        }
    }

    pub fn execute(&mut self, cmd: SubscriptionCommand) {
        let result = match cmd.action {
            Action::Subscribe => match cmd.data_type.type_name() {
                stringify!(OrderBookDelta) => self.handle_subscribe_book_deltas(&cmd),
                stringify!(OrderBook) => self.handle_subscribe_book_snapshots(&cmd),
                stringify!(Bar) => self.handle_subscribe_bars(&cmd),
                _ => Ok(()), // No other actions for engine
            },
            Action::Unsubscribe => match cmd.data_type.type_name() {
                stringify!(OrderBookDelta) => self.handle_unsubscribe_book_deltas(&cmd),
                stringify!(OrderBook) => self.handle_unsubscribe_book_snapshots(&cmd),
                stringify!(Bar) => self.handle_unsubscribe_bars(&cmd),
                _ => Ok(()), // No other actions for engine
            },
        };

        if let Err(e) = result {
            log::error!("{e}");
            return;
        }

        if let Some(client) = self.get_client_mut(&cmd.client_id, &cmd.venue) {
            client.execute(cmd);
        } else {
            log::error!(
                "Cannot handle command: no client found for {}",
                cmd.client_id
            );
        }
    }

    /// Sends a [`DataRequest`] to an endpoint that must be a data client implementation.
    pub fn request(&self, req: DataRequest) {
        if let Some(client) = self.get_client(&req.client_id, &req.venue) {
            client.through_request(req);
        } else {
            log::error!(
                "Cannot handle request: no client found for {}",
                req.client_id
            );
        }
    }

    pub fn process(&mut self, data: &dyn Any) {
        if let Some(instrument) = data.downcast_ref::<InstrumentAny>() {
            self.handle_instrument(instrument.clone());
        } else {
            log::error!("Cannot process data {data:?}, type is unrecognized");
        }
    }

    pub fn process_data(&mut self, data: Data) {
        match data {
            Data::Delta(delta) => self.handle_delta(delta),
            Data::Deltas(deltas) => self.handle_deltas(deltas.into_inner()),
            Data::Depth10(depth) => self.handle_depth10(depth),
            Data::Quote(quote) => self.handle_quote(quote),
            Data::Trade(trade) => self.handle_trade(trade),
            Data::Bar(bar) => self.handle_bar(bar),
        }
    }

    pub fn response(&self, resp: DataResponse) {
        log::debug!("{}", format!("{RECV}{RES} {resp:?}"));

        match resp.data_type.type_name() {
            stringify!(InstrumentAny) => {
                let instruments = Arc::downcast::<Vec<InstrumentAny>>(resp.data.clone())
                    .expect("Invalid response data");
                self.handle_instruments(instruments);
            }
            stringify!(QuoteTick) => {
                let quotes = Arc::downcast::<Vec<QuoteTick>>(resp.data.clone())
                    .expect("Invalid response data");
                self.handle_quotes(quotes);
            }
            stringify!(TradeTick) => {
                let trades = Arc::downcast::<Vec<TradeTick>>(resp.data.clone())
                    .expect("Invalid response data");
                self.handle_trades(trades);
            }
            stringify!(Bar) => {
                let bars =
                    Arc::downcast::<Vec<Bar>>(resp.data.clone()).expect("Invalid response data");
                self.handle_bars(bars);
            }
            type_name => log::error!("Cannot handle request, type {type_name} is unrecognized"),
        }

        self.msgbus.as_ref().borrow().send_response(resp);
    }

    // -- DATA HANDLERS ---------------------------------------------------------------------------

    fn handle_instrument(&mut self, instrument: InstrumentAny) {
        if let Err(e) = self
            .cache
            .as_ref()
            .borrow_mut()
            .add_instrument(instrument.clone())
        {
            log::error!("Error on cache insert: {e}");
        }

        let mut msgbus = self.msgbus.borrow_mut();
        let topic = msgbus.switchboard.get_instrument_topic(instrument.id());
        msgbus.publish(&topic, &instrument as &dyn Any); // TODO: Optimize
    }

    fn handle_delta(&mut self, delta: OrderBookDelta) {
        let deltas = if self.config.buffer_deltas {
            let buffer_deltas = self
                .buffered_deltas_map
                .entry(delta.instrument_id)
                .or_default();
            buffer_deltas.push(delta);

            if !RecordFlag::F_LAST.matches(delta.flags) {
                return; // Not the last delta for event
            }

            // SAFETY: We know the deltas exists already
            let deltas = self
                .buffered_deltas_map
                .remove(&delta.instrument_id)
                .unwrap();
            OrderBookDeltas::new(delta.instrument_id, deltas)
        } else {
            OrderBookDeltas::new(delta.instrument_id, vec![delta])
        };

        let mut msgbus = self.msgbus.borrow_mut();
        let topic = msgbus.switchboard.get_deltas_topic(deltas.instrument_id);
        msgbus.publish(&topic, &deltas as &dyn Any);
    }

    fn handle_deltas(&mut self, deltas: OrderBookDeltas) {
        let deltas = if self.config.buffer_deltas {
            let buffer_deltas = self
                .buffered_deltas_map
                .entry(deltas.instrument_id)
                .or_default();
            buffer_deltas.extend(deltas.deltas);

            let mut is_last_delta = false;
            for delta in buffer_deltas.iter_mut() {
                if RecordFlag::F_LAST.matches(delta.flags) {
                    is_last_delta = true;
                }
            }

            if !is_last_delta {
                return;
            }

            // SAFETY: We know the deltas exists already
            let buffer_deltas = self
                .buffered_deltas_map
                .remove(&deltas.instrument_id)
                .unwrap();
            OrderBookDeltas::new(deltas.instrument_id, buffer_deltas)
        } else {
            deltas
        };

        let mut msgbus = self.msgbus.borrow_mut();
        let topic = msgbus.switchboard.get_deltas_topic(deltas.instrument_id);
        msgbus.publish(&topic, &deltas as &dyn Any); // TODO: Optimize
    }

    fn handle_depth10(&mut self, depth: OrderBookDepth10) {
        let mut msgbus = self.msgbus.borrow_mut();
        let topic = msgbus.switchboard.get_depth_topic(depth.instrument_id);
        msgbus.publish(&topic, &depth as &dyn Any); // TODO: Optimize
    }

    fn handle_quote(&mut self, quote: QuoteTick) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_quote(quote) {
            log::error!("Error on cache insert: {e}");
        }

        // TODO: Handle synthetics

        let mut msgbus = self.msgbus.borrow_mut();
        let topic = msgbus.switchboard.get_quote_topic(quote.instrument_id);
        msgbus.publish(&topic, &quote as &dyn Any); // TODO: Optimize
    }

    fn handle_trade(&mut self, trade: TradeTick) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_trade(trade) {
            log::error!("Error on cache insert: {e}");
        }

        // TODO: Handle synthetics

        let mut msgbus = self.msgbus.borrow_mut();
        let topic = msgbus.switchboard.get_trade_topic(trade.instrument_id);
        msgbus.publish(&topic, &trade as &dyn Any); // TODO: Optimize
    }

    fn handle_bar(&mut self, bar: Bar) {
        // TODO: Handle additional bar logic
        if self.config.validate_data_sequence {
            if let Some(last_bar) = self.cache.as_ref().borrow().bar(&bar.bar_type) {
                if bar.ts_event < last_bar.ts_event {
                    log::warn!(
                        "Bar {bar} was prior to last bar `ts_event` {}",
                        last_bar.ts_event
                    );
                    return; // Bar is out of sequence
                }
                if bar.ts_init < last_bar.ts_init {
                    log::warn!(
                        "Bar {bar} was prior to last bar `ts_init` {}",
                        last_bar.ts_init
                    );
                    return; // Bar is out of sequence
                }
                // TODO: Implement `bar.is_revision` logic
            }
        }

        if let Err(e) = self.cache.as_ref().borrow_mut().add_bar(bar) {
            log::error!("Error on cache insert: {e}");
        }

        let mut msgbus = self.msgbus.borrow_mut();
        let topic = msgbus.switchboard.get_bar_topic(bar.bar_type);
        msgbus.publish(&topic, &bar as &dyn Any); // TODO: Optimize
    }

    // -- SUBSCRIPTION HANDLERS -------------------------------------------------------------------

    fn handle_subscribe_book_deltas(
        &mut self,
        command: &SubscriptionCommand,
    ) -> anyhow::Result<()> {
        let instrument_id = command.data_type.instrument_id().ok_or_else(|| {
            anyhow::anyhow!(
                "Invalid order book deltas subscription: did not contain an `instrument_id`, {}",
                command.data_type
            )
        })?;

        if instrument_id.is_synthetic() {
            anyhow::bail!("Cannot subscribe for synthetic instrument `OrderBookDelta` data");
        }

        if !self.subscribed_order_book_deltas().contains(&instrument_id) {
            return Ok(());
        }

        let data_type = command.data_type.clone();
        let book_type = data_type.book_type();
        let depth = data_type.depth();
        let managed = data_type.managed();

        self.setup_order_book(&instrument_id, book_type, depth, true, managed)?;

        Ok(())
    }

    fn handle_subscribe_book_snapshots(
        &mut self,
        command: &SubscriptionCommand,
    ) -> anyhow::Result<()> {
        let instrument_id = command.data_type.instrument_id().ok_or_else(|| {
            anyhow::anyhow!(
                "Invalid order book snapshots subscription: did not contain an `instrument_id`, {}",
                command.data_type
            )
        })?;

        if self.subscribed_order_book_deltas().contains(&instrument_id) {
            return Ok(());
        }

        if instrument_id.is_synthetic() {
            anyhow::bail!("Cannot subscribe for synthetic instrument `OrderBookDelta` data");
        }

        let data_type = command.data_type.clone();
        let book_type = data_type.book_type();
        let depth = data_type.depth();
        let interval_ms = data_type.interval_ms();
        let managed = data_type.managed();

        {
            if !self.book_intervals.contains_key(&interval_ms) {
                let interval_ns = millis_to_nanos(interval_ms.get() as f64);
                let mut msgbus = self.msgbus.borrow_mut();
                let topic = msgbus.switchboard.get_snapshots_topic(instrument_id);

                let snap_info = BookSnapshotInfo {
                    instrument_id,
                    venue: instrument_id.venue,
                    is_composite: instrument_id.symbol.is_composite(),
                    root: Ustr::from(instrument_id.symbol.root()),
                    topic,
                    interval_ms,
                };

                let now_ns = self.clock.timestamp_ns().as_u64();
                let mut start_time_ns = now_ns - (now_ns % interval_ns);

                if start_time_ns - NANOSECONDS_IN_MILLISECOND <= now_ns {
                    start_time_ns += NANOSECONDS_IN_SECOND; // Add one second
                }

                let snapshotter = Rc::new(BookSnapshotter::new(
                    snap_info,
                    self.cache.clone(),
                    self.msgbus.clone(),
                ));
                self.book_snapshotters
                    .insert(instrument_id, snapshotter.clone());
                let timer_name = snapshotter.timer_name;

                let callback =
                    TimeEventCallback::Rust(Rc::new(move |event| snapshotter.snapshot(event)));

                self.clock.set_timer_ns(
                    &timer_name,
                    interval_ns,
                    start_time_ns.into(),
                    None,
                    Some(callback),
                );
            }
        }

        self.setup_order_book(&instrument_id, book_type, depth, false, managed)?;

        Ok(())
    }

    fn handle_subscribe_bars(&mut self, command: &SubscriptionCommand) -> anyhow::Result<()> {
        // TODO: Handle aggregators
        Ok(())
    }

    fn handle_unsubscribe_book_deltas(
        &mut self,
        command: &SubscriptionCommand,
    ) -> anyhow::Result<()> {
        let instrument_id = command.data_type.instrument_id().ok_or_else(|| {
            anyhow::anyhow!(
                "Invalid order book snapshots subscription: did not contain an `instrument_id`, {}",
                command.data_type
            )
        })?;

        if !self.subscribed_order_book_deltas().contains(&instrument_id) {
            log::warn!("Cannot unsubscribe from `OrderBookDeltas` data: not subscribed");
            return Ok(());
        }

        let topics = {
            let mut msgbus = self.msgbus.borrow_mut();
            vec![
                msgbus.switchboard.get_deltas_topic(instrument_id),
                msgbus.switchboard.get_depth_topic(instrument_id),
                msgbus.switchboard.get_snapshots_topic(instrument_id),
            ]
        };

        self.maintain_book_updater(&instrument_id, &topics);
        self.maintain_book_snapshotter(&instrument_id);

        Ok(())
    }

    fn handle_unsubscribe_book_snapshots(
        &mut self,
        command: &SubscriptionCommand,
    ) -> anyhow::Result<()> {
        let instrument_id = command.data_type.instrument_id().ok_or_else(|| {
            anyhow::anyhow!(
                "Invalid order book snapshots subscription: did not contain an `instrument_id`, {}",
                command.data_type
            )
        })?;

        if !self.subscribed_order_book_deltas().contains(&instrument_id) {
            log::warn!("Cannot unsubscribe from `OrderBook` snapshots: not subscribed");
            return Ok(());
        }

        let topics = {
            let mut msgbus = self.msgbus.borrow_mut();
            vec![
                msgbus.switchboard.get_deltas_topic(instrument_id),
                msgbus.switchboard.get_depth_topic(instrument_id),
                msgbus.switchboard.get_snapshots_topic(instrument_id),
            ]
        };

        self.maintain_book_updater(&instrument_id, &topics);
        self.maintain_book_snapshotter(&instrument_id);

        Ok(())
    }

    fn handle_unsubscribe_bars(&mut self, command: &SubscriptionCommand) -> anyhow::Result<()> {
        // TODO: Handle aggregators
        Ok(())
    }

    fn maintain_book_updater(&mut self, instrument_id: &InstrumentId, topics: &[Ustr]) {
        if let Some(updater) = self.book_updaters.get(instrument_id) {
            let handler = ShareableMessageHandler(updater.clone());
            let mut msgbus = self.msgbus.borrow_mut();

            // Unsubscribe handler if it is the last subscriber
            for topic in topics {
                if msgbus.subscriptions_count(*topic) == 1
                    && msgbus.is_subscribed(*topic, handler.clone())
                {
                    log::debug!("Unsubscribing BookUpdater from {topic}");
                    msgbus.unsubscribe(*topic, handler.clone());
                }
            }

            // Check remaining subscriptions, if none then remove updater
            let still_subscribed = topics
                .iter()
                .any(|topic| msgbus.is_subscribed(*topic, handler.clone()));
            if !still_subscribed {
                self.book_updaters.remove(instrument_id);
                log::debug!("Removed BookUpdater for instrument ID {instrument_id}");
            }
        }
    }

    fn maintain_book_snapshotter(&mut self, instrument_id: &InstrumentId) {
        if let Some(snapshotter) = self.book_snapshotters.get(instrument_id) {
            let mut msgbus = self.msgbus.borrow_mut();

            let topic = msgbus.switchboard.get_snapshots_topic(*instrument_id);

            // Check remaining snapshot subscriptions, if none then remove snapshotter
            if msgbus.subscriptions_count(topic) == 0 {
                let timer_name = snapshotter.timer_name;
                self.book_snapshotters.remove(instrument_id);
                if self.clock.timer_names().contains(&timer_name.as_str()) {
                    self.clock.cancel_timer(&timer_name);
                }
                log::debug!("Removed BookSnapshotter for instrument ID {instrument_id}");
            }
        }
    }

    // -- RESPONSE HANDLERS -----------------------------------------------------------------------

    fn handle_instruments(&self, instruments: Arc<Vec<InstrumentAny>>) {
        // TODO improve by adding bulk update methods to cache and database
        let mut cache = self.cache.as_ref().borrow_mut();
        for instrument in instruments.iter() {
            if let Err(e) = cache.add_instrument(instrument.clone()) {
                log::error!("Error on cache insert: {e}");
            }
        }
    }

    fn handle_quotes(&self, quotes: Arc<Vec<QuoteTick>>) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_quotes(&quotes) {
            log::error!("Error on cache insert: {e}");
        }
    }

    fn handle_trades(&self, trades: Arc<Vec<TradeTick>>) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_trades(&trades) {
            log::error!("Error on cache insert: {e}");
        }
    }

    fn handle_bars(&self, bars: Arc<Vec<Bar>>) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_bars(&bars) {
            log::error!("Error on cache insert: {e}");
        }
    }

    // -- INTERNAL --------------------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn setup_order_book(
        &mut self,
        instrument_id: &InstrumentId,
        book_type: BookType,
        depth: Option<usize>,
        only_deltas: bool,
        managed: bool,
    ) -> anyhow::Result<()> {
        let mut cache = self.cache.borrow_mut();
        if managed && !cache.has_order_book(instrument_id) {
            let book = OrderBook::new(*instrument_id, book_type);
            log::debug!("Created {book}");
            cache.add_order_book(book)?;
        }

        let mut msgbus = self.msgbus.borrow_mut();

        // Set up subscriptions
        let updater = Rc::new(BookUpdater::new(instrument_id, self.cache.clone()));
        self.book_updaters.insert(*instrument_id, updater.clone());

        let handler = ShareableMessageHandler(updater);

        let topic = msgbus.switchboard.get_deltas_topic(*instrument_id);
        if !msgbus.is_subscribed(topic, handler.clone()) {
            msgbus.subscribe(topic, handler.clone(), Some(self.msgbus_priority));
        }

        let topic = msgbus.switchboard.get_depth_topic(*instrument_id);
        if !only_deltas && !msgbus.is_subscribed(topic, handler.clone()) {
            msgbus.subscribe(topic, handler, Some(self.msgbus_priority));
        }

        Ok(())
    }
}

pub struct SubscriptionCommandHandler {
    pub id: Ustr,
    pub engine_ref: Rc<RefCell<DataEngine>>,
}

impl MessageHandler for SubscriptionCommandHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, msg: &dyn Any) {
        self.engine_ref.borrow_mut().enqueue(msg);
    }
    fn handle_response(&self, _resp: DataResponse) {}
    fn handle_data(&self, _data: Data) {}
    fn as_any(&self) -> &dyn Any {
        self
    }
}
