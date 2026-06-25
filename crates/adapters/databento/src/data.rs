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
    fmt::Debug,
    path::PathBuf,
    str::FromStr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::AHashMap;
use databento::{dbn, live::Subscription};
use indexmap::IndexMap;
use nautilus_common::{
    clients::DataClient,
    live::{runner::get_data_event_sender, runtime::get_runtime},
    messages::{
        DataEvent, DataResponse,
        data::{
            BarsResponse, InstrumentResponse, InstrumentsResponse, QuotesResponse, RequestBars,
            RequestInstrument, RequestInstruments, RequestQuotes, RequestTrades,
            SubscribeBookDeltas, SubscribeInstrument, SubscribeInstrumentStatus, SubscribeQuotes,
            SubscribeTrades, TradesResponse, UnsubscribeBookDeltas, UnsubscribeInstrumentStatus,
            UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
};
use nautilus_core::{
    AtomicMap, MUTEX_POISONED, Params, UnixNanos,
    string::secret::REDACTED,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{CustomData, Data},
    enums::BarAggregation,
    identifiers::{ClientId, InstrumentId, Symbol, Venue},
    instruments::{Instrument, InstrumentAny},
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::{
    common::{Credential, DATABENTO_VENUE},
    historical::{DatabentoHistoricalClient, RangeQueryParams},
    live::{DatabentoFeedHandler, DatabentoMessage, HandlerCommand},
    loader::DatabentoDataLoader,
    symbology::instrument_id_to_symbol_string,
    types::{Dataset, PublisherId},
};

const PRICE_PRECISION_PARAM: &str = "price_precision";
const SCHEMA_PARAM: &str = "schema";
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
#[derive(Clone)]
pub struct DatabentoDataClientConfig {
    /// Databento API credential.
    pub(crate) credential: Credential,
    /// Path to publishers.json file.
    pub publishers_filepath: PathBuf,
    /// Venue-to-dataset overrides applied on top of the publishers.json mappings.
    pub venue_dataset_map: IndexMap<String, String>,
    /// Whether to use exchange as venue for GLBX instruments.
    pub use_exchange_as_venue: bool,
    /// Whether to timestamp bars on close.
    pub bars_timestamp_on_close: bool,
    /// Reconnection timeout in minutes (None for infinite retries).
    pub reconnect_timeout_mins: Option<u64>,
}

impl Debug for DatabentoDataClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(DatabentoDataClientConfig))
            .field("credential", &REDACTED)
            .field("publishers_filepath", &self.publishers_filepath)
            .field("venue_dataset_map", &self.venue_dataset_map)
            .field("use_exchange_as_venue", &self.use_exchange_as_venue)
            .field("bars_timestamp_on_close", &self.bars_timestamp_on_close)
            .field("reconnect_timeout_mins", &self.reconnect_timeout_mins)
            .finish()
    }
}

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
    /// Client identifier.
    client_id: ClientId,
    /// Client configuration.
    config: DatabentoDataClientConfig,
    /// Connection state.
    is_connected: AtomicBool,
    /// Historical client for on-demand data requests.
    historical: DatabentoHistoricalClient,
    /// Data loader for venue-to-dataset mapping.
    loader: DatabentoDataLoader,
    /// Feed handler command senders per dataset.
    cmd_channels: Arc<Mutex<AHashMap<String, tokio::sync::mpsc::UnboundedSender<HandlerCommand>>>>,
    /// Task handles for lifecycle management.
    task_handles: Arc<Mutex<Vec<JoinHandle<()>>>>,
    /// Cancellation token for graceful shutdown.
    cancellation_token: CancellationToken,
    /// Publisher to venue mapping.
    publisher_venue_map: Arc<IndexMap<PublisherId, Venue>>,
    /// Symbol to venue mapping (for caching).
    symbol_venue_map: Arc<AtomicMap<Symbol, Venue>>,
    /// Data event sender for forwarding data to the async runner.
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
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
        let historical = DatabentoHistoricalClient::new(
            config.credential.clone(),
            config.publishers_filepath.clone(),
            clock,
            config.use_exchange_as_venue,
        )?;

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

        Ok(Self {
            client_id,
            config,
            is_connected: AtomicBool::new(false),
            historical,
            loader,
            cmd_channels: Arc::new(Mutex::new(AHashMap::new())),
            task_handles: Arc::new(Mutex::new(Vec::new())),
            cancellation_token: CancellationToken::new(),
            publisher_venue_map: Arc::new(publisher_venue_map),
            symbol_venue_map: Arc::new(AtomicMap::new()),
            data_sender,
        })
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
        let mut channels = self.cmd_channels.lock().expect(MUTEX_POISONED);

        if !channels.contains_key(dataset) {
            log::info!("Creating new feed handler for dataset: {dataset}");
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
        price_precision: Option<(Symbol, u8)>,
        subscription: Subscription,
        start_after_subscribe: bool,
    ) -> anyhow::Result<()> {
        let tx = {
            let channels = self.cmd_channels.lock().expect(MUTEX_POISONED);
            channels
                .get(dataset)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("No feed handler found for dataset: {dataset}"))?
        };

        send_subscription_commands(
            &tx,
            dataset,
            price_precision,
            subscription,
            start_after_subscribe,
        )
    }

    fn send_close_to_active_feeds(&self) {
        let channels = self.cmd_channels.lock().expect(MUTEX_POISONED);
        for (dataset, tx) in channels.iter() {
            if let Err(e) = tx.send(HandlerCommand::Close) {
                log::warn!("Failed to send close command to dataset {dataset}: {e}");
            }
        }
    }

    fn clear_feed_channels(&self) {
        let mut channels = self.cmd_channels.lock().expect(MUTEX_POISONED);
        channels.clear();
    }

    /// Initializes the live feed handler for streaming data.
    fn initialize_live_feed(
        &self,
        dataset: String,
    ) -> tokio::sync::mpsc::UnboundedSender<HandlerCommand> {
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (msg_tx, msg_rx) = tokio::sync::mpsc::unbounded_channel();

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

        let feed_handle = get_runtime().spawn(async move {
            if let Err(e) = feed_handler.run().await {
                log::error!("Feed handler error: {e}");
            }
        });

        let cancellation_token = self.cancellation_token.clone();
        let data_sender = self.data_sender.clone();

        // Spawn message processing task with cancellation support
        let msg_handle = get_runtime().spawn(async move {
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
                                log::info!("Received instrument definition: {}", instrument.id());
                                if let Err(e) = data_sender.send(DataEvent::Instrument(*instrument)) {
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
                                let data = Data::Custom(CustomData::from_arc(Arc::new(imbalance)));
                                if let Err(e) = data_sender.send(DataEvent::Data(data)) {
                                    log::error!("Failed to send imbalance data event: {e}");
                                }
                            }
                            Some(DatabentoMessage::Statistics(statistics)) => {
                                log::debug!("Received statistics: {statistics:?}");
                                let data = Data::Custom(CustomData::from_arc(Arc::new(statistics)));
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
                                log::info!("Feed handler closed");
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
        });

        {
            let mut handles = self.task_handles.lock().expect(MUTEX_POISONED);
            handles.push(feed_handle);
            handles.push(msg_handle);
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
        self.cancellation_token = CancellationToken::new();

        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::debug!("Resetting");
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        log::debug!("Disposing");
        self.stop()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        log::debug!("Connecting...");

        if self.cancellation_token.is_cancelled() {
            self.cancellation_token = CancellationToken::new();
        }

        self.is_connected.store(true, Ordering::Relaxed);

        log::info!("Connected");
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        log::debug!("Disconnecting...");

        self.send_close_to_active_feeds();
        self.clear_feed_channels();

        let handles = {
            let mut task_handles = self.task_handles.lock().expect(MUTEX_POISONED);
            std::mem::take(&mut *task_handles)
        };

        for handle in handles {
            if let Err(e) = handle.await
                && !e.is_cancelled()
            {
                log::error!("Task join error: {e}");
            }
        }

        self.is_connected.store(false, Ordering::Relaxed);
        self.cancellation_token = CancellationToken::new();

        log::info!("Disconnected");
        Ok(())
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
        log::debug!("Subscribe instrument: {cmd:?}");

        let dataset = self.get_dataset_for_venue(cmd.instrument_id.venue)?;
        let start_after_subscribe = self.get_or_create_feed_handler(&dataset);

        self.symbol_venue_map
            .insert(cmd.instrument_id.symbol, cmd.instrument_id.venue);
        let symbol = cmd.instrument_id.symbol.to_string();

        let subscription = Subscription::builder()
            .schema(databento::dbn::Schema::Definition)
            .symbols(symbol)
            .build();

        self.send_subscription_to_dataset(&dataset, None, subscription, start_after_subscribe)?;

        Ok(())
    }

    /// Subscribes to quote tick data for the specified instruments.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    fn subscribe_quotes(&mut self, cmd: SubscribeQuotes) -> anyhow::Result<()> {
        log::debug!("Subscribe quotes: {cmd:?}");

        let dataset = self.get_dataset_for_venue(cmd.instrument_id.venue)?;
        let symbol = cmd.instrument_id.symbol.to_string();
        let price_precision = price_precision_from_params(cmd.params.as_ref())?
            .map(|precision| (cmd.instrument_id.symbol, precision));
        let schema = schema_from_params(cmd.params.as_ref(), dbn::Schema::Mbp1, QUOTE_SCHEMAS)?;

        let subscription = Subscription::builder()
            .schema(schema)
            .symbols(symbol)
            .build();

        let start_after_subscribe = self.get_or_create_feed_handler(&dataset);
        self.symbol_venue_map
            .insert(cmd.instrument_id.symbol, cmd.instrument_id.venue);

        self.send_subscription_to_dataset(
            &dataset,
            price_precision,
            subscription,
            start_after_subscribe,
        )?;

        Ok(())
    }

    /// Subscribes to trade tick data for the specified instruments.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()> {
        log::debug!("Subscribe trades: {cmd:?}");

        let dataset = self.get_dataset_for_venue(cmd.instrument_id.venue)?;
        let symbol = cmd.instrument_id.symbol.to_string();
        let price_precision = price_precision_from_params(cmd.params.as_ref())?
            .map(|precision| (cmd.instrument_id.symbol, precision));
        let schema = schema_from_params(cmd.params.as_ref(), dbn::Schema::Trades, TRADE_SCHEMAS)?;

        let subscription = Subscription::builder()
            .schema(schema)
            .symbols(symbol)
            .build();

        let start_after_subscribe = self.get_or_create_feed_handler(&dataset);
        self.symbol_venue_map
            .insert(cmd.instrument_id.symbol, cmd.instrument_id.venue);

        self.send_subscription_to_dataset(
            &dataset,
            price_precision,
            subscription,
            start_after_subscribe,
        )?;

        Ok(())
    }

    /// Subscribes to order book delta updates for the specified instruments.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    fn subscribe_book_deltas(&mut self, cmd: SubscribeBookDeltas) -> anyhow::Result<()> {
        log::debug!("Subscribe book deltas: {cmd:?}");

        let dataset = self.get_dataset_for_venue(cmd.instrument_id.venue)?;
        let start_after_subscribe = self.get_or_create_feed_handler(&dataset);

        self.symbol_venue_map
            .insert(cmd.instrument_id.symbol, cmd.instrument_id.venue);
        let symbol = cmd.instrument_id.symbol.to_string();

        let subscription = Subscription::builder()
            .schema(databento::dbn::Schema::Mbo) // Market by order for book deltas
            .symbols(symbol)
            .build();

        self.send_subscription_to_dataset(&dataset, None, subscription, start_after_subscribe)?;

        Ok(())
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
        log::debug!("Subscribe instrument status: {cmd:?}");

        let dataset = self.get_dataset_for_venue(cmd.instrument_id.venue)?;
        let start_after_subscribe = self.get_or_create_feed_handler(&dataset);

        self.symbol_venue_map
            .insert(cmd.instrument_id.symbol, cmd.instrument_id.venue);
        let symbol = cmd.instrument_id.symbol.to_string();

        let subscription = Subscription::builder()
            .schema(databento::dbn::Schema::Status)
            .symbols(symbol)
            .build();

        self.send_subscription_to_dataset(&dataset, None, subscription, start_after_subscribe)?;

        Ok(())
    }

    // Unsubscribe methods
    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        log::debug!("Unsubscribe quotes: {cmd:?}");

        // Note: Databento live API doesn't support granular unsubscribing.
        // The feed handler manages subscriptions and can handle reconnections
        // with the appropriate subscription state.
        log::warn!(
            "Databento does not support granular unsubscribing - ignoring unsubscribe request for {}",
            cmd.instrument_id
        );

        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        log::debug!("Unsubscribe trades: {cmd:?}");

        // Note: Databento live API doesn't support granular unsubscribing.
        // The feed handler manages subscriptions and can handle reconnections
        // with the appropriate subscription state.
        log::warn!(
            "Databento does not support granular unsubscribing - ignoring unsubscribe request for {}",
            cmd.instrument_id
        );

        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        log::debug!("Unsubscribe book deltas: {cmd:?}");

        // Note: Databento live API doesn't support granular unsubscribing.
        // The feed handler manages subscriptions and can handle reconnections
        // with the appropriate subscription state.
        log::warn!(
            "Databento does not support granular unsubscribing - ignoring unsubscribe request for {}",
            cmd.instrument_id
        );

        Ok(())
    }

    fn unsubscribe_instrument_status(
        &mut self,
        cmd: &UnsubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        log::debug!("Unsubscribe instrument status: {cmd:?}");

        // Note: Databento live API doesn't support granular unsubscribing.
        // The feed handler manages subscriptions and can handle reconnections
        // with the appropriate subscription state.
        log::warn!(
            "Databento does not support granular unsubscribing - ignoring unsubscribe request for {}",
            cmd.instrument_id
        );

        Ok(())
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
        let start_nanos = request
            .start
            .map(|dt| UnixNanos::from(dt.timestamp_nanos_opt().unwrap_or(0) as u64));
        let end_nanos = request
            .end
            .map(|dt| UnixNanos::from(dt.timestamp_nanos_opt().unwrap_or(0) as u64));
        let request_params = request.params;

        get_runtime().spawn(async move {
            let query_params = instruments_query_params(dataset, start_nanos, end_nanos);

            match historical_client.get_range_instruments(query_params).await {
                Ok(instruments) => {
                    log::info!("Retrieved {} instruments", instruments.len());

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

                    if let Err(e) = data_sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send instruments response: {e}");
                    }
                }
                Err(e) => {
                    log::error!("Failed to request instruments: {e}");
                }
            }
        });

        Ok(())
    }

    fn request_instrument(&self, request: RequestInstrument) -> anyhow::Result<()> {
        log::debug!("Request instrument: {request:?}");

        let dataset = self.get_dataset_for_venue(request.instrument_id.venue)?;
        let historical_client = self.historical.clone();
        let data_sender = self.data_sender.clone();
        let instrument_id = request.instrument_id;
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start_nanos = request
            .start
            .map(|dt| UnixNanos::from(dt.timestamp_nanos_opt().unwrap_or(0) as u64));
        let end_nanos = request
            .end
            .map(|dt| UnixNanos::from(dt.timestamp_nanos_opt().unwrap_or(0) as u64));
        let request_params = request.params;

        get_runtime().spawn(async move {
            let query_params =
                instrument_query_params(dataset, instrument_id, start_nanos, end_nanos);

            match historical_client.get_range_instruments(query_params).await {
                Ok(instruments) => {
                    let instrument = requested_instrument(instruments, instrument_id);

                    let Some(instrument) = instrument else {
                        log::error!("Instrument not found: {instrument_id}");
                        return;
                    };

                    let response = DataResponse::Instrument(Box::new(InstrumentResponse::new(
                        request_id,
                        client_id,
                        instrument.id(),
                        instrument,
                        start_nanos,
                        end_nanos,
                        get_atomic_clock_realtime().get_time_ns(),
                        request_params,
                    )));

                    if let Err(e) = data_sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send instrument response: {e}");
                    }
                }
                Err(e) => {
                    log::error!("Failed to request instrument {instrument_id}: {e}");
                }
            }
        });

        Ok(())
    }

    fn request_quotes(&self, request: RequestQuotes) -> anyhow::Result<()> {
        log::debug!("Request quotes: {request:?}");

        let historical_client = self.historical.clone();
        let data_sender = self.data_sender.clone();
        let dataset = self.get_dataset_for_venue(request.instrument_id.venue)?;
        let instrument_id = request.instrument_id;
        let symbols = historical_client.prepare_symbols_from_instrument_ids(&[instrument_id]);
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start_nanos = request
            .start
            .map(|dt| UnixNanos::from(dt.timestamp_nanos_opt().unwrap_or(0) as u64));
        let end_nanos = request
            .end
            .map(|dt| UnixNanos::from(dt.timestamp_nanos_opt().unwrap_or(0) as u64));
        let limit = request.limit.map(|limit| limit.get() as u64);
        let request_params = request.params;
        let price_precision = price_precision_from_params(request_params.as_ref())?;
        let schema = schema_from_params(request_params.as_ref(), dbn::Schema::Mbp1, QUOTE_SCHEMAS)?
            .to_string();

        get_runtime().spawn(async move {
            let params = RangeQueryParams {
                dataset,
                symbols,
                start: start_nanos.unwrap_or_default(),
                end: end_nanos,
                limit,
                price_precision,
            };

            match historical_client
                .get_range_quotes(params, Some(schema))
                .await
            {
                Ok(quotes) => {
                    log::info!("Retrieved {} quotes", quotes.len());
                    let response = DataResponse::Quotes(QuotesResponse::new(
                        request_id,
                        client_id,
                        instrument_id,
                        quotes,
                        start_nanos,
                        end_nanos,
                        get_atomic_clock_realtime().get_time_ns(),
                        request_params,
                    ));

                    if let Err(e) = data_sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send quotes response: {e}");
                    }
                }
                Err(e) => {
                    log::error!("Failed to request quotes: {e}");
                }
            }
        });

        Ok(())
    }

    fn request_trades(&self, request: RequestTrades) -> anyhow::Result<()> {
        log::debug!("Request trades: {request:?}");

        let historical_client = self.historical.clone();
        let data_sender = self.data_sender.clone();
        let dataset = self.get_dataset_for_venue(request.instrument_id.venue)?;
        let instrument_id = request.instrument_id;
        let symbols = historical_client.prepare_symbols_from_instrument_ids(&[instrument_id]);
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start_nanos = request
            .start
            .map(|dt| UnixNanos::from(dt.timestamp_nanos_opt().unwrap_or(0) as u64));
        let end_nanos = request
            .end
            .map(|dt| UnixNanos::from(dt.timestamp_nanos_opt().unwrap_or(0) as u64));
        let limit = request.limit.map(|limit| limit.get() as u64);
        let request_params = request.params;
        let price_precision = price_precision_from_params(request_params.as_ref())?;
        let schema =
            schema_from_params(request_params.as_ref(), dbn::Schema::Trades, TRADE_SCHEMAS)?
                .to_string();

        get_runtime().spawn(async move {
            let params = RangeQueryParams {
                dataset,
                symbols,
                start: start_nanos.unwrap_or_default(),
                end: end_nanos,
                limit,
                price_precision,
            };

            match historical_client
                .get_range_trades(params, Some(schema))
                .await
            {
                Ok(trades) => {
                    log::info!("Retrieved {} trades", trades.len());
                    let response = DataResponse::Trades(TradesResponse::new(
                        request_id,
                        client_id,
                        instrument_id,
                        trades,
                        start_nanos,
                        end_nanos,
                        get_atomic_clock_realtime().get_time_ns(),
                        request_params,
                    ));

                    if let Err(e) = data_sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send trades response: {e}");
                    }
                }
                Err(e) => {
                    log::error!("Failed to request trades: {e}");
                }
            }
        });

        Ok(())
    }

    fn request_bars(&self, request: RequestBars) -> anyhow::Result<()> {
        log::debug!("Request bars: {request:?}");

        let historical_client = self.historical.clone();
        let data_sender = self.data_sender.clone();
        let instrument_id = request.bar_type.instrument_id();
        let dataset = self.get_dataset_for_venue(instrument_id.venue)?;
        let symbols = historical_client.prepare_symbols_from_instrument_ids(&[instrument_id]);
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let bar_type = request.bar_type;
        let start_nanos = request
            .start
            .map(|dt| UnixNanos::from(dt.timestamp_nanos_opt().unwrap_or(0) as u64));
        let end_nanos = request
            .end
            .map(|dt| UnixNanos::from(dt.timestamp_nanos_opt().unwrap_or(0) as u64));
        let limit = request.limit.map(|limit| limit.get() as u64);
        let request_params = request.params;
        let price_precision = price_precision_from_params(request_params.as_ref())?;
        let timestamp_on_close = self.config.bars_timestamp_on_close;

        get_runtime().spawn(async move {
            let params = RangeQueryParams {
                dataset,
                symbols,
                start: start_nanos.unwrap_or_default(),
                end: end_nanos,
                limit,
                price_precision,
            };

            let aggregation = match bar_type.spec().aggregation {
                BarAggregation::Second => BarAggregation::Second,
                BarAggregation::Minute => BarAggregation::Minute,
                BarAggregation::Hour => BarAggregation::Hour,
                BarAggregation::Day => BarAggregation::Day,
                _ => {
                    log::error!(
                        "Unsupported bar aggregation: {:?}",
                        bar_type.spec().aggregation
                    );
                    return;
                }
            };

            match historical_client
                .get_range_bars(params, aggregation, timestamp_on_close)
                .await
            {
                Ok(bars) => {
                    log::info!("Retrieved {} bars", bars.len());
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

                    if let Err(e) = data_sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send bars response: {e}");
                    }
                }
                Err(e) => {
                    log::error!("Failed to request bars: {e}");
                }
            }
        });

        Ok(())
    }
}

