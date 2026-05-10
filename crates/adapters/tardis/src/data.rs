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

//! Tardis data client for streaming replay or live data into the engine.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use ahash::{AHashMap, AHashSet};
use futures_util::{SinkExt, StreamExt};
use nautilus_common::{
    clients::DataClient,
    live::{runner::get_data_event_sender, runtime::get_runtime},
    messages::{
        DataEvent,
        data::{
            subscribe::{SubscribeFundingRates, SubscribeIndexPrices, SubscribeMarkPrices},
            unsubscribe::{UnsubscribeFundingRates, UnsubscribeIndexPrices, UnsubscribeMarkPrices},
        },
    },
};
use nautilus_core::string::urlencoding;
use nautilus_model::{
    data::Data,
    identifiers::{ClientId, Venue},
};
use tokio::{sync::mpsc::UnboundedSender, task::JoinHandle};
use tokio_tungstenite::{connect_async, tungstenite};
use tokio_util::sync::CancellationToken;

use crate::{
    common::{
        consts::{
            WS_HEARTBEAT_INTERVAL_SECS, WS_INITIAL_RECONNECT_DELAY_SECS,
            WS_MAX_RECONNECT_DELAY_SECS,
        },
        enums::TardisDataType,
        urls::resolve_ws_base_url,
    },
    config::{BookSnapshotOutput, TardisDataClientConfig},
    http::TardisHttpClient,
    machine::{
        cache::DerivativeTickerCache,
        client::determine_instrument_info,
        message::WsMessage,
        parse::{
            parse_derivative_ticker_index_price, parse_derivative_ticker_mark_price,
            parse_tardis_ws_message, parse_tardis_ws_message_funding_rate,
        },
        types::{TardisInstrumentKey, TardisInstrumentMiniInfo},
    },
};

/// Tardis data client for streaming replay or live data into the platform.
#[derive(Debug)]
pub struct TardisDataClient {
    client_id: ClientId,
    config: TardisDataClientConfig,
    is_connected: Arc<AtomicBool>,
    cancellation_token: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
    data_sender: UnboundedSender<DataEvent>,
}

impl TardisDataClient {
    /// Creates a new [`TardisDataClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the data event sender is not initialized.
    pub fn new(client_id: ClientId, config: TardisDataClientConfig) -> anyhow::Result<Self> {
        let data_sender = get_data_event_sender();

        Ok(Self {
            client_id,
            config,
            is_connected: Arc::new(AtomicBool::new(false)),
            cancellation_token: CancellationToken::new(),
            tasks: Vec::new(),
            data_sender,
        })
    }

    /// Returns `true` if the client is configured for live streaming mode.
    fn is_stream_mode(&self) -> bool {
        self.config.options.is_empty() && !self.config.stream_options.is_empty()
    }

    /// Builds the WebSocket URL for connecting to the Tardis Machine Server.
    ///
    /// Ensures `derivative_ticker` is included in the data types for each
    /// option set so that mark price, index price, and funding rate events
    /// are available without requiring manual configuration.
    fn build_ws_url(&self, base_url: &str) -> anyhow::Result<String> {
        let deriv = TardisDataType::DerivativeTicker.as_tardis_str();

        if self.is_stream_mode() {
            let mut options = self.config.stream_options.clone();
            for opt in &mut options {
                if !opt.data_types.iter().any(|dt| dt == deriv) {
                    opt.data_types.push(deriv.to_string());
                }
            }
            let options_json = serde_json::to_string(&options)?;
            Ok(format!(
                "{base_url}/ws-stream-normalized?options={}",
                urlencoding::encode(&options_json)
            ))
        } else {
            let mut options = self.config.options.clone();
            for opt in &mut options {
                if !opt.data_types.iter().any(|dt| dt == deriv) {
                    opt.data_types.push(deriv.to_string());
                }
            }
            let options_json = serde_json::to_string(&options)?;
            Ok(format!(
                "{base_url}/ws-replay-normalized?options={}",
                urlencoding::encode(&options_json)
            ))
        }
    }

