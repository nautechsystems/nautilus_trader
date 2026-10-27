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

//! Provides a unified data client that combines Databento's live streaming and historical data capabilities.
//!
//! This module implements a data client that manages connections to multiple Databento datasets,
//! handles live market data subscriptions, and provides access to historical data on demand.

use std::{
    path::PathBuf,
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use ahash::AHashMap;
use databento::{dbn, live::Subscription};
use indexmap::IndexMap;
use nautilus_common::{
    clients::{DataClient, execute_bar_subscriptions, execute_instrument_subscriptions},
    live::{runner::get_data_event_sender, sender::EventSender},
    messages::{
        DataEvent, DataResponse,
        data::{
            BarsResponse, BookDeltasResponse, BookDepthResponse, DataRequestFailed,
            DataResponseEnvelope, InstrumentResponse, InstrumentsResponse, IntoInstrumentIds,
            QuotesResponse, RequestBars, RequestBookDeltas, RequestBookDepth, RequestInstrument,
            RequestInstruments, RequestQuotes, RequestTrades, SubscribeBars, SubscribeBookDeltas,
            SubscribeBookDepth, SubscribeCustomData, SubscribeInstrument,
            SubscribeInstrumentStatus, SubscribeQuotes, SubscribeTrades, SubscriptionError,
            TradesResponse, UnsubscribeBars, UnsubscribeBookDeltas, UnsubscribeBookDepth,
            UnsubscribeCustomData, UnsubscribeInstrumentStatus, UnsubscribeQuotes,
            UnsubscribeTrades,
        },
    },
};
use nautilus_core::{
    AtomicMap, DurationNanos, Params, UnixNanos,
    datetime::datetime_to_unix_nanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::task::TaskGroup;
use nautilus_model::{
    data::{BarSpecification, BarType, CustomData, Data, DataType, custom::CustomDataTrait},
    enums::{BarAggregation, PriceType},
    identifiers::{ClientId, InstrumentId, Symbol, Venue},
    instruments::{Instrument, InstrumentAny},
};
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

use crate::{
    common::{Credential, DATABENTO_VENUE, ohlcv_schema_from_aggregation},
    historical::{DatabentoHistoricalClient, RangeQueryParams},
    live::{DatabentoFeedHandler, DatabentoMessage, HandlerCommand},
    loader::DatabentoDataLoader,
    symbology::{check_consistent_symbology, infer_symbology_type, instrument_id_to_symbol_string},
    types::{DatabentoImbalance, DatabentoStatistics, Dataset, PublisherId},
};

const PRICE_PRECISION_PARAM: &str = "price_precision";
const SCHEMA_PARAM: &str = "schema";
const MBP10_DEPTH: usize = 10;
const QUOTE_SCHEMAS: &[dbn::Schema] = &[
    dbn::Schema::Mbp1,
    dbn::Schema::Bbo1S,
    dbn::Schema::Bbo1M,
    dbn::Schema::Cmbp1,
    dbn::Schema::Cbbo1S,
    dbn::Schema::Cbbo1M,
    dbn::Schema::Tbbo,
    dbn::Schema::Tcbbo,
];
const TRADE_SCHEMAS: &[dbn::Schema] = &[
    dbn::Schema::Trades,
    dbn::Schema::Tbbo,
    dbn::Schema::Tcbbo,
    dbn::Schema::Mbp1,
    dbn::Schema::Cmbp1,
];

/// Configuration for the Databento data client.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.databento", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.databento")
)]
pub struct DatabentoDataClientConfig {
    /// Databento API credential.
    pub(crate) credential: Credential,
    /// Path to publishers.json file.
    pub publishers_filepath: PathBuf,
    /// Override for the Databento Historical API base URL.
    pub historical_base_url: Option<String>,
    /// Override for the Databento live gateway address.
    pub live_gateway_addr: Option<String>,
    /// Venue-to-dataset overrides applied on top of the publishers.json mappings.
    pub venue_dataset_map: IndexMap<String, String>,
    /// Whether to use exchange as venue for GLBX instruments.
    pub use_exchange_as_venue: bool,
    /// Whether to timestamp bars on close.
    pub bars_timestamp_on_close: bool,
    /// Reconnection timeout in minutes (None for infinite retries).
    pub reconnect_timeout_mins: Option<u64>,
}

#[cfg(feature = "python")]
nautilus_core::impl_pyo3_config_getters!(DatabentoDataClientConfig {
    publishers_filepath: PathBuf,
    historical_base_url: Option<String>,
    live_gateway_addr: Option<String>,
    use_exchange_as_venue: bool,
    bars_timestamp_on_close: bool,
    venue_dataset_map: IndexMap<String, String>,
});

impl DatabentoDataClientConfig {
    /// Creates a new [`DatabentoDataClientConfig`] instance.
    #[must_use]
    pub fn new(
        api_key: impl Into<String>,
        publishers_filepath: PathBuf,
        use_exchange_as_venue: bool,
        bars_timestamp_on_close: bool,
    ) -> Self {
        Self {
            credential: Credential::new(api_key),
            publishers_filepath,
            venue_dataset_map: IndexMap::new(),
            use_exchange_as_venue,
            bars_timestamp_on_close,
            reconnect_timeout_mins: Some(10), // Default: 10 minutes
            historical_base_url: None,
            live_gateway_addr: None,
        }
    }

    /// Returns the API key associated with this config.
    #[must_use]
    pub fn api_key(&self) -> &str {
        self.credential.api_key()
    }

    /// Returns a masked version of the API key for logging purposes.
    #[must_use]
    pub fn api_key_masked(&self) -> String {
        self.credential.api_key_masked()
    }
}

/// A Databento data client that combines live streaming and historical data functionality.
///
/// This client uses the existing `DatabentoFeedHandler` for live data subscriptions
/// and `DatabentoHistoricalClient` for historical data requests. It supports multiple
/// datasets simultaneously, with separate feed handlers per dataset.
#[cfg_attr(feature = "python", pyo3::pyclass)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.databento")
)]
#[derive(Debug)]
pub struct DatabentoDataClient {
    client_id: ClientId,
    config: DatabentoDataClientConfig,
    is_connected: AtomicBool,
    historical: DatabentoHistoricalClient,
    loader: DatabentoDataLoader,
    cmd_channels: Arc<Mutex<AHashMap<String, tokio::sync::mpsc::UnboundedSender<HandlerCommand>>>>,
    task_handles: TaskGroup,
    cancellation_token: CancellationToken,
    publisher_venue_map: Arc<IndexMap<PublisherId, Venue>>,
    symbol_venue_map: Arc<AtomicMap<Symbol, Venue>>,
    data_sender: EventSender<DataEvent>,
}

impl DatabentoDataClient {
    /// Creates a new [`DatabentoDataClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if client creation or publisher configuration loading fails.
    pub fn new(
        client_id: ClientId,
        config: DatabentoDataClientConfig,
        clock: &'static AtomicTime,
    ) -> anyhow::Result<Self> {
        let historical = match &config.historical_base_url {
            Some(base_url) => DatabentoHistoricalClient::new_with_base_url(
                config.credential.clone(),
                config.publishers_filepath.clone(),
                clock,
                config.use_exchange_as_venue,
                base_url,
            )?,
            None => DatabentoHistoricalClient::new(
                config.credential.clone(),
                config.publishers_filepath.clone(),
                clock,
                config.use_exchange_as_venue,
            )?,
        };

        // Create data loader for venue-to-dataset mapping
        let mut loader = DatabentoDataLoader::new(Some(config.publishers_filepath.clone()))?;
        for (venue, dataset) in &config.venue_dataset_map {
            loader.set_dataset_for_venue(
                Dataset::from(dataset.as_str()),
                Venue::from(venue.as_str()),
            );
        }

        // Load publisher configuration
        let file_content = std::fs::read_to_string(&config.publishers_filepath)?;
        let publishers_vec: Vec<crate::types::DatabentoPublisher> =
            serde_json::from_str(&file_content)?;

        let publisher_venue_map = publishers_vec
            .into_iter()
            .map(|p| (p.publisher_id, Venue::from(p.venue.as_str())))
            .collect::<IndexMap<u16, Venue>>();

        let data_sender = get_data_event_sender();

        let task_handles = TaskGroup::new();

        Ok(Self {
            client_id,
            config,
            is_connected: AtomicBool::new(false),
            historical,
            loader,
            cmd_channels: Arc::new(Mutex::new(AHashMap::new())),
            cancellation_token: task_handles.cancellation_token(),
            task_handles,
            publisher_venue_map: Arc::new(publisher_venue_map),
            symbol_venue_map: Arc::new(AtomicMap::new()),
            data_sender,
        })
    }

    /// Returns the API key associated with this client.
    #[must_use]
    pub fn api_key(&self) -> &str {
        self.config.api_key()
    }

    /// Returns a masked version of the API key for logging purposes.
    #[must_use]
    pub fn api_key_masked(&self) -> String {
        self.config.api_key_masked()
    }

    /// Gets the dataset for a given venue using the data loader.
    ///
    /// # Errors
    ///
    /// Returns an error if the venue-to-dataset mapping cannot be found.
    fn get_dataset_for_venue(&self, venue: Venue) -> anyhow::Result<String> {
        self.loader
            .get_dataset_for_venue(&venue)
            .map(ToString::to_string)
            .ok_or_else(|| anyhow::anyhow!("No dataset found for venue: {venue}"))
    }

    /// Gets or creates a feed handler for the specified dataset.
    fn get_or_create_feed_handler(&self, dataset: &str) -> bool {
        let mut channels = self.cmd_channels.lock();

        if !channels.contains_key(dataset) {
            log::debug!("Creating new feed handler for dataset: {dataset}");
            let cmd_tx = self.initialize_live_feed(dataset.to_string());
            channels.insert(dataset.to_string(), cmd_tx);

            log::debug!("Feed handler created for dataset: {dataset}, channel stored");
            return true;
        }

        false
    }

    fn send_subscription_to_dataset(
        &self,
        dataset: &str,
        price_precisions: impl IntoIterator<Item = (Symbol, u8)>,
        subscription: Subscription,
        start_after_subscribe: bool,
    ) -> anyhow::Result<()> {
        let tx = {
            let channels = self.cmd_channels.lock();
            channels
                .get(dataset)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("No feed handler found for dataset: {dataset}"))?
        };

        send_subscription_commands(
            &tx,
            dataset,
            price_precisions,
            subscription,
            start_after_subscribe,
        )
    }

    fn subscribe_schema(
        &self,
        instrument_id: InstrumentId,
        schema: dbn::Schema,
    ) -> anyhow::Result<()> {
        let symbol = instrument_id.symbol.to_string();
        // Records resolve to the underlying contract, so a continuous or parent symbol would
        // publish on topics that its subscriber does not listen on
        anyhow::ensure!(
            infer_symbology_type(&symbol) == dbn::SType::RawSymbol,
            "Unsupported symbol {symbol} for a Databento {schema} subscription, only raw symbols \
             are available (not continuous or parent symbols)",
        );

        let dataset = self.get_dataset_for_venue(instrument_id.venue)?;
        let start_after_subscribe = self.get_or_create_feed_handler(&dataset);

        self.symbol_venue_map
            .insert(instrument_id.symbol, instrument_id.venue);

        let subscription = Subscription::builder()
            .schema(schema)
            .symbols(symbol)
            .build();

        self.send_subscription_to_dataset(&dataset, None, subscription, start_after_subscribe)
    }

    fn send_close_to_active_feeds(&self) {
        let channels = self.cmd_channels.lock();
        for (dataset, tx) in channels.iter() {
            if let Err(e) = tx.send(HandlerCommand::Close) {
                log::warn!("Failed to send close command to dataset {dataset}: {e}");
            }
        }
    }

    fn clear_feed_channels(&self) {
        let mut channels = self.cmd_channels.lock();
        channels.clear();
    }

    fn abort_active_tasks(&self) {
        self.task_handles.begin_shutdown();
    }

    fn spawn_task<F>(&self, future: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        if let Err(e) = self.task_handles.spawn(future) {
            log::debug!("Skipping Databento task after shutdown began: {e}");
        }
    }

    /// Initializes the live feed handler for streaming data.
    fn initialize_live_feed(
        &self,
        dataset: String,
    ) -> tokio::sync::mpsc::UnboundedSender<HandlerCommand> {
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (msg_tx, msg_rx) = tokio::sync::mpsc::unbounded_channel();
        let feed_dataset = dataset.clone();
        let feed_channels = self.cmd_channels.clone();

        let mut feed_handler = DatabentoFeedHandler::new(
            self.config.credential.clone(),
            dataset,
            cmd_rx,
            msg_tx,
            (*self.publisher_venue_map).clone(),
            self.symbol_venue_map.clone(),
            self.config.use_exchange_as_venue,
            self.config.bars_timestamp_on_close,
            self.config.reconnect_timeout_mins,
        );

        if let Some(addr) = &self.config.live_gateway_addr {
            feed_handler = feed_handler.with_gateway_addr(addr.clone());
        }

        let feed_future = async move {
            // run() logs its terminal error and forwards it as `DatabentoMessage::Error`
            let _ = feed_handler.run().await;
            feed_channels.lock().remove(&feed_dataset);
        };

        let cancellation_token = self.cancellation_token.clone();
        let data_sender = self.data_sender.clone();

        // Spawn message processing task with cancellation support
        let msg_future = async move {
            let mut msg_rx = msg_rx;

            loop {
                tokio::select! {
                    msg = msg_rx.recv() => {
                        match msg {
                            Some(DatabentoMessage::Data(data)) => {
                                log::debug!("Received data: {data:?}");
                                if let Err(e) = data_sender.send(DataEvent::Data(data)) {
                                    log::error!("Failed to send data event: {e}");
                                }
                            }
                            Some(DatabentoMessage::Instrument(instrument)) => {
                                log::debug!("Received instrument definition: {}", instrument.id());
                                if let Err(e) = data_sender.send(DataEvent::Data(Data::Instrument(instrument))) {
                                    log::error!("Failed to send instrument: {e}");
                                }
                            }
                            Some(DatabentoMessage::Status(status)) => {
                                log::debug!("Received status: {status:?}");
                                if let Err(e) =
                                    data_sender.send(DataEvent::Data(Data::InstrumentStatus(status)))
                                {
                                    log::error!("Failed to send status data event: {e}");
                                }
                            }
                            Some(DatabentoMessage::Imbalance(imbalance)) => {
                                log::debug!("Received imbalance: {imbalance:?}");
                                let instrument_id = imbalance.instrument_id;
                                let data = Data::Custom(custom_data_for_instrument(Arc::new(imbalance), instrument_id));
                                if let Err(e) = data_sender.send(DataEvent::Data(data)) {
                                    log::error!("Failed to send imbalance data event: {e}");
                                }
                            }
                            Some(DatabentoMessage::Statistics(statistics)) => {
                                log::debug!("Received statistics: {statistics:?}");
                                let instrument_id = statistics.instrument_id;
                                let data = Data::Custom(custom_data_for_instrument(Arc::new(statistics), instrument_id));
                                if let Err(e) = data_sender.send(DataEvent::Data(data)) {
                                    log::error!("Failed to send statistics data event: {e}");
                                }
                            }
                            Some(DatabentoMessage::SubscriptionAck(ack)) => {
                                log::debug!("Received subscription ack: {}", ack.message);
                            }
                            Some(DatabentoMessage::Error(error)) => {
                                log::error!("Feed handler error: {error}");
                            }
                            Some(DatabentoMessage::Close) => {
                                log::debug!("Feed handler closed");
                                break;
                            }
                            None => {
                                log::debug!("Message channel closed");
                                break;
                            }
                        }
                    }
                    () = cancellation_token.cancelled() => {
                        log::debug!("Message processing cancelled");
                        break;
                    }
                }
            }
        };

        if let Err(e) = self.task_handles.spawn(feed_future) {
            log::warn!("Skipping Databento feed task after shutdown began: {e}");
        }

        if let Err(e) = self.task_handles.spawn(msg_future) {
            log::warn!("Skipping Databento message task after shutdown began: {e}");
        }

        cmd_tx
    }
}

