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

//! WebSocket message handler for Kraken Spot v2.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use ahash::AHashMap;
use nautilus_core::{AtomicTime, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_model::{
    data::{Data, OrderBookDeltas},
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::websocket::WebSocketClient;
use serde_json::Value;
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use super::{
    enums::KrakenWsChannel,
    messages::{
        KrakenWsBookData, KrakenWsMessage, KrakenWsResponse, KrakenWsTickerData, KrakenWsTradeData,
        NautilusWsMessage,
    },
    parse::{parse_book_deltas, parse_quote_tick, parse_trade_tick},
};

/// Commands sent from the outer client to the inner message handler.
#[derive(Debug)]
#[allow(
    clippy::large_enum_variant,
    reason = "Commands are ephemeral and immediately consumed"
)]
pub enum SpotHandlerCommand {
    /// Set the WebSocketClient for the handler to use.
    SetClient(WebSocketClient),
    /// Disconnect the WebSocket connection.
    Disconnect,
    /// Send text payload to the WebSocket.
    SendText { payload: String },
    /// Initialize the instruments cache with the given instruments.
    InitializeInstruments(Vec<InstrumentAny>),
    /// Update a single instrument in the cache.
    UpdateInstrument(InstrumentAny),
}

/// WebSocket message handler for Kraken.
pub(super) struct SpotFeedHandler {
    clock: &'static AtomicTime,
    signal: Arc<AtomicBool>,
    client: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<SpotHandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    instruments_cache: AHashMap<Ustr, InstrumentAny>,
    book_sequence: u64,
}

