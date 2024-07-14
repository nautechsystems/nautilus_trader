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

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use std::{
    any::Any,
    cell::RefCell,
    collections::{HashMap, HashSet},
    marker::PhantomData,
    ops::Deref,
    rc::Rc,
    sync::Arc,
};

use log;
use nautilus_common::{
    cache::Cache,
    clock::Clock,
    component::{Disposed, PreInitialized, Ready, Running, Starting, State, Stopped, Stopping},
    enums::ComponentState,
    logging::{CMD, RECV, RES},
    messages::data::{DataCommand, DataCommandAction, DataRequest, DataResponse},
    msgbus::MessageBus,
};
use nautilus_core::correctness;
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
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{any::InstrumentAny, synthetic::SyntheticInstrument},
};

use crate::client::DataClient;

pub struct DataEngineConfig {
    pub debug: bool,
    pub time_bars_build_with_no_updates: bool,
    pub time_bars_timestamp_on_close: bool,
    pub time_bars_interval_type: String, // Make this an enum `BarIntervalType`
    pub validate_data_sequence: bool,
    pub buffer_deltas: bool,
}

pub struct DataEngine<State = PreInitialized> {
    state: PhantomData<State>,
    clock: Box<dyn Clock>,
    cache: Rc<RefCell<Cache>>,
    msgbus: Rc<RefCell<MessageBus>>,
    clients: HashMap<ClientId, Box<dyn DataClient>>,
    default_client: Option<Box<dyn DataClient>>,
    routing_map: HashMap<Venue, ClientId>,
    // order_book_intervals: HashMap<(InstrumentId, usize), Vec<fn(&OrderBook)>>,  // TODO
    // bar_aggregators:  // TODO
    synthetic_quote_feeds: HashMap<InstrumentId, Vec<SyntheticInstrument>>,
    synthetic_trade_feeds: HashMap<InstrumentId, Vec<SyntheticInstrument>>,
    buffered_deltas_map: HashMap<InstrumentId, Vec<OrderBookDelta>>,
    config: DataEngineConfig,
}

impl DataEngine {
    #[must_use]
    pub fn new(
        clock: Box<dyn Clock>,
        cache: Rc<RefCell<Cache>>,
        msgbus: Rc<RefCell<MessageBus>>,
        config: DataEngineConfig,
    ) -> Self {
        Self {
            state: PhantomData::<PreInitialized>,
            clock,
            cache,
            msgbus,
            clients: HashMap::new(),
            default_client: None,
            routing_map: HashMap::new(),
            synthetic_quote_feeds: HashMap::new(),
            synthetic_trade_feeds: HashMap::new(),
            buffered_deltas_map: HashMap::new(),
            config,
        }
    }
}

impl<S: State> DataEngine<S> {
    fn transition<NewState>(self) -> DataEngine<NewState> {
        DataEngine {
            state: PhantomData,
            clock: self.clock,
            cache: self.cache,
            msgbus: self.msgbus,
            clients: self.clients,
            default_client: self.default_client,
            routing_map: self.routing_map,
            synthetic_quote_feeds: self.synthetic_quote_feeds,
            synthetic_trade_feeds: self.synthetic_trade_feeds,
            buffered_deltas_map: self.buffered_deltas_map,
            config: self.config,
        }
    }

    #[must_use]
    pub fn state(&self) -> ComponentState {
        S::state()
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

    #[must_use]
    pub fn default_client(&self) -> Option<&dyn DataClient> {
        self.default_client.as_deref()
    }

    // -- SUBSCRIPTIONS ---------------------------------------------------------------------------

    fn collect_subscriptions<F, T>(&self, get_subs: F) -> Vec<T>
    where
        F: Fn(&Box<dyn DataClient>) -> &HashSet<T>,
        T: Clone,
    {
        let mut subs = Vec::new();
        for client in self.clients.values() {
            subs.extend(get_subs(client).iter().cloned());
        }
        subs
    }

    #[must_use]
    pub fn subscribed_custom_data(&self) -> Vec<DataType> {
        self.collect_subscriptions(|client| client.subscribed_generic_data())
    }

    #[must_use]
    pub fn subscribed_instruments(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| client.subscribed_instruments())
    }