#[async_trait::async_trait(?Send)]
impl DataClient for DatabentoDataClient {
    /// Returns the client identifier.
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    /// Returns the venue associated with this client (None for multi-venue clients).
    fn venue(&self) -> Option<Venue> {
        None
    }

    /// Starts the data client.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to start.
    fn start(&mut self) -> anyhow::Result<()> {
        log::debug!("Starting");
        Ok(())
    }

    /// Stops the data client and cancels all active subscriptions.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to stop cleanly.
    fn stop(&mut self) -> anyhow::Result<()> {
        log::debug!("Stopping");

        self.send_close_to_active_feeds();
        self.clear_feed_channels();
        self.cancellation_token.cancel();
        self.abort_active_tasks();
        self.is_connected.store(false, Ordering::Relaxed);

        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::debug!("Resetting");
        self.send_close_to_active_feeds();
        self.clear_feed_channels();
        self.abort_active_tasks();
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        log::debug!("Disposing");
        self.stop()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        log::debug!("Connecting...");

        if !self.task_handles.is_open() {
            self.task_handles
                .finish_shutdown(Duration::from_secs(1), Duration::from_secs(2))
                .await
                .map_err(|e| anyhow::anyhow!("Failed to terminate Databento tasks: {e}"))?;
            self.task_handles
                .start_generation()
                .map_err(|e| anyhow::anyhow!("Failed to start Databento task generation: {e}"))?;
            self.cancellation_token = self.task_handles.cancellation_token();
        }

        self.is_connected.store(true, Ordering::Relaxed);

        log::info!("Connected");
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        log::debug!("Disconnecting...");

        self.send_close_to_active_feeds();
        self.clear_feed_channels();
        self.task_handles.begin_shutdown();

        let tasks_result = self
            .task_handles
            .finish_shutdown(Duration::from_secs(1), Duration::from_secs(2))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to terminate Databento tasks: {e}"));

        self.is_connected.store(false, Ordering::Relaxed);

        log::info!("Disconnected");
        tasks_result
    }

    /// Returns whether the client is currently connected.
    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    /// Subscribes to instrument definition data for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    fn subscribe_instrument(&mut self, cmd: SubscribeInstrument) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&cmd.instrument_ids, |instrument_id| {
            let dataset = self.get_dataset_for_venue(instrument_id.venue)?;
            let start_after_subscribe = self.get_or_create_feed_handler(&dataset);

            self.symbol_venue_map
                .insert(instrument_id.symbol, instrument_id.venue);
            let symbol = instrument_id.symbol.to_string();

            let subscription = Subscription::builder()
                .schema(databento::dbn::Schema::Definition)
                .symbols(symbol)
                .build();

            self.send_subscription_to_dataset(&dataset, None, subscription, start_after_subscribe)?;

