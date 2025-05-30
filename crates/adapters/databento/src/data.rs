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

//! Databento data client implementation leveraging existing live and historical clients.

use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::AHashMap;
use databento::live::Subscription;
use indexmap::IndexMap;
use nautilus_common::messages::data::{
    RequestBars, RequestInstruments, RequestQuotes, RequestTrades, SubscribeBookDeltas,
    SubscribeInstrumentStatus, SubscribeQuotes, SubscribeTrades, UnsubscribeBookDeltas,
    UnsubscribeInstrumentStatus, UnsubscribeQuotes, UnsubscribeTrades,
};
use nautilus_core::time::AtomicTime;
use nautilus_data::client::DataClient;
use nautilus_model::{
    enums::BarAggregation,
    identifiers::{ClientId, Symbol, Venue},
    instruments::Instrument,
};
use tokio::sync::mpsc;

use crate::{
    historical::{DatabentoHistoricalClient, RangeQueryParams},
    live::{DatabentoFeedHandler, LiveCommand, LiveMessage},
    loader::DatabentoDataLoader,
    symbology::instrument_id_to_symbol_string,
    types::PublisherId,
};

/// Configuration for the Databento data client.
#[derive(Debug, Clone)]
pub struct DatabentoDataClientConfig {
    /// Databento API key.
    pub api_key: String,
    /// Path to publishers.json file.
    pub publishers_filepath: PathBuf,
    /// Whether to use exchange as venue for GLBX instruments.
    pub use_exchange_as_venue: bool,
    /// Whether to timestamp bars on close.
    pub bars_timestamp_on_close: bool,
}

impl DatabentoDataClientConfig {
    /// Creates a new [`DatabentoDataClientConfig`] instance.
    #[must_use]
    pub fn new(
        api_key: String,
        publishers_filepath: PathBuf,
        use_exchange_as_venue: bool,
        bars_timestamp_on_close: bool,
    ) -> Self {
        Self {
            api_key,
            publishers_filepath,
            use_exchange_as_venue,
            bars_timestamp_on_close,
        }
    }
}