    #[must_use]
    pub fn subscribed_order_book_deltas(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| client.subscribed_order_book_deltas())
    }

    #[must_use]
    pub fn subscribed_order_book_snapshots(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| client.subscribed_order_book_snapshots())
    }

    #[must_use]
    pub fn subscribed_quote_ticks(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| client.subscribed_quote_ticks())
    }

    #[must_use]
    pub fn subscribed_trade_ticks(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| client.subscribed_trade_ticks())
    }

    #[must_use]
    pub fn subscribed_bars(&self) -> Vec<BarType> {
        self.collect_subscriptions(|client| client.subscribed_bars())
    }

    #[must_use]
    pub fn subscribed_venue_status(&self) -> Vec<Venue> {
        self.collect_subscriptions(|client| client.subscribed_venue_status())
    }

    #[must_use]
    pub fn subscribed_instrument_status(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| client.subscribed_instrument_status())
    }

    #[must_use]
    pub fn subscribed_instrument_close(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| client.subscribed_instrument_close())
    }
}

impl DataEngine<PreInitialized> {
    // pub fn register_catalog(&mut self, catalog: ParquetDataCatalog) {}  TODO: Implement catalog

    /// Register the given data `client` with the engine.
    pub fn register_client(&mut self, client: Box<dyn DataClient>, routing: Option<Venue>) {
        if let Some(routing) = routing {
            self.routing_map.insert(routing, client.client_id());
            log::info!("Set client {} routing for {routing}", client.client_id());
        }

        log::info!("Registered client {}", client.client_id());
        self.clients.insert(client.client_id(), client);
    }

    /// Register the given data `client` with the engine as the default routing client.
    ///
    /// When a specific venue routing cannot be found, this client will receive messages.
    ///
    /// # Warnings
    ///
    /// Any existing default routing client will be overwritten.
    pub fn register_default_client(&mut self, client: Box<dyn DataClient>) {
        log::info!("Registered default client {}", client.client_id());
        self.default_client = Some(client);
    }

    /// Deregister the data client with the given `client_id` from the engine.
    ///
    /// # Panics
    ///
    /// If a client with `client_id` has not already been registered.
    pub fn deregister_client(&mut self, client_id: ClientId) {
        // TODO: We could return a `Result` but then this is part of system wiring and instead of
        // propagating results all over the place it may be cleaner to just immediately fail
        // for these sorts of design-time errors?
        correctness::check_key_in_map(&client_id, &self.clients, "client_id", "clients").unwrap();

        self.clients.remove(&client_id);
        log::info!("Deregistered client {client_id}");
    }

    fn initialize(self) -> DataEngine<Ready> {
        self.transition()
    }
}

impl DataEngine<Ready> {
    #[must_use]
    pub fn start(self) -> DataEngine<Starting> {
        for client in self.clients.values() {
            client.start();
        }
        self.transition()
    }

    #[must_use]
    pub fn stop(self) -> DataEngine<Stopping> {
        for client in self.clients.values() {
            client.stop();
        }
        self.transition()
    }

    #[must_use]
    pub fn reset(self) -> Self {
        for client in self.clients.values() {
            client.reset();
        }
        self.transition()
    }

    #[must_use]
    pub fn dispose(mut self) -> DataEngine<Disposed> {
        for client in self.clients.values() {
            client.dispose();
        }
        self.clock.cancel_timers();
        self.transition()
    }
}

impl DataEngine<Starting> {
    #[must_use]
    pub fn on_start(self) -> DataEngine<Running> {
        self.transition()
    }
}

impl DataEngine<Running> {
    pub fn connect(&self) {
        todo!() //  Implement actual client connections for a live/sandbox context
    }

    pub fn disconnect(&self) {
        todo!() // Implement actual client connections for a live/sandbox context
    }

    #[must_use]
    pub fn stop(self) -> DataEngine<Stopping> {
        self.transition()
    }

    pub fn process(&self, data: Data) {
        match data {
            Data::Delta(delta) => self.handle_delta(delta),
            Data::Deltas(deltas) => self.handle_deltas(deltas.deref().clone()), // TODO: Optimize
            Data::Depth10(depth) => self.handle_depth10(depth),
            Data::Quote(quote) => self.handle_quote(quote),
            Data::Trade(trade) => self.handle_trade(trade),
            Data::Bar(bar) => self.handle_bar(bar),
        }
    }