            Ok(())
        })
    }

    /// Subscribes to quote tick data for the specified instruments.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    fn subscribe_quotes(&mut self, cmd: SubscribeQuotes) -> anyhow::Result<()> {
        self.subscribe_batch(
            cmd.instrument_ids,
            cmd.params.as_ref(),
            dbn::Schema::Mbp1,
            QUOTE_SCHEMAS,
        )
    }

    /// Subscribes to trade tick data for the specified instruments.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()> {
        self.subscribe_batch(
            cmd.instrument_ids,
            cmd.params.as_ref(),
            dbn::Schema::Trades,
            TRADE_SCHEMAS,
        )
    }

    /// Subscribes to order book delta updates for the specified instruments.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    fn subscribe_book_deltas(&mut self, cmd: SubscribeBookDeltas) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&cmd.instrument_ids, |instrument_id| {
            let dataset = self.get_dataset_for_venue(instrument_id.venue)?;
            let start_after_subscribe = self.get_or_create_feed_handler(&dataset);

            self.symbol_venue_map
                .insert(instrument_id.symbol, instrument_id.venue);
            let symbol = instrument_id.symbol.to_string();

            let subscription = Subscription::builder()
                .schema(databento::dbn::Schema::Mbo) // Market by order for book deltas
                .symbols(symbol)
                .build();

            self.send_subscription_to_dataset(&dataset, None, subscription, start_after_subscribe)?;

            Ok(())
        })
    }

    /// Subscribes to `DatabentoStatistics` or `DatabentoImbalance` for the instrument given by the
    /// identifier of the data type.
    ///
    /// # Errors
    ///
    /// Returns an error if the data type is not supported, has metadata, has a missing or invalid
    /// instrument ID identifier, or the subscription request fails.
    fn subscribe(&mut self, cmd: SubscribeCustomData) -> anyhow::Result<()> {
        let schema = custom_data_schema(&cmd.data_type)?;
        let instrument_id = custom_data_instrument_id(&cmd.data_type)?;
        self.subscribe_schema(instrument_id, schema)
    }

    /// Subscribes to 10-level order book depth snapshots for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the requested depth is not 10, the symbol is not a raw symbol, or the
    /// subscription request fails.
    fn subscribe_book_depth(&mut self, cmd: SubscribeBookDepth) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&cmd.instrument_ids, |instrument_id| {
            let cmd = cmd.with_instrument_ids(instrument_id);
            if let Some(depth) = cmd.depth {
                anyhow::ensure!(
                    depth.get() == MBP10_DEPTH,
                    "Unsupported book depth {depth} for Databento `mbp-10`, only a depth of {MBP10_DEPTH} is available",
                );
            }

            self.subscribe_schema(instrument_id, dbn::Schema::Mbp10)
        })
    }

    /// Subscribes to OHLCV bars for the specified bar type.
    ///
    /// # Errors
    ///
    /// Returns an error if the bar type has no matching OHLCV schema or the subscription request
    /// fails.
    fn subscribe_bars(&mut self, cmd: SubscribeBars) -> anyhow::Result<()> {
        execute_bar_subscriptions(&cmd.bar_types, |bar_type| {
            let schema = ohlcv_schema(bar_type)?;
            self.subscribe_schema(bar_type.instrument_id(), schema)
        })
    }

    /// Subscribes to instrument status updates for the specified instruments.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    fn subscribe_instrument_status(
        &mut self,
        cmd: SubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&cmd.instrument_ids, |instrument_id| {
            let dataset = self.get_dataset_for_venue(instrument_id.venue)?;
            let start_after_subscribe = self.get_or_create_feed_handler(&dataset);

            self.symbol_venue_map
                .insert(instrument_id.symbol, instrument_id.venue);
            let symbol = instrument_id.symbol.to_string();

            let subscription = Subscription::builder()
                .schema(databento::dbn::Schema::Status)
                .symbols(symbol)
                .build();

            self.send_subscription_to_dataset(&dataset, None, subscription, start_after_subscribe)?;

            Ok(())
        })
    }

    // Unsubscribe methods
    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        log::debug!(
            "Databento does not support granular unsubscribing, ignoring quote unsubscribe request for {:?}",
            cmd.instrument_ids
        );
        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        // Note: Databento live API doesn't support granular unsubscribing.
        // The feed handler manages subscriptions and can handle reconnections
        // with the appropriate subscription state.
        log::debug!(
            "Databento does not support granular unsubscribing - ignoring unsubscribe request for {:?}",
            cmd.instrument_ids
        );

        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&cmd.instrument_ids, |instrument_id| {
            // Note: Databento live API doesn't support granular unsubscribing.
            // The feed handler manages subscriptions and can handle reconnections
            // with the appropriate subscription state.
            log::debug!(
                "Databento does not support granular unsubscribing - ignoring unsubscribe request for {instrument_id}"
            );

            Ok(())
        })
    }

    fn unsubscribe(&mut self, cmd: &UnsubscribeCustomData) -> anyhow::Result<()> {
        // Note: Databento live API doesn't support granular unsubscribing.
        // The feed handler manages subscriptions and can handle reconnections
        // with the appropriate subscription state.
        log::debug!(
            "Databento does not support granular unsubscribing - ignoring unsubscribe request for {}",
            cmd.data_type
        );

        Ok(())
    }

    fn unsubscribe_book_depth(&mut self, cmd: &UnsubscribeBookDepth) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&cmd.instrument_ids, |instrument_id| {
            // Note: Databento live API doesn't support granular unsubscribing.
            // The feed handler manages subscriptions and can handle reconnections
            // with the appropriate subscription state.
            log::debug!(
                "Databento does not support granular unsubscribing - ignoring unsubscribe request for {instrument_id}"
            );

            Ok(())
        })
    }

    fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        execute_bar_subscriptions(&cmd.bar_types, |bar_type| {
            // Note: Databento live API doesn't support granular unsubscribing.
            // The feed handler manages subscriptions and can handle reconnections
            // with the appropriate subscription state.
            log::debug!(
                "Databento does not support granular unsubscribing - ignoring unsubscribe request for {bar_type}"
            );

            Ok(())
        })
    }

    fn unsubscribe_instrument_status(
        &mut self,
        cmd: &UnsubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&cmd.instrument_ids, |instrument_id| {
            // Note: Databento live API doesn't support granular unsubscribing.
            // The feed handler manages subscriptions and can handle reconnections
            // with the appropriate subscription state.
            log::debug!(
                "Databento does not support granular unsubscribing - ignoring unsubscribe request for {instrument_id}"
            );

            Ok(())
        })
    }

    fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        log::debug!("Request instruments: {request:?}");

        let historical_client = self.historical.clone();
        let data_sender = self.data_sender.clone();
        let dataset = request
            .venue
            .map(|venue| self.get_dataset_for_venue(venue))
            .transpose()?
            .unwrap_or_else(|| "GLBX.MDP3".to_string());
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let venue = request.venue.unwrap_or(*DATABENTO_VENUE);
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let request_params = request.params;
        let (query_start, query_end) = resolve_request_time_range(start_nanos, end_nanos);

        self.spawn_task(async move {
            let query_params = instruments_query_params(dataset, query_start, query_end);

            match historical_client.get_range_instruments(query_params).await {
                Ok(instruments) => {
                    log::debug!("Retrieved {} instruments", instruments.len());

                    let response = DataResponse::Instruments(InstrumentsResponse::new(
                        request_id,
                        client_id,
                        venue,
                        instruments,
                        start_nanos,
                        end_nanos,
                        get_atomic_clock_realtime().get_time_ns(),
                        request_params,
                    ));

                    send_data_response(&data_sender, response, "instruments");
                }
                Err(e) => {
                    send_request_result(&data_sender, request_id, None, &[e.to_string()]);
                }
            }
        });

        Ok(())
    }

    fn request_instrument(&self, request: RequestInstrument) -> anyhow::Result<()> {
        let groups = self.group_request_instruments(&request.instrument_ids)?;
        let historical_client = self.historical.clone();
        let data_sender = self.data_sender.clone();
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start = datetime_to_unix_nanos(request.start);
        let end = datetime_to_unix_nanos(request.end);
        let (query_start, query_end) = resolve_request_time_range(start, end);
        self.spawn_task(async move {
            let mut data = Vec::new();
            let mut failures = Vec::new();

            for (dataset, ids) in groups {
                let symbols = historical_client.prepare_symbols_from_instrument_ids(&ids);

                let params = RangeQueryParams {
                    dataset: dataset.clone(),
                    symbols,
                    start: query_start,
                    end: query_end,
                    limit: None,
                    price_precision: None,
                };

                match historical_client.get_range_instruments(params).await {
                    Ok(instruments) => {
                        for id in ids {
                            if let Some(instrument) = requested_instrument(&instruments, id) {
                                data.push(instrument);
                            } else {
                                failures.push(format!("Instrument not found: {id}"));
                            }
                        }
                    }
                    Err(e) => failures.push(format!("Dataset {dataset}: {e}")),
                }
            }

            let ts_init = get_atomic_clock_realtime().get_time_ns();

            let response = if request.instrument_ids.len() == 1 && data.len() == 1 {
                let instrument = data.remove(0);
                Some(DataResponse::Instrument(Box::new(InstrumentResponse::new(
                    request.request_id,
                    client_id,
                    instrument.id(),
                    instrument,
                    start,
                    end,
                    ts_init,
                    request.params,
                ))))
            } else if request.instrument_ids.len() > 1 {
                Some(DataResponse::Instruments(InstrumentsResponse::new(
                    request.request_id,
                    client_id,
                    *DATABENTO_VENUE,
                    data,
                    start,
                    end,
                    ts_init,
                    request.params,
                )))
            } else {
                None
            };

            send_request_result(&data_sender, request.request_id, response, &failures);
        });

        Ok(())
    }

    fn request_quotes(&self, request: RequestQuotes) -> anyhow::Result<()> {
        anyhow::ensure!(
            request.instrument_ids.len() == 1 || request.limit.is_none(),
            "A record limit is unsupported for multi-instrument requests"
        );
        let groups = self.group_request_instruments(&request.instrument_ids)?;
        let historical_client = self.historical.clone();
        let data_sender = self.data_sender.clone();
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start = datetime_to_unix_nanos(request.start);
        let end = datetime_to_unix_nanos(request.end);
        let limit = request.limit.map(|value| value.get() as u64);
        let precision = price_precision_from_params(request.params.as_ref())?;
        let schema = schema_from_params(request.params.as_ref(), dbn::Schema::Mbp1, QUOTE_SCHEMAS)?
            .to_string();
        let (query_start, query_end) = resolve_request_time_range(start, end);
        self.spawn_task(async move {
            let mut data = Vec::new();
            let mut failures = Vec::new();

            for (dataset, ids) in groups {
                for &id in &ids {
                    seed_price_precision_if_needed(
                        &historical_client,
                        &dataset,
                        id,
                        query_start,
                        query_end,
                        precision,
                    )
                    .await;
                }

                let symbols = historical_client.prepare_symbols_from_instrument_ids(&ids);

                let params = RangeQueryParams {
                    dataset: dataset.clone(),
                    symbols,
                    start: query_start,
                    end: query_end,
                    limit,
                    price_precision: precision,
                };

                match historical_client
                    .get_range_quotes(params, Some(schema.clone()))
                    .await
                {
                    Ok(rows) => data.extend(rows),
                    Err(e) => failures.push(format!("Dataset {dataset}: {e}")),
                }
            }

            data.sort_by_key(|row| (row.ts_init, row.instrument_id));
            let response = DataResponse::Quotes(QuotesResponse::new(
                request.request_id,
                client_id,
                request.instrument_ids,
                data,
                start,
                end,
                get_atomic_clock_realtime().get_time_ns(),
                request.params,
            ));
            send_request_result(&data_sender, request.request_id, Some(response), &failures);
        });

        Ok(())
    }

    fn request_trades(&self, request: RequestTrades) -> anyhow::Result<()> {
        anyhow::ensure!(
            request.instrument_ids.len() == 1 || request.limit.is_none(),
            "A record limit is unsupported for multi-instrument requests"
        );
        let groups = self.group_request_instruments(&request.instrument_ids)?;
        let historical_client = self.historical.clone();
        let data_sender = self.data_sender.clone();
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start = datetime_to_unix_nanos(request.start);
        let end = datetime_to_unix_nanos(request.end);
        let limit = request.limit.map(|value| value.get() as u64);
        let precision = price_precision_from_params(request.params.as_ref())?;
        let schema =
            schema_from_params(request.params.as_ref(), dbn::Schema::Trades, TRADE_SCHEMAS)?
                .to_string();
        let (query_start, query_end) = resolve_request_time_range(start, end);
        self.spawn_task(async move {
            let mut data = Vec::new();
            let mut failures = Vec::new();

            for (dataset, ids) in groups {
                for &id in &ids {
                    seed_price_precision_if_needed(
                        &historical_client,
                        &dataset,
                        id,
                        query_start,
                        query_end,
                        precision,
                    )
                    .await;
                }

                let symbols = historical_client.prepare_symbols_from_instrument_ids(&ids);

                let params = RangeQueryParams {
                    dataset: dataset.clone(),
                    symbols,
                    start: query_start,
                    end: query_end,
                    limit,
                    price_precision: precision,
                };

                match historical_client
                    .get_range_trades(params, Some(schema.clone()))
                    .await
                {
                    Ok(rows) => data.extend(rows),
                    Err(e) => failures.push(format!("Dataset {dataset}: {e}")),
                }
            }

            data.sort_by_key(|row| (row.ts_init, row.instrument_id));
            let response = DataResponse::Trades(TradesResponse::new(
                request.request_id,
                client_id,
                request.instrument_ids,
                data,
                start,
                end,
                get_atomic_clock_realtime().get_time_ns(),
                request.params,
            ));
            send_request_result(&data_sender, request.request_id, Some(response), &failures);
        });

        Ok(())
    }

    fn request_bars(&self, request: RequestBars) -> anyhow::Result<()> {
        let bar_type = request.single_bar_type()?;
        let aggregation = bar_type.spec().aggregation;
        anyhow::ensure!(
            matches!(
                aggregation,
                BarAggregation::Second
                    | BarAggregation::Minute
                    | BarAggregation::Hour
                    | BarAggregation::Day
            ),
            "Unsupported bar aggregation: {aggregation:?}"
        );

        log::debug!("Request bars: {request:?}");

        let historical_client = self.historical.clone();
        let data_sender = self.data_sender.clone();
        let instrument_id = bar_type.instrument_id();
        let dataset = self.get_dataset_for_venue(instrument_id.venue)?;
        let symbols = historical_client.prepare_symbols_from_instrument_ids(&[instrument_id]);
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let limit = request.limit.map(|limit| limit.get() as u64);
        let request_params = request.params;
        let price_precision = price_precision_from_params(request_params.as_ref())?;
        let timestamp_on_close = self.config.bars_timestamp_on_close;
        let spec = BarSpecification::new(1, aggregation, PriceType::Last);
        let interval = DurationNanos::try_from(spec.timedelta()).expect("supported OHLCV interval");
        let (start, end) = resolve_request_bounds(start_nanos, end_nanos);

        // Databento indexes bars by open, while ts_init always records the close
        let (query_start, query_end) = resolve_request_time_range(
            Some(start.saturating_sub(interval)),
            Some(end.saturating_sub(interval)),
        );

        self.spawn_task(async move {
            seed_price_precision_if_needed(
                &historical_client,
                dataset.as_str(),
                instrument_id,
                query_start,
                query_end,
                price_precision,
            )
            .await;

            let params = RangeQueryParams {
                dataset,
                symbols,
                start: query_start,
                end: query_end,
                limit,
                price_precision,
            };

            match historical_client
                .get_range_bars(params, aggregation, timestamp_on_close)
                .await
            {
                Ok(bars) => {
                    log::debug!("Retrieved {} bars", bars.len());
                    let response = DataResponse::Bars(BarsResponse::new(
                        request_id,
                        client_id,
                        bar_type,
                        bars,
                        start_nanos,
                        end_nanos,
                        get_atomic_clock_realtime().get_time_ns(),
                        request_params,
                    ));

                    send_data_response(&data_sender, response, "bars");
                }
                Err(e) => {
                    send_request_result(&data_sender, request_id, None, &[e.to_string()]);
                }
            }
        });

        Ok(())
    }

    fn request_book_depth(&self, request: RequestBookDepth) -> anyhow::Result<()> {
        let instrument_id = request.single_instrument_id()?;

        log::debug!("Request book depth: {request:?}");

        let historical_client = self.historical.clone();
        let data_sender = self.data_sender.clone();
        let dataset = self.get_dataset_for_venue(instrument_id.venue)?;
        let symbols = historical_client.prepare_symbols_from_instrument_ids(&[instrument_id]);
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let limit = request.limit.map(|limit| limit.get() as u64);
        let depth = request.depth.map(|depth| depth.get());
        let request_params = request.params;
        let price_precision = price_precision_from_params(request_params.as_ref())?;
        let (query_start, query_end) = resolve_request_time_range(start_nanos, end_nanos);

        self.spawn_task(async move {
            seed_price_precision_if_needed(
                &historical_client,
                dataset.as_str(),
                instrument_id,
                query_start,
                query_end,
                price_precision,
            )
            .await;

            let params = RangeQueryParams {
                dataset,
                symbols,
                start: query_start,
                end: query_end,
                limit,
                price_precision,
            };

            match historical_client
                .get_range_order_book_depth(params, depth)
                .await
            {
                Ok(depths) => {
                    log::debug!("Retrieved {} order book depths", depths.len());
                    let response = DataResponse::BookDepth(BookDepthResponse::new(
                        request_id,
                        client_id,
                        instrument_id,
                        depths,
                        start_nanos,
                        end_nanos,
                        get_atomic_clock_realtime().get_time_ns(),
                        request_params,
                    ));

                    send_data_response(&data_sender, response, "book depth");
                }
                Err(e) => {
                    send_request_result(&data_sender, request_id, None, &[e.to_string()]);
                }
            }
        });

        Ok(())
    }

    fn request_book_deltas(&self, request: RequestBookDeltas) -> anyhow::Result<()> {
        let instrument_id = request.single_instrument_id()?;

        log::debug!("Request book deltas: {request:?}");

        let historical_client = self.historical.clone();
        let data_sender = self.data_sender.clone();
        let dataset = self.get_dataset_for_venue(instrument_id.venue)?;
        let symbols = historical_client.prepare_symbols_from_instrument_ids(&[instrument_id]);
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let limit = request.limit.map(|limit| limit.get() as u64);
        let request_params = request.params;
        let price_precision = price_precision_from_params(request_params.as_ref())?;
        let (query_start, query_end) = resolve_request_time_range(start_nanos, end_nanos);

        self.spawn_task(async move {
            seed_price_precision_if_needed(
                &historical_client,
                dataset.as_str(),
                instrument_id,
                query_start,
                query_end,
                price_precision,
            )
            .await;

            let params = RangeQueryParams {
                dataset,
                symbols,
                start: query_start,
                end: query_end,
                limit,
                price_precision,
            };

            match historical_client.get_range_order_book_deltas(params).await {
                Ok(deltas) => {
                    log::debug!("Retrieved {} order book deltas", deltas.len());
                    let response = BookDeltasResponse::new(
                        request_id,
                        client_id,
                        instrument_id,
                        deltas,
                        start_nanos,
                        end_nanos,
                        get_atomic_clock_realtime().get_time_ns(),
                        request_params,
                    );

                    send_book_deltas_response(&data_sender, response);
                }
                Err(e) => {
                    send_request_result(&data_sender, request_id, None, &[e.to_string()]);
                }
            }
        });

        Ok(())
    }
}

impl DatabentoDataClient {
    fn subscribe_batch(
        &self,
        instrument_ids: Vec<InstrumentId>,
        params: Option<&Params>,
        default_schema: dbn::Schema,
        schemas: &[dbn::Schema],
    ) -> anyhow::Result<()> {
        let price_precision = price_precision_from_params(params)?;
        let schema = schema_from_params(params, default_schema, schemas)?;
        let ids = instrument_ids.into_instrument_ids()?;
        let groups = self.group_instruments(&ids)?;

        let mut accepted = Vec::new();
        let mut failure = None;

        for (dataset, ids) in &groups {
            match self.subscribe_dataset(dataset, ids, schema, price_precision) {
                Ok(()) => accepted.extend_from_slice(ids),
                Err(e) => {
                    log::error!(
                        "Databento subscription enqueue failed: dataset={dataset}, schema={schema}, instrument_ids={ids:?}"
                    );

                    if failure.is_none() {
                        failure = Some(e);
                    }
                }
            }
        }

        match failure {
            Some(e) => Err(SubscriptionError::instruments(accepted, e).into()),
            None => Ok(()),
        }
    }

    fn group_request_instruments(
        &self,
        ids: &[InstrumentId],
    ) -> anyhow::Result<IndexMap<String, Vec<InstrumentId>>> {
        let groups = self.group_instruments(ids)?;
        for ids in groups.values() {
            let symbols = ids.iter().map(|id| id.symbol.as_str()).collect::<Vec<_>>();
            check_consistent_symbology(&symbols)?;
        }

        Ok(groups)
    }

    fn group_instruments(
        &self,
        ids: &[InstrumentId],
    ) -> anyhow::Result<IndexMap<String, Vec<InstrumentId>>> {
        let mut groups: IndexMap<String, Vec<InstrumentId>> = IndexMap::new();
        let mut symbol_venues = AHashMap::new();

        for &id in ids {
            let dataset = self.get_dataset_for_venue(id.venue)?;
            if let Some(venue) = symbol_venues.insert(id.symbol, id.venue) {
                anyhow::ensure!(
                    venue == id.venue,
                    "Identifiers share raw symbol {} across different venues",
                    id.symbol
                );
            }

            groups.entry(dataset).or_default().push(id);
        }

        Ok(groups)
    }