    /// Spawns the WebSocket message processing loop using an already-connected
    /// stream. The initial handshake happens in `connect()` so callers get an
    /// error if the first connection fails. In stream mode the spawned task
    /// handles subsequent reconnections automatically.
    fn spawn_ws_task(
        &mut self,
        ws_stream: tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        url: String,
        instrument_map: AHashMap<TardisInstrumentKey, Arc<TardisInstrumentMiniInfo>>,
        book_snapshot_output: BookSnapshotOutput,
        is_stream_mode: bool,
    ) {
        let sender = self.data_sender.clone();
        let cancel = self.cancellation_token.clone();
        let connected = self.is_connected.clone();

        let handle = get_runtime().spawn(async move {
            let mut reconnect_delay = Duration::from_secs(WS_INITIAL_RECONNECT_DELAY_SECS);
            let instrument_map = instrument_map;

            // Process the initial (already-connected) stream
            let should_reconnect = Self::run_ws_session(
                ws_stream,
                &cancel,
                &sender,
                &instrument_map,
                &book_snapshot_output,
            )
            .await;

            if !should_reconnect || !is_stream_mode || cancel.is_cancelled() {
                connected.store(false, Ordering::Release);
                return;
            }

            // Mark disconnected while reconnecting so health checks see the outage
            connected.store(false, Ordering::Release);

            // Reconnection loop (stream mode only)
            loop {
                log::warn!(
                    "Stream disconnected, reconnecting in {}s",
                    reconnect_delay.as_secs()
                );

                tokio::select! {
                    () = tokio::time::sleep(reconnect_delay) => {}
                    () = cancel.cancelled() => break,
                }

                reconnect_delay = std::cmp::min(
                    reconnect_delay * 2,
                    Duration::from_secs(WS_MAX_RECONNECT_DELAY_SECS),
                );

                // Reconnect WS first (critical path), then refresh instruments
                let ws_result = tokio::select! {
                    result = connect_async(&url) => Some(result),
                    () = cancel.cancelled() => None,
                };

                let Some(ws_result) = ws_result else {
                    break;
                };

                match ws_result {
                    Ok((ws_stream, _)) => {
                        log::info!("Reconnected to Tardis Machine");
                        connected.store(true, Ordering::Release);
                        reconnect_delay = Duration::from_secs(WS_INITIAL_RECONNECT_DELAY_SECS);

                        let should_reconnect = Self::run_ws_session(
                            ws_stream,
                            &cancel,
                            &sender,
                            &instrument_map,
                            &book_snapshot_output,
                        )
                        .await;

                        if !should_reconnect || cancel.is_cancelled() {
                            break;
                        }

                        connected.store(false, Ordering::Release);
                    }
                    Err(e) => {
                        if cancel.is_cancelled() {
                            break;
                        }

                        log::warn!(
                            "Failed to reconnect to Tardis Machine: {e}, retrying in {}s",
                            reconnect_delay.as_secs()
                        );
                    }
                }
            }

            connected.store(false, Ordering::Release);
        });

        self.tasks.push(handle);
    }

