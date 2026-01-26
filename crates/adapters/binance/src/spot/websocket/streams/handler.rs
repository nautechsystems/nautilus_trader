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

//! Binance Spot WebSocket message handler.
//!
//! The handler runs in a dedicated Tokio task as the I/O boundary between the client
//! orchestrator and the network layer. It exclusively owns the `WebSocketClient` and
//! processes commands from the client via an unbounded channel.
//!
//! Key responsibilities:
//! - Command processing: Receives `HandlerCommand` from client, executes WebSocket operations.
//! - SBE binary decoding: Routes binary frames to appropriate SBE decoders.
//! - Message transformation: Parses raw venue messages into Nautilus domain events.
//! - Subscription tracking: Manages pending subscription state.

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use ahash::AHashMap;
use nautilus_model::{
    data::Data,
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{
    RECONNECTED,
    websocket::{SubscriptionState, WebSocketClient},
};
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

// Re-export for backwards compatibility
pub use super::parse::{MarketDataMessage, decode_market_data};
use super::{
    messages::{
        BinanceSpotWsMessage, BinanceWsErrorMsg, BinanceWsErrorResponse, BinanceWsResponse,
        BinanceWsSubscription, HandlerCommand, NautilusSpotDataWsMessage,
    },
    parse::{
        decode_market_data as decode_sbe, parse_bbo_event, parse_depth_diff, parse_depth_snapshot,
        parse_trades_event,
    },
};
use crate::common::consts::BINANCE_RATE_LIMIT_KEY_SUBSCRIPTION;

/// Binance Spot WebSocket feed handler.
///
/// Runs in a dedicated Tokio task, processing commands from the client
/// and transforming raw WebSocket messages into Nautilus domain events.
pub(super) struct BinanceSpotWsFeedHandler {
    #[allow(dead_code)] // Reserved for shutdown signal handling
    signal: Arc<AtomicBool>,
    inner: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    #[allow(dead_code)] // Reserved for async message emission
    out_tx: tokio::sync::mpsc::UnboundedSender<BinanceSpotWsMessage>,
    subscriptions: SubscriptionState,
    instruments_cache: AHashMap<Ustr, InstrumentAny>,
    request_id_counter: Arc<AtomicU64>,
    pending_messages: VecDeque<BinanceSpotWsMessage>,
    pending_requests: AHashMap<u64, Vec<String>>,
}

impl BinanceSpotWsFeedHandler {
    /// Creates a new handler instance.
    pub(super) fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<HandlerCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        out_tx: tokio::sync::mpsc::UnboundedSender<BinanceSpotWsMessage>,
        subscriptions: SubscriptionState,
        request_id_counter: Arc<AtomicU64>,
    ) -> Self {
        Self {
            signal,
            inner: None,
            cmd_rx,
            raw_rx,
            out_tx,
            subscriptions,
            instruments_cache: AHashMap::new(),
            request_id_counter,
            pending_messages: VecDeque::new(),
            pending_requests: AHashMap::new(),
        }
    }

    /// Main event loop - processes commands and raw messages.
    ///
    /// Returns `Some(message)` when there's output to emit, `None` when disconnected.
    pub(super) async fn next(&mut self) -> Option<BinanceSpotWsMessage> {
        // Return any pending messages first
        if let Some(message) = self.pending_messages.pop_front() {
            return Some(message);
        }

        loop {
            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        HandlerCommand::SetClient(client) => {
                            log::debug!("Handler received WebSocket client");
                            self.inner = Some(client);
                        }
                        HandlerCommand::Disconnect => {
                            log::debug!("Handler disconnecting WebSocket client");
                            self.inner = None;
                            return None;
                        }
                        HandlerCommand::InitializeInstruments(instruments) => {
                            for inst in instruments {
                                self.instruments_cache.insert(inst.symbol().inner(), inst);
                            }
                        }
                        HandlerCommand::UpdateInstrument(inst) => {
                            self.instruments_cache.insert(inst.symbol().inner(), inst);
                        }
                        HandlerCommand::Subscribe { streams } => {
                            if let Err(e) = self.handle_subscribe(streams).await {
                                log::error!("Failed to handle subscribe command: {e}");
                            }
                        }
                        HandlerCommand::Unsubscribe { streams } => {
                            if let Err(e) = self.handle_unsubscribe(streams).await {
                                log::error!("Failed to handle unsubscribe command: {e}");
                            }
                        }
                    }
                }
                Some(msg) = self.raw_rx.recv() => {
                    if let Message::Text(ref text) = msg
                        && text.as_str() == RECONNECTED
                    {
                        log::info!("Handler received reconnection signal");
                        return Some(BinanceSpotWsMessage::Reconnected);
                    }

                    let messages = self.handle_message(msg);
                    if !messages.is_empty() {
                        let mut iter = messages.into_iter();
                        let first = iter.next();
                        self.pending_messages.extend(iter);
                        if let Some(msg) = first {
                            return Some(msg);
                        }
                    }
                }
                else => {
                    return None;
                }
            }
        }
    }

    /// Handle incoming WebSocket message.
    fn handle_message(&mut self, msg: Message) -> Vec<BinanceSpotWsMessage> {
        match msg {
            Message::Binary(data) => self.handle_binary_frame(&data),
            Message::Text(text) => self.handle_text_frame(&text),
            Message::Close(_) => {
                log::debug!("Received close frame");
                vec![]
            }
            Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => vec![],
        }
    }

    /// Handle binary SBE frame.
    fn handle_binary_frame(&mut self, data: &[u8]) -> Vec<BinanceSpotWsMessage> {
        match decode_sbe(data) {
            Ok(MarketDataMessage::Trades(event)) => self.handle_trades_event(&event),
            Ok(MarketDataMessage::BestBidAsk(event)) => self.handle_bbo_event(&event),
            Ok(MarketDataMessage::DepthSnapshot(event)) => self.handle_depth_snapshot(&event),
            Ok(MarketDataMessage::DepthDiff(event)) => self.handle_depth_diff(&event),
            Err(e) => {
                log::error!("SBE decode error: {e}");
                vec![BinanceSpotWsMessage::Data(
                    NautilusSpotDataWsMessage::RawBinary(data.to_vec()),
                )]
            }
        }
    }

    /// Handle text JSON frame.
    fn handle_text_frame(&mut self, text: &str) -> Vec<BinanceSpotWsMessage> {
        if let Ok(response) = serde_json::from_str::<BinanceWsResponse>(text) {
            self.handle_subscription_response(response);
            return vec![];
        }

        // Error response includes id for request correlation
        if let Ok(error) = serde_json::from_str::<BinanceWsErrorResponse>(text) {
            if let Some(id) = error.id
                && let Some(streams) = self.pending_requests.remove(&id)
            {
                for stream in &streams {
                    self.subscriptions.mark_failure(stream);
                }
                log::warn!(
                    "Subscription request failed: id={id}, streams={streams:?}, code={}, msg={}",
                    error.code,
                    error.msg
                );
            }
            return vec![BinanceSpotWsMessage::Error(BinanceWsErrorMsg {
                code: error.code,
                msg: error.msg,
            })];
        }

        if let Ok(value) = serde_json::from_str(text) {
            vec![BinanceSpotWsMessage::Data(
                NautilusSpotDataWsMessage::RawJson(value),
            )]
        } else {
            log::warn!("Failed to parse JSON message: {text}");
            vec![]
        }
    }

    /// Handle subscription response.
    fn handle_subscription_response(&mut self, response: BinanceWsResponse) {
        if let Some(streams) = self.pending_requests.remove(&response.id) {
            if response.result.is_none() {
                // Success - confirm subscriptions
                for stream in &streams {
                    self.subscriptions.confirm_subscribe(stream);
                }
                log::debug!("Subscription confirmed: streams={streams:?}");
            } else {
                // Failure - mark streams as failed
                for stream in &streams {
                    self.subscriptions.mark_failure(stream);
                }
                log::warn!(
                    "Subscription failed: streams={streams:?}, result={:?}",
                    response.result
                );
            }
        } else {
            log::debug!("Received response for unknown request: id={}", response.id);
        }
    }

    /// Handle trades stream event.
    fn handle_trades_event(
        &self,
        event: &crate::common::sbe::stream::TradesStreamEvent,
    ) -> Vec<BinanceSpotWsMessage> {
        let symbol = Ustr::from(&event.symbol);

        let Some(instrument) = self.instruments_cache.get(&symbol) else {
            log::warn!("No instrument in cache for trades: symbol={}", event.symbol);
            return vec![];
        };

        let trades = parse_trades_event(event, instrument);
        if trades.is_empty() {
            vec![]
        } else {
            vec![BinanceSpotWsMessage::Data(NautilusSpotDataWsMessage::Data(
                trades,
            ))]
        }
    }

    /// Handle best bid/ask event.
    fn handle_bbo_event(
        &self,
        event: &crate::common::sbe::stream::BestBidAskStreamEvent,
    ) -> Vec<BinanceSpotWsMessage> {
        let symbol = Ustr::from(&event.symbol);

        let Some(instrument) = self.instruments_cache.get(&symbol) else {
            log::warn!("No instrument in cache for BBO: symbol={}", event.symbol);
            return vec![];
        };

        let quote = parse_bbo_event(event, instrument);
        vec![BinanceSpotWsMessage::Data(NautilusSpotDataWsMessage::Data(
            vec![Data::from(quote)],
        ))]
    }

    /// Handle depth snapshot event.
    fn handle_depth_snapshot(
        &self,
        event: &crate::common::sbe::stream::DepthSnapshotStreamEvent,
    ) -> Vec<BinanceSpotWsMessage> {
        let symbol = Ustr::from(&event.symbol);

        let Some(instrument) = self.instruments_cache.get(&symbol) else {
            log::warn!(
                "No instrument in cache for depth snapshot: symbol={}",
                event.symbol
            );
            return vec![];
        };

        match parse_depth_snapshot(event, instrument) {
            Some(deltas) => vec![BinanceSpotWsMessage::Data(
                NautilusSpotDataWsMessage::Deltas(deltas),
            )],
            None => vec![],
        }
    }

    /// Handle depth diff event.
    fn handle_depth_diff(
        &self,
        event: &crate::common::sbe::stream::DepthDiffStreamEvent,
    ) -> Vec<BinanceSpotWsMessage> {
        let symbol = Ustr::from(&event.symbol);

        let Some(instrument) = self.instruments_cache.get(&symbol) else {
            log::warn!(
                "No instrument in cache for depth diff: symbol={}",
                event.symbol
            );
            return vec![];
        };

        match parse_depth_diff(event, instrument) {
            Some(deltas) => vec![BinanceSpotWsMessage::Data(
                NautilusSpotDataWsMessage::Deltas(deltas),
            )],
            None => vec![],
        }
    }

    /// Handle subscribe command.
    async fn handle_subscribe(&mut self, streams: Vec<String>) -> anyhow::Result<()> {
        let request_id = self.request_id_counter.fetch_add(1, Ordering::SeqCst);
        let request = BinanceWsSubscription::subscribe(streams.clone(), request_id);
        let payload = serde_json::to_string(&request)?;

        // Track pending request for confirmation
        self.pending_requests.insert(request_id, streams.clone());

        // Mark streams as pending
        for stream in &streams {
            self.subscriptions.mark_subscribe(stream);
        }

        self.send_text(
            payload,
            Some(BINANCE_RATE_LIMIT_KEY_SUBSCRIPTION.as_slice()),
        )
        .await?;
        Ok(())
    }

    /// Handle unsubscribe command.
    async fn handle_unsubscribe(&mut self, streams: Vec<String>) -> anyhow::Result<()> {
        let request_id = self.request_id_counter.fetch_add(1, Ordering::SeqCst);
        let request = BinanceWsSubscription::unsubscribe(streams.clone(), request_id);
        let payload = serde_json::to_string(&request)?;

        self.send_text(
            payload,
            Some(BINANCE_RATE_LIMIT_KEY_SUBSCRIPTION.as_slice()),
        )
        .await?;

        // Immediately confirm unsubscribe (don't wait for response)
        // We don't track unsubscribe failures - the stream will simply stop
        for stream in &streams {
            self.subscriptions.mark_unsubscribe(stream);
            self.subscriptions.confirm_unsubscribe(stream);
        }

        Ok(())
    }

    /// Send text message via WebSocket.
    async fn send_text(
        &self,
        payload: String,
        rate_limit_keys: Option<&[Ustr]>,
    ) -> anyhow::Result<()> {
        let Some(client) = &self.inner else {
            anyhow::bail!("No active WebSocket client");
        };
        client
            .send_text(payload, rate_limit_keys)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send message: {e}"))?;
        Ok(())
    }
}
