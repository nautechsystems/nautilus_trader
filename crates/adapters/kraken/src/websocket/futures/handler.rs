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

//! WebSocket message handler for Kraken Futures.

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::AHashMap;
use dashmap::DashSet;
use nautilus_core::{AtomicTime, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{
    data::{IndexPriceUpdate, MarkPriceUpdate},
    instruments::{Instrument, InstrumentAny},
    types::Price,
};
use nautilus_network::websocket::WebSocketClient;
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use super::messages::{FuturesWsMessage, KrakenFuturesTickerData};

/// Commands sent from the outer client to the inner message handler.
#[derive(Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "Commands are ephemeral and immediately consumed"
)]
pub enum HandlerCommand {
    /// Set the WebSocketClient for the handler to use.
    SetClient(WebSocketClient),
    /// Subscribe to a product's ticker feed.
    Subscribe(String),
    /// Unsubscribe from a product's ticker feed.
    Unsubscribe(String),
    /// Disconnect the WebSocket connection.
    Disconnect,
    /// Initialize the instruments cache with the given instruments.
    InitializeInstruments(Vec<InstrumentAny>),
    /// Update a single instrument in the cache.
    UpdateInstrument(InstrumentAny),
}

/// WebSocket message handler for Kraken Futures.
pub struct FuturesFeedHandler {
    clock: &'static AtomicTime,
    signal: Arc<AtomicBool>,
    client: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    subscriptions: Arc<DashSet<String>>,
    instruments_cache: AHashMap<Ustr, InstrumentAny>,
    pending_messages: VecDeque<FuturesWsMessage>,
}

