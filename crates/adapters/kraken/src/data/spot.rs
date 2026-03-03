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

//! Kraken Spot data client implementation.

use std::{
    future::Future,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::AHashMap;
use anyhow::Context;
use async_trait::async_trait;
use futures_util::StreamExt;
use nautilus_common::{
    clients::DataClient,
    live::{get_data_event_sender, get_runtime},
    messages::{
        DataEvent,
        data::{
            BarsResponse, DataResponse, InstrumentResponse, InstrumentsResponse, RequestBars,
            RequestInstrument, RequestInstruments, RequestTrades, SubscribeBars,
            SubscribeBookDeltas, SubscribeIndexPrices, SubscribeInstrument, SubscribeInstruments,
            SubscribeMarkPrices, SubscribeQuotes, SubscribeTrades, TradesResponse, UnsubscribeBars,
            UnsubscribeBookDeltas, UnsubscribeIndexPrices, UnsubscribeMarkPrices,
            UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
};
use nautilus_core::{
    datetime::datetime_to_unix_nanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{Data, OrderBookDeltas_API},
    enums::{AggregationSource, BookType},
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::{
    common::consts::KRAKEN_VENUE,
    config::KrakenDataClientConfig,
    http::KrakenSpotHttpClient,
    websocket::spot_v2::{client::KrakenSpotWebSocketClient, messages::NautilusWsMessage},
};

/// Kraken Spot data client.
///
/// Provides real-time market data from Kraken Spot markets through WebSocket v2.
#[allow(dead_code)]
#[derive(Debug)]
pub struct KrakenSpotDataClient {
    clock: &'static AtomicTime,
    client_id: ClientId,
    config: KrakenDataClientConfig,
    http: KrakenSpotHttpClient,
    ws: KrakenSpotWebSocketClient,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
    instruments: Arc<RwLock<AHashMap<InstrumentId, InstrumentAny>>>,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
}

impl KrakenSpotDataClient {
    /// Creates a new [`KrakenSpotDataClient`] instance.
    pub fn new(client_id: ClientId, config: KrakenDataClientConfig) -> anyhow::Result<Self> {
        let cancellation_token = CancellationToken::new();

        let http = KrakenSpotHttpClient::new(
            config.environment,
            config.base_url.clone(),
            config.timeout_secs,
            None,
            None,
            None,
            config.http_proxy.clone(),
            config.max_requests_per_second,
        )?;

        let ws = KrakenSpotWebSocketClient::new(config.clone(), cancellation_token.clone());

        Ok(Self {
            clock: get_atomic_clock_realtime(),
            client_id,
            config,
            http,
            ws,
            is_connected: AtomicBool::new(false),
            cancellation_token,
            tasks: Vec::new(),
            instruments: Arc::new(RwLock::new(AHashMap::new())),
            data_sender: get_data_event_sender(),
        })
    }

    /// Returns the cached instruments.
    #[must_use]
    pub fn instruments(&self) -> Vec<InstrumentAny> {
        self.instruments
            .read()
            .map(|guard| guard.values().cloned().collect())
            .unwrap_or_default()
    }

    /// Returns a cached instrument by ID.
    #[must_use]
    pub fn get_instrument(&self, instrument_id: &InstrumentId) -> Option<InstrumentAny> {
        self.instruments
            .read()
            .ok()
            .and_then(|guard| guard.get(instrument_id).cloned())
    }

    async fn load_instruments(&mut self) -> anyhow::Result<Vec<InstrumentAny>> {
        let instruments = self
            .http
            .request_instruments(None)
            .await
            .context("Failed to load spot instruments")?;

        if let Ok(mut guard) = self.instruments.write() {
            for instrument in &instruments {
                guard.insert(instrument.id(), instrument.clone());
            }
        }

        self.http.cache_instruments(instruments.clone());

        log::info!(
            "Loaded instruments: client_id={}, count={}",
            self.client_id,
            instruments.len()
        );

        Ok(instruments)
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

    fn spawn_message_handler(&mut self) -> anyhow::Result<()> {
        let stream = self.ws.stream().map_err(|e| anyhow::anyhow!("{e}"))?;
        let data_sender = self.data_sender.clone();
        let cancellation_token = self.cancellation_token.clone();

        let handle = get_runtime().spawn(async move {
            tokio::pin!(stream);

            loop {
                tokio::select! {
                    () = cancellation_token.cancelled() => {
                        log::debug!("Spot message handler cancelled");
                        break;
                    }
                    msg = stream.next() => {
                        match msg {
                            Some(ws_msg) => {
                                Self::handle_ws_message(ws_msg, &data_sender);
                            }
                            None => {
                                log::debug!("Spot WebSocket stream ended");
                                break;
                            }
                        }
                    }
                }
            }
        });

        self.tasks.push(handle);
        Ok(())
    }

    fn handle_ws_message(
        msg: NautilusWsMessage,
        sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    ) {
        match msg {
            NautilusWsMessage::Data(data_vec) => {
                for data in data_vec {
                    if let Err(e) = sender.send(DataEvent::Data(data)) {
                        log::error!("Failed to send data event: {e}");
                    }
                }
            }
            NautilusWsMessage::Deltas(deltas) => {
                let api_deltas = OrderBookDeltas_API::new(deltas);
                if let Err(e) = sender.send(DataEvent::Data(Data::Deltas(api_deltas))) {
                    log::error!("Failed to send deltas event: {e}");
                }
            }
            NautilusWsMessage::Reconnected => {
                log::info!("Spot WebSocket reconnected");
            }
            NautilusWsMessage::OrderRejected(_)
            | NautilusWsMessage::OrderAccepted(_)
            | NautilusWsMessage::OrderCanceled(_)
            | NautilusWsMessage::OrderExpired(_)
            | NautilusWsMessage::OrderUpdated(_)
            | NautilusWsMessage::OrderStatusReport(_)
            | NautilusWsMessage::FillReport(_) => {}
        }
    }
}

#[async_trait(?Send)]
impl DataClient for KrakenSpotDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(*KRAKEN_VENUE)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        log::info!(
            "Starting Spot data client: client_id={}, environment={:?}",
            self.client_id,
            self.config.environment
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Stopping Spot data client: {}", self.client_id);
        self.cancellation_token.cancel();
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::info!("Resetting Spot data client: {}", self.client_id);
        self.cancellation_token.cancel();

        for task in self.tasks.drain(..) {
            task.abort();
        }

        let mut ws = self.ws.clone();
        get_runtime().spawn(async move {
            let _ = ws.close().await;
        });

        if let Ok(mut instruments) = self.instruments.write() {
            instruments.clear();
        }

        self.is_connected.store(false, Ordering::Relaxed);
        self.cancellation_token = CancellationToken::new();
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        log::info!("Disposing Spot data client: {}", self.client_id);
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

        let instruments = self.load_instruments().await?;

        self.ws
            .connect()
            .await
            .context("Failed to connect spot WebSocket")?;
        self.ws
            .wait_until_active(10.0)
            .await
            .context("Spot WebSocket failed to become active")?;

        self.spawn_message_handler()?;
        self.ws.cache_instruments(instruments.clone());

        for instrument in instruments {
            if let Err(e) = self.data_sender.send(DataEvent::Instrument(instrument)) {
                log::error!("Failed to send instrument: {e}");
            }
        }

        self.is_connected.store(true, Ordering::Release);
        log::info!("Connected: client_id={}, product_type=Spot", self.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.is_disconnected() {
            return Ok(());
        }

        self.cancellation_token.cancel();
        let _ = self.ws.close().await;

        for handle in self.tasks.drain(..) {
            if let Err(e) = handle.await {
                log::error!("Error joining WebSocket task: {e:?}");
            }
        }

        self.cancellation_token = CancellationToken::new();
        self.is_connected.store(false, Ordering::Relaxed);

        log::info!("Disconnected: client_id={}", self.client_id);
        Ok(())
    }

    fn subscribe_instruments(&mut self, _cmd: &SubscribeInstruments) -> anyhow::Result<()> {
        log::debug!("subscribe_instruments: Kraken instruments are fetched via HTTP on connect");
        Ok(())
    }

    fn subscribe_instrument(&mut self, _cmd: &SubscribeInstrument) -> anyhow::Result<()> {
        log::debug!("subscribe_instrument: Kraken instruments are fetched via HTTP on connect");
        Ok(())
    }

    fn subscribe_book_deltas(&mut self, cmd: &SubscribeBookDeltas) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let depth = cmd.depth;

        if cmd.book_type != BookType::L2_MBP {
            log::warn!(
                "Book type {:?} not supported by Kraken, skipping subscription",
                cmd.book_type
            );
            return Ok(());
        }

        if let Some(d) = depth {
            let d_val = d.get();
            if !matches!(d_val, 10 | 25 | 100 | 500 | 1000) {
                log::warn!("Invalid depth {d_val} for Kraken Spot, valid: 10, 25, 100, 500, 1000");
                return Ok(());
            }
        }

        let ws = self.ws.clone();
        self.spawn_ws(
            async move {
                ws.subscribe_book(instrument_id, depth.map(|d| d.get() as u32))
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))
            },
            "subscribe book",
        );

        log::info!("Subscribed to book: instrument_id={instrument_id}, depth={depth:?}");
        Ok(())
    }

    fn subscribe_quotes(&mut self, cmd: &SubscribeQuotes) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws.clone();

        self.spawn_ws(
            async move {
                ws.subscribe_quotes(instrument_id)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))
            },
            "subscribe quotes",
        );

        log::info!("Subscribed to quotes: instrument_id={instrument_id}");
        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: &SubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws.clone();

        self.spawn_ws(
            async move {
                ws.subscribe_trades(instrument_id)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))
            },
            "subscribe trades",
        );

        log::info!("Subscribed to trades: instrument_id={instrument_id}");
        Ok(())
    }

    fn subscribe_mark_prices(&mut self, cmd: &SubscribeMarkPrices) -> anyhow::Result<()> {
        log::warn!(
            "Mark price subscription not supported for Spot instrument {}",
            cmd.instrument_id
        );
        Ok(())
    }

    fn subscribe_index_prices(&mut self, cmd: &SubscribeIndexPrices) -> anyhow::Result<()> {
        log::warn!(
            "Index price subscription not supported for Spot instrument {}",
            cmd.instrument_id
        );
        Ok(())
    }

    fn subscribe_bars(&mut self, cmd: &SubscribeBars) -> anyhow::Result<()> {
        let bar_type = cmd.bar_type;

        if bar_type.aggregation_source() != AggregationSource::External {
            log::warn!("Cannot subscribe to {bar_type} bars: only EXTERNAL bars supported");
            return Ok(());
        }

        if !bar_type.spec().is_time_aggregated() {
            log::warn!("Cannot subscribe to {bar_type} bars: only time-based bars supported");
            return Ok(());
        }

        let ws = self.ws.clone();
        self.spawn_ws(
            async move {
                ws.subscribe_bars(bar_type)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))
            },
            "subscribe bars",
        );

        log::info!("Subscribed to bars: bar_type={bar_type}");
        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws.clone();

        self.spawn_ws(
            async move {
                ws.unsubscribe_book(instrument_id)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))
            },
            "unsubscribe book",
        );

        log::info!("Unsubscribed from book: instrument_id={instrument_id}");
        Ok(())
    }

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws.clone();

        self.spawn_ws(
            async move {
                ws.unsubscribe_quotes(instrument_id)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))
            },
            "unsubscribe quotes",
        );

        log::info!("Unsubscribed from quotes: instrument_id={instrument_id}");
        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws.clone();

        self.spawn_ws(
            async move {
                ws.unsubscribe_trades(instrument_id)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))
            },
            "unsubscribe trades",
        );

        log::info!("Unsubscribed from trades: instrument_id={instrument_id}");
        Ok(())
    }

    fn unsubscribe_mark_prices(&mut self, _cmd: &UnsubscribeMarkPrices) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_index_prices(&mut self, _cmd: &UnsubscribeIndexPrices) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        let bar_type = cmd.bar_type;
        let ws = self.ws.clone();

        self.spawn_ws(
            async move {
                ws.unsubscribe_bars(bar_type)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))
            },
            "unsubscribe bars",
        );

        log::info!("Unsubscribed from bars: bar_type={bar_type}");
        Ok(())
    }

    fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        let http = self.http.clone();
        let sender = self.data_sender.clone();
        let instruments_cache = self.instruments.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let venue = *KRAKEN_VENUE;
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match http.request_instruments(None).await {
                Ok(instruments) => {
                    if let Ok(mut guard) = instruments_cache.write() {
                        for instrument in &instruments {
                            guard.insert(instrument.id(), instrument.clone());
                        }
                    }
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
                Err(e) => log::error!("Instruments request failed: {e:?}"),
            }
        });

        Ok(())
    }

    fn request_instrument(&self, request: RequestInstrument) -> anyhow::Result<()> {
        let http = self.http.clone();
        let sender = self.data_sender.clone();
        let instruments = self.instruments.clone();
        let instrument_id = request.instrument_id;
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            {
                if let Ok(guard) = instruments.read()
                    && let Some(instrument) = guard.get(&instrument_id)
                {
                    let response = DataResponse::Instrument(Box::new(InstrumentResponse::new(
                        request_id,
                        client_id,
                        instrument.id(),
                        instrument.clone(),
                        start_nanos,
                        end_nanos,
                        clock.get_time_ns(),
                        params,
                    )));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send instrument response: {e}");
                    }
                    return;
                }
            }

            match http.request_instruments(None).await {
                Ok(all_instruments) => {
                    if let Ok(mut guard) = instruments.write() {
                        for instrument in &all_instruments {
                            guard.insert(instrument.id(), instrument.clone());
                        }
                    }
                    http.cache_instruments(all_instruments.clone());

                    let instrument = all_instruments
                        .into_iter()
                        .find(|i| i.id() == instrument_id);

                    if let Some(instrument) = instrument {
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
                            log::error!("Failed to send instrument response: {e}");
                        }
                    } else {
                        log::error!("Instrument not found: {instrument_id}");
                    }
                }
                Err(e) => log::error!("Instrument request failed: {e:?}"),
            }
        });

        Ok(())
    }
    fn request_trades(&self, request: RequestTrades) -> anyhow::Result<()> {
        let http = self.http.clone();
        let sender = self.data_sender.clone();
        let instrument_id = request.instrument_id;
        let start = request.start;
        let end = request.end;
        let limit = request.limit.map(|n| n.get() as u64);
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let params = request.params;
        let clock = self.clock;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);

        get_runtime().spawn(async move {
            match http.request_trades(instrument_id, start, end, limit).await {
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
                        log::error!("Failed to send trades response: {e}");
                    }
                }
                Err(e) => log::error!("Trades request failed: {e:?}"),
            }
        });

        Ok(())
    }

    fn request_bars(&self, request: RequestBars) -> anyhow::Result<()> {
        let http = self.http.clone();
        let sender = self.data_sender.clone();
        let bar_type = request.bar_type;
        let start = request.start;
        let end = request.end;
        let limit = request.limit.map(|n| n.get() as u64);
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let params = request.params;
        let clock = self.clock;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);

        get_runtime().spawn(async move {
            match http.request_bars(bar_type, start, end, limit).await {
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
                        log::error!("Failed to send bars response: {e}");
                    }
                }
                Err(e) => log::error!("Bars request failed: {e:?}"),
            }
        });

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use nautilus_common::{live::runner::set_data_event_sender, messages::DataEvent};
    use nautilus_model::identifiers::ClientId;
    use rstest::rstest;

    use super::*;
    use crate::config::KrakenDataClientConfig;

    fn setup_test_env() {
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);
    }

    #[rstest]
    fn test_spot_data_client_new() {
        setup_test_env();
        let config = KrakenDataClientConfig::default();
        let client = KrakenSpotDataClient::new(ClientId::from("KRAKEN"), config);
        assert!(client.is_ok());

        let client = client.unwrap();
        assert_eq!(client.client_id(), ClientId::from("KRAKEN"));
        assert_eq!(client.venue(), Some(*KRAKEN_VENUE));
        assert!(!client.is_connected());
        assert!(client.is_disconnected());
        assert!(client.instruments().is_empty());
    }

    #[rstest]
    fn test_spot_data_client_start_stop() {
        setup_test_env();
        let config = KrakenDataClientConfig::default();
        let mut client = KrakenSpotDataClient::new(ClientId::from("KRAKEN"), config).unwrap();

        assert!(client.start().is_ok());
        assert!(client.stop().is_ok());
        assert!(client.is_disconnected());
    }
}