    pub fn execute(&mut self, command: DataCommand) {
        if self.config.debug {
            log::debug!("{}", format!("{RECV}{CMD} commmand")); // TODO: Display for command
        }

        // Determine the client ID
        let client_id = if self.clients.contains_key(&command.client_id) {
            Some(command.client_id)
        } else {
            self.routing_map.get(&command.venue).copied()
        };

        if let Some(client_id) = client_id {
            match command.action {
                DataCommandAction::Subscribe => self.handle_subscribe(client_id, command),
                DataCommandAction::Unsubscibe => self.handle_unsubscribe(client_id, command),
            }
        } else {
            log::error!(
                "Cannot execute command: no data client configured for {} or `client_id` {}",
                command.venue,
                command.client_id
            );
        }
    }

    pub fn request(&mut self, request: DataRequest) {
        if self.config.debug {
            log::debug!("{}", format!("{RECV}{RES} response")); // TODO: Display for response
        }

        // Determine the client ID
        let client_id = if self.clients.contains_key(&request.client_id) {
            Some(request.client_id)
        } else {
            self.routing_map.get(&request.venue).copied()
        };

        if let Some(client_id) = client_id {
            match request.data_type.type_name() {
                stringify!(InstrumentAny) => self.handle_instruments_request(client_id, request),
                stringify!(QuoteTick) => self.handle_quote_ticks_request(client_id, request),
                stringify!(TradeTick) => self.handle_trade_ticks_request(client_id, request),
                stringify!(Bar) => self.handle_bars_request(client_id, request),
                _ => self.handle_request(client_id, request),
            }
        } else {
            log::error!(
                "Cannot execute request: no data client configured for {} or `client_id` {}",
                request.venue,
                request.client_id
            );
        }
    }

    pub fn response(&self, response: DataResponse) {
        if self.config.debug {
            log::debug!("{}", format!("{RECV}{RES} response")); // TODO: Display for response
        }

        match response.data_type.type_name() {
            stringify!(InstrumentAny) => {
                let instruments = Arc::downcast::<Vec<InstrumentAny>>(response.data.clone())
                    .expect("Invalid response data");
                self.handle_instruments(instruments);
            }
            stringify!(QuoteTick) => {
                let quotes = Arc::downcast::<Vec<QuoteTick>>(response.data.clone())
                    .expect("Invalid response data");
                self.handle_quotes(quotes);
            }
            stringify!(TradeTick) => {
                let trades = Arc::downcast::<Vec<TradeTick>>(response.data.clone())
                    .expect("Invalid response data");
                self.handle_trades(trades);
            }
            stringify!(Bar) => {
                let bars = Arc::downcast::<Vec<Bar>>(response.data.clone())
                    .expect("Invalid response data");
                self.handle_bars(bars);
            }
            _ => {} // Nothing else to handle
        }

        // self.msgbus.response()  // TODO: Send response to registered handler
    }

    // -- DATA HANDLERS ---------------------------------------------------------------------------

    fn handle_instrument(&self, instrument: InstrumentAny) {
        if let Err(e) = self.cache.borrow_mut().add_instrument(instrument.clone()) {
            log::error!("Error on cache insert: {e}");
        }

        let instrument_id = instrument.id();
        let topic = format!(
            "data.instrument.{}.{}",
            instrument_id.venue, instrument_id.symbol
        );

        self.msgbus
            .borrow()
            .publish(&topic, &instrument as &dyn Any); // TODO: Optimize
    }

    fn handle_delta(&self, delta: OrderBookDelta) {
        // TODO: Manage buffered deltas
        // TODO: Manage book

        let topic = format!(
            "data.book.deltas.{}.{}",
            delta.instrument_id.venue, delta.instrument_id.symbol
        );

        self.msgbus.borrow().publish(&topic, &delta as &dyn Any); // TODO: Optimize
    }

