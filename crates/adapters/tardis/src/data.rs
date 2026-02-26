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

//! Tardis data client for streaming replay data into the live engine.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use ahash::AHashSet;
use futures_util::{SinkExt, StreamExt};
use nautilus_common::{
    clients::DataClient,
    live::{runner::get_data_event_sender, runtime::get_runtime},
    messages::DataEvent,
};
use nautilus_core::{
    parsing::precision_from_str,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::identifiers::{ClientId, Venue};
use tokio::{sync::mpsc::UnboundedSender, task::JoinHandle};
use tokio_tungstenite::{connect_async, tungstenite};
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use crate::{
    common::consts::TARDIS_MACHINE_WS_URL,
    config::TardisDataClientConfig,
    http::{TardisHttpClient, parse::parse_instrument_any},
    machine::{
        client::determine_instrument_info,
        message::WsMessage,
        parse::{parse_tardis_ws_message, parse_tardis_ws_message_funding_rate},
        types::{TardisInstrumentKey, TardisInstrumentMiniInfo},
    },
    parse::{normalize_instrument_id, parse_instrument_id},
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
    #[allow(dead_code)]
    clock: &'static AtomicTime,
}

impl TardisDataClient {
    /// Creates a new [`TardisDataClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the data event sender is not initialized.
    pub fn new(client_id: ClientId, config: TardisDataClientConfig) -> anyhow::Result<Self> {
        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();

        Ok(Self {
            client_id,
            config,
            is_connected: Arc::new(AtomicBool::new(false)),
            cancellation_token: CancellationToken::new(),
            tasks: Vec::new(),
            data_sender,
            clock,
        })
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
        self.tasks.clear();
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

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected() {
            return Ok(());
        }

        if self.config.options.is_empty() {
            anyhow::bail!("Replay options cannot be empty");
        }

        let normalize_symbols = self.config.normalize_symbols;
        let book_snapshot_output = self.config.book_snapshot_output.clone();

        let http_client = TardisHttpClient::new(
            self.config.api_key.as_deref(),
            None, // base_url
            None, // timeout_secs
            normalize_symbols,
        )?;

        let base_url = self
            .config
            .tardis_ws_url
            .clone()
            .or_else(|| std::env::var(TARDIS_MACHINE_WS_URL).ok())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "Tardis Machine `tardis_ws_url` must be provided or \
                     set in the '{TARDIS_MACHINE_WS_URL}' environment variable"
                )
            })?;

        let exchanges: AHashSet<_> = self.config.options.iter().map(|opt| opt.exchange).collect();
        let mut instrument_map: HashMap<TardisInstrumentKey, Arc<TardisInstrumentMiniInfo>> =
            HashMap::new();

        for exchange in &exchanges {
            log::info!("Fetching instruments for {exchange}");

            let instruments_info = http_client
                .instruments_info(*exchange, None, None)
                .await
                .map_err(|e| {
                    anyhow::anyhow!("Failed to fetch instrument info for {exchange}: {e}")
                })?;

            log::info!(
                "Received {} instruments for {exchange}",
                instruments_info.len()
            );

            for inst in &instruments_info {
                let instrument_type = inst.instrument_type;
                let price_precision = precision_from_str(&inst.price_increment.to_string());
                let size_precision = precision_from_str(&inst.amount_increment.to_string());

                let instrument_id = if normalize_symbols {
                    normalize_instrument_id(exchange, inst.id, &instrument_type, inst.inverse)
                } else {
                    parse_instrument_id(exchange, inst.id)
                };

                let info = TardisInstrumentMiniInfo::new(
                    instrument_id,
                    Some(Ustr::from(&inst.id)),
                    *exchange,
                    price_precision,
                    size_precision,
                );
                let key = info.as_tardis_instrument_key();
                instrument_map.insert(key, Arc::new(info));
            }

            // Parse and emit Nautilus instrument definitions
            for inst in instruments_info {
                for instrument in parse_instrument_any(inst, None, None, normalize_symbols) {
                    if let Err(e) = self.data_sender.send(DataEvent::Instrument(instrument)) {
                        log::error!("Failed to send instrument event: {e}");
                    }
                }
            }
        }

        let options_json = serde_json::to_string(&self.config.options)?;
        let url = format!(
            "{base_url}/ws-replay-normalized?options={}",
            urlencoding::encode(&options_json)
        );

        log::info!("Connecting to Tardis Machine replay");
        log::debug!("URL: {base_url}/ws-replay-normalized?options={options_json}");

        let (ws_stream, _) = connect_async(&url)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to connect to Tardis Machine: {e}"))?;

        log::info!("Connected to Tardis Machine");

        // Ensure a fresh token so reconnect after stop() works
        self.cancellation_token = CancellationToken::new();

        let sender = self.data_sender.clone();
        let cancel = self.cancellation_token.clone();
        let connected = self.is_connected.clone();

        let handle = get_runtime().spawn(async move {
            let (mut writer, mut reader) = ws_stream.split();

            // Child token inherits cancellation from parent `cancel`, so
            // reset()/stop() cancelling the main token also stops the heartbeat
            let heartbeat_token = cancel.child_token();
            let heartbeat_signal = heartbeat_token.clone();
            get_runtime().spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(10));
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

            loop {
                let msg = tokio::select! {
                    msg = reader.next() => msg,
                    () = cancel.cancelled() => {
                        log::debug!("Replay stream task cancelled");
                        break;
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

                                let info = determine_instrument_info(&ws_msg, &instrument_map);
                                if let Some(info) = info {
                                    let event = if matches!(ws_msg, WsMessage::DerivativeTicker(_))
                                    {
                                        parse_tardis_ws_message_funding_rate(ws_msg, info)
                                            .map(DataEvent::FundingRate)
                                    } else {
                                        parse_tardis_ws_message(ws_msg, info, &book_snapshot_output)
                                            .map(DataEvent::Data)
                                    };

                                    if let Some(event) = event
                                        && let Err(e) = sender.send(event)
                                    {
                                        log::error!("Failed to send data event: {e}");
                                        break;
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
                        break;
                    }
                    Some(Ok(_)) => {} // Skip ping/pong/binary/frame
                    Some(Err(e)) => {
                        log::error!("WebSocket error: {e}");
                        break;
                    }
                    None => {
                        log::info!("Replay stream ended");
                        break;
                    }
                }
            }

            heartbeat_token.cancel();
            connected.store(false, Ordering::Release);
        });

        self.tasks.push(handle);
        self.is_connected.store(true, Ordering::Release);
        log::info!("Connected: {}", self.client_id);

        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.is_disconnected() {
            return Ok(());
        }

        self.cancellation_token.cancel();
        self.cancellation_token = CancellationToken::new();

        let handles: Vec<_> = self.tasks.drain(..).collect();
        for handle in handles {
            if let Err(e) = handle.await {
                log::error!("Error joining replay task: {e}");
            }
        }

        self.is_connected.store(false, Ordering::Release);
        log::info!("Disconnected: {}", self.client_id);

        Ok(())
    }
}
