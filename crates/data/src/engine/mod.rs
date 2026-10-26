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
//! just need to override the `execute`, `process`, `send`, and `receive` methods.

pub mod bar;
pub mod book;
pub mod config;

#[cfg(feature = "defi")]
pub mod pool;

#[cfg(feature = "streaming")]
mod streaming;

mod commands;
mod dispatch;
mod futures;
mod handlers;
mod option_chain;
mod registry;
mod requests;
mod responses;
mod spread_quote;
mod synthetic;
mod time_range;

use std::{
    any::{Any, type_name},
    cell::RefCell,
    collections::VecDeque,
    fmt::{Debug, Display},
    mem,
    num::NonZeroUsize,
    rc::Rc,
    str::FromStr,
};

use ::futures::future::join_all;
use ahash::{AHashMap, AHashSet};
use anyhow::Context;
pub use bar::BarAggregatorSubscription;
use bar::{BarAggregationSubscription, BarAggregatorKey, bar_aggregator_key};
use book::{
    BookDeltasKey, BookSnapshotInfos, BookSnapshotKey, BookSnapshotSource, BookSnapshotter,
    BookSubscription, BookSubscriptionOwner, BookUpdater,
};
pub(crate) use commands::{DeferredCommand, DeferredCommandQueue};
use config::DataEngineConfig;
use dispatch::{log_error_on_cache_insert, process_engine_bar};
use futures::{ContinuousFutureRoller, ContinuousFutureSubscriptionState, datetime_to_unix_nanos};
use handlers::{
    BAR_AGGREGATOR_PRIORITY, BarBarHandler, BarQuoteHandler, BarTradeHandler, SpreadQuoteHandler,
};
use indexmap::IndexMap;
#[cfg(feature = "defi")]
use nautilus_common::messages::defi::PoolSnapshotResponse;
use nautilus_common::{
    cache::Cache,
    clock::Clock,
    logging::{RECV, RES},
    messages::data::{
        BarsResponse, BookDeltasResponse, BookDepthResponse, CustomDataResponse, DataCommand,
        DataRequestFailed, DataResponse, DataResponseEnvelope, IdentifierCountError,
        OptionChainReferencePriceResponse, PARAMS_IS_PARENT, QuoteRouting, RequestBars,
        RequestCommand, RequestJoin, RequestOptionChainReferencePrice, RequestQuotes,
        RequestTrades, SubscribeBars, SubscribeBookDeltas, SubscribeBookDepth, SubscribeCommand,
        SubscribeOptionChain, SubscribeOptionGreeks, SubscribeQuotes, SubscribeTrades,
        UnsubscribeBars, UnsubscribeBookDeltas, UnsubscribeBookDepth, UnsubscribeBookSnapshots,
        UnsubscribeCommand, UnsubscribeInstrumentStatus, UnsubscribeOptionChain,
        UnsubscribeOptionGreeks, UnsubscribeQuotes, UnsubscribeTrades, ensure_routable_bar_types,
        ensure_routable_instrument_ids, is_parent_subscription, quote_routing,
    },
    msgbus::{
        self, BusPayloadType, ShareableMessageHandler, TypedHandler, TypedIntoHandler,
        switchboard::{self, MessagingSwitchboard},
    },
    runner::get_data_cmd_sender,
    timer::{TimeEvent, TimeEventCallback},
};
use nautilus_core::{
    DurationNanos, Params, UUID4, UnixNanos, WeakCell,
    correctness::{FAILED, check_key_in_map, check_key_not_in_map, check_predicate_true},
    datetime::NANOSECONDS_IN_DAY,
};
#[cfg(feature = "defi")]
use nautilus_model::defi::DefiData;
use nautilus_model::{
    data::{
        Bar, BarType, CustomData, Data, DataRef, DataType, FundingRateUpdate, HasTsInit,
        IndexPriceUpdate, InstrumentClose, InstrumentStatus, MarkPriceUpdate, OrderBookDelta,
        OrderBookDeltas, OrderBookDepth, QuoteTick, TradeTick,
        option_chain::{OptionGreeks, StrikeRange},
    },
    enums::{
        AggregationSource, BarAggregation, BookType, InstrumentClass, MarketStatusAction,
        PriceType, RecordFlag,
    },
    identifiers::{
        ClientId, GENERIC_SPREAD_ID_SEPARATOR, InstrumentId, OptionSeriesId, Venue,
        parse_generic_spread_id_legs,
    },
    instruments::{Instrument, InstrumentAny, SyntheticInstrument},
    orderbook::OrderBook,
    types::{Price, Quantity},
};
use option_chain::{
    OptionChainBootstrapper, OptionChainGreeksBootstrap, PendingOptionChainRequest,
};
use requests::{
    ContinuousFutureRequest, ContinuousFutureRequestState, ContinuousFutureSegment,
    ContinuousFutureSource, RequestBarAggregation, continuous_future_request_from_bars,
    continuous_future_subscription_from_bars, has_continuous_future_params,
    request_bar_aggregation_from_params, request_params, response_params,
};
use responses::{
    log_if_empty_response, parent_request_window, rebind_response_correlation,
    rebuild_pipeline_response, same_instrument,
};
use serde_json::Value;
use spread_quote::SpreadQuoteState;
#[cfg(feature = "streaming")]
use streaming::CatalogMap;
use time_range::{
    TimeRangePipelineState, has_time_range_pipeline_params, is_time_range_pipeline_variant,
};
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
    subscription::{
        SubscriptionKey, SubscriptionRegistry, SubscriptionRelease, split_subscribe,
        split_unsubscribe,
    },
};

/// Provides a high-performance `DataEngine` for all environments.
#[derive(Debug)]
pub struct DataEngine {
    clock: Rc<RefCell<dyn Clock>>,
    cache: Rc<RefCell<Cache>>,
    config: DataEngineConfig,
    msgbus_priority: u32,
    subscriptions_external: SubscriptionRegistry<(ClientId, SubscriptionKey), SubscribeCommand>,
    clients: IndexMap<ClientId, DataClientAdapter>,
    default_client_id: Option<ClientId>,
    routing_map: IndexMap<Venue, ClientId>,
    book_intervals: AHashMap<NonZeroUsize, BookSnapshotInfos>,
    book_snapshot_counts: IndexMap<BookSnapshotKey, usize>,
    book_snapshot_sources: AHashMap<InstrumentId, BookSnapshotSource>,
    book_deltas_counts: IndexMap<BookDeltasKey, usize>,
    book_depth_counts: IndexMap<BookDeltasKey, usize>,
    book_updaters: AHashMap<InstrumentId, Rc<BookUpdater>>,
    book_subscriptions: AHashMap<InstrumentId, BookSubscription>,
    book_subscription_owners: AHashMap<InstrumentId, Vec<Rc<BookSubscriptionOwner>>>,
    book_snapshotters: AHashMap<NonZeroUsize, Rc<BookSnapshotter>>,
    bar_aggregators: IndexMap<BarAggregatorKey, Rc<RefCell<Box<dyn BarAggregator>>>>,
    bar_aggregator_handlers: AHashMap<BarAggregatorKey, Vec<BarAggregatorSubscription>>,
    subscriptions_bar_aggregation: AHashMap<BarType, BarAggregationSubscription>,
    request_bar_aggregations: AHashMap<UUID4, RequestBarAggregation>,
    inflight_requests: AHashMap<UUID4, RequestCommand>,
    request_pipeline_parent_request: AHashMap<UUID4, RequestCommand>,
    request_pipeline_n_components: AHashMap<UUID4, usize>,
    request_pipeline_parent_request_id: AHashMap<UUID4, (UUID4, usize)>,
    request_pipeline_responses: AHashMap<UUID4, Vec<Option<Vec<DataResponseEnvelope>>>>,
    time_range_pipeline_requests: AHashMap<UUID4, TimeRangePipelineState>,
    time_range_pipeline_parent_request_id: AHashMap<UUID4, UUID4>,
    parent_join_request_id: AHashMap<UUID4, UUID4>,
    pending_join_requests: AHashMap<UUID4, RequestJoin>,
    continuous_future_requests: AHashMap<UUID4, ContinuousFutureRequestState>,
    continuous_future_parent_request_id: AHashMap<UUID4, UUID4>,
    continuous_future_subscriptions: AHashMap<BarType, ContinuousFutureSubscriptionState>,
    continuous_future_roller: Option<Rc<ContinuousFutureRoller>>,
    spread_quote_states: AHashMap<InstrumentId, SpreadQuoteState>,
    option_chain_managers: AHashMap<OptionSeriesId, Rc<RefCell<OptionChainManager>>>,
    option_chain_instrument_index: AHashMap<InstrumentId, OptionSeriesId>,
    deferred_cmd_queue: DeferredCommandQueue,
    option_chain_bootstrapper: Option<Rc<OptionChainBootstrapper>>,
    pending_option_chain_requests: AHashMap<UUID4, PendingOptionChainRequest>,
    option_chain_greeks_bootstraps: AHashMap<OptionSeriesId, OptionChainGreeksBootstrap>,
    synthetic_quote_feeds: AHashMap<InstrumentId, Vec<SyntheticInstrument>>,
    synthetic_trade_feeds: AHashMap<InstrumentId, Vec<SyntheticInstrument>>,
    subscribed_synthetic_quotes: AHashMap<InstrumentId, usize>,
    subscribed_synthetic_trades: AHashMap<InstrumentId, usize>,
    buffered_deltas_map: AHashMap<InstrumentId, OrderBookDeltas>,
    deltas_frame: Vec<OrderBookDelta>,
    command_count: u64,
    data_count: u64,
    request_count: u64,
    response_count: u64,
    #[cfg(feature = "streaming")]
    catalogs: CatalogMap,
    #[cfg(feature = "defi")]
    pub(crate) pool_updaters: AHashMap<InstrumentId, Rc<PoolUpdater>>,
    #[cfg(feature = "defi")]
    pub(crate) pool_snapshot_pending: AHashMap<InstrumentId, UUID4>,
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
        if let Some(client_ids) = &config.external_clients {
            let mut cache = cache.borrow_mut();
            for client_id in client_ids {
                cache.add_external_data_client(*client_id);
            }
        }