impl FuturesFeedHandler {
    /// Creates a new [`FuturesFeedHandler`] instance.
    pub fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        subscriptions: Arc<DashSet<String>>,
    ) -> Self {
        Self {
            clock: get_atomic_clock_realtime(),
            signal,
            client: None,
            cmd_rx,
            raw_rx,
            subscriptions,
            instruments_cache: AHashMap::new(),
            pending_messages: VecDeque::new(),
        }
    }

    pub fn is_stopped(&self) -> bool {
        self.signal.load(Ordering::Relaxed)
    }

    fn get_instrument(&self, symbol: &Ustr) -> Option<&InstrumentAny> {
        self.instruments_cache.get(symbol)
    }

    /// Processes messages and commands, returning when stopped or stream ends.
    pub async fn next(&mut self) -> Option<FuturesWsMessage> {
        // First drain any pending messages from previous ticker processing
        if let Some(msg) = self.pending_messages.pop_front() {
            return Some(msg);
        }

        loop {
            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        HandlerCommand::SetClient(client) => {
                            tracing::debug!("WebSocketClient received by futures handler");
                            self.client = Some(client);
                        }
                        HandlerCommand::Subscribe(product_id) => {
                            if let Some(ref client) = self.client {
                                let msg = format!(
                                    r#"{{"event":"subscribe","feed":"ticker","product_ids":["{product_id}"]}}"#
                                );
                                if let Err(e) = client.send_text(msg, None).await {
                                    tracing::error!("Failed to send subscribe: {e}");
                                }
                            }
                        }
                        HandlerCommand::Unsubscribe(product_id) => {
                            if let Some(ref client) = self.client {
                                let msg = format!(
                                    r#"{{"event":"unsubscribe","feed":"ticker","product_ids":["{product_id}"]}}"#
                                );
                                if let Err(e) = client.send_text(msg, None).await {
                                    tracing::error!("Failed to send unsubscribe: {e}");
                                }
                            }
                        }
                        HandlerCommand::Disconnect => {
                            tracing::debug!("Disconnect command received");
                            if let Some(client) = self.client.take() {
                                client.disconnect().await;
                            }
                            return None;
                        }
                        HandlerCommand::InitializeInstruments(instruments) => {
                            for inst in instruments {
                                // Key by raw_symbol (e.g., "PI_XBTUSD") since that's what
                                // WebSocket messages use
                                self.instruments_cache.insert(inst.raw_symbol().inner(), inst);
                            }
                            tracing::debug!(
                                "Initialized {} instruments in futures handler cache",
                                self.instruments_cache.len()
                            );
                        }
                        HandlerCommand::UpdateInstrument(inst) => {
                            self.instruments_cache.insert(inst.raw_symbol().inner(), inst);
                        }
                    }
                    continue;
                }

                msg = self.raw_rx.recv() => {
                    let msg = match msg {
                        Some(msg) => msg,
                        None => {
                            tracing::debug!("WebSocket stream closed");
                            return None;
                        }
                    };

                    if self.signal.load(Ordering::Relaxed) {
                        tracing::debug!("Stop signal received");
                        return None;
                    }

                    let text = match msg {
                        Message::Text(text) => text.to_string(),
                        Message::Binary(data) => {
                            match std::str::from_utf8(&data) {
                                Ok(s) => s.to_string(),
                                Err(_) => continue,
                            }
                        }
                        Message::Ping(data) => {
                            tracing::trace!("Received ping frame with {} bytes", data.len());
                            if let Some(client) = &self.client
                                && let Err(e) = client.send_pong(data.to_vec()).await
                            {
                                tracing::warn!(error = %e, "Failed to send pong frame");
                            }
                            continue;
                        }
                        Message::Pong(_) => {
                            tracing::trace!("Received pong");
                            continue;
                        }
                        Message::Close(_) => {
                            tracing::info!("WebSocket connection closed");
                            return None;
                        }
                        Message::Frame(_) => {
                            tracing::trace!("Received raw frame");
                            continue;
                        }
                    };

                    let ts_init = self.clock.get_time_ns();
                    self.parse_message(&text, ts_init);

                    // Return first pending message if any were produced
                    if let Some(msg) = self.pending_messages.pop_front() {
                        return Some(msg);
                    }

                    continue;
                }
            }
        }
    }

    fn parse_message(&mut self, text: &str, ts_init: UnixNanos) {
        // Check if it's a ticker message
        if text.contains("\"feed\":\"ticker\"") && text.contains("\"product_id\"") {
            self.handle_ticker_message(text, ts_init);
            return;
        }

        if text.contains("\"event\":\"info\"") {
            tracing::debug!("Received info message: {text}");
        } else if text.contains("\"event\":\"subscribed\"") {
            tracing::debug!("Subscription confirmed: {text}");
        } else if text.contains("\"feed\":\"heartbeat\"") {
            tracing::trace!("Heartbeat received");
        }
    }

    fn handle_ticker_message(&mut self, text: &str, ts_init: UnixNanos) {
        let ticker = match serde_json::from_str::<KrakenFuturesTickerData>(text) {
            Ok(t) => t,
            Err(e) => {
                tracing::trace!("Failed to parse ticker: {e}");
                return;
            }
        };

        let Some(instrument) = self.get_instrument(&Ustr::from(ticker.product_id.as_str())) else {
            return;
        };

        let ts_event = ticker
            .time
            .map(|t| UnixNanos::from((t as u64) * 1_000_000))
            .unwrap_or(ts_init);

        let instrument_id = instrument.id();
        let price_precision = instrument.price_precision();

        // Enqueue mark price if present and subscribed
        if let Some(mark_price) = ticker.mark_price
            && self
                .subscriptions
                .contains(&format!("mark:{}", ticker.product_id))
        {
            let update = MarkPriceUpdate::new(
                instrument_id,
                Price::new(mark_price, price_precision),
                ts_event,
                ts_init,
            );
            self.pending_messages
                .push_back(FuturesWsMessage::MarkPrice(update));
        }

        // Enqueue index price if present and subscribed
        if let Some(index_price) = ticker.index
            && self
                .subscriptions
                .contains(&format!("index:{}", ticker.product_id))
        {
            let update = IndexPriceUpdate::new(
                instrument_id,
                Price::new(index_price, price_precision),
                ts_event,
                ts_init,
            );
            self.pending_messages
                .push_back(FuturesWsMessage::IndexPrice(update));
        }
    }
}