    fn handle_deltas(&self, deltas: OrderBookDeltas) {
        // TODO: Manage book

        let topic = format!(
            "data.book.snapshots.{}.{}", // TODO: Revise snapshots topic component
            deltas.instrument_id.venue, deltas.instrument_id.symbol
        );
        self.msgbus.borrow().publish(&topic, &deltas as &dyn Any); // TODO: Optimize
    }

    fn handle_depth10(&self, depth: OrderBookDepth10) {
        // TODO: Manage book

        let topic = format!(
            "data.book.depth.{}.{}",
            depth.instrument_id.venue, depth.instrument_id.symbol
        );
        self.msgbus.borrow().publish(&topic, &depth as &dyn Any); // TODO: Optimize
    }

    fn handle_quote(&self, quote: QuoteTick) {
        if let Err(e) = self.cache.borrow_mut().add_quote(quote) {
            log::error!("Error on cache insert: {e}");
        }

        // TODO: Handle synthetics

        let topic = format!(
            "data.quotes.{}.{}",
            quote.instrument_id.venue, quote.instrument_id.symbol
        );
        self.msgbus.borrow().publish(&topic, &quote as &dyn Any); // TODO: Optimize
    }

    fn handle_trade(&self, trade: TradeTick) {
        if let Err(e) = self.cache.borrow_mut().add_trade(trade) {
            log::error!("Error on cache insert: {e}");
        }

        // TODO: Handle synthetics

        let topic = format!(
            "data.trades.{}.{}",
            trade.instrument_id.venue, trade.instrument_id.symbol
        );
        self.msgbus.borrow().publish(&topic, &trade as &dyn Any); // TODO: Optimize
    }

    fn handle_bar(&self, bar: Bar) {
        if let Err(e) = self.cache.borrow_mut().add_bar(bar) {
            log::error!("Error on cache insert: {e}");
        }

        // TODO: Handle additional bar logic

        let topic = format!("data.bars.{}", bar.bar_type);
        self.msgbus.borrow().publish(&topic, &bar as &dyn Any); // TODO: Optimize
    }

    // -- COMMAND HANDLERS ------------------------------------------------------------------------

    fn handle_subscribe(&mut self, client_id: ClientId, command: DataCommand) {
        match command.data_type.type_name() {
            stringify!(InstrumentAny) => self.handle_subscribe_instrument(client_id, command),
            stringify!(OrderBookDelta) => self.handle_subscribe_deltas(client_id, command),
            stringify!(OrderBookDeltas) => self.handle_subscribe_snapshots(client_id, command),
            stringify!(OrderBookDepth10) => self.handle_subscribe_snapshots(client_id, command),
            stringify!(QuoteTick) => self.handle_subscribe_quote_ticks(client_id, command),
            stringify!(TradeTick) => self.handle_subscribe_trade_ticks(client_id, command),
            stringify!(Bar) => self.handle_subscribe_bars(client_id, command),
            _ => self.handle_subscribe_generic(client_id, command),
        }
    }

    fn handle_unsubscribe(&mut self, client_id: ClientId, command: DataCommand) {
        match command.data_type.type_name() {
            stringify!(InstrumentAny) => self.handle_unsubscribe_instrument(client_id, command),
            stringify!(OrderBookDelta) => self.handle_unsubscribe_deltas(client_id, command),
            stringify!(OrderBookDeltas) => self.handle_unsubscribe_snapshots(client_id, command),
            stringify!(OrderBookDepth10) => self.handle_unsubscribe_snapshots(client_id, command),
            stringify!(QuoteTick) => self.handle_unsubscribe_quote_ticks(client_id, command),
            stringify!(TradeTick) => self.handle_unsubscribe_trade_ticks(client_id, command),
            stringify!(Bar) => self.handle_unsubscribe_bars(client_id, command),
            _ => self.handle_unsubscribe_generic(client_id, command),
        }
    }

    fn handle_subscribe_instrument(&mut self, client_id: ClientId, command: DataCommand) {
        let instrument_id = command.data_type.parse_instrument_id_from_metadata();
        let venue = command.data_type.parse_venue_from_metadata();

        // SAFETY: client_id already determined
        let client = self.clients.get_mut(&client_id).unwrap();

        if let Some(instrument_id) = instrument_id {
            if !client.subscribed_instruments().contains(&instrument_id) {
                client
                    .subscribe_instrument(instrument_id)
                    .expect("Error on subscribe");
            }
        }

        if let Some(venue) = venue {
            if !client.subscribed_instrument_venues().contains(&venue) {
                client
                    .subscribe_instruments(Some(venue))
                    .expect("Error on subscribe");
            }
        }
    }

