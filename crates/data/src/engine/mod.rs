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

pub mod bar;
pub mod book;
mod commands;
pub mod config;
mod handlers;
mod requests;

#[cfg(feature = "defi")]
pub mod pool;

#[cfg(feature = "streaming")]
mod streaming;

use std::{
    any::{Any, type_name},
    cell::{Ref, RefCell},
    collections::VecDeque,
    fmt::{Debug, Display},
    num::NonZeroUsize,
    rc::Rc,
};

use ahash::{AHashMap, AHashSet};
pub use bar::BarAggregatorSubscription;
use bar::{BarAggregatorKey, bar_aggregator_key};
use book::{
    BookSnapshotInfo, BookSnapshotInfos, BookSnapshotKey, BookSnapshotUnsubscribeResult,
    BookSnapshotter, BookUpdater,
};
pub(crate) use commands::{DeferredCommand, DeferredCommandQueue};
use config::DataEngineConfig;
use futures::future::join_all;
use handlers::{
    BAR_AGGREGATOR_PRIORITY, BarBarHandler, BarQuoteHandler, BarTradeHandler, SpreadQuoteHandler,
};
use indexmap::IndexMap;
use nautilus_common::{
    cache::Cache,
    clock::Clock,
    logging::{RECV, RES},
    messages::data::{
        DataCommand, DataResponse, ForwardPricesResponse, RequestCommand, RequestForwardPrices,
        SubscribeBars, SubscribeBookDeltas, SubscribeBookDepth10, SubscribeBookSnapshots,
        SubscribeCommand, SubscribeOptionChain, SubscribeQuotes, UnsubscribeBars,
        UnsubscribeBookDeltas, UnsubscribeBookDepth10, UnsubscribeBookSnapshots,
        UnsubscribeCommand, UnsubscribeInstrumentStatus, UnsubscribeOptionChain,
        UnsubscribeOptionGreeks, UnsubscribeQuotes, is_parent_subscription,
    },
    msgbus::{
        self, ShareableMessageHandler, TypedHandler, TypedIntoHandler,
        switchboard::{self, MessagingSwitchboard},
    },
    runner::get_data_cmd_sender,
    timer::{TimeEvent, TimeEventCallback},
};
use nautilus_core::{
    Params, UUID4, WeakCell,
    correctness::{
        FAILED, check_key_in_map, check_key_not_in_map, check_predicate_false, check_predicate_true,
    },
    datetime::millis_to_nanos_unchecked,
};
#[cfg(feature = "defi")]
use nautilus_model::defi::DefiData;
use nautilus_model::{
    data::{
        Bar, BarType, CustomData, Data, DataType, FundingRateUpdate, IndexPriceUpdate,
        InstrumentClose, InstrumentStatus, MarkPriceUpdate, OrderBookDelta, OrderBookDeltas,
        OrderBookDepth10, QuoteTick, TradeTick,
        option_chain::{OptionGreeks, StrikeRange},
    },
    enums::{
        AggregationSource, BarAggregation, BookType, InstrumentClass, MarketStatusAction,
        OrderSide, PriceType, RecordFlag,
    },
    identifiers::{ClientId, InstrumentId, OptionSeriesId, Symbol, Venue},
    instruments::{Instrument, InstrumentAny, SyntheticInstrument},
    orderbook::OrderBook,
    types::{Price, Quantity},
};
use requests::{RequestBarAggregation, request_bar_aggregation_from_params, request_params};
#[cfg(feature = "streaming")]
use streaming::CatalogMap;
use ustr::Ustr;

#[cfg(feature = "defi")]
#[allow(unused_imports)] // Brings DeFi impl blocks into scope
use crate::defi::engine as _;
#[cfg(feature = "defi")]
use crate::engine::pool::PoolUpdater;
use crate::{
    aggregation::{
        BarAggregator, RenkoBarAggregator, SpreadQuoteAggregator, TickBarAggregator,
        TickImbalanceBarAggregator, TickRunsBarAggregator, TimeBarAggregator, ValueBarAggregator,
        ValueImbalanceBarAggregator, ValueRunsBarAggregator, VolumeBarAggregator,
        VolumeImbalanceBarAggregator, VolumeRunsBarAggregator,
    },
    client::DataClientAdapter,
    option_chains::OptionChainManager,
};

/// Provides a high-performance `DataEngine` for all environments.
#[derive(Debug)]
pub struct DataEngine {
    pub(crate) clock: Rc<RefCell<dyn Clock>>,
    pub(crate) cache: Rc<RefCell<Cache>>,
    pub(crate) external_clients: AHashSet<ClientId>,
    clients: IndexMap<ClientId, DataClientAdapter>,
    default_client: Option<DataClientAdapter>,
    routing_map: IndexMap<Venue, ClientId>,
    book_intervals: AHashMap<NonZeroUsize, BookSnapshotInfos>,
    book_snapshot_counts: IndexMap<BookSnapshotKey, usize>,
    book_deltas_subs: AHashSet<InstrumentId>,
    book_depth10_subs: AHashSet<InstrumentId>,
    book_updaters: AHashMap<InstrumentId, Rc<BookUpdater>>,
    book_deltas_parent_expansions: AHashMap<InstrumentId, Vec<InstrumentId>>,
    book_depth10_parent_expansions: AHashMap<InstrumentId, Vec<InstrumentId>>,
    book_snapshotters: AHashMap<NonZeroUsize, Rc<BookSnapshotter>>,
    bar_aggregators: IndexMap<BarAggregatorKey, Rc<RefCell<Box<dyn BarAggregator>>>>,
    bar_aggregator_handlers: AHashMap<BarAggregatorKey, Vec<BarAggregatorSubscription>>,
    request_bar_aggregations: AHashMap<UUID4, RequestBarAggregation>,
    spread_quote_aggregators: AHashMap<InstrumentId, Rc<RefCell<SpreadQuoteAggregator>>>,
    spread_quote_handlers: AHashMap<InstrumentId, Vec<(InstrumentId, TypedHandler<QuoteTick>)>>,
    option_chain_managers: AHashMap<OptionSeriesId, Rc<RefCell<OptionChainManager>>>,
    option_chain_instrument_index: AHashMap<InstrumentId, OptionSeriesId>,
    deferred_cmd_queue: DeferredCommandQueue,
    pending_option_chain_requests: AHashMap<UUID4, SubscribeOptionChain>,
    synthetic_quote_feeds: AHashMap<InstrumentId, Vec<SyntheticInstrument>>,
    synthetic_trade_feeds: AHashMap<InstrumentId, Vec<SyntheticInstrument>>,
    subscribed_synthetic_quotes: AHashSet<InstrumentId>,
    subscribed_synthetic_trades: AHashSet<InstrumentId>,
    buffered_deltas_map: AHashMap<InstrumentId, OrderBookDeltas>,
    command_count: u64,
    data_count: u64,
    request_count: u64,
    response_count: u64,
    pub(crate) msgbus_priority: u32,
    pub(crate) config: DataEngineConfig,
    #[cfg(feature = "streaming")]
    catalogs: CatalogMap,
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
            routing_map: IndexMap::new(),
            book_intervals: AHashMap::new(),
            book_snapshot_counts: IndexMap::new(),
            book_deltas_subs: AHashSet::new(),
            book_depth10_subs: AHashSet::new(),
            book_updaters: AHashMap::new(),
            book_deltas_parent_expansions: AHashMap::new(),
            book_depth10_parent_expansions: AHashMap::new(),
            book_snapshotters: AHashMap::new(),
            bar_aggregators: IndexMap::new(),
            bar_aggregator_handlers: AHashMap::new(),
            request_bar_aggregations: AHashMap::new(),
            spread_quote_aggregators: AHashMap::new(),
            spread_quote_handlers: AHashMap::new(),
            option_chain_managers: AHashMap::new(),
            option_chain_instrument_index: AHashMap::new(),
            deferred_cmd_queue: Rc::new(RefCell::new(VecDeque::new())),
            pending_option_chain_requests: AHashMap::new(),
            synthetic_quote_feeds: AHashMap::new(),
            synthetic_trade_feeds: AHashMap::new(),
            subscribed_synthetic_quotes: AHashSet::new(),
            subscribed_synthetic_trades: AHashSet::new(),
            buffered_deltas_map: AHashMap::new(),
            command_count: 0,
            data_count: 0,
            request_count: 0,
            response_count: 0,
            msgbus_priority: 10, // High-priority for built-in component
            config,
            #[cfg(feature = "streaming")]
            catalogs: CatalogMap::new(),
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

    /// Registers all message bus handlers for the data engine.
    pub fn register_msgbus_handlers(engine: &Rc<RefCell<Self>>) {
        let weak = WeakCell::from(Rc::downgrade(engine));

        let weak1 = weak.clone();
        msgbus::register_data_command_endpoint(
            MessagingSwitchboard::data_engine_execute(),
            TypedIntoHandler::from(move |cmd: DataCommand| {
                if let Some(rc) = weak1.upgrade() {
                    rc.borrow_mut().execute(cmd);
                }
            }),
        );

        msgbus::register_data_command_endpoint(
            MessagingSwitchboard::data_engine_queue_execute(),
            TypedIntoHandler::from(move |cmd: DataCommand| {
                get_data_cmd_sender().clone().execute(cmd);
            }),
        );

        // Register process handler (polymorphic - uses Any)
        let weak2 = weak.clone();
        msgbus::register_any(
            MessagingSwitchboard::data_engine_process(),
            ShareableMessageHandler::from_any(move |data: &dyn Any| {
                if let Some(rc) = weak2.upgrade() {
                    rc.borrow_mut().process(data);
                }
            }),
        );

        // Register process_data handler (typed - takes ownership)
        let weak3 = weak.clone();
        msgbus::register_data_endpoint(
            MessagingSwitchboard::data_engine_process_data(),
            TypedIntoHandler::from(move |data: Data| {
                if let Some(rc) = weak3.upgrade() {
                    rc.borrow_mut().process_data(data);
                }
            }),
        );

        // Register process_defi_data handler (typed - takes ownership)
        #[cfg(feature = "defi")]
        {
            let weak4 = weak.clone();
            msgbus::register_defi_data_endpoint(
                MessagingSwitchboard::data_engine_process_defi_data(),
                TypedIntoHandler::from(move |data: DefiData| {
                    if let Some(rc) = weak4.upgrade() {
                        rc.borrow_mut().process_defi_data(data);
                    }
                }),
            );
        }

        let weak5 = weak;
        msgbus::register_data_response_endpoint(
            MessagingSwitchboard::data_engine_response(),
            TypedIntoHandler::from(move |resp: DataResponse| {
                if let Some(rc) = weak5.upgrade() {
                    rc.borrow_mut().response(resp);
                }
            }),
        );
    }

    /// Returns the total count of data commands received by the engine.
    #[must_use]
    pub const fn command_count(&self) -> u64 {
        self.command_count
    }

    /// Returns the total count of data stream objects received by the engine.
    #[must_use]
    pub const fn data_count(&self) -> u64 {
        self.data_count
    }

    #[cfg(feature = "defi")]
    pub(crate) const fn increment_data_count(&mut self) {
        self.data_count += 1;
    }

    /// Returns the total count of data requests received by the engine.
    #[must_use]
    pub const fn request_count(&self) -> u64 {
        self.request_count
    }

    /// Returns the total count of data responses received by the engine.
    #[must_use]
    pub const fn response_count(&self) -> u64 {
        self.response_count
    }

    /// Returns whether an `OptionChainManager` exists for the given series.
    #[must_use]
    pub fn has_option_chain_manager(&self, series_id: &OptionSeriesId) -> bool {
        self.option_chain_managers.contains_key(series_id)
    }

    /// Returns the count of pending option-chain bootstrap requests.
    #[must_use]
    pub fn pending_option_chain_request_count(&self) -> usize {
        self.pending_option_chain_requests.len()
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
            log::debug!("Set client {client_id} routing for {routing}");
        }

        if client.venue.is_none() && self.default_client.is_none() {
            self.default_client = Some(client);
            log::debug!("Registered client {client_id} for default routing");
        } else {
            self.clients.insert(client_id, client);
            log::debug!("Registered client {client_id}");
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
        log::debug!("Registered default client {client_id}");
    }

    /// Starts all registered data clients and re-arms bar aggregator timers.
    pub fn start(&mut self) {
        for client in self.get_clients_mut() {
            if let Err(e) = client.start() {
                log::error!("{e}");
            }
        }

        for aggregator in self.bar_aggregators.values() {
            if aggregator.borrow().bar_type().spec().is_time_aggregated() {
                aggregator
                    .borrow_mut()
                    .start_timer(Some(aggregator.clone()));
            }
        }

        for aggregator in self.spread_quote_aggregators.values() {
            aggregator
                .borrow_mut()
                .start_timer(Some(aggregator.clone()));
        }
    }

    /// Stops all registered data clients and bar aggregator timers.
    pub fn stop(&mut self) {
        for client in self.get_clients_mut() {
            if let Err(e) = client.stop() {
                log::error!("{e}");
            }
        }

        for aggregator in self.bar_aggregators.values() {
            aggregator.borrow_mut().stop();
        }

        for aggregator in self.spread_quote_aggregators.values() {
            aggregator.borrow_mut().stop_timer();
        }
    }