/// A Databento data client that combines live streaming and historical data functionality.
///
/// This client uses the existing `DatabentoFeedHandler` for live data subscriptions
/// and `DatabentoHistoricalClient` for historical data requests. It supports multiple
/// datasets simultaneously, with separate feed handlers per dataset.
#[cfg_attr(feature = "python", pyo3::pyclass)]
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
    cmd_channels: Arc<Mutex<AHashMap<String, mpsc::UnboundedSender<LiveCommand>>>>,
    /// Publisher to venue mapping.
    publisher_venue_map: Arc<IndexMap<PublisherId, Venue>>,
    /// Symbol to venue mapping (for caching).
    symbol_venue_map: Arc<RwLock<AHashMap<Symbol, Venue>>>,
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
            config.api_key.clone(),
            config.publishers_filepath.clone(),
            clock,
            config.use_exchange_as_venue,
        )?;

        // Create data loader for venue-to-dataset mapping
        let loader = DatabentoDataLoader::new(Some(config.publishers_filepath.clone()))?;

        // Load publisher configuration
        let file_content = std::fs::read_to_string(&config.publishers_filepath)?;
        let publishers_vec: Vec<crate::types::DatabentoPublisher> =
            serde_json::from_str(&file_content)?;

        let publisher_venue_map = publishers_vec
            .into_iter()
            .map(|p| (p.publisher_id, Venue::from(p.venue.as_str())))
            .collect::<IndexMap<u16, Venue>>();

        Ok(Self {
            client_id,
            config,
            is_connected: AtomicBool::new(false),
            historical,
            loader,
            cmd_channels: Arc::new(Mutex::new(AHashMap::new())),
            publisher_venue_map: Arc::new(publisher_venue_map),
            symbol_venue_map: Arc::new(RwLock::new(AHashMap::new())),
        })
    }

    /// Gets the dataset for a given venue using the data loader.
    fn get_dataset_for_venue(&self, venue: Venue) -> anyhow::Result<String> {
        self.loader
            .get_dataset_for_venue(&venue)
            .map(|dataset| dataset.to_string())
            .ok_or_else(|| anyhow::anyhow!("No dataset found for venue: {venue}"))
    }

    /// Gets or creates a feed handler for the specified dataset.
    ///
    /// # Errors
    ///
    /// Returns an error if the feed handler cannot be created.
    fn get_or_create_feed_handler(&self, dataset: &str) -> anyhow::Result<()> {
        let mut channels = self.cmd_channels.lock().unwrap();

        if !channels.contains_key(dataset) {
            tracing::info!("Creating new feed handler for dataset: {dataset}");
            let cmd_tx = self.initialize_live_feed(dataset.to_string())?;
            channels.insert(dataset.to_string(), cmd_tx);

            self.send_command_to_dataset(dataset, LiveCommand::Start)?;
        }

        Ok(())
    }

    /// Sends a command to a specific dataset's feed handler.
    ///
    /// # Errors
    ///
    /// Returns an error if the command cannot be sent.
    fn send_command_to_dataset(&self, dataset: &str, cmd: LiveCommand) -> anyhow::Result<()> {
        let channels = self.cmd_channels.lock().unwrap();
        if let Some(tx) = channels.get(dataset) {
            tx.send(cmd)
                .map_err(|e| anyhow::anyhow!("Failed to send command to dataset {dataset}: {e}"))?;
        } else {
            anyhow::bail!("No feed handler found for dataset: {dataset}");
        }
        Ok(())
    }

    /// Initializes the live feed handler for streaming data.
    ///
    /// # Errors
    ///
    /// Returns an error if the feed handler cannot be started.
    fn initialize_live_feed(
        &self,
        dataset: String,
    ) -> anyhow::Result<mpsc::UnboundedSender<LiveCommand>> {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (msg_tx, msg_rx) = mpsc::channel(1000);

        let mut feed_handler = DatabentoFeedHandler::new(
            self.config.api_key.clone(),
            dataset,
            cmd_rx,
            msg_tx,
            (*self.publisher_venue_map).clone(),
            self.symbol_venue_map.clone(),
            self.config.use_exchange_as_venue,
            self.config.bars_timestamp_on_close,
        );

        // Spawn the feed handler task
        tokio::spawn(async move {
            if let Err(e) = feed_handler.run().await {
                tracing::error!("Feed handler error: {e}");
            }
        });

        // Spawn message processing task
        tokio::spawn(async move {
            let mut msg_rx = msg_rx;
            while let Some(msg) = msg_rx.recv().await {
                match msg {
                    LiveMessage::Data(data) => {
                        tracing::debug!("Received data: {data:?}");
                        // TODO: Forward to message bus or data engine
                    }
                    LiveMessage::Instrument(instrument) => {
                        tracing::debug!("Received instrument: {}", instrument.id());
                        // TODO: Forward to cache or instrument manager
                    }
                    LiveMessage::Status(status) => {
                        tracing::debug!("Received status: {status:?}");
                        // TODO: Forward to appropriate handler
                    }
                    LiveMessage::Imbalance(imbalance) => {
                        tracing::debug!("Received imbalance: {imbalance:?}");
                        // TODO: Forward to appropriate handler
                    }
                    LiveMessage::Statistics(statistics) => {
                        tracing::debug!("Received statistics: {statistics:?}");
                        // TODO: Forward to appropriate handler
                    }
                    LiveMessage::Error(error) => {
                        tracing::error!("Feed handler error: {error}");
                        // TODO: Handle error appropriately
                    }
                    LiveMessage::Close => {
                        tracing::info!("Feed handler closed");
                        break;
                    }
                }
            }
        });

        Ok(cmd_tx)
    }
}