        Self {
            clock,
            cache,
            config,
            msgbus_priority: 10, // High-priority for built-in component
            subscriptions_external: SubscriptionRegistry::default(),
            clients: IndexMap::new(),
            default_client_id: None,
            routing_map: IndexMap::new(),
            book_intervals: AHashMap::new(),
            book_snapshot_counts: IndexMap::new(),
            book_snapshot_sources: AHashMap::new(),
            book_deltas_counts: IndexMap::new(),
            book_depth_counts: IndexMap::new(),
            book_updaters: AHashMap::new(),
            book_subscriptions: AHashMap::new(),
            book_subscription_owners: AHashMap::new(),
            book_snapshotters: AHashMap::new(),
            bar_aggregators: IndexMap::new(),
            bar_aggregator_handlers: AHashMap::new(),
            subscriptions_bar_aggregation: AHashMap::new(),
            request_bar_aggregations: AHashMap::new(),
            inflight_requests: AHashMap::new(),
            request_pipeline_parent_request: AHashMap::new(),
            request_pipeline_n_components: AHashMap::new(),
            request_pipeline_parent_request_id: AHashMap::new(),
            request_pipeline_responses: AHashMap::new(),
            time_range_pipeline_requests: AHashMap::new(),
            time_range_pipeline_parent_request_id: AHashMap::new(),
            parent_join_request_id: AHashMap::new(),
            pending_join_requests: AHashMap::new(),
            continuous_future_requests: AHashMap::new(),
            continuous_future_parent_request_id: AHashMap::new(),
            continuous_future_subscriptions: AHashMap::new(),
            continuous_future_roller: None,
            spread_quote_states: AHashMap::new(),
            option_chain_managers: AHashMap::new(),
            option_chain_instrument_index: AHashMap::new(),
            deferred_cmd_queue: Rc::new(RefCell::new(VecDeque::new())),
            option_chain_bootstrapper: None,
            pending_option_chain_requests: AHashMap::new(),
            option_chain_greeks_bootstraps: AHashMap::new(),
            synthetic_quote_feeds: AHashMap::new(),
            synthetic_trade_feeds: AHashMap::new(),
            subscribed_synthetic_quotes: AHashMap::new(),
            subscribed_synthetic_trades: AHashMap::new(),
            buffered_deltas_map: AHashMap::new(),
            deltas_frame: Vec::new(),
            command_count: 0,
            data_count: 0,
            request_count: 0,
            response_count: 0,
            #[cfg(feature = "streaming")]
            catalogs: CatalogMap::new(),
            #[cfg(feature = "defi")]
            pool_updaters: AHashMap::new(),
            #[cfg(feature = "defi")]
            pool_snapshot_pending: AHashMap::new(),
            #[cfg(feature = "defi")]
            pool_event_buffers: AHashMap::new(),
        }
    }

    /// Returns a reference to the clock.
    #[must_use]
    pub fn clock(&self) -> &Rc<RefCell<dyn Clock>> {
        &self.clock
    }

    /// Returns a reference to the cache.
    #[must_use]
    pub fn cache(&self) -> &Rc<RefCell<Cache>> {
        &self.cache
    }

    /// Returns a reference to the configuration.
    #[must_use]
    pub const fn config(&self) -> &DataEngineConfig {
        &self.config
    }

    #[cfg(feature = "defi")]
    #[must_use]
    pub(crate) const fn msgbus_priority(&self) -> u32 {
        self.msgbus_priority
    }

    /// Registers all message bus handlers for the data engine.
    pub fn register_msgbus_handlers(engine: &Rc<RefCell<Self>>) {
        let weak = WeakCell::from(Rc::downgrade(engine));
        engine.borrow_mut().continuous_future_roller =
            Some(Rc::new(ContinuousFutureRoller::new(engine)));
        engine.borrow_mut().option_chain_bootstrapper =
            Some(Rc::new(OptionChainBootstrapper::new(engine)));

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
                get_data_cmd_sender().execute(cmd);
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

    /// Returns the number of request pipelines awaiting leg responses.
    #[must_use]
    pub fn request_pipeline_count(&self) -> usize {
        self.request_pipeline_parent_request.len()
    }

    /// Returns the number of time-range pipelines awaiting child responses.
    #[must_use]
    pub fn time_range_pipeline_count(&self) -> usize {
        self.time_range_pipeline_requests.len()
    }

    /// Returns the number of `RequestJoin` originals awaiting finalization.
    #[must_use]
    pub fn pending_join_request_count(&self) -> usize {
        self.pending_join_requests.len()
    }

    /// Starts all registered data clients and re-arms bar aggregator timers.
    pub fn start(&mut self) {
        for client in self.get_clients_mut() {
            if client.start().is_err() {
                log::error!("Failed to start data client {}", client.client_id());
            }
        }

        for ((_, request_id), aggregator) in &self.bar_aggregators {
            // Request-scoped or historical aggregators run on private clocks;
            // re-arming them here would perturb an in-flight request's timer state
            let is_subscription = request_id.is_none() && !aggregator.borrow().is_historical();
            if is_subscription && aggregator.borrow().bar_type().spec().is_time_aggregated() {
                aggregator
                    .borrow_mut()
                    .start_timer(Some(Rc::clone(aggregator)));
            }
        }

        for state in self.spread_quote_states.values() {
            state
                .aggregator
                .borrow_mut()
                .start_timer(Some(Rc::clone(&state.aggregator)));
        }
    }

    /// Stops all registered data clients and bar aggregator timers.
    pub fn stop(&mut self) {
        for client in self.get_clients_mut() {
            if client.stop().is_err() {
                log::error!("Failed to stop data client {}", client.client_id());
            }
        }

        for aggregator in self.bar_aggregators.values() {
            aggregator.borrow_mut().stop();
        }

        for state in self.spread_quote_states.values() {
            state.aggregator.borrow_mut().stop_timer();
        }
    }

    /// Resets all registered data clients and clears engine state.
    pub fn reset(&mut self) {
        for client in self.get_clients_mut() {
            match client.reset() {
                Ok(()) => client.clear_subscription_state(),
                Err(_) => log::error!("Failed to reset data client {}", client.client_id()),
            }
        }

        let keys: Vec<BarAggregatorKey> = self.bar_aggregators.keys().copied().collect();
        for (bar_type, request_id) in keys {
            if let Err(e) = self.stop_bar_aggregator(bar_type, request_id) {
                log::error!("Error stopping bar aggregator during reset for {bar_type}: {e}");
            }
        }
        self.subscriptions_bar_aggregation.clear();

        self.request_bar_aggregations.clear();
        self.inflight_requests.clear();
        self.request_pipeline_parent_request.clear();
        self.request_pipeline_n_components.clear();
        self.request_pipeline_parent_request_id.clear();
        self.request_pipeline_responses.clear();
        self.time_range_pipeline_requests.clear();
        self.time_range_pipeline_parent_request_id.clear();
        self.parent_join_request_id.clear();
        self.pending_join_requests.clear();
        self.continuous_future_requests.clear();
        self.continuous_future_parent_request_id.clear();

        for state in self.continuous_future_subscriptions.values_mut() {
            if let Some(name) = state.timer_name.take() {
                self.clock.borrow_mut().cancel_timer(&name);
            }
        }
        self.continuous_future_subscriptions.clear();

        let spread_ids: Vec<InstrumentId> = self.spread_quote_states.keys().copied().collect();
        for spread_id in spread_ids {
            self.stop_spread_quote_aggregation(spread_id);
        }

        // Tear down option chain managers to unregister their msgbus handlers
        let managers: Vec<_> = self.option_chain_managers.drain().collect();
        for (_, manager) in managers {
            manager.borrow_mut().teardown(&self.clock);
        }

        self.option_chain_instrument_index.clear();
        self.cancel_pending_option_chain_requests(None);
        self.clear_option_chain_greeks_bootstraps();

        // Unsubscribe BookUpdaters before dropping; otherwise the typed router
        // keeps dispatching to abandoned updaters. `book_updaters` is keyed by
        // per-underlying id, so the literal per-underlying topic is the same
        // string the subscribe path used.
        let book_updaters: Vec<(InstrumentId, Rc<BookUpdater>)> =
            self.book_updaters.drain().collect();
        for (instrument_id, updater) in book_updaters {
            let deltas_topic = switchboard::get_book_deltas_topic(instrument_id);
            let depth_topic = switchboard::get_book_depth_topic(instrument_id);
            let deltas_handler: TypedHandler<OrderBookDeltas> =
                TypedHandler::new(Rc::clone(&updater));
            let depth_handler: TypedHandler<OrderBookDepth> = TypedHandler::new(updater);
            msgbus::unsubscribe_book_deltas(deltas_topic.into(), &deltas_handler);
            msgbus::unsubscribe_book_depth(depth_topic.into(), &depth_handler);
        }

        self.book_subscriptions.clear();
        self.book_subscription_owners.clear();

        self.book_deltas_counts.clear();
        self.book_depth_counts.clear();
        self.book_intervals.clear();
        self.book_snapshot_counts.clear();
        self.book_snapshot_sources.clear();
        self.book_snapshotters.clear();
        self.buffered_deltas_map.clear();
        self.deltas_frame.clear();

        self.synthetic_quote_feeds.clear();
        self.synthetic_trade_feeds.clear();
        self.subscribed_synthetic_quotes.clear();
        self.subscribed_synthetic_trades.clear();
        self.subscriptions_external.clear();

        self.deferred_cmd_queue.borrow_mut().clear();

        #[cfg(feature = "defi")]
        self.clear_pool_updaters();

        self.clock.borrow_mut().cancel_timers();

        self.command_count = 0;
        self.data_count = 0;
        self.request_count = 0;
        self.response_count = 0;
    }

    /// Disposes the engine, stopping all clients and canceling any timers.
    pub fn dispose(&mut self) {
        for client in self.get_clients_mut() {
            if client.dispose().is_err() {
                log::error!("Failed to dispose data client {}", client.client_id());
            }
        }

        self.clear_option_chain_greeks_bootstraps();

        // Continuous-future source handlers live outside bar_aggregator_handlers,
        // so release them before dropping the aggregators
        let mut cf_sources = Vec::new();

        for state in self.continuous_future_subscriptions.values_mut() {
            if let Some(name) = state.timer_name.take() {
                self.clock.borrow_mut().cancel_timer(&name);
            }

            if let Some(subscription) = state.active_source_subscription.take() {
                cf_sources.push((state.target_bar_type, subscription));
            }
        }

        for (target_bar_type, subscription) in cf_sources {
            self.unsubscribe_continuous_future_source(target_bar_type, subscription);
        }
        self.continuous_future_subscriptions.clear();

        // Unsubscribe aggregator msgbus handlers so the typed routers don't keep
        // entries pointing at dropped aggregators
        let keys: Vec<BarAggregatorKey> = self.bar_aggregators.keys().copied().collect();
        for (bar_type, request_id) in keys {
            if let Err(e) = self.stop_bar_aggregator(bar_type, request_id) {
                log::error!("Error stopping bar aggregator during dispose for {bar_type}: {e}");
            }
        }

        self.subscriptions_external.clear();

        self.clock.borrow_mut().cancel_timers();
    }

    /// Connects all registered data clients concurrently.
    ///
    /// Connection failures are logged but do not prevent the node from running.
    /// Logs identify the client but omit provider error text, which can contain credentials.
    pub async fn connect(&mut self) {
        let futures: Vec<_> = self
            .get_clients_mut()
            .into_iter()
            .map(|client| async move {
                let client_id = client.client_id();
                (client_id, client.connect().await)
            })
            .collect();

        let results = join_all(futures).await;

        for (client_id, result) in results {
            if result.is_err() {
                log::error!("Failed to connect data client {client_id}: connection failed");
            }
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
            .map(|client| async move {
                let client_id = client.client_id();
                (client_id, client.disconnect().await)
            })
            .collect();

        let results = join_all(futures).await;

        // A closed session cannot answer, so discard its pending bootstraps
        #[cfg(feature = "defi")]
        self.abandon_pool_snapshots();

        let failed_clients: Vec<_> = results
            .into_iter()
            .filter_map(|(client_id, result)| result.err().map(|_| client_id.to_string()))
            .collect();

        if failed_clients.is_empty() {
            Ok(())
        } else {
            anyhow::bail!(
                "Failed to disconnect data clients: {}",
                failed_clients.join(", ")
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

    /// Returns all bar types currently subscribed across all clients,
    /// including internally aggregated subscriptions (v1 parity).
    #[must_use]
    pub fn subscribed_bars(&self) -> Vec<BarType> {
        let mut subscribed = self.collect_subscriptions(|client| &client.subscriptions_bars);
        subscribed.extend(
            self.bar_aggregators
                .keys()
                .filter(|(_, request_id)| request_id.is_none())
                .map(|(bar_type, _)| *bar_type),
        );
        subscribed
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
    /// This is the final synchronous dispatch point for data commands. Runtime command producers
    /// should send to `DataEngine.queue_execute`, which lets the runner sequence command execution
    /// before this method runs. The engine also calls this method for child commands generated while
    /// processing a parent command, where immediate in-engine ordering matters.
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

        let command_context = match &cmd {
            DataCommand::Subscribe(c) => (
                "Subscribe",
                c.client_id().copied(),
                c.venue().copied(),
                Some(c.command_id()),
            ),
            DataCommand::Unsubscribe(c) => (
                "Unsubscribe",
                c.client_id().copied(),
                c.venue().copied(),
                Some(c.command_id()),
            ),
            DataCommand::Request(c) => (
                "Request",
                c.client_id().copied(),
                c.venue().copied(),
                Some(*c.request_id()),
            ),
            #[cfg(feature = "defi")]
            DataCommand::DefiRequest(c) => (
                "DefiRequest",
                c.client_id().copied(),
                c.venue().copied(),
                Some(*c.request_id()),
            ),
            #[cfg(feature = "defi")]
            DataCommand::DefiSubscribe(c) => (
                "DefiSubscribe",
                c.client_id().copied(),
                c.venue().copied(),
                Some(c.command_id()),
            ),
            #[cfg(feature = "defi")]
            DataCommand::DefiUnsubscribe(c) => (
                "DefiUnsubscribe",
                c.client_id().copied(),
                c.venue().copied(),
                Some(c.command_id()),
            ),
            _ => ("Unknown", None, None, None),
        };

        let result = match cmd {
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
        };

        if let Err(e) = result {
            log::error!("Data command failed: {command_context:?}: {e}");
        }
    }

    /// Handles a subscribe command, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription is invalid (e.g., synthetic instrument for book data),
    /// or if the underlying client operation fails.
    pub fn execute_subscribe(&mut self, cmd: SubscribeCommand) -> anyhow::Result<()> {
        cmd.validate_routing(&self.cache.borrow())?;
        if let Some(client_id) = cmd.client_id().copied()
            && self.is_external_client(client_id)
        {
            self.subscribe_external(client_id, &cmd);
            return Ok(());
        }

        self.retain_intent(&cmd)?;

        let retained = cmd.clone();
        let (groups, result) = self.prepare_subscribe_groups(cmd);
        self.dispatch_subscribe_groups(&retained, groups);
        result
    }

    fn retain_intent(&mut self, cmd: &SubscribeCommand) -> anyhow::Result<()> {
        let retain_intent = match cmd {
            SubscribeCommand::Quotes(command) => {
                self.quote_routing(&command.instrument_ids, command.params.as_ref())?
                    == QuoteRouting::Client
            }
            SubscribeCommand::Trades(command) => !command.instrument_ids[0].is_synthetic(),
            _ => false,
        };

        if let Some(client) = self.get_command_client(cmd.client_id(), cmd.venue()) {
            if let SubscribeCommand::Bars(command) = cmd {
                let external: Vec<_> = command
                    .bar_types
                    .iter()
                    .copied()
                    .filter(BarType::is_externally_aggregated)
                    .collect();

                if !external.is_empty() {
                    let intent = SubscribeCommand::Bars(command.with_bar_types(external));
                    client.retain_intent(&intent);
                }
            } else if retain_intent {
                client.retain_intent(cmd);
            }
        }

        Ok(())
    }

    fn prepare_subscribe_groups(
        &mut self,
        cmd: SubscribeCommand,
    ) -> (Vec<SubscribeCommand>, anyhow::Result<()>) {
        let members = if matches!(
            &cmd,
            SubscribeCommand::Quotes(_) | SubscribeCommand::Trades(_)
        ) {
            vec![cmd]
        } else {
            split_subscribe(cmd)
        };

        let mut groups: IndexMap<Option<Value>, Vec<SubscribeCommand>> = IndexMap::new();
        let mut result = Ok(());

        for member in members {
            let prepared = match self.prepare_subscribe_member(&member) {
                Ok(Some(prepared)) => prepared,
                Ok(None) => continue,
                Err(e) => {
                    log::error!(
                        "Subscribe preparation failed: command={}, client={:?}, instrument_ids={:?}, bar_types={:?}: {e}",
                        member.command_id(),
                        member.client_id(),
                        member.instrument_ids(),
                        member.bar_types(),
                    );

                    if result.is_ok() {
                        result = Err(e);
                    }

                    continue;
                }
            };

            // Members share one parent; only catalog-inferred params can differ
            let params = prepared.params().map(|params| {
                Value::Object(params.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            });

            groups.entry(params).or_default().push(prepared);
        }

        let groups = groups
            .into_values()
            .map(|mut members| {
                if members.len() == 1 {
                    return members.pop().expect("nonempty group");
                }

                let first = &members[0];
                if first.instrument_ids().is_some() {
                    let ids: Vec<_> = members
                        .iter()
                        .flat_map(|member| member.instrument_ids().expect("same parent kind"))
                        .copied()
                        .collect();
                    first.with_instrument_ids(ids).expect("instrument command")
                } else if first.bar_types().is_some() {
                    let types: Vec<_> = members
                        .iter()
                        .flat_map(|member| member.bar_types().expect("same parent kind"))
                        .copied()
                        .collect();
                    first.with_bar_types(types).expect("bar command")
                } else {
                    first.clone()
                }
            })
            .collect();

        (groups, result)
    }

    fn dispatch_subscribe_groups(&mut self, cmd: &SubscribeCommand, groups: Vec<SubscribeCommand>) {
        for group in groups {
            let retained = if let Some(ids) = group.instrument_ids() {
                cmd.with_instrument_ids(ids).expect("instrument parent")
            } else if let Some(types) = group.bar_types() {
                cmd.with_bar_types(types).expect("bar parent")
            } else {
                cmd.clone()
            };

            let retain_on_failure = !matches!(
                &group,
                SubscribeCommand::BookDeltas(_) | SubscribeCommand::BookDepth(_)
            );

            if let Some(client) = self.get_command_client(group.client_id(), group.venue()) {
                client.execute_subscribe_with_retained(group, retained, retain_on_failure);
            } else {
                log::error!(
                    "Cannot handle command: no client found for client_id={:?}, venue={:?}",
                    group.client_id(),
                    group.venue()
                );
            }
        }
    }

    fn prepare_subscribe_member(
        &mut self,
        cmd: &SubscribeCommand,
    ) -> anyhow::Result<Option<SubscribeCommand>> {
        // Update internal engine state
        match cmd {
            SubscribeCommand::BookDeltas(book_cmd) => {
                if !self.subscribe_book_deltas(book_cmd)? && self.client_subscription_active(cmd) {
                    return Ok(None);
                }
            }
            SubscribeCommand::BookDepth(book_cmd) => {
                if !self.subscribe_book_depth(book_cmd)? && self.client_subscription_active(cmd) {
                    return Ok(None);
                }
            }
            SubscribeCommand::BookSnapshots(cmd) => {
                // Handles client forwarding internally (forwards as BookDeltas)
                self.subscribe_book_snapshots(cmd)?;
                return Ok(None);
            }
            SubscribeCommand::Bars(cmd) if has_continuous_future_params(cmd.params.as_ref()) => {
                self.subscribe_continuous_future_bars(cmd)?;
                return Ok(None);
            }
            SubscribeCommand::Bars(cmd) => {
                self.subscribe_bars(cmd)?;
                if cmd
                    .single_bar_type()
                    .expect("member preparation receives one bar type")
                    .is_internally_aggregated()
                {
                    return Ok(None);
                }
            }
            SubscribeCommand::OptionChain(cmd) if cmd.snapshot_interval_ms == Some(0) => {
                anyhow::bail!(
                    "Cannot subscribe option chain {} with a zero `snapshot_interval_ms`; use `None` for raw mode",
                    cmd.series_id,
                );
            }
            SubscribeCommand::OptionChain(cmd) => {
                self.subscribe_option_chain(cmd);
                return Ok(None);
            }
            SubscribeCommand::Quotes(cmd) if self.subscribe_quotes(cmd)? => {
                return Ok(None);
            }
            SubscribeCommand::Trades(cmd) if self.subscribe_trades(cmd) => {
                return Ok(None);
            }
            SubscribeCommand::Instrument(cmd)
                if cmd.instrument_ids.iter().any(InstrumentId::is_synthetic) =>
            {
                anyhow::bail!("Cannot subscribe for synthetic instrument `Instrument` data");
            }
            SubscribeCommand::InstrumentStatus(cmd)
                if cmd.instrument_ids.iter().any(InstrumentId::is_synthetic) =>
            {
                anyhow::bail!("Cannot subscribe for synthetic instrument `InstrumentStatus` data");
            }
            SubscribeCommand::InstrumentClose(cmd)
                if cmd.instrument_ids.iter().any(InstrumentId::is_synthetic) =>
            {
                anyhow::bail!("Cannot subscribe for synthetic instrument `InstrumentClose` data");
            }
            SubscribeCommand::OptionGreeks(cmd)
                if cmd.instrument_ids.iter().any(InstrumentId::is_synthetic) =>
            {
                anyhow::bail!("Cannot subscribe for synthetic instrument `OptionGreeks` data");
            }
            _ => {} // Do nothing else
        }

        let cmd = cmd.clone();
        #[cfg(feature = "streaming")]
        let cmd = self.subscribe_command_with_prefilled_start_ns(cmd)?;

        Ok(Some(cmd))
    }

    fn client_subscription_active(&mut self, cmd: &SubscribeCommand) -> bool {
        self.get_command_client(cmd.client_id(), cmd.venue())
            .is_some_and(|client| client.has_active_subscription(cmd))
    }

    /// Handles an unsubscribe command, updating internal state and forwarding to the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client operation fails.
    pub fn execute_unsubscribe(&mut self, cmd: &UnsubscribeCommand) -> anyhow::Result<()> {
        if let Some(bar_types) = cmd.bar_types() {
            ensure_routable_bar_types(bar_types, cmd.client_id().is_some())?;
        }

        if let Some(ids) = cmd.instrument_ids() {
            ensure_routable_instrument_ids(ids, cmd.client_id().is_some())?;
        }

        if let UnsubscribeCommand::Trades(trade) = cmd
            && !trade
                .instrument_ids
                .iter()
                .all(|id| id.is_synthetic() == trade.instrument_ids[0].is_synthetic())
        {
            anyhow::bail!("trade identifier lists cannot mix client and synthetic routes");
        }

        if let Some(client_id) = cmd.client_id().copied()
            && self.is_external_client(client_id)
        {
            self.unsubscribe_external(client_id, cmd);
            return Ok(());
        }

        if matches!(
            &cmd,
            UnsubscribeCommand::Instrument(_)
                | UnsubscribeCommand::InstrumentStatus(_)
                | UnsubscribeCommand::InstrumentClose(_)
                | UnsubscribeCommand::OptionGreeks(_)
                | UnsubscribeCommand::BookDeltas(_)
                | UnsubscribeCommand::BookDepth(_)
                | UnsubscribeCommand::BookSnapshots(_)
        ) && cmd
            .instrument_ids()
            .is_some_and(|ids| ids.len() > 1 && ids.iter().any(InstrumentId::is_synthetic))
        {
            anyhow::bail!("Cannot unsubscribe synthetic instruments for this data type");
        }

        let (prepared, result) = self.prepare_unsubscribe_members(cmd);

        for command in UnsubscribeCommand::group(prepared) {
            if let Some(client) = self.get_command_client(command.client_id(), command.venue()) {
                client.execute_unsubscribe(&command);
            } else {
                log::error!(
                    "Cannot handle command: no client found for client_id={:?}, venue={:?}",
                    command.client_id(),
                    command.venue()
                );
            }
        }

        result
    }

    fn prepare_unsubscribe_members(
        &mut self,
        cmd: &UnsubscribeCommand,
    ) -> (Vec<UnsubscribeCommand>, anyhow::Result<()>) {
        let members = if matches!(
            cmd,
            UnsubscribeCommand::Quotes(_) | UnsubscribeCommand::Trades(_)
        ) {
            vec![cmd.clone()]
        } else {
            split_unsubscribe(cmd)
        };

        let mut prepared = Vec::new();
        let mut result = Ok(());

        for member in members {
            match self.prepare_unsubscribe_member(&member) {
                Ok(true) => prepared.push(member),
                Ok(false) => {}
                Err(e) => {
                    log::error!(
                        "Unsubscribe preparation failed: command={}, client={:?}, instrument_ids={:?}, bar_types={:?}: {e}",
                        member.command_id(),
                        member.client_id(),
                        member.instrument_ids(),
                        member.bar_types(),
                    );

                    if result.is_ok() {
                        result = Err(e);
                    }
                }
            }
        }

        (prepared, result)
    }

    fn prepare_unsubscribe_member(&mut self, cmd: &UnsubscribeCommand) -> anyhow::Result<bool> {
        if matches!(
            cmd,
            UnsubscribeCommand::BookDeltas(_)
                | UnsubscribeCommand::BookDepth(_)
                | UnsubscribeCommand::BookSnapshots(_)
        ) && !self.release_book_subscription(cmd)
        {
            return Ok(false);
        }

        match &cmd {
            UnsubscribeCommand::BookDeltas(cmd) if !self.unsubscribe_book_deltas(cmd) => {
                return Ok(false);
            }
            UnsubscribeCommand::BookDepth(cmd) if !self.unsubscribe_book_depth(cmd) => {
                return Ok(false);
            }
            UnsubscribeCommand::BookSnapshots(cmd) => {
                // Handles client forwarding internally (forwards as BookDeltas)
                self.unsubscribe_book_snapshots(cmd);
                return Ok(false);
            }
            UnsubscribeCommand::Bars(cmd)
                if self.continuous_future_subscriptions.contains_key(
                    &cmd.single_bar_type()
                        .expect("member preparation receives one bar type")
                        .standard(),
                ) =>
            {
                // Don't tear down the chain while other actors remain subscribed
                let topic = switchboard::get_bars_topic(
                    cmd.single_bar_type()
                        .expect("member preparation receives one bar type")
                        .standard(),
                );

                if msgbus::exact_subscriber_count_bars(topic) == 0 {
                    self.unsubscribe_continuous_future_bars(cmd);
                }

                return Ok(false);
            }
            UnsubscribeCommand::Bars(cmd) => {
                self.unsubscribe_bars(cmd);
                if cmd
                    .single_bar_type()
                    .expect("member preparation receives one bar type")
                    .is_internally_aggregated()
                {
                    return Ok(false);
                }
            }
            UnsubscribeCommand::OptionChain(cmd) => {
                self.unsubscribe_option_chain(cmd);
                return Ok(false);
            }
            UnsubscribeCommand::Quotes(cmd) if self.unsubscribe_quotes(cmd)? => {
                return Ok(false);
            }
            UnsubscribeCommand::Trades(cmd) if self.unsubscribe_trades(cmd) => {
                return Ok(false);
            }
            UnsubscribeCommand::Instrument(cmd)
                if cmd.instrument_ids.iter().any(InstrumentId::is_synthetic) =>
            {
                anyhow::bail!("Cannot unsubscribe from synthetic instrument `Instrument` data");
            }
            UnsubscribeCommand::InstrumentStatus(cmd)
                if cmd.instrument_ids.iter().any(InstrumentId::is_synthetic) =>
            {
                anyhow::bail!(
                    "Cannot unsubscribe from synthetic instrument `InstrumentStatus` data"
                );
            }
            UnsubscribeCommand::InstrumentClose(cmd)
                if cmd.instrument_ids.iter().any(InstrumentId::is_synthetic) =>
            {
                anyhow::bail!(
                    "Cannot unsubscribe from synthetic instrument `InstrumentClose` data"
                );
            }
            UnsubscribeCommand::OptionGreeks(cmd)
                if cmd.instrument_ids.iter().any(InstrumentId::is_synthetic) =>
            {
                anyhow::bail!("Cannot unsubscribe from synthetic instrument `OptionGreeks` data");
            }
            _ => {}
        }

        Ok(true)
    }

    fn subscribe_external(&mut self, client_id: ClientId, cmd: &SubscribeCommand) {
        let dispatch = match cmd {
            SubscribeCommand::OptionChain(command) => {
                self.retain_external_option_chain(client_id, command, cmd);
                cmd.clone()
            }
            _ => {
                let mut acquired = false;
                let mut fresh_ids = Vec::new();
                let mut fresh_bar_types = Vec::new();

                for member in split_subscribe(cmd.clone()) {
                    let key = SubscriptionKey::from_subscribe(&member);

                    if !self.subscriptions_external.retain(
                        (client_id, key.clone()),
                        member.command_id(),
                        member,
                    ) {
                        continue;
                    }

                    acquired = true;

                    match key {
                        SubscriptionKey::Bars(bar_type) => fresh_bar_types.push(bar_type),
                        SubscriptionKey::Quotes(id)
                        | SubscriptionKey::Trades(id)
                        | SubscriptionKey::Instrument(id)
                        | SubscriptionKey::BookDeltas(id)
                        | SubscriptionKey::BookDepth(id)
                        | SubscriptionKey::MarkPrices(id)
                        | SubscriptionKey::IndexPrices(id)
                        | SubscriptionKey::FundingRates(id)
                        | SubscriptionKey::InstrumentStatus(id)
                        | SubscriptionKey::InstrumentClose(id)
                        | SubscriptionKey::OptionGreeks(id)
                        | SubscriptionKey::BookSnapshots(id, _) => fresh_ids.push(id),
                        _ => {}
                    }
                }

                if !acquired {
                    return;
                }

                if cmd.bar_types().is_some() {
                    cmd.with_bar_types(fresh_bar_types)
                        .expect("bar command has types")
                } else {
                    cmd.with_instrument_ids(fresh_ids)
                        .unwrap_or_else(|| cmd.clone())
                }
            }
        };

        register_external_streaming_type(&dispatch);
        publish_external_data_command(client_id, &dispatch);

        if self.config.debug {
            log::debug!("Skipping subscribe command for external client {client_id}: {cmd:?}");
        }
    }

    fn unsubscribe_external(&mut self, client_id: ClientId, cmd: &UnsubscribeCommand) {
        let mut releases = IndexMap::new();

        for member in split_unsubscribe(cmd) {
            let key = (client_id, SubscriptionKey::from_unsubscribe(&member));

            let (acquisition_id, command) = match self.subscriptions_external.release(&key) {
                SubscriptionRelease::Retained => continue,
                SubscriptionRelease::Final(subscribe) => (
                    subscribe.command_id(),
                    subscribe.into_unsubscribe(
                        cmd.command_id(),
                        cmd.ts_init(),
                        cmd.correlation_id(),
                    ),
                ),
                SubscriptionRelease::Untracked => (cmd.command_id(), member),
            };

            self.subscriptions_external.remove(&key);
            releases
                .entry(acquisition_id)
                .or_insert_with(Vec::new)
                .push(command);
        }

        for command in releases.into_values().flat_map(UnsubscribeCommand::group) {
            publish_external_data_command(client_id, &command);
        }

        if self.config.debug {
            log::debug!("Skipping unsubscribe command for external client {client_id}: {cmd:?}");
        }
    }

    fn subscribe_quotes(&mut self, cmd: &SubscribeQuotes) -> anyhow::Result<bool> {
        match self.quote_routing(&cmd.instrument_ids, cmd.params.as_ref())? {
            QuoteRouting::Client => Ok(false),
            QuoteRouting::Synthetic => {
                for id in &cmd.instrument_ids {
                    self.subscribe_synthetic_quotes(*id);
                }

                Ok(true)
            }
            QuoteRouting::Spread => {
                for id in &cmd.instrument_ids {
                    let member = cmd.with_instrument_ids(*id);
                    self.subscribe_spread_quotes(&member);
                }

                Ok(true)
            }
        }
    }

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<bool> {
        match self.quote_routing(&cmd.instrument_ids, cmd.params.as_ref())? {
            QuoteRouting::Client => Ok(false),
            QuoteRouting::Synthetic => {
                for id in &cmd.instrument_ids {
                    self.unsubscribe_synthetic_quotes(*id);
                }

                Ok(true)
            }
            QuoteRouting::Spread => {
                for id in &cmd.instrument_ids {
                    let member = cmd.with_instrument_ids(*id);
                    self.unsubscribe_spread_quotes(&member);
                }

                Ok(true)
            }
        }
    }

    fn subscribe_trades(&mut self, cmd: &SubscribeTrades) -> bool {
        if !cmd.instrument_ids[0].is_synthetic() {
            return false;
        }

        for id in &cmd.instrument_ids {
            self.subscribe_synthetic_trades(*id);
        }

        true
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> bool {
        if !cmd.instrument_ids[0].is_synthetic() {
            return false;
        }

        for id in &cmd.instrument_ids {
            self.unsubscribe_synthetic_trades(*id);
        }

        true
    }

    fn quote_routing(
        &self,
        ids: &[InstrumentId],
        params: Option<&Params>,
    ) -> anyhow::Result<QuoteRouting> {
        quote_routing(ids, None, params, &self.cache.borrow())
    }

    /// Sends a [`RequestCommand`] to a suitable data client implementation.
    ///
    /// Submission errors also send the requester a terminal failure envelope.
    ///
    /// # Errors
    ///
    /// Returns an error if validation fails, no client is found for the given client ID or venue,
    /// or the client rejects direct submission.
    pub fn execute_request(&mut self, req: RequestCommand) -> anyhow::Result<()> {
        let request_id = *req.request_id();
        let result = self.execute_request_inner(req);
        if let Err(e) = &result {
            self.process_response_envelope(DataResponseEnvelope::terminal(
                request_id,
                Some(e.to_string()),
            ));
        }

        result
    }

    fn execute_request_inner(&mut self, req: RequestCommand) -> anyhow::Result<()> {
        Self::validate_request(&req)?;

        // Skip requests for external clients
        if let Some(cid) = req.client_id()
            && self.is_external_client(*cid)
        {
            if self.config.debug {
                log::debug!("Skipping data request for external client {cid}: {req:?}");
            }
            return Ok(());
        }

        if let RequestCommand::Join(join) = req {
            return self.handle_request_join(join);
        }

        if has_continuous_future_params(request_params(&req)) {
            return self.execute_continuous_future_request(req);
        }

        let request_id = *req.request_id();
        self.prepare_request_bar_aggregators(&req)?;

        if has_time_range_pipeline_params(request_params(&req))
            && is_time_range_pipeline_variant(&req)
        {
            let result = self.execute_time_range_pipeline_request(req);
            if result.is_err() {
                self.cleanup_request_bar_aggregators(&request_id);
            }
            return result;
        }

        #[cfg(feature = "streaming")]
        if self.catalogs_registered() && streaming::is_date_range_variant(&req) {
            let result = self.dispatch_date_range_request(req);
            if result.is_err() {
                self.cleanup_request_bar_aggregators(&request_id);
            }
            return result;
        }

        let result = self.dispatch_request_to_client(req);

        if result.is_err() {
            self.cleanup_request_bar_aggregators(&request_id);
        }

        result.map(|_| ())
    }

    fn validate_request(req: &RequestCommand) -> anyhow::Result<()> {
        let (instrument_ids, requires_single, limit) = match req {
            RequestCommand::Instrument(cmd) => (&cmd.instrument_ids, false, None),
            RequestCommand::BookSnapshot(cmd) => (&cmd.instrument_ids, true, None),
            RequestCommand::BookDeltas(cmd) => (&cmd.instrument_ids, true, None),
            RequestCommand::BookDepth(cmd) => (&cmd.instrument_ids, true, None),
            RequestCommand::Quotes(cmd) => (&cmd.instrument_ids, false, cmd.limit),
            RequestCommand::Trades(cmd) => (&cmd.instrument_ids, false, cmd.limit),
            RequestCommand::FundingRates(cmd) => (&cmd.instrument_ids, true, None),
            RequestCommand::Bars(cmd) => {
                ensure_routable_bar_types(&cmd.bar_types, cmd.client_id.is_some())?;
                cmd.single_bar_type()?;
                return Ok(());
            }
            _ => return Ok(()),
        };

        ensure_routable_instrument_ids(instrument_ids, req.client_id().is_some())?;
        anyhow::ensure!(
            !requires_single || instrument_ids.len() == 1,
            "request requires one instrument ID for this data type, received {}",
            instrument_ids.len(),
        );
        anyhow::ensure!(
            instrument_ids.len() == 1 || limit.is_none(),
            "A record limit is unsupported for multi-instrument requests",
        );
        Ok(())
    }

    pub(super) fn dispatch_request_to_client(
        &mut self,
        req: RequestCommand,
    ) -> anyhow::Result<ClientId> {
        self.inflight_requests
            .insert(*req.request_id(), req.clone());
        let client_id = req.client_id().copied();
        let venue = req.venue().copied();
        let Some(client) = self.get_client(client_id.as_ref(), venue.as_ref()) else {
            anyhow::bail!("Cannot handle request: no client found for {client_id:?} {venue:?}");
        };
        let resolved_client_id = client.client_id();

        #[rustfmt::skip]
        let result = match req {
            RequestCommand::Data(req) => client.request_data(req),
            RequestCommand::Instrument(req) => client.request_instrument(req),
            RequestCommand::Instruments(req) => client.request_instruments(req),
            RequestCommand::BookSnapshot(req) => client.request_book_snapshot(req),
            RequestCommand::BookDeltas(req) => client.request_book_deltas(req),
            RequestCommand::BookDepth(req) => client.request_book_depth(req),
            RequestCommand::Quotes(req) => client.request_quotes(req),
            RequestCommand::Trades(req) => client.request_trades(req),
            RequestCommand::FundingRates(req) => client.request_funding_rates(req),
            RequestCommand::OptionChainReferencePrice(req) => client.request_option_chain_reference_price(req),
            RequestCommand::Bars(req) => client.request_bars(req),
            RequestCommand::Join(_) => anyhow::bail!("RequestJoin must be handled by handle_request_join"),
        };

        result.map_err(|e| {
            let context = format!("Data client {resolved_client_id} request failed");

            let context = if let Some(rejection) = e.downcast_ref::<IdentifierCountError>() {
                format!("{context}: {rejection}")
            } else {
                context
            };

            e.context(context)
        })?;

        Ok(resolved_client_id)
    }

    /// Processes a dynamically-typed data message.
    ///
    /// Supports instruments, response envelopes, request failures, funding rates, option greeks,
    /// instrument status, and custom data; unrecognized types are logged as errors.
    pub fn process(&mut self, data: &dyn Any) {
        if let Some(response) = data.downcast_ref::<DataResponseEnvelope>() {
            self.response_envelope(response.clone());
            return;
        }

        if let Some(failure) = data.downcast_ref::<DataRequestFailed>() {
            self.request_failed(failure.clone());
            return;
        }

        self.data_count += 1;
        // Dynamically-typed entry point: `FundingRateUpdate`, `OptionGreeks`, `InstrumentStatus`,
        // and custom data are also `Data` enum variants handled in `process_data`, but can arrive
        // here as typed data, whereas `InstrumentAny` is not a `Data` variant.
        if let Some(instrument) = data.downcast_ref::<InstrumentAny>() {
            self.handle_instrument(instrument);
        } else if let Some(funding_rate) = data.downcast_ref::<FundingRateUpdate>() {
            self.handle_funding_rate(*funding_rate);
        } else if let Some(option_greeks) = data.downcast_ref::<OptionGreeks>() {
            self.cache.borrow_mut().add_option_greeks(*option_greeks);
            self.feed_option_greeks_to_pre_bootstrap_chain(option_greeks);
            let topic = switchboard::get_option_greeks_topic(option_greeks.instrument_id);
            msgbus::publish_option_greeks(topic, option_greeks);
            self.drain_deferred_commands();
        } else if let Some(status) = data.downcast_ref::<InstrumentStatus>() {
            self.handle_instrument_status(*status);
        } else if let Some(custom) = data.downcast_ref::<CustomData>() {
            self.handle_custom_data(custom);
        } else {
            #[cfg(feature = "defi")]
            if let Some(response) = data.downcast_ref::<PoolSnapshotResponse>() {
                self.handle_pool_snapshot_response(response);
                return;
            }

            log::error!("Cannot process data {data:?}, type is unrecognized");
        }
    }

    /// Processes a `Data` enum instance, dispatching to live handlers.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "callers hand over ownership; the payload is only moved when a DeFi handler consumes it"
    )]
    pub fn process_data(&mut self, data: Data) {
        #[cfg(feature = "defi")]
        let data = match data {
            Data::Defi(defi) => {
                self.process_defi_data(*defi);
                return;
            }
            data => data,
        };

        self.process_data_ref(DataRef::from(&data));
    }

    /// Processes a borrowed `Data` enum view, dispatching to live handlers.
    ///
    /// DeFi payloads are cloned because the DeFi handler may buffer them while a pool snapshot
    /// is pending; no other variant clones its payload.
    pub fn process_data_ref(&mut self, data: DataRef<'_>) {
        #[cfg(feature = "defi")]
        let data = match data {
            DataRef::Defi(defi) => {
                self.process_defi_data(defi.clone());
                return;
            }
            data => data,
        };

        self.data_count += 1;

        match data {
            DataRef::Instrument(instrument) => self.handle_instrument(instrument),
            DataRef::BookDelta(delta) => self.handle_delta(*delta),
            DataRef::BookDeltas(deltas) => self.handle_deltas(deltas),
            DataRef::BookDepth(depth) => self.handle_depth(depth),
            DataRef::Quote(quote) => {
                self.handle_quote(*quote);
                self.drain_deferred_commands();
            }
            DataRef::Trade(trade) => self.handle_trade(*trade),
            DataRef::Bar(bar) => self.handle_bar(*bar),
            DataRef::MarkPrice(mark_price) => {
                self.handle_mark_price(*mark_price);
                self.drain_deferred_commands();
            }
            DataRef::IndexPrice(index_price) => {
                self.handle_index_price(*index_price);
                self.drain_deferred_commands();
            }
            DataRef::FundingRate(funding_rate) => {
                self.handle_funding_rate(*funding_rate);
                self.drain_deferred_commands();
            }
            DataRef::OptionGreeks(greeks) => {
                self.cache.borrow_mut().add_option_greeks(*greeks);
                self.feed_option_greeks_to_pre_bootstrap_chain(greeks);
                let topic = switchboard::get_option_greeks_topic(greeks.instrument_id);
                msgbus::publish_option_greeks(topic, greeks);
                self.drain_deferred_commands();
            }
            DataRef::InstrumentStatus(status) => {
                self.handle_instrument_status(*status);
                self.drain_deferred_commands();
            }
            DataRef::InstrumentClose(close) => self.handle_instrument_close(*close),
            DataRef::Custom(custom) => self.handle_custom_data(custom),
            #[cfg(feature = "defi")]
            DataRef::Defi(_) => unreachable!("handled before market data dispatch"),
            #[cfg(not(feature = "defi"))]
            #[allow(
                unreachable_patterns,
                reason = "DeFi variants can exist without this crate's defi feature"
            )]
            other => log_defi_data_dropped(other),
        }
    }

    /// Processes a `Data` instance through the pipeline bus path.
    ///
    /// Pipeline mode publishes each item on the `data.pipeline.` topic family and gates cache
    /// writes on `disable_historical_cache`. None of the live-only side effects (synthetic
    /// republish, option-chain expiry, depth-derived quotes, deferred-command drains) run in this
    /// path.
    pub fn process_pipeline(&mut self, data: Data) {
        #[cfg(feature = "defi")]
        let data = match data {
            Data::Defi(defi) => {
                self.process_defi_data(*defi);
                return;
            }
            data => data,
        };

        self.data_count += 1;

        match data {
            Data::Instrument(instrument) => self.handle_instrument(&instrument),
            Data::BookDelta(delta) => self.handle_delta_pipeline(delta),
            Data::BookDeltas(deltas) => self.handle_deltas_pipeline(&deltas),
            Data::BookDepth(depth) => self.handle_depth_pipeline(&depth),
            Data::Quote(quote) => self.handle_quote_pipeline(quote),
            Data::Trade(trade) => self.handle_trade_pipeline(trade),
            Data::Bar(bar) => self.handle_bar_pipeline(bar),
            Data::MarkPrice(mark_price) => self.handle_mark_price_pipeline(mark_price),
            Data::IndexPrice(index_price) => self.handle_index_price_pipeline(index_price),
            Data::FundingRate(funding_rate) => {
                self.handle_funding_rate_pipeline(funding_rate);
            }
            Data::OptionGreeks(greeks) => self.handle_option_greeks_pipeline(greeks),
            Data::InstrumentStatus(status) => self.handle_instrument_status_pipeline(status),
            Data::InstrumentClose(close) => self.handle_instrument_close_pipeline(close),
            Data::Custom(custom) => self.handle_custom_data_pipeline(&custom),
            #[cfg(feature = "defi")]
            Data::Defi(_) => unreachable!("handled before market data dispatch"),
            #[cfg(not(feature = "defi"))]
            #[allow(
                unreachable_patterns,
                reason = "DeFi variants can exist without this crate's defi feature"
            )]
            other => log_defi_data_dropped(DataRef::from(&other)),
        }
    }

    /// Processes a payload-only reply as a completed request.
    ///
    /// Payload-only replies do not certify coverage for catalog write-back. Use
    /// [`Self::response_envelope`] to report explicit source completion.
    pub fn response(&mut self, mut resp: DataResponse) {
        resp.trim_to_bounds();
        let request = self.inflight_requests.remove(resp.correlation_id());
        #[cfg(feature = "streaming")]
        if request
            .as_ref()
            .and_then(request_params)
            .is_some_and(|params| params.get_bool(streaming::PARAM_UPDATE_CATALOG) == Some(true))
        {
            log::warn!(
                "Catalog write-back requires an explicit completed response envelope, request={}",
                resp.correlation_id()
            );
        }

        #[cfg(not(feature = "streaming"))]
        let _ = request;
        let envelope = DataResponseEnvelope::new(resp, true, None);
        self.process_response_envelope(envelope);
    }

    /// Completes failed adapter work while preserving any returned rows.
    pub fn request_failed(&mut self, failure: DataRequestFailed) {
        log::error!(
            "Data request {} failed: {}",
            failure.request_id,
            failure.reason
        );

        let envelope = if let Some(mut response) = failure.response {
            response.trim_to_bounds();
            DataResponseEnvelope::new(response, true, None)
                .with_correlation_id(failure.request_id)
                .with_failure(failure.reason)
        } else {
            DataResponseEnvelope::terminal(failure.request_id, Some(failure.reason))
        };

        self.process_response_envelope(envelope);
    }

    /// Processes an adapter envelope, retaining intermediate request handlers.
    pub fn response_envelope(&mut self, mut envelope: DataResponseEnvelope) {
        if let Some(response) = envelope.response_mut() {
            response.trim_to_bounds();
        }

        self.process_response_envelope(envelope);
    }

    fn process_response_envelope(&mut self, envelope: DataResponseEnvelope) {
        let request = self.inflight_requests.remove(envelope.correlation_id());
        #[cfg(feature = "streaming")]
        if envelope.completed()
            && envelope.failure_reason().is_none()
            && let (Some(request), Some(response)) = (&request, envelope.response())
        {
            self.update_catalog_from_response(response, request);
        }

        #[cfg(not(feature = "streaming"))]
        let _ = request;

        let Some(envelope) = self.handle_request_pipeline_response(envelope) else {
            return;
        };

        if self
            .request_pipeline_parent_request_id
            .contains_key(envelope.correlation_id())
        {
            self.process_response_envelope(envelope);
            return;
        }

        if self.handle_continuous_future_child_response(&envelope)
            || self.handle_time_range_pipeline_child_response(&envelope)
        {
            return;
        }

        let Some(resp) = envelope.response().cloned() else {
            self.finish_empty_response(&envelope);
            return;
        };

        self.route_response_payload(resp, &envelope);
    }

    fn finish_empty_response(&mut self, envelope: &DataResponseEnvelope) {
        if let Some(original_id) = self
            .parent_join_request_id
            .remove(envelope.correlation_id())
        {
            if let Some(original) = self.pending_join_requests.remove(&original_id) {
                for child_id in original.request_ids {
                    msgbus::send_response_envelope(&DataResponseEnvelope::terminal(
                        child_id,
                        envelope.failure_reason().map(str::to_owned),
                    ));
                }
            }

            self.process_response_envelope(DataResponseEnvelope::terminal(
                original_id,
                envelope.failure_reason().map(str::to_owned),
            ));
            return;
        }

        self.cleanup_request_bar_aggregators(envelope.correlation_id());
        msgbus::send_response_envelope(envelope);
    }

    fn route_response_payload(&mut self, resp: DataResponse, envelope: &DataResponseEnvelope) {
        let resp = rebind_response_correlation(resp, *envelope.correlation_id());
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

        if self
            .parent_join_request_id
            .contains_key(resp.correlation_id())
        {
            self.finalize_request_join(resp, envelope.failure_reason().map(str::to_owned));
            return;
        }

        if !self.handle_response_payload(&resp) {
            return;
        }

        self.process_request_bar_aggregation_response(&resp, envelope.completed());

        msgbus::send_response_envelope(envelope);
    }

    fn handle_response_payload(&mut self, resp: &DataResponse) -> bool {
        let correlation_id = *resp.correlation_id();

        match resp {
            DataResponse::Instrument(r) => {
                self.handle_instrument_response(r.data.clone());
            }
            DataResponse::Instruments(r) => {
                self.handle_instruments(&r.data);
            }
            DataResponse::Quotes(r) => {
                if !log_if_empty_response(
                    &r.data,
                    &format_args!("{:?}", r.instrument_ids),
                    &correlation_id,
                ) {
                    self.handle_quotes(&r.data);
                }
            }
            DataResponse::Trades(r) => {
                if !log_if_empty_response(
                    &r.data,
                    &format_args!("{:?}", r.instrument_ids),
                    &correlation_id,
                ) {
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
            DataResponse::BookDeltas(r) => {
                if !log_if_empty_response(&r.data, &r.instrument_id, &correlation_id) {
                    self.handle_book_deltas_response(r);
                }
            }
            DataResponse::BookDepth(r) => {
                if !log_if_empty_response(&r.data, &r.instrument_id, &correlation_id) {
                    self.handle_book_depth_response(r);
                }
            }
            DataResponse::OptionChainReferencePrice(r) => {
                self.process_request_bar_aggregation_response(resp, true);
                self.handle_option_chain_reference_price_response(&correlation_id, r);
                return false;
            }
            DataResponse::Data(_) => {}
        }

        true
    }

    /// Registers a parent request whose response will be rebuilt from `n_components` leg responses.
    pub fn new_request_pipeline(&mut self, parent: RequestCommand, n_components: usize) {
        let parent_id = *parent.request_id();
        self.request_pipeline_n_components
            .insert(parent_id, n_components);
        self.request_pipeline_parent_request
            .insert(parent_id, parent);
        self.request_pipeline_responses
            .insert(parent_id, Vec::with_capacity(n_components));
    }

    /// Registers a leg `request_id` as a child of the pipeline keyed by `parent_id`.
    ///
    /// # Panics
    ///
    /// Panics if the parent pipeline is not registered.
    pub fn register_request_pipeline_leg(&mut self, leg_id: UUID4, parent_id: UUID4) {
        let slots = self
            .request_pipeline_responses
            .get_mut(&parent_id)
            .expect("pipeline registered before its legs");
        let index = slots.len();
        slots.push(None);
        self.request_pipeline_parent_request_id
            .insert(leg_id, (parent_id, index));
    }

    fn handle_request_pipeline_response(
        &mut self,
        envelope: DataResponseEnvelope,
    ) -> Option<DataResponseEnvelope> {
        let leg_id = *envelope.correlation_id();

        let Some(&(parent_id, index)) = self.request_pipeline_parent_request_id.get(&leg_id) else {
            return Some(envelope);
        };

        let completed = envelope.completed();
        let slots = self.request_pipeline_responses.get_mut(&parent_id)?;
        slots[index].get_or_insert_with(Vec::new).push(envelope);

        if !completed {
            return None;
        }

        self.request_pipeline_parent_request_id.remove(&leg_id);
        let expected = *self.request_pipeline_n_components.get(&parent_id)?;

        if slots
            .iter()
            .flatten()
            .filter(|slot| slot.last().is_some_and(DataResponseEnvelope::completed))
            .count()
            < expected
        {
            return None;
        }

        let slots = self.request_pipeline_responses.remove(&parent_id)?;
        self.request_pipeline_n_components.remove(&parent_id);
        let parent = self.request_pipeline_parent_request.remove(&parent_id);
        let failures = slots
            .iter()
            .flatten()
            .flatten()
            .filter_map(|value| value.failure_reason())
            .collect::<Vec<_>>();
        let failure_reason = (!failures.is_empty()).then(|| failures.join("; "));
        let legs = slots
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|value| value.response().cloned())
            .collect::<Vec<_>>();

        if legs.is_empty() {
            return Some(DataResponseEnvelope::terminal(parent_id, failure_reason));
        }

        let (parent_start, parent_end) = parent_request_window(parent.as_ref());
        if self.emit_partitioned_book_responses(parent_id, parent.as_ref(), &legs) {
            return Some(DataResponseEnvelope::terminal(parent_id, failure_reason));
        }

        let rebuilt = rebuild_pipeline_response(parent_id, parent.as_ref(), legs);

        let Some(mut rebuilt) = rebuilt else {
            let reason = failure_reason
                .unwrap_or_else(|| "Request pipeline could not rebuild its response".to_owned());

            return Some(DataResponseEnvelope::terminal(parent_id, Some(reason)));
        };

        if let DataResponse::BookDeltas(r) = &mut rebuilt {
            self.book_deltas_snapshot_replay(r);
        }

        // Client definitions can have a newer ts_init than the requested history
        if !matches!(parent, Some(RequestCommand::Instrument(_)))
            && (parent_start.is_some() || parent_end.is_some())
        {
            rebuilt.trim_to_bounds();
        }

        let result = DataResponseEnvelope::new(rebuilt, true, None);
        Some(if let Some(reason) = failure_reason {
            result.with_failure(reason)
        } else {
            result
        })
    }

    fn emit_partitioned_book_responses(
        &mut self,
        parent_id: UUID4,
        parent: Option<&RequestCommand>,
        legs: &[DataResponse],
    ) -> bool {
        if !matches!(parent, Some(RequestCommand::BookDeltas(_))) {
            return false;
        }

        let mut ids = legs.iter().filter_map(|r| match r {
            DataResponse::BookDeltas(r) => Some(r.instrument_id),
            _ => None,
        });

        let Some(first) = ids.next() else {
            return false;
        };

        if !ids.any(|id| !same_instrument(id, first)) {
            return false;
        }

        let (start, end) = parent_request_window(parent);

        for response in legs {
            let child_id = *response.correlation_id();
            let mut response = rebind_response_correlation(response.clone(), parent_id);
            if let DataResponse::BookDeltas(r) = &mut response {
                r.start = start.or(r.start);
                r.end = end.or(r.end);
                self.book_deltas_snapshot_replay(r);
            }

            response.trim_to_bounds();
            self.process_response_envelope(DataResponseEnvelope::new(
                response,
                false,
                Some(child_id),
            ));
        }

        true
    }

    fn handle_request_join(&mut self, req: RequestJoin) -> anyhow::Result<()> {
        if has_time_range_pipeline_params(req.params.as_ref()) {
            return self.execute_time_range_pipeline_request(RequestCommand::Join(req));
        }

        let now_ns = self.clock.borrow().timestamp_ns();
        let now_dt = now_ns.to_datetime_utc();
        let zero = jiff::Timestamp::UNIX_EPOCH;
        let start = req.start.unwrap_or(zero).min(now_dt);
        let end = req.end.unwrap_or(now_dt).min(now_dt);
        let dated = req.with_dates(Some(start), Some(end), now_ns);

        let original_id = req.request_id;
        let dated_id = dated.request_id;

        self.pending_join_requests.insert(original_id, req);
        self.parent_join_request_id.insert(dated_id, original_id);

        let leg_ids: Vec<UUID4> = dated.request_ids.clone();
        self.new_request_pipeline(RequestCommand::Join(dated), leg_ids.len());
        for leg_id in leg_ids {
            self.register_request_pipeline_leg(leg_id, dated_id);
        }

        Ok(())
    }

    fn finalize_request_join(&mut self, resp: DataResponse, failure_reason: Option<String>) {
        let dated_id = *resp.correlation_id();
        let Some(original_id) = self.parent_join_request_id.remove(&dated_id) else {
            log::error!("parent_join_request_id missing for dated correlation {dated_id}");
            return;
        };

        let Some(original) = self.pending_join_requests.remove(&original_id) else {
            log::error!("pending_join_requests missing for original {original_id}");
            return;
        };

        for leg_request_id in &original.request_ids {
            let terminal = DataResponseEnvelope::terminal(*leg_request_id, failure_reason.clone());
            msgbus::send_response_envelope(&terminal);
        }

        // Route the final join response through the normal response path so
        // bounds-trim against the parent window runs and the per-variant
        // handlers (cache writes, request bar aggregators) fire. The pipeline
        // and join staging maps for this request have already been popped, so
        // the recursive call cannot re-enter either gate.
        let final_resp = rebind_response_correlation(resp, original_id);
        let envelope = DataResponseEnvelope::new(final_resp, true, None);
        self.process_response_envelope(if let Some(reason) = failure_reason {
            envelope.with_failure(reason)
        } else {
            envelope
        });
    }
}

fn register_external_streaming_type(cmd: &SubscribeCommand) {
    if let Some(payload_type) = streaming_payload_type(cmd) {
        msgbus::get_message_bus()
            .borrow_mut()
            .add_streaming_type(payload_type);
    }
}

fn publish_external_data_command<T>(client_id: ClientId, command: &T)
where
    T: Any,
{
    let topic = format!("commands.data.{client_id}");
    msgbus::publish_any(topic.into(), command);
}

#[rustfmt::skip]
fn streaming_payload_type(cmd: &SubscribeCommand) -> Option<BusPayloadType> {
    match cmd {
        SubscribeCommand::Data(cmd) => Some(BusPayloadType::Custom(Ustr::from(
            cmd.data_type.type_name(),
        ))),
        SubscribeCommand::Instrument(_) | SubscribeCommand::Instruments(_) => Some(BusPayloadType::Instrument),
        SubscribeCommand::BookDeltas(_) | SubscribeCommand::BookSnapshots(_) => Some(BusPayloadType::OrderBookDeltas),
        SubscribeCommand::BookDepth(_) => Some(BusPayloadType::OrderBookDepth),
        SubscribeCommand::Quotes(_) => Some(BusPayloadType::QuoteTick),
        SubscribeCommand::Trades(_) => Some(BusPayloadType::TradeTick),
        SubscribeCommand::Bars(_) => Some(BusPayloadType::Bar),
        SubscribeCommand::MarkPrices(_) => Some(BusPayloadType::MarkPriceUpdate),
        SubscribeCommand::IndexPrices(_) => Some(BusPayloadType::IndexPriceUpdate),
        SubscribeCommand::FundingRates(_) => Some(BusPayloadType::FundingRateUpdate),
        SubscribeCommand::OptionGreeks(_) => Some(BusPayloadType::OptionGreeks),
        SubscribeCommand::InstrumentStatus(_)
        | SubscribeCommand::InstrumentClose(_)
        | SubscribeCommand::OptionChain(_) => None,
    }
}

#[cfg(not(feature = "defi"))]
fn log_defi_data_dropped(data: DataRef<'_>) {
    log::error!("Cannot process data {data:?}, nautilus-data built without its `defi` feature");
}
