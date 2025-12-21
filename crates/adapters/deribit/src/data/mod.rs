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

//! Live market data client implementation for the Deribit adapter.

use std::sync::{
    Arc, RwLock,
    atomic::{AtomicBool, Ordering},
};

use ahash::AHashMap;
use anyhow::Context;
use async_trait::async_trait;
use nautilus_common::{
    live::{runner::get_data_event_sender, runtime::get_runtime},
    messages::{
        DataEvent, DataResponse,
        data::{
            InstrumentResponse, InstrumentsResponse, RequestBars, RequestInstrument,
            RequestInstruments, RequestTrades, SubscribeBars, SubscribeBookDeltas,
            SubscribeBookDepth10, SubscribeBookSnapshots, SubscribeFundingRates,
            SubscribeIndexPrices, SubscribeInstrument, SubscribeInstruments, SubscribeMarkPrices,
            SubscribeQuotes, SubscribeTrades, TradesResponse, UnsubscribeBars,
            UnsubscribeBookDeltas, UnsubscribeBookDepth10, UnsubscribeBookSnapshots,
            UnsubscribeFundingRates, UnsubscribeIndexPrices, UnsubscribeInstrument,
            UnsubscribeInstruments, UnsubscribeMarkPrices, UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
};
use nautilus_core::{
    datetime::datetime_to_unix_nanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_data::client::DataClient;
use nautilus_model::{
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::{
    common::consts::DERIBIT_VENUE,
    config::DeribitDataClientConfig,
    http::{client::DeribitHttpClient, models::DeribitCurrency},
    websocket::client::DeribitWebSocketClient,
};

/// Deribit live data client.
#[derive(Debug)]
pub struct DeribitDataClient {
    client_id: ClientId,
    config: DeribitDataClientConfig,
    http_client: DeribitHttpClient,
    _ws_client: Option<DeribitWebSocketClient>,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instruments: Arc<RwLock<AHashMap<InstrumentId, InstrumentAny>>>,
    clock: &'static AtomicTime,
}

impl DeribitDataClient {
    /// Creates a new [`DeribitDataClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to initialize.
    pub fn new(client_id: ClientId, config: DeribitDataClientConfig) -> anyhow::Result<Self> {
        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();

        let http_client = if config.has_api_credentials() {
            DeribitHttpClient::new_with_env(
                config.api_key.clone(),
                config.api_secret.clone(),
                config.use_testnet,
                config.http_timeout_secs,
                config.max_retries,
                config.retry_delay_initial_ms,
                config.retry_delay_max_ms,
                None, // proxy_url
            )?
        } else {
            DeribitHttpClient::new(
                config.base_url_http.clone(),
                config.use_testnet,
                config.http_timeout_secs,
                config.max_retries,
                config.retry_delay_initial_ms,
                config.retry_delay_max_ms,
                None, // proxy_url
            )?
        };

        let ws_client = DeribitWebSocketClient::new(
            Some(config.ws_url()),
            config.api_key.clone(),
            config.api_secret.clone(),
            config.heartbeat_interval_secs,
            config.use_testnet,
        )?;

        Ok(Self {
            client_id,
            config,
            http_client,
            _ws_client: Some(ws_client),
            is_connected: AtomicBool::new(false),
            cancellation_token: CancellationToken::new(),
            tasks: Vec::new(),
            data_sender,
            instruments: Arc::new(RwLock::new(AHashMap::new())),
            clock,
        })
    }
}

#[async_trait(?Send)]
impl DataClient for DeribitDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(*DERIBIT_VENUE)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        tracing::info!(
            client_id = %self.client_id,
            use_testnet = %self.config.use_testnet,
            "Starting Deribit data client"
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        tracing::info!("Stopping Deribit data client: {}", self.client_id);
        self.cancellation_token.cancel();
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        tracing::info!("Resetting Deribit data client: {}", self.client_id);
        self.is_connected.store(false, Ordering::Relaxed);
        self.cancellation_token = CancellationToken::new();
        self.tasks.clear();
        if let Ok(mut instruments) = self.instruments.write() {
            instruments.clear();
        }
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        tracing::info!("Disposing Deribit data client: {}", self.client_id);
        self.stop()
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::SeqCst)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected() {
            return Ok(());
        }

        // TODO add instruments fetching

        self.is_connected.store(true, Ordering::Release);
        tracing::info!(client_id = %self.client_id, "Connected");
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        todo!("Implement disconnect");
    }

    fn subscribe_instruments(&mut self, _cmd: &SubscribeInstruments) -> anyhow::Result<()> {
        todo!("Implement subscribe_instruments");
    }

    fn subscribe_instrument(&mut self, _cmd: &SubscribeInstrument) -> anyhow::Result<()> {
        todo!("Implement subscribe_instrument");
    }

    fn subscribe_book_deltas(&mut self, _cmd: &SubscribeBookDeltas) -> anyhow::Result<()> {
        todo!("Implement subscribe_book_deltas");
    }

    fn subscribe_book_depth10(&mut self, _cmd: &SubscribeBookDepth10) -> anyhow::Result<()> {
        todo!("Implement subscribe_book_depth10")
    }

    fn subscribe_book_snapshots(&mut self, _cmd: &SubscribeBookSnapshots) -> anyhow::Result<()> {
        todo!("Implement subscribe_book_snapshots");
    }

    fn subscribe_quotes(&mut self, _cmd: &SubscribeQuotes) -> anyhow::Result<()> {
        todo!("Implement subscribe_quotes")
    }

    fn subscribe_trades(&mut self, _cmd: &SubscribeTrades) -> anyhow::Result<()> {
        todo!("Implement subscribe_trades")
    }

    fn subscribe_mark_prices(&mut self, _cmd: &SubscribeMarkPrices) -> anyhow::Result<()> {
        todo!("Implement subscribe_mark_prices")
    }

    fn subscribe_index_prices(&mut self, _cmd: &SubscribeIndexPrices) -> anyhow::Result<()> {
        todo!("Implement subscribe_index_prices")
    }

    fn subscribe_funding_rates(&mut self, _cmd: &SubscribeFundingRates) -> anyhow::Result<()> {
        todo!("Implement subscribe_funding_rates")
    }

    fn subscribe_bars(&mut self, _cmd: &SubscribeBars) -> anyhow::Result<()> {
        todo!("Implement subscribe_bars");
    }

    fn unsubscribe_instruments(&mut self, _cmd: &UnsubscribeInstruments) -> anyhow::Result<()> {
        todo!("Implement unsubscribe_instruments");
    }

    fn unsubscribe_instrument(&mut self, _cmd: &UnsubscribeInstrument) -> anyhow::Result<()> {
        todo!("Implement unsubscribe_instrument");
    }

    fn unsubscribe_book_deltas(&mut self, _cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        todo!("Implement unsubscribe_book_deltas");
    }

    fn unsubscribe_book_depth10(&mut self, _cmd: &UnsubscribeBookDepth10) -> anyhow::Result<()> {
        todo!("Implement unsubscribe_book_depth10");
    }

    fn unsubscribe_book_snapshots(
        &mut self,
        _cmd: &UnsubscribeBookSnapshots,
    ) -> anyhow::Result<()> {
        todo!("Implement unsubscribe_book_snapshots");
    }

    fn unsubscribe_quotes(&mut self, _cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        todo!("Implement unsubscribe_quotes");
    }

    fn unsubscribe_trades(&mut self, _cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        todo!("Implement unsubscribe_trades");
    }

    fn unsubscribe_mark_prices(&mut self, _cmd: &UnsubscribeMarkPrices) -> anyhow::Result<()> {
        todo!("Implement unsubscribe_mark_prices");
    }

    fn unsubscribe_index_prices(&mut self, _cmd: &UnsubscribeIndexPrices) -> anyhow::Result<()> {
        todo!("Implement unsubscribe_index_prices");
    }

    fn unsubscribe_funding_rates(&mut self, _cmd: &UnsubscribeFundingRates) -> anyhow::Result<()> {
        todo!("Implement unsubscribe_funding_rates")
    }

    fn unsubscribe_bars(&mut self, _cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        todo!("Implement unsubscribe_bars");
    }

    fn request_instruments(&self, request: &RequestInstruments) -> anyhow::Result<()> {
        if request.start.is_some() {
            tracing::warn!(
                "Requesting instruments for {:?} with specified `start` which has no effect",
                request.venue
            );
        }
        if request.end.is_some() {
            tracing::warn!(
                "Requesting instruments for {:?} with specified `end` which has no effect",
                request.venue
            );
        }

        let http_client = self.http_client.clone();
        let instruments_cache = Arc::clone(&self.instruments);
        let sender = self.data_sender.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params.clone();
        let clock = self.clock;
        let venue = *DERIBIT_VENUE;

        // Get instrument kinds from config, default to Future if empty
        let instrument_kinds = if self.config.instrument_kinds.is_empty() {
            vec![crate::http::models::DeribitInstrumentKind::Future]
        } else {
            self.config.instrument_kinds.clone()
        };

        get_runtime().spawn(async move {
            let mut all_instruments = Vec::new();
            for kind in &instrument_kinds {
                tracing::debug!("Requesting instruments for currency=ANY, kind={:?}", kind);

                match http_client
                    .request_instruments(DeribitCurrency::ANY, Some(*kind))
                    .await
                {
                    Ok(instruments) => {
                        tracing::info!(
                            "Fetched {} instruments for ANY/{:?}",
                            instruments.len(),
                            kind
                        );

                        for instrument in instruments {
                            // Cache the instrument
                            {
                                let mut guard = instruments_cache
                                    .write()
                                    .expect("instrument cache lock poisoned");
                                guard.insert(instrument.id(), instrument.clone());
                            }

                            all_instruments.push(instrument);
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to fetch instruments for ANY/{:?}: {:?}", kind, e);
                    }
                }
            }

            // Send response with all collected instruments
            let response = DataResponse::Instruments(InstrumentsResponse::new(
                request_id,
                client_id,
                venue,
                all_instruments,
                start_nanos,
                end_nanos,
                clock.get_time_ns(),
                params,
            ));

            if let Err(e) = sender.send(DataEvent::Response(response)) {
                tracing::error!("Failed to send instruments response: {}", e);
            }
        });

        Ok(())
    }

    fn request_instrument(&self, request: &RequestInstrument) -> anyhow::Result<()> {
        if request.start.is_some() {
            tracing::warn!(
                "Requesting instrument {} with specified `start` which has no effect",
                request.instrument_id
            );
        }
        if request.end.is_some() {
            tracing::warn!(
                "Requesting instrument {} with specified `end` which has no effect",
                request.instrument_id
            );
        }

        // First, check if instrument exists in cache
        if let Some(instrument) = self
            .instruments
            .read()
            .expect("instrument cache lock poisoned")
            .get(&request.instrument_id)
            .cloned()
        {
            let response = DataResponse::Instrument(Box::new(InstrumentResponse::new(
                request.request_id,
                request.client_id.unwrap_or(self.client_id),
                instrument.id(),
                instrument,
                datetime_to_unix_nanos(request.start),
                datetime_to_unix_nanos(request.end),
                self.clock.get_time_ns(),
                request.params.clone(),
            )));

            if let Err(e) = self.data_sender.send(DataEvent::Response(response)) {
                tracing::error!("Failed to send instrument response: {}", e);
            }
            return Ok(());
        }

        tracing::debug!(
            "Instrument {} not in cache, fetching from API",
            request.instrument_id
        );

        let http_client = self.http_client.clone();
        let instruments_cache = Arc::clone(&self.instruments);
        let sender = self.data_sender.clone();
        let instrument_id = request.instrument_id;
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params.clone();
        let clock = self.clock;

        get_runtime().spawn(async move {
            match http_client
                .request_instrument(instrument_id)
                .await
                .context("failed to request instrument from Deribit")
            {
                Ok(instrument) => {
                    tracing::info!("Successfully fetched instrument: {}", instrument_id);

                    // Cache the instrument
                    {
                        let mut guard = instruments_cache
                            .write()
                            .expect("instrument cache lock poisoned");
                        guard.insert(instrument.id(), instrument.clone());
                    }

                    // Send response
                    let response = DataResponse::Instrument(Box::new(InstrumentResponse::new(
                        request_id,
                        client_id,
                        instrument.id(),
                        instrument,
                        start_nanos,
                        end_nanos,
                        clock.get_time_ns(),
                        params,
                    )));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        tracing::error!("Failed to send instrument response: {}", e);
                    }
                }
                Err(e) => {
                    tracing::error!("Instrument request failed for {}: {:?}", instrument_id, e);
                }
            }
        });

        Ok(())
    }

    fn request_trades(&self, request: &RequestTrades) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let sender = self.data_sender.clone();
        let instrument_id = request.instrument_id;
        let start = request.start;
        let end = request.end;
        let limit = request.limit.map(|n| n.get() as u32);
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let params = request.params.clone();
        let clock = self.clock;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);

        get_runtime().spawn(async move {
            match http_client
                .request_trades(instrument_id, start, end, limit)
                .await
                .context("failed to request trades from Deribit")
            {
                Ok(trades) => {
                    let response = DataResponse::Trades(TradesResponse::new(
                        request_id,
                        client_id,
                        instrument_id,
                        trades,
                        start_nanos,
                        end_nanos,
                        clock.get_time_ns(),
                        params,
                    ));
                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        tracing::error!("Failed to send trades response: {e}");
                    }
                }
                Err(e) => tracing::error!("Trades request failed for {}: {:?}", instrument_id, e),
            }
        });

        Ok(())
    }

    fn request_bars(&self, _request: &RequestBars) -> anyhow::Result<()> {
        todo!("Implement request_bars");
    }
}
