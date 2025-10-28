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

pub mod book;
pub mod config;
mod handlers;

#[cfg(feature = "defi")]
pub mod pool;

use std::{
    any::Any,
    cell::{Ref, RefCell},
    collections::hash_map::Entry,
    fmt::Display,
    num::NonZeroUsize,
    rc::Rc,
};

use ahash::{AHashMap, AHashSet};
use book::{BookSnapshotInfo, BookSnapshotter, BookUpdater};
use config::DataEngineConfig;
use handlers::{BarBarHandler, BarQuoteHandler, BarTradeHandler};
use indexmap::IndexMap;
use nautilus_common::{
    cache::Cache,
    clock::Clock,
    logging::{RECV, RES},
    messages::data::{
        DataCommand, DataResponse, RequestCommand, SubscribeBars, SubscribeBookDeltas,
        SubscribeBookDepth10, SubscribeBookSnapshots, SubscribeCommand, UnsubscribeBars,
        UnsubscribeBookDeltas, UnsubscribeBookDepth10, UnsubscribeBookSnapshots,
        UnsubscribeCommand,
    },
    msgbus::{self, MStr, Topic, handler::ShareableMessageHandler, switchboard},
    timer::{TimeEvent, TimeEventCallback},
};
use nautilus_core::{
    correctness::{
        FAILED, check_key_in_map, check_key_not_in_map, check_predicate_false, check_predicate_true,
    },
    datetime::millis_to_nanos,
};
#[cfg(feature = "defi")]
use nautilus_model::defi::DefiData;
use nautilus_model::{
    data::{
        Bar, BarType, Data, DataType, FundingRateUpdate, IndexPriceUpdate, InstrumentClose,
        MarkPriceUpdate, OrderBookDelta, OrderBookDeltas, OrderBookDepth10, QuoteTick, TradeTick,
    },
    enums::{AggregationSource, BarAggregation, BookType, PriceType, RecordFlag},
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny, SyntheticInstrument},
    orderbook::OrderBook,
};
use nautilus_persistence::backend::catalog::ParquetDataCatalog;
use ustr::Ustr;

#[cfg(feature = "defi")]
#[allow(unused_imports)] // Brings DeFi impl blocks into scope
use crate::defi::engine as _;
#[cfg(feature = "defi")]
use crate::engine::pool::PoolUpdater;
use crate::{
    aggregation::{
        BarAggregator, RenkoBarAggregator, TickBarAggregator, TimeBarAggregator,
        ValueBarAggregator, VolumeBarAggregator,
    },
    client::DataClientAdapter,
};

/// Provides a high-performance `DataEngine` for all environments.
#[derive(Debug)]
pub struct DataEngine {
    pub(crate) clock: Rc<RefCell<dyn Clock>>,
    pub(crate) cache: Rc<RefCell<Cache>>,
    pub(crate) external_clients: AHashSet<ClientId>,
    clients: IndexMap<ClientId, DataClientAdapter>,
    default_client: Option<DataClientAdapter>,
    catalogs: AHashMap<Ustr, ParquetDataCatalog>,
    routing_map: IndexMap<Venue, ClientId>,
    book_intervals: AHashMap<NonZeroUsize, AHashSet<InstrumentId>>,
    book_updaters: AHashMap<InstrumentId, Rc<BookUpdater>>,
    book_snapshotters: AHashMap<InstrumentId, Rc<BookSnapshotter>>,
    bar_aggregators: AHashMap<BarType, Rc<RefCell<Box<dyn BarAggregator>>>>,
    bar_aggregator_handlers: AHashMap<BarType, Vec<(MStr<Topic>, ShareableMessageHandler)>>,
    _synthetic_quote_feeds: AHashMap<InstrumentId, Vec<SyntheticInstrument>>,
    _synthetic_trade_feeds: AHashMap<InstrumentId, Vec<SyntheticInstrument>>,
    buffered_deltas_map: AHashMap<InstrumentId, OrderBookDeltas>,
    pub(crate) msgbus_priority: u8,
    pub(crate) config: DataEngineConfig,
    #[cfg(feature = "defi")]
    pub(crate) pool_updaters: AHashMap<InstrumentId, Rc<PoolUpdater>>,
    #[cfg(feature = "defi")]
    pub(crate) pool_updaters_pending: AHashSet<InstrumentId>,
    #[cfg(feature = "defi")]
    pub(crate) pool_snapshot_pending: AHashSet<InstrumentId>,
    #[cfg(feature = "defi")]
    pub(crate) pool_event_buffers: AHashMap<InstrumentId, Vec<DefiData>>,
}