    fn handle_subscribe_deltas(&mut self, client_id: ClientId, command: DataCommand) {
        let instrument_id = command
            .data_type
            .parse_instrument_id_from_metadata()
            .expect("Error on subscribe: no 'instrument_id' in metadata");

        let book_type = command.data_type.parse_book_type_from_metadata();
        let depth = command.data_type.parse_depth_from_metadata();

        // SAFETY: client_id already determined
        let client = self.clients.get_mut(&client_id).unwrap();

        if !client
            .subscribed_order_book_deltas()
            .contains(&instrument_id)
        {
            client
                .subscribe_order_book_deltas(instrument_id, book_type, depth)
                .expect("Error on subscribe");
        }
    }

    fn handle_subscribe_snapshots(&mut self, client_id: ClientId, command: DataCommand) {
        let instrument_id = command
            .data_type
            .parse_instrument_id_from_metadata()
            .expect("Error on subscribe: no 'instrument_id' in metadata");

        let book_type = command.data_type.parse_book_type_from_metadata();
        let depth = command.data_type.parse_depth_from_metadata();

        // SAFETY: client_id already determined
        let client = self.clients.get_mut(&client_id).unwrap();

        if !client
            .subscribed_order_book_snapshots()
            .contains(&instrument_id)
        {
            client
                .subscribe_order_book_snapshots(instrument_id, book_type, depth)
                .expect("Error on subscribe");
        }
    }

    fn handle_subscribe_quote_ticks(&mut self, client_id: ClientId, command: DataCommand) {
        let instrument_id = command
            .data_type
            .parse_instrument_id_from_metadata()
            .expect("Error on subscribe: no 'instrument_id' in metadata");

        let book_type = command.data_type.parse_book_type_from_metadata();
        let depth = command.data_type.parse_depth_from_metadata();

        // SAFETY: client_id already determined
        let client = self.clients.get_mut(&client_id).unwrap();

        if !client.subscribed_quote_ticks().contains(&instrument_id) {
            client
                .subscribe_quote_ticks(instrument_id)
                .expect("Error on subscribe");
        }
    }

    fn handle_subscribe_trade_ticks(&mut self, client_id: ClientId, command: DataCommand) {
        let instrument_id = command
            .data_type
            .parse_instrument_id_from_metadata()
            .expect("Error on subscribe: no 'instrument_id' in metadata");

        // SAFETY: client_id already determined
        let client = self.clients.get_mut(&client_id).unwrap();

        if !client.subscribed_trade_ticks().contains(&instrument_id) {
            client
                .subscribe_trade_ticks(instrument_id)
                .expect("Error on subscribe");
        }
    }

    fn handle_subscribe_bars(&mut self, client_id: ClientId, command: DataCommand) {
        let bar_type = command.data_type.parse_bar_type_from_metadata();

        // SAFETY: client_id already determined
        let client = self.clients.get_mut(&client_id).unwrap();

        if !client.subscribed_bars().contains(&bar_type) {
            client.subscribe_bars(bar_type).expect("Error on subscribe");
        }
    }

    fn handle_subscribe_generic(&mut self, client_id: ClientId, command: DataCommand) {
        // SAFETY: client_id already determined
        let client = self.clients.get_mut(&client_id).unwrap();
        if !client
            .subscribed_generic_data()
            .contains(&command.data_type)
        {
            client
                .subscribe(command.data_type)
                .expect("Error on subscribe");
        }
    }

    fn handle_unsubscribe_instrument(&mut self, client_id: ClientId, command: DataCommand) {
        let instrument_id = command.data_type.parse_instrument_id_from_metadata();
        let venue = command.data_type.parse_venue_from_metadata();

        // SAFETY: client_id already determined
        let client = self.clients.get_mut(&client_id).unwrap();

        if let Some(instrument_id) = instrument_id {
            if client.subscribed_instruments().contains(&instrument_id) {
                client
                    .unsubscribe_instrument(instrument_id)
                    .expect("Error on unsubscribe");
            }
        }

        if let Some(venue) = venue {
            if client.subscribed_instrument_venues().contains(&venue) {
                client
                    .unsubscribe_instruments(Some(venue))
                    .expect("Error on unsubscribe");
            }
        }
    }