fn instruments_query_params(
    dataset: String,
    start_nanos: Option<UnixNanos>,
    end_nanos: Option<UnixNanos>,
) -> RangeQueryParams {
    RangeQueryParams {
        dataset,
        symbols: vec!["ALL_SYMBOLS".to_string()],
        start: start_nanos.unwrap_or_default(),
        end: end_nanos,
        limit: None,
        price_precision: None,
    }
}

fn instrument_query_params(
    dataset: String,
    instrument_id: InstrumentId,
    start_nanos: Option<UnixNanos>,
    end_nanos: Option<UnixNanos>,
) -> RangeQueryParams {
    RangeQueryParams {
        dataset,
        symbols: vec![instrument_id_to_symbol_string(
            instrument_id,
            &mut AHashMap::new(),
        )],
        start: start_nanos.unwrap_or_default(),
        end: end_nanos,
        limit: None,
        price_precision: None,
    }
}

fn requested_instrument(
    instruments: Vec<InstrumentAny>,
    instrument_id: InstrumentId,
) -> Option<InstrumentAny> {
    instruments
        .into_iter()
        .rev()
        .find(|instrument| instrument.id() == instrument_id)
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
    price_precision: Option<(Symbol, u8)>,
    subscription: Subscription,
    start_after_subscribe: bool,
) -> anyhow::Result<()> {
    if let Some((symbol, precision)) = price_precision {
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
    use std::path::PathBuf;

    use nautilus_common::live::runner::replace_data_event_sender;
    use nautilus_core::UUID4;
    use nautilus_model::{
        identifiers::{ClientId, InstrumentId},
        instruments::{CurrencyPair, InstrumentAny},
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;
    use serde_json::json;

    use super::*;

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

    fn currency_pair_with_ts_init(instrument_id: &str, ts_init: UnixNanos) -> InstrumentAny {
        let instrument_id = InstrumentId::from(instrument_id);
        InstrumentAny::CurrencyPair(CurrencyPair::new(
            instrument_id,
            instrument_id.symbol,
            Currency::from("BTC"),
            Currency::from("USDT"),
            2,
            6,
            Price::from("0.01"),
            Quantity::from("0.000001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            ts_init,
        ))
    }

    #[rstest]
    fn test_instruments_query_params_requests_all_symbols() {
        let start = UnixNanos::from(1_000_000_000);
        let end = UnixNanos::from(2_000_000_000);

        let params = instruments_query_params("GLBX.MDP3".to_string(), Some(start), Some(end));

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

        let params = instrument_query_params("GLBX.MDP3".to_string(), instrument_id, None, None);

        assert_eq!(params.dataset, "GLBX.MDP3");
        assert_eq!(params.symbols, vec!["ESM4"]);
        assert_eq!(params.start, UnixNanos::default());
        assert_eq!(params.end, None);
        assert_eq!(params.limit, None);
        assert_eq!(params.price_precision, None);
    }

    #[rstest]
    fn test_requested_instrument_filters_exact_id() {
        let requested_id = InstrumentId::from("BTCUSDT.BINANCE");
        let instruments = vec![
            currency_pair("ETHUSDT.BINANCE"),
            currency_pair("BTCUSDT.BINANCE"),
        ];

        let instrument = requested_instrument(instruments, requested_id).expect("instrument");

        assert_eq!(instrument.id(), requested_id);
    }

    #[rstest]
    fn test_requested_instrument_returns_latest_matching_id() {
        let requested_id = InstrumentId::from("BTCUSDT.BINANCE");
        let instruments = vec![
            currency_pair_with_ts_init("BTCUSDT.BINANCE", UnixNanos::from(1)),
            currency_pair_with_ts_init("BTCUSDT.BINANCE", UnixNanos::from(2)),
        ];

        let instrument = requested_instrument(instruments, requested_id).expect("instrument");

        assert_eq!(instrument.ts_init(), UnixNanos::from(2));
    }

    #[rstest]
    fn test_requested_instrument_returns_none_on_miss() {
        let instruments = vec![currency_pair("ETHUSDT.BINANCE")];

        let instrument = requested_instrument(instruments, InstrumentId::from("BTCUSDT.BINANCE"));

        assert!(instrument.is_none());
    }

    #[rstest]
    fn test_price_precision_from_params() {
        let mut params = Params::new();
        params.insert(PRICE_PRECISION_PARAM.to_string(), json!(5));

        let price_precision = price_precision_from_params(Some(&params)).unwrap();

        assert_eq!(price_precision, Some(5));
    }

    #[rstest]
    fn test_price_precision_from_params_rejects_out_of_range_value() {
        let mut params = Params::new();
        params.insert(
            PRICE_PRECISION_PARAM.to_string(),
            json!(u64::from(u8::MAX) + 1),
        );

        let result = price_precision_from_params(Some(&params));

        assert!(result.is_err());
    }

    #[rstest]
    fn test_schema_from_params_returns_default() {
        let schema = schema_from_params(None, dbn::Schema::Mbp1, QUOTE_SCHEMAS).unwrap();

        assert_eq!(schema, dbn::Schema::Mbp1);
    }

    #[rstest]
    fn test_schema_from_params_accepts_allowed_value() {
        let mut params = Params::new();
        params.insert(SCHEMA_PARAM.to_string(), json!("tbbo"));

        let schema = schema_from_params(Some(&params), dbn::Schema::Mbp1, QUOTE_SCHEMAS).unwrap();

        assert_eq!(schema, dbn::Schema::Tbbo);
    }

    #[rstest]
    fn test_schema_from_params_rejects_disallowed_value() {
        let mut params = Params::new();
        params.insert(SCHEMA_PARAM.to_string(), json!("mbo"));

        let result = schema_from_params(Some(&params), dbn::Schema::Mbp1, QUOTE_SCHEMAS);

        assert!(result.is_err());
    }

    #[rstest]
    #[case::quotes(SubscribeKind::Quotes)]
    #[case::trades(SubscribeKind::Trades)]
    fn test_invalid_subscribe_params_do_not_create_feed_handler(#[case] kind: SubscribeKind) {
        let mut client = test_data_client();
        let mut params = Params::new();
        params.insert(SCHEMA_PARAM.to_string(), json!("definition"));

        let result = match kind {
            SubscribeKind::Quotes => client.subscribe_quotes(subscribe_quotes_cmd(Some(params))),
            SubscribeKind::Trades => client.subscribe_trades(subscribe_trades_cmd(Some(params))),
        };

        assert!(result.is_err());
        assert!(client.cmd_channels.lock().expect(MUTEX_POISONED).is_empty());
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
