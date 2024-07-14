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
    cell::RefCell,
    collections::{HashMap, HashSet},
    marker::PhantomData,
    ops::Deref,
    rc::Rc,
};

use log;
use nautilus_common::{
    cache::Cache,
    clock::Clock,
    component::{Disposed, PreInitialized, Ready, Running, Starting, State, Stopped, Stopping},
    enums::ComponentState,
    logging::{CMD, RECV},
    messages::data::{DataCommand, DataCommandAction, DataRequest, DataResponse},
    msgbus::MessageBus,
};
use nautilus_core::correctness;
use nautilus_model::{
    data::{bar::BarType, delta::OrderBookDelta, Data, DataType},
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::synthetic::SyntheticInstrument,
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
    pub fn start(self) -> DataEngine<Starting> {
        for client in self.clients.values() {
            client.start()
        }
        self.transition()
    }

    pub fn stop(self) -> DataEngine<Stopping> {
        for client in self.clients.values() {
            client.stop()
        }
        self.transition()
    }

    pub fn reset(self) -> DataEngine<Ready> {
        for client in self.clients.values() {
            client.reset()
        }
        self.transition()
    }

    pub fn dispose(mut self) -> DataEngine<Disposed> {
        for client in self.clients.values() {
            client.dispose()
        }
        self.clock.cancel_timers();
        self.transition()
    }
}

impl DataEngine<Starting> {
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

    pub fn stop(self) -> DataEngine<Stopping> {
        self.transition()
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

    fn handle_subscribe(&mut self, client_id: ClientId, command: DataCommand) {
        match command.data_type.type_name() {
            stringify!(InstrumentAny) => self.handle_subscribe_instrument(client_id, command),
            stringify!(OrderBookDelta) => {
                self.handle_subscribe_order_book_deltas(client_id, command)
            }
            stringify!(QuoteTick) => self.handle_subscribe_quote_ticks(client_id, command),
            stringify!(TradeTick) => self.handle_subscribe_trade_ticks(client_id, command),
            _ => panic!(
                "Invalid data type for `Subscribe` action {}",
                command.data_type.type_name()
            ),
        }
    }

    fn handle_unsubscribe(&mut self, client_id: ClientId, command: DataCommand) {
        match command.data_type.type_name() {
            stringify!(InstrumentAny) => self.handle_unsubscribe_instrument(client_id, command),
            stringify!(OrderBookDelta) => {
                self.handle_unsubscribe_order_book_deltas(client_id, command)
            }
            stringify!(QuoteTick) => self.handle_unsubscribe_quote_ticks(client_id, command),
            stringify!(TradeTick) => self.handle_unsubscribe_trade_ticks(client_id, command),
            _ => panic!(
                "Invalid data type for `Unsubscribe` action {}",
                command.data_type.type_name()
            ),
        }
    }

    fn handle_subscribe_instrument(&mut self, client_id: ClientId, command: DataCommand) {
        let instrument_id = command
            .data_type
            .parse_instrument_id_from_metadata()
            .expect("Invalid `InstrumentId`");

        let venue = command
            .data_type
            .parse_venue_from_metadata()
            .expect("Invalid `Venue`");

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

    fn handle_subscribe_order_book_deltas(&mut self, client_id: ClientId, command: DataCommand) {
        let instrument_id = command
            .data_type
            .parse_instrument_id_from_metadata()
            .expect("Invalid `InstrumentId`")
            .expect("Error on subscribe");
        let book_type = command.data_type.parse_book_type_from_metadata().unwrap();
        let depth = command.data_type.parse_depth_from_metadata().unwrap();

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

    fn handle_subscribe_quote_ticks(&mut self, client_id: ClientId, command: DataCommand) {
        let instrument_id = command
            .data_type
            .parse_instrument_id_from_metadata()
            .expect("Invalid `InstrumentId`")
            .expect("Error on subscribe");
        let book_type = command.data_type.parse_book_type_from_metadata().unwrap();
        let depth = command.data_type.parse_depth_from_metadata().unwrap();

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
            .expect("Invalid `InstrumentId`")
            .expect("Error on subscribe");

        // SAFETY: client_id already determined
        let client = self.clients.get_mut(&client_id).unwrap();

        if !client.subscribed_trade_ticks().contains(&instrument_id) {
            client
                .subscribe_trade_ticks(instrument_id)
                .expect("Error on subscribe");
        }
    }

    fn handle_unsubscribe_instrument(&mut self, client_id: ClientId, command: DataCommand) {
        let instrument_id = command
            .data_type
            .parse_instrument_id_from_metadata()
            .expect("Invalid `InstrumentId`");

        let venue = command
            .data_type
            .parse_venue_from_metadata()
            .expect("Invalid `Venue`");

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

    fn handle_unsubscribe_order_book_deltas(&mut self, client_id: ClientId, command: DataCommand) {
        let instrument_id = command
            .data_type
            .parse_instrument_id_from_metadata()
            .expect("Invalid `InstrumentId`")
            .expect("Error on unsubscribe");

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

    fn handle_unsubscribe_quote_ticks(&mut self, client_id: ClientId, command: DataCommand) {
        let instrument_id = command
            .data_type
            .parse_instrument_id_from_metadata()
            .expect("Invalid `InstrumentId`")
            .expect("Error on unsubscribe");

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
            .expect("Invalid `InstrumentId`")
            .expect("Error on unsubscribe");

        // SAFETY: client_id already determined
        let client = self.clients.get_mut(&client_id).unwrap();

        if client.subscribed_trade_ticks().contains(&instrument_id) {
            client
                .unsubscribe_trade_ticks(instrument_id)
                .expect("Error on unsubscribe");
        }
    }

    pub fn process(&self, data: Data) {
        todo!()
    }

    pub fn request(&self, request: DataRequest) {
        todo!()
    }

    pub fn response(&self, response: DataResponse) {
        todo!()
    }

    fn update_order_book(&self, data: &Data) {
        // Only apply data if there is a book being managed,
        // as it may be being managed manually.
        if let Some(book) = self.cache.borrow_mut().order_book(data.instrument_id()) {
            match data {
                Data::Delta(delta) => book.apply_delta(delta),
                Data::Deltas(deltas) => book.apply_deltas(deltas.deref()),
                Data::Depth10(depth) => book.apply_depth(depth),
                _ => log::error!("Invalid data type for book update"),
            }
        }
    }
}

impl DataEngine<Stopping> {
    pub fn on_stop(self) -> DataEngine<Stopped> {
        self.transition()
    }
}

impl DataEngine<Stopped> {
    pub fn reset(self) -> DataEngine<Ready> {
        self.transition()
    }

    pub fn dispose(self) -> DataEngine<Disposed> {
        self.transition()
    }
}