    fn subscribe_dataset(
        &self,
        dataset: &str,
        ids: &[InstrumentId],
        schema: dbn::Schema,
        price_precision: Option<u8>,
    ) -> anyhow::Result<()> {
        let symbols: Vec<String> = ids.iter().map(|id| id.symbol.to_string()).collect();
        let precisions: Vec<(Symbol, u8)> = ids
            .iter()
            .filter_map(|id| price_precision.map(|precision| (id.symbol, precision)))
            .collect();
        let subscription = Subscription::builder()
            .schema(schema)
            .symbols(symbols)
            .build();
        let start_after_subscribe = self.get_or_create_feed_handler(dataset);
        let previous_venues: Vec<_> = ids
            .iter()
            .map(|id| (id.symbol, self.symbol_venue_map.get_cloned(&id.symbol)))
            .collect();

        for id in ids {
            self.symbol_venue_map.insert(id.symbol, id.venue);
        }

        if let Err(e) = self.send_subscription_to_dataset(
            dataset,
            precisions,
            subscription,
            start_after_subscribe,
        ) {
            for (symbol, previous) in previous_venues {
                if let Some(venue) = previous {
                    self.symbol_venue_map.insert(symbol, venue);
                } else {
                    self.symbol_venue_map.remove(&symbol);
                }
            }

            return Err(e);
        }

        Ok(())
    }
}

fn instruments_query_params(
    dataset: String,
    start_nanos: UnixNanos,
    end_nanos: Option<UnixNanos>,
) -> RangeQueryParams {
    RangeQueryParams {
        dataset,
        symbols: vec!["ALL_SYMBOLS".to_string()],
        start: start_nanos,
        end: end_nanos,
        limit: None,
        price_precision: None,
    }
}

fn instrument_query_params(
    dataset: String,
    instrument_id: InstrumentId,
    start_nanos: UnixNanos,
    end_nanos: Option<UnixNanos>,
) -> RangeQueryParams {
    RangeQueryParams {
        dataset,
        symbols: vec![instrument_id_to_symbol_string(
            instrument_id,
            &mut AHashMap::new(),
        )],
        start: start_nanos,
        end: end_nanos,
        limit: None,
        price_precision: None,
    }
}

fn resolve_request_time_range(
    start_nanos: Option<UnixNanos>,
    end_nanos: Option<UnixNanos>,
) -> (UnixNanos, Option<UnixNanos>) {
    let (start, end) = resolve_request_bounds(start_nanos, end_nanos);

    // Engine bounds include the end; Databento's query excludes it
    (start, Some(end.saturating_add(DurationNanos::new(1))))
}

fn resolve_request_bounds(
    start_nanos: Option<UnixNanos>,
    end_nanos: Option<UnixNanos>,
) -> (UnixNanos, UnixNanos) {
    let end = end_nanos.unwrap_or_else(|| get_atomic_clock_realtime().get_time_ns());
    let mut start = start_nanos.unwrap_or_else(|| start_of_utc_day(end));

    if start > end {
        start = end;
    }

    (start, end)
}

fn start_of_utc_day(timestamp: UnixNanos) -> UnixNanos {
    timestamp.floor(DurationNanos::from_days(1))
}

async fn seed_price_precision_if_needed(
    historical_client: &DatabentoHistoricalClient,
    dataset: &str,
    instrument_id: InstrumentId,
    start_nanos: UnixNanos,
    end_nanos: Option<UnixNanos>,
    price_precision: Option<u8>,
) {
    if price_precision.is_some()
        || historical_client
            .price_precision(instrument_id.symbol)
            .is_some()
    {
        return;
    }

    let query_start = start_of_utc_day(start_nanos);
    let query_params =
        instrument_query_params(dataset.to_string(), instrument_id, query_start, end_nanos);

    if let Err(e) = historical_client.get_range_instruments(query_params).await {
        log::warn!("Failed to seed price precision for {instrument_id}: {e}");
    }
}

fn send_request_result(
    data_sender: &EventSender<DataEvent>,
    request_id: nautilus_core::UUID4,
    response: Option<DataResponse>,
    failures: &[String],
) {
    let event = if failures.is_empty() {
        let Some(response) = response else {
            return;
        };

        let envelope = DataResponseEnvelope::new(response, true, None);
        DataEvent::ResponseEnvelope(envelope)
    } else {
        DataEvent::RequestFailed(DataRequestFailed {
            request_id,
            reason: failures.join("; "),
            response,
        })
    };

    if let Err(e) = data_sender.send(event) {
        log::error!("Failed to send request result: {e}");
    }
}

fn send_data_response(data_sender: &EventSender<DataEvent>, response: DataResponse, label: &str) {
    let envelope = DataResponseEnvelope::new(response, true, None);
    if let Err(e) = data_sender.send(DataEvent::ResponseEnvelope(envelope)) {
        log::error!("Failed to send {label} response: {e}");
    }
}

fn send_book_deltas_response(sender: &EventSender<DataEvent>, response: BookDeltasResponse) {
    let responses = partition_book_deltas_response(response);
    let last = responses.len() - 1;
    for (index, response) in responses.into_iter().enumerate() {
        let envelope =
            DataResponseEnvelope::new(DataResponse::BookDeltas(response), index == last, None);

        if let Err(e) = sender.send(DataEvent::ResponseEnvelope(envelope)) {
            log::error!("Failed to send book deltas response: {e}");
        }
    }
}

fn partition_book_deltas_response(response: BookDeltasResponse) -> Vec<BookDeltasResponse> {
    if response.data.is_empty() {
        return vec![response];
    }

    let mut partitions = IndexMap::new();
    for delta in response.data.iter().copied() {
        partitions
            .entry(delta.instrument_id)
            .or_insert_with(Vec::new)
            .push(delta);
    }

    partitions
        .into_iter()
        .map(|(instrument_id, data)| {
            let mut child = response.clone();
            child.instrument_id = instrument_id;
            child.data = data.into();
            child
        })
        .collect()
}

fn requested_instrument(
    instruments: &[InstrumentAny],
    instrument_id: InstrumentId,
) -> Option<InstrumentAny> {
    instruments
        .iter()
        .rev()
        .find(|instrument| instrument.id() == instrument_id)
        .cloned()
}

fn price_precision_from_params(params: Option<&Params>) -> anyhow::Result<Option<u8>> {
    let Some(price_precision) = params.and_then(|params| params.get_u64(PRICE_PRECISION_PARAM))
    else {
        return Ok(None);
    };

    Ok(Some(u8::try_from(price_precision).map_err(|_| {
        anyhow::anyhow!(
            "`{PRICE_PRECISION_PARAM}` must be less than or equal to {}",
            u8::MAX
        )
    })?))
}

fn custom_data_for_instrument(
    data: Arc<dyn CustomDataTrait>,
    instrument_id: InstrumentId,
) -> CustomData {
    let data_type = DataType::new(data.type_name(), None, Some(instrument_id.to_string()));
    CustomData::new(data, data_type)
}

fn custom_data_schema(data_type: &DataType) -> anyhow::Result<dbn::Schema> {
    match data_type.type_name() {
        name if name == DatabentoStatistics::type_name_static() => Ok(dbn::Schema::Statistics),
        name if name == DatabentoImbalance::type_name_static() => Ok(dbn::Schema::Imbalance),
        name => anyhow::bail!(
            "Unsupported custom data type for Databento live subscription: {name}, expected {} or {}",
            DatabentoStatistics::type_name_static(),
            DatabentoImbalance::type_name_static(),
        ),
    }
}

fn custom_data_instrument_id(data_type: &DataType) -> anyhow::Result<InstrumentId> {
    // Records are published without metadata, so metadata would change the subscribed topic
    anyhow::ensure!(
        data_type
            .metadata()
            .is_none_or(|metadata| metadata.is_empty()),
        "{} subscriptions do not support metadata, received {}",
        data_type.type_name(),
        data_type.metadata_str(),
    );

    let identifier = data_type.identifier().ok_or_else(|| {
        anyhow::anyhow!(
            "{} subscriptions require an instrument ID identifier",
            data_type.type_name()
        )
    })?;

    InstrumentId::from_str(identifier)
        .map_err(|e| anyhow::anyhow!("Invalid instrument ID identifier `{identifier}`: {e}"))
}

fn ohlcv_schema(bar_type: BarType) -> anyhow::Result<dbn::Schema> {
    let spec = bar_type.spec();
    anyhow::ensure!(
        spec.price_type == PriceType::Last,
        "Unsupported bar price type for Databento OHLCV: {spec}, only LAST is emitted",
    );
    anyhow::ensure!(
        spec.step.get() == 1,
        "Unsupported bar step for Databento OHLCV: {spec}, only a step of 1 is available",
    );

    ohlcv_schema_from_aggregation(spec.aggregation)
}