impl DataEngine {
    /// Creates a new [`DataEngine`] instance.
    #[must_use]
    pub fn new(
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
        config: Option<DataEngineConfig>,
    ) -> Self {
        let config = config.unwrap_or_default();

        let external_clients: AHashSet<ClientId> = config
            .external_clients
            .clone()
            .unwrap_or_default()
            .into_iter()
            .collect();

        Self {
            clock,
            cache,
            external_clients,
            clients: IndexMap::new(),
            default_client: None,
            catalogs: AHashMap::new(),
            routing_map: IndexMap::new(),
            book_intervals: AHashMap::new(),
            book_updaters: AHashMap::new(),
            book_snapshotters: AHashMap::new(),
            bar_aggregators: AHashMap::new(),
            bar_aggregator_handlers: AHashMap::new(),
            _synthetic_quote_feeds: AHashMap::new(),
            _synthetic_trade_feeds: AHashMap::new(),
            buffered_deltas_map: AHashMap::new(),
            msgbus_priority: 10, // High-priority for built-in component
            config,
            #[cfg(feature = "defi")]
            pool_updaters: AHashMap::new(),
            #[cfg(feature = "defi")]
            pool_updaters_pending: AHashSet::new(),
            #[cfg(feature = "defi")]
            pool_snapshot_pending: AHashSet::new(),
            #[cfg(feature = "defi")]
            pool_event_buffers: AHashMap::new(),
        }
    }

