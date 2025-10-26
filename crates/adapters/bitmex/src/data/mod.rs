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

//! Live market data client implementation for the BitMEX adapter.

use std::{
    future::Future,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::AHashMap;
use anyhow::Context;
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
use nautilus_common::{
    messages::{
        DataEvent,
        data::{
            BarsResponse, DataResponse, InstrumentResponse, InstrumentsResponse, RequestBars,
            RequestInstrument, RequestInstruments, RequestTrades, SubscribeBars,
            SubscribeBookDeltas, SubscribeBookDepth10, SubscribeBookSnapshots,
            SubscribeFundingRates, SubscribeIndexPrices, SubscribeMarkPrices, SubscribeQuotes,
            SubscribeTrades, TradesResponse, UnsubscribeBars, UnsubscribeBookDeltas,
            UnsubscribeBookDepth10, UnsubscribeBookSnapshots, UnsubscribeFundingRates,
            UnsubscribeIndexPrices, UnsubscribeMarkPrices, UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
    runner::get_data_event_sender,
};
use nautilus_core::{
    UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_data::client::DataClient;
use nautilus_model::{
    data::Data,
    enums::BookType,
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
};
use tokio::{task::JoinHandle, time::Duration};
use tokio_util::sync::CancellationToken;

use crate::{
    common::consts::BITMEX_VENUE,
    config::BitmexDataClientConfig,
    http::client::BitmexHttpClient,
    websocket::{client::BitmexWebSocketClient, messages::NautilusWsMessage},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BitmexBookChannel {
    OrderBookL2,
    OrderBookL2_25,
    OrderBook10,
}

#[derive(Debug)]
pub struct BitmexDataClient {
    client_id: ClientId,
    config: BitmexDataClientConfig,
    http_client: BitmexHttpClient,
    ws_client: Option<BitmexWebSocketClient>,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instruments: Arc<RwLock<AHashMap<InstrumentId, InstrumentAny>>>,
    book_channels: Arc<RwLock<AHashMap<InstrumentId, BitmexBookChannel>>>,
    clock: &'static AtomicTime,
    instrument_refresh_active: bool,
}

impl BitmexDataClient {
    /// Creates a new [`BitmexDataClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client cannot be constructed.
    pub fn new(client_id: ClientId, config: BitmexDataClientConfig) -> anyhow::Result<Self> {
        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();

        let http_client = BitmexHttpClient::new(
            Some(config.http_base_url()),
            config.api_key.clone(),
            config.api_secret.clone(),
            config.use_testnet,
            config.http_timeout_secs,
            config.max_retries,
            config.retry_delay_initial_ms,
            config.retry_delay_max_ms,
            config.recv_window_ms,
            config.max_requests_per_second,
            config.max_requests_per_minute,
        )
        .context("failed to construct BitMEX HTTP client")?;

        Ok(Self {
            client_id,
            config,
            http_client,
            ws_client: None,
            is_connected: AtomicBool::new(false),
            cancellation_token: CancellationToken::new(),
            tasks: Vec::new(),
            data_sender,
            instruments: Arc::new(RwLock::new(AHashMap::new())),
            book_channels: Arc::new(RwLock::new(AHashMap::new())),
            clock,
            instrument_refresh_active: false,
        })
    }

    fn venue(&self) -> Venue {
        *BITMEX_VENUE
    }

    fn ws_client(&self) -> anyhow::Result<&BitmexWebSocketClient> {
        self.ws_client
            .as_ref()
            .context("websocket client not initialized; call connect first")
    }

    fn ws_client_mut(&mut self) -> anyhow::Result<&mut BitmexWebSocketClient> {
        self.ws_client
            .as_mut()
            .context("websocket client not initialized; call connect first")
    }

    fn send_data(sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>, data: Data) {
        if let Err(e) = sender.send(DataEvent::Data(data)) {
            tracing::error!("Failed to emit data event: {e}");
        }
    }

    fn spawn_ws<F>(&self, fut: F, context: &'static str)
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        tokio::spawn(async move {
            if let Err(e) = fut.await {
                tracing::error!("{context}: {e:?}");
            }
        });
    }

    fn spawn_stream_task(
        &mut self,
        stream: impl futures_util::Stream<Item = NautilusWsMessage> + Send + 'static,
    ) -> anyhow::Result<()> {
        let data_sender = self.data_sender.clone();
        let instruments = Arc::clone(&self.instruments);
        let cancellation = self.cancellation_token.clone();

        let handle = tokio::spawn(async move {
            tokio::pin!(stream);

            loop {
                tokio::select! {
                    maybe_msg = stream.next() => {
                        match maybe_msg {
                            Some(msg) => Self::handle_ws_message(msg, &data_sender, &instruments),
                            None => {
                                tracing::debug!("BitMEX websocket stream ended");
                                break;
                            }
                        }
                    }
                    _ = cancellation.cancelled() => {
                        tracing::debug!("BitMEX websocket stream task cancelled");
                        break;
                    }
                }
            }
        });

        self.tasks.push(handle);
        Ok(())
    }

    fn handle_ws_message(
        message: NautilusWsMessage,
        sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
        instruments: &Arc<RwLock<AHashMap<InstrumentId, InstrumentAny>>>,
    ) {
        match message {
            NautilusWsMessage::Data(payloads) => {
                for data in payloads {
                    Self::send_data(sender, data);
                }
            }
            NautilusWsMessage::FundingRateUpdates(updates) => {
                for update in updates {
                    tracing::debug!(
                        instrument = %update.instrument_id,
                        rate = %update.rate,
                        "Funding rate update received (not forwarded)",
                    );
                }
            }
            NautilusWsMessage::OrderStatusReports(_)
            | NautilusWsMessage::OrderUpdated(_)
            | NautilusWsMessage::FillReports(_)
            | NautilusWsMessage::PositionStatusReport(_)
            | NautilusWsMessage::AccountState(_) => {
                tracing::debug!("Ignoring trading message on data client");
            }
            NautilusWsMessage::Reconnected => {
                tracing::info!("BitMEX websocket reconnected");
            }
        }

        // Instrument updates arrive via the REST bootstrap. Keep the argument alive so Clippy
        // doesn't flag the unused parameter warning when we expand handling later.
        let _ = instruments;
    }

    async fn bootstrap_instruments(&mut self) -> anyhow::Result<Vec<InstrumentAny>> {
        let http = self.http_client.clone();
        let mut instruments = http
            .request_instruments(self.config.active_only)
            .await
            .context("failed to request BitMEX instruments")?;

        instruments.sort_by_key(|instrument| instrument.id());

        {
            let mut guard = self
                .instruments
                .write()
                .expect("instrument cache lock poisoned");
            guard.clear();
            for instrument in &instruments {
                guard.insert(instrument.id(), instrument.clone());
            }
        }

        for instrument in &instruments {
            self.http_client.add_instrument(instrument.clone());
        }

        Ok(instruments)
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    fn maybe_spawn_instrument_refresh(&mut self) -> anyhow::Result<()> {
        let Some(minutes) = self.config.update_instruments_interval_mins else {
            return Ok(());
        };

        if minutes == 0 || self.instrument_refresh_active {
            return Ok(());
        }

        let interval_secs = minutes.saturating_mul(60);
        if interval_secs == 0 {
            return Ok(());
        }

        let interval = Duration::from_secs(interval_secs);
        let cancellation = self.cancellation_token.clone();
        let instruments_cache = Arc::clone(&self.instruments);
        let active_only = self.config.active_only;
        let client_id = self.client_id;
        let http_client = self.http_client.clone();

        let handle = tokio::spawn(async move {
            let http_client = http_client;
            loop {
                let sleep = tokio::time::sleep(interval);
                tokio::pin!(sleep);
                tokio::select! {
                    _ = cancellation.cancelled() => {
                        tracing::debug!("BitMEX instrument refresh task cancelled");
                        break;
                    }
                    _ = &mut sleep => {
                        match http_client.request_instruments(active_only).await {
                            Ok(mut instruments) => {
                                instruments.sort_by_key(|instrument| instrument.id());

                                {
                                    let mut guard = instruments_cache
                                        .write()
                                        .expect("instrument cache lock poisoned");
                                    guard.clear();
                                    for instrument in instruments.iter() {
                                        guard.insert(instrument.id(), instrument.clone());
                                    }
                                }

                                for instrument in instruments {
                                    http_client.add_instrument(instrument);
                                }

                                tracing::debug!(client_id=%client_id, "BitMEX instruments refreshed");
                            }
                            Err(e) => {
                                tracing::warn!(client_id=%client_id, error=?e, "Failed to refresh BitMEX instruments");
                            }
                        }
                    }
                }
            }
        });

        self.tasks.push(handle);
        self.instrument_refresh_active = true;
        Ok(())
    }
}

fn datetime_to_unix_nanos(value: Option<DateTime<Utc>>) -> Option<UnixNanos> {
    value
        .and_then(|dt| dt.timestamp_nanos_opt())
        .and_then(|nanos| u64::try_from(nanos).ok())
        .map(UnixNanos::from)
}

#[async_trait::async_trait]
impl DataClient for BitmexDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(self.venue())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        tracing::info!("Starting BitMEX data client {id}", id = self.client_id);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        tracing::info!("Stopping BitMEX data client {id}", id = self.client_id);
        self.cancellation_token.cancel();
        self.is_connected.store(false, Ordering::Relaxed);
        self.instrument_refresh_active = false;
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        tracing::debug!("Resetting BitMEX data client {id}", id = self.client_id);
        self.is_connected.store(false, Ordering::Relaxed);
        self.cancellation_token = CancellationToken::new();
        self.tasks.clear();
        self.book_channels
            .write()
            .expect("book channel cache lock poisoned")
            .clear();
        self.instrument_refresh_active = false;
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        self.stop()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected() {
            return Ok(());
        }

        if self.ws_client.is_none() {
            let ws = BitmexWebSocketClient::new(
                Some(self.config.ws_url()),
                self.config.api_key.clone(),
                self.config.api_secret.clone(),
                None,
                self.config.heartbeat_interval_secs,
            )
            .context("failed to construct BitMEX websocket client")?;
            self.ws_client = Some(ws);
        }

        let instruments = self.bootstrap_instruments().await?;
        if let Some(ws) = self.ws_client.as_mut() {
            ws.initialize_instruments_cache(instruments);
        }

        let ws = self.ws_client_mut()?;
        ws.connect()
            .await
            .context("failed to connect BitMEX websocket")?;
        ws.wait_until_active(10.0)
            .await
            .context("BitMEX websocket did not become active")?;

        let stream = ws.stream();
        self.spawn_stream_task(stream)?;
        self.maybe_spawn_instrument_refresh()?;

        self.is_connected.store(true, Ordering::Relaxed);
        tracing::info!("BitMEX data client connected");
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.is_disconnected() {
            return Ok(());
        }

        self.cancellation_token.cancel();

        if let Some(ws) = self.ws_client.as_mut()
            && let Err(e) = ws.close().await
        {
            tracing::warn!("Error while closing BitMEX websocket: {e:?}");
        }

        for handle in self.tasks.drain(..) {
            if let Err(e) = handle.await {
                tracing::error!("Error joining websocket task: {e:?}");
            }
        }

        self.cancellation_token = CancellationToken::new();
        self.is_connected.store(false, Ordering::Relaxed);
        self.book_channels
            .write()
            .expect("book channel cache lock poisoned")
            .clear();
        self.instrument_refresh_active = false;

        tracing::info!("BitMEX data client disconnected");
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.is_connected()
    }

    fn is_disconnected(&self) -> bool {
        self.is_disconnected()
    }

    fn subscribe_book_deltas(&mut self, cmd: &SubscribeBookDeltas) -> anyhow::Result<()> {
        if cmd.book_type != BookType::L2_MBP {
            anyhow::bail!("BitMEX only supports L2_MBP order book deltas");
        }

        let instrument_id = cmd.instrument_id;
        let depth = cmd.depth.map_or(0, |d| d.get());
        let channel = if depth > 0 && depth <= 25 {
            BitmexBookChannel::OrderBookL2_25
        } else {
            BitmexBookChannel::OrderBookL2
        };

        let ws = self.ws_client()?.clone();
        let book_channels = Arc::clone(&self.book_channels);
        self.spawn_ws(
            async move {
                match channel {
                    BitmexBookChannel::OrderBookL2 => ws
                        .subscribe_book(instrument_id)
                        .await
                        .map_err(|err| anyhow::anyhow!(err))?,
                    BitmexBookChannel::OrderBookL2_25 => ws
                        .subscribe_book_25(instrument_id)
                        .await
                        .map_err(|err| anyhow::anyhow!(err))?,
                    BitmexBookChannel::OrderBook10 => unreachable!(),
                }
                book_channels
                    .write()
                    .expect("book channel cache lock poisoned")
                    .insert(instrument_id, channel);
                Ok(())
            },
            "BitMEX book delta subscription",
        );

        Ok(())
    }

    fn subscribe_book_depth10(&mut self, cmd: &SubscribeBookDepth10) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws_client()?.clone();
        let book_channels = Arc::clone(&self.book_channels);
        self.spawn_ws(
            async move {
                ws.subscribe_book_depth10(instrument_id)
                    .await
                    .map_err(|err| anyhow::anyhow!(err))?;
                book_channels
                    .write()
                    .expect("book channel cache lock poisoned")
                    .insert(instrument_id, BitmexBookChannel::OrderBook10);
                Ok(())
            },
            "BitMEX book depth10 subscription",
        );
        Ok(())
    }

    fn subscribe_book_snapshots(&mut self, cmd: &SubscribeBookSnapshots) -> anyhow::Result<()> {
        if cmd.book_type != BookType::L2_MBP {
            anyhow::bail!("BitMEX only supports L2_MBP order book snapshots");
        }

        let depth = cmd.depth.map_or(10, |d| d.get());
        if depth != 10 {
            tracing::warn!("BitMEX orderBook10 provides 10 levels; requested depth={depth}");
        }

        let instrument_id = cmd.instrument_id;
        let ws = self.ws_client()?.clone();
        let book_channels = Arc::clone(&self.book_channels);
        self.spawn_ws(
            async move {
                ws.subscribe_book_depth10(instrument_id)
                    .await
                    .map_err(|err| anyhow::anyhow!(err))?;
                book_channels
                    .write()
                    .expect("book channel cache lock poisoned")
                    .insert(instrument_id, BitmexBookChannel::OrderBook10);
                Ok(())
            },
            "BitMEX book snapshot subscription",
        );
        Ok(())
    }

    fn subscribe_quotes(&mut self, cmd: &SubscribeQuotes) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws_client()?.clone();
        self.spawn_ws(
            async move {
                ws.subscribe_quotes(instrument_id)
                    .await
                    .map_err(|err| anyhow::anyhow!(err))
            },
            "BitMEX quote subscription",
        );
        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: &SubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws_client()?.clone();
        self.spawn_ws(
            async move {
                ws.subscribe_trades(instrument_id)
                    .await
                    .map_err(|err| anyhow::anyhow!(err))
            },
            "BitMEX trade subscription",
        );
        Ok(())
    }

    fn subscribe_mark_prices(&mut self, cmd: &SubscribeMarkPrices) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws_client()?.clone();
        self.spawn_ws(
            async move {
                ws.subscribe_mark_prices(instrument_id)
                    .await
                    .map_err(|err| anyhow::anyhow!(err))
            },
            "BitMEX mark price subscription",
        );
        Ok(())
    }

    fn subscribe_index_prices(&mut self, cmd: &SubscribeIndexPrices) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws_client()?.clone();
        self.spawn_ws(
            async move {
                ws.subscribe_index_prices(instrument_id)
                    .await
                    .map_err(|err| anyhow::anyhow!(err))
            },
            "BitMEX index price subscription",
        );
        Ok(())
    }

    fn subscribe_funding_rates(&mut self, cmd: &SubscribeFundingRates) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws_client()?.clone();
        self.spawn_ws(
            async move {
                ws.subscribe_funding_rates(instrument_id)
                    .await
                    .map_err(|err| anyhow::anyhow!(err))
            },
            "BitMEX funding rate subscription",
        );
        Ok(())
    }

    fn subscribe_bars(&mut self, cmd: &SubscribeBars) -> anyhow::Result<()> {
        let bar_type = cmd.bar_type;
        let ws = self.ws_client()?.clone();
        self.spawn_ws(
            async move {
                ws.subscribe_bars(bar_type)
                    .await
                    .map_err(|err| anyhow::anyhow!(err))
            },
            "BitMEX bar subscription",
        );
        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws_client()?.clone();
        let book_channels = Arc::clone(&self.book_channels);
        self.spawn_ws(
            async move {
                let channel = book_channels
                    .write()
                    .expect("book channel cache lock poisoned")
                    .remove(&instrument_id);

                match channel {
                    Some(BitmexBookChannel::OrderBookL2) => ws
                        .unsubscribe_book(instrument_id)
                        .await
                        .map_err(|err| anyhow::anyhow!(err))?,
                    Some(BitmexBookChannel::OrderBookL2_25) => ws
                        .unsubscribe_book_25(instrument_id)
                        .await
                        .map_err(|err| anyhow::anyhow!(err))?,
                    Some(BitmexBookChannel::OrderBook10) | None => ws
                        .unsubscribe_book(instrument_id)
                        .await
                        .map_err(|err| anyhow::anyhow!(err))?,
                }
                Ok(())
            },
            "BitMEX book delta unsubscribe",
        );
        Ok(())
    }

    fn unsubscribe_book_depth10(&mut self, cmd: &UnsubscribeBookDepth10) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws_client()?.clone();
        let book_channels = Arc::clone(&self.book_channels);
        self.spawn_ws(
            async move {
                book_channels
                    .write()
                    .expect("book channel cache lock poisoned")
                    .remove(&instrument_id);
                ws.unsubscribe_book_depth10(instrument_id)
                    .await
                    .map_err(|err| anyhow::anyhow!(err))
            },
            "BitMEX book depth10 unsubscribe",
        );
        Ok(())
    }

    fn unsubscribe_book_snapshots(&mut self, cmd: &UnsubscribeBookSnapshots) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws_client()?.clone();
        let book_channels = Arc::clone(&self.book_channels);
        self.spawn_ws(
            async move {
                book_channels
                    .write()
                    .expect("book channel cache lock poisoned")
                    .remove(&instrument_id);
                ws.unsubscribe_book_depth10(instrument_id)
                    .await
                    .map_err(|err| anyhow::anyhow!(err))
            },
            "BitMEX book snapshot unsubscribe",
        );
        Ok(())
    }

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws_client()?.clone();
        self.spawn_ws(
            async move {
                ws.unsubscribe_quotes(instrument_id)
                    .await
                    .map_err(|err| anyhow::anyhow!(err))
            },
            "BitMEX quote unsubscribe",
        );
        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws_client()?.clone();
        self.spawn_ws(
            async move {
                ws.unsubscribe_trades(instrument_id)
                    .await
                    .map_err(|err| anyhow::anyhow!(err))
            },
            "BitMEX trade unsubscribe",
        );
        Ok(())
    }

    fn unsubscribe_mark_prices(&mut self, cmd: &UnsubscribeMarkPrices) -> anyhow::Result<()> {
        let ws = self.ws_client()?.clone();
        let instrument_id = cmd.instrument_id;
        self.spawn_ws(
            async move {
                ws.unsubscribe_mark_prices(instrument_id)
                    .await
                    .map_err(|err| anyhow::anyhow!(err))
            },
            "BitMEX mark price unsubscribe",
        );
        Ok(())
    }

    fn unsubscribe_index_prices(&mut self, cmd: &UnsubscribeIndexPrices) -> anyhow::Result<()> {
        let ws = self.ws_client()?.clone();
        let instrument_id = cmd.instrument_id;
        self.spawn_ws(
            async move {
                ws.unsubscribe_index_prices(instrument_id)
                    .await
                    .map_err(|err| anyhow::anyhow!(err))
            },
            "BitMEX index price unsubscribe",
        );
        Ok(())
    }

    fn unsubscribe_funding_rates(&mut self, cmd: &UnsubscribeFundingRates) -> anyhow::Result<()> {
        let ws = self.ws_client()?.clone();
        let instrument_id = cmd.instrument_id;
        self.spawn_ws(
            async move {
                ws.unsubscribe_funding_rates(instrument_id)
                    .await
                    .map_err(|err| anyhow::anyhow!(err))
            },
            "BitMEX funding rate unsubscribe",
        );
        Ok(())
    }

    fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        let bar_type = cmd.bar_type;
        let ws = self.ws_client()?.clone();
        self.spawn_ws(
            async move {
                ws.unsubscribe_bars(bar_type)
                    .await
                    .map_err(|err| anyhow::anyhow!(err))
            },
            "BitMEX bar unsubscribe",
        );
        Ok(())
    }

    fn request_instruments(&self, request: &RequestInstruments) -> anyhow::Result<()> {
        let venue = request.venue.unwrap_or_else(|| self.venue());
        if let Some(req_venue) = request.venue
            && req_venue != self.venue()
        {
            tracing::warn!("Ignoring mismatched venue in instruments request: {req_venue}");
        }

        let http = self.http_client.clone();
        let instruments_cache = Arc::clone(&self.instruments);
        let sender = self.data_sender.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let params = request.params.clone();
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let clock = self.clock;
        let active_only = self.config.active_only;

        tokio::spawn(async move {
            let http_client = http;
            match http_client
                .request_instruments(active_only)
                .await
                .context("failed to request instruments from BitMEX")
            {
                Ok(instruments) => {
                    {
                        let mut guard = instruments_cache
                            .write()
                            .expect("instrument cache lock poisoned");
                        guard.clear();
                        for instrument in &instruments {
                            guard.insert(instrument.id(), instrument.clone());
                            http_client.add_instrument(instrument.clone());
                        }
                    }

                    let response = DataResponse::Instruments(InstrumentsResponse::new(
                        request_id,
                        client_id,
                        venue,
                        instruments,
                        start_nanos,
                        end_nanos,
                        clock.get_time_ns(),
                        params,
                    ));
                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        tracing::error!("Failed to send instruments response: {e}");
                    }
                }
                Err(e) => tracing::error!("Instrument request failed: {e:?}"),
            }
        });

        Ok(())
    }

    fn request_instrument(&self, request: &RequestInstrument) -> anyhow::Result<()> {
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
                tracing::error!("Failed to send instrument response: {e}");
            }
            return Ok(());
        }

        let http_client = self.http_client.clone();
        let instruments_cache = Arc::clone(&self.instruments);
        let sender = self.data_sender.clone();
        let instrument_id = request.instrument_id;
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start = request.start;
        let end = request.end;
        let params = request.params.clone();
        let clock = self.clock;

        tokio::spawn(async move {
            match http_client
                .request_instrument(instrument_id)
                .await
                .context("failed to request instrument from BitMEX")
            {
                Ok(Some(instrument)) => {
                    http_client.add_instrument(instrument.clone());
                    {
                        let mut guard = instruments_cache
                            .write()
                            .expect("instrument cache lock poisoned");
                        guard.insert(instrument.id(), instrument.clone());
                    }

                    let response = DataResponse::Instrument(Box::new(InstrumentResponse::new(
                        request_id,
                        client_id,
                        instrument.id(),
                        instrument,
                        datetime_to_unix_nanos(start),
                        datetime_to_unix_nanos(end),
                        clock.get_time_ns(),
                        params,
                    )));
                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        tracing::error!("Failed to send instrument response: {e}");
                    }
                }
                Ok(None) => tracing::warn!("BitMEX instrument {instrument_id} not found"),
                Err(e) => tracing::error!("Instrument request failed: {e:?}"),
            }
        });

        Ok(())
    }

    fn request_trades(&self, request: &RequestTrades) -> anyhow::Result<()> {
        let http = self.http_client.clone();
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

        tokio::spawn(async move {
            match http
                .request_trades(instrument_id, start, end, limit)
                .await
                .context("failed to request trades from BitMEX")
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
                Err(e) => tracing::error!("Trade request failed: {e:?}"),
            }
        });

        Ok(())
    }

    fn request_bars(&self, request: &RequestBars) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let bar_type = request.bar_type;
        let start = request.start;
        let end = request.end;
        let limit = request.limit.map(|n| n.get() as u32);
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let params = request.params.clone();
        let clock = self.clock;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);

        tokio::spawn(async move {
            match http
                .request_bars(bar_type, start, end, limit, false)
                .await
                .context("failed to request bars from BitMEX")
            {
                Ok(bars) => {
                    let response = DataResponse::Bars(BarsResponse::new(
                        request_id,
                        client_id,
                        bar_type,
                        bars,
                        start_nanos,
                        end_nanos,
                        clock.get_time_ns(),
                        params,
                    ));
                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        tracing::error!("Failed to send bars response: {e}");
                    }
                }
                Err(e) => tracing::error!("Bar request failed: {e:?}"),
            }
        });

        Ok(())
    }
}
