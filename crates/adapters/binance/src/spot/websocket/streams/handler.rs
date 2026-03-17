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
//! The handler is a stateless I/O boundary: it decodes raw SBE binary frames
//! into venue-specific event types and emits them on the output channel.
//! Domain conversion happens in the data client layer.

use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use ahash::AHashMap;
use nautilus_network::{
    RECONNECTED,
    websocket::{SubscriptionState, WebSocketClient},
};
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

pub use super::parse::{MarketDataMessage, decode_market_data};
use super::{
    messages::{
        BinanceSpotWsMessage, BinanceSpotWsStreamsCommand, BinanceWsErrorMsg,
        BinanceWsErrorResponse, BinanceWsResponse, BinanceWsSubscription,
    },
    parse::decode_market_data as decode_sbe,
};
use crate::common::consts::BINANCE_RATE_LIMIT_KEY_SUBSCRIPTION;

/// Binance Spot WebSocket feed handler.
///
/// Decodes raw SBE binary frames into venue-specific event types without
/// performing domain conversion. The data client layer owns instrument
/// lookups and Nautilus type construction.
pub(super) struct BinanceSpotWsFeedHandler {
    #[allow(dead_code)]
    signal: Arc<AtomicBool>,
    inner: Option<WebSocketClient>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<BinanceSpotWsStreamsCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    #[allow(dead_code)]
    out_tx: tokio::sync::mpsc::UnboundedSender<BinanceSpotWsMessage>,
    subscriptions: SubscriptionState,
    request_id_counter: Arc<AtomicU64>,
    pending_messages: VecDeque<BinanceSpotWsMessage>,
    pending_requests: AHashMap<u64, Vec<String>>,
}

impl BinanceSpotWsFeedHandler {
    /// Creates a new handler instance.
    pub(super) fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<BinanceSpotWsStreamsCommand>,
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
            request_id_counter,
            pending_messages: VecDeque::new(),
            pending_requests: AHashMap::new(),
        }
    }

    /// Returns the next message from the handler.
    ///
    /// Processes both commands and raw WebSocket messages.
    pub(super) async fn next(&mut self) -> Option<BinanceSpotWsMessage> {
        if let Some(message) = self.pending_messages.pop_front() {
            return Some(message);
        }

        loop {
            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    match cmd {
                        BinanceSpotWsStreamsCommand::SetClient(client) => {
                            log::debug!("Handler received WebSocket client");
                            self.inner = Some(client);
                        }
                        BinanceSpotWsStreamsCommand::Disconnect => {
                            log::debug!("Handler disconnecting WebSocket client");
                            self.inner = None;
                            return None;
                        }
                        BinanceSpotWsStreamsCommand::Subscribe { streams } => {
                            if let Err(e) = self.handle_subscribe(streams).await {
                                log::error!("Failed to handle subscribe command: {e}");
                            }
                        }
                        BinanceSpotWsStreamsCommand::Unsubscribe { streams } => {
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

    fn handle_binary_frame(&self, data: &[u8]) -> Vec<BinanceSpotWsMessage> {
        match decode_sbe(data) {
            Ok(MarketDataMessage::Trades(event)) => {
                vec![BinanceSpotWsMessage::Trades(event)]
            }
            Ok(MarketDataMessage::BestBidAsk(event)) => {
                vec![BinanceSpotWsMessage::BestBidAsk(event)]
            }
            Ok(MarketDataMessage::DepthSnapshot(event)) => {
                vec![BinanceSpotWsMessage::DepthSnapshot(event)]
            }
            Ok(MarketDataMessage::DepthDiff(event)) => {
                vec![BinanceSpotWsMessage::DepthDiff(event)]
            }
            Err(e) => {
                log::error!("SBE decode error: {e}");
                vec![BinanceSpotWsMessage::RawBinary(data.to_vec())]
            }
        }
    }

    fn handle_text_frame(&mut self, text: &str) -> Vec<BinanceSpotWsMessage> {
        if let Ok(response) = serde_json::from_str::<BinanceWsResponse>(text) {
            self.handle_subscription_response(&response);
            return vec![];
        }

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
            vec![BinanceSpotWsMessage::RawJson(value)]
        } else {
            log::warn!("Failed to parse JSON message: {text}");
            vec![]
        }
    }

    fn handle_subscription_response(&mut self, response: &BinanceWsResponse) {
        if let Some(streams) = self.pending_requests.remove(&response.id) {
            if response.result.is_none() {
                for stream in &streams {
                    self.subscriptions.confirm_subscribe(stream);
                }
                log::debug!("Subscription confirmed: streams={streams:?}");
            } else {
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

    async fn handle_subscribe(&mut self, streams: Vec<String>) -> anyhow::Result<()> {
        let request_id = self.request_id_counter.fetch_add(1, Ordering::SeqCst);
        let request = BinanceWsSubscription::subscribe(streams.clone(), request_id);
        let payload = serde_json::to_string(&request)?;

        self.pending_requests.insert(request_id, streams.clone());

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

    async fn handle_unsubscribe(&self, streams: Vec<String>) -> anyhow::Result<()> {
        let request_id = self.request_id_counter.fetch_add(1, Ordering::SeqCst);
        let request = BinanceWsSubscription::unsubscribe(streams.clone(), request_id);
        let payload = serde_json::to_string(&request)?;

        self.send_text(
            payload,
            Some(BINANCE_RATE_LIMIT_KEY_SUBSCRIPTION.as_slice()),
        )
        .await?;

        for stream in &streams {
            self.subscriptions.mark_unsubscribe(stream);
            self.subscriptions.confirm_unsubscribe(stream);
        }

        Ok(())
    }

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