    fn handle_unsubscribe_deltas(&mut self, client_id: ClientId, command: DataCommand) {
        let instrument_id = command
            .data_type
            .parse_instrument_id_from_metadata()
            .expect("Error on subscribe: no 'instrument_id' in metadata");

        // SAFETY: client_id already determined
        let client = self.clients.get_mut(&client_id).unwrap();

        if client
            .subscribed_order_book_deltas()
            .contains(&instrument_id)
        {
            client
                .unsubscribe_order_book_deltas(instrument_id)
                .expect("Error on subscribe");
        }
    }

    fn handle_unsubscribe_snapshots(&mut self, client_id: ClientId, command: DataCommand) {
        let instrument_id = command
            .data_type
            .parse_instrument_id_from_metadata()
            .expect("Error on subscribe: no 'instrument_id' in metadata");

        // SAFETY: client_id already determined
        let client = self.clients.get_mut(&client_id).unwrap();

        if client
            .subscribed_order_book_snapshots()
            .contains(&instrument_id)
        {
            client
                .unsubscribe_order_book_snapshots(instrument_id)
                .expect("Error on subscribe");
        }
    }

    fn handle_unsubscribe_quote_ticks(&mut self, client_id: ClientId, command: DataCommand) {
        let instrument_id = command
            .data_type
            .parse_instrument_id_from_metadata()
            .expect("Error on subscribe: no 'instrument_id' in metadata");

        // SAFETY: client_id already determined
        let client = self.clients.get_mut(&client_id).unwrap();
        if client.subscribed_quote_ticks().contains(&instrument_id) {
            client
                .unsubscribe_quote_ticks(instrument_id)
                .expect("Error on unsubscribe");
        }
    }

    fn handle_unsubscribe_trade_ticks(&mut self, client_id: ClientId, command: DataCommand) {
        let instrument_id = command
            .data_type
            .parse_instrument_id_from_metadata()
            .expect("Error on subscribe: no 'instrument_id' in metadata");

        // SAFETY: client_id already determined
        let client = self.clients.get_mut(&client_id).unwrap();

        if client.subscribed_trade_ticks().contains(&instrument_id) {
            client
                .unsubscribe_trade_ticks(instrument_id)
                .expect("Error on unsubscribe");
        }
    }

    fn handle_unsubscribe_bars(&mut self, client_id: ClientId, command: DataCommand) {
        let bar_type = command.data_type.parse_bar_type_from_metadata();

        // SAFETY: client_id already determined
        let client = self.clients.get_mut(&client_id).unwrap();

        if client.subscribed_bars().contains(&bar_type) {
            client
                .unsubscribe_bars(bar_type)
                .expect("Error on unsubscribe");
        }
    }

    fn handle_unsubscribe_generic(&mut self, client_id: ClientId, command: DataCommand) {
        // SAFETY: client_id already determined
        let client = self.clients.get_mut(&client_id).unwrap();
        if client
            .subscribed_generic_data()
            .contains(&command.data_type)
        {
            client
                .unsubscribe(command.data_type)
                .expect("Error on unsubscribe");
        }
    }

    // -- REQUEST HANDLERS ------------------------------------------------------------------------

    fn handle_request(&mut self, client_id: ClientId, request: DataRequest) {
        // SAFETY: client_id already determined
        let client = self.clients.get_mut(&client_id).unwrap();
        client.request(request.correlation_id, request.data_type);
    }

    fn handle_instruments_request(&mut self, client_id: ClientId, request: DataRequest) {
        let instrument_id = request.data_type.parse_instrument_id_from_metadata();
        let venue = request.data_type.parse_venue_from_metadata();
        let start = request.data_type.parse_start_from_metadata();
        let end = request.data_type.parse_end_from_metadata();

        // SAFETY: client_id already determined
        let client = self.clients.get_mut(&client_id).unwrap();

        if let Some(instrument_id) = instrument_id {
            client.request_instrument(request.correlation_id, instrument_id, start, end);
        }

        if let Some(venue) = venue {
            client.request_instruments(request.correlation_id, venue, start, end);
        }
    }