    /// Runs a single WebSocket session: starts heartbeat, processes messages,
    /// and returns whether the caller should attempt reconnection.
    async fn run_ws_session(
        ws_stream: tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        cancel: &CancellationToken,
        sender: &UnboundedSender<DataEvent>,
        instrument_map: &AHashMap<TardisInstrumentKey, Arc<TardisInstrumentMiniInfo>>,
        book_snapshot_output: &BookSnapshotOutput,
    ) -> bool {
        let (mut writer, mut reader) = ws_stream.split();

        let heartbeat_token = cancel.child_token();
        let heartbeat_signal = heartbeat_token.clone();

        get_runtime().spawn(async move {
            let mut interval =
                tokio::time::interval(Duration::from_secs(WS_HEARTBEAT_INTERVAL_SECS));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        log::trace!("Sending PING");

                        if let Err(e) = writer.send(tungstenite::Message::Ping(vec![].into())).await {
                            log::debug!("Heartbeat send failed: {e}");
                            break;
                        }
                    }
                    () = heartbeat_signal.cancelled() => break,
                }
            }
        });

        let should_reconnect = Self::run_ws_loop(
            &mut reader,
            cancel,
            sender,
            instrument_map,
            book_snapshot_output,
        )
        .await;

        heartbeat_token.cancel();
        should_reconnect
    }

    /// Extracts and sends all data events from a `DerivativeTicker` message:
    /// funding rate, mark price, and index price. Only emits events when values
    /// change from the previous update. Returns `false` if the channel is broken
    /// and the caller should exit the loop.
    fn send_derivative_ticker_events(
        ws_msg: &WsMessage,
        info: &Arc<TardisInstrumentMiniInfo>,
        sender: &UnboundedSender<DataEvent>,
        cache: &mut DerivativeTickerCache,
    ) -> bool {
        if let Some(funding) = parse_tardis_ws_message_funding_rate(ws_msg.clone(), info)
            && cache.should_emit_funding_rate(&funding)
            && sender.send(DataEvent::FundingRate(funding)).is_err()
        {
            return false;
        }

        if let WsMessage::DerivativeTicker(msg) = ws_msg {
            if let Ok(Some(mark_price)) =
                parse_derivative_ticker_mark_price(msg, info.instrument_id, info.price_precision)
                && cache.should_emit_mark_price(&mark_price)
                && sender
                    .send(DataEvent::Data(Data::MarkPriceUpdate(mark_price)))
                    .is_err()
            {
                return false;
            }

            if let Ok(Some(index_price)) =
                parse_derivative_ticker_index_price(msg, info.instrument_id, info.price_precision)
                && cache.should_emit_index_price(&index_price)
                && sender
                    .send(DataEvent::Data(Data::IndexPriceUpdate(index_price)))
                    .is_err()
            {
                return false;
            }
        }

        true
    }

    /// Processes WebSocket messages until the stream ends, an error occurs, or
    /// the cancellation token fires. Returns `true` if the caller should attempt
    /// reconnection (stream mode only).
    async fn run_ws_loop(
        reader: &mut futures_util::stream::SplitStream<
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
        >,
        cancel: &CancellationToken,
        sender: &UnboundedSender<DataEvent>,
        instrument_map: &AHashMap<TardisInstrumentKey, Arc<TardisInstrumentMiniInfo>>,
        book_snapshot_output: &BookSnapshotOutput,
    ) -> bool {
        let mut ticker_cache = DerivativeTickerCache::default();

        loop {
            let msg = tokio::select! {
                msg = reader.next() => msg,
                () = cancel.cancelled() => {
                    log::debug!("Stream task cancelled");
                    return false;
                }
            };

            match msg {
                Some(Ok(tungstenite::Message::Text(text))) => {
                    match serde_json::from_str::<WsMessage>(&text) {
                        Ok(ws_msg) => {
                            if matches!(ws_msg, WsMessage::Disconnect(_)) {
                                log::debug!("Received disconnect message");
                                continue;
                            }

                            let info = determine_instrument_info(&ws_msg, instrument_map);

                            if let Some(info) = info {
                                if matches!(ws_msg, WsMessage::DerivativeTicker(_)) {
                                    if !Self::send_derivative_ticker_events(
                                        &ws_msg,
                                        &info,
                                        sender,
                                        &mut ticker_cache,
                                    ) {
                                        return false;
                                    }
                                } else {
                                    let event = parse_tardis_ws_message(
                                        ws_msg,
                                        &info,
                                        book_snapshot_output,
                                    )
                                    .map(DataEvent::Data);

                                    if let Some(event) = event
                                        && let Err(e) = sender.send(event)
                                    {
                                        log::error!("Failed to send data event: {e}");
                                        return false;
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to deserialize message: {e}");
                        }
                    }
                }
                Some(Ok(tungstenite::Message::Close(frame))) => {
                    if let Some(frame) = frame {
                        log::info!("WebSocket closed: {} {}", frame.code, frame.reason);
                    } else {
                        log::info!("WebSocket closed");
                    }
                    return true;
                }
                Some(Ok(_)) => {}
                Some(Err(e)) => {
                    log::error!("WebSocket error: {e}");
                    return true;
                }
                None => {
                    log::info!("Stream ended");
                    return true;
                }
            }
        }
    }
}

#[async_trait::async_trait(?Send)]
impl DataClient for TardisDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        None // Tardis is multi-venue
    }

    fn start(&mut self) -> anyhow::Result<()> {
        log::info!("Starting {}", self.client_id);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Stopping {}", self.client_id);
        self.cancellation_token.cancel();

        for handle in self.tasks.drain(..) {
            handle.abort();
        }
        self.is_connected.store(false, Ordering::Release);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        self.cancellation_token.cancel();

        for handle in self.tasks.drain(..) {
            handle.abort();
        }
        self.cancellation_token = CancellationToken::new();
        self.is_connected.store(false, Ordering::Release);
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        self.stop()
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Acquire)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    fn subscribe_mark_prices(&mut self, cmd: SubscribeMarkPrices) -> anyhow::Result<()> {
        log::info!("Subscribed mark prices for {}", cmd.instrument_id);
        Ok(())
    }

    fn subscribe_index_prices(&mut self, cmd: SubscribeIndexPrices) -> anyhow::Result<()> {
        log::info!("Subscribed index prices for {}", cmd.instrument_id);
        Ok(())
    }

    fn subscribe_funding_rates(&mut self, cmd: SubscribeFundingRates) -> anyhow::Result<()> {
        log::info!("Subscribed funding rates for {}", cmd.instrument_id);
        Ok(())
    }

    fn unsubscribe_mark_prices(&mut self, cmd: &UnsubscribeMarkPrices) -> anyhow::Result<()> {
        log::info!("Unsubscribed mark prices for {}", cmd.instrument_id);
        Ok(())
    }

    fn unsubscribe_index_prices(&mut self, cmd: &UnsubscribeIndexPrices) -> anyhow::Result<()> {
        log::info!("Unsubscribed index prices for {}", cmd.instrument_id);
        Ok(())
    }

    fn unsubscribe_funding_rates(&mut self, cmd: &UnsubscribeFundingRates) -> anyhow::Result<()> {
        log::info!("Unsubscribed funding rates for {}", cmd.instrument_id);
        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected() {
            return Ok(());
        }

        if self.config.options.is_empty() && self.config.stream_options.is_empty() {
            anyhow::bail!("Either replay `options` or `stream_options` must be provided");
        }

        let is_stream_mode = self.is_stream_mode();
        let book_snapshot_output = self.config.book_snapshot_output.clone();

        let http_client = TardisHttpClient::new(
            self.config.api_key.as_deref(),
            None,
            None,
            self.config.normalize_symbols,
            self.config.proxy_url.clone(),
        )?;

        let exchanges: AHashSet<_> = if is_stream_mode {
            self.config
                .stream_options
                .iter()
                .map(|opt| opt.exchange)
                .collect()
        } else {
            self.config.options.iter().map(|opt| opt.exchange).collect()
        };

        let base_url = resolve_ws_base_url(self.config.tardis_ws_url.as_deref())?;
        let (instrument_map, instruments) = http_client
            .bootstrap_instruments(&exchanges)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to bootstrap instruments: {e}"))?;

        for instrument in instruments {
            if let Err(e) = self.data_sender.send(DataEvent::Instrument(instrument)) {
                log::error!("Failed to send instrument event: {e}");
            }
        }

        let url = self.build_ws_url(&base_url)?;

        let mode_label = if is_stream_mode { "stream" } else { "replay" };
        log::info!("Connecting to Tardis Machine {mode_label}");
        log::debug!("URL: {url}");

        self.cancellation_token = CancellationToken::new();

        let (ws_stream, _) = connect_async(&url)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to connect to Tardis Machine: {e}"))?;

        log::info!("Connected to Tardis Machine");

        self.spawn_ws_task(
            ws_stream,
            url,
            instrument_map,
            book_snapshot_output,
            is_stream_mode,
        );
        self.is_connected.store(true, Ordering::Release);

        log::info!("Connected: {}", self.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.cancellation_token.cancel();
        self.cancellation_token = CancellationToken::new();

        let handles: Vec<_> = self.tasks.drain(..).collect();
        if !handles.is_empty() {
            for handle in handles {
                if let Err(e) = handle.await {
                    log::error!("Error joining task: {e}");
                }
            }
            log::info!("Disconnected: {}", self.client_id);
        }

        self.is_connected.store(false, Ordering::Release);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;
    use nautilus_common::live::runner::set_data_event_sender;
    use nautilus_model::identifiers::ClientId;
    use rstest::rstest;

    use super::*;
    use crate::{
        common::enums::TardisExchange, config::TardisDataClientConfig,
        machine::types::ReplayNormalizedRequestOptions,
    };

    fn setup_test_env() {
        use std::cell::OnceCell;

        thread_local! {
            static INIT: OnceCell<()> = const { OnceCell::new() };
        }

        INIT.with(|cell| {
            cell.get_or_init(|| {
                let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
                set_data_event_sender(sender);
            });
        });
    }

    #[rstest]
    fn test_build_ws_url_injects_derivative_ticker() {
        setup_test_env();

        let config = TardisDataClientConfig {
            options: vec![ReplayNormalizedRequestOptions {
                exchange: TardisExchange::BinanceFutures,
                symbols: Some(vec!["BTCUSDT".to_string()]),
                from: NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
                to: NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
                data_types: vec!["trade".to_string()],
                with_disconnect_messages: Some(false),
            }],
            ..Default::default()
        };

        let client = TardisDataClient::new(ClientId::new("TARDIS"), config).unwrap();
        let url = client.build_ws_url("ws://localhost:8001").unwrap();

        assert!(
            url.contains("derivative_ticker"),
            "URL should contain derivative_ticker but was: {url}"
        );
        assert!(url.contains("trade"), "URL should still contain trade");
    }

    #[rstest]
    fn test_build_ws_url_does_not_duplicate_derivative_ticker() {
        setup_test_env();

        let config = TardisDataClientConfig {
            options: vec![ReplayNormalizedRequestOptions {
                exchange: TardisExchange::BinanceFutures,
                symbols: Some(vec!["BTCUSDT".to_string()]),
                from: NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
                to: NaiveDate::from_ymd_opt(2024, 1, 2).unwrap(),
                data_types: vec!["trade".to_string(), "derivative_ticker".to_string()],
                with_disconnect_messages: Some(false),
            }],
            ..Default::default()
        };

        let client = TardisDataClient::new(ClientId::new("TARDIS"), config).unwrap();
        let ws_url = client.build_ws_url("ws://localhost:8001").unwrap();

        let decoded = urlencoding::decode(ws_url.split("options=").nth(1).unwrap()).unwrap();
        let count = decoded.matches("derivative_ticker").count();
        assert_eq!(count, 1, "derivative_ticker should appear exactly once");
    }
}
