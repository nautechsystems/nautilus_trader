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

//! Binance Futures WebSocket handler for JSON market data streams.

use std::{
    collections::HashMap,
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use nautilus_model::{
    data::Data,
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{
    RECONNECTED,
    websocket::{SubscriptionState, WebSocketClient},
};
use ustr::Ustr;

use super::{
    messages::{
        BinanceFuturesAggTradeMsg, BinanceFuturesBookTickerMsg, BinanceFuturesDepthUpdateMsg,
        BinanceFuturesHandlerCommand, BinanceFuturesTradeMsg, BinanceFuturesWsErrorMsg,
        BinanceFuturesWsErrorResponse, BinanceFuturesWsSubscribeRequest,
        BinanceFuturesWsSubscribeResponse, NautilusFuturesWsMessage,
    },
    parse::{
        extract_event_type, extract_symbol, parse_agg_trade, parse_book_ticker, parse_depth_update,
        parse_trade,
    },
};
use crate::common::enums::{BinanceWsEventType, BinanceWsMethod};

/// Handler for Binance Futures WebSocket JSON streams.
pub struct BinanceFuturesWsFeedHandler {
    #[allow(dead_code)] // Reserved for shutdown signal handling
    signal: Arc<AtomicBool>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<BinanceFuturesHandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    #[allow(dead_code)] // Reserved for async message emission
    out_tx: tokio::sync::mpsc::UnboundedSender<NautilusFuturesWsMessage>,
    client: Option<WebSocketClient>,
    instruments: HashMap<Ustr, InstrumentAny>,
    subscriptions_state: SubscriptionState,
    request_id_counter: Arc<AtomicU64>,
    pending_requests: HashMap<u64, Vec<String>>,
}

impl Debug for BinanceFuturesWsFeedHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BinanceFuturesWsFeedHandler")
            .field("instruments_count", &self.instruments.len())
            .field("pending_requests", &self.pending_requests.len())
            .finish_non_exhaustive()
    }
}

