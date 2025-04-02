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

#[cfg(test)]
mod tests;

use std::{
    any::Any,
    cell::{Ref, RefCell},
    collections::{HashMap, HashSet, VecDeque},
    num::NonZeroUsize,
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
    messages::data::{
        DataCommand, DataRequest, DataResponse, SubscribeBars, SubscribeBookDeltas,
        SubscribeBookDepth10, SubscribeBookSnapshots, SubscribeCommand, UnsubscribeBars,
        UnsubscribeBookDeltas, UnsubscribeBookDepth10, UnsubscribeBookSnapshots,
        UnsubscribeCommand,
    },
    msgbus::{
        self, get_message_bus,
        handler::{MessageHandler, ShareableMessageHandler},
        switchboard::{self},
    },
    timer::TimeEventCallback,
};
use nautilus_core::{
    correctness::{FAILED, check_key_in_index_map, check_key_not_in_index_map},
    datetime::{NANOSECONDS_IN_MILLISECOND, NANOSECONDS_IN_SECOND, millis_to_nanos},
};
use nautilus_model::{
    data::{
        Bar, BarType, Data, DataType, OrderBookDelta, OrderBookDeltas, OrderBookDepth10, QuoteTick,
        TradeTick,
        prices::{IndexPriceUpdate, MarkPriceUpdate},
    },
    enums::{AggregationSource, BarAggregation, BookType, PriceType, RecordFlag},
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny, SyntheticInstrument},
    orderbook::OrderBook,
};
use ustr::Ustr;

use crate::{
    aggregation::{
        BarAggregator, TickBarAggregator, TimeBarAggregator, ValueBarAggregator,
        VolumeBarAggregator,
    },
    client::DataClientAdapter,
};

/// Provides a high-performance `DataEngine` for all environments.
pub struct DataEngine {
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    clients: IndexMap<ClientId, DataClientAdapter>,
    default_client: Option<DataClientAdapter>,
    external_clients: HashSet<ClientId>,
    routing_map: IndexMap<Venue, ClientId>,
    book_intervals: HashMap<NonZeroUsize, HashSet<InstrumentId>>,
    book_updaters: HashMap<InstrumentId, Rc<BookUpdater>>,
    book_snapshotters: HashMap<InstrumentId, Rc<BookSnapshotter>>,
    bar_aggregators: HashMap<BarType, Box<dyn BarAggregator>>,
    synthetic_quote_feeds: HashMap<InstrumentId, Vec<SyntheticInstrument>>,
    synthetic_trade_feeds: HashMap<InstrumentId, Vec<SyntheticInstrument>>,
    buffered_deltas_map: HashMap<InstrumentId, Vec<OrderBookDelta>>, // TODO: Use OrderBookDeltas?
    msgbus_priority: u8,
    command_queue: VecDeque<DataCommand>,
    config: DataEngineConfig,
}

impl DataEngine {
    /// Creates a new [`DataEngine`] instance.
    #[must_use]
    pub fn new(
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
        config: Option<DataEngineConfig>,
    ) -> Self {
        Self {
            clock,
            cache,
            clients: IndexMap::new(),
            default_client: None,
            external_clients: HashSet::new(),
            routing_map: IndexMap::new(),
            book_intervals: HashMap::new(),
            book_updaters: HashMap::new(),
            book_snapshotters: HashMap::new(),
            bar_aggregators: HashMap::new(),
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

    pub fn dispose(self) {
        self.clients.values().for_each(|client| client.dispose());
        self.clock.borrow_mut().cancel_timers();
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
    pub fn registered_clients(&self) -> Vec<ClientId> {
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
        client_id: Option<&ClientId>,
        venue: Option<&Venue>,
    ) -> Option<&mut DataClientAdapter> {
        if let Some(client_id) = client_id {
            // Try to get client directly from clients map
            if self.clients.contains_key(client_id) {
                return self.clients.get_mut(client_id);
            }
        }

        if let Some(venue) = venue {
            // If not found, try to get client_id from routing map
            if let Some(mapped_client_id) = self.routing_map.get(venue) {
                return self.clients.get_mut(mapped_client_id);
            }
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
    pub fn subscribed_book_deltas(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_book_deltas)
    }

    #[must_use]
    pub fn subscribed_book_snapshots(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_book_snapshots)
    }

    #[must_use]
    pub fn subscribed_quotes(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_quotes)
    }

    #[must_use]
    pub fn subscribed_trades(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_trades)
    }

    #[must_use]
    pub fn subscribed_bars(&self) -> Vec<BarType> {
        self.collect_subscriptions(|client| &client.subscriptions_bars)
    }

    #[must_use]
    pub fn subscribed_mark_prices(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_mark_prices)
    }

