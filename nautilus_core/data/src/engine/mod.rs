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

//! Provides a generic `DataEngine` for all environments.
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

pub mod runner;

use std::{
    any::Any,
    cell::RefCell,
    collections::{HashMap, HashSet},
    num::NonZeroU64,
    ops::Deref,
    rc::Rc,
    sync::Arc,
};

use indexmap::IndexMap;
use nautilus_common::{
    cache::Cache,
    clock::Clock,
    logging::{RECV, RES},
    messages::data::{DataRequest, DataResponse, SubscriptionCommand},
    msgbus::{handler::MessageHandler, MessageBus},
    timer::TimeEvent,
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

const UNINITIALIZED: &str = "`DataEngine` was not initialized before use";

pub struct DataEngineConfig {
    pub time_bars_build_with_no_updates: bool,
    pub time_bars_timestamp_on_close: bool,
    pub time_bars_interval_type: String, // Make this an enum `BarIntervalType`
    pub validate_data_sequence: bool,
    pub buffer_deltas: bool,
    pub external_clients: Option<Vec<ClientId>>,
    pub debug: bool,
}

impl Default for DataEngineConfig {
    fn default() -> Self {
        Self {
            time_bars_build_with_no_updates: true,
            time_bars_timestamp_on_close: true,
            time_bars_interval_type: "left_open".to_string(), // Make this an enum `BarIntervalType`
            validate_data_sequence: false,
            buffer_deltas: false,
            external_clients: None,
            debug: false,
        }
    }
}

pub struct DataEngine {
    clock: Box<dyn Clock>,
    cache: Rc<RefCell<Cache>>,
    msgbus: Rc<RefCell<MessageBus>>,
    clients: IndexMap<ClientId, DataClientAdapter>,
    default_client: Option<DataClientAdapter>,
    external_clients: HashSet<ClientId>,
    routing_map: IndexMap<Venue, ClientId>,
    order_book_intervals: HashMap<NonZeroU64, HashSet<InstrumentId>>,
    bar_aggregators: Vec<Box<dyn BarAggregator>>, // TODO: dyn for now
    synthetic_quote_feeds: HashMap<InstrumentId, Vec<SyntheticInstrument>>,
    synthetic_trade_feeds: HashMap<InstrumentId, Vec<SyntheticInstrument>>,
    buffered_deltas_map: HashMap<InstrumentId, Vec<OrderBookDelta>>,
    handler_ref: Option<Rc<RefCell<Self>>>,
    msgbus_priority: u8,
    config: DataEngineConfig,
}

impl DataEngine {
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
            order_book_intervals: HashMap::new(),
            bar_aggregators: Vec::new(),
            synthetic_quote_feeds: HashMap::new(),
            synthetic_trade_feeds: HashMap::new(),
            buffered_deltas_map: HashMap::new(),
            handler_ref: None,   // Assigned at system initialization
            msgbus_priority: 10, // High-priority for built-in component
            config: config.unwrap_or_default(),
        }
    }

    // pub fn register_catalog(&mut self, catalog: ParquetDataCatalog) {}  TODO: Implement catalog

    /// Register the given data `client` with the engine as the default routing client.
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

    pub fn execute(&mut self, msg: &dyn Any) {
        if let Some(cmd) = msg.downcast_ref::<SubscriptionCommand>() {
            match cmd.data_type.type_name() {
                stringify!(OrderBookDelta) => self.handle_subscribe_book_deltas(cmd),
                stringify!(OrderBook) => self.handle_subscribe_book_snapshots(cmd),
                // stringify!(QuoteTick) => self.handle_subscribe_quote_ticks(cmd),
                // stringify!(TradeTick) => self.handle_subscribe_trade_ticks(cmd),
                // stringify!(Bar) => self.handle_subscribe_bars(cmd),
                type_name => Err(anyhow::anyhow!(
                    "Cannot handle subscription, type `{type_name}` is unrecognized"
                )),
            }
            .unwrap_or_else(|e| log::error!("{e}"));

            if let Some(client) = self.get_client_mut(&cmd.client_id, &cmd.venue) {
                client.execute(cmd.clone());

                // TBD if we want to do the below instead
                // if client.handles_order_book_deltas {
                //     client.subscribe_order_book_deltas(instrument_id, book_type, depth)?;
                // } else if client.handles_order_book_snapshots {
                //     client.subscribe_order_book_snapshots(instrument_id, book_type, depth)?;
                // } else {
                //     anyhow::bail!("Cannot subscribe order book for {instrument_id}: client does not handle book subscriptions");
                // }
                // client.execute(command);
            } else {
                log::error!(
                    "Cannot handle command: no client found for {}",
                    cmd.client_id
                );
            };
        } else {
            log::error!("Invalid message type received: {msg:?}");
        }
    }

    /// Send a [`DataRequest`] to an endpoint that must be a data client implementation.
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
            Data::Deltas(deltas) => self.handle_deltas(deltas.deref().clone()), // TODO: Optimize
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

            // TODO: Improve efficiency, the FFI API will go along with Cython
            OrderBookDeltas::new(delta.instrument_id, buffer_deltas.clone())
        } else {
            // TODO: Improve efficiency, the FFI API will go along with Cython
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

            // TODO: Improve efficiency, the FFI API will go along with Cython
            OrderBookDeltas::new(deltas.instrument_id, buffer_deltas.clone())
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
                    return; // `bar` is out of sequence
                }
                if bar.ts_init < last_bar.ts_init {
                    log::warn!(
                        "Bar {bar} was prior to last bar `ts_init` {}",
                        last_bar.ts_init
                    );
                    return; // `bar` is out of sequence
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
        let data_type = command.data_type.clone();
        let instrument_id = data_type.instrument_id();
        let book_type = data_type.book_type();
        let depth = data_type.depth();
        let managed = data_type.managed();

        if let Some(instrument_id) = instrument_id {
            if instrument_id.is_synthetic() {
                anyhow::bail!("Cannot subscribe for synthetic instrument `OrderBookDelta` data");
            }

            if self.subscribed_order_book_deltas().contains(&instrument_id) {
                return Ok(());
            }

            self.setup_order_book(&instrument_id, book_type, depth, true, managed)?;

            Ok(())
        } else {
            anyhow::bail!("Invalid order book deltas subscription: did not contain an `instrument_id`, {data_type}");
        }
    }

    fn handle_subscribe_book_snapshots(
        &mut self,
        command: &SubscriptionCommand,
    ) -> anyhow::Result<()> {
        let data_type = command.data_type.clone();
        let instrument_id = data_type.instrument_id();
        let book_type = data_type.book_type();
        let depth = data_type.depth();
        let interval_ms = data_type.interval_ms();
        let managed = data_type.managed();

        if let Some(instrument_id) = instrument_id {
            if instrument_id.is_synthetic() {
                anyhow::bail!("Cannot subscribe for synthetic instrument `OrderBookDelta` data");
            }

            if self.subscribed_order_book_deltas().contains(&instrument_id) {
                return Ok(());
            }

            // TODO: Set up timer at interval
            if !self.order_book_intervals.contains_key(&interval_ms) {
                let timer_name = format!("OrderBook|{interval_ms}");
                let interval_ns = millis_to_nanos(interval_ms.get() as f64);
                let now_ns = self.clock.timestamp_ns().as_u64();
                let mut start_time_ns = now_ns - (now_ns % interval_ns);

                if start_time_ns - NANOSECONDS_IN_MILLISECOND <= now_ns {
                    start_time_ns += NANOSECONDS_IN_SECOND; // Add one second
                }

                // let callback = SafeTimeEventCallback {
                //     callback: Box::new(move |event| self.snapshot_order_book(event)),
                // };
                //
                // self.clock.set_timer_ns(
                //     timer_name.as_str(),
                //     interval_ns,
                //     start_time_ns,
                //     None,
                //     handler,
                // )
            }

            self.setup_order_book(&instrument_id, book_type, depth, true, managed)?;

            Ok(())
        } else {
            anyhow::bail!("Invalid order book deltas subscription: did not contain an `instrument_id`, {data_type}");
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
        {
            let mut cache = self.cache.borrow_mut();
            if managed && !cache.has_order_book(instrument_id) {
                let book = OrderBook::new(*instrument_id, book_type);
                log::debug!("Created {book}");
                cache.add_order_book(book)?;
            }
        }

        // TODO: TBD based on ShareableMessageHandler
        // let handler_ref = self.handler_ref.clone().expect(UNINITIALIZED);
        //
        // {
        //     let mut msgbus = self.msgbus.borrow_mut();
        //
        //     // Set up subscriptions
        //     let topic = msgbus.switchboard.get_deltas_topic(*instrument_id);
        //     let handler = ShareableMessageHandler(Rc::new(BookDataHandler {
        //         id: Ustr::from(stringify!(update_order_book)),
        //         engine_ref: handler_ref,
        //     }));
        //
        //     if !msgbus.is_subscribed(topic.as_str(), handler.clone()) {
        //         msgbus.subscribe(topic, handler.clone(), Some(self.msgbus_priority));
        //     }
        //
        //     let topic = msgbus.switchboard.get_depth_topic(*instrument_id);
        //
        //     if !only_deltas && !msgbus.is_subscribed(topic.as_str(), handler.clone()) {
        //         msgbus.subscribe(topic, handler, Some(self.msgbus_priority));
        //     }
        // }

        Ok(())
    }

    fn update_order_book(&self, data: &Data) {
        // Only apply data if there is a book being managed,
        // as it may be being managed manually.
        if let Some(book) = self
            .cache
            .as_ref()
            .borrow_mut()
            .order_book(&data.instrument_id())
        {
            match data {
                Data::Delta(delta) => book.apply_delta(delta),
                Data::Deltas(deltas) => book.apply_deltas(deltas),
                Data::Depth10(depth) => book.apply_depth(depth),
                _ => log::error!("Invalid data type for book update, was {data:?}"),
            }
        }
    }

    fn snapshot_order_book(&self, event: TimeEvent) {
        todo!()
    }
}