fn schema_from_params(
    params: Option<&Params>,
    default_schema: dbn::Schema,
    allowed_schemas: &[dbn::Schema],
) -> anyhow::Result<dbn::Schema> {
    let schema = if let Some(schema) = params.and_then(|params| params.get_str(SCHEMA_PARAM)) {
        dbn::Schema::from_str(schema)?
    } else {
        default_schema
    };

    if allowed_schemas.contains(&schema) {
        return Ok(schema);
    }

    let allowed = allowed_schemas
        .iter()
        .map(dbn::Schema::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    anyhow::bail!(
        "Invalid `{SCHEMA_PARAM}` '{}'. Must be one of: {allowed}",
        schema.as_str()
    );
}

fn send_subscription_commands(
    tx: &tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    dataset: &str,
    price_precisions: impl IntoIterator<Item = (Symbol, u8)>,
    subscription: Subscription,
    start_after_subscribe: bool,
) -> anyhow::Result<()> {
    for (symbol, precision) in price_precisions {
        tx.send(HandlerCommand::SetPricePrecision(symbol, precision))
            .map_err(|e| anyhow::anyhow!("Failed to send command to dataset {dataset}: {e}"))?;
    }

    tx.send(HandlerCommand::Subscribe(subscription))
        .map_err(|e| anyhow::anyhow!("Failed to send command to dataset {dataset}: {e}"))?;

    if start_after_subscribe {
        tx.send(HandlerCommand::Start)
            .map_err(|e| anyhow::anyhow!("Failed to send command to dataset {dataset}: {e}"))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{num::NonZeroUsize, path::PathBuf};

    use databento::dbn::{
        MappingInterval, SymbolMapping,
        decode::{DbnMetadata, DecodeRecord, dbn::Decoder},
        encode::EncodeRecord,
    };
    use nautilus_common::{
        live::runner::replace_data_event_sender,
        msgbus::{self, switchboard::get_custom_topic},
    };
    use nautilus_core::UUID4;
    use nautilus_model::{
        data::{BatchView, NautilusDataType, OrderBookDelta, QuoteTick},
        enums::BookType,
        identifiers::{ClientId, InstrumentId},
        instruments::{CurrencyPair, InstrumentAny},
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;
    use serde_json::json;
    use time::macros::date;
    use tokio::{
        io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
        net::TcpListener,
    };

    use super::*;
    use crate::enums::{DatabentoStatisticType, DatabentoStatisticUpdateAction};

    #[derive(Clone, Copy)]
    enum SubscribeKind {
        Quotes,
        Trades,
    }

    fn currency_pair(instrument_id: &str) -> InstrumentAny {
        currency_pair_with_ts_init(instrument_id, UnixNanos::default())
    }

    fn test_data_client() -> DatabentoDataClient {
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        replace_data_event_sender(sender);

        let config = DatabentoDataClientConfig::new(
            "32-character-with-lots-of-filler",
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("publishers.json"),
            true,
            true,
        );
        DatabentoDataClient::new(
            ClientId::from("DATABENTO-TEST"),
            config,
            get_atomic_clock_realtime(),
        )
        .expect("test client should initialize")
    }

    #[rstest]
    #[case::quotes(NautilusDataType::QuoteTick, true)]
    #[case::trades(NautilusDataType::TradeTick, true)]
    #[case::bars_close(NautilusDataType::Bar, true)]
    #[case::bars_open(NautilusDataType::Bar, false)]
    #[tokio::test]
    #[ignore = "Requires DATABENTO_API_KEY and GLBX.MDP3 historical access"]
    async fn test_historical_request_list_live(
        #[case] kind: NautilusDataType,
        #[case] timestamp_on_close: bool,
    ) {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        replace_data_event_sender(sender);
        let api_key = std::env::var("DATABENTO_API_KEY").unwrap();

        let config = DatabentoDataClientConfig::new(
            api_key,
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("publishers.json"),
            true,
            timestamp_on_close,
        );
        let client_id = ClientId::from("DATABENTO-HISTORY");
        let client =
            DatabentoDataClient::new(client_id, config, get_atomic_clock_realtime()).unwrap();
        let mut ids = vec![
            InstrumentId::from("ESM4.GLBX"),
            InstrumentId::from("NQM4.GLBX"),
        ];
        let start = UnixNanos::from(1_717_423_200_000_000_000);

        let end = if kind == NautilusDataType::Bar {
            ids.truncate(1);
            start
        } else {
            start + DurationNanos::from_secs(1)
        };

        let request_id = UUID4::new();
        let params = Params::new([(PRICE_PRECISION_PARAM.to_owned(), json!(2))].into());
        let bar_type = "ESM4.GLBX-1-MINUTE-LAST-EXTERNAL"
            .parse::<BarType>()
            .unwrap();

        match kind {
            NautilusDataType::QuoteTick => client
                .request_quotes(RequestQuotes::new(
                    ids.clone(),
                    Some(start.to_datetime_utc()),
                    Some(end.to_datetime_utc()),
                    None,
                    Some(client_id),
                    request_id,
                    UnixNanos::from(37),
                    Some(params.clone()),
                ))
                .unwrap(),
            NautilusDataType::TradeTick => client
                .request_trades(RequestTrades::new(
                    ids.clone(),
                    Some(start.to_datetime_utc()),
                    Some(end.to_datetime_utc()),
                    None,
                    Some(client_id),
                    request_id,
                    UnixNanos::from(37),
                    Some(params.clone()),
                ))
                .unwrap(),
            NautilusDataType::Bar => client
                .request_bars(RequestBars::new(
                    [bar_type],
                    Some(start.to_datetime_utc()),
                    Some(end.to_datetime_utc()),
                    None,
                    Some(client_id),
                    request_id,
                    UnixNanos::from(37),
                    Some(params.clone()),
                ))
                .unwrap(),
            _ => panic!("unexpected live request kind"),
        }

        let event = tokio::time::timeout(Duration::from_secs(60), receiver.recv())
            .await
            .unwrap()
            .unwrap();

        let DataEvent::ResponseEnvelope(envelope) = event else {
            panic!("expected successful historical envelope, received {event:?}");
        };

        let response = envelope.response().unwrap();

        let (response_ids, rows) = match (kind, response) {
            (NautilusDataType::QuoteTick, DataResponse::Quotes(response)) => (
                response.instrument_ids.clone(),
                response
                    .data
                    .iter()
                    .map(|row| (row.ts_init, row.instrument_id))
                    .collect::<Vec<_>>(),
            ),
            (NautilusDataType::TradeTick, DataResponse::Trades(response)) => (
                response.instrument_ids.clone(),
                response
                    .data
                    .iter()
                    .map(|row| (row.ts_init, row.instrument_id))
                    .collect::<Vec<_>>(),
            ),
            (NautilusDataType::Bar, DataResponse::Bars(response)) => {
                let expected_event = if timestamp_on_close {
                    start
                } else {
                    start - DurationNanos::from_mins(1)
                };

                assert_eq!(response.bar_type, bar_type);
                assert_eq!(response.data.len(), 1);
                assert_eq!(response.data[0].bar_type, bar_type);
                assert_eq!(response.data[0].ts_init, start);
                assert_eq!(response.data[0].ts_event, expected_event);
                (
                    vec![response.bar_type.instrument_id()],
                    response
                        .data
                        .iter()
                        .map(|row| (row.ts_init, row.bar_type.instrument_id()))
                        .collect::<Vec<_>>(),
                )
            }
            _ => panic!("unexpected historical response variant"),
        };

        let mut actual_ids = rows.iter().map(|(_, id)| *id).collect::<Vec<_>>();
        actual_ids.sort_unstable();
        actual_ids.dedup();
        let mut expected_ids = ids.clone();
        expected_ids.sort_unstable();

        assert_eq!(envelope.correlation_id(), &request_id);
        assert!(envelope.completed());
        assert_eq!(envelope.sub_correlation_id(), None);
        assert_eq!(envelope.failure_reason(), None);
        assert_eq!(response.correlation_id(), &request_id);
        assert_eq!(response.client_id(), &client_id);
        assert_eq!(response.start(), Some(start));
        assert_eq!(response.end(), Some(end));
        assert_eq!(response.params(), Some(&params));
        assert_eq!(response_ids, ids);
        assert_eq!(actual_ids, expected_ids);
        assert!(rows.is_sorted());
        assert!(rows.iter().all(|(ts, _)| *ts >= start && *ts <= end));
        assert_eq!(
            receiver.try_recv().unwrap_err(),
            tokio::sync::mpsc::error::TryRecvError::Empty
        );
    }

    #[rstest]
    #[case::raw("ESM4.GLBX", "NQM4.GLBX")]
    #[case::parent("ES.FUT.GLBX", "NQ.FUT.GLBX")]
    fn test_historical_request_groups_keep_dataset_and_input_order(
        #[case] first: &str,
        #[case] third: &str,
    ) {
        let client = test_data_client();
        let first = InstrumentId::from(first);
        let second = InstrumentId::from("AAPL.EQUS");
        let third = InstrumentId::from(third);
        let groups = client
            .group_request_instruments(&[first, second, third])
            .unwrap();
        assert_eq!(
            groups.into_iter().collect::<Vec<_>>(),
            vec![
                ("GLBX.MDP3".to_owned(), vec![first, third]),
                ("EQUS.MINI".to_owned(), vec![second])
            ]
        );
    }

    #[rstest]
    #[case::instruments(NautilusDataType::Instrument)]
    #[case::quotes(NautilusDataType::QuoteTick)]
    #[case::trades(NautilusDataType::TradeTick)]
    #[tokio::test]
    async fn test_historical_requests_reject_mixed_symbology_before_dispatch(
        #[case] kind: NautilusDataType,
        #[values(false, true)] earlier_dataset: bool,
    ) {
        let client = test_data_client();
        let mut ids = Vec::new();
        if earlier_dataset {
            ids.push(InstrumentId::from("AAPL.EQUS"));
        }

        ids.extend([
            InstrumentId::from("ES.FUT.GLBX"),
            InstrumentId::from("ESM4.GLBX"),
        ]);
        let request_id = UUID4::new();

        let result = match kind {
            NautilusDataType::Instrument => client.request_instrument(RequestInstrument::new(
                ids,
                None,
                None,
                None,
                request_id,
                UnixNanos::from(37),
                None,
            )),
            NautilusDataType::QuoteTick => client.request_quotes(RequestQuotes::new(
                ids,
                None,
                None,
                None,
                None,
                request_id,
                UnixNanos::from(37),
                None,
            )),
            NautilusDataType::TradeTick => client.request_trades(RequestTrades::new(
                ids,
                None,
                None,
                None,
                None,
                request_id,
                UnixNanos::from(37),
                None,
            )),
            _ => panic!("unexpected historical request kind"),
        };

        assert_eq!(
            result.unwrap_err().to_string(),
            "Inconsistent symbology types: 'parent' for ES.FUT vs 'raw_symbol' for ESM4"
        );
        assert!(client.task_handles.is_empty());
    }

    #[rstest]
    #[case::quotes(true)]
    #[case::trades(false)]
    fn test_historical_request_lists_reject_limit_before_dispatch(#[case] quotes: bool) {
        let client = test_data_client();
        let ids = [
            InstrumentId::from("ESM4.GLBX"),
            InstrumentId::from("AAPL.EQUS"),
        ];

        let result = if quotes {
            client.request_quotes(RequestQuotes::new(
                ids,
                None,
                None,
                NonZeroUsize::new(5),
                None,
                UUID4::new(),
                UnixNanos::from(37),
                None,
            ))
        } else {
            client.request_trades(RequestTrades::new(
                ids,
                None,
                None,
                NonZeroUsize::new(5),
                None,
                UUID4::new(),
                UnixNanos::from(37),
                None,
            ))
        };

        assert_eq!(
            result.unwrap_err().to_string(),
            "A record limit is unsupported for multi-instrument requests"
        );
    }

    #[rstest]
    fn test_historical_bars_reject_unsupported_aggregation_before_dispatch() {
        let client = test_data_client();

        let request = RequestBars::new(
            BarType::from("ESM4.GLBX-100-TICK-LAST-EXTERNAL"),
            None,
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::from(37),
            None,
        );
        assert_eq!(
            client.request_bars(request).unwrap_err().to_string(),
            "Unsupported bar aggregation: Tick"
        );
        assert!(client.task_handles.is_empty());
    }

    #[rstest]
    fn test_dataset_failure_preserves_successful_group_rows() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let sender: EventSender<DataEvent> = sender.into();
        let root = UUID4::new();
        let first = InstrumentId::from("ESM6.GLBX");
        let second = InstrumentId::from("AAPL.EQUS");

        let quote = QuoteTick {
            instrument_id: first,
            ts_init: UnixNanos::from(17),
            ..Default::default()
        };

        let response = QuotesResponse::new(
            root,
            ClientId::from("HISTORY"),
            [first, second],
            vec![quote],
            Some(UnixNanos::from(11)),
            Some(UnixNanos::from(29)),
            UnixNanos::from(31),
            None,
        );

        send_request_result(
            &sender,
            root,
            Some(DataResponse::Quotes(response)),
            &["Dataset EQUS: source unavailable".to_owned()],
        );

        let DataEvent::RequestFailed(failure) = receiver.try_recv().unwrap() else {
            panic!("Expected failed dataset result");
        };

        assert_eq!(failure.request_id, root);
        assert_eq!(failure.reason, "Dataset EQUS: source unavailable");

        let Some(DataResponse::Quotes(response)) = failure.response else {
            panic!("Expected successful sibling rows");
        };

        assert_eq!(response.correlation_id, root);
        assert_eq!(response.client_id, ClientId::from("HISTORY"));
        assert_eq!(response.instrument_ids, vec![first, second]);
        assert_eq!(response.data.as_ref(), &[quote]);
        assert_eq!(response.start, Some(UnixNanos::from(11)));
        assert_eq!(response.end, Some(UnixNanos::from(29)));
        assert_eq!(response.ts_init, UnixNanos::from(31));
        assert_eq!(response.params, None);
        assert_eq!(
            receiver.try_recv().unwrap_err(),
            tokio::sync::mpsc::error::TryRecvError::Empty
        );
    }

    #[rstest]
    fn test_book_reply_completes_after_all_partitioned_payloads() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let sender: EventSender<DataEvent> = sender.into();
        let root = UUID4::new();
        let first = InstrumentId::from("ESM6.GLBX");
        let second = InstrumentId::from("ESU6.GLBX");
        let a = OrderBookDelta::clear(first, 11, UnixNanos::from(13), UnixNanos::from(17));
        let b = OrderBookDelta::clear(second, 19, UnixNanos::from(23), UnixNanos::from(29));
        let c = OrderBookDelta::clear(first, 31, UnixNanos::from(37), UnixNanos::from(41));

        let response = BookDeltasResponse::new(
            root,
            ClientId::from("HISTORY"),
            InstrumentId::from("ES.FUT.GLBX"),
            vec![a, b, c],
            None,
            None,
            UnixNanos::from(43),
            None,
        );
        send_book_deltas_response(&sender, response);

        let DataEvent::ResponseEnvelope(left) = receiver.try_recv().unwrap() else {
            panic!("expected first envelope");
        };

        let DataEvent::ResponseEnvelope(right) = receiver.try_recv().unwrap() else {
            panic!("expected final envelope");
        };

        let Some(DataResponse::BookDeltas(left_payload)) = left.response() else {
            panic!("expected first book");
        };

        let Some(DataResponse::BookDeltas(right_payload)) = right.response() else {
            panic!("expected second book");
        };

        assert_eq!(
            (left.correlation_id(), right.correlation_id()),
            (&root, &root)
        );
        assert!(!left.completed());
        assert!(right.completed());
        assert_eq!(left_payload.instrument_id, first);
        assert_eq!(right_payload.instrument_id, second);
        assert_eq!(left_payload.data.as_ref(), &vec![a, c]);
        assert_eq!(right_payload.data.as_ref(), &vec![b]);
        assert_eq!(
            receiver.try_recv().unwrap_err(),
            tokio::sync::mpsc::error::TryRecvError::Empty
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_stop_closes_task_admission_until_disconnect_drains() {
        let mut client = test_data_client();

        client
            .task_handles
            .spawn(async { std::future::pending::<()>().await })
            .unwrap();
        client.is_connected.store(true, Ordering::Relaxed);

        client.stop().unwrap();

        assert!(!client.task_handles.is_open());
        assert!(client.is_disconnected());

        client.disconnect().await.unwrap();
        assert!(client.task_handles.is_empty());
    }

    #[rstest]
    #[case("EQUS", "EQUS.PLUS")] // overrides the apply_default EQUS -> EQUS.MINI mapping
    #[case("GLBX", "EQUS.MINI")] // overrides the apply_default GLBX -> GLBX.MDP3 mapping
    fn test_venue_dataset_map_overrides_default(#[case] venue: &str, #[case] dataset: &str) {
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        replace_data_event_sender(sender);

        let mut config = DatabentoDataClientConfig::new(
            "32-character-with-lots-of-filler",
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("publishers.json"),
            true,
            true,
        );
        config.venue_dataset_map = IndexMap::from([(venue.to_string(), dataset.to_string())]);

        let client = DatabentoDataClient::new(
            ClientId::from("DATABENTO-TEST"),
            config,
            get_atomic_clock_realtime(),
        )
        .expect("test client should initialize");

        assert_eq!(
            client.get_dataset_for_venue(Venue::from(venue)).unwrap(),
            dataset
        );

        // The override is targeted: an unrelated venue keeps its default.
        assert_eq!(
            client.get_dataset_for_venue(Venue::from("XCBO")).unwrap(),
            "OPRA.PILLAR"
        );
    }

    #[rstest]
    fn test_config_base_url_overrides_default_to_none() {
        let config = DatabentoDataClientConfig::new(
            "32-character-with-lots-of-filler",
            PathBuf::from("test_publishers.json"),
            true,
            true,
        );

        assert!(config.historical_base_url.is_none());
        assert!(config.live_gateway_addr.is_none());
    }

    #[rstest]
    fn test_historical_base_url_override_invalid_errors() {
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        replace_data_event_sender(sender);

        let mut config = DatabentoDataClientConfig::new(
            "32-character-with-lots-of-filler",
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("publishers.json"),
            true,
            true,
        );
        config.historical_base_url = Some("not a url".to_string());

        let result = DatabentoDataClient::new(
            ClientId::from("DATABENTO-TEST"),
            config,
            get_atomic_clock_realtime(),
        );

        assert!(result.is_err());
    }

    fn subscribe_quotes_cmd(params: Option<Params>) -> SubscribeQuotes {
        SubscribeQuotes::new(
            InstrumentId::from("ESM4.GLBX"),
            Some(ClientId::from("DATABENTO-TEST")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            params,
        )
    }

    fn subscribe_trades_cmd(params: Option<Params>) -> SubscribeTrades {
        SubscribeTrades::new(
            InstrumentId::from("ESM4.GLBX"),
            Some(ClientId::from("DATABENTO-TEST")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            params,
        )
    }

    fn subscribe_book_depth_cmd(depth: Option<NonZeroUsize>) -> SubscribeBookDepth {
        SubscribeBookDepth::new(
            InstrumentId::from("ESM4.GLBX"),
            BookType::L2_MBP,
            Some(ClientId::from("DATABENTO-TEST")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            depth,
            true,
            None,
            None,
        )
    }

    fn currency_pair_with_ts_init(instrument_id: &str, ts_init: UnixNanos) -> InstrumentAny {
        let instrument_id = InstrumentId::from(instrument_id);
        InstrumentAny::CurrencyPair(
            CurrencyPair::builder()
                .instrument_id(instrument_id)
                .raw_symbol(instrument_id.symbol)
                .base_currency(Currency::from("BTC"))
                .quote_currency(Currency::from("USDT"))
                .price_precision(2)
                .size_precision(6)
                .price_increment(Price::from("0.01"))
                .size_increment(Quantity::from("0.000001"))
                .ts_event(UnixNanos::default())
                .ts_init(ts_init)
                .build()
                .unwrap(),
        )
    }

    #[rstest]
    fn test_instruments_query_params_requests_all_symbols() {
        let start = UnixNanos::from(1_000_000_000);
        let end = UnixNanos::from(2_000_000_000);

        let params = instruments_query_params("GLBX.MDP3".to_string(), start, Some(end));

        assert_eq!(params.dataset, "GLBX.MDP3");
        assert_eq!(params.symbols, vec!["ALL_SYMBOLS"]);
        assert_eq!(params.start, start);
        assert_eq!(params.end, Some(end));
        assert_eq!(params.limit, None);
        assert_eq!(params.price_precision, None);
    }

    #[rstest]
    fn test_instrument_query_params_requests_single_symbol() {
        let instrument_id = InstrumentId::from("ESM4.GLBX");

        let start = UnixNanos::from(1_000_000_000);
        let end = UnixNanos::from(2_000_000_000);

        let params =
            instrument_query_params("GLBX.MDP3".to_string(), instrument_id, start, Some(end));

        assert_eq!(params.dataset, "GLBX.MDP3");
        assert_eq!(params.symbols, vec!["ESM4"]);
        assert_eq!(params.start, start);
        assert_eq!(params.end, Some(end));
        assert_eq!(params.limit, None);
        assert_eq!(params.price_precision, None);
    }

    #[rstest]
    #[case::intraday(1_782_302_400_000_000_001)]
    #[case::midnight(1_782_259_200_000_000_000)]
    #[case::day_end(1_782_345_599_999_999_999)]
    #[tokio::test]
    async fn test_seed_price_precision_includes_midnight(#[case] start: u64) {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_data/test_data.definition.futures_contract.dbn.zst");
        let mut decoder = Decoder::from_zstd_file(path).unwrap();
        let mut metadata = decoder.metadata().clone();
        let record = decoder
            .decode_record::<dbn::InstrumentDefMsg>()
            .unwrap()
            .unwrap();
        metadata.symbols = vec!["ESU6".to_string()];

        metadata.mappings = vec![SymbolMapping {
            raw_symbol: "ESU6".to_string(),
            intervals: vec![MappingInterval {
                start_date: date!(2026 - 06 - 24),
                end_date: date!(2026 - 06 - 25),
                symbol: record.hd.instrument_id.to_string(),
            }],
        }];

        let mut body = Vec::new();
        {
            let mut encoder = dbn::encode::dbn::Encoder::with_zstd(&mut body, &metadata).unwrap();
            encoder.encode_record(record).unwrap();
            encoder.flush().unwrap();
        }

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}/", listener.local_addr().unwrap());

        let server = tokio::spawn(serve_historical_response(listener, body));

        let client = DatabentoHistoricalClient::new_with_base_url(
            Credential::new("32-character-with-lots-of-filler"),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("publishers.json"),
            get_atomic_clock_realtime(),
            false,
            &base_url,
        )
        .unwrap();
        let instrument_id = InstrumentId::from("ESU6.GLBX");
        let end = UnixNanos::from(1_782_345_600_000_000_123);

        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            seed_price_precision_if_needed(
                &client,
                "GLBX.MDP3",
                instrument_id,
                start.into(),
                Some(end),
                None,
            ),
        )
        .await
        .unwrap();
        let request = server.await.unwrap();
        let fields: AHashMap<_, _> = request
            .split('&')
            .map(|field| field.split_once('=').unwrap())
            .collect();

        assert_eq!(fields["dataset"], "GLBX.MDP3");
        assert_eq!(fields["schema"], "definition");
        assert_eq!(fields["symbols"], "ESU6");
        assert_eq!(fields["start"], "1782259200000000000");
        assert_eq!(fields["end"], end.to_string());
        assert_eq!(client.price_precision(instrument_id.symbol), Some(2));
    }

    async fn serve_historical_response(listener: TcpListener, body: Vec<u8>) -> String {
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = BufReader::new(stream);
        let mut line = String::new();
        stream.read_line(&mut line).await.unwrap();
        assert_eq!(line, "POST /v0/timeseries.get_range HTTP/1.1\r\n");
        let mut content_length = None;

        loop {
            line.clear();
            stream.read_line(&mut line).await.unwrap();

            if line == "\r\n" {
                break;
            }

            if let Some((name, value)) = line.split_once(':')
                && name.eq_ignore_ascii_case("content-length")
            {
                content_length = Some(value.trim().parse::<usize>().unwrap());
            }
        }

        let mut request = vec![0; content_length.unwrap()];
        stream.read_exact(&mut request).await.unwrap();
        let headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len(),
        );
        stream.write_all(headers.as_bytes()).await.unwrap();
        stream.write_all(&body).await.unwrap();
        stream.shutdown().await.unwrap();
        String::from_utf8(request).unwrap()
    }

    #[rstest]
    #[case::explicit(Some(5), None)]
    #[case::cached(None, Some(3))]
    #[tokio::test]
    async fn test_seed_price_precision_skips_known_precision(
        #[case] price_precision: Option<u8>,
        #[case] cached_precision: Option<u8>,
    ) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}/", listener.local_addr().unwrap());

        let client = DatabentoHistoricalClient::new_with_base_url(
            Credential::new("32-character-with-lots-of-filler"),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("publishers.json"),
            get_atomic_clock_realtime(),
            false,
            &base_url,
        )
        .unwrap();
        let instrument_id = InstrumentId::from("ESU6.GLBX");
        if let Some(precision) = cached_precision {
            client.set_price_precision(instrument_id.symbol, precision);
        }

        tokio::select! {
            () = seed_price_precision_if_needed(
                &client,
                "GLBX.MDP3",
                instrument_id,
                1_706_443_200_000_000_001.into(),
                Some(1_706_486_400_000_000_123.into()),
                price_precision,
            ) => {}
            result = listener.accept() => panic!("Unexpected definition request: {result:?}"),
            () = tokio::time::sleep(std::time::Duration::from_secs(5)) => {
                panic!("Precision seeding did not finish");
            }
        }

        assert_eq!(
            client.price_precision(instrument_id.symbol),
            cached_precision
        );
    }

    #[rstest]
    fn test_resolve_request_time_range_defaults_to_end_day() {
        let end = UnixNanos::from(1_706_443_200_000_000_001);

        let (start, resolved_end) = resolve_request_time_range(None, Some(end));

        assert_eq!(start, UnixNanos::from(1_706_400_000_000_000_000));
        assert_eq!(resolved_end, Some(end + DurationNanos::new(1)));
    }

    #[rstest]
    #[case::range(17, 23, 17, 24)]
    #[case::point(17, 17, 17, 18)]
    #[case::zero(0, 0, 0, 1)]
    #[case::inverted(23, 17, 17, 18)]
    fn test_resolve_request_time_range_converts_inclusive_bounds(
        #[case] start: u64,
        #[case] end: u64,
        #[case] expected_start: u64,
        #[case] expected_end: u64,
    ) {
        let (start, end) = resolve_request_time_range(Some(start.into()), Some(end.into()));

        assert_eq!(start, UnixNanos::from(expected_start));
        assert_eq!(end, Some(UnixNanos::from(expected_end)));
    }

    #[rstest]
    #[case::quotes(false, true, false)]
    #[case::bars_close(true, true, false)]
    #[case::bars_open(true, false, false)]
    #[case::bars_default_start(true, true, true)]
    #[tokio::test]
    async fn test_historical_request_sends_inclusive_bounds(
        #[case] bars: bool,
        #[case] timestamp_on_close: bool,
        #[case] missing_start: bool,
    ) {
        let filename = if bars {
            "test_data.ohlcv-1m.dbn.zst"
        } else {
            "test_data.mbp-1.dbn.zst"
        };

        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("test_data")
            .join(filename);
        let decoder = Decoder::from_zstd_file(path).unwrap();
        let mut metadata = decoder.metadata().clone();
        metadata.dataset = "GLBX.MDP3".to_owned();
        let mut body = Vec::new();
        {
            let mut encoder = dbn::encode::dbn::Encoder::with_zstd(&mut body, &metadata).unwrap();
            encoder.flush().unwrap();
        }

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base_url = format!("http://{}/", listener.local_addr().unwrap());

        let server = tokio::spawn(serve_historical_response(listener, body));
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        replace_data_event_sender(sender);

        let mut config = DatabentoDataClientConfig::new(
            "32-character-with-lots-of-filler",
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("publishers.json"),
            true,
            timestamp_on_close,
        );
        config.historical_base_url = Some(base_url);
        let client_id = ClientId::from("DATABENTO-BOUNDS");
        let client =
            DatabentoDataClient::new(client_id, config, get_atomic_clock_realtime()).unwrap();
        let id = InstrumentId::from("ESM4.GLBX");
        let start = UnixNanos::from(1_717_423_200_000_000_000);
        let request_start = (!missing_start).then_some(start);
        let end = start + DurationNanos::from_secs(60) + DurationNanos::new(123);
        let request_id = UUID4::new();
        let params = Params::new([(PRICE_PRECISION_PARAM.to_owned(), json!(2))].into());

        if bars {
            let bar_type = "ESM4.GLBX-1-MINUTE-LAST-EXTERNAL"
                .parse::<BarType>()
                .unwrap();
            client
                .request_bars(RequestBars::new(
                    bar_type,
                    request_start.map(|timestamp| timestamp.to_datetime_utc()),
                    Some(end.to_datetime_utc()),
                    None,
                    Some(client_id),
                    request_id,
                    UnixNanos::from(37),
                    Some(params.clone()),
                ))
                .unwrap();
        } else {
            client
                .request_quotes(RequestQuotes::new(
                    id,
                    request_start.map(|timestamp| timestamp.to_datetime_utc()),
                    Some(end.to_datetime_utc()),
                    None,
                    Some(client_id),
                    request_id,
                    UnixNanos::from(37),
                    Some(params.clone()),
                ))
                .unwrap();
        }

        let event = tokio::time::timeout(Duration::from_secs(5), receiver.recv())
            .await
            .unwrap()
            .unwrap();

        let DataEvent::ResponseEnvelope(envelope) = event else {
            panic!("expected completed response, received {event:?}");
        };

        let response = envelope.response().unwrap();
        let request = server.await.unwrap();
        let fields: AHashMap<_, _> = request
            .split('&')
            .map(|field| field.split_once('=').unwrap())
            .collect();

        let offset = if bars {
            DurationNanos::from_secs(60)
        } else {
            DurationNanos::new(0)
        };

        let expected_start = if missing_start {
            UnixNanos::from(1_717_372_800_000_000_000)
        } else {
            start
        };

        assert_eq!(fields["dataset"], "GLBX.MDP3");
        assert_eq!(fields["schema"], if bars { "ohlcv-1m" } else { "mbp-1" });
        assert_eq!(fields["symbols"], "ESM4");
        assert_eq!(fields["start"], (expected_start - offset).to_string());
        assert_eq!(
            fields["end"],
            (end - offset + DurationNanos::new(1)).to_string()
        );
        assert_eq!(envelope.correlation_id(), &request_id);
        assert!(envelope.completed());
        assert_eq!(envelope.failure_reason(), None);
        assert_eq!(response.start(), request_start);
        assert_eq!(response.end(), Some(end));
        assert_eq!(response.client_id(), &client_id);
        assert_eq!(response.params(), Some(&params));
        assert_eq!(response.record_count(), Some(0));
    }

    #[rstest]
    fn test_requested_instrument_filters_exact_id() {
        let requested_id = InstrumentId::from("BTCUSDT.BINANCE");
        let instruments = vec![
            currency_pair("ETHUSDT.BINANCE"),
            currency_pair("BTCUSDT.BINANCE"),
        ];

        let instrument = requested_instrument(&instruments, requested_id).expect("instrument");

        assert_eq!(instrument.id(), requested_id);
    }

    #[rstest]
    fn test_requested_instrument_returns_latest_matching_id() {
        let requested_id = InstrumentId::from("BTCUSDT.BINANCE");
        let instruments = vec![
            currency_pair_with_ts_init("BTCUSDT.BINANCE", UnixNanos::from(1)),
            currency_pair_with_ts_init("BTCUSDT.BINANCE", UnixNanos::from(2)),
        ];

        let instrument = requested_instrument(&instruments, requested_id).expect("instrument");

        assert_eq!(instrument.ts_init(), UnixNanos::from(2));
    }

    #[rstest]
    fn test_requested_instrument_returns_none_on_miss() {
        let instruments = vec![currency_pair("ETHUSDT.BINANCE")];

        let instrument = requested_instrument(&instruments, InstrumentId::from("BTCUSDT.BINANCE"));

        assert!(instrument.is_none());
    }

    #[rstest]
    fn test_price_precision_from_params() {
        let params = Params::new([(PRICE_PRECISION_PARAM.to_string(), json!(5))].into());

        let price_precision = price_precision_from_params(Some(&params)).unwrap();

        assert_eq!(price_precision, Some(5));
    }

    #[rstest]
    fn test_price_precision_from_params_rejects_out_of_range_value() {
        let params = Params::new(
            [(
                PRICE_PRECISION_PARAM.to_string(),
                json!(u64::from(u8::MAX) + 1),
            )]
            .into(),
        );

        let result = price_precision_from_params(Some(&params));

        assert!(result.is_err());
    }

    #[rstest]
    fn test_partition_book_deltas_response_by_child_instrument(
        #[values(false, true)] narrowed: bool,
    ) {
        let correlation_id = UUID4::new();
        let client_id = ClientId::from("DATABENTO-TEST");
        let parent = InstrumentId::from("ES.FUT.GLBX");
        let child_a = InstrumentId::from("ESM6.GLBX");
        let child_b = InstrumentId::from("ESU6.GLBX");
        let deltas = vec![
            OrderBookDelta::clear(child_a, 1, UnixNanos::from(1_000), UnixNanos::from(1_000)),
            OrderBookDelta::clear(child_b, 2, UnixNanos::from(2_000), UnixNanos::from(2_000)),
            OrderBookDelta::clear(child_a, 3, UnixNanos::from(3_000), UnixNanos::from(3_000)),
        ];
        let params = Params::new([(PRICE_PRECISION_PARAM.to_string(), json!(5))].into());

        let rows = if narrowed {
            let hidden = OrderBookDelta::clear(
                InstrumentId::from("HIDDEN.GLBX"),
                4,
                UnixNanos::from(5_000),
                UnixNanos::from(5_000),
            );
            let mut backing = vec![hidden];
            backing.extend_from_slice(&deltas);
            backing.push(hidden);
            BatchView::new(Arc::new(backing), 1..4)
        } else {
            deltas.clone().into()
        };

        let response = BookDeltasResponse::new(
            correlation_id,
            client_id,
            parent,
            rows,
            Some(UnixNanos::from(500)),
            Some(UnixNanos::from(4_000)),
            UnixNanos::from(5_000),
            Some(params.clone()),
        );

        let responses = partition_book_deltas_response(response);

        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0].correlation_id, correlation_id);
        assert_eq!(responses[1].correlation_id, correlation_id);
        assert_eq!(responses[0].client_id, client_id);
        assert_eq!(responses[1].client_id, client_id);
        assert_eq!(responses[0].instrument_id, child_a);
        assert_eq!(responses[1].instrument_id, child_b);
        assert_eq!(responses[0].data.as_ref(), &vec![deltas[0], deltas[2]]);
        assert_eq!(responses[1].data.as_ref(), &vec![deltas[1]]);
        assert_eq!(responses[0].start, Some(UnixNanos::from(500)));
        assert_eq!(responses[1].start, Some(UnixNanos::from(500)));
        assert_eq!(responses[0].end, Some(UnixNanos::from(4_000)));
        assert_eq!(responses[1].end, Some(UnixNanos::from(4_000)));
        assert_eq!(responses[0].ts_init, UnixNanos::from(5_000));
        assert_eq!(responses[1].ts_init, UnixNanos::from(5_000));
        assert_eq!(responses[0].params, Some(params.clone()));
        assert_eq!(responses[1].params, Some(params));
    }

    #[rstest]
    fn test_partition_book_deltas_response_preserves_homogeneous_response() {
        let correlation_id = UUID4::new();
        let client_id = ClientId::from("DATABENTO-TEST");
        let instrument_id = InstrumentId::from("ESM6.GLBX");
        let delta = OrderBookDelta::clear(
            instrument_id,
            1,
            UnixNanos::from(1_000),
            UnixNanos::from(1_000),
        );
        let response = BookDeltasResponse::new(
            correlation_id,
            client_id,
            instrument_id,
            vec![delta],
            Some(UnixNanos::from(500)),
            Some(UnixNanos::from(1_500)),
            UnixNanos::from(2_000),
            None,
        );

        let responses = partition_book_deltas_response(response);

        assert_eq!(responses.len(), 1);
        assert_eq!(responses[0].correlation_id, correlation_id);
        assert_eq!(responses[0].client_id, client_id);
        assert_eq!(responses[0].instrument_id, instrument_id);
        assert_eq!(responses[0].data.as_ref(), &vec![delta]);
        assert_eq!(responses[0].start, Some(UnixNanos::from(500)));
        assert_eq!(responses[0].end, Some(UnixNanos::from(1_500)));
        assert_eq!(responses[0].ts_init, UnixNanos::from(2_000));
        assert_eq!(responses[0].params, None);
    }

    #[rstest]
    fn test_partition_book_deltas_response_preserves_empty_parent_response() {
        let correlation_id = UUID4::new();
        let parent = InstrumentId::from("ES.FUT.GLBX");
        let response = BookDeltasResponse::new(
            correlation_id,
            ClientId::from("DATABENTO-TEST"),
            parent,
            Vec::new(),
            None,
            None,
            UnixNanos::from(1_000),
            None,
        );

        let responses = partition_book_deltas_response(response);

        assert_eq!(responses.len(), 1);
        let response = &responses[0];
        assert_eq!(response.correlation_id, correlation_id);
        assert_eq!(response.instrument_id, parent);
        assert!(response.data.is_empty());
    }

    #[rstest]
    fn test_schema_from_params_returns_default() {
        let schema = schema_from_params(None, dbn::Schema::Mbp1, QUOTE_SCHEMAS).unwrap();

        assert_eq!(schema, dbn::Schema::Mbp1);
    }

    #[rstest]
    fn test_schema_from_params_accepts_allowed_value() {
        let params = Params::new([(SCHEMA_PARAM.to_string(), json!("tbbo"))].into());

        let schema = schema_from_params(Some(&params), dbn::Schema::Mbp1, QUOTE_SCHEMAS).unwrap();

        assert_eq!(schema, dbn::Schema::Tbbo);
    }

    #[rstest]
    fn test_schema_from_params_rejects_disallowed_value() {
        let params = Params::new([(SCHEMA_PARAM.to_string(), json!("mbo"))].into());

        let result = schema_from_params(Some(&params), dbn::Schema::Mbp1, QUOTE_SCHEMAS);

        assert!(result.is_err());
    }

    #[rstest]
    #[case::quotes(SubscribeKind::Quotes)]
    #[case::trades(SubscribeKind::Trades)]
    fn test_invalid_subscribe_params_do_not_create_feed_handler(#[case] kind: SubscribeKind) {
        let mut client = test_data_client();
        let params = Params::new([(SCHEMA_PARAM.to_string(), json!("definition"))].into());
        let result = match kind {
            SubscribeKind::Quotes => client.subscribe_quotes(subscribe_quotes_cmd(Some(params))),
            SubscribeKind::Trades => client.subscribe_trades(subscribe_trades_cmd(Some(params))),
        };

        assert!(result.is_err());
        assert!(client.cmd_channels.lock().is_empty());
    }

    #[rstest]
    fn test_bar_list_reports_exact_full_types_and_continues_after_invalid_member() {
        let mut client = test_data_client();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client
            .cmd_channels
            .lock()
            .insert("GLBX.MDP3".to_string(), tx);
        let first = BarType::from("ESM4.GLBX-1-SECOND-LAST-EXTERNAL");
        let invalid = BarType::from("ESM4.GLBX-5-MINUTE-LAST-EXTERNAL");
        let last = BarType::from("ESM4.GLBX-1-MINUTE-LAST-EXTERNAL");

        let command = SubscribeBars::new(
            [first, invalid, last],
            Some(client.client_id),
            None,
            UUID4::new(),
            UnixNanos::from(17),
            Some(UUID4::new()),
            None,
        );
        let error = client.subscribe_bars(command).unwrap_err();
        let partial = error.downcast_ref::<SubscriptionError>().unwrap();
        assert_eq!(partial.bar_types_accepted, vec![first, last]);
        assert_eq!(partial.instrument_ids_accepted, Vec::<InstrumentId>::new());

        for schema in [dbn::Schema::Ohlcv1S, dbn::Schema::Ohlcv1M] {
            let HandlerCommand::Subscribe(actual) = rx.try_recv().unwrap() else {
                panic!("expected bar schema subscription");
            };

            let expected = Subscription::builder()
                .schema(schema)
                .symbols("ESM4")
                .build();
            assert_eq!(actual, expected);
        }

        assert_eq!(
            rx.try_recv().unwrap_err(),
            tokio::sync::mpsc::error::TryRecvError::Empty
        );
    }

    macro_rules! batch_tests {
        ($batch_test:ident, $failure_test:ident, $validation_test:ident, $command:ident, $subscribe:ident, $schema:ident) => {
            #[rstest]
            fn $batch_test() {
                let mut client = test_data_client();
                let (glbx_tx, mut glbx_rx) = tokio::sync::mpsc::unbounded_channel();
                let (opra_tx, mut opra_rx) = tokio::sync::mpsc::unbounded_channel();
                client
                    .cmd_channels
                    .lock()
                    .insert("GLBX.MDP3".to_string(), glbx_tx);
                client
                    .cmd_channels
                    .lock()
                    .insert("OPRA.PILLAR".to_string(), opra_tx);
                let first = InstrumentId::from("ESM4.GLBX");
                let second = InstrumentId::from("NQM4.GLBX");
                let third = InstrumentId::from("SPY_OPTION.XCBO");
                let params = Params::new([(PRICE_PRECISION_PARAM.to_string(), json!(5))].into());
                let command = $command(Some(params))
                    .with_instrument_ids([second, third, first, second]);
                let original = serde_json::to_value(&command).unwrap();
                client.$subscribe(command.clone()).unwrap();
                assert!(
                    matches!(glbx_rx.try_recv().unwrap(), HandlerCommand::SetPricePrecision(symbol, 5) if symbol == second.symbol)
                );
                assert!(
                    matches!(glbx_rx.try_recv().unwrap(), HandlerCommand::SetPricePrecision(symbol, 5) if symbol == first.symbol)
                );

                let HandlerCommand::Subscribe(glbx_subscription) = glbx_rx.try_recv().unwrap()
                else {
                    panic!("expected GLBX subscription")
                };

                assert!(
                    matches!(opra_rx.try_recv().unwrap(), HandlerCommand::SetPricePrecision(symbol, 5) if symbol == third.symbol)
                );

                let HandlerCommand::Subscribe(opra_subscription) = opra_rx.try_recv().unwrap()
                else {
                    panic!("expected OPRA subscription")
                };

                let expected_glbx = Subscription::builder()
                    .schema(dbn::Schema::$schema)
                    .symbols(vec![second.symbol.to_string(), first.symbol.to_string()])
                    .build();
                let expected_opra = Subscription::builder()
                    .schema(dbn::Schema::$schema)
                    .symbols(vec![third.symbol.to_string()])
                    .build();
                assert_eq!(glbx_subscription, expected_glbx);
                assert_eq!(opra_subscription, expected_opra);
                assert_eq!(
                    glbx_rx.try_recv().unwrap_err(),
                    tokio::sync::mpsc::error::TryRecvError::Empty
                );
                assert_eq!(
                    opra_rx.try_recv().unwrap_err(),
                    tokio::sync::mpsc::error::TryRecvError::Empty
                );
                assert_eq!(client.symbol_venue_map.len(), 3);

                for id in [first, second, third] {
                    assert_eq!(
                        client.symbol_venue_map.get_cloned(&id.symbol),
                        Some(id.venue)
                    );
                }

                assert_eq!(serde_json::to_value(command).unwrap(), original);
            }

            #[rstest]
            #[case::failure_first(true)]
            #[case::failure_middle(false)]
            fn $failure_test(
                #[case] failure_first: bool,
                #[values(false, true)] previous_mapping: bool,
            ) {
                let mut client = test_data_client();
                let (glbx_tx, mut glbx_rx) = tokio::sync::mpsc::unbounded_channel();
                let (opra_tx, opra_rx) = tokio::sync::mpsc::unbounded_channel();
                let (equs_tx, mut equs_rx) = tokio::sync::mpsc::unbounded_channel();
                drop(opra_rx);
                client
                    .cmd_channels
                    .lock()
                    .insert("GLBX.MDP3".to_string(), glbx_tx);
                client
                    .cmd_channels
                    .lock()
                    .insert("OPRA.PILLAR".to_string(), opra_tx);
                client
                    .cmd_channels
                    .lock()
                    .insert("EQUS.MINI".to_string(), equs_tx);
                let first = InstrumentId::from("ESM4.GLBX");
                let second = InstrumentId::from("SPY_OPTION.XCBO");
                let third = InstrumentId::from("AAPL.EQUS");
                let previous = previous_mapping.then_some(Venue::from("PREVIOUS"));
                if let Some(venue) = previous {
                    client.symbol_venue_map.insert(second.symbol, venue);
                }
                let ids = if failure_first {
                    [second, first, third]
                } else {
                    [first, second, third]
                };
                let command = $command(None).with_instrument_ids(ids);
                let result = client.$subscribe(command);
                let error = result.unwrap_err();
                let partial = error
                    .downcast_ref::<SubscriptionError>()
                    .expect("partial subscription error");

                assert_eq!(partial.instrument_ids_accepted, vec![first, third]);
                assert_eq!(partial.bar_types_accepted, Vec::<BarType>::new());
                assert_eq!(
                    partial.source.to_string(),
                    "Failed to send command to dataset OPRA.PILLAR: channel closed"
                );
                assert_eq!(client.symbol_venue_map.get_cloned(&second.symbol), previous);

                for (id, receiver) in [(first, &mut glbx_rx), (third, &mut equs_rx)] {
                    let HandlerCommand::Subscribe(accepted) = receiver.try_recv().unwrap() else {
                        panic!("expected accepted subscription");
                    };
                    let expected = Subscription::builder()
                        .schema(dbn::Schema::$schema)
                        .symbols(vec![id.symbol.to_string()])
                        .build();
                    assert_eq!(accepted, expected);
                    assert_eq!(client.symbol_venue_map.get_cloned(&id.symbol), Some(id.venue));
                    assert_eq!(
                        receiver.try_recv().unwrap_err(),
                        tokio::sync::mpsc::error::TryRecvError::Empty
                    );
                }
            }

            #[rstest]
            #[case::unknown_dataset("UNKNOWN.UNKNOWN", "No dataset found for venue: UNKNOWN")]
            #[case::ambiguous_symbol(
                "ESM4.XCBO",
                "Identifiers share raw symbol ESM4 across different venues"
            )]
            fn $validation_test(#[case] invalid_id: &str, #[case] error: &str) {
                let mut client = test_data_client();
                let command = $command(None).with_instrument_ids([
                    InstrumentId::from("ESM4.GLBX"),
                    InstrumentId::from(invalid_id),
                ]);
                let result = client.$subscribe(command);
                assert_eq!(result.unwrap_err().to_string(), error);
                assert!(client.cmd_channels.lock().is_empty());
                assert!(client.symbol_venue_map.is_empty());
            }
        };
    }

    batch_tests!(
        test_quote_batches_send_one_subscription_per_dataset,
        test_quote_batch_enqueue_failure_can_follow_accepted_dataset,
        test_quote_batches_validate_all_members_before_dispatch,
        subscribe_quotes_cmd,
        subscribe_quotes,
        Mbp1
    );

    batch_tests!(
        test_trade_batches_send_one_subscription_per_dataset,
        test_trade_batch_enqueue_failure_can_follow_accepted_dataset,
        test_trade_batches_validate_all_members_before_dispatch,
        subscribe_trades_cmd,
        subscribe_trades,
        Trades
    );

    #[rstest]
    fn test_trade_batch_unsubscribe_is_noop() {
        let mut client = test_data_client();

        let command = UnsubscribeTrades::new(
            [
                InstrumentId::from("ESM4.GLBX"),
                InstrumentId::from("SPY_OPTION.XCBO"),
            ],
            Some(ClientId::from("DATABENTO")),
            None,
            UUID4::new(),
            UnixNanos::from(37),
            None,
            None,
        );
        client.unsubscribe_trades(&command).unwrap();
        assert!(client.cmd_channels.lock().is_empty());
        assert!(client.symbol_venue_map.is_empty());
    }

    #[rstest]
    fn test_send_subscription_commands_starts_after_subscribe() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let subscription = Subscription::builder()
            .schema(dbn::Schema::Mbp1)
            .symbols(vec!["ESM4"])
            .build();

        send_subscription_commands(
            &tx,
            "GLBX.MDP3",
            Some((Symbol::from("ESM4"), 2)),
            subscription,
            true,
        )
        .unwrap();

        assert!(matches!(
            rx.try_recv().unwrap(),
            HandlerCommand::SetPricePrecision(symbol, 2) if symbol == Symbol::from("ESM4")
        ));
        assert!(matches!(
            rx.try_recv().unwrap(),
            HandlerCommand::Subscribe(sub) if sub.schema == dbn::Schema::Mbp1
        ));
        assert!(matches!(rx.try_recv().unwrap(), HandlerCommand::Start));
    }

    #[rstest]
    #[case::second("ESM4.GLBX-1-SECOND-LAST-EXTERNAL", dbn::Schema::Ohlcv1S)]
    #[case::minute("ESM4.GLBX-1-MINUTE-LAST-EXTERNAL", dbn::Schema::Ohlcv1M)]
    #[case::hour("ESM4.GLBX-1-HOUR-LAST-EXTERNAL", dbn::Schema::Ohlcv1H)]
    #[case::day("ESM4.GLBX-1-DAY-LAST-EXTERNAL", dbn::Schema::Ohlcv1D)]
    fn test_ohlcv_schema_maps_aggregation(#[case] bar_type: &str, #[case] expected: dbn::Schema) {
        let schema = ohlcv_schema(BarType::from(bar_type)).unwrap();

        assert_eq!(schema, expected);
    }

    #[rstest]
    #[case::step("ESM4.GLBX-5-MINUTE-LAST-EXTERNAL")]
    #[case::aggregation("ESM4.GLBX-100-TICK-LAST-EXTERNAL")]
    #[case::bid("ESM4.GLBX-1-MINUTE-BID-EXTERNAL")]
    #[case::mid("ESM4.GLBX-1-MINUTE-MID-EXTERNAL")]
    fn test_ohlcv_schema_rejects_unsupported_bar_type(#[case] bar_type: &str) {
        assert!(ohlcv_schema(BarType::from(bar_type)).is_err());
    }

    #[rstest]
    #[case::statistics("DatabentoStatistics", dbn::Schema::Statistics)]
    #[case::imbalance("DatabentoImbalance", dbn::Schema::Imbalance)]
    fn test_subscribe_custom_data_sends_schema_subscription(
        #[case] type_name: &str,
        #[case] expected: dbn::Schema,
    ) {
        let mut client = test_data_client();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client
            .cmd_channels
            .lock()
            .insert("GLBX.MDP3".to_string(), tx);
        let command = SubscribeCustomData::new(
            Some(ClientId::from("DATABENTO-TEST")),
            None,
            DataType::new(type_name, None, Some("ESM4.GLBX".to_string())),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );

        client.subscribe(command).unwrap();

        let HandlerCommand::Subscribe(subscription) = rx.try_recv().unwrap() else {
            panic!("expected subscription");
        };
        assert_eq!(subscription.schema, expected);
        assert_eq!(
            subscription.symbols,
            databento::Symbols::Symbols(vec!["ESM4".to_string()])
        );
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    #[case::unsupported_type("DatabentoOther", Some("ESM4.GLBX"))]
    #[case::missing_identifier("DatabentoStatistics", None)]
    #[case::invalid_identifier("DatabentoStatistics", Some("ESM4"))]
    fn test_subscribe_custom_data_rejects_invalid_data_type(
        #[case] type_name: &str,
        #[case] identifier: Option<&str>,
    ) {
        let mut client = test_data_client();
        let command = SubscribeCustomData::new(
            Some(ClientId::from("DATABENTO-TEST")),
            None,
            DataType::new(type_name, None, identifier.map(str::to_string)),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );

        assert!(client.subscribe(command).is_err());
        assert!(client.cmd_channels.lock().is_empty());
    }

    #[rstest]
    fn test_custom_data_for_instrument_matches_subscribed_topic() {
        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let statistics = DatabentoStatistics::new(
            instrument_id,
            DatabentoStatisticType::SettlementPrice,
            DatabentoStatisticUpdateAction::Added,
            Some(Price::from("5000.25")),
            None,
            1,
            2,
            3,
            UnixNanos::from(4),
            5,
            UnixNanos::from(6),
            UnixNanos::from(7),
            UnixNanos::from(8),
        );
        let subscribed = DataType::new("DatabentoStatistics", None, Some("ESM4.GLBX".to_string()));

        let custom = custom_data_for_instrument(Arc::new(statistics), instrument_id);

        assert_eq!(custom.data_type, subscribed);
        assert_eq!(
            get_custom_topic(&custom.data_type),
            get_custom_topic(&subscribed)
        );
        assert_eq!(custom.data_type.identifier(), Some("ESM4.GLBX"));
    }

    #[rstest]
    fn test_subscribe_schema_sends_raw_symbol() {
        let client = test_data_client();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client
            .cmd_channels
            .lock()
            .insert("GLBX.MDP3".to_string(), tx);

        client
            .subscribe_schema(InstrumentId::from("ESM4.GLBX"), dbn::Schema::Ohlcv1M)
            .unwrap();

        let HandlerCommand::Subscribe(subscription) = rx.try_recv().unwrap() else {
            panic!("expected subscription");
        };
        assert_eq!(subscription.stype_in, dbn::SType::RawSymbol);
        assert_eq!(
            subscription.symbols,
            databento::Symbols::Symbols(vec!["ESM4".to_string()])
        );
    }

    #[rstest]
    #[case::continuous("ES.c.0.GLBX")]
    #[case::parent("ES.FUT.GLBX")]
    #[case::instrument_id("12345.GLBX")]
    fn test_subscribe_schema_rejects_non_raw_symbols(#[case] instrument_id: &str) {
        let client = test_data_client();

        let result = client.subscribe_schema(InstrumentId::from(instrument_id), dbn::Schema::Mbp10);

        assert!(result.is_err());
        assert!(client.cmd_channels.lock().is_empty());
    }

    #[rstest]
    fn test_subscribe_custom_data_rejects_metadata() {
        let mut client = test_data_client();
        let metadata = Params::new([("venue".to_string(), json!("GLBX"))].into());

        let command = SubscribeCustomData::new(
            Some(ClientId::from("DATABENTO-TEST")),
            None,
            DataType::new(
                "DatabentoStatistics",
                Some(metadata),
                Some("ESM4.GLBX".to_string()),
            ),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );

        assert!(client.subscribe(command).is_err());
        assert!(client.cmd_channels.lock().is_empty());
    }

    #[rstest]
    fn test_subscribed_custom_data_reaches_subscriber() {
        let instrument_id = InstrumentId::from("ESM4.GLBX");
        let subscribed = DataType::new("DatabentoStatistics", None, Some("ESM4.GLBX".to_string()));
        let other = DataType::new("DatabentoStatistics", None, Some("ESU4.GLBX".to_string()));
        let handler = msgbus::stubs::get_message_saving_handler::<CustomData>(None);
        msgbus::subscribe_any(get_custom_topic(&subscribed).into(), handler.clone(), None);
        let statistics = DatabentoStatistics::new(
            instrument_id,
            DatabentoStatisticType::SettlementPrice,
            DatabentoStatisticUpdateAction::Added,
            Some(Price::from("5000.25")),
            None,
            1,
            2,
            3,
            UnixNanos::from(4),
            5,
            UnixNanos::from(6),
            UnixNanos::from(7),
            UnixNanos::from(8),
        );
        let custom = custom_data_for_instrument(Arc::new(statistics), instrument_id);

        msgbus::publish_any(get_custom_topic(&custom.data_type), &custom);
        msgbus::publish_any(get_custom_topic(&other), &custom);

        let received = msgbus::stubs::get_saved_messages::<CustomData>(&handler);
        assert_eq!(received, vec![custom]);
    }

    #[rstest]
    fn test_subscribe_book_depth_accepts_depth_ten() {
        let mut client = test_data_client();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client
            .cmd_channels
            .lock()
            .insert("GLBX.MDP3".to_string(), tx);

        client
            .subscribe_book_depth(subscribe_book_depth_cmd(NonZeroUsize::new(10)))
            .unwrap();

        let HandlerCommand::Subscribe(subscription) = rx.try_recv().unwrap() else {
            panic!("expected subscription");
        };
        assert_eq!(subscription.schema, dbn::Schema::Mbp10);
    }

    #[rstest]
    #[case::five(5)]
    #[case::twenty(20)]
    fn test_subscribe_book_depth_rejects_other_depths(#[case] depth: usize) {
        let mut client = test_data_client();

        let result =
            client.subscribe_book_depth(subscribe_book_depth_cmd(NonZeroUsize::new(depth)));

        assert!(result.is_err());
        assert!(client.cmd_channels.lock().is_empty());
    }

    #[rstest]
    fn test_subscribe_bars_sends_ohlcv_subscription() {
        let mut client = test_data_client();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client
            .cmd_channels
            .lock()
            .insert("GLBX.MDP3".to_string(), tx);
        let command = SubscribeBars::new(
            BarType::from("ESM4.GLBX-1-MINUTE-LAST-EXTERNAL"),
            Some(ClientId::from("DATABENTO-TEST")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );

        client.subscribe_bars(command).unwrap();

        let HandlerCommand::Subscribe(subscription) = rx.try_recv().unwrap() else {
            panic!("expected subscription");
        };
        assert_eq!(subscription.schema, dbn::Schema::Ohlcv1M);
        assert_eq!(
            subscription.symbols,
            databento::Symbols::Symbols(vec!["ESM4".to_string()])
        );
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_subscribe_book_depth_sends_mbp10_subscription() {
        let mut client = test_data_client();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client
            .cmd_channels
            .lock()
            .insert("GLBX.MDP3".to_string(), tx);
        client
            .subscribe_book_depth(subscribe_book_depth_cmd(None))
            .unwrap();

        let HandlerCommand::Subscribe(subscription) = rx.try_recv().unwrap() else {
            panic!("expected subscription");
        };
        assert_eq!(subscription.schema, dbn::Schema::Mbp10);
        assert_eq!(
            subscription.symbols,
            databento::Symbols::Symbols(vec!["ESM4".to_string()])
        );
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_send_subscription_commands_without_precision_or_start() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let subscription = Subscription::builder()
            .schema(dbn::Schema::Mbp1)
            .symbols(vec!["ESM4"])
            .build();

        send_subscription_commands(&tx, "GLBX.MDP3", None, subscription, false).unwrap();

        assert!(matches!(
            rx.try_recv().unwrap(),
            HandlerCommand::Subscribe(sub) if sub.schema == dbn::Schema::Mbp1
        ));
        assert!(matches!(
            rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
    }
}