    #[must_use]
    pub fn subscribed_index_prices(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_index_prices)
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
        if let Some(cmd) = cmd.downcast_ref::<DataCommand>() {
            self.command_queue.push_back(cmd.clone());
        } else {
            log::error!("Invalid message type received: {cmd:?}");
        }
    }

    pub fn execute(&mut self, cmd: DataCommand) {
        let result = match cmd {
            DataCommand::Subscribe(cmd) => self.execute_subscribe(cmd),
            DataCommand::Unsubscribe(cmd) => self.execute_unsubscribe(cmd),
            DataCommand::Request(cmd) => {
                self.execute_request(cmd);
                Ok(())
            }
        };

        if let Err(e) = result {
            log::error!("{e}");
        }
    }

    pub fn execute_subscribe(&mut self, cmd: SubscribeCommand) -> anyhow::Result<()> {
        match &cmd {
            SubscribeCommand::BookDeltas(cmd) => self.subscribe_book_deltas(cmd)?,
            SubscribeCommand::BookDepth10(cmd) => self.subscribe_book_depth10(cmd)?,
            SubscribeCommand::BookSnapshots(cmd) => self.subscribe_book_snapshots(cmd)?,
            SubscribeCommand::Bars(cmd) => self.subscribe_bars(cmd)?,
            _ => {} // Do nothing else
        }

        if let Some(client) = self.get_client_mut(cmd.client_id(), cmd.venue()) {
            client.execute_subscribe_command(cmd.clone());
        } else {
            log::error!(
                "Cannot handle command: no client found for client_id={:?}, venue={:?}",
                cmd.client_id(),
                cmd.venue(),
            );
        }

        Ok(())
    }

    pub fn execute_unsubscribe(&mut self, cmd: UnsubscribeCommand) -> anyhow::Result<()> {
        match &cmd {
            UnsubscribeCommand::BookDeltas(cmd) => self.unsubscribe_book_deltas(cmd)?,
            UnsubscribeCommand::BookDepth10(cmd) => self.unsubscribe_book_depth10(cmd)?,
            UnsubscribeCommand::BookSnapshots(cmd) => self.unsubscribe_book_snapshots(cmd)?,
            UnsubscribeCommand::Bars(cmd) => self.unsubscribe_bars(cmd)?,
            _ => {} // Do nothing else
        }

        if let Some(client) = self.get_client_mut(cmd.client_id(), cmd.venue()) {
            client.execute_unsubscribe_command(cmd.clone());
        } else {
            log::error!(
                "Cannot handle command: no client found for client_id={:?}, venue={:?}",
                cmd.client_id(),
                cmd.venue(),
            );
        }

        Ok(())
    }

    /// Sends a [`DataRequest`] to an endpoint that must be a data client implementation.
    pub fn execute_request(&self, req: DataRequest) {
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
        // TODO: Eventually these could be added to the `Data` enum? process here for now
        if let Some(instrument) = data.downcast_ref::<InstrumentAny>() {
            self.handle_instrument(instrument.clone());
        } else if let Some(mark_price) = data.downcast_ref::<MarkPriceUpdate>() {
            self.handle_mark_price(*mark_price);
        } else if let Some(index_price) = data.downcast_ref::<IndexPriceUpdate>() {
            self.handle_index_price(*index_price);
        } else {
            log::error!("Cannot process data {data:?}, type is unrecognized");
        }
    }

    pub fn process_data(&mut self, data: Data) {
        match data {
            Data::Delta(delta) => self.handle_delta(delta),
            Data::Deltas(deltas) => self.handle_deltas(deltas.into_inner()),
            Data::Depth10(depth) => self.handle_depth10(*depth),
            Data::Quote(quote) => self.handle_quote(quote),
            Data::Trade(trade) => self.handle_trade(trade),
            Data::Bar(bar) => self.handle_bar(bar),
        }
    }

    // TODO: Upgrade to response message handling
    pub fn response(&self, resp: DataResponse) {
        log::debug!("{RECV}{RES} {resp:?}");

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

        get_message_bus().borrow().send_response(resp);
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

        let topic = switchboard::get_instrument_topic(instrument.id());
        msgbus::publish(&topic, &instrument as &dyn Any); // TODO: Optimize
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

        let topic = switchboard::get_book_deltas_topic(deltas.instrument_id);
        msgbus::publish(&topic, &deltas as &dyn Any);
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

        let topic = switchboard::get_book_deltas_topic(deltas.instrument_id);
        msgbus::publish(&topic, &deltas as &dyn Any); // TODO: Optimize
    }

    fn handle_depth10(&mut self, depth: OrderBookDepth10) {
        let topic = switchboard::get_book_depth10_topic(depth.instrument_id);
        msgbus::publish(&topic, &depth as &dyn Any); // TODO: Optimize
    }

    fn handle_quote(&mut self, quote: QuoteTick) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_quote(quote) {
            log::error!("Error on cache insert: {e}");
        }

        // TODO: Handle synthetics

        let topic = switchboard::get_quotes_topic(quote.instrument_id);
        msgbus::publish(&topic, &quote as &dyn Any); // TODO: Optimize
    }

    fn handle_trade(&mut self, trade: TradeTick) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_trade(trade) {
            log::error!("Error on cache insert: {e}");
        }

        // TODO: Handle synthetics

        let topic = switchboard::get_trades_topic(trade.instrument_id);
        msgbus::publish(&topic, &trade as &dyn Any); // TODO: Optimize
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

        let topic = switchboard::get_bars_topic(bar.bar_type);
        msgbus::publish(&topic, &bar as &dyn Any); // TODO: Optimize
    }

    fn handle_mark_price(&mut self, mark_price: MarkPriceUpdate) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_mark_price(mark_price) {
            log::error!("Error on cache insert: {e}");
        }

        let topic = switchboard::get_mark_price_topic(mark_price.instrument_id);
        msgbus::publish(&topic, &mark_price as &dyn Any); // TODO: Optimize
    }

    fn handle_index_price(&mut self, index_price: IndexPriceUpdate) {
        if let Err(e) = self
            .cache
            .as_ref()
            .borrow_mut()
            .add_index_price(index_price)
        {
            log::error!("Error on cache insert: {e}");
        }

        let topic = switchboard::get_index_price_topic(index_price.instrument_id);
        msgbus::publish(&topic, &index_price as &dyn Any); // TODO: Optimize
    }

    // -- SUBSCRIPTION HANDLERS -------------------------------------------------------------------

    fn subscribe_book_deltas(&mut self, cmd: &SubscribeBookDeltas) -> anyhow::Result<()> {
        if cmd.instrument_id.is_synthetic() {
            anyhow::bail!("Cannot subscribe for synthetic instrument `OrderBookDelta` data");
        }

        self.setup_order_book(
            &cmd.instrument_id,
            cmd.book_type,
            cmd.depth,
            true,
            cmd.managed,
        )?;

        Ok(())
    }

    fn subscribe_book_depth10(&mut self, cmd: &SubscribeBookDepth10) -> anyhow::Result<()> {
        if cmd.instrument_id.is_synthetic() {
            anyhow::bail!("Cannot subscribe for synthetic instrument `OrderBookDepth10` data");
        }

        self.setup_order_book(
            &cmd.instrument_id,
            cmd.book_type,
            cmd.depth, // TODO
            false,
            cmd.managed,
        )?;

        Ok(())
    }

    fn subscribe_book_snapshots(&mut self, cmd: &SubscribeBookSnapshots) -> anyhow::Result<()> {
        if self.subscribed_book_deltas().contains(&cmd.instrument_id) {
            return Ok(());
        }

        if cmd.instrument_id.is_synthetic() {
            anyhow::bail!("Cannot subscribe for synthetic instrument `OrderBookDelta` data");
        }

        {
            if !self.book_intervals.contains_key(&cmd.interval_ms) {
                let interval_ns = millis_to_nanos(cmd.interval_ms.get() as f64);
                let topic = switchboard::get_book_snapshots_topic(cmd.instrument_id);

                let snap_info = BookSnapshotInfo {
                    instrument_id: cmd.instrument_id,
                    venue: cmd.instrument_id.venue,
                    is_composite: cmd.instrument_id.symbol.is_composite(),
                    root: Ustr::from(cmd.instrument_id.symbol.root()),
                    topic,
                    interval_ms: cmd.interval_ms,
                };

                let now_ns = self.clock.borrow().timestamp_ns().as_u64();
                let mut start_time_ns = now_ns - (now_ns % interval_ns);

                if start_time_ns - NANOSECONDS_IN_MILLISECOND <= now_ns {
                    start_time_ns += NANOSECONDS_IN_SECOND; // Add one second
                }

                let snapshotter = Rc::new(BookSnapshotter::new(snap_info, self.cache.clone()));
                self.book_snapshotters
                    .insert(cmd.instrument_id, snapshotter.clone());
                let timer_name = snapshotter.timer_name;

                let callback =
                    TimeEventCallback::Rust(Rc::new(move |event| snapshotter.snapshot(event)));

                self.clock
                    .borrow_mut()
                    .set_timer_ns(
                        &timer_name,
                        interval_ns,
                        start_time_ns.into(),
                        None,
                        Some(callback),
                    )
                    .expect(FAILED);
            }
        }

        self.setup_order_book(&cmd.instrument_id, cmd.book_type, cmd.depth, false, true)?;

        Ok(())
    }

    fn subscribe_bars(&mut self, cmd: &SubscribeBars) -> anyhow::Result<()> {
        match cmd.bar_type.aggregation_source() {
            AggregationSource::Internal => {
                if !self.bar_aggregators.contains_key(&cmd.bar_type.standard()) {
                    self.start_bar_aggregator(cmd.bar_type)?;
                }
            }
            AggregationSource::External => {
                if cmd.bar_type.instrument_id().is_synthetic() {
                    anyhow::bail!(
                        "Cannot subscribe for externally aggregated synthetic instrument bar data"
                    );
                }
            }
        }

        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        if !self.subscribed_book_deltas().contains(&cmd.instrument_id) {
            log::warn!("Cannot unsubscribe from `OrderBookDeltas` data: not subscribed");
            return Ok(());
        }

        let topics = vec![
            switchboard::get_book_deltas_topic(cmd.instrument_id),
            switchboard::get_book_depth10_topic(cmd.instrument_id),
            switchboard::get_book_snapshots_topic(cmd.instrument_id),
        ];

        self.maintain_book_updater(&cmd.instrument_id, &topics);
        self.maintain_book_snapshotter(&cmd.instrument_id);

        Ok(())
    }

    fn unsubscribe_book_depth10(&mut self, cmd: &UnsubscribeBookDepth10) -> anyhow::Result<()> {
        if !self.subscribed_book_deltas().contains(&cmd.instrument_id) {
            log::warn!("Cannot unsubscribe from `OrderBookDeltas` data: not subscribed");
            return Ok(());
        }

        let topics = vec![
            switchboard::get_book_deltas_topic(cmd.instrument_id),
            switchboard::get_book_depth10_topic(cmd.instrument_id),
            switchboard::get_book_snapshots_topic(cmd.instrument_id),
        ];

        self.maintain_book_updater(&cmd.instrument_id, &topics);
        self.maintain_book_snapshotter(&cmd.instrument_id);

        Ok(())
    }

    fn unsubscribe_book_snapshots(&mut self, cmd: &UnsubscribeBookSnapshots) -> anyhow::Result<()> {
        if !self.subscribed_book_deltas().contains(&cmd.instrument_id) {
            log::warn!("Cannot unsubscribe from `OrderBook` snapshots: not subscribed");
            return Ok(());
        }

        let topics = vec![
            switchboard::get_book_deltas_topic(cmd.instrument_id),
            switchboard::get_book_depth10_topic(cmd.instrument_id),
            switchboard::get_book_snapshots_topic(cmd.instrument_id),
        ];

        self.maintain_book_updater(&cmd.instrument_id, &topics);
        self.maintain_book_snapshotter(&cmd.instrument_id);

        Ok(())
    }

    const fn unsubscribe_bars(&mut self, command: &UnsubscribeBars) -> anyhow::Result<()> {
        // TODO: Handle aggregators
        Ok(())
    }

    fn maintain_book_updater(&mut self, instrument_id: &InstrumentId, topics: &[Ustr]) {
        if let Some(updater) = self.book_updaters.get(instrument_id) {
            let handler = ShareableMessageHandler(updater.clone());

            // Unsubscribe handler if it is the last subscriber
            for topic in topics {
                if msgbus::subscriptions_count(*topic) == 1
                    && msgbus::is_subscribed(*topic, handler.clone())
                {
                    log::debug!("Unsubscribing BookUpdater from {topic}");
                    msgbus::unsubscribe(*topic, handler.clone());
                }
            }

            // Check remaining subscriptions, if none then remove updater
            let still_subscribed = topics
                .iter()
                .any(|topic| msgbus::is_subscribed(*topic, handler.clone()));
            if !still_subscribed {
                self.book_updaters.remove(instrument_id);
                log::debug!("Removed BookUpdater for instrument ID {instrument_id}");
            }
        }
    }

    fn maintain_book_snapshotter(&mut self, instrument_id: &InstrumentId) {
        if let Some(snapshotter) = self.book_snapshotters.get(instrument_id) {
            let topic = switchboard::get_book_snapshots_topic(*instrument_id);

            // Check remaining snapshot subscriptions, if none then remove snapshotter
            if msgbus::subscriptions_count(topic) == 0 {
                let timer_name = snapshotter.timer_name;
                self.book_snapshotters.remove(instrument_id);
                let mut clock = self.clock.borrow_mut();
                if clock.timer_names().contains(&timer_name.as_str()) {
                    clock.cancel_timer(&timer_name);
                }
                log::debug!("Removed BookSnapshotter for instrument ID {instrument_id}");
            }
        }
    }

    // -- RESPONSE HANDLERS -----------------------------------------------------------------------

    fn handle_instruments(&self, instruments: Arc<Vec<InstrumentAny>>) {
        // TODO: Improve by adding bulk update methods to cache and database
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
        depth: Option<NonZeroUsize>,
        only_deltas: bool,
        managed: bool,
    ) -> anyhow::Result<()> {
        let mut cache = self.cache.borrow_mut();
        if managed && !cache.has_order_book(instrument_id) {
            let book = OrderBook::new(*instrument_id, book_type);
            log::debug!("Created {book}");
            cache.add_order_book(book)?;
        }

        // Set up subscriptions
        let updater = Rc::new(BookUpdater::new(instrument_id, self.cache.clone()));
        self.book_updaters.insert(*instrument_id, updater.clone());

        let handler = ShareableMessageHandler(updater);

        let topic = switchboard::get_book_deltas_topic(*instrument_id);
        if !msgbus::is_subscribed(topic, handler.clone()) {
            msgbus::subscribe(topic, handler.clone(), Some(self.msgbus_priority));
        }

        let topic = switchboard::get_book_depth10_topic(*instrument_id);
        if !only_deltas && !msgbus::is_subscribed(topic, handler.clone()) {
            msgbus::subscribe(topic, handler, Some(self.msgbus_priority));
        }

        Ok(())
    }

    fn create_bar_aggregator(
        &mut self,
        instrument: &InstrumentAny,
        bar_type: BarType,
    ) -> Box<dyn BarAggregator> {
        let cache = self.cache.clone();

        let handler = move |bar: Bar| {
            if let Err(e) = cache.as_ref().borrow_mut().add_bar(bar) {
                log::error!("Error on cache insert: {e}");
            }

            let topic = switchboard::get_bars_topic(bar.bar_type);
            msgbus::publish(&topic, &bar as &dyn Any);
        };

        let clock = self.clock.clone();
        let config = self.config.clone();

        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();

        if bar_type.spec().is_time_aggregated() {
            Box::new(TimeBarAggregator::new(
                bar_type,
                price_precision,
                size_precision,
                clock,
                handler,
                false, // await_partial
                config.time_bars_build_with_no_updates,
                config.time_bars_timestamp_on_close,
                config.time_bars_interval_type,
                None,  // TODO: Implement
                20,    // TODO: TBD, composite bar build delay
                false, // TODO: skip_first_non_full_bar, make it config dependent
            ))
        } else {
            match bar_type.spec().aggregation {
                BarAggregation::Tick => Box::new(TickBarAggregator::new(
                    bar_type,
                    price_precision,
                    size_precision,
                    handler,
                    false,
                )) as Box<dyn BarAggregator>,
                BarAggregation::Volume => Box::new(VolumeBarAggregator::new(
                    bar_type,
                    price_precision,
                    size_precision,
                    handler,
                    false,
                )) as Box<dyn BarAggregator>,
                BarAggregation::Value => Box::new(ValueBarAggregator::new(
                    bar_type,
                    price_precision,
                    size_precision,
                    handler,
                    false,
                )) as Box<dyn BarAggregator>,
                _ => panic!(
                    "Cannot create aggregator: {} aggregation not currently supported",
                    bar_type.spec().aggregation
                ),
            }
        }
    }

    fn start_bar_aggregator(&mut self, bar_type: BarType) -> anyhow::Result<()> {
        let instrument = {
            let cache = self.cache.borrow();
            cache
                .instrument(&bar_type.instrument_id())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Cannot start bar aggregation: no instrument found for {}",
                        bar_type.instrument_id(),
                    )
                })?
                .clone()
        };

        let aggregator = if let Some(aggregator) = self.bar_aggregators.get_mut(&bar_type) {
            aggregator
        } else {
            let aggregator = self.create_bar_aggregator(&instrument, bar_type);
            self.bar_aggregators.insert(bar_type, aggregator);
            self.bar_aggregators.get_mut(&bar_type).unwrap()
        };

        // TODO: Subscribe to data

        aggregator.set_is_running(true);

        Ok(())
    }

    fn stop_bar_aggregator(&mut self, bar_type: BarType) -> anyhow::Result<()> {
        let aggregator = self
            .bar_aggregators
            .remove(&bar_type.standard())
            .ok_or_else(|| {
                anyhow::anyhow!("Cannot stop bar aggregator: no aggregator to stop for {bar_type}")
            })?;

        // TODO: If its a `TimeBarAggregator` then call `.stop()`
        // if let Some(aggregator) = (aggregator as &dyn BarAggregator)
        //     .as_any()
        //     .downcast_ref::<TimeBarAggregator<_, _>>()
        // {
        //     aggregator.stop();
        // };

        if bar_type.is_composite() {
            let composite_bar_type = bar_type.composite();
            // TODO: Unsubscribe the `aggregator.handle_bar`
        } else if bar_type.spec().price_type == PriceType::Last {
            // TODO: Unsubscribe `aggregator.handle_trade_tick`
            todo!()
        } else {
            // TODO: Unsubscribe `aggregator.handle_quote_tick`
            todo!()
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

    fn as_any(&self) -> &dyn Any {
        self
    }
}
