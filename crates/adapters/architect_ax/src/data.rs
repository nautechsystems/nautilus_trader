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

//! Live market data client implementation for the AX Exchange adapter.

use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::Context;
use async_trait::async_trait;
use dashmap::DashMap;
use futures_util::StreamExt;
use nautilus_common::{
    clients::DataClient,
    live::{runner::get_data_event_sender, runtime::get_runtime},
    messages::{
        DataEvent, DataResponse,
        data::{
            BarsResponse, InstrumentResponse, InstrumentsResponse, RequestBars, RequestInstrument,
            RequestInstruments, SubscribeBars, SubscribeBookDeltas, SubscribeQuotes,
            SubscribeTrades, UnsubscribeBars, UnsubscribeBookDeltas, UnsubscribeQuotes,
            UnsubscribeTrades,
        },
    },
};
use nautilus_core::{
    datetime::datetime_to_unix_nanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{Data, OrderBookDeltas_API},
    identifiers::{ClientId, Venue},
    instruments::InstrumentAny,
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use crate::{
    common::{consts::AX_VENUE, enums::AxMarketDataLevel, parse::map_bar_spec_to_candle_width},
    config::AxDataClientConfig,
    http::client::AxHttpClient,
    websocket::{data::client::AxMdWebSocketClient, messages::NautilusDataWsMessage},
};

/// AX Exchange data client for live market data streaming and historical data requests.
///
/// This client integrates with the Nautilus DataEngine to provide:
/// - Real-time market data via WebSocket subscriptions
/// - Historical data via REST API requests
/// - Automatic instrument discovery and caching
/// - Connection lifecycle management
#[derive(Debug)]
pub struct AxDataClient {
    /// The client ID for this data client.
    client_id: ClientId,
    /// Configuration for the data client.
    config: AxDataClientConfig,
    /// HTTP client for REST API requests.
    http_client: AxHttpClient,
    /// WebSocket client for real-time data streaming.
    ws_client: AxMdWebSocketClient,
    /// Whether the client is currently connected.
    is_connected: AtomicBool,
    /// Cancellation token for async operations.
    cancellation_token: CancellationToken,
    /// Background task handles.
    tasks: Vec<JoinHandle<()>>,
    /// Channel sender for emitting data events to the DataEngine.
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    /// Cached instruments by symbol (shared with HTTP client).
    instruments: Arc<DashMap<Ustr, InstrumentAny>>,
    /// High-resolution clock for timestamps.
    clock: &'static AtomicTime,
}

impl AxDataClient {
    /// Creates a new [`AxDataClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the data event sender cannot be obtained.
    pub fn new(
        client_id: ClientId,
        config: AxDataClientConfig,
        http_client: AxHttpClient,
        ws_client: AxMdWebSocketClient,
    ) -> anyhow::Result<Self> {
        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();

        // Share instruments cache with HTTP client
        let instruments = http_client.instruments_cache.clone();

        Ok(Self {
            client_id,
            config,
            http_client,
            ws_client,
            is_connected: AtomicBool::new(false),
            cancellation_token: CancellationToken::new(),
            tasks: Vec::new(),
            data_sender,
            instruments,
            clock,
        })
    }

    /// Returns the venue for this data client.
    #[must_use]
    pub fn venue(&self) -> Venue {
        *AX_VENUE
    }

    /// Returns a reference to the instruments cache.
    #[must_use]
    pub fn instruments(&self) -> &Arc<DashMap<Ustr, InstrumentAny>> {
        &self.instruments
    }

    /// Spawns a message handler task to forward WebSocket data to the DataEngine.
    fn spawn_message_handler(&mut self) {
        let stream = self.ws_client.stream();
        let data_sender = self.data_sender.clone();
        let cancellation_token = self.cancellation_token.clone();

        let handle = get_runtime().spawn(async move {
            tokio::pin!(stream);

            loop {
                tokio::select! {
                    () = cancellation_token.cancelled() => {
                        log::debug!("Message handler cancelled");
                        break;
                    }
                    msg = stream.next() => {
                        match msg {
                            Some(ws_msg) => {
                                Self::handle_ws_message(ws_msg, &data_sender);
                            }
                            None => {
                                log::debug!("WebSocket stream ended");
                                break;
                            }
                        }
                    }
                }
            }
        });

        self.tasks.push(handle);
    }

    /// Handles a WebSocket message and forwards data to the DataEngine.
    fn handle_ws_message(
        msg: NautilusDataWsMessage,
        sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    ) {
        match msg {
            NautilusDataWsMessage::Data(data_vec) => {
                for data in data_vec {
                    if let Err(e) = sender.send(DataEvent::Data(data)) {
                        log::error!("Failed to send data event: {e}");
                    }
                }
            }
            NautilusDataWsMessage::Deltas(deltas) => {
                let api_deltas = OrderBookDeltas_API::new(deltas);
                if let Err(e) = sender.send(DataEvent::Data(Data::Deltas(api_deltas))) {
                    log::error!("Failed to send deltas event: {e}");
                }
            }
            NautilusDataWsMessage::Bar(bar) => {
                if let Err(e) = sender.send(DataEvent::Data(Data::Bar(bar))) {
                    log::error!("Failed to send bar event: {e}");
                }
            }
            NautilusDataWsMessage::Heartbeat => {
                log::trace!("Received heartbeat");
            }
            NautilusDataWsMessage::Reconnected => {
                log::info!("WebSocket reconnected");
            }
            NautilusDataWsMessage::Error(err) => {
                log::error!("WebSocket error: {err:?}");
            }
        }
    }

    fn spawn_ws<F>(&self, fut: F, context: &'static str)
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        get_runtime().spawn(async move {
            if let Err(e) = fut.await {
                log::error!("{context}: {e:?}");
            }
        });
    }
}

#[async_trait(?Send)]
impl DataClient for AxDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(*AX_VENUE)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        log::debug!("Starting {}", self.client_id);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::debug!("Stopping {}", self.client_id);
        self.cancellation_token.cancel();
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::debug!("Resetting {}", self.client_id);
        self.cancellation_token.cancel();
        self.tasks.clear();
        self.cancellation_token = CancellationToken::new();
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        log::debug!("Disposing {}", self.client_id);
        self.cancellation_token.cancel();
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Acquire)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        log::info!("Connecting {}", self.client_id);

        if self.config.has_api_credentials() {
            let api_key = self
                .config
                .api_key
                .clone()
                .or_else(|| std::env::var("AX_API_KEY").ok())
                .context("AX_API_KEY not configured")?;

            let api_secret = self
                .config
                .api_secret
                .clone()
                .or_else(|| std::env::var("AX_API_SECRET").ok())
                .context("AX_API_SECRET not configured")?;

            let token = self
                .http_client
                .authenticate(&api_key, &api_secret, 86400)
                .await
                .context("Failed to authenticate with Ax")?;
            log::info!("Authenticated with Ax");
            self.ws_client.set_auth_token(token);
        }

        let instruments = self
            .http_client
            .request_instruments(None, None)
            .await
            .context("Failed to fetch instruments")?;

        for instrument in &instruments {
            self.ws_client.cache_instrument(instrument.clone());
        }
        self.http_client.cache_instruments(instruments);
        log::info!(
            "Cached {} instruments",
            self.http_client.get_cached_symbols().len()
        );

        self.ws_client
            .connect()
            .await
            .context("Failed to connect WebSocket")?;
        log::info!("WebSocket connected");
        self.spawn_message_handler();

        self.is_connected.store(true, Ordering::Release);
        log::info!("Connected {}", self.client_id);

        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        log::info!("Disconnecting {}", self.client_id);
        self.cancellation_token.cancel();
        self.ws_client.close().await;

        for task in self.tasks.drain(..) {
            task.abort();
        }

        self.is_connected.store(false, Ordering::Release);
        log::info!("Disconnected {}", self.client_id);

        Ok(())
    }

    fn subscribe_quotes(&mut self, cmd: &SubscribeQuotes) -> anyhow::Result<()> {
        let symbol = cmd.instrument_id.symbol.to_string();
        log::debug!("Subscribing to quotes for {symbol}");

        let ws = self.ws_client.clone();
        self.spawn_ws(
            async move {
                ws.subscribe(&symbol, AxMarketDataLevel::Level1)
                    .await
                    .map_err(|e| anyhow::anyhow!(e))
            },
            "subscribe quotes",
        );

        Ok(())
    }

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        let symbol = cmd.instrument_id.symbol.to_string();
        log::debug!("Unsubscribing from quotes for {symbol}");

        let ws = self.ws_client.clone();
        self.spawn_ws(
            async move {
                ws.unsubscribe(&symbol)
                    .await
                    .map_err(|e| anyhow::anyhow!(e))
            },
            "unsubscribe quotes",
        );

        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: &SubscribeTrades) -> anyhow::Result<()> {
        let symbol = cmd.instrument_id.symbol.to_string();
        log::debug!("Subscribing to trades for {symbol}");

        // Trades come with Level1 subscription
        let ws = self.ws_client.clone();
        self.spawn_ws(
            async move {
                ws.subscribe(&symbol, AxMarketDataLevel::Level1)
                    .await
                    .map_err(|e| anyhow::anyhow!(e))
            },
            "subscribe trades",
        );

        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        let symbol = cmd.instrument_id.symbol.to_string();
        log::debug!("Unsubscribing from trades for {symbol}");

        let ws = self.ws_client.clone();
        self.spawn_ws(
            async move {
                ws.unsubscribe(&symbol)
                    .await
                    .map_err(|e| anyhow::anyhow!(e))
            },
            "unsubscribe trades",
        );

        Ok(())
    }

    fn subscribe_book_deltas(&mut self, cmd: &SubscribeBookDeltas) -> anyhow::Result<()> {
        let symbol = cmd.instrument_id.symbol.to_string();
        let level = AxMarketDataLevel::Level2;
        log::debug!("Subscribing to book deltas for {symbol} at {level:?}");

        let ws = self.ws_client.clone();
        self.spawn_ws(
            async move {
                ws.subscribe(&symbol, level)
                    .await
                    .map_err(|e| anyhow::anyhow!(e))
            },
            "subscribe book deltas",
        );

        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        let symbol = cmd.instrument_id.symbol.to_string();
        log::debug!("Unsubscribing from book deltas for {symbol}");

        let ws = self.ws_client.clone();
        self.spawn_ws(
            async move {
                ws.unsubscribe(&symbol)
                    .await
                    .map_err(|e| anyhow::anyhow!(e))
            },
            "unsubscribe book deltas",
        );

        Ok(())
    }

    fn subscribe_bars(&mut self, cmd: &SubscribeBars) -> anyhow::Result<()> {
        let bar_type = cmd.bar_type;
        let symbol = bar_type.instrument_id().symbol.to_string();
        let width = map_bar_spec_to_candle_width(&bar_type.spec())?;
        log::debug!("Subscribing to bars for {bar_type} (width: {width:?})");

        let ws = self.ws_client.clone();
        self.spawn_ws(
            async move {
                ws.subscribe_candles(&symbol, width)
                    .await
                    .map_err(|e| anyhow::anyhow!(e))
            },
            "subscribe bars",
        );

        Ok(())
    }

    fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        let bar_type = cmd.bar_type;
        let symbol = bar_type.instrument_id().symbol.to_string();
        let width = map_bar_spec_to_candle_width(&bar_type.spec())?;
        log::debug!("Unsubscribing from bars for {bar_type}");

        let ws = self.ws_client.clone();
        self.spawn_ws(
            async move {
                ws.unsubscribe_candles(&symbol, width)
                    .await
                    .map_err(|e| anyhow::anyhow!(e))
            },
            "unsubscribe bars",
        );

        Ok(())
    }

    fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let venue = *AX_VENUE;
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match http.request_instruments(None, None).await {
                Ok(instruments) => {
                    log::info!("Fetched {} instruments from Ax", instruments.len());
                    http.cache_instruments(instruments.clone());

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
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let instrument_id = request.instrument_id;
        let symbol = instrument_id.symbol.to_string();
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match http.request_instrument(&symbol, None, None).await {
                Ok(instrument) => {
                    log::debug!("Fetched instrument {symbol} from Ax");
                    http.cache_instrument(instrument.clone());

                    let response = DataResponse::Instrument(Box::new(InstrumentResponse::new(
                        request_id,
                        client_id,
                        instrument_id,
                        instrument,
                        start_nanos,
                        end_nanos,
                        clock.get_time_ns(),
                        params,
                    )));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send instrument response: {e}");
                    }
                }
                Err(e) => {
                    log::error!("Failed to request instrument {symbol}: {e}");
                }
            }
        });

        Ok(())
    }

    fn request_bars(&self, request: RequestBars) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let bar_type = request.bar_type;
        let symbol = bar_type.instrument_id().symbol.to_string();
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;
        let width = match map_bar_spec_to_candle_width(&bar_type.spec()) {
            Ok(w) => w,
            Err(e) => {
                log::error!("Failed to map bar type {bar_type}: {e}");
                return Err(e);
            }
        };

        get_runtime().spawn(async move {
            let start_ns = start_nanos.map_or(0, |n| n.as_i64());
            let end_ns = end_nanos.map_or(clock.get_time_ns().as_i64(), |n| n.as_i64());

            match http.request_bars(&symbol, start_ns, end_ns, width).await {
                Ok(bars) => {
                    log::debug!("Fetched {} bars for {symbol}", bars.len());

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
                        log::error!("Failed to send bars response: {e}");
                    }
                }
                Err(e) => {
                    log::error!("Failed to request bars for {symbol}: {e}");
                }
            }
        });

        Ok(())
    }
}