impl BinanceFuturesWsFeedHandler {
    /// Creates a new handler instance.
    pub fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<BinanceFuturesHandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
        out_tx: tokio::sync::mpsc::UnboundedSender<NautilusFuturesWsMessage>,
        subscriptions_state: SubscriptionState,
        request_id_counter: Arc<AtomicU64>,
    ) -> Self {
        Self {
            signal,
            cmd_rx,
            raw_rx,
            out_tx,
            client: None,
            instruments: HashMap::new(),
            subscriptions_state,
            request_id_counter,
            pending_requests: HashMap::new(),
        }
    }

    /// Returns the next message from the handler.
    ///
    /// Processes both commands and raw WebSocket messages.
    pub async fn next(&mut self) -> Option<NautilusFuturesWsMessage> {
        loop {
            if self.signal.load(Ordering::Relaxed) {
                return None;
            }

            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    self.handle_command(cmd).await;
                }
                Some(raw) = self.raw_rx.recv() => {
                    if let Some(msg) = self.handle_raw_message(raw).await {
                        return Some(msg);
                    }
                }
                else => {
                    return None;
                }
            }
        }
    }

    async fn handle_command(&mut self, cmd: BinanceFuturesHandlerCommand) {
        match cmd {
            BinanceFuturesHandlerCommand::SetClient(client) => {
                self.client = Some(client);
            }
            BinanceFuturesHandlerCommand::Disconnect => {
                if let Some(client) = &self.client {
                    let _ = client.disconnect().await;
                }
                self.client = None;
            }
            BinanceFuturesHandlerCommand::InitializeInstruments(instruments) => {
                for inst in instruments {
                    self.instruments.insert(inst.raw_symbol().inner(), inst);
                }
            }
            BinanceFuturesHandlerCommand::UpdateInstrument(instrument) => {
                self.instruments
                    .insert(instrument.raw_symbol().inner(), instrument);
            }
            BinanceFuturesHandlerCommand::Subscribe { streams } => {
                self.send_subscribe(streams).await;
            }
            BinanceFuturesHandlerCommand::Unsubscribe { streams } => {
                self.send_unsubscribe(streams).await;
            }
        }
    }

    async fn send_subscribe(&mut self, streams: Vec<String>) {
        let Some(client) = &self.client else {
            tracing::warn!("Cannot subscribe: no client connected");
            return;
        };

        let request_id = self.request_id_counter.fetch_add(1, Ordering::Relaxed);

        // Track pending request
        self.pending_requests.insert(request_id, streams.clone());

        // Mark streams as pending subscribe
        for stream in &streams {
            self.subscriptions_state.mark_subscribe(stream);
        }

        let request = BinanceFuturesWsSubscribeRequest {
            method: BinanceWsMethod::Subscribe,
            params: streams,
            id: request_id,
        };

        let json = match serde_json::to_string(&request) {
            Ok(j) => j,
            Err(e) => {
                tracing::error!(error = %e, "Failed to serialize subscribe request");
                return;
            }
        };

        if let Err(e) = client.send_text(json, None).await {
            tracing::error!(error = %e, "Failed to send subscribe request");
        }
    }

    async fn send_unsubscribe(&mut self, streams: Vec<String>) {
        let Some(client) = &self.client else {
            tracing::warn!("Cannot unsubscribe: no client connected");
            return;
        };

        let request_id = self.request_id_counter.fetch_add(1, Ordering::Relaxed);

        let request = BinanceFuturesWsSubscribeRequest {
            method: BinanceWsMethod::Unsubscribe,
            params: streams.clone(),
            id: request_id,
        };

        let json = match serde_json::to_string(&request) {
            Ok(j) => j,
            Err(e) => {
                tracing::error!(error = %e, "Failed to serialize unsubscribe request");
                return;
            }
        };

        if let Err(e) = client.send_text(json, None).await {
            tracing::error!(error = %e, "Failed to send unsubscribe request");
        }

        // Mark as unsubscribed
        for stream in &streams {
            self.subscriptions_state.confirm_unsubscribe(stream);
        }
    }

    async fn handle_raw_message(&mut self, raw: Vec<u8>) -> Option<NautilusFuturesWsMessage> {
        // Check for reconnection signal
        if let Ok(text) = std::str::from_utf8(&raw)
            && text == RECONNECTED
        {
            tracing::info!("WebSocket reconnected signal received");
            return Some(NautilusFuturesWsMessage::Reconnected);
        }

        // Parse JSON
        let json: serde_json::Value = match serde_json::from_slice(&raw) {
            Ok(j) => j,
            Err(e) => {
                tracing::warn!(error = %e, "Failed to parse JSON message");
                return None;
            }
        };

        // Check for subscription response
        if json.get("result").is_some() || json.get("id").is_some() {
            self.handle_subscription_response(&json);
            return None;
        }

        // Check for error response
        if let Some(code) = json.get("code")
            && let Some(code) = code.as_i64()
        {
            let msg = json
                .get("msg")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error")
                .to_string();
            return Some(NautilusFuturesWsMessage::Error(BinanceFuturesWsErrorMsg {
                code,
                msg,
            }));
        }

        // Handle stream data
        self.handle_stream_data(&json)
    }

    fn handle_subscription_response(&mut self, json: &serde_json::Value) {
        if let Ok(response) =
            serde_json::from_value::<BinanceFuturesWsSubscribeResponse>(json.clone())
        {
            if let Some(streams) = self.pending_requests.remove(&response.id) {
                if response.result.is_none() {
                    // Success - confirm subscriptions
                    for stream in &streams {
                        self.subscriptions_state.confirm_subscribe(stream);
                    }
                    tracing::debug!(streams = ?streams, "Subscription confirmed");
                } else {
                    // Failure - mark streams as failed
                    for stream in &streams {
                        self.subscriptions_state.mark_failure(stream);
                    }
                    tracing::warn!(streams = ?streams, result = ?response.result, "Subscription failed");
                }
            }
        } else if let Ok(error) =
            serde_json::from_value::<BinanceFuturesWsErrorResponse>(json.clone())
        {
            if let Some(id) = error.id
                && let Some(streams) = self.pending_requests.remove(&id)
            {
                for stream in &streams {
                    self.subscriptions_state.mark_failure(stream);
                }
            }
            tracing::warn!(code = error.code, msg = %error.msg, "WebSocket error response");
        }
    }

    fn handle_stream_data(&self, json: &serde_json::Value) -> Option<NautilusFuturesWsMessage> {
        let event_type = extract_event_type(json)?;
        let symbol = extract_symbol(json)?;

        // Look up instrument
        let Some(instrument) = self.instruments.get(&symbol) else {
            tracing::warn!(
                symbol = %symbol,
                event_type = ?event_type,
                "No instrument in cache, dropping message"
            );
            return None;
        };

        match event_type {
            BinanceWsEventType::AggTrade => {
                if let Ok(msg) = serde_json::from_value::<BinanceFuturesAggTradeMsg>(json.clone()) {
                    match parse_agg_trade(&msg, instrument) {
                        Ok(trade) => {
                            return Some(NautilusFuturesWsMessage::Data(vec![Data::Trade(trade)]));
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "Failed to parse aggregate trade");
                        }
                    }
                }
            }
            BinanceWsEventType::Trade => {
                if let Ok(msg) = serde_json::from_value::<BinanceFuturesTradeMsg>(json.clone()) {
                    match parse_trade(&msg, instrument) {
                        Ok(trade) => {
                            return Some(NautilusFuturesWsMessage::Data(vec![Data::Trade(trade)]));
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "Failed to parse trade");
                        }
                    }
                }
            }
            BinanceWsEventType::BookTicker => {
                if let Ok(msg) = serde_json::from_value::<BinanceFuturesBookTickerMsg>(json.clone())
                {
                    match parse_book_ticker(&msg, instrument) {
                        Ok(quote) => {
                            return Some(NautilusFuturesWsMessage::Data(vec![Data::Quote(quote)]));
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "Failed to parse book ticker");
                        }
                    }
                }
            }
            BinanceWsEventType::DepthUpdate => {
                if let Ok(msg) =
                    serde_json::from_value::<BinanceFuturesDepthUpdateMsg>(json.clone())
                {
                    match parse_depth_update(&msg, instrument) {
                        Ok(deltas) => {
                            return Some(NautilusFuturesWsMessage::Deltas(deltas));
                        }
                        Err(e) => {
                            tracing::warn!(error = %e, "Failed to parse depth update");
                        }
                    }
                }
            }
            BinanceWsEventType::MarkPriceUpdate
            | BinanceWsEventType::Kline
            | BinanceWsEventType::ForceOrder
            | BinanceWsEventType::Ticker24Hr
            | BinanceWsEventType::MiniTicker24Hr => {
                // Pass through as raw JSON for now
                return Some(NautilusFuturesWsMessage::RawJson(json.clone()));
            }
            BinanceWsEventType::Unknown => {
                tracing::debug!(event_type = ?json.get("e"), "Unknown event type");
            }
        }

        None
    }
}