#[async_trait::async_trait]
impl DataClient for DatabentoDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        None
    }

    fn start(&self) -> anyhow::Result<()> {
        tracing::debug!("Starting Databento data client");
        Ok(())
    }

    fn stop(&self) -> anyhow::Result<()> {
        tracing::debug!("Stopping Databento data client");

        // Send close command to all active feed handlers
        let channels = self.cmd_channels.lock().unwrap();
        for (dataset, tx) in channels.iter() {
            if let Err(e) = tx.send(LiveCommand::Close) {
                tracing::error!("Failed to send close command to dataset {dataset}: {e}");
            }
        }

        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&self) -> anyhow::Result<()> {
        tracing::debug!("Resetting Databento data client");
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn dispose(&self) -> anyhow::Result<()> {
        tracing::debug!("Disposing Databento data client");
        self.stop()
    }

    async fn connect(&self) -> anyhow::Result<()> {
        tracing::debug!("Connecting Databento data client");

        // Connection will happen lazily when subscriptions are made
        // No need to create feed handlers upfront since we don't know which datasets will be needed
        self.is_connected.store(true, Ordering::Relaxed);

        tracing::info!("Databento data client connected");
        Ok(())
    }

    async fn disconnect(&self) -> anyhow::Result<()> {
        tracing::debug!("Disconnecting Databento data client");

        // Send close command to all active feed handlers
        {
            let channels = self.cmd_channels.lock().unwrap();
            for (dataset, tx) in channels.iter() {
                if let Err(e) = tx.send(LiveCommand::Close) {
                    tracing::error!("Failed to send close command to dataset {dataset}: {e}");
                }
            }
        }

        self.is_connected.store(false, Ordering::Relaxed);

        // Clear all command senders
        {
            let mut channels = self.cmd_channels.lock().unwrap();
            channels.clear();
        }

        tracing::info!("Databento data client disconnected");
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    // Live subscription methods using the feed handler
    fn subscribe_quotes(&mut self, cmd: &SubscribeQuotes) -> anyhow::Result<()> {
        tracing::debug!("Subscribe quotes: {cmd:?}");

        let dataset = self.get_dataset_for_venue(cmd.instrument_id.venue)?;
        self.get_or_create_feed_handler(&dataset)?;

        let symbol = instrument_id_to_symbol_string(
            cmd.instrument_id,
            &mut self.symbol_venue_map.write().unwrap(),
        );

        let subscription = Subscription::builder()
            .schema(databento::dbn::Schema::Mbp1) // Market by price level 1 for quotes
            .symbols(symbol)
            .build();

        self.send_command_to_dataset(&dataset, LiveCommand::Subscribe(subscription))?;

        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: &SubscribeTrades) -> anyhow::Result<()> {
        tracing::debug!("Subscribe trades: {cmd:?}");

        let dataset = self.get_dataset_for_venue(cmd.instrument_id.venue)?;
        self.get_or_create_feed_handler(&dataset)?;

        let symbol = instrument_id_to_symbol_string(
            cmd.instrument_id,
            &mut self.symbol_venue_map.write().unwrap(),
        );

        let subscription = Subscription::builder()
            .schema(databento::dbn::Schema::Trades)
            .symbols(symbol)
            .build();

        self.send_command_to_dataset(&dataset, LiveCommand::Subscribe(subscription))?;

        Ok(())
    }

    fn subscribe_book_deltas(&mut self, cmd: &SubscribeBookDeltas) -> anyhow::Result<()> {
        tracing::debug!("Subscribe book deltas: {cmd:?}");

        let dataset = self.get_dataset_for_venue(cmd.instrument_id.venue)?;
        self.get_or_create_feed_handler(&dataset)?;

        let symbol = instrument_id_to_symbol_string(
            cmd.instrument_id,
            &mut self.symbol_venue_map.write().unwrap(),
        );

        let subscription = Subscription::builder()
            .schema(databento::dbn::Schema::Mbo) // Market by order for book deltas
            .symbols(symbol)
            .build();

        self.send_command_to_dataset(&dataset, LiveCommand::Subscribe(subscription))?;

        Ok(())
    }

    fn subscribe_instrument_status(
        &mut self,
        cmd: &SubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        tracing::debug!("Subscribe instrument status: {cmd:?}");

        let dataset = self.get_dataset_for_venue(cmd.instrument_id.venue)?;
        self.get_or_create_feed_handler(&dataset)?;

        let symbol = instrument_id_to_symbol_string(
            cmd.instrument_id,
            &mut self.symbol_venue_map.write().unwrap(),
        );

        let subscription = Subscription::builder()
            .schema(databento::dbn::Schema::Status)
            .symbols(symbol)
            .build();

        self.send_command_to_dataset(&dataset, LiveCommand::Subscribe(subscription))?;

        Ok(())
    }

    // Unsubscribe methods
    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        tracing::debug!("Unsubscribe quotes: {cmd:?}");

        // Note: Databento live API doesn't support granular unsubscribing.
        // The feed handler manages subscriptions and can handle reconnections
        // with the appropriate subscription state.
        tracing::warn!(
            "Databento does not support granular unsubscribing - ignoring unsubscribe request for {}",
            cmd.instrument_id
        );

        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        tracing::debug!("Unsubscribe trades: {cmd:?}");

        // Note: Databento live API doesn't support granular unsubscribing.
        // The feed handler manages subscriptions and can handle reconnections
        // with the appropriate subscription state.
        tracing::warn!(
            "Databento does not support granular unsubscribing - ignoring unsubscribe request for {}",
            cmd.instrument_id
        );

        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        tracing::debug!("Unsubscribe book deltas: {cmd:?}");

        // Note: Databento live API doesn't support granular unsubscribing.
        // The feed handler manages subscriptions and can handle reconnections
        // with the appropriate subscription state.
        tracing::warn!(
            "Databento does not support granular unsubscribing - ignoring unsubscribe request for {}",
            cmd.instrument_id
        );

        Ok(())
    }

    fn unsubscribe_instrument_status(
        &mut self,
        cmd: &UnsubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        tracing::debug!("Unsubscribe instrument status: {cmd:?}");

        // Note: Databento live API doesn't support granular unsubscribing.
        // The feed handler manages subscriptions and can handle reconnections
        // with the appropriate subscription state.
        tracing::warn!(
            "Databento does not support granular unsubscribing - ignoring unsubscribe request for {}",
            cmd.instrument_id
        );

        Ok(())
    }

    // Historical data request methods using the historical client
    fn request_instruments(&self, request: &RequestInstruments) -> anyhow::Result<()> {
        tracing::debug!("Request instruments: {request:?}");

        let historical_client = self.historical.clone();
        let request = request.clone();

        tokio::spawn(async move {
            // Convert request to historical query parameters
            // For now, use a default symbol set or derive from venue
            let symbols = vec!["ALL_SYMBOLS".to_string()]; // TODO: Improve symbol handling

            let params = RangeQueryParams {
                dataset: "GLBX.MDP3".to_string(), // TODO: Make configurable
                symbols,
                start: request
                    .start
                    .map(|dt| dt.timestamp_nanos_opt().unwrap_or(0) as u64)
                    .unwrap_or(0)
                    .into(),
                end: request
                    .end
                    .map(|dt| dt.timestamp_nanos_opt().unwrap_or(0) as u64)
                    .map(Into::into),
                limit: None,
                price_precision: None,
            };

            match historical_client.get_range_instruments(params).await {
                Ok(instruments) => {
                    tracing::info!("Retrieved {} instruments", instruments.len());
                    // TODO: Send instruments to message bus or cache
                }
                Err(e) => {
                    tracing::error!("Failed to request instruments: {e}");
                }
            }
        });

        Ok(())
    }

    fn request_quotes(&self, request: &RequestQuotes) -> anyhow::Result<()> {
        tracing::debug!("Request quotes: {request:?}");

        let historical_client = self.historical.clone();
        let request = request.clone();

        tokio::spawn(async move {
            let symbols = vec![instrument_id_to_symbol_string(
                request.instrument_id,
                &mut AHashMap::new(), // TODO: Use proper symbol map
            )];

            let params = RangeQueryParams {
                dataset: "GLBX.MDP3".to_string(), // TODO: Make configurable
                symbols,
                start: request
                    .start
                    .map(|dt| dt.timestamp_nanos_opt().unwrap_or(0) as u64)
                    .unwrap_or(0)
                    .into(),
                end: request
                    .end
                    .map(|dt| dt.timestamp_nanos_opt().unwrap_or(0) as u64)
                    .map(Into::into),
                limit: request.limit.map(|l| l.get() as u64),
                price_precision: None,
            };

            match historical_client.get_range_quotes(params, None).await {
                Ok(quotes) => {
                    tracing::info!("Retrieved {} quotes", quotes.len());
                    // TODO: Send quotes to message bus
                }
                Err(e) => {
                    tracing::error!("Failed to request quotes: {e}");
                }
            }
        });

        Ok(())
    }

    fn request_trades(&self, request: &RequestTrades) -> anyhow::Result<()> {
        tracing::debug!("Request trades: {request:?}");

        let historical_client = self.historical.clone();
        let request = request.clone();

        tokio::spawn(async move {
            let symbols = vec![instrument_id_to_symbol_string(
                request.instrument_id,
                &mut AHashMap::new(), // TODO: Use proper symbol map
            )];

            let params = RangeQueryParams {
                dataset: "GLBX.MDP3".to_string(), // TODO: Make configurable
                symbols,
                start: request
                    .start
                    .map(|dt| dt.timestamp_nanos_opt().unwrap_or(0) as u64)
                    .unwrap_or(0)
                    .into(),
                end: request
                    .end
                    .map(|dt| dt.timestamp_nanos_opt().unwrap_or(0) as u64)
                    .map(Into::into),
                limit: request.limit.map(|l| l.get() as u64),
                price_precision: None,
            };

            match historical_client.get_range_trades(params).await {
                Ok(trades) => {
                    tracing::info!("Retrieved {} trades", trades.len());
                    // TODO: Send trades to message bus
                }
                Err(e) => {
                    tracing::error!("Failed to request trades: {e}");
                }
            }
        });

        Ok(())
    }

    fn request_bars(&self, request: &RequestBars) -> anyhow::Result<()> {
        tracing::debug!("Request bars: {request:?}");

        let historical_client = self.historical.clone();
        let request = request.clone();

        tokio::spawn(async move {
            let symbols = vec![instrument_id_to_symbol_string(
                request.bar_type.instrument_id(),
                &mut AHashMap::new(), // TODO: Use proper symbol map
            )];

            let params = RangeQueryParams {
                dataset: "GLBX.MDP3".to_string(), // TODO: Make configurable
                symbols,
                start: request
                    .start
                    .map(|dt| dt.timestamp_nanos_opt().unwrap_or(0) as u64)
                    .unwrap_or(0)
                    .into(),
                end: request
                    .end
                    .map(|dt| dt.timestamp_nanos_opt().unwrap_or(0) as u64)
                    .map(Into::into),
                limit: request.limit.map(|l| l.get() as u64),
                price_precision: None,
            };

            // Map bar aggregation from the request
            let aggregation = match request.bar_type.spec().aggregation {
                BarAggregation::Second => BarAggregation::Second,
                BarAggregation::Minute => BarAggregation::Minute,
                BarAggregation::Hour => BarAggregation::Hour,
                BarAggregation::Day => BarAggregation::Day,
                _ => {
                    tracing::error!(
                        "Unsupported bar aggregation: {:?}",
                        request.bar_type.spec().aggregation
                    );
                    return;
                }
            };

            match historical_client
                .get_range_bars(params, aggregation, true)
                .await
            {
                Ok(bars) => {
                    tracing::info!("Retrieved {} bars", bars.len());
                    // TODO: Send bars to message bus
                }
                Err(e) => {
                    tracing::error!("Failed to request bars: {e}");
                }
            }
        });

        Ok(())
    }
}