impl SpotFeedHandler {
    /// Creates a new [`FeedHandler`] instance.
    pub(super) fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<SpotHandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    ) -> Self {
        Self {
            clock: get_atomic_clock_realtime(),
            signal,
            client: None,
            cmd_rx,
            raw_rx,
            instruments_cache: AHashMap::new(),
            book_sequence: 0,
        }
    }

    pub(super) fn is_stopped(&self) -> bool {
        self.signal.load(Ordering::Relaxed)
    }

    fn get_instrument(&self, symbol: &Ustr) -> Option<InstrumentAny> {
        self.instruments_cache.get(symbol).cloned()
    }

    /// Processes messages and commands, returning when stopped or stream ends.
    pub(super) async fn next(&mut self) -> Option<NautilusWsMessage> {
        loop {
            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        SpotHandlerCommand::SetClient(client) => {
                            tracing::debug!("WebSocketClient received by handler");
                            self.client = Some(client);
                        }
                        SpotHandlerCommand::Disconnect => {
                            tracing::debug!("Disconnect command received");
                            if let Some(client) = self.client.take() {
                                client.disconnect().await;
                            }
                        }
                        SpotHandlerCommand::SendText { payload } => {
                            if let Some(client) = &self.client
                                && let Err(e) = client.send_text(payload.clone(), None).await
                            {
                                tracing::error!(error = %e, "Failed to send text");
                            }
                        }
                        SpotHandlerCommand::InitializeInstruments(instruments) => {
                            for inst in instruments {
                                // Cache by symbol (ISO 4217-A3 format like "ETH/USD")
                                // which matches what v2 WebSocket messages use
                                self.instruments_cache.insert(inst.symbol().inner(), inst);
                            }
                        }
                        SpotHandlerCommand::UpdateInstrument(inst) => {
                            self.instruments_cache.insert(inst.symbol().inner(), inst);
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

                    if let Message::Ping(data) = &msg {
                        tracing::trace!("Received ping frame with {} bytes", data.len());
                        if let Some(client) = &self.client
                            && let Err(e) = client.send_pong(data.to_vec()).await
                        {
                            tracing::warn!(error = %e, "Failed to send pong frame");
                        }
                        continue;
                    }

                    if self.signal.load(Ordering::Relaxed) {
                        tracing::debug!("Stop signal received");
                        return None;
                    }

                    let text = match msg {
                        Message::Text(text) => text.to_string(),
                        Message::Binary(data) => {
                            match String::from_utf8(data.to_vec()) {
                                Ok(text) => text,
                                Err(e) => {
                                    tracing::warn!("Failed to decode binary message: {e}");
                                    continue;
                                }
                            }
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
                        _ => continue,
                    };

                    let ts_init = self.clock.get_time_ns();

                    if let Some(nautilus_msg) = self.parse_message(&text, ts_init) {
                        return Some(nautilus_msg);
                    }

                    continue;
                }
            }
        }
    }

    fn parse_message(&mut self, text: &str, ts_init: UnixNanos) -> Option<NautilusWsMessage> {
        // Try to parse as a data message first
        if let Ok(msg) = serde_json::from_str::<KrakenWsMessage>(text) {
            return self.handle_data_message(msg, ts_init);
        }

        // Check for control messages (heartbeat, status, subscription responses)
        if let Ok(value) = serde_json::from_str::<Value>(text) {
            if value.get("channel").and_then(|v| v.as_str()) == Some("heartbeat") {
                tracing::trace!("Received heartbeat");
                return None;
            }

            if value.get("channel").and_then(|v| v.as_str()) == Some("status") {
                tracing::debug!("Received status message");
                return None;
            }

            if value.get("method").is_some() {
                if let Ok(response) = serde_json::from_value::<KrakenWsResponse>(value) {
                    match response {
                        KrakenWsResponse::Subscribe(sub) => {
                            if sub.success {
                                if let Some(result) = &sub.result {
                                    tracing::debug!(
                                        channel = ?result.channel,
                                        req_id = ?sub.req_id,
                                        "Subscription confirmed"
                                    );
                                } else {
                                    tracing::debug!(req_id = ?sub.req_id, "Subscription confirmed");
                                }
                            } else {
                                tracing::warn!(
                                    error = ?sub.error,
                                    req_id = ?sub.req_id,
                                    "Subscription failed"
                                );
                            }
                        }
                        KrakenWsResponse::Unsubscribe(unsub) => {
                            if unsub.success {
                                tracing::debug!(req_id = ?unsub.req_id, "Unsubscription confirmed");
                            } else {
                                tracing::warn!(
                                    error = ?unsub.error,
                                    req_id = ?unsub.req_id,
                                    "Unsubscription failed"
                                );
                            }
                        }
                        KrakenWsResponse::Pong(pong) => {
                            tracing::trace!(req_id = ?pong.req_id, "Received pong");
                        }
                        KrakenWsResponse::Other => {
                            tracing::debug!("Received unknown subscription response");
                        }
                    }
                } else {
                    tracing::debug!("Received subscription response (failed to parse details)");
                }
                return None;
            }
        }

        tracing::warn!("Failed to parse message: {text}");
        None
    }

    fn handle_data_message(
        &mut self,
        msg: KrakenWsMessage,
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        match msg.channel {
            KrakenWsChannel::Book => self.handle_book_message(msg, ts_init),
            KrakenWsChannel::Ticker => self.handle_ticker_message(msg, ts_init),
            KrakenWsChannel::Trade => self.handle_trade_message(msg, ts_init),
            KrakenWsChannel::Ohlc => self.handle_ohlc_message(msg, ts_init),
            _ => {
                tracing::warn!("Unhandled channel: {:?}", msg.channel);
                None
            }
        }
    }

    fn handle_book_message(
        &mut self,
        msg: KrakenWsMessage,
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        let mut all_deltas = Vec::new();
        let mut instrument_id = None;

        for data in msg.data {
            match serde_json::from_value::<KrakenWsBookData>(data) {
                Ok(book_data) => {
                    let instrument = self.get_instrument(&book_data.symbol)?;
                    instrument_id = Some(instrument.id());

                    match parse_book_deltas(&book_data, &instrument, self.book_sequence, ts_init) {
                        Ok(mut deltas) => {
                            self.book_sequence += deltas.len() as u64;
                            all_deltas.append(&mut deltas);
                        }
                        Err(e) => {
                            tracing::error!("Failed to parse book deltas: {e}");
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to deserialize book data: {e}");
                }
            }
        }

        if all_deltas.is_empty() {
            None
        } else {
            let deltas = OrderBookDeltas::new(instrument_id?, all_deltas);
            Some(NautilusWsMessage::Deltas(deltas))
        }
    }

    fn handle_ticker_message(
        &self,
        msg: KrakenWsMessage,
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        let mut quotes = Vec::new();

        for data in msg.data {
            match serde_json::from_value::<KrakenWsTickerData>(data) {
                Ok(ticker_data) => {
                    let instrument = self.get_instrument(&ticker_data.symbol)?;

                    match parse_quote_tick(&ticker_data, &instrument, ts_init) {
                        Ok(quote) => quotes.push(Data::Quote(quote)),
                        Err(e) => {
                            tracing::error!("Failed to parse quote tick: {e}");
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to deserialize ticker data: {e}");
                }
            }
        }

        if quotes.is_empty() {
            None
        } else {
            Some(NautilusWsMessage::Data(quotes))
        }
    }

    fn handle_trade_message(
        &self,
        msg: KrakenWsMessage,
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        let mut trades = Vec::new();

        for data in msg.data {
            match serde_json::from_value::<KrakenWsTradeData>(data) {
                Ok(trade_data) => {
                    let instrument = self.get_instrument(&trade_data.symbol)?;

                    match parse_trade_tick(&trade_data, &instrument, ts_init) {
                        Ok(trade) => trades.push(Data::Trade(trade)),
                        Err(e) => {
                            tracing::error!("Failed to parse trade tick: {e}");
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to deserialize trade data: {e}");
                }
            }
        }

        if trades.is_empty() {
            None
        } else {
            Some(NautilusWsMessage::Data(trades))
        }
    }

    fn handle_ohlc_message(
        &self,
        _msg: KrakenWsMessage,
        _ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        // OHLC/Bar parsing not yet implemented in parse.rs
        tracing::debug!("OHLC message received but parsing not yet implemented");
        None
    }
}