    /// Resets all registered data clients and clears engine state.
    pub fn reset(&mut self) {
        for client in self.get_clients_mut() {
            if let Err(e) = client.reset() {
                log::error!("{e}");
            }
        }

        let keys: Vec<BarAggregatorKey> = self.bar_aggregators.keys().copied().collect();
        for (bar_type, request_id) in keys {
            if let Err(e) = self.stop_bar_aggregator(bar_type, request_id) {
                log::error!("Error stopping bar aggregator during reset for {bar_type}: {e}");
            }
        }

        self.request_bar_aggregations.clear();

        let spread_ids: Vec<InstrumentId> = self.spread_quote_aggregators.keys().copied().collect();
        for spread_id in spread_ids {
            self.stop_spread_quote_aggregator(spread_id);
        }

        // Tear down option chain managers to unregister their msgbus handlers
        let managers: Vec<_> = self.option_chain_managers.drain().collect();
        for (_, manager) in managers {
            manager.borrow_mut().teardown(&self.clock);
        }

        self.option_chain_instrument_index.clear();
        self.pending_option_chain_requests.clear();

        // Unsubscribe BookUpdaters before dropping; otherwise the typed router
        // keeps dispatching to abandoned updaters. `book_updaters` is keyed by
        // per-underlying id, so the literal per-underlying topic is the same
        // string the subscribe path used.
        let book_updaters: Vec<(InstrumentId, Rc<BookUpdater>)> =
            self.book_updaters.drain().collect();
        for (instrument_id, updater) in book_updaters {
            let deltas_topic = switchboard::get_book_deltas_topic(instrument_id);
            let depth_topic = switchboard::get_book_depth10_topic(instrument_id);
            let deltas_handler: TypedHandler<OrderBookDeltas> = TypedHandler::new(updater.clone());
            let depth_handler: TypedHandler<OrderBookDepth10> = TypedHandler::new(updater);
            msgbus::unsubscribe_book_deltas(deltas_topic.into(), &deltas_handler);
            msgbus::unsubscribe_book_depth10(depth_topic.into(), &depth_handler);
        }

        self.book_deltas_parent_expansions.clear();
        self.book_depth10_parent_expansions.clear();

        self.book_deltas_subs.clear();
        self.book_depth10_subs.clear();
        self.book_intervals.clear();
        self.book_snapshot_counts.clear();
        self.book_snapshotters.clear();
        self.buffered_deltas_map.clear();

        self.synthetic_quote_feeds.clear();
        self.synthetic_trade_feeds.clear();
        self.subscribed_synthetic_quotes.clear();
        self.subscribed_synthetic_trades.clear();

        self.deferred_cmd_queue.borrow_mut().clear();

        self.clock.borrow_mut().cancel_timers();

        self.command_count = 0;
        self.data_count = 0;
        self.request_count = 0;
        self.response_count = 0;
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

    /// Connects all registered data clients concurrently.
    ///
    /// Connection failures are logged but do not prevent the node from running.
    pub async fn connect(&mut self) {
        let futures: Vec<_> = self
            .get_clients_mut()
            .into_iter()
            .map(DataClientAdapter::connect)
            .collect();

        let results = join_all(futures).await;

        for error in results.into_iter().filter_map(Result::err) {
            log::error!("Failed to connect data client: {error}");
        }
    }

    /// Disconnects all registered data clients concurrently.
    ///
    /// # Errors
    ///
    /// Returns an error if any client fails to disconnect.
    pub async fn disconnect(&mut self) -> anyhow::Result<()> {
        let futures: Vec<_> = self
            .get_clients_mut()
            .into_iter()
            .map(DataClientAdapter::disconnect)
            .collect();

        let results = join_all(futures).await;
        let errors: Vec<_> = results.into_iter().filter_map(Result::err).collect();

        if errors.is_empty() {
            Ok(())
        } else {
            let error_msgs: Vec<_> = errors.iter().map(ToString::to_string).collect();
            anyhow::bail!(
                "Failed to disconnect data clients: {}",
                error_msgs.join("; ")
            )
        }
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

    /// Returns connection status for each registered client.
    #[must_use]
    pub fn client_connection_status(&self) -> Vec<(ClientId, bool)> {
        self.get_clients()
            .into_iter()
            .map(|client| (client.client_id(), client.is_connected()))
            .collect()
    }

    /// Returns a list of all registered client IDs, including the default client if set.
    #[must_use]
    pub fn registered_clients(&self) -> Vec<ClientId> {
        self.get_clients()
            .into_iter()
            .map(|client| client.client_id())
            .collect()
    }

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

    /// Resolves the client for a subscribe/unsubscribe command.
    ///
    /// When `BACKTEST` is registered, all commands route through it regardless of
    /// the command's `client_id` or `venue`. Request paths skip this override.
    fn get_command_client(
        &mut self,
        client_id: Option<&ClientId>,
        venue: Option<&Venue>,
    ) -> Option<&mut DataClientAdapter> {
        let backtest_id = ClientId::new("BACKTEST");
        // BACKTEST may live in `clients` or as the default (venue=None branch in
        // `register_client`)
        if self.clients.contains_key(&backtest_id) {
            return self.clients.get_mut(&backtest_id);
        }
        let default_is_backtest = self
            .default_client
            .as_ref()
            .is_some_and(|c| c.client_id() == backtest_id);
        if default_is_backtest {
            return self.default_client.as_mut();
        }
        self.get_client(client_id, venue)
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

    /// Returns all instrument IDs for which book depth10 subscriptions exist.
    #[must_use]
    pub fn subscribed_book_depth10(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_book_depth10)
    }

    /// Returns all instrument IDs for which book snapshot subscriptions exist.
    #[must_use]
    pub fn subscribed_book_snapshots(&self) -> Vec<InstrumentId> {
        self.book_snapshot_counts
            .keys()
            .map(|(instrument_id, _)| *instrument_id)
            .collect()
    }

    /// Returns all instrument IDs for which quote subscriptions exist.
    #[must_use]
    pub fn subscribed_quotes(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_quotes)
    }

    /// Returns all synthetic instrument IDs for which quote subscriptions exist.
    #[must_use]
    pub fn subscribed_synthetic_quotes(&self) -> Vec<InstrumentId> {
        self.subscribed_synthetic_quotes.iter().copied().collect()
    }

    /// Returns all instrument IDs for which trade subscriptions exist.
    #[must_use]
    pub fn subscribed_trades(&self) -> Vec<InstrumentId> {
        self.collect_subscriptions(|client| &client.subscriptions_trades)
    }

    /// Returns all synthetic instrument IDs for which trade subscriptions exist.
    #[must_use]
    pub fn subscribed_synthetic_trades(&self) -> Vec<InstrumentId> {
        self.subscribed_synthetic_trades.iter().copied().collect()
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

    /// Executes a `DataCommand` by delegating to subscribe, unsubscribe, or request handlers.
    ///
    /// Errors during execution are logged.
    pub fn execute(&mut self, cmd: DataCommand) {
        match &cmd {
            DataCommand::Subscribe(_) | DataCommand::Unsubscribe(_) => self.command_count += 1,
            DataCommand::Request(_) => self.request_count += 1,
            #[cfg(feature = "defi")]
            DataCommand::DefiRequest(_) => self.request_count += 1,
            #[cfg(feature = "defi")]
            DataCommand::DefiSubscribe(_) | DataCommand::DefiUnsubscribe(_) => {
                self.command_count += 1;
            }
            _ => {}
        }

        if let Err(e) = match cmd {
            DataCommand::Subscribe(c) => self.execute_subscribe(c),
            DataCommand::Unsubscribe(c) => self.execute_unsubscribe(&c),
            DataCommand::Request(c) => self.execute_request(c),
            #[cfg(feature = "defi")]
            DataCommand::DefiRequest(c) => self.execute_defi_request(c),
            #[cfg(feature = "defi")]
            DataCommand::DefiSubscribe(c) => self.execute_defi_subscribe(c),
            #[cfg(feature = "defi")]
            DataCommand::DefiUnsubscribe(c) => self.execute_defi_unsubscribe(&c),
            _ => {
                log::warn!("Unhandled DataCommand variant");
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
    pub fn execute_subscribe(&mut self, cmd: SubscribeCommand) -> anyhow::Result<()> {
        // Update internal engine state
        match &cmd {
            SubscribeCommand::BookDeltas(cmd) => self.subscribe_book_deltas(cmd)?,
            SubscribeCommand::BookDepth10(cmd) => self.subscribe_book_depth10(cmd)?,
            SubscribeCommand::BookSnapshots(cmd) => {
                // Handles client forwarding internally (forwards as BookDeltas)
                return self.subscribe_book_snapshots(cmd);
            }
            SubscribeCommand::Bars(cmd) => self.subscribe_bars(cmd)?,
            SubscribeCommand::OptionChain(cmd) => {
                self.subscribe_option_chain(cmd);
                return Ok(());
            }
            SubscribeCommand::Quotes(cmd) if cmd.instrument_id.is_synthetic() => {
                self.subscribe_synthetic_quotes(cmd.instrument_id);
                return Ok(());
            }
            SubscribeCommand::Quotes(cmd)
                if self.is_spread_quote_command(cmd.instrument_id, cmd.params.as_ref()) =>
            {
                self.subscribe_spread_quotes(cmd);
                return Ok(());
            }
            SubscribeCommand::Trades(cmd) if cmd.instrument_id.is_synthetic() => {
                self.subscribe_synthetic_trades(cmd.instrument_id);
                return Ok(());
            }
            SubscribeCommand::Instrument(cmd) if cmd.instrument_id.is_synthetic() => {
                anyhow::bail!("Cannot subscribe for synthetic instrument `Instrument` data");
            }
            SubscribeCommand::InstrumentStatus(cmd) if cmd.instrument_id.is_synthetic() => {
                anyhow::bail!("Cannot subscribe for synthetic instrument `InstrumentStatus` data");
            }
            SubscribeCommand::InstrumentClose(cmd) if cmd.instrument_id.is_synthetic() => {
                anyhow::bail!("Cannot subscribe for synthetic instrument `InstrumentClose` data");
            }
            SubscribeCommand::OptionGreeks(cmd) if cmd.instrument_id.is_synthetic() => {
                anyhow::bail!("Cannot subscribe for synthetic instrument `OptionGreeks` data");
            }
            _ => {} // Do nothing else
        }

        if let Some(client_id) = cmd.client_id()
            && self.external_clients.contains(client_id)
        {
            if self.config.debug {
                log::debug!("Skipping subscribe command for external client {client_id}: {cmd:?}");
            }
            return Ok(());
        }

        #[cfg(feature = "streaming")]
        let cmd = self.subscribe_command_with_prefilled_start_ns(cmd)?;

        if let Some(client) = self.get_command_client(cmd.client_id(), cmd.venue()) {
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
            UnsubscribeCommand::BookDeltas(cmd) if !self.unsubscribe_book_deltas(cmd) => {
                return Ok(());
            }
            UnsubscribeCommand::BookDepth10(cmd) if !self.unsubscribe_book_depth10(cmd) => {
                return Ok(());
            }
            UnsubscribeCommand::BookSnapshots(cmd) => {
                // Handles client forwarding internally (forwards as BookDeltas)
                self.unsubscribe_book_snapshots(cmd);
                return Ok(());
            }
            UnsubscribeCommand::Bars(cmd) => self.unsubscribe_bars(cmd),
            UnsubscribeCommand::OptionChain(cmd) => {
                self.unsubscribe_option_chain(cmd);
                return Ok(());
            }
            UnsubscribeCommand::Quotes(cmd) if cmd.instrument_id.is_synthetic() => {
                self.unsubscribe_synthetic_quotes(cmd.instrument_id);
                return Ok(());
            }
            UnsubscribeCommand::Quotes(cmd)
                if self.is_spread_quote_command(cmd.instrument_id, cmd.params.as_ref()) =>
            {
                self.unsubscribe_spread_quotes(cmd);
                return Ok(());
            }
            UnsubscribeCommand::Trades(cmd) if cmd.instrument_id.is_synthetic() => {
                self.unsubscribe_synthetic_trades(cmd.instrument_id);
                return Ok(());
            }
            UnsubscribeCommand::Instrument(cmd) if cmd.instrument_id.is_synthetic() => {
                anyhow::bail!("Cannot unsubscribe from synthetic instrument `Instrument` data");
            }
            UnsubscribeCommand::InstrumentStatus(cmd) if cmd.instrument_id.is_synthetic() => {
                anyhow::bail!(
                    "Cannot unsubscribe from synthetic instrument `InstrumentStatus` data"
                );
            }
            UnsubscribeCommand::InstrumentClose(cmd) if cmd.instrument_id.is_synthetic() => {
                anyhow::bail!(
                    "Cannot unsubscribe from synthetic instrument `InstrumentClose` data"
                );
            }
            UnsubscribeCommand::OptionGreeks(cmd) if cmd.instrument_id.is_synthetic() => {
                anyhow::bail!("Cannot unsubscribe from synthetic instrument `OptionGreeks` data");
            }
            _ => {}
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

        // Keep client subscribed while exact-topic subscribers remain
        if Self::topic_has_remaining_subscribers(cmd) {
            return Ok(());
        }

        if let Some(client) = self.get_command_client(cmd.client_id(), cmd.venue()) {
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

    fn topic_has_remaining_subscribers(cmd: &UnsubscribeCommand) -> bool {
        // Exact match only; wildcard observers must not block venue detach.
        // BookDeltas/Depth10 excluded: binary engine state cannot distinguish
        // the internal BookUpdater handler from external subscribers
        match cmd {
            UnsubscribeCommand::Quotes(c) => {
                let topic = switchboard::get_quotes_topic(c.instrument_id);
                msgbus::exact_subscriber_count_quotes(topic) > 0
            }
            UnsubscribeCommand::Trades(c) => {
                let topic = switchboard::get_trades_topic(c.instrument_id);
                msgbus::exact_subscriber_count_trades(topic) > 0
            }
            UnsubscribeCommand::MarkPrices(c) => {
                let topic = switchboard::get_mark_price_topic(c.instrument_id);
                msgbus::exact_subscriber_count_mark_prices(topic) > 0
            }
            UnsubscribeCommand::IndexPrices(c) => {
                let topic = switchboard::get_index_price_topic(c.instrument_id);
                msgbus::exact_subscriber_count_index_prices(topic) > 0
            }
            UnsubscribeCommand::FundingRates(c) => {
                let topic = switchboard::get_funding_rate_topic(c.instrument_id);
                msgbus::exact_subscriber_count_funding_rates(topic) > 0
            }
            UnsubscribeCommand::OptionGreeks(c) => {
                let topic = switchboard::get_option_greeks_topic(c.instrument_id);
                msgbus::exact_subscriber_count_option_greeks(topic) > 0
            }
            _ => false,
        }
    }

    /// Sends a [`RequestCommand`] to a suitable data client implementation.
    ///
    /// # Errors
    ///
    /// Returns an error if no client is found for the given client ID or venue,
    /// or if the client fails to process the request.
    pub fn execute_request(&mut self, req: RequestCommand) -> anyhow::Result<()> {
        // Skip requests for external clients
        if let Some(cid) = req.client_id()
            && self.external_clients.contains(cid)
        {
            if self.config.debug {
                log::debug!("Skipping data request for external client {cid}: {req:?}");
            }
            return Ok(());
        }

        let request_id = *req.request_id();
        self.prepare_request_bar_aggregators(&req)?;

        let result = if let Some(client) = self.get_client(req.client_id(), req.venue()) {
            match req {
                RequestCommand::Data(req) => client.request_data(req),
                RequestCommand::Instrument(req) => client.request_instrument(req),
                RequestCommand::Instruments(req) => client.request_instruments(req),
                RequestCommand::BookSnapshot(req) => client.request_book_snapshot(req),
                RequestCommand::BookDepth(req) => client.request_book_depth(req),
                RequestCommand::Quotes(req) => client.request_quotes(req),
                RequestCommand::Trades(req) => client.request_trades(req),
                RequestCommand::FundingRates(req) => client.request_funding_rates(req),
                RequestCommand::ForwardPrices(req) => client.request_forward_prices(req),
                RequestCommand::Bars(req) => client.request_bars(req),
            }
        } else {
            Err(anyhow::anyhow!(
                "Cannot handle request: no client found for {:?} {:?}",
                req.client_id(),
                req.venue()
            ))
        };

        if result.is_err() {
            self.cleanup_request_bar_aggregators(&request_id);
        }

        result
    }

    fn prepare_request_bar_aggregators(&mut self, req: &RequestCommand) -> anyhow::Result<()> {
        let request_id = *req.request_id();
        let Some(state) = request_bar_aggregation_from_params(request_params(req))? else {
            return Ok(());
        };

        if !self.can_start_request_bar_aggregators(request_id, &state) {
            anyhow::bail!(
                "Cannot request aggregated bars: one of the aggregators in `bar_types` is already running"
            );
        }

        self.request_bar_aggregations
            .insert(request_id, state.clone());

        if let Err(e) = self.init_request_bar_aggregators(request_id, &state) {
            self.cleanup_request_bar_aggregators(&request_id);
            return Err(e);
        }

        Ok(())
    }

    fn can_start_request_bar_aggregators(
        &self,
        request_id: UUID4,
        state: &RequestBarAggregation,
    ) -> bool {
        let aggregator_request_id = state.aggregator_request_id(request_id);
        state.bar_types.iter().all(|bar_type| {
            let key = bar_aggregator_key(*bar_type, aggregator_request_id);
            self.bar_aggregators
                .get(&key)
                .is_none_or(|aggregator| !aggregator.borrow().is_running())
        })
    }

    fn init_request_bar_aggregators(
        &mut self,
        request_id: UUID4,
        state: &RequestBarAggregation,
    ) -> anyhow::Result<()> {
        let aggregator_request_id = state.aggregator_request_id(request_id);

        for bar_type in &state.bar_types {
            self.create_bar_aggregator_for_key(*bar_type, aggregator_request_id)?;
            self.setup_bar_aggregator(*bar_type, true, aggregator_request_id)?;

            let key = bar_aggregator_key(*bar_type, aggregator_request_id);
            if let Some(aggregator) = self.bar_aggregators.get(&key) {
                aggregator.borrow_mut().set_is_running(true);
            }
        }

        Ok(())
    }

    fn cleanup_request_bar_aggregators(&mut self, request_id: &UUID4) -> bool {
        let Some(state) = self.request_bar_aggregations.remove(request_id) else {
            return false;
        };
        let aggregator_request_id = state.aggregator_request_id(*request_id);

        for bar_type in state.bar_types {
            let key = bar_aggregator_key(bar_type, aggregator_request_id);
            let has_live_handlers =
                state.update_subscriptions && self.bar_aggregator_handlers.contains_key(&key);
            let keep_running = if has_live_handlers {
                match self.setup_bar_aggregator(bar_type, false, aggregator_request_id) {
                    Ok(()) => true,
                    Err(e) => {
                        log::error!(
                            "Error starting live request bar aggregator for {bar_type}: {e}"
                        );
                        false
                    }
                }
            } else {
                false
            };

            if let Some(aggregator) = self.bar_aggregators.get(&key) {
                aggregator.borrow_mut().set_is_running(keep_running);
            }

            if !state.update_subscriptions
                && let Err(e) = self.stop_bar_aggregator(bar_type, aggregator_request_id)
            {
                log::error!("Error stopping request bar aggregator for {bar_type}: {e}");
            }
        }

        true
    }

    /// Processes a dynamically-typed data message.
    ///
    /// Currently supports `InstrumentAny`, funding rates, instrument status, option greeks, and
    /// custom data; unrecognized types are logged as errors.
    pub fn process(&mut self, data: &dyn Any) {
        self.data_count += 1;
        // TODO: Eventually these can be added to the `Data` enum (C/Cython blocking), process here for now
        if let Some(instrument) = data.downcast_ref::<InstrumentAny>() {
            self.handle_instrument(instrument);
        } else if let Some(funding_rate) = data.downcast_ref::<FundingRateUpdate>() {
            self.handle_funding_rate(*funding_rate);
        } else if let Some(status) = data.downcast_ref::<InstrumentStatus>() {
            self.handle_instrument_status(*status);
        } else if let Some(option_greeks) = data.downcast_ref::<OptionGreeks>() {
            self.cache.borrow_mut().add_option_greeks(*option_greeks);
            let topic = switchboard::get_option_greeks_topic(option_greeks.instrument_id);
            msgbus::publish_option_greeks(topic, option_greeks);
            self.drain_deferred_commands();
        } else if let Some(custom) = data.downcast_ref::<CustomData>() {
            self.handle_custom_data(custom);
        } else {
            log::error!("Cannot process data {data:?}, type is unrecognized");
        }
    }

    /// Processes a `Data` enum instance, dispatching to live handlers.
    pub fn process_data(&mut self, data: Data) {
        self.data_count += 1;

        match data {
            Data::Delta(delta) => self.handle_delta(delta),
            Data::Deltas(deltas) => self.handle_deltas(deltas.into_inner()),
            Data::Depth10(depth) => self.handle_depth10(*depth),
            Data::Quote(quote) => {
                self.handle_quote(quote);
                self.drain_deferred_commands();
            }
            Data::Trade(trade) => self.handle_trade(trade),
            Data::Bar(bar) => self.handle_bar(bar),
            Data::MarkPriceUpdate(mark_price) => {
                self.handle_mark_price(mark_price);
                self.drain_deferred_commands();
            }
            Data::IndexPriceUpdate(index_price) => {
                self.handle_index_price(index_price);
                self.drain_deferred_commands();
            }
            Data::InstrumentStatus(status) => {
                self.handle_instrument_status(status);
                self.drain_deferred_commands();
            }
            Data::InstrumentClose(close) => self.handle_instrument_close(close),
            Data::Custom(custom) => self.handle_custom_data(&custom),
        }
    }

    /// Processes a `Data` instance through the pipeline bus path.
    ///
    /// Pipeline mode publishes each item on the `data.pipeline.` topic family and gates cache
    /// writes on `disable_historical_cache`. None of the live-only side effects (synthetic
    /// republish, option-chain expiry, depth-derived quotes, deferred-command drains) run in this
    /// path.
    pub fn process_pipeline(&mut self, data: Data) {
        self.data_count += 1;

        match data {
            Data::Delta(delta) => self.handle_delta_pipeline(delta),
            Data::Deltas(deltas) => self.handle_deltas_pipeline(&deltas.into_inner()),
            Data::Depth10(depth) => self.handle_depth10_pipeline(*depth),
            Data::Quote(quote) => self.handle_quote_pipeline(quote),
            Data::Trade(trade) => self.handle_trade_pipeline(trade),
            Data::Bar(bar) => self.handle_bar_pipeline(bar),
            Data::MarkPriceUpdate(mark_price) => self.handle_mark_price_pipeline(mark_price),
            Data::IndexPriceUpdate(index_price) => self.handle_index_price_pipeline(index_price),
            Data::InstrumentStatus(status) => self.handle_instrument_status_pipeline(status),
            Data::InstrumentClose(close) => self.handle_instrument_close_pipeline(close),
            Data::Custom(custom) => self.handle_custom_data_pipeline(&custom),
        }
    }

    /// Processes a `DataResponse`, handling and publishing the response message.
    #[expect(clippy::needless_pass_by_value)] // Required by message bus dispatch
    pub fn response(&mut self, resp: DataResponse) {
        if log::log_enabled!(log::Level::Debug) {
            let correlation_id = resp.correlation_id();
            match resp.record_count() {
                Some(count) => log::debug!(
                    "{RECV}{RES} {} correlation_id={correlation_id} records={count}",
                    resp.kind(),
                ),
                None => log::debug!(
                    "{RECV}{RES} {} correlation_id={correlation_id}",
                    resp.kind(),
                ),
            }
        }
        log::trace!("{RECV}{RES} {resp:?}");

        self.response_count += 1;
        let correlation_id = *resp.correlation_id();

        match &resp {
            DataResponse::Instrument(r) => {
                self.handle_instrument_response(r.data.clone());
            }
            DataResponse::Instruments(r) => {
                self.handle_instruments(&r.data);
            }
            DataResponse::Quotes(r) => {
                if !log_if_empty_response(&r.data, &r.instrument_id, &correlation_id) {
                    self.handle_quotes(&r.data);
                }
            }
            DataResponse::Trades(r) => {
                if !log_if_empty_response(&r.data, &r.instrument_id, &correlation_id) {
                    self.handle_trades(&r.data);
                }
            }
            DataResponse::FundingRates(r) => {
                if !log_if_empty_response(&r.data, &r.instrument_id, &correlation_id) {
                    self.handle_funding_rates(&r.data);
                }
            }
            DataResponse::Bars(r) => {
                if !log_if_empty_response(&r.data, &r.bar_type, &correlation_id) {
                    self.handle_bars(&r.data);
                }
            }
            DataResponse::Book(r) => self.handle_book_response(&r.data),
            DataResponse::ForwardPrices(r) => {
                self.process_request_bar_aggregation_response(&resp);
                return self.handle_forward_prices_response(&correlation_id, r);
            }
            DataResponse::Data(_) => {}
        }

        self.process_request_bar_aggregation_response(&resp);

        msgbus::send_response(&correlation_id, &resp);
    }

    fn process_request_bar_aggregation_response(&mut self, resp: &DataResponse) {
        let correlation_id = *resp.correlation_id();
        let Some(state) = self.request_bar_aggregations.get(&correlation_id).cloned() else {
            return;
        };

        match resp {
            DataResponse::Quotes(r) => {
                for quote in &r.data {
                    self.update_request_bar_aggregators_from_quote(&state, correlation_id, *quote);
                }
            }
            DataResponse::Trades(r) => {
                for trade in &r.data {
                    self.update_request_bar_aggregators_from_trade(&state, correlation_id, *trade);
                }
            }
            DataResponse::Bars(r) => {
                for bar in &r.data {
                    self.update_request_bar_aggregators_from_bar(&state, correlation_id, *bar);
                }
            }
            _ => {}
        }

        self.cleanup_request_bar_aggregators(&correlation_id);
    }

    fn update_request_bar_aggregators_from_quote(
        &self,
        state: &RequestBarAggregation,
        request_id: UUID4,
        quote: QuoteTick,
    ) {
        let aggregator_request_id = state.aggregator_request_id(request_id);

        for bar_type in &state.bar_types {
            if bar_type.is_composite()
                || bar_type.instrument_id() != quote.instrument_id
                || bar_type.spec().price_type == PriceType::Last
            {
                continue;
            }

            self.update_request_bar_aggregator(*bar_type, aggregator_request_id, |aggregator| {
                aggregator.handle_quote(quote);
            });
        }
    }

    fn update_request_bar_aggregators_from_trade(
        &self,
        state: &RequestBarAggregation,
        request_id: UUID4,
        trade: TradeTick,
    ) {
        let aggregator_request_id = state.aggregator_request_id(request_id);

        for bar_type in &state.bar_types {
            if bar_type.is_composite()
                || bar_type.instrument_id() != trade.instrument_id
                || bar_type.spec().price_type != PriceType::Last
            {
                continue;
            }

            self.update_request_bar_aggregator(*bar_type, aggregator_request_id, |aggregator| {
                aggregator.handle_trade(trade);
            });
        }
    }

    fn update_request_bar_aggregators_from_bar(
        &self,
        state: &RequestBarAggregation,
        request_id: UUID4,
        bar: Bar,
    ) {
        let aggregator_request_id = state.aggregator_request_id(request_id);

        for bar_type in &state.bar_types {
            if !bar_type.is_composite()
                || bar_type.composite().standard() != bar.bar_type.standard()
            {
                continue;
            }

            self.update_request_bar_aggregator(*bar_type, aggregator_request_id, |aggregator| {
                aggregator.handle_bar(bar);
            });
        }
    }

    fn update_request_bar_aggregator<F>(
        &self,
        bar_type: BarType,
        request_id: Option<UUID4>,
        update: F,
    ) where
        F: FnOnce(&mut dyn BarAggregator),
    {
        let key = bar_aggregator_key(bar_type, request_id);
        let Some(aggregator) = self.bar_aggregators.get(&key) else {
            log::error!("Cannot update request bar aggregator: no aggregator found for {bar_type}");
            return;
        };

        update(aggregator.borrow_mut().as_mut());
    }

    #[inline]
    fn pipeline_cache_writes_allowed(&self) -> bool {
        !self.config.disable_historical_cache
    }

    fn handle_instrument(&mut self, instrument: &InstrumentAny) {
        log::debug!("Handling instrument: {}", instrument.id());

        if let Err(e) = self
            .cache
            .as_ref()
            .borrow_mut()
            .add_instrument(instrument.clone())
        {
            log_error_on_cache_insert(&e);
        }

        let topic = switchboard::get_instrument_topic(instrument.id());
        log::debug!("Publishing instrument to topic: {topic}");
        msgbus::publish_instrument(topic, instrument);

        self.update_option_chains(instrument);
    }

    fn update_option_chains(&mut self, instrument: &InstrumentAny) {
        let Some(underlying) = instrument.underlying() else {
            return;
        };
        let Some(expiration_ns) = instrument.expiration_ns() else {
            return;
        };
        let Some(strike) = instrument.strike_price() else {
            return;
        };
        let Some(kind) = instrument.option_kind() else {
            return;
        };

        let venue = instrument.id().venue;
        let settlement = instrument.settlement_currency().code;
        let series_id = OptionSeriesId::new(venue, underlying, settlement, expiration_ns);

        // Clone Rc to release borrow on self.option_chain_managers before accessing self.clients
        let Some(manager_rc) = self.option_chain_managers.get(&series_id).cloned() else {
            return;
        };

        let clock = self.clock.clone();
        let client = self.get_command_client(None, Some(&venue));

        if manager_rc
            .borrow_mut()
            .add_instrument(instrument.id(), strike, kind, client, &clock)
        {
            self.option_chain_instrument_index
                .insert(instrument.id(), series_id);
        }
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

            self.buffered_deltas_map
                .remove(&delta.instrument_id)
                .expect("buffered deltas exist")
        } else {
            OrderBookDeltas::new(delta.instrument_id, vec![delta])
        };

        let topic = switchboard::get_book_deltas_topic(deltas.instrument_id);
        msgbus::publish_deltas(topic, &deltas);
    }

    fn handle_deltas(&mut self, deltas: OrderBookDeltas) {
        if self.config.buffer_deltas {
            let instrument_id = deltas.instrument_id;

            for delta in deltas.deltas {
                if let Some(buffered_deltas) = self.buffered_deltas_map.get_mut(&instrument_id) {
                    buffered_deltas.deltas.push(delta);
                    buffered_deltas.flags = delta.flags;
                    buffered_deltas.sequence = delta.sequence;
                    buffered_deltas.ts_event = delta.ts_event;
                    buffered_deltas.ts_init = delta.ts_init;
                } else {
                    let buffered_deltas = OrderBookDeltas::new(instrument_id, vec![delta]);
                    self.buffered_deltas_map
                        .insert(instrument_id, buffered_deltas);
                }

                if RecordFlag::F_LAST.matches(delta.flags) {
                    let deltas_to_publish = self
                        .buffered_deltas_map
                        .remove(&instrument_id)
                        .expect("buffered deltas exist");
                    let topic = switchboard::get_book_deltas_topic(instrument_id);
                    msgbus::publish_deltas(topic, &deltas_to_publish);
                }
            }
        } else {
            let topic = switchboard::get_book_deltas_topic(deltas.instrument_id);
            msgbus::publish_deltas(topic, &deltas);
        }
    }

    fn handle_depth10(&self, depth: OrderBookDepth10) {
        let topic = switchboard::get_book_depth10_topic(depth.instrument_id);
        msgbus::publish_depth10(topic, &depth);

        if self.config.emit_quotes_from_book_depths
            && let Some(quote) = derive_quote_from_depth(&depth)
        {
            book::publish_quote_if_changed(&self.cache, quote);
        }
    }

    fn handle_quote(&self, quote: QuoteTick) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_quote(quote) {
            log_error_on_cache_insert(&e);
        }

        for synthetic_quote in self.synthetic_quotes_from_quote(quote) {
            let topic = switchboard::get_quotes_topic(synthetic_quote.instrument_id);
            msgbus::publish_quote(topic, &synthetic_quote);
        }

        let topic = switchboard::get_quotes_topic(quote.instrument_id);
        msgbus::publish_quote(topic, &quote);
    }

    fn handle_trade(&self, trade: TradeTick) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_trade(trade) {
            log_error_on_cache_insert(&e);
        }

        for synthetic_trade in self.synthetic_trades_from_trade(trade) {
            let topic = switchboard::get_trades_topic(synthetic_trade.instrument_id);
            msgbus::publish_trade(topic, &synthetic_trade);
        }

        let topic = switchboard::get_trades_topic(trade.instrument_id);
        msgbus::publish_trade(topic, &trade);
    }

    fn synthetic_quotes_from_quote(&self, update: QuoteTick) -> Vec<QuoteTick> {
        let Some(synthetics) = self.synthetic_quote_feeds.get(&update.instrument_id) else {
            return Vec::new();
        };

        synthetics
            .iter()
            .filter_map(|synthetic| self.synthetic_quote_from_update(synthetic, update))
            .collect()
    }

    fn synthetic_quote_from_update(
        &self,
        synthetic: &SyntheticInstrument,
        update: QuoteTick,
    ) -> Option<QuoteTick> {
        let cache = self.cache.borrow();
        let mut bid_inputs = Vec::with_capacity(synthetic.components.len());
        let mut ask_inputs = Vec::with_capacity(synthetic.components.len());

        for instrument_id in &synthetic.components {
            let (bid_price, ask_price) = if *instrument_id == update.instrument_id {
                (update.bid_price, update.ask_price)
            } else {
                let Some(component_quote) = cache.quote(instrument_id) else {
                    log::warn!(
                        "Cannot calculate synthetic instrument {} price, no quotes for {} yet",
                        synthetic.id,
                        instrument_id,
                    );
                    return None;
                };
                (component_quote.bid_price, component_quote.ask_price)
            };

            bid_inputs.push(bid_price.as_f64());
            ask_inputs.push(ask_price.as_f64());
        }
        drop(cache);

        let bid_price = match synthetic.calculate(&bid_inputs) {
            Ok(price) => price,
            Err(e) => {
                log::error!(
                    "Cannot calculate synthetic instrument {} bid price: {e}",
                    synthetic.id
                );
                return None;
            }
        };
        let ask_price = match synthetic.calculate(&ask_inputs) {
            Ok(price) => price,
            Err(e) => {
                log::error!(
                    "Cannot calculate synthetic instrument {} ask price: {e}",
                    synthetic.id
                );
                return None;
            }
        };
        let size_one = Quantity::from(1);

        Some(QuoteTick::new(
            synthetic.id,
            bid_price,
            ask_price,
            size_one,
            size_one,
            update.ts_event,
            self.clock.borrow().timestamp_ns(),
        ))
    }

    fn synthetic_trades_from_trade(&self, update: TradeTick) -> Vec<TradeTick> {
        let Some(synthetics) = self.synthetic_trade_feeds.get(&update.instrument_id) else {
            return Vec::new();
        };

        synthetics
            .iter()
            .filter_map(|synthetic| self.synthetic_trade_from_update(synthetic, update))
            .collect()
    }

    fn synthetic_trade_from_update(
        &self,
        synthetic: &SyntheticInstrument,
        update: TradeTick,
    ) -> Option<TradeTick> {
        let cache = self.cache.borrow();
        let mut inputs = Vec::with_capacity(synthetic.components.len());

        for instrument_id in &synthetic.components {
            let price = if *instrument_id == update.instrument_id {
                update.price
            } else {
                let Some(component_trade) = cache.trade(instrument_id) else {
                    log::warn!(
                        "Cannot calculate synthetic instrument {} price, no trades for {} yet",
                        synthetic.id,
                        instrument_id,
                    );
                    return None;
                };
                component_trade.price
            };

            inputs.push(price.as_f64());
        }
        drop(cache);

        let price = match synthetic.calculate(&inputs) {
            Ok(price) => price,
            Err(e) => {
                log::error!(
                    "Cannot calculate synthetic instrument {} trade price: {e}",
                    synthetic.id
                );
                return None;
            }
        };

        Some(TradeTick::new(
            synthetic.id,
            price,
            Quantity::from(1),
            update.aggressor_side,
            update.trade_id,
            update.ts_event,
            self.clock.borrow().timestamp_ns(),
        ))
    }

    fn handle_bar(&self, bar: Bar) {
        process_engine_bar(&self.cache, self.config.validate_data_sequence, true, bar);
    }

    fn handle_mark_price(&self, mark_price: MarkPriceUpdate) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_mark_price(mark_price) {
            log_error_on_cache_insert(&e);
        }

        let topic = switchboard::get_mark_price_topic(mark_price.instrument_id);
        msgbus::publish_mark_price(topic, &mark_price);
    }

    fn handle_index_price(&self, index_price: IndexPriceUpdate) {
        if let Err(e) = self
            .cache
            .as_ref()
            .borrow_mut()
            .add_index_price(index_price)
        {
            log_error_on_cache_insert(&e);
        }

        let topic = switchboard::get_index_price_topic(index_price.instrument_id);
        msgbus::publish_index_price(topic, &index_price);
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
        msgbus::publish_funding_rate(topic, &funding_rate);
    }

    fn handle_instrument_status(&mut self, status: InstrumentStatus) {
        if let Err(e) = self
            .cache
            .as_ref()
            .borrow_mut()
            .add_instrument_status(status)
        {
            log_error_on_cache_insert(&e);
        }

        let topic = switchboard::get_instrument_status_topic(status.instrument_id);
        msgbus::publish_any(topic, &status);

        if self
            .option_chain_instrument_index
            .contains_key(&status.instrument_id)
            && matches!(
                status.action,
                MarketStatusAction::Close | MarketStatusAction::NotAvailableForTrading
            )
        {
            self.expire_option_chain_instrument(status.instrument_id);
        }
    }

    /// Removes a settled/expired instrument from its option chain manager.
    ///
    /// Looks up the owning series via the reverse index, delegates removal to
    /// the manager (which unregisters msgbus handlers and pushes deferred wire
    /// unsubscribes), then drains those commands. When the series catalog
    /// becomes empty, the entire manager is torn down.
    fn expire_option_chain_instrument(&mut self, instrument_id: InstrumentId) {
        let Some(series_id) = self.option_chain_instrument_index.remove(&instrument_id) else {
            return;
        };

        let Some(manager_rc) = self.option_chain_managers.get(&series_id).cloned() else {
            return;
        };

        let series_empty = manager_rc
            .borrow_mut()
            .handle_instrument_expired(&instrument_id);

        // Drain deferred unsubscribe commands pushed by the manager
        self.drain_deferred_commands();

        log::info!(
            "Expired instrument {instrument_id} from option chain {series_id} (series_empty={series_empty})",
        );

        if series_empty {
            manager_rc.borrow_mut().teardown(&self.clock);
            self.option_chain_managers.remove(&series_id);

            log::info!("Torn down empty option chain manager for {series_id}");
        }
    }

    fn handle_instrument_close(&self, close: InstrumentClose) {
        let topic = switchboard::get_instrument_close_topic(close.instrument_id);
        msgbus::publish_any(topic, &close);
    }

    fn handle_custom_data(&self, custom: &CustomData) {
        log::debug!("Processing custom data: {}", custom.data.type_name());
        let topic = switchboard::get_custom_topic(&custom.data_type);
        msgbus::publish_any(topic, custom);
    }

    fn handle_delta_pipeline(&self, delta: OrderBookDelta) {
        // Pipeline deltas are not buffered; replays arrive pre-batched
        let deltas = OrderBookDeltas::new(delta.instrument_id, vec![delta]);
        let topic = switchboard::get_pipeline_book_deltas_topic(deltas.instrument_id);
        msgbus::publish_deltas(topic, &deltas);
    }

    fn handle_deltas_pipeline(&self, deltas: &OrderBookDeltas) {
        let topic = switchboard::get_pipeline_book_deltas_topic(deltas.instrument_id);
        msgbus::publish_deltas(topic, deltas);
    }

    fn handle_depth10_pipeline(&self, depth: OrderBookDepth10) {
        let topic = switchboard::get_pipeline_book_depth10_topic(depth.instrument_id);
        msgbus::publish_depth10(topic, &depth);
    }

    fn handle_quote_pipeline(&self, quote: QuoteTick) {
        if self.pipeline_cache_writes_allowed()
            && let Err(e) = self.cache.as_ref().borrow_mut().add_quote(quote)
        {
            log_error_on_cache_insert(&e);
        }

        let topic = switchboard::get_pipeline_quotes_topic(quote.instrument_id);
        msgbus::publish_quote(topic, &quote);
    }

    fn handle_trade_pipeline(&self, trade: TradeTick) {
        if self.pipeline_cache_writes_allowed()
            && let Err(e) = self.cache.as_ref().borrow_mut().add_trade(trade)
        {
            log_error_on_cache_insert(&e);
        }

        let topic = switchboard::get_pipeline_trades_topic(trade.instrument_id);
        msgbus::publish_trade(topic, &trade);
    }

    fn handle_bar_pipeline(&self, bar: Bar) {
        if !validate_bar_sequence(&self.cache, self.config.validate_data_sequence, &bar) {
            return;
        }

        if self.pipeline_cache_writes_allowed()
            && let Err(e) = self.cache.as_ref().borrow_mut().add_bar(bar)
        {
            log_error_on_cache_insert(&e);
        }

        let topic = switchboard::get_pipeline_bars_topic(bar.bar_type);
        msgbus::publish_bar(topic, &bar);
    }

    fn handle_mark_price_pipeline(&self, mark_price: MarkPriceUpdate) {
        if self.pipeline_cache_writes_allowed()
            && let Err(e) = self.cache.as_ref().borrow_mut().add_mark_price(mark_price)
        {
            log_error_on_cache_insert(&e);
        }

        let topic = switchboard::get_pipeline_mark_price_topic(mark_price.instrument_id);
        msgbus::publish_mark_price(topic, &mark_price);
    }

    fn handle_index_price_pipeline(&self, index_price: IndexPriceUpdate) {
        if self.pipeline_cache_writes_allowed()
            && let Err(e) = self
                .cache
                .as_ref()
                .borrow_mut()
                .add_index_price(index_price)
        {
            log_error_on_cache_insert(&e);
        }

        let topic = switchboard::get_pipeline_index_price_topic(index_price.instrument_id);
        msgbus::publish_index_price(topic, &index_price);
    }

    fn handle_instrument_status_pipeline(&self, status: InstrumentStatus) {
        if self.pipeline_cache_writes_allowed()
            && let Err(e) = self
                .cache
                .as_ref()
                .borrow_mut()
                .add_instrument_status(status)
        {
            log_error_on_cache_insert(&e);
        }

        let topic = switchboard::get_pipeline_instrument_status_topic(status.instrument_id);
        msgbus::publish_any(topic, &status);
    }

    fn handle_instrument_close_pipeline(&self, close: InstrumentClose) {
        let topic = switchboard::get_pipeline_instrument_close_topic(close.instrument_id);
        msgbus::publish_any(topic, &close);
    }

    fn handle_custom_data_pipeline(&self, custom: &CustomData) {
        log::debug!("Pipeline custom data: {}", custom.data.type_name());
        let topic = switchboard::get_pipeline_custom_topic(&custom.data_type);
        msgbus::publish_any(topic, custom);
    }

    /// Drains deferred subscribe/unsubscribe commands pushed by option chain
    /// managers (or any other component) and executes them against the appropriate
    /// data client.
    fn drain_deferred_commands(&mut self) {
        // Loop because expire_series pushes Unsubscribe commands; converges in <= 3 iterations
        loop {
            let commands: VecDeque<DeferredCommand> =
                std::mem::take(&mut *self.deferred_cmd_queue.borrow_mut());

            if commands.is_empty() {
                break;
            }

            for cmd in commands {
                match cmd {
                    DeferredCommand::Subscribe(sub) => {
                        let client = self.get_command_client(sub.client_id(), sub.venue());
                        if let Some(client) = client {
                            client.execute_subscribe(sub);
                        }
                    }
                    DeferredCommand::Unsubscribe(unsub) => {
                        let client = self.get_command_client(unsub.client_id(), unsub.venue());
                        if let Some(client) = client {
                            client.execute_unsubscribe(&unsub);
                        }
                    }
                    DeferredCommand::ExpireInstrument(instrument_id) => {
                        self.expire_option_chain_instrument(instrument_id);
                    }
                    DeferredCommand::ExpireSeries(series_id) => {
                        self.expire_series(series_id);
                    }
                }
            }
        }
    }

    /// Proactively expires all instruments for a series and tears down the manager.
    ///
    /// `handle_instrument_expired` removes each instrument from the aggregator and pushes
    /// deferred unsubscribe commands. `teardown` then cancels the snapshot timer and clears
    /// the handler lists (the aggregator is already empty at that point).
    fn expire_series(&mut self, series_id: OptionSeriesId) {
        let Some(manager_rc) = self.option_chain_managers.get(&series_id).cloned() else {
            return;
        };

        let instrument_ids: Vec<InstrumentId> = self
            .option_chain_instrument_index
            .iter()
            .filter(|(_, sid)| **sid == series_id)
            .map(|(id, _)| *id)
            .collect();

        for id in &instrument_ids {
            self.option_chain_instrument_index.remove(id);
            manager_rc.borrow_mut().handle_instrument_expired(id);
        }

        manager_rc.borrow_mut().teardown(&self.clock);
        self.option_chain_managers.remove(&series_id);

        log::info!("Proactively torn down expired option chain {series_id}");
    }

    fn subscribe_book_deltas(&mut self, cmd: &SubscribeBookDeltas) -> anyhow::Result<()> {
        if cmd.instrument_id.is_synthetic() {
            anyhow::bail!("Cannot subscribe for synthetic instrument `OrderBookDelta` data");
        }

        // Validate parent shape BEFORE mutating subscription state so a parse
        // failure leaves the engine bookkeeping unchanged.
        let parent = resolve_parent_components(&cmd.instrument_id, cmd.params.as_ref())?;

        self.book_deltas_subs.insert(cmd.instrument_id);
        if cmd.managed {
            self.setup_book_updater(&cmd.instrument_id, cmd.book_type, true, parent)?;
        }

        Ok(())
    }

    fn subscribe_book_depth10(&mut self, cmd: &SubscribeBookDepth10) -> anyhow::Result<()> {
        if cmd.instrument_id.is_synthetic() {
            anyhow::bail!("Cannot subscribe for synthetic instrument `OrderBookDepth10` data");
        }

        let parent = resolve_parent_components(&cmd.instrument_id, cmd.params.as_ref())?;

        self.book_depth10_subs.insert(cmd.instrument_id);
        if cmd.managed {
            self.setup_book_updater(&cmd.instrument_id, cmd.book_type, false, parent)?;
        }

        Ok(())
    }

    fn subscribe_book_snapshots(&mut self, cmd: &SubscribeBookSnapshots) -> anyhow::Result<()> {
        if cmd.instrument_id.is_synthetic() {
            anyhow::bail!("Cannot subscribe for synthetic instrument `OrderBookDelta` data");
        }

        let parent = resolve_parent_components(&cmd.instrument_id, cmd.params.as_ref())?;

        let had_snapshots = self.has_book_snapshot_subscriptions(&cmd.instrument_id);
        let inserted = self.increment_book_snapshot_subscription(cmd, parent);

        if inserted && !had_snapshots {
            // Always run setup so the depth10 handler is registered alongside
            // the deltas handler when this is the first snapshot for the id;
            // setup_book_updater is idempotent and the typed router dedups
            // overlapping subscribes.
            self.setup_book_updater(&cmd.instrument_id, cmd.book_type, false, parent)?;
        }

        if had_snapshots || self.book_deltas_subs.contains(&cmd.instrument_id) {
            return Ok(());
        }

        if let Some(client_id) = cmd.client_id.as_ref()
            && self.external_clients.contains(client_id)
        {
            if self.config.debug {
                log::debug!("Skipping subscribe command for external client {client_id}: {cmd:?}");
            }
            return Ok(());
        }

        log::debug!(
            "Forwarding BookSnapshots as BookDeltas for {}, client_id={:?}, venue={:?}",
            cmd.instrument_id,
            cmd.client_id,
            cmd.venue,
        );

        if let Some(client) = self.get_command_client(cmd.client_id.as_ref(), cmd.venue.as_ref()) {
            let deltas_cmd = SubscribeBookDeltas::new(
                cmd.instrument_id,
                cmd.book_type,
                cmd.client_id,
                cmd.venue,
                UUID4::new(),
                cmd.ts_init,
                cmd.depth,
                true, // managed
                Some(cmd.command_id),
                cmd.params.clone(),
            );
            log::debug!(
                "Calling client.execute_subscribe for BookDeltas: {}",
                cmd.instrument_id
            );
            client.execute_subscribe(SubscribeCommand::BookDeltas(deltas_cmd));
        } else {
            log::error!(
                "Cannot handle command: no client found for client_id={:?}, venue={:?}",
                cmd.client_id,
                cmd.venue,
            );
        }

        Ok(())
    }

    fn subscribe_bars(&mut self, cmd: &SubscribeBars) -> anyhow::Result<()> {
        match cmd.bar_type.aggregation_source() {
            AggregationSource::Internal => {
                let key = bar_aggregator_key(cmd.bar_type, None);

                if self
                    .bar_aggregators
                    .get(&key)
                    .is_none_or(|aggregator| !aggregator.borrow().is_running())
                    || !self.bar_aggregator_handlers.contains_key(&key)
                {
                    self.start_bar_aggregator(cmd.bar_type, None)?;
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

    fn subscribe_synthetic_quotes(&mut self, instrument_id: InstrumentId) {
        let Some(synthetic) = self.cache.borrow().synthetic(&instrument_id).cloned() else {
            log::error!(
                "Cannot subscribe to `QuoteTick` data for synthetic instrument {instrument_id}, not found",
            );
            return;
        };

        if !self.subscribed_synthetic_quotes.insert(instrument_id) {
            return;
        }

        for component_id in &synthetic.components {
            let synthetics = self.synthetic_quote_feeds.entry(*component_id).or_default();
            if !synthetics
                .iter()
                .any(|registered| registered.id == synthetic.id)
            {
                synthetics.push(synthetic.clone());
            }
        }
    }

    fn subscribe_synthetic_trades(&mut self, instrument_id: InstrumentId) {
        let Some(synthetic) = self.cache.borrow().synthetic(&instrument_id).cloned() else {
            log::error!(
                "Cannot subscribe to `TradeTick` data for synthetic instrument {instrument_id}, not found",
            );
            return;
        };

        if !self.subscribed_synthetic_trades.insert(instrument_id) {
            return;
        }

        for component_id in &synthetic.components {
            let synthetics = self.synthetic_trade_feeds.entry(*component_id).or_default();
            if !synthetics
                .iter()
                .any(|registered| registered.id == synthetic.id)
            {
                synthetics.push(synthetic.clone());
            }
        }
    }

    fn is_spread_quote_command(
        &self,
        instrument_id: InstrumentId,
        params: Option<&Params>,
    ) -> bool {
        if !params
            .and_then(|params| params.get_bool("aggregate_spread_quotes"))
            .unwrap_or(false)
        {
            return false;
        }

        self.cache
            .borrow()
            .instrument(&instrument_id)
            .is_some_and(InstrumentAny::is_spread)
    }

    fn subscribe_spread_quotes(&mut self, cmd: &SubscribeQuotes) {
        if self
            .spread_quote_aggregators
            .contains_key(&cmd.instrument_id)
        {
            log::warn!(
                "SpreadQuoteAggregator for {} is currently in use, subscription can't be started",
                cmd.instrument_id,
            );
            return;
        }

        let Some(instrument) = self.cache.borrow().instrument(&cmd.instrument_id).cloned() else {
            log::error!(
                "Cannot create spread quote aggregator: no instrument found for {}",
                cmd.instrument_id,
            );
            return;
        };
        let Some(legs) = spread_instrument_legs(&instrument) else {
            log::error!(
                "Cannot create spread quote aggregator: invalid spread legs for {}",
                cmd.instrument_id,
            );
            return;
        };

        if legs.len() <= 1 {
            log::error!(
                "Cannot create spread quote aggregator: spread instrument {} should have more than one leg",
                cmd.instrument_id,
            );
            return;
        }

        let cache = self.cache.clone();
        let handler = Box::new(move |quote: QuoteTick| {
            if let Err(e) = cache.borrow_mut().add_quote(quote) {
                log_error_on_cache_insert(&e);
            }
            let topic = switchboard::get_quotes_topic(quote.instrument_id);
            msgbus::publish_quote(topic, &quote);
        });
        let aggregator = Rc::new(RefCell::new(SpreadQuoteAggregator::new(
            cmd.instrument_id,
            &legs,
            matches!(instrument, InstrumentAny::FuturesSpread(_)),
            instrument.price_precision(),
            instrument.size_precision(),
            handler,
            self.clock.clone(),
            false,
            spread_quote_update_interval_seconds(cmd.params.as_ref()),
            cmd.params
                .as_ref()
                .and_then(|params| params.get_u64("quote_build_delay"))
                .unwrap_or(0),
            None,
            None,
        )));

        let mut handlers = Vec::with_capacity(legs.len());
        for (leg_id, _) in &legs {
            let topic = switchboard::get_quotes_topic(*leg_id);
            let handler = TypedHandler::new(SpreadQuoteHandler::new(
                &aggregator,
                cmd.instrument_id,
                *leg_id,
            ));
            msgbus::subscribe_quotes(topic.into(), handler.clone(), Some(BAR_AGGREGATOR_PRIORITY));
            handlers.push((*leg_id, handler));
        }

        aggregator
            .borrow_mut()
            .start_timer(Some(aggregator.clone()));
        aggregator.borrow_mut().set_running(true);
        self.spread_quote_aggregators
            .insert(cmd.instrument_id, aggregator);
        self.spread_quote_handlers
            .insert(cmd.instrument_id, handlers);

        for (leg_id, _) in legs {
            let subscribe = SubscribeQuotes::new(
                leg_id,
                cmd.client_id,
                cmd.venue,
                UUID4::new(),
                cmd.ts_init,
                Some(cmd.command_id),
                cmd.params.clone(),
            );
            self.execute(DataCommand::Subscribe(SubscribeCommand::Quotes(subscribe)));
        }
    }

    fn unsubscribe_spread_quotes(&mut self, cmd: &UnsubscribeQuotes) {
        let Some(leg_ids) = self.stop_spread_quote_aggregator(cmd.instrument_id) else {
            return;
        };

        for leg_id in leg_ids {
            let unsubscribe = UnsubscribeQuotes::new(
                leg_id,
                cmd.client_id,
                cmd.venue,
                UUID4::new(),
                cmd.ts_init,
                Some(cmd.command_id),
                cmd.params.clone(),
            );
            self.execute(DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(
                unsubscribe,
            )));
        }
    }

    fn stop_spread_quote_aggregator(
        &mut self,
        spread_instrument_id: InstrumentId,
    ) -> Option<Vec<InstrumentId>> {
        let Some(aggregator) = self.spread_quote_aggregators.remove(&spread_instrument_id) else {
            log::warn!(
                "Cannot stop spread quote aggregator: no aggregator to stop for {spread_instrument_id}",
            );
            return None;
        };

        aggregator.borrow_mut().stop_timer();
        aggregator.borrow_mut().set_running(false);

        let handlers = self
            .spread_quote_handlers
            .remove(&spread_instrument_id)
            .unwrap_or_default();
        let mut leg_ids = Vec::with_capacity(handlers.len());
        for (leg_id, handler) in handlers {
            let topic = switchboard::get_quotes_topic(leg_id);
            msgbus::unsubscribe_quotes(topic.into(), &handler);
            leg_ids.push(leg_id);
        }

        Some(leg_ids)
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> bool {
        if !self.book_deltas_subs.contains(&cmd.instrument_id) {
            log::warn!("Cannot unsubscribe from `OrderBookDeltas` data: not subscribed");
            return false;
        }

        self.book_deltas_subs.remove(&cmd.instrument_id);
        self.maintain_book_updater(&cmd.instrument_id);

        // Snapshot subscriptions reuse the deltas feed.
        // Keep the client subscribed until the last snapshot consumer is gone.
        !self.has_book_snapshot_subscriptions(&cmd.instrument_id)
    }

    fn unsubscribe_book_depth10(&mut self, cmd: &UnsubscribeBookDepth10) -> bool {
        if !self.book_depth10_subs.contains(&cmd.instrument_id) {
            log::warn!("Cannot unsubscribe from `OrderBookDepth10` data: not subscribed");
            return false;
        }

        self.book_depth10_subs.remove(&cmd.instrument_id);
        self.maintain_book_updater(&cmd.instrument_id);

        true
    }

    fn unsubscribe_book_snapshots(&mut self, cmd: &UnsubscribeBookSnapshots) {
        match self.decrement_book_snapshot_subscription(cmd.instrument_id, cmd.interval_ms) {
            BookSnapshotUnsubscribeResult::NotSubscribed => {
                log::warn!("Cannot unsubscribe from `OrderBook` snapshots: not subscribed");
                return;
            }
            BookSnapshotUnsubscribeResult::Decremented => return,
            BookSnapshotUnsubscribeResult::Removed => {}
        }

        if self.has_book_snapshot_subscriptions(&cmd.instrument_id) {
            return;
        }

        self.maintain_book_updater(&cmd.instrument_id);

        if self.book_deltas_subs.contains(&cmd.instrument_id) {
            return;
        }

        if let Some(client_id) = cmd.client_id.as_ref()
            && self.external_clients.contains(client_id)
        {
            return;
        }

        if let Some(client) = self.get_command_client(cmd.client_id.as_ref(), cmd.venue.as_ref()) {
            let deltas_cmd = UnsubscribeBookDeltas::new(
                cmd.instrument_id,
                cmd.client_id,
                cmd.venue,
                UUID4::new(),
                cmd.ts_init,
                Some(cmd.command_id),
                cmd.params.clone(),
            );
            client.execute_unsubscribe(&UnsubscribeCommand::BookDeltas(deltas_cmd));
        }
    }

    fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) {
        let bar_type = cmd.bar_type;

        // Don't remove aggregator if other exact-topic subscribers still exist
        let topic = switchboard::get_bars_topic(bar_type.standard());
        if msgbus::exact_subscriber_count_bars(topic) > 0 {
            return;
        }

        if self
            .bar_aggregators
            .contains_key(&bar_aggregator_key(bar_type, None))
            && let Err(e) = self.stop_bar_aggregator(bar_type, None)
        {
            log::error!("Error stopping bar aggregator for {bar_type}: {e}");
        }

        // After stopping a composite, check if the source aggregator is now orphaned
        if bar_type.is_composite() {
            let source_type = bar_type.composite();
            let source_topic = switchboard::get_bars_topic(source_type);
            if msgbus::exact_subscriber_count_bars(source_topic) == 0
                && self
                    .bar_aggregators
                    .contains_key(&bar_aggregator_key(source_type, None))
                && let Err(e) = self.stop_bar_aggregator(source_type, None)
            {
                log::error!("Error stopping source bar aggregator for {source_type}: {e}");
            }
        }
    }

    fn unsubscribe_synthetic_quotes(&mut self, instrument_id: InstrumentId) {
        if !self.subscribed_synthetic_quotes.remove(&instrument_id) {
            log::warn!("Cannot unsubscribe from synthetic `QuoteTick` data: not subscribed");
            return;
        }

        self.synthetic_quote_feeds.retain(|_, synthetics| {
            synthetics.retain(|synthetic| synthetic.id != instrument_id);
            !synthetics.is_empty()
        });
    }

    fn unsubscribe_synthetic_trades(&mut self, instrument_id: InstrumentId) {
        if !self.subscribed_synthetic_trades.remove(&instrument_id) {
            log::warn!("Cannot unsubscribe from synthetic `TradeTick` data: not subscribed");
            return;
        }

        self.synthetic_trade_feeds.retain(|_, synthetics| {
            synthetics.retain(|synthetic| synthetic.id != instrument_id);
            !synthetics.is_empty()
        });
    }

    fn subscribe_option_chain(&mut self, cmd: &SubscribeOptionChain) {
        let series_id = cmd.series_id;

        // Handle edits to existing subscriptions by tearing down and re-setting up the OptionChainManager.
        if let Some(old) = self.option_chain_managers.remove(&series_id) {
            log::info!("Re-subscribing option chain for {series_id}, tearing down previous");
            let all_ids = old.borrow().all_instrument_ids();
            let old_venue = old.borrow().venue();
            old.borrow_mut().teardown(&self.clock);
            self.forward_option_chain_unsubscribes(&all_ids, old_venue, cmd.client_id);
        }

        // Drain any stale pending forward price requests for this series
        self.pending_option_chain_requests
            .retain(|_, pending_cmd| pending_cmd.series_id != series_id);

        // For ATM-based strike ranges, request forward prices from the adapter
        // to enable instant bootstrap without waiting for the first WebSocket tick.
        if !matches!(cmd.strike_range, StrikeRange::Fixed(_)) {
            // Extract client_id first to avoid borrow conflicts
            let resolved_client_id = self
                .get_client(cmd.client_id.as_ref(), Some(&series_id.venue))
                .map(|c| c.client_id);

            if let Some(client_id) = resolved_client_id {
                let request_id = UUID4::new();
                let ts_init = self.clock.borrow().timestamp_ns();

                // Pick any one option instrument at this expiry from cache
                // to enable single-instrument forward price fetch (1 HTTP call)
                let sample_instrument_id = {
                    let cache = self.cache.borrow();
                    cache
                        .instruments(&series_id.venue, Some(&series_id.underlying))
                        .iter()
                        .find(|i| {
                            i.expiration_ns() == Some(series_id.expiration_ns)
                                && i.settlement_currency().code == series_id.settlement_currency
                        })
                        .map(|i| i.id())
                };

                let request = RequestForwardPrices::new(
                    series_id.venue,
                    series_id.underlying,
                    sample_instrument_id,
                    Some(client_id),
                    request_id,
                    ts_init,
                    None,
                );

                self.pending_option_chain_requests
                    .insert(request_id, cmd.clone());

                let req_cmd = RequestCommand::ForwardPrices(request);
                if let Err(e) = self.execute_request(req_cmd) {
                    log::warn!("Failed to request forward prices for {series_id}: {e}");
                    let cmd = self
                        .pending_option_chain_requests
                        .remove(&request_id)
                        .expect("just inserted");
                    self.create_option_chain_manager(&cmd, None);
                }

                return;
            }
        }

        self.create_option_chain_manager(cmd, None);
    }

    /// Creates and stores an `OptionChainManager` for the given subscription.
    fn create_option_chain_manager(
        &mut self,
        cmd: &SubscribeOptionChain,
        initial_atm_price: Option<Price>,
    ) {
        let series_id = cmd.series_id;
        let cache = self.cache.clone();
        let clock = self.clock.clone();
        let priority = self.msgbus_priority;
        let deferred_cmd_queue = self.deferred_cmd_queue.clone();

        let manager_rc = {
            let client = self.get_command_client(cmd.client_id.as_ref(), Some(&series_id.venue));
            OptionChainManager::create_and_setup(
                series_id,
                &cache,
                cmd,
                &clock,
                priority,
                client,
                initial_atm_price,
                deferred_cmd_queue,
            )
        };

        // Index all instruments for reverse lookup
        for id in manager_rc.borrow().all_instrument_ids() {
            self.option_chain_instrument_index.insert(id, series_id);
        }

        self.option_chain_managers.insert(series_id, manager_rc);
    }

    fn unsubscribe_option_chain(&mut self, cmd: &UnsubscribeOptionChain) {
        let series_id = cmd.series_id;

        let Some(manager_rc) = self.option_chain_managers.remove(&series_id) else {
            log::warn!("Cannot unsubscribe option chain for {series_id}: not subscribed");
            return;
        };

        // Extract info before teardown
        let all_ids = manager_rc.borrow().all_instrument_ids();
        let venue = manager_rc.borrow().venue();

        // Remove all instruments from reverse index
        for id in &all_ids {
            self.option_chain_instrument_index.remove(id);
        }

        manager_rc.borrow_mut().teardown(&self.clock);

        // Forward wire-level unsubscribes to the data client
        self.forward_option_chain_unsubscribes(&all_ids, venue, cmd.client_id);

        log::info!("Unsubscribed option chain for {series_id}");
    }

    /// Forwards wire-level unsubscribe commands for all option chain instruments.
    fn forward_option_chain_unsubscribes(
        &mut self,
        instrument_ids: &[InstrumentId],
        venue: Venue,
        client_id: Option<ClientId>,
    ) {
        let ts_init = self.clock.borrow().timestamp_ns();

        let Some(client) = self.get_command_client(client_id.as_ref(), Some(&venue)) else {
            log::error!(
                "Cannot forward option chain unsubscribes: no client found for venue={venue}",
            );
            return;
        };

        for instrument_id in instrument_ids {
            client.execute_unsubscribe(&UnsubscribeCommand::Quotes(UnsubscribeQuotes::new(
                *instrument_id,
                client_id,
                Some(venue),
                UUID4::new(),
                ts_init,
                None,
                None,
            )));
            client.execute_unsubscribe(&UnsubscribeCommand::OptionGreeks(
                UnsubscribeOptionGreeks::new(
                    *instrument_id,
                    client_id,
                    Some(venue),
                    UUID4::new(),
                    ts_init,
                    None,
                    None,
                ),
            ));
            client.execute_unsubscribe(&UnsubscribeCommand::InstrumentStatus(
                UnsubscribeInstrumentStatus::new(
                    *instrument_id,
                    client_id,
                    Some(venue),
                    UUID4::new(),
                    ts_init,
                    None,
                    None,
                ),
            ));
        }
    }

    fn maintain_book_updater(&mut self, instrument_id: &InstrumentId) {
        // Determine which per-underlying books this subscription touched, then
        // for each book check whether any other active subscription still
        // wants it before unsubscribing/dropping the shared BookUpdater.
        //
        // The presence of a memoized expansion identifies a parent teardown.
        // Concrete subscriptions touch only the exact id.
        let is_parent = self
            .book_deltas_parent_expansions
            .contains_key(instrument_id)
            || self
                .book_depth10_parent_expansions
                .contains_key(instrument_id);
        let target_ids: Vec<InstrumentId> = if is_parent {
            let mut set: AHashSet<InstrumentId> = AHashSet::new();

            if let Some(expansion) = self.book_deltas_parent_expansions.get(instrument_id) {
                set.extend(expansion.iter().copied());
            }

            if let Some(expansion) = self.book_depth10_parent_expansions.get(instrument_id) {
                set.extend(expansion.iter().copied());
            }

            if set.is_empty() {
                return;
            }

            set.into_iter().collect()
        } else {
            vec![*instrument_id]
        };

        if is_parent {
            // Each parent kind (deltas / depth10 / snapshots) writes its own
            // memo via setup_book_updater. Keep each memo alive while any
            // sibling subscription that drives the same handler kind remains
            // active for this parent id.
            let parent_still_needs_deltas = self.book_deltas_subs.contains(instrument_id)
                || self.book_depth10_subs.contains(instrument_id)
                || self.has_book_snapshot_subscriptions(instrument_id);
            let parent_still_needs_depth10 = self.book_depth10_subs.contains(instrument_id)
                || self.has_book_snapshot_subscriptions(instrument_id);

            if !parent_still_needs_deltas {
                self.book_deltas_parent_expansions.remove(instrument_id);
            }

            if !parent_still_needs_depth10 {
                self.book_depth10_parent_expansions.remove(instrument_id);
            }
        }

        for target_id in &target_ids {
            let wants_deltas = self.is_underlying_wanted_for_deltas(target_id);
            let wants_depth10 = self.is_underlying_wanted_for_depth10(target_id);

            let Some(updater) = self.book_updaters.get(target_id).cloned() else {
                continue;
            };

            let deltas_handler: TypedHandler<OrderBookDeltas> = TypedHandler::new(updater.clone());
            let depth_handler: TypedHandler<OrderBookDepth10> = TypedHandler::new(updater);

            if !wants_deltas {
                let topic = switchboard::get_book_deltas_topic(*target_id);
                msgbus::unsubscribe_book_deltas(topic.into(), &deltas_handler);
            }

            if !wants_depth10 {
                let topic = switchboard::get_book_depth10_topic(*target_id);
                msgbus::unsubscribe_book_depth10(topic.into(), &depth_handler);
            }

            if !wants_deltas && !wants_depth10 {
                self.book_updaters.remove(target_id);
                log::debug!("Removed BookUpdater for instrument ID {target_id}");
            }
        }
    }

    fn has_book_snapshot_subscriptions(&self, instrument_id: &InstrumentId) -> bool {
        self.book_snapshot_counts
            .keys()
            .any(|(id, _)| id == instrument_id)
    }

    fn increment_book_snapshot_subscription(
        &mut self,
        cmd: &SubscribeBookSnapshots,
        parent: Option<(Ustr, InstrumentClass)>,
    ) -> bool {
        let key = (cmd.instrument_id, cmd.interval_ms);

        if let Some(count) = self.book_snapshot_counts.get_mut(&key) {
            *count += 1;
            return false;
        }

        self.book_snapshot_counts.insert(key, 1);

        let snapshot_infos = if let Some(snapshot_infos) = self.book_intervals.get(&cmd.interval_ms)
        {
            snapshot_infos.clone()
        } else {
            let snapshot_infos = Rc::new(RefCell::new(IndexMap::new()));
            self.book_intervals
                .insert(cmd.interval_ms, snapshot_infos.clone());
            self.schedule_book_snapshotter(cmd.interval_ms, snapshot_infos.clone());
            snapshot_infos
        };

        let topic = switchboard::get_book_snapshots_topic(cmd.instrument_id, cmd.interval_ms);
        let snap_info = BookSnapshotInfo {
            instrument_id: cmd.instrument_id,
            venue: cmd.instrument_id.venue,
            parent,
            topic,
            interval_ms: cmd.interval_ms,
        };

        snapshot_infos
            .borrow_mut()
            .insert(cmd.instrument_id, snap_info);

        true
    }

    fn decrement_book_snapshot_subscription(
        &mut self,
        instrument_id: InstrumentId,
        interval_ms: NonZeroUsize,
    ) -> BookSnapshotUnsubscribeResult {
        let key = (instrument_id, interval_ms);

        let Some(count) = self.book_snapshot_counts.get_mut(&key) else {
            return BookSnapshotUnsubscribeResult::NotSubscribed;
        };

        if *count > 1 {
            *count -= 1;
            return BookSnapshotUnsubscribeResult::Decremented;
        }

        self.book_snapshot_counts.shift_remove(&key);

        let remove_interval = if let Some(snapshot_infos) = self.book_intervals.get(&interval_ms) {
            let mut snapshot_infos = snapshot_infos.borrow_mut();
            snapshot_infos.shift_remove(&instrument_id);
            snapshot_infos.is_empty()
        } else {
            false
        };

        if remove_interval {
            self.book_intervals.remove(&interval_ms);

            if let Some(snapshotter) = self.book_snapshotters.remove(&interval_ms) {
                let timer_name = snapshotter.timer_name;
                let mut clock = self.clock.borrow_mut();
                if clock.timer_exists(&timer_name) {
                    clock.cancel_timer(&timer_name);
                }
            }
        }

        BookSnapshotUnsubscribeResult::Removed
    }

    fn schedule_book_snapshotter(
        &mut self,
        interval_ms: NonZeroUsize,
        snapshot_infos: BookSnapshotInfos,
    ) {
        let interval_ns = millis_to_nanos_unchecked(interval_ms.get() as f64);
        let now_ns = self.clock.borrow().timestamp_ns().as_u64();
        let start_time_ns = now_ns - (now_ns % interval_ns) + interval_ns;

        let snapshotter = Rc::new(BookSnapshotter::new(
            interval_ms,
            snapshot_infos,
            self.cache.clone(),
        ));
        let timer_name = snapshotter.timer_name;
        let snapshotter_callback = snapshotter.clone();
        let callback_fn: Rc<dyn Fn(TimeEvent)> =
            Rc::new(move |event| snapshotter_callback.snapshot(event));
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

        self.book_snapshotters.insert(interval_ms, snapshotter);
    }

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

    fn handle_funding_rates(&self, funding_rates: &[FundingRateUpdate]) {
        if let Err(e) = self
            .cache
            .as_ref()
            .borrow_mut()
            .add_funding_rates(funding_rates)
        {
            log_error_on_cache_insert(&e);
        }
    }

    fn handle_bars(&self, bars: &[Bar]) {
        if let Err(e) = self.cache.as_ref().borrow_mut().add_bars(bars) {
            log_error_on_cache_insert(&e);
        }
    }

    fn handle_book_response(&self, book: &OrderBook) {
        log::debug!("Adding order book {} to cache", book.instrument_id);

        if let Err(e) = self
            .cache
            .as_ref()
            .borrow_mut()
            .add_order_book(book.clone())
        {
            log_error_on_cache_insert(&e);
        }
    }

    /// Handles a `ForwardPricesResponse` by extracting the forward price
    /// for the pending option chain and creating the manager with instant bootstrap.
    fn handle_forward_prices_response(
        &mut self,
        correlation_id: &UUID4,
        resp: &ForwardPricesResponse,
    ) {
        let Some(cmd) = self.pending_option_chain_requests.remove(correlation_id) else {
            log::debug!(
                "No pending option chain request for correlation_id={correlation_id}, ignoring"
            );
            return;
        };

        let series_id = cmd.series_id;

        // Find a forward price that matches an instrument in this series.
        // We look up each forward price instrument in the cache to match by expiry and currency.
        let cache = self.cache.borrow();
        let mut best_price: Option<Price> = None;

        for fp in &resp.data {
            // Check if any cached instrument with this id belongs to our series
            if let Some(instrument) = cache.instrument(&fp.instrument_id)
                && let Some(expiration) = instrument.expiration_ns()
                && expiration == series_id.expiration_ns
                && instrument.settlement_currency().code == series_id.settlement_currency
            {
                match Price::from_decimal(fp.forward_price) {
                    Ok(price) => best_price = Some(price),
                    Err(e) => log::warn!("Invalid forward price for {}: {e}", fp.instrument_id),
                }
                break;
            }
        }
        drop(cache);

        if let Some(price) = best_price {
            log::info!("Forward price for {series_id}: {price} (instant bootstrap)");
        } else {
            log::info!(
                "No matching forward price found for {series_id}, will bootstrap from live data",
            );
        }

        self.create_option_chain_manager(&cmd, best_price);
    }

    fn setup_book_updater(
        &mut self,
        instrument_id: &InstrumentId,
        book_type: BookType,
        only_deltas: bool,
        parent: Option<(Ustr, InstrumentClass)>,
    ) -> anyhow::Result<()> {
        // One BookUpdater per cache book (keyed by per-underlying id), shared
        // across overlapping subscriptions. Parent subs are expanded into
        // their underlyings here; the expansion is memoized so unsubscribe
        // mirrors the exact set even if the cache composition changes later.
        let target_ids: Vec<InstrumentId> = if let Some((root, class)) = parent {
            self.cache
                .borrow()
                .instruments_by_parent(&instrument_id.venue, &root, class)
                .iter()
                .map(|i| i.id())
                .collect()
        } else {
            vec![*instrument_id]
        };

        if parent.is_some() {
            self.book_deltas_parent_expansions
                .insert(*instrument_id, target_ids.clone());

            if !only_deltas {
                self.book_depth10_parent_expansions
                    .insert(*instrument_id, target_ids.clone());
            }
        }

        {
            let mut cache = self.cache.borrow_mut();
            for target_id in &target_ids {
                if !cache.has_order_book(target_id) {
                    let book = OrderBook::new(*target_id, book_type);
                    log::debug!("Created {book}");
                    cache.add_order_book(book)?;
                }
            }
        }

        for target_id in &target_ids {
            let updater = self
                .book_updaters
                .entry(*target_id)
                .or_insert_with(|| {
                    Rc::new(BookUpdater::new(
                        target_id,
                        self.cache.clone(),
                        self.config.emit_quotes_from_book,
                    ))
                })
                .clone();

            // Subscribe handler to the literal per-underlying topic. The
            // typed router dedups (pattern, handler_id) pairs, so overlapping
            // composite + exact subscriptions register exactly one handler
            // entry per book and a single delta apply per publish.
            let deltas_topic = switchboard::get_book_deltas_topic(*target_id);
            let deltas_handler = TypedHandler::new(updater.clone());
            msgbus::subscribe_book_deltas(
                deltas_topic.into(),
                deltas_handler,
                Some(self.msgbus_priority),
            );

            if !only_deltas {
                let depth_topic = switchboard::get_book_depth10_topic(*target_id);
                let depth_handler = TypedHandler::new(updater);
                msgbus::subscribe_book_depth10(
                    depth_topic.into(),
                    depth_handler,
                    Some(self.msgbus_priority),
                );
            }
        }

        Ok(())
    }

    fn is_underlying_wanted_for_deltas(&self, target_id: &InstrumentId) -> bool {
        // Any of {deltas, depth10, snapshots} subs causes setup_book_updater to
        // subscribe the deltas handler (depth10/snapshots use only_deltas=false),
        // so all three keep the per-underlying deltas handler alive.
        if self.book_deltas_subs.contains(target_id)
            || self.book_depth10_subs.contains(target_id)
            || self.has_book_snapshot_subscriptions(target_id)
        {
            return true;
        }
        self.book_deltas_parent_expansions
            .values()
            .any(|expansion| expansion.contains(target_id))
    }

    fn is_underlying_wanted_for_depth10(&self, target_id: &InstrumentId) -> bool {
        // Snapshots use only_deltas=false, so they drive the depth10 handler
        // as well as the deltas handler.
        if self.book_depth10_subs.contains(target_id)
            || self.has_book_snapshot_subscriptions(target_id)
        {
            return true;
        }
        self.book_depth10_parent_expansions
            .values()
            .any(|expansion| expansion.contains(target_id))
    }

    fn create_bar_aggregator(
        &self,
        instrument: &InstrumentAny,
        bar_type: BarType,
    ) -> Box<dyn BarAggregator> {
        let cache = self.cache.clone();
        let validate_sequence = self.config.validate_data_sequence;

        let handler = move |bar: Bar| {
            process_engine_bar(&cache, validate_sequence, true, bar);
        };

        let clock = self.clock.clone();
        let config = self.config.clone();

        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();

        if bar_type.spec().is_time_aggregated() {
            let time_bars_origin_offset = config
                .time_bars_origin_offset
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
                config.time_bars_build_delay,
                config.time_bars_skip_first_non_full_bar,
            ))
        } else {
            match bar_type.spec().aggregation {
                BarAggregation::Tick => Box::new(TickBarAggregator::new(
                    bar_type,
                    price_precision,
                    size_precision,
                    handler,
                )) as Box<dyn BarAggregator>,
                BarAggregation::TickImbalance => Box::new(TickImbalanceBarAggregator::new(
                    bar_type,
                    price_precision,
                    size_precision,
                    handler,
                )) as Box<dyn BarAggregator>,
                BarAggregation::TickRuns => Box::new(TickRunsBarAggregator::new(
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
                BarAggregation::VolumeImbalance => Box::new(VolumeImbalanceBarAggregator::new(
                    bar_type,
                    price_precision,
                    size_precision,
                    handler,
                )) as Box<dyn BarAggregator>,
                BarAggregation::VolumeRuns => Box::new(VolumeRunsBarAggregator::new(
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
                BarAggregation::ValueImbalance => Box::new(ValueImbalanceBarAggregator::new(
                    bar_type,
                    price_precision,
                    size_precision,
                    handler,
                )) as Box<dyn BarAggregator>,
                BarAggregation::ValueRuns => Box::new(ValueRunsBarAggregator::new(
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
                other => unreachable!(
                    "Unsupported internal bar aggregation dispatch for {other:?}; update `create_bar_aggregator`"
                ),
            }
        }
    }

    fn create_bar_aggregator_for_key(
        &mut self,
        bar_type: BarType,
        request_id: Option<UUID4>,
    ) -> anyhow::Result<()> {
        let key = bar_aggregator_key(bar_type, request_id);
        if self.bar_aggregators.contains_key(&key) {
            return Ok(());
        }

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
        let aggregator = self.create_bar_aggregator(&instrument, bar_type);
        self.bar_aggregators
            .insert(key, Rc::new(RefCell::new(aggregator)));

        Ok(())
    }

    fn start_bar_aggregator(
        &mut self,
        bar_type: BarType,
        request_id: Option<UUID4>,
    ) -> anyhow::Result<()> {
        let key = bar_aggregator_key(bar_type, request_id);
        let bar_type_std = bar_type.standard();

        self.create_bar_aggregator_for_key(bar_type, request_id)?;
        let aggregator = self
            .bar_aggregators
            .get(&key)
            .ok_or_else(|| anyhow::anyhow!("Cannot start bar aggregation for {bar_type}"))?
            .clone();
        let defer_live_activation = request_id.is_none()
            && aggregator.borrow().is_running()
            && !self.bar_aggregator_handlers.contains_key(&key);

        if !self.bar_aggregator_handlers.contains_key(&key) {
            // Subscribe to underlying data topics
            let mut subscriptions = Vec::new();

            if bar_type.is_composite() {
                let topic = switchboard::get_bars_topic(bar_type.composite());
                let handler = TypedHandler::new(BarBarHandler::new(&aggregator, bar_type_std));
                msgbus::subscribe_bars(topic.into(), handler.clone(), None);
                subscriptions.push(BarAggregatorSubscription::Bar { topic, handler });
            } else if bar_type.spec().price_type == PriceType::Last {
                let topic = switchboard::get_trades_topic(bar_type.instrument_id());
                let handler = TypedHandler::new(BarTradeHandler::new(&aggregator, bar_type_std));
                msgbus::subscribe_trades(
                    topic.into(),
                    handler.clone(),
                    Some(BAR_AGGREGATOR_PRIORITY),
                );
                subscriptions.push(BarAggregatorSubscription::Trade { topic, handler });
            } else {
                // Warn if imbalance/runs aggregation is wired to quotes (needs aggressor_side from trades)
                if matches!(
                    bar_type.spec().aggregation,
                    BarAggregation::TickImbalance
                        | BarAggregation::VolumeImbalance
                        | BarAggregation::ValueImbalance
                        | BarAggregation::TickRuns
                        | BarAggregation::VolumeRuns
                        | BarAggregation::ValueRuns
                ) {
                    log::warn!(
                        "Bar type {bar_type} uses imbalance/runs aggregation which requires trade \
                         data with `aggressor_side`, but `price_type` is not LAST so it will receive \
                         quote data: bars will not emit correctly",
                    );
                }

                let topic = switchboard::get_quotes_topic(bar_type.instrument_id());
                let handler = TypedHandler::new(BarQuoteHandler::new(&aggregator, bar_type_std));
                msgbus::subscribe_quotes(
                    topic.into(),
                    handler.clone(),
                    Some(BAR_AGGREGATOR_PRIORITY),
                );
                subscriptions.push(BarAggregatorSubscription::Quote { topic, handler });
            }

            self.bar_aggregator_handlers.insert(key, subscriptions);
        }

        if defer_live_activation {
            return Ok(());
        }

        // Setup time bar aggregator if needed (matches Cython _setup_bar_aggregator)
        self.setup_bar_aggregator(bar_type, false, request_id)?;

        aggregator.borrow_mut().set_is_running(true);

        Ok(())
    }

    /// Sets up a bar aggregator, matching Cython `_setup_bar_aggregator` logic.
    ///
    /// This method handles historical mode, message bus subscriptions, and time bar aggregator setup.
    fn setup_bar_aggregator(
        &self,
        bar_type: BarType,
        historical: bool,
        request_id: Option<UUID4>,
    ) -> anyhow::Result<()> {
        let key = bar_aggregator_key(bar_type, request_id);
        let aggregator = self.bar_aggregators.get(&key).ok_or_else(|| {
            anyhow::anyhow!("Cannot setup bar aggregator: no aggregator found for {bar_type}")
        })?;

        // Set historical mode and handler
        let cache = self.cache.clone();
        let validate_sequence = self.config.validate_data_sequence;
        let publish = !historical;
        let handler: Box<dyn FnMut(Bar)> = Box::new(move |bar: Bar| {
            process_engine_bar(&cache, validate_sequence, publish, bar);
        });

        aggregator
            .borrow_mut()
            .set_historical_mode(historical, handler);

        // For TimeBarAggregator, set clock and start timer
        if bar_type.spec().is_time_aggregated() {
            use nautilus_common::clock::TestClock;

            if historical {
                // Each aggregator gets its own independent clock
                let test_clock = Rc::new(RefCell::new(TestClock::new()));
                aggregator.borrow_mut().set_clock(test_clock);
                // Set weak reference for historical mode (start_timer called later from preprocess_historical_events)
                // Store weak reference so start_timer can use it when called later
                let aggregator_weak = Rc::downgrade(aggregator);
                aggregator.borrow_mut().set_aggregator_weak(aggregator_weak);
            } else {
                aggregator.borrow_mut().set_clock(self.clock.clone());
                aggregator
                    .borrow_mut()
                    .start_timer(Some(aggregator.clone()));
            }
        }

        Ok(())
    }

    fn stop_bar_aggregator(
        &mut self,
        bar_type: BarType,
        request_id: Option<UUID4>,
    ) -> anyhow::Result<()> {
        let key = bar_aggregator_key(bar_type, request_id);
        let aggregator = self.bar_aggregators.shift_remove(&key).ok_or_else(|| {
            anyhow::anyhow!("Cannot stop bar aggregator: no aggregator to stop for {bar_type}")
        })?;

        aggregator.borrow_mut().stop();

        // Unsubscribe any registered message handlers
        if let Some(subs) = self.bar_aggregator_handlers.remove(&key) {
            for sub in subs {
                match sub {
                    BarAggregatorSubscription::Bar { topic, handler } => {
                        msgbus::unsubscribe_bars(topic.into(), &handler);
                    }
                    BarAggregatorSubscription::Trade { topic, handler } => {
                        msgbus::unsubscribe_trades(topic.into(), &handler);
                    }
                    BarAggregatorSubscription::Quote { topic, handler } => {
                        msgbus::unsubscribe_quotes(topic.into(), &handler);
                    }
                }
            }
        }

        Ok(())
    }
}

// Resolves parent expansion components for a book subscription command.
//
// Returns Ok(Some((root, class))) when params carries PARAMS_IS_PARENT=true and
// the instrument_id parses as a recognised <root>.<class> shape; Ok(None) for
// concrete (non-parent) subscriptions; Err when the caller asserts a parent
// subscription but the id cannot be parsed, so subscribe entries can reject up
// front before touching state.
fn resolve_parent_components(
    instrument_id: &InstrumentId,
    params: Option<&Params>,
) -> anyhow::Result<Option<(Ustr, InstrumentClass)>> {
    if !is_parent_subscription(params) {
        return Ok(None);
    }
    let Some((root, class)) = instrument_id.parse_parent_components() else {
        anyhow::bail!(
            "Cannot expand parent subscription for {instrument_id}: \
             symbol does not parse as `<root>.<class>` with a recognised class suffix"
        );
    };
    Ok(Some((Ustr::from(root), class)))
}

fn spread_quote_update_interval_seconds(params: Option<&Params>) -> Option<u64> {
    match params.and_then(|params| params.get("update_interval_seconds")) {
        Some(value) if value.is_null() => None,
        Some(value) => value.as_u64().filter(|interval| *interval > 0),
        None => Some(1),
    }
}

const GENERIC_SPREAD_ID_SEPARATOR: &str = "___";

fn spread_instrument_legs(instrument: &InstrumentAny) -> Option<Vec<(InstrumentId, i64)>> {
    if !instrument.is_spread() {
        return None;
    }

    let instrument_id = instrument.id();
    let symbol = instrument_id.symbol.as_str();
    if !symbol.contains(GENERIC_SPREAD_ID_SEPARATOR) {
        return Some(vec![(instrument_id, 1)]);
    }

    symbol
        .split(GENERIC_SPREAD_ID_SEPARATOR)
        .map(|component| parse_spread_leg(component, instrument_id.venue))
        .collect()
}

fn parse_spread_leg(component: &str, venue: Venue) -> Option<(InstrumentId, i64)> {
    if let Some(rest) = component.strip_prefix("((") {
        let (ratio, symbol) = rest.split_once("))")?;
        return parse_spread_leg_parts(ratio, symbol, venue, -1);
    }

    let rest = component.strip_prefix('(')?;
    let (ratio, symbol) = rest.split_once(')')?;
    parse_spread_leg_parts(ratio, symbol, venue, 1)
}

fn parse_spread_leg_parts(
    ratio: &str,
    symbol: &str,
    venue: Venue,
    sign: i64,
) -> Option<(InstrumentId, i64)> {
    if symbol.is_empty() {
        return None;
    }

    let ratio = ratio.parse::<i64>().ok()?.checked_mul(sign)?;
    if ratio == 0 {
        return None;
    }

    Some((InstrumentId::new(Symbol::new(symbol), venue), ratio))
}

#[inline(always)]
fn log_error_on_cache_insert<T: Display>(e: &T) {
    log::error!("Error on cache insert: {e}");
}

// Top-of-book `QuoteTick` from an `OrderBookDepth10`. Returns `None` for
// `NoOrderSide` padding or zero size.
fn derive_quote_from_depth(depth: &OrderBookDepth10) -> Option<QuoteTick> {
    let bid = depth.bids.first()?;
    let ask = depth.asks.first()?;

    if bid.side == OrderSide::NoOrderSide
        || ask.side == OrderSide::NoOrderSide
        || bid.size.raw == 0
        || ask.size.raw == 0
    {
        return None;
    }

    Some(QuoteTick::new(
        depth.instrument_id,
        bid.price,
        ask.price,
        bid.size,
        ask.size,
        depth.ts_event,
        depth.ts_init,
    ))
}

// Validates a bar against `last_bar` before writing and (optionally) publishing.
// Shared by `handle_bar` and aggregator-emitted bars so both honour
// `validate_data_sequence`.
fn process_engine_bar(
    cache: &Rc<RefCell<Cache>>,
    validate_sequence: bool,
    publish: bool,
    bar: Bar,
) {
    if !validate_bar_sequence(cache, validate_sequence, &bar) {
        return;
    }

    if let Err(e) = cache.as_ref().borrow_mut().add_bar(bar) {
        log_error_on_cache_insert(&e);
    }

    if publish {
        let topic = switchboard::get_bars_topic(bar.bar_type);
        msgbus::publish_bar(topic, &bar);
    }
}

fn validate_bar_sequence(cache: &Rc<RefCell<Cache>>, validate_sequence: bool, bar: &Bar) -> bool {
    if !validate_sequence {
        return true;
    }

    let Some(last_bar) = cache.as_ref().borrow().bar(&bar.bar_type).copied() else {
        return true;
    };

    if bar.ts_event < last_bar.ts_event {
        log::warn!(
            "Bar {bar} was prior to last bar `ts_event` {}",
            last_bar.ts_event,
        );
        return false;
    }

    if bar.ts_init < last_bar.ts_init {
        log::warn!(
            "Bar {bar} was prior to last bar `ts_init` {}",
            last_bar.ts_init,
        );
        return false;
    }

    // Bar revision overwrite needs a `Bar.is_revision` field on the model;
    // not present today. Tracked under #8 in the data engine parity plan
    true
}

#[inline(always)]
fn log_if_empty_response<T, I: Display>(data: &[T], id: &I, correlation_id: &UUID4) -> bool {
    if data.is_empty() {
        let name = type_name::<T>();
        let short_name = name.rsplit("::").next().unwrap_or(name);
        log::warn!("Received empty {short_name} response for {id} {correlation_id}");
        return true;
    }
    false
}