    /// Returns a read-only reference to the engines clock.
    #[must_use]
    pub fn get_clock(&self) -> Ref<'_, dyn Clock> {
        self.clock.borrow()
    }

    /// Returns a read-only reference to the engines cache.
    #[must_use]
    pub fn get_cache(&self) -> Ref<'_, Cache> {
        self.cache.borrow()
    }

    /// Returns the `Rc<RefCell<Cache>>` used by this engine.
    #[must_use]
    pub fn cache_rc(&self) -> Rc<RefCell<Cache>> {
        Rc::clone(&self.cache)
    }

    /// Registers the `catalog` with the engine with an optional specific `name`.
    ///
    /// # Panics
    ///
    /// Panics if a catalog with the same `name` has already been registered.
    pub fn register_catalog(&mut self, catalog: ParquetDataCatalog, name: Option<String>) {
        let name = Ustr::from(&name.unwrap_or("catalog_0".to_string()));

        check_key_not_in_map(&name, &self.catalogs, "name", "catalogs").expect(FAILED);

        self.catalogs.insert(name, catalog);
        log::info!("Registered catalog <{name}>");
    }

    /// Registers the `client` with the engine with an optional venue `routing`.
    ///
    ///
    /// # Panics
    ///
    /// Panics if a client with the same client ID has already been registered.
    pub fn register_client(&mut self, client: DataClientAdapter, routing: Option<Venue>) {
        let client_id = client.client_id();

        if let Some(default_client) = &self.default_client {
            check_predicate_false(
                default_client.client_id() == client.client_id(),
                "client_id already registered as default client",
            )
            .expect(FAILED);
        }

        check_key_not_in_map(&client_id, &self.clients, "client_id", "clients").expect(FAILED);

        if let Some(routing) = routing {
            self.routing_map.insert(routing, client_id);
            log::info!("Set client {client_id} routing for {routing}");
        }

        if client.venue.is_none() && self.default_client.is_none() {
            self.default_client = Some(client);
            log::info!("Registered client {client_id} for default routing");
        } else {
            self.clients.insert(client_id, client);
            log::info!("Registered client {client_id}");
        }
    }

    /// Deregisters the client for the `client_id`.
    ///
    /// # Panics
    ///
    /// Panics if the client ID has not been registered.
    pub fn deregister_client(&mut self, client_id: &ClientId) {
        check_key_in_map(client_id, &self.clients, "client_id", "clients").expect(FAILED);

        self.clients.shift_remove(client_id);
        log::info!("Deregistered client {client_id}");
    }

    /// Registers the data `client` with the engine as the default routing client.
    ///
    /// When a specific venue routing cannot be found, this client will receive messages.
    ///
    /// # Warnings
    ///
    /// Any existing default routing client will be overwritten.
    ///
    /// # Panics
    ///
    /// Panics if a default client has already been registered.
    pub fn register_default_client(&mut self, client: DataClientAdapter) {
        check_predicate_true(
            self.default_client.is_none(),
            "default client already registered",
        )
        .expect(FAILED);

        let client_id = client.client_id();

        self.default_client = Some(client);
        log::info!("Registered default client {client_id}");
    }

    /// Starts all registered data clients.
    pub fn start(&mut self) {
        for client in self.get_clients_mut() {
            if let Err(e) = client.start() {
                log::error!("{e}");
            }
        }
    }

    /// Stops all registered data clients.
    pub fn stop(&mut self) {
        for client in self.get_clients_mut() {
            if let Err(e) = client.stop() {
                log::error!("{e}");
            }
        }
    }

    /// Resets all registered data clients to their initial state.
    pub fn reset(&mut self) {
        for client in self.get_clients_mut() {
            if let Err(e) = client.reset() {
                log::error!("{e}");
            }
        }
    }

    /// Disposes the engine, stopping all clients and canceling any timers.
    pub fn dispose(&mut self) {
        for client in self.get_clients_mut() {
            if let Err(e) = client.dispose() {
                log::error!("{e}");
            }
        }

        self.clock.borrow_mut().cancel_timers();
    }

    /// Returns `true` if all registered data clients are currently connected.
    #[must_use]
    pub fn check_connected(&self) -> bool {
        self.get_clients()
            .iter()
            .all(|client| client.is_connected())
    }

    /// Returns `true` if all registered data clients are currently disconnected.
    #[must_use]
    pub fn check_disconnected(&self) -> bool {
        self.get_clients()
            .iter()
            .all(|client| !client.is_connected())
    }

    /// Returns a list of all registered client IDs, including the default client if set.
    #[must_use]
    pub fn registered_clients(&self) -> Vec<ClientId> {
        self.get_clients()
            .into_iter()
            .map(|client| client.client_id())
            .collect()
    }

    // -- SUBSCRIPTIONS ---------------------------------------------------------------------------

    pub(crate) fn collect_subscriptions<F, T>(&self, get_subs: F) -> Vec<T>
    where
        F: Fn(&DataClientAdapter) -> &AHashSet<T>,
        T: Clone,
    {
        self.get_clients()
            .into_iter()
            .flat_map(get_subs)
            .cloned()
            .collect()
    }

    #[must_use]
    pub fn get_clients(&self) -> Vec<&DataClientAdapter> {
        let (default_opt, clients_map) = (&self.default_client, &self.clients);
        let mut clients: Vec<&DataClientAdapter> = clients_map.values().collect();

        if let Some(default) = default_opt {
            clients.push(default);
        }

        clients
    }

    #[must_use]
    pub fn get_clients_mut(&mut self) -> Vec<&mut DataClientAdapter> {
        let (default_opt, clients_map) = (&mut self.default_client, &mut self.clients);
        let mut clients: Vec<&mut DataClientAdapter> = clients_map.values_mut().collect();

        if let Some(default) = default_opt {
            clients.push(default);
        }

        clients
    }

    pub fn get_client(
        &mut self,
        client_id: Option<&ClientId>,
        venue: Option<&Venue>,
    ) -> Option<&mut DataClientAdapter> {
        if let Some(client_id) = client_id {
            // Explicit ID: first look in registered clients
            if let Some(client) = self.clients.get_mut(client_id) {
                return Some(client);
            }

            // Then check if it matches the default client
            if let Some(default) = self.default_client.as_mut()
                && default.client_id() == *client_id
            {
                return Some(default);
            }

            // Unknown explicit client
            return None;
        }

        if let Some(v) = venue {
            // Route by venue if mapped client still registered
            if let Some(client_id) = self.routing_map.get(v) {
                return self.clients.get_mut(client_id);
            }
        }

        // Fallback to default client
        self.get_default_client()
    }

    const fn get_default_client(&mut self) -> Option<&mut DataClientAdapter> {
        self.default_client.as_mut()
    }

    /// Returns all custom data types currently subscribed across all clients.
    #[must_use]
    pub fn subscribed_custom_data(&self) -> Vec<DataType> {
        self.collect_subscriptions(|client| &client.subscriptions_custom)
    }

    /// Returns all instrument IDs currently subscribed across all clients.
    #[must_use]
    pub fn subscribed_instruments(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_instrument)
    }

    /// Returns all instrument IDs for which book delta subscriptions exist.
    #[must_use]
    pub fn subscribed_book_deltas(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_book_deltas)
    }

    /// Returns all instrument IDs for which book snapshot subscriptions exist.
    #[must_use]
    pub fn subscribed_book_snapshots(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_book_snapshots)
    }

    /// Returns all instrument IDs for which quote subscriptions exist.
    #[must_use]
    pub fn subscribed_quotes(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_quotes)
    }

    /// Returns all instrument IDs for which trade subscriptions exist.
    #[must_use]
    pub fn subscribed_trades(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_trades)
    }

    /// Returns all bar types currently subscribed across all clients.
    #[must_use]
    pub fn subscribed_bars(&self) -> Vec<BarType> {
        self.collect_subscriptions(|client| &client.subscriptions_bars)
    }

    /// Returns all instrument IDs for which mark price subscriptions exist.
    #[must_use]
    pub fn subscribed_mark_prices(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_mark_prices)
    }

    /// Returns all instrument IDs for which index price subscriptions exist.
    #[must_use]
    pub fn subscribed_index_prices(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_index_prices)
    }

    /// Returns all instrument IDs for which funding rate subscriptions exist.
    #[must_use]
    pub fn subscribed_funding_rates(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_funding_rates)
    }

    /// Returns all instrument IDs for which status subscriptions exist.
    #[must_use]
    pub fn subscribed_instrument_status(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_instrument_status)
    }

    /// Returns all instrument IDs for which instrument close subscriptions exist.
    #[must_use]
    pub fn subscribed_instrument_close(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_instrument_close)
    }

    // -- COMMANDS --------------------------------------------------------------------------------

    /// Executes a `DataCommand` by delegating to subscribe, unsubscribe, or request handlers.
    ///
    /// Errors during execution are logged.
    pub fn execute(&mut self, cmd: &DataCommand) {
        if let Err(e) = match cmd {
            DataCommand::Subscribe(c) => self.execute_subscribe(c),
            DataCommand::Unsubscribe(c) => self.execute_unsubscribe(c),
            DataCommand::Request(c) => self.execute_request(c),
            #[cfg(feature = "defi")]
            DataCommand::DefiRequest(c) => self.execute_defi_request(c),
            #[cfg(feature = "defi")]
            DataCommand::DefiSubscribe(c) => self.execute_defi_subscribe(c),
            #[cfg(feature = "defi")]
            DataCommand::DefiUnsubscribe(c) => self.execute_defi_unsubscribe(c),
            _ => {
                log::warn!("Unhandled DataCommand variant: {cmd:?}");
                Ok(())
            }
        } {
            log::error!("{e}");
        }
    }

    /// Handles a subscribe command, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription is invalid (e.g., synthetic instrument for book data),
    /// or if the underlying client operation fails.
    pub fn execute_subscribe(&mut self, cmd: &SubscribeCommand) -> anyhow::Result<()> {
        // Update internal engine state
        match &cmd {
            SubscribeCommand::BookDeltas(cmd) => self.subscribe_book_deltas(cmd)?,
            SubscribeCommand::BookDepth10(cmd) => self.subscribe_book_depth10(cmd)?,
            SubscribeCommand::BookSnapshots(cmd) => self.subscribe_book_snapshots(cmd)?,
            SubscribeCommand::Bars(cmd) => self.subscribe_bars(cmd)?,
            _ => {} // Do nothing else
        }

        if let Some(client_id) = cmd.client_id()
            && self.external_clients.contains(client_id)
        {
            if self.config.debug {
                log::debug!("Skipping subscribe command for external client {client_id}: {cmd:?}",);
            }
            return Ok(());
        }

        if let Some(client) = self.get_client(cmd.client_id(), cmd.venue()) {
            client.execute_subscribe(cmd);
        } else {
            log::error!(
                "Cannot handle command: no client found for client_id={:?}, venue={:?}",
                cmd.client_id(),
                cmd.venue(),
            );
        }

        Ok(())
    }

    /// Handles an unsubscribe command, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client operation fails.
    pub fn execute_unsubscribe(&mut self, cmd: &UnsubscribeCommand) -> anyhow::Result<()> {
        match &cmd {
            UnsubscribeCommand::BookDeltas(cmd) => self.unsubscribe_book_deltas(cmd)?,
            UnsubscribeCommand::BookDepth10(cmd) => self.unsubscribe_book_depth10(cmd)?,
            UnsubscribeCommand::BookSnapshots(cmd) => self.unsubscribe_book_snapshots(cmd)?,
            UnsubscribeCommand::Bars(cmd) => self.unsubscribe_bars(cmd)?,
            _ => {} // Do nothing else
        }

        if let Some(client_id) = cmd.client_id()
            && self.external_clients.contains(client_id)
        {
            if self.config.debug {
                log::debug!(
                    "Skipping unsubscribe command for external client {client_id}: {cmd:?}",
                );
            }
            return Ok(());
        }

        if let Some(client) = self.get_client(cmd.client_id(), cmd.venue()) {
            client.execute_unsubscribe(cmd);
        } else {
            log::error!(
                "Cannot handle command: no client found for client_id={:?}, venue={:?}",
                cmd.client_id(),
                cmd.venue(),
            );
        }

        Ok(())
    }

    /// Sends a [`RequestCommand`] to a suitable data client implementation.
    ///
    /// # Errors
    ///
    /// Returns an error if no client is found for the given client ID or venue,
    /// or if the client fails to process the request.
    pub fn execute_request(&mut self, req: &RequestCommand) -> anyhow::Result<()> {
        // Skip requests for external clients
        if let Some(cid) = req.client_id()
            && self.external_clients.contains(cid)
        {
            if self.config.debug {
                log::debug!("Skipping data request for external client {cid}: {req:?}");
            }
            return Ok(());
        }
        if let Some(client) = self.get_client(req.client_id(), req.venue()) {
            match req {
                RequestCommand::Data(req) => client.request_data(req),
                RequestCommand::Instrument(req) => client.request_instrument(req),
                RequestCommand::Instruments(req) => client.request_instruments(req),
                RequestCommand::BookSnapshot(req) => client.request_book_snapshot(req),
                RequestCommand::BookDepth(req) => client.request_book_depth(req),
                RequestCommand::Quotes(req) => client.request_quotes(req),
                RequestCommand::Trades(req) => client.request_trades(req),
                RequestCommand::Bars(req) => client.request_bars(req),
            }
        } else {
            anyhow::bail!(
                "Cannot handle request: no client found for {:?} {:?}",
                req.client_id(),
                req.venue()
            );
        }
    }

    /// Processes a dynamically-typed data message.
    ///
    /// Currently supports `InstrumentAny` and `FundingRateUpdate`; unrecognized types are logged as errors.
    pub fn process(&mut self, data: &dyn Any) {
        // TODO: Eventually these could be added to the `Data` enum? process here for now
        if let Some(data) = data.downcast_ref::<Data>() {
            self.process_data(data.clone()); // TODO: Optimize (not necessary if we change handler)
            return;
        }

        #[cfg(feature = "defi")]
        if let Some(data) = data.downcast_ref::<DefiData>() {
            self.process_defi_data(data.clone()); // TODO: Optimize (not necessary if we change handler)
            return;
        }

        if let Some(instrument) = data.downcast_ref::<InstrumentAny>() {
            self.handle_instrument(instrument.clone());
        } else if let Some(funding_rate) = data.downcast_ref::<FundingRateUpdate>() {
            self.handle_funding_rate(*funding_rate);
        } else {
            log::error!("Cannot process data {data:?}, type is unrecognized");
        }
    }

    /// Processes a `Data` enum instance, dispatching to appropriate handlers.
    pub fn process_data(&mut self, data: Data) {
        match data {
            Data::Delta(delta) => self.handle_delta(delta),
            Data::Deltas(deltas) => self.handle_deltas(deltas.into_inner()),
            Data::Depth10(depth) => self.handle_depth10(*depth),
            Data::Quote(quote) => self.handle_quote(quote),
            Data::Trade(trade) => self.handle_trade(trade),
            Data::Bar(bar) => self.handle_bar(bar),
            Data::MarkPriceUpdate(mark_price) => self.handle_mark_price(mark_price),
            Data::IndexPriceUpdate(index_price) => self.handle_index_price(index_price),
            Data::InstrumentClose(close) => self.handle_instrument_close(close),
        }
    }

    /// Processes a `DataResponse`, handling and publishing the response message.
    pub fn response(&self, resp: DataResponse) {
        log::debug!("{RECV}{RES} {resp:?}");

        match &resp {
            DataResponse::Instrument(resp) => {
                self.handle_instrument_response(resp.data.clone());
            }
            DataResponse::Instruments(resp) => {
                self.handle_instruments(&resp.data);
            }
            DataResponse::Quotes(resp) => self.handle_quotes(&resp.data),
            DataResponse::Trades(resp) => self.handle_trades(&resp.data),
            DataResponse::Bars(resp) => self.handle_bars(&resp.data),
            _ => todo!(),
        }

        msgbus::send_response(resp.correlation_id(), &resp);
    }

    // -- DATA HANDLERS ---------------------------------------------------------------------------

    fn handle_instrument(&mut self, instrument: InstrumentAny) {
        if let Err(e) = self
            .cache
            .as_ref()
            .borrow_mut()
            .add_instrument(instrument.clone())
        {
            log_error_on_cache_insert(&e);
        }

        let topic = switchboard::get_instrument_topic(instrument.id());
        msgbus::publish(topic, &instrument as &dyn Any);
    }

    fn handle_delta(&mut self, delta: OrderBookDelta) {
        let deltas = if self.config.buffer_deltas {
            if let Some(buffered_deltas) = self.buffered_deltas_map.get_mut(&delta.instrument_id) {
                buffered_deltas.deltas.push(delta);
                buffered_deltas.flags = delta.flags;
                buffered_deltas.sequence = delta.sequence;
                buffered_deltas.ts_event = delta.ts_event;
                buffered_deltas.ts_init = delta.ts_init;
            } else {
                let buffered_deltas = OrderBookDeltas::new(delta.instrument_id, vec![delta]);
                self.buffered_deltas_map
                    .insert(delta.instrument_id, buffered_deltas);
            }

            if !RecordFlag::F_LAST.matches(delta.flags) {
                return; // Not the last delta for event
            }

            // SAFETY: We know the deltas exists already
            self.buffered_deltas_map
                .remove(&delta.instrument_id)
                .unwrap()
        } else {
            OrderBookDeltas::new(delta.instrument_id, vec![delta])
        };

        let topic = switchboard::get_book_deltas_topic(deltas.instrument_id);
        msgbus::publish(topic, &deltas as &dyn Any);
    }

    fn handle_deltas(&mut self, deltas: OrderBookDeltas) {
        let deltas = if self.config.buffer_deltas {
            let mut is_last_delta = false;
            for delta in &deltas.deltas {
                if RecordFlag::F_LAST.matches(delta.flags) {
                    is_last_delta = true;
                    break;
                }
            }

            let instrument_id = deltas.instrument_id;

            if let Some(buffered_deltas) = self.buffered_deltas_map.get_mut(&instrument_id) {
                buffered_deltas.deltas.extend(deltas.deltas);

                if let Some(last_delta) = buffered_deltas.deltas.last() {
                    buffered_deltas.flags = last_delta.flags;
                    buffered_deltas.sequence = last_delta.sequence;
                    buffered_deltas.ts_event = last_delta.ts_event;
                    buffered_deltas.ts_init = last_delta.ts_init;
                }
            } else {
                self.buffered_deltas_map.insert(instrument_id, deltas);
            }

            if !is_last_delta {
                return;
            }

            // SAFETY: We know the deltas exists already
            self.buffered_deltas_map.remove(&instrument_id).unwrap()
        } else {
            deltas
        };

        let topic = switchboard::get_book_deltas_topic(deltas.instrument_id);
        msgbus::publish(topic, &deltas as &dyn Any);
    }

    fn handle_depth10(&mut self, depth: OrderBookDepth10) {
        let topic = switchboard::get_book_depth10_topic(depth.instrument_id);
        msgbus::publish(topic, &depth as &dyn Any);
    }

    fn handle_quote(&mut self, quote: QuoteTick) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_quote(quote) {
            log_error_on_cache_insert(&e);
        }

        // TODO: Handle synthetics

        let topic = switchboard::get_quotes_topic(quote.instrument_id);
        msgbus::publish(topic, &quote as &dyn Any);
    }

    fn handle_trade(&mut self, trade: TradeTick) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_trade(trade) {
            log_error_on_cache_insert(&e);
        }

        // TODO: Handle synthetics

        let topic = switchboard::get_trades_topic(trade.instrument_id);
        msgbus::publish(topic, &trade as &dyn Any);
    }

    fn handle_bar(&mut self, bar: Bar) {
        // TODO: Handle additional bar logic
        if self.config.validate_data_sequence
            && let Some(last_bar) = self.cache.as_ref().borrow().bar(&bar.bar_type)
        {
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

        if let Err(e) = self.cache.as_ref().borrow_mut().add_bar(bar) {
            log_error_on_cache_insert(&e);
        }

        let topic = switchboard::get_bars_topic(bar.bar_type);
        msgbus::publish(topic, &bar as &dyn Any);
    }

    fn handle_mark_price(&mut self, mark_price: MarkPriceUpdate) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_mark_price(mark_price) {
            log_error_on_cache_insert(&e);
        }

        let topic = switchboard::get_mark_price_topic(mark_price.instrument_id);
        msgbus::publish(topic, &mark_price as &dyn Any);
    }

    fn handle_index_price(&mut self, index_price: IndexPriceUpdate) {
        if let Err(e) = self
            .cache
            .as_ref()
            .borrow_mut()
            .add_index_price(index_price)
        {
            log_error_on_cache_insert(&e);
        }

        let topic = switchboard::get_index_price_topic(index_price.instrument_id);
        msgbus::publish(topic, &index_price as &dyn Any);
    }

    /// Handles a funding rate update by adding it to the cache and publishing to the message bus.
    pub fn handle_funding_rate(&mut self, funding_rate: FundingRateUpdate) {
        if let Err(e) = self
            .cache
            .as_ref()
            .borrow_mut()
            .add_funding_rate(funding_rate)
        {
            log_error_on_cache_insert(&e);
        }

        let topic = switchboard::get_funding_rate_topic(funding_rate.instrument_id);
        msgbus::publish(topic, &funding_rate as &dyn Any);
    }

    fn handle_instrument_close(&mut self, close: InstrumentClose) {
        let topic = switchboard::get_instrument_close_topic(close.instrument_id);
        msgbus::publish(topic, &close as &dyn Any);
    }

    // -- SUBSCRIPTION HANDLERS -------------------------------------------------------------------

    fn subscribe_book_deltas(&mut self, cmd: &SubscribeBookDeltas) -> anyhow::Result<()> {
        if cmd.instrument_id.is_synthetic() {
            anyhow::bail!("Cannot subscribe for synthetic instrument `OrderBookDelta` data");
        }

        self.setup_book_updater(&cmd.instrument_id, cmd.book_type, true, cmd.managed)?;

        Ok(())
    }

    fn subscribe_book_depth10(&mut self, cmd: &SubscribeBookDepth10) -> anyhow::Result<()> {
        if cmd.instrument_id.is_synthetic() {
            anyhow::bail!("Cannot subscribe for synthetic instrument `OrderBookDepth10` data");
        }

        self.setup_book_updater(&cmd.instrument_id, cmd.book_type, false, cmd.managed)?;

        Ok(())
    }

    fn subscribe_book_snapshots(&mut self, cmd: &SubscribeBookSnapshots) -> anyhow::Result<()> {
        if self.subscribed_book_deltas().contains(&cmd.instrument_id) {
            return Ok(());
        }

        if cmd.instrument_id.is_synthetic() {
            anyhow::bail!("Cannot subscribe for synthetic instrument `OrderBookDelta` data");
        }

        // Track snapshot intervals per instrument, and set up timer on first subscription
        let first_for_interval = match self.book_intervals.entry(cmd.interval_ms) {
            Entry::Vacant(e) => {
                let mut set = AHashSet::new();
                set.insert(cmd.instrument_id);
                e.insert(set);
                true
            }
            Entry::Occupied(mut e) => {
                e.get_mut().insert(cmd.instrument_id);
                false
            }
        };

        if first_for_interval {
            // Initialize snapshotter and schedule its timer
            let interval_ns = millis_to_nanos(cmd.interval_ms.get() as f64);
            let topic = switchboard::get_book_snapshots_topic(cmd.instrument_id, cmd.interval_ms);

            let snap_info = BookSnapshotInfo {
                instrument_id: cmd.instrument_id,
                venue: cmd.instrument_id.venue,
                is_composite: cmd.instrument_id.symbol.is_composite(),
                root: Ustr::from(cmd.instrument_id.symbol.root()),
                topic,
                interval_ms: cmd.interval_ms,
            };

            // Schedule the first snapshot at the next interval boundary
            let now_ns = self.clock.borrow().timestamp_ns().as_u64();
            let start_time_ns = now_ns - (now_ns % interval_ns) + interval_ns;

            let snapshotter = Rc::new(BookSnapshotter::new(snap_info, self.cache.clone()));
            self.book_snapshotters
                .insert(cmd.instrument_id, snapshotter.clone());
            let timer_name = snapshotter.timer_name;

            let callback_fn: Rc<dyn Fn(TimeEvent)> =
                Rc::new(move |event| snapshotter.snapshot(event));
            let callback = TimeEventCallback::from(callback_fn);

            self.clock
                .borrow_mut()
                .set_timer_ns(
                    &timer_name,
                    interval_ns,
                    Some(start_time_ns.into()),
                    None,
                    Some(callback),
                    None,
                    None,
                )
                .expect(FAILED);
        }

        self.setup_book_updater(&cmd.instrument_id, cmd.book_type, false, true)?;

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
            // TODO: Unsubscribe from snapshots?
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
            // TODO: Unsubscribe from snapshots?
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

        // Remove instrument from interval tracking, and drop empty intervals
        let mut to_remove = Vec::new();
        for (interval, set) in &mut self.book_intervals {
            if set.remove(&cmd.instrument_id) && set.is_empty() {
                to_remove.push(*interval);
            }
        }

        for interval in to_remove {
            self.book_intervals.remove(&interval);
        }

        let topics = vec![
            switchboard::get_book_deltas_topic(cmd.instrument_id),
            switchboard::get_book_depth10_topic(cmd.instrument_id),
            // TODO: Unsubscribe from snapshots (add interval_ms to message?)
        ];

        self.maintain_book_updater(&cmd.instrument_id, &topics);
        self.maintain_book_snapshotter(&cmd.instrument_id);

        Ok(())
    }

    /// Unsubscribe internal bar aggregator for the given bar type.
    fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        // If we have an internal aggregator for this bar type, stop and remove it
        let bar_type = cmd.bar_type;
        if self.bar_aggregators.contains_key(&bar_type.standard()) {
            if let Err(e) = self.stop_bar_aggregator(bar_type) {
                log::error!("Error stopping bar aggregator for {bar_type}: {e}");
            }
            self.bar_aggregators.remove(&bar_type.standard());
            log::debug!("Removed bar aggregator for {bar_type}");
        }
        Ok(())
    }

    fn maintain_book_updater(&mut self, instrument_id: &InstrumentId, topics: &[MStr<Topic>]) {
        if let Some(updater) = self.book_updaters.get(instrument_id) {
            let handler = ShareableMessageHandler(updater.clone());

            // Unsubscribe handler if it is the last subscriber
            for topic in topics {
                if msgbus::subscriptions_count(topic.as_str()) == 1
                    && msgbus::is_subscribed(topic.as_str(), handler.clone())
                {
                    log::debug!("Unsubscribing BookUpdater from {topic}");
                    msgbus::unsubscribe_topic(*topic, handler.clone());
                }
            }

            // Check remaining subscriptions, if none then remove updater
            let still_subscribed = topics
                .iter()
                .any(|topic| msgbus::is_subscribed(topic.as_str(), handler.clone()));
            if !still_subscribed {
                self.book_updaters.remove(instrument_id);
                log::debug!("Removed BookUpdater for instrument ID {instrument_id}");
            }
        }
    }

    fn maintain_book_snapshotter(&mut self, instrument_id: &InstrumentId) {
        if let Some(snapshotter) = self.book_snapshotters.get(instrument_id) {
            let topic = switchboard::get_book_snapshots_topic(
                *instrument_id,
                snapshotter.snap_info.interval_ms,
            );

            // Check remaining snapshot subscriptions, if none then remove snapshotter
            if msgbus::subscriptions_count(topic.as_str()) == 0 {
                let timer_name = snapshotter.timer_name;
                self.book_snapshotters.remove(instrument_id);
                let mut clock = self.clock.borrow_mut();
                if clock.timer_exists(&timer_name) {
                    clock.cancel_timer(&timer_name);
                }
                log::debug!("Removed BookSnapshotter for instrument ID {instrument_id}");
            }
        }
    }

    // -- RESPONSE HANDLERS -----------------------------------------------------------------------

    fn handle_instrument_response(&self, instrument: InstrumentAny) {
        let mut cache = self.cache.as_ref().borrow_mut();
        if let Err(e) = cache.add_instrument(instrument) {
            log_error_on_cache_insert(&e);
        }
    }

    fn handle_instruments(&self, instruments: &[InstrumentAny]) {
        // TODO: Improve by adding bulk update methods to cache and database
        let mut cache = self.cache.as_ref().borrow_mut();
        for instrument in instruments {
            if let Err(e) = cache.add_instrument(instrument.clone()) {
                log_error_on_cache_insert(&e);
            }
        }
    }

    fn handle_quotes(&self, quotes: &[QuoteTick]) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_quotes(quotes) {
            log_error_on_cache_insert(&e);
        }
    }

    fn handle_trades(&self, trades: &[TradeTick]) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_trades(trades) {
            log_error_on_cache_insert(&e);
        }
    }

    fn handle_bars(&self, bars: &[Bar]) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_bars(bars) {
            log_error_on_cache_insert(&e);
        }
    }

    // -- INTERNAL --------------------------------------------------------------------------------

    #[allow(clippy::too_many_arguments)]
    fn setup_book_updater(
        &mut self,
        instrument_id: &InstrumentId,
        book_type: BookType,
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
        if !msgbus::is_subscribed(topic.as_str(), handler.clone()) {
            msgbus::subscribe(topic.into(), handler.clone(), Some(self.msgbus_priority));
        }

        let topic = switchboard::get_book_depth10_topic(*instrument_id);
        if !only_deltas && !msgbus::is_subscribed(topic.as_str(), handler.clone()) {
            msgbus::subscribe(topic.into(), handler, Some(self.msgbus_priority));
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
                log_error_on_cache_insert(&e);
            }

            let topic = switchboard::get_bars_topic(bar.bar_type);
            msgbus::publish(topic, &bar as &dyn Any);
        };

        let clock = self.clock.clone();
        let config = self.config.clone();

        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();

        if bar_type.spec().is_time_aggregated() {
            // Get time_bars_origin_offset from config
            let time_bars_origin_offset = config
                .time_bars_origins
                .get(&bar_type.spec().aggregation)
                .map(|duration| chrono::TimeDelta::from_std(*duration).unwrap_or_default());

            Box::new(TimeBarAggregator::new(
                bar_type,
                price_precision,
                size_precision,
                clock,
                handler,
                config.time_bars_build_with_no_updates,
                config.time_bars_timestamp_on_close,
                config.time_bars_interval_type,
                time_bars_origin_offset,
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
                )) as Box<dyn BarAggregator>,
                BarAggregation::Volume => Box::new(VolumeBarAggregator::new(
                    bar_type,
                    price_precision,
                    size_precision,
                    handler,
                )) as Box<dyn BarAggregator>,
                BarAggregation::Value => Box::new(ValueBarAggregator::new(
                    bar_type,
                    price_precision,
                    size_precision,
                    handler,
                )) as Box<dyn BarAggregator>,
                BarAggregation::Renko => Box::new(RenkoBarAggregator::new(
                    bar_type,
                    price_precision,
                    size_precision,
                    instrument.price_increment(),
                    handler,
                )) as Box<dyn BarAggregator>,
                _ => panic!(
                    "BarAggregation {:?} is not currently implemented. Supported aggregations: MILLISECOND, SECOND, MINUTE, HOUR, DAY, WEEK, MONTH, YEAR, TICK, VOLUME, VALUE, RENKO",
                    bar_type.spec().aggregation
                ),
            }
        }
    }

    fn start_bar_aggregator(&mut self, bar_type: BarType) -> anyhow::Result<()> {
        // Get the instrument for this bar type
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

        // Use standard form of bar type as key
        let bar_key = bar_type.standard();

        // Create or retrieve aggregator in Rc<RefCell>
        let aggregator = if let Some(rc) = self.bar_aggregators.get(&bar_key) {
            rc.clone()
        } else {
            let agg = self.create_bar_aggregator(&instrument, bar_type);
            let rc = Rc::new(RefCell::new(agg));
            self.bar_aggregators.insert(bar_key, rc.clone());
            rc
        };

        // Subscribe to underlying data topics
        let mut handlers = Vec::new();

        if bar_type.is_composite() {
            let topic = switchboard::get_bars_topic(bar_type.composite());
            let handler =
                ShareableMessageHandler(Rc::new(BarBarHandler::new(aggregator.clone(), bar_key)));

            if !msgbus::is_subscribed(topic.as_str(), handler.clone()) {
                msgbus::subscribe(topic.into(), handler.clone(), Some(self.msgbus_priority));
            }

            handlers.push((topic, handler));
        } else if bar_type.spec().price_type == PriceType::Last {
            let topic = switchboard::get_trades_topic(bar_type.instrument_id());
            let handler =
                ShareableMessageHandler(Rc::new(BarTradeHandler::new(aggregator.clone(), bar_key)));

            if !msgbus::is_subscribed(topic.as_str(), handler.clone()) {
                msgbus::subscribe(topic.into(), handler.clone(), Some(self.msgbus_priority));
            }

            handlers.push((topic, handler));
        } else {
            let topic = switchboard::get_quotes_topic(bar_type.instrument_id());
            let handler =
                ShareableMessageHandler(Rc::new(BarQuoteHandler::new(aggregator.clone(), bar_key)));

            if !msgbus::is_subscribed(topic.as_str(), handler.clone()) {
                msgbus::subscribe(topic.into(), handler.clone(), Some(self.msgbus_priority));
            }

            handlers.push((topic, handler));
        }

        self.bar_aggregator_handlers.insert(bar_key, handlers);
        aggregator.borrow_mut().set_is_running(true);

        Ok(())
    }

    fn stop_bar_aggregator(&mut self, bar_type: BarType) -> anyhow::Result<()> {
        let aggregator = self
            .bar_aggregators
            .remove(&bar_type.standard())
            .ok_or_else(|| {
                anyhow::anyhow!("Cannot stop bar aggregator: no aggregator to stop for {bar_type}")
            })?;

        aggregator.borrow_mut().stop();

        // Unsubscribe any registered message handlers
        let bar_key = bar_type.standard();
        if let Some(subs) = self.bar_aggregator_handlers.remove(&bar_key) {
            for (topic, handler) in subs {
                if msgbus::is_subscribed(topic.as_str(), handler.clone()) {
                    msgbus::unsubscribe_topic(topic, handler);
                }
            }
        }

        Ok(())
    }
}

#[inline(always)]
fn log_error_on_cache_insert<T: Display>(e: &T) {
    log::error!("Error on cache insert: {e}");
}
