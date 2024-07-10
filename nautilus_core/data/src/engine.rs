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

use std::collections::HashMap;

use log;
use nautilus_common::{cache::Cache, msgbus::MessageBus};
use nautilus_core::{correctness, time::AtomicTime};
use nautilus_model::{
    data::delta::OrderBookDelta,
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

pub struct DataEngine {
    pub command_count: u64,
    pub event_count: u64,
    pub request_count: u64,
    pub response_count: u64,
    clock: &'static AtomicTime,
    cache: &'static Cache,
    msgbus: &'static MessageBus,
    clients: HashMap<ClientId, DataClient>,
    default_client: Option<DataClient>,
    routing_map: HashMap<Venue, ClientId>,
    // order_book_intervals: HashMap<(InstrumentId, usize), Vec<fn(&OrderBook)>>,  // TODO
    //bar_aggregators:  // TODO
    synthetic_quote_feeds: HashMap<InstrumentId, Vec<SyntheticInstrument>>,
    synthetic_trade_feeds: HashMap<InstrumentId, Vec<SyntheticInstrument>>,
    buffered_deltas_map: HashMap<InstrumentId, Vec<OrderBookDelta>>,
    config: DataEngineConfig,
}

impl DataEngine {
    #[must_use]
    pub fn registed_clients(&self) -> Vec<ClientId> {
        self.clients.keys().copied().collect()
    }

    #[must_use]
    pub const fn default_client(&self) -> Option<&DataClient> {
        self.default_client.as_ref()
    }

    #[must_use]
    pub fn check_connected(&self) -> bool {
        self.clients.values().all(|client| client.is_connected)
    }

    #[must_use]
    pub fn check_disconnected(&self) -> bool {
        self.clients.values().all(|client| !client.is_connected)
    }

    pub fn connect(&self) {
        todo!() //  Implement actual client connections for a live/sandbox context
    }

    pub fn disconnect(&self) {
        todo!() // Implement actual client connections for a live/sandbox context
    }

    pub fn register_catalog(&self) {
        todo!()
    }

    /// Register the given data `client` with the engine.
    pub fn register_client(&mut self, client: DataClient, routing: Option<Venue>) {
        if let Some(routing) = routing {
            self.routing_map.insert(routing, client.client_id);
            log::info!("Set client {} routing for {routing}", client.client_id);
        }

        log::info!("Registered client {}", client.client_id);
        self.clients.insert(client.client_id, client);
    }

    /// Register the given data `client` with the engine as the default routing client.
    ///
    /// When a specific venue routing cannot be found, this client will receive messages.
    ///
    /// # Warnings
    ///
    /// Any existing default routing client will be overwritten.
    pub fn register_default_client(&mut self, client: DataClient) {
        log::info!("Registered default client {}", client.client_id);
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
}