pub struct SubscriptionCommandHandler {
    id: Ustr,
    engine_ref: Rc<RefCell<DataEngine>>,
}

impl MessageHandler for SubscriptionCommandHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, message: &dyn Any) {
        self.engine_ref.borrow_mut().execute(message);
    }
    fn handle_response(&self, _resp: DataResponse) {}
    fn handle_data(&self, _data: Data) {}
    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub struct BookDataHandler {
    id: Ustr,
    engine_ref: Rc<RefCell<DataEngine>>,
}

impl MessageHandler for BookDataHandler {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, message: &dyn Any) {}
    fn handle_response(&self, _resp: DataResponse) {}
    fn handle_data(&self, data: Data) {
        self.engine_ref.borrow_mut().update_order_book(&data);
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::cell::OnceCell;

    use indexmap::indexmap;
    use log::LevelFilter;
    use nautilus_common::{
        clock::TestClock,
        logging::{init_logging, logger::LoggerConfig, writer::FileWriterConfig},
        messages::data::Action,
        msgbus::{
            handler::ShareableMessageHandler,
            stubs::{get_message_saving_handler, get_saved_messages},
            switchboard::MessagingSwitchboard,
        },
    };
    use nautilus_core::{nanos::UnixNanos, uuid::UUID4};
    use nautilus_model::{
        data::{
            deltas::OrderBookDeltas_API,
            stubs::{stub_delta, stub_deltas, stub_depth10},
        },
        enums::BookType,
        identifiers::TraderId,
        instruments::{currency_pair::CurrencyPair, stubs::audusd_sim},
    };
    use rstest::*;

    use super::*;
    use crate::mocks::MockDataClient;

    fn init_logger(stdout_level: LevelFilter) {
        let mut config = LoggerConfig::default();
        config.stdout_level = stdout_level;
        init_logging(
            TraderId::default(),
            UUID4::new(),
            config,
            FileWriterConfig::default(),
        );
    }

    #[fixture]
    fn trader_id() -> TraderId {
        TraderId::default()
    }

    #[fixture]
    fn client_id() -> ClientId {
        ClientId::default()
    }

    #[fixture]
    fn venue() -> Venue {
        Venue::default()
    }

    #[fixture]
    fn clock() -> Box<TestClock> {
        Box::new(TestClock::new())
    }

    #[fixture]
    fn cache() -> Rc<RefCell<Cache>> {
        // Ensure there is only ever one instance of the cache *per test*
        thread_local! {
            static CACHE: OnceCell<Rc<RefCell<Cache>>> = const { OnceCell::new() };
        }
        Rc::new(RefCell::new(Cache::default()))
    }

    #[fixture]
    fn msgbus(trader_id: TraderId) -> Rc<RefCell<MessageBus>> {
        // Ensure there is only ever one instance of the message bus *per test*
        thread_local! {
            static MSGBUS: OnceCell<Rc<RefCell<MessageBus>>> = const { OnceCell::new() };
        }

        MSGBUS.with(|cell| {
            cell.get_or_init(|| {
                Rc::new(RefCell::new(MessageBus::new(
                    trader_id,
                    UUID4::new(),
                    None,
                    None,
                )))
            })
            .clone()
        })
    }

    #[fixture]
    fn switchboard(msgbus: Rc<RefCell<MessageBus>>) -> MessagingSwitchboard {
        msgbus.borrow().switchboard.clone()
    }

    #[fixture]
    fn data_engine(
        clock: Box<TestClock>,
        cache: Rc<RefCell<Cache>>,
        msgbus: Rc<RefCell<MessageBus>>,
    ) -> Rc<RefCell<DataEngine>> {
        let data_engine = DataEngine::new(clock, cache, msgbus, None);
        Rc::new(RefCell::new(data_engine))
    }

    #[fixture]
    fn data_client(
        client_id: ClientId,
        venue: Venue,
        cache: Rc<RefCell<Cache>>,
        msgbus: Rc<RefCell<MessageBus>>,
        clock: Box<TestClock>,
    ) -> DataClientAdapter {
        let client = Box::new(MockDataClient::new(cache, msgbus, client_id, venue));
        DataClientAdapter::new(client_id, venue, true, true, client, clock)
    }

    #[rstest]
    fn test_execute_subscribe_custom_data(
        audusd_sim: CurrencyPair,
        msgbus: Rc<RefCell<MessageBus>>,
        switchboard: MessagingSwitchboard,
        data_engine: Rc<RefCell<DataEngine>>,
        data_client: DataClientAdapter,
    ) {
        let client_id = data_client.client_id;
        let venue = data_client.venue;
        data_engine.borrow_mut().register_client(data_client, None);

        let data_type = DataType::new(stringify!(String), None);
        let cmd = SubscriptionCommand::new(
            client_id,
            venue,
            data_type.clone(),
            Action::Subscribe,
            UUID4::new(),
            UnixNanos::default(),
        );

        let endpoint = switchboard.data_engine_execute;
        let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
            id: endpoint,
            engine_ref: data_engine.clone(),
        }));
        msgbus.borrow_mut().register(endpoint, handler);
        msgbus.borrow().send(&endpoint, &cmd as &dyn Any);

        assert!(data_engine
            .borrow()
            .subscribed_custom_data()
            .contains(&data_type));
    }

    #[rstest]
    fn test_execute_subscribe_order_book_deltas(
        audusd_sim: CurrencyPair,
        msgbus: Rc<RefCell<MessageBus>>,
        switchboard: MessagingSwitchboard,
        data_engine: Rc<RefCell<DataEngine>>,
        data_client: DataClientAdapter,
    ) {
        let client_id = data_client.client_id;
        let venue = data_client.venue;
        data_engine.borrow_mut().register_client(data_client, None);

        let metadata = indexmap! {
            "instrument_id".to_string() => audusd_sim.id.to_string(),
            "book_type".to_string() => BookType::L3_MBO.to_string(),
            "managed".to_string() => "true".to_string(),
        };
        let data_type = DataType::new(stringify!(OrderBookDelta), Some(metadata));
        let cmd = SubscriptionCommand::new(
            client_id,
            venue,
            data_type,
            Action::Subscribe,
            UUID4::new(),
            UnixNanos::default(),
        );

        let endpoint = switchboard.data_engine_execute;
        let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
            id: endpoint,
            engine_ref: data_engine.clone(),
        }));
        msgbus.borrow_mut().register(endpoint, handler);
        msgbus.borrow().send(&endpoint, &cmd as &dyn Any);

        assert!(data_engine
            .borrow()
            .subscribed_order_book_deltas()
            .contains(&audusd_sim.id));
    }

    #[rstest]
    fn test_execute_subscribe_order_book_snapshots(
        audusd_sim: CurrencyPair,
        msgbus: Rc<RefCell<MessageBus>>,
        switchboard: MessagingSwitchboard,
        data_engine: Rc<RefCell<DataEngine>>,
        data_client: DataClientAdapter,
    ) {
        let client_id = data_client.client_id;
        let venue = data_client.venue;
        data_engine.borrow_mut().register_client(data_client, None);

        let metadata = indexmap! {
            "instrument_id".to_string() => audusd_sim.id.to_string(),
            "book_type".to_string() => BookType::L2_MBP.to_string(),
            "managed".to_string() => "true".to_string(),
        };
        let data_type = DataType::new(stringify!(OrderBookDeltas), Some(metadata));
        let cmd = SubscriptionCommand::new(
            client_id,
            venue,
            data_type,
            Action::Subscribe,
            UUID4::new(),
            UnixNanos::default(),
        );

        let endpoint = switchboard.data_engine_execute;
        let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
            id: endpoint,
            engine_ref: data_engine.clone(),
        }));
        msgbus.borrow_mut().register(endpoint, handler);
        msgbus.borrow().send(&endpoint, &cmd as &dyn Any);

        assert!(data_engine
            .borrow()
            .subscribed_order_book_snapshots()
            .contains(&audusd_sim.id));
    }

    #[rstest]
    fn test_execute_subscribe_instrument(
        audusd_sim: CurrencyPair,
        msgbus: Rc<RefCell<MessageBus>>,
        switchboard: MessagingSwitchboard,
        data_engine: Rc<RefCell<DataEngine>>,
        data_client: DataClientAdapter,
    ) {
        let client_id = data_client.client_id;
        let venue = data_client.venue;
        data_engine.borrow_mut().register_client(data_client, None);

        let metadata = indexmap! {
            "instrument_id".to_string() => audusd_sim.id.to_string(),
        };
        let data_type = DataType::new(stringify!(InstrumentAny), Some(metadata));
        let cmd = SubscriptionCommand::new(
            client_id,
            venue,
            data_type,
            Action::Subscribe,
            UUID4::new(),
            UnixNanos::default(),
        );

        let endpoint = switchboard.data_engine_execute;
        let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
            id: endpoint,
            engine_ref: data_engine.clone(),
        }));
        msgbus.borrow_mut().register(endpoint, handler);
        msgbus.borrow().send(&endpoint, &cmd as &dyn Any);

        assert!(data_engine
            .borrow()
            .subscribed_instruments()
            .contains(&audusd_sim.id));
    }

    #[rstest]
    fn test_execute_subscribe_quote_ticks(
        audusd_sim: CurrencyPair,
        msgbus: Rc<RefCell<MessageBus>>,
        switchboard: MessagingSwitchboard,
        data_engine: Rc<RefCell<DataEngine>>,
        data_client: DataClientAdapter,
    ) {
        let client_id = data_client.client_id;
        let venue = data_client.venue;
        data_engine.borrow_mut().register_client(data_client, None);

        let metadata = indexmap! {
            "instrument_id".to_string() => audusd_sim.id.to_string(),
        };
        let data_type = DataType::new(stringify!(QuoteTick), Some(metadata));
        let cmd = SubscriptionCommand::new(
            client_id,
            venue,
            data_type,
            Action::Subscribe,
            UUID4::new(),
            UnixNanos::default(),
        );

        let endpoint = switchboard.data_engine_execute;
        let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
            id: endpoint,
            engine_ref: data_engine.clone(),
        }));
        msgbus.borrow_mut().register(endpoint, handler);
        msgbus.borrow().send(&endpoint, &cmd as &dyn Any);

        assert!(data_engine
            .borrow()
            .subscribed_quote_ticks()
            .contains(&audusd_sim.id));
    }

    #[rstest]
    fn test_execute_subscribe_trade_ticks(
        audusd_sim: CurrencyPair,
        msgbus: Rc<RefCell<MessageBus>>,
        switchboard: MessagingSwitchboard,
        data_engine: Rc<RefCell<DataEngine>>,
        data_client: DataClientAdapter,
    ) {
        let client_id = data_client.client_id;
        let venue = data_client.venue;
        data_engine.borrow_mut().register_client(data_client, None);

        let metadata = indexmap! {
            "instrument_id".to_string() => audusd_sim.id.to_string(),
        };
        let data_type = DataType::new(stringify!(TradeTick), Some(metadata));
        let cmd = SubscriptionCommand::new(
            client_id,
            venue,
            data_type,
            Action::Subscribe,
            UUID4::new(),
            UnixNanos::default(),
        );

        let endpoint = switchboard.data_engine_execute;
        let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
            id: endpoint,
            engine_ref: data_engine.clone(),
        }));
        msgbus.borrow_mut().register(endpoint, handler);
        msgbus.borrow().send(&endpoint, &cmd as &dyn Any);

        assert!(data_engine
            .borrow()
            .subscribed_trade_ticks()
            .contains(&audusd_sim.id));
    }

    #[rstest]
    fn test_execute_subscribe_bars(
        audusd_sim: CurrencyPair,
        msgbus: Rc<RefCell<MessageBus>>,
        switchboard: MessagingSwitchboard,
        data_engine: Rc<RefCell<DataEngine>>,
        data_client: DataClientAdapter,
    ) {
        let client_id = data_client.client_id;
        let venue = data_client.venue;
        data_engine.borrow_mut().register_client(data_client, None);

        let bar_type = BarType::from("AUDUSD.SIM-1-MINUTE-LAST-INTERNAL");
        let metadata = indexmap! {
            "bar_type".to_string() => bar_type.to_string(),
        };
        let data_type = DataType::new(stringify!(Bar), Some(metadata));
        let cmd = SubscriptionCommand::new(
            client_id,
            venue,
            data_type,
            Action::Subscribe,
            UUID4::new(),
            UnixNanos::default(),
        );

        let endpoint = switchboard.data_engine_execute;
        let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
            id: endpoint,
            engine_ref: data_engine.clone(),
        }));
        msgbus.borrow_mut().register(endpoint, handler);
        msgbus.borrow().send(&endpoint, &cmd as &dyn Any);

        assert!(data_engine.borrow().subscribed_bars().contains(&bar_type));
    }

    #[rstest]
    fn test_process_instrument(
        audusd_sim: CurrencyPair,
        msgbus: Rc<RefCell<MessageBus>>,
        switchboard: MessagingSwitchboard,
        data_engine: Rc<RefCell<DataEngine>>,
        data_client: DataClientAdapter,
    ) {
        let client_id = data_client.client_id;
        let venue = data_client.venue;
        data_engine.borrow_mut().register_client(data_client, None);

        let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
        let metadata = indexmap! {
            "instrument_id".to_string() => audusd_sim.id().to_string(),
        };
        let data_type = DataType::new(stringify!(InstrumentAny), Some(metadata));
        let cmd = SubscriptionCommand::new(
            client_id,
            venue,
            data_type,
            Action::Subscribe,
            UUID4::new(),
            UnixNanos::default(),
        );

        let endpoint = switchboard.data_engine_execute;
        let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
            id: endpoint,
            engine_ref: data_engine.clone(),
        }));
        msgbus.borrow_mut().register(endpoint, handler);
        msgbus.borrow().send(&endpoint, &cmd as &dyn Any);

        let handler = get_message_saving_handler::<InstrumentAny>(None);
        {
            let mut msgbus = msgbus.borrow_mut();
            let topic = msgbus.switchboard.get_instrument_topic(audusd_sim.id());
            msgbus.subscribe(topic, handler.clone(), None);
        }

        let mut data_engine = data_engine.borrow_mut();
        data_engine.process(&audusd_sim as &dyn Any);
        let cache = &data_engine.cache.borrow();
        let messages = get_saved_messages::<InstrumentAny>(handler);

        assert_eq!(
            cache.instrument(&audusd_sim.id()),
            Some(audusd_sim.clone()).as_ref()
        );
        assert_eq!(messages.len(), 1);
        assert!(messages.contains(&audusd_sim));
    }

    #[rstest]
    fn test_process_order_book_delta(
        audusd_sim: CurrencyPair,
        msgbus: Rc<RefCell<MessageBus>>,
        switchboard: MessagingSwitchboard,
        data_engine: Rc<RefCell<DataEngine>>,
        data_client: DataClientAdapter,
    ) {
        let client_id = data_client.client_id;
        let venue = data_client.venue;
        data_engine.borrow_mut().register_client(data_client, None);

        let metadata = indexmap! {
            "instrument_id".to_string() => audusd_sim.id.to_string(),
            "book_type".to_string() => BookType::L3_MBO.to_string(),
            "managed".to_string() => "true".to_string(),
        };
        let data_type = DataType::new(stringify!(OrderBookDelta), Some(metadata));
        let cmd = SubscriptionCommand::new(
            client_id,
            venue,
            data_type,
            Action::Subscribe,
            UUID4::new(),
            UnixNanos::default(),
        );

        let endpoint = switchboard.data_engine_execute;
        let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
            id: endpoint,
            engine_ref: data_engine.clone(),
        }));
        msgbus.borrow_mut().register(endpoint, handler);
        msgbus.borrow().send(&endpoint, &cmd as &dyn Any);

        let delta = stub_delta();
        let handler = get_message_saving_handler::<OrderBookDeltas>(None);
        {
            let mut msgbus = msgbus.borrow_mut();
            let topic = msgbus.switchboard.get_deltas_topic(delta.instrument_id);
            msgbus.subscribe(topic, handler.clone(), None);
        }

        let mut data_engine = data_engine.borrow_mut();
        data_engine.process_data(Data::Delta(delta));
        let cache = &data_engine.cache.borrow();
        let messages = get_saved_messages::<OrderBookDeltas>(handler);

        assert_eq!(messages.len(), 1);
    }

    #[rstest]
    fn test_process_order_book_deltas(
        audusd_sim: CurrencyPair,
        msgbus: Rc<RefCell<MessageBus>>,
        switchboard: MessagingSwitchboard,
        data_engine: Rc<RefCell<DataEngine>>,
        data_client: DataClientAdapter,
    ) {
        let client_id = data_client.client_id;
        let venue = data_client.venue;
        data_engine.borrow_mut().register_client(data_client, None);

        let metadata = indexmap! {
            "instrument_id".to_string() => audusd_sim.id.to_string(),
            "book_type".to_string() => BookType::L3_MBO.to_string(),
            "managed".to_string() => "true".to_string(),
        };
        let data_type = DataType::new(stringify!(OrderBookDeltas), Some(metadata));
        let cmd = SubscriptionCommand::new(
            client_id,
            venue,
            data_type,
            Action::Subscribe,
            UUID4::new(),
            UnixNanos::default(),
        );

        let endpoint = switchboard.data_engine_execute;
        let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
            id: endpoint,
            engine_ref: data_engine.clone(),
        }));
        msgbus.borrow_mut().register(endpoint, handler);
        msgbus.borrow().send(&endpoint, &cmd as &dyn Any);

        // TODO: Using FFI API wrapper temporarily until Cython gone
        let deltas = OrderBookDeltas_API::new(stub_deltas());
        let handler = get_message_saving_handler::<OrderBookDeltas>(None);
        {
            let mut msgbus = msgbus.borrow_mut();
            let topic = msgbus.switchboard.get_deltas_topic(deltas.instrument_id);
            msgbus.subscribe(topic, handler.clone(), None);
        }

        let mut data_engine = data_engine.borrow_mut();
        data_engine.process_data(Data::Deltas(deltas.clone()));
        let cache = &data_engine.cache.borrow();
        let messages = get_saved_messages::<OrderBookDeltas>(handler);

        assert_eq!(messages.len(), 1);
        assert!(messages.contains(&deltas));
    }

    #[rstest]
    fn test_process_order_book_depth10(
        audusd_sim: CurrencyPair,
        msgbus: Rc<RefCell<MessageBus>>,
        switchboard: MessagingSwitchboard,
        data_engine: Rc<RefCell<DataEngine>>,
        data_client: DataClientAdapter,
    ) {
        let client_id = data_client.client_id;
        let venue = data_client.venue;
        data_engine.borrow_mut().register_client(data_client, None);

        let metadata = indexmap! {
            "instrument_id".to_string() => audusd_sim.id.to_string(),
            "book_type".to_string() => BookType::L3_MBO.to_string(),
            "managed".to_string() => "true".to_string(),
        };
        let data_type = DataType::new(stringify!(OrderBookDepth10), Some(metadata));
        let cmd = SubscriptionCommand::new(
            client_id,
            venue,
            data_type,
            Action::Subscribe,
            UUID4::new(),
            UnixNanos::default(),
        );

        let endpoint = switchboard.data_engine_execute;
        let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
            id: endpoint,
            engine_ref: data_engine.clone(),
        }));
        msgbus.borrow_mut().register(endpoint, handler);
        msgbus.borrow().send(&endpoint, &cmd as &dyn Any);

        let depth = stub_depth10();
        let handler = get_message_saving_handler::<OrderBookDepth10>(None);
        {
            let mut msgbus = msgbus.borrow_mut();
            let topic = msgbus.switchboard.get_depth_topic(depth.instrument_id);
            msgbus.subscribe(topic, handler.clone(), None);
        }

        let mut data_engine = data_engine.borrow_mut();
        data_engine.process_data(Data::Depth10(depth));
        let cache = &data_engine.cache.borrow();
        let messages = get_saved_messages::<OrderBookDepth10>(handler);

        assert_eq!(messages.len(), 1);
        assert!(messages.contains(&depth));
    }

    #[rstest]
    fn test_process_quote_tick(
        audusd_sim: CurrencyPair,
        msgbus: Rc<RefCell<MessageBus>>,
        switchboard: MessagingSwitchboard,
        data_engine: Rc<RefCell<DataEngine>>,
        data_client: DataClientAdapter,
    ) {
        let client_id = data_client.client_id;
        let venue = data_client.venue;
        data_engine.borrow_mut().register_client(data_client, None);

        let metadata = indexmap! {
            "instrument_id".to_string() => audusd_sim.id.to_string(),
        };
        let data_type = DataType::new(stringify!(QuoteTick), Some(metadata));
        let cmd = SubscriptionCommand::new(
            client_id,
            venue,
            data_type,
            Action::Subscribe,
            UUID4::new(),
            UnixNanos::default(),
        );

        let endpoint = switchboard.data_engine_execute;
        let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
            id: endpoint,
            engine_ref: data_engine.clone(),
        }));
        msgbus.borrow_mut().register(endpoint, handler);
        msgbus.borrow().send(&endpoint, &cmd as &dyn Any);

        let quote = QuoteTick::default();
        let handler = get_message_saving_handler::<QuoteTick>(None);
        {
            let mut msgbus = msgbus.borrow_mut();
            let topic = msgbus.switchboard.get_quote_topic(quote.instrument_id);
            msgbus.subscribe(topic, handler.clone(), None);
        }

        let mut data_engine = data_engine.borrow_mut();
        data_engine.process_data(Data::Quote(quote));
        let cache = &data_engine.cache.borrow();
        let messages = get_saved_messages::<QuoteTick>(handler);

        assert_eq!(cache.quote(&quote.instrument_id), Some(quote).as_ref());
        assert_eq!(messages.len(), 1);
        assert!(messages.contains(&quote));
    }

    #[rstest]
    fn test_process_trade_tick(
        audusd_sim: CurrencyPair,
        msgbus: Rc<RefCell<MessageBus>>,
        switchboard: MessagingSwitchboard,
        data_engine: Rc<RefCell<DataEngine>>,
        data_client: DataClientAdapter,
    ) {
        let client_id = data_client.client_id;
        let venue = data_client.venue;
        data_engine.borrow_mut().register_client(data_client, None);

        let metadata = indexmap! {
            "instrument_id".to_string() => audusd_sim.id.to_string(),
        };
        let data_type = DataType::new(stringify!(TradeTick), Some(metadata));
        let cmd = SubscriptionCommand::new(
            client_id,
            venue,
            data_type,
            Action::Subscribe,
            UUID4::new(),
            UnixNanos::default(),
        );

        let endpoint = switchboard.data_engine_execute;
        let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
            id: endpoint,
            engine_ref: data_engine.clone(),
        }));
        msgbus.borrow_mut().register(endpoint, handler);
        msgbus.borrow().send(&endpoint, &cmd as &dyn Any);

        let trade = TradeTick::default();
        let handler = get_message_saving_handler::<TradeTick>(None);
        {
            let mut msgbus = msgbus.borrow_mut();
            let topic = msgbus.switchboard.get_trade_topic(trade.instrument_id);
            msgbus.subscribe(topic, handler.clone(), None);
        }

        let mut data_engine = data_engine.borrow_mut();
        data_engine.process_data(Data::Trade(trade));
        let cache = &data_engine.cache.borrow();
        let messages = get_saved_messages::<TradeTick>(handler);

        assert_eq!(cache.trade(&trade.instrument_id), Some(trade).as_ref());
        assert_eq!(messages.len(), 1);
        assert!(messages.contains(&trade));
    }

    #[rstest]
    fn test_process_bar(
        audusd_sim: CurrencyPair,
        msgbus: Rc<RefCell<MessageBus>>,
        switchboard: MessagingSwitchboard,
        data_engine: Rc<RefCell<DataEngine>>,
        data_client: DataClientAdapter,
    ) {
        let client_id = data_client.client_id;
        let venue = data_client.venue;
        data_engine.borrow_mut().register_client(data_client, None);

        let bar = Bar::default();
        let metadata = indexmap! {
            "bar_type".to_string() => bar.bar_type.to_string(),
        };
        let data_type = DataType::new(stringify!(Bar), Some(metadata));
        let cmd = SubscriptionCommand::new(
            client_id,
            venue,
            data_type,
            Action::Subscribe,
            UUID4::new(),
            UnixNanos::default(),
        );

        let endpoint = switchboard.data_engine_execute;
        let handler = ShareableMessageHandler(Rc::new(SubscriptionCommandHandler {
            id: endpoint,
            engine_ref: data_engine.clone(),
        }));
        msgbus.borrow_mut().register(endpoint, handler);
        msgbus.borrow().send(&endpoint, &cmd as &dyn Any);

        let handler = get_message_saving_handler::<Bar>(None);
        {
            let mut msgbus = msgbus.borrow_mut();
            let topic = msgbus.switchboard.get_bar_topic(bar.bar_type);
            msgbus.subscribe(topic, handler.clone(), None);
        }

        let mut data_engine = data_engine.borrow_mut();
        data_engine.process_data(Data::Bar(bar));
        let cache = &data_engine.cache.borrow();
        let messages = get_saved_messages::<Bar>(handler);

        assert_eq!(cache.bar(&bar.bar_type), Some(bar).as_ref());
        assert_eq!(messages.len(), 1);
        assert!(messages.contains(&bar));
    }
}