    fn handle_quote_ticks_request(&mut self, client_id: ClientId, request: DataRequest) {
        let instrument_id = request
            .data_type
            .parse_instrument_id_from_metadata()
            .expect("Error on request: no 'instrument_id' found in metadata");
        let start = request.data_type.parse_start_from_metadata();
        let end = request.data_type.parse_end_from_metadata();
        let limit = request.data_type.parse_limit_from_metadata();

        // SAFETY: client_id already determined
        let client = self.clients.get_mut(&client_id).unwrap();
        client.request_quote_ticks(request.correlation_id, instrument_id, start, end, limit);
    }

    fn handle_trade_ticks_request(&mut self, client_id: ClientId, request: DataRequest) {
        let instrument_id = request
            .data_type
            .parse_instrument_id_from_metadata()
            .expect("Error on request: no 'instrument_id' found in metadata");
        let start = request.data_type.parse_start_from_metadata();
        let end = request.data_type.parse_end_from_metadata();
        let limit = request.data_type.parse_limit_from_metadata();

        // SAFETY: client_id already determined
        let client = self.clients.get_mut(&client_id).unwrap();
        client.request_trade_ticks(request.correlation_id, instrument_id, start, end, limit);
    }

    fn handle_bars_request(&mut self, client_id: ClientId, request: DataRequest) {
        let bar_type = request.data_type.parse_bar_type_from_metadata();
        let start = request.data_type.parse_start_from_metadata();
        let end = request.data_type.parse_end_from_metadata();
        let limit = request.data_type.parse_limit_from_metadata();

        // SAFETY: client_id already determined
        let client = self.clients.get_mut(&client_id).unwrap();
        client.request_bars(request.correlation_id, bar_type, start, end, limit);
    }

    // -- RESPONSE HANDLERS -----------------------------------------------------------------------

    fn handle_instruments(&self, instruments: Arc<Vec<InstrumentAny>>) {
        for instrument in instruments.iter() {
            if let Err(e) = self.cache.borrow_mut().add_instrument(instrument.clone()) {
                log::error!("Error on cache insert: {e}");
            }
        }
    }

    fn handle_quotes(&self, quotes: Arc<Vec<QuoteTick>>) {
        for quote in quotes.iter() {
            if let Err(e) = self.cache.borrow_mut().add_quote(*quote) {
                log::error!("Error on cache insert: {e}");
            }
        }
    }

    fn handle_trades(&self, trades: Arc<Vec<TradeTick>>) {
        for trade in trades.iter() {
            if let Err(e) = self.cache.borrow_mut().add_trade(*trade) {
                log::error!("Error on cache insert: {e}");
            }
        }
    }

    fn handle_bars(&self, bars: Arc<Vec<Bar>>) {
        for bar in bars.iter() {
            if let Err(e) = self.cache.borrow_mut().add_bar(*bar) {
                log::error!("Error on cache insert: {e}");
            }
        }
    }

    // -- INTERNAL --------------------------------------------------------------------------------

    fn update_order_book(&self, data: &Data) {
        // Only apply data if there is a book being managed,
        // as it may be being managed manually.
        if let Some(book) = self.cache.borrow_mut().order_book(data.instrument_id()) {
            match data {
                Data::Delta(delta) => book.apply_delta(delta),
                Data::Deltas(deltas) => book.apply_deltas(deltas),
                Data::Depth10(depth) => book.apply_depth(depth),
                _ => log::error!("Invalid data type for book update"),
            }
        }
    }
}

impl DataEngine<Stopping> {
    #[must_use]
    pub fn on_stop(self) -> DataEngine<Stopped> {
        self.transition()
    }
}

impl DataEngine<Stopped> {
    #[must_use]
    pub fn reset(self) -> DataEngine<Ready> {
        self.transition()
    }

    #[must_use]
    pub fn dispose(self) -> DataEngine<Disposed> {
        self.transition()
    }
}
