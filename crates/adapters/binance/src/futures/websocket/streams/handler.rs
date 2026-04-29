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

//! Binance Futures WebSocket handler for JSON streams.
//!
//! The handler is a stateless I/O boundary: it deserializes raw JSON into
//! venue-specific types and emits them on the output channel. Domain conversion
//! happens in the data and execution client layers.

use std::{
    fmt::Debug,
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

use super::{
    messages::{
        BinanceFuturesAccountConfigMsg, BinanceFuturesAccountUpdateMsg, BinanceFuturesAggTradeMsg,
        BinanceFuturesAlgoUpdateMsg, BinanceFuturesBookTickerMsg, BinanceFuturesDepthUpdateMsg,
        BinanceFuturesKlineMsg, BinanceFuturesLiquidationMsg, BinanceFuturesListenKeyExpiredMsg,
        BinanceFuturesMarginCallMsg, BinanceFuturesMarkPriceMsg, BinanceFuturesOrderUpdateMsg,
        BinanceFuturesTickerMsg, BinanceFuturesTradeLiteMsg, BinanceFuturesTradeMsg,
        BinanceFuturesWsErrorMsg, BinanceFuturesWsErrorResponse, BinanceFuturesWsStreamsCommand,
        BinanceFuturesWsStreamsMessage, BinanceFuturesWsSubscribeRequest,
        BinanceFuturesWsSubscribeResponse,
    },
    parse_data::extract_event_type,
};
use crate::common::{
    consts::BINANCE_RATE_LIMIT_KEY_SUBSCRIPTION,
    enums::{BinanceWsEventType, BinanceWsMethod},
};

/// Handler for Binance Futures WebSocket JSON streams.
///
/// Deserializes raw JSON into venue-specific types without performing
/// domain conversion. The data and execution client layers own instrument
/// lookups and Nautilus type construction.
pub struct BinanceFuturesDataWsFeedHandler {
    #[allow(dead_code)]
    signal: Arc<AtomicBool>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<BinanceFuturesWsStreamsCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    #[allow(dead_code)]
    out_tx: tokio::sync::mpsc::UnboundedSender<BinanceFuturesWsStreamsMessage>,
    inner: Option<WebSocketClient>,
    subscriptions_state: SubscriptionState,
    request_id_counter: Arc<AtomicU64>,
    pending_requests: AHashMap<u64, Vec<String>>,
}

impl Debug for BinanceFuturesDataWsFeedHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BinanceFuturesDataWsFeedHandler))
            .field("pending_requests", &self.pending_requests.len())
            .finish_non_exhaustive()
    }
}

impl BinanceFuturesDataWsFeedHandler {
    /// Creates a new handler instance.
    pub fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<BinanceFuturesWsStreamsCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
        out_tx: tokio::sync::mpsc::UnboundedSender<BinanceFuturesWsStreamsMessage>,
        subscriptions_state: SubscriptionState,
        request_id_counter: Arc<AtomicU64>,
    ) -> Self {
        Self {
            signal,
            cmd_rx,
            raw_rx,
            out_tx,
            inner: None,
            subscriptions_state,
            request_id_counter,
            pending_requests: AHashMap::new(),
        }
    }

    /// Returns the next message from the handler.
    ///
    /// Processes both commands and raw WebSocket messages.
    pub async fn next(&mut self) -> Option<BinanceFuturesWsStreamsMessage> {
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

    async fn handle_command(&mut self, cmd: BinanceFuturesWsStreamsCommand) {
        match cmd {
            BinanceFuturesWsStreamsCommand::SetClient(client) => {
                self.inner = Some(client);
            }
            BinanceFuturesWsStreamsCommand::Disconnect => {
                if let Some(client) = &self.inner {
                    let () = client.disconnect().await;
                }
                self.inner = None;
            }
            BinanceFuturesWsStreamsCommand::Subscribe { streams } => {
                self.send_subscribe(streams).await;
            }
            BinanceFuturesWsStreamsCommand::Unsubscribe { streams } => {
                self.send_unsubscribe(streams).await;
            }
        }
    }

    async fn send_subscribe(&mut self, streams: Vec<String>) {
        let Some(client) = &self.inner else {
            log::warn!("Cannot subscribe: no client connected");
            return;
        };

        let request_id = self.request_id_counter.fetch_add(1, Ordering::Relaxed);

        self.pending_requests.insert(request_id, streams.clone());

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
                log::error!("Failed to serialize subscribe request: {e}");
                return;
            }
        };

        if let Err(e) = client
            .send_text(json, Some(BINANCE_RATE_LIMIT_KEY_SUBSCRIPTION.as_slice()))
            .await
        {
            log::error!("Failed to send subscribe request: {e}");
        }
    }

    async fn send_unsubscribe(&self, streams: Vec<String>) {
        let Some(client) = &self.inner else {
            log::warn!("Cannot unsubscribe: no client connected");
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
                log::error!("Failed to serialize unsubscribe request: {e}");
                return;
            }
        };

        if let Err(e) = client
            .send_text(json, Some(BINANCE_RATE_LIMIT_KEY_SUBSCRIPTION.as_slice()))
            .await
        {
            log::error!("Failed to send unsubscribe request: {e}");
        }

        for stream in &streams {
            self.subscriptions_state.mark_unsubscribe(stream);
            self.subscriptions_state.confirm_unsubscribe(stream);
        }
    }

    async fn handle_raw_message(&mut self, raw: Vec<u8>) -> Option<BinanceFuturesWsStreamsMessage> {
        if let Ok(text) = std::str::from_utf8(&raw)
            && text == RECONNECTED
        {
            log::info!("WebSocket reconnected signal received");
            return Some(BinanceFuturesWsStreamsMessage::Reconnected);
        }

        let json: serde_json::Value = match serde_json::from_slice(&raw) {
            Ok(j) => j,
            Err(e) => {
                log::warn!("Failed to parse JSON message: {e}");
                return None;
            }
        };

        if json.get("result").is_some() || json.get("id").is_some() {
            self.handle_subscription_response(&json);
            return None;
        }

        if let Some(code) = json.get("code")
            && let Some(code) = code.as_i64()
        {
            let msg = json
                .get("msg")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error")
                .to_string();
            return Some(BinanceFuturesWsStreamsMessage::Error(
                BinanceFuturesWsErrorMsg { code, msg },
            ));
        }

        self.handle_stream_data(&json)
    }

    fn handle_subscription_response(&mut self, json: &serde_json::Value) {
        if let Ok(response) =
            serde_json::from_value::<BinanceFuturesWsSubscribeResponse>(json.clone())
        {
            if let Some(streams) = self.pending_requests.remove(&response.id) {
                if response.result.is_none() {
                    for stream in &streams {
                        self.subscriptions_state.confirm_subscribe(stream);
                    }
                    log::debug!("Subscription confirmed: streams={streams:?}");
                } else {
                    for stream in &streams {
                        self.subscriptions_state.mark_failure(stream);
                    }
                    log::warn!(
                        "Subscription failed: streams={streams:?}, result={:?}",
                        response.result
                    );
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
            log::warn!(
                "WebSocket error response: code={}, msg={}",
                error.code,
                error.msg
            );
        }
    }

    fn handle_stream_data(
        &self,
        json: &serde_json::Value,
    ) -> Option<BinanceFuturesWsStreamsMessage> {
        let event_type = extract_event_type(json)?;

        match event_type {
            BinanceWsEventType::AggTrade => {
                serde_json::from_value::<BinanceFuturesAggTradeMsg>(json.clone())
                    .map(BinanceFuturesWsStreamsMessage::AggTrade)
                    .map_err(|e| log::warn!("Failed to parse aggregate trade: {e}"))
                    .ok()
            }
            BinanceWsEventType::Trade => {
                serde_json::from_value::<BinanceFuturesTradeMsg>(json.clone())
                    .map(BinanceFuturesWsStreamsMessage::Trade)
                    .map_err(|e| log::warn!("Failed to parse trade: {e}"))
                    .ok()
            }
            BinanceWsEventType::BookTicker => {
                serde_json::from_value::<BinanceFuturesBookTickerMsg>(json.clone())
                    .map(BinanceFuturesWsStreamsMessage::BookTicker)
                    .map_err(|e| log::warn!("Failed to parse book ticker: {e}"))
                    .ok()
            }
            BinanceWsEventType::DepthUpdate => {
                serde_json::from_value::<BinanceFuturesDepthUpdateMsg>(json.clone())
                    .map(BinanceFuturesWsStreamsMessage::DepthUpdate)
                    .map_err(|e| log::warn!("Failed to parse depth update: {e}"))
                    .ok()
            }
            BinanceWsEventType::MarkPriceUpdate => {
                serde_json::from_value::<BinanceFuturesMarkPriceMsg>(json.clone())
                    .map(BinanceFuturesWsStreamsMessage::MarkPrice)
                    .map_err(|e| log::warn!("Failed to parse mark price: {e}"))
                    .ok()
            }
            BinanceWsEventType::Kline => {
                serde_json::from_value::<BinanceFuturesKlineMsg>(json.clone())
                    .map(BinanceFuturesWsStreamsMessage::Kline)
                    .map_err(|e| log::warn!("Failed to parse kline: {e}"))
                    .ok()
            }
            BinanceWsEventType::ForceOrder => {
                serde_json::from_value::<BinanceFuturesLiquidationMsg>(json.clone())
                    .map(BinanceFuturesWsStreamsMessage::ForceOrder)
                    .map_err(|e| log::warn!("Failed to parse force order: {e}"))
                    .ok()
            }
            BinanceWsEventType::Ticker24Hr => {
                serde_json::from_value::<BinanceFuturesTickerMsg>(json.clone())
                    .map(BinanceFuturesWsStreamsMessage::Ticker)
                    .map_err(|e| log::warn!("Failed to parse ticker: {e}"))
                    .ok()
            }
            BinanceWsEventType::MiniTicker24Hr => {
                log::debug!("Mini ticker not yet supported, skipping");
                None
            }
            BinanceWsEventType::AccountUpdate => {
                serde_json::from_value::<BinanceFuturesAccountUpdateMsg>(json.clone())
                    .map(|msg| {
                        log::debug!(
                            "Account update: reason={:?}, balances={}, positions={}",
                            msg.account.reason,
                            msg.account.balances.len(),
                            msg.account.positions.len()
                        );
                        BinanceFuturesWsStreamsMessage::AccountUpdate(msg)
                    })
                    .map_err(|e| log::warn!("Failed to parse account update: {e}"))
                    .ok()
            }
            BinanceWsEventType::OrderTradeUpdate => {
                serde_json::from_value::<BinanceFuturesOrderUpdateMsg>(json.clone())
                    .map(|msg| {
                        log::debug!(
                            "Order update: symbol={}, order_id={}, exec={:?}, status={:?}",
                            msg.order.symbol,
                            msg.order.order_id,
                            msg.order.execution_type,
                            msg.order.order_status
                        );
                        BinanceFuturesWsStreamsMessage::OrderUpdate(Box::new(msg))
                    })
                    .map_err(|e| log::warn!("Failed to parse order update: {e}"))
                    .ok()
            }
            BinanceWsEventType::TradeLite => {
                serde_json::from_value::<BinanceFuturesTradeLiteMsg>(json.clone())
                    .map(|msg| {
                        log::debug!(
                            "Trade lite: symbol={}, order_id={}, trade_id={}",
                            msg.symbol,
                            msg.order_id,
                            msg.trade_id
                        );
                        BinanceFuturesWsStreamsMessage::TradeLite(Box::new(msg))
                    })
                    .map_err(|e| log::warn!("Failed to parse trade lite: {e}"))
                    .ok()
            }
            BinanceWsEventType::AlgoUpdate => {
                serde_json::from_value::<BinanceFuturesAlgoUpdateMsg>(json.clone())
                    .map(|msg| {
                        log::debug!(
                            "Algo order update: symbol={}, algo_id={}, status={:?}",
                            msg.algo_order.symbol,
                            msg.algo_order.algo_id,
                            msg.algo_order.algo_status
                        );
                        BinanceFuturesWsStreamsMessage::AlgoUpdate(Box::new(msg))
                    })
                    .map_err(|e| log::warn!("Failed to parse algo order update: {e}"))
                    .ok()
            }
            BinanceWsEventType::MarginCall => {
                serde_json::from_value::<BinanceFuturesMarginCallMsg>(json.clone())
                    .map(|msg| {
                        log::warn!(
                            "Margin call: cross_wallet_balance={}, positions_at_risk={}",
                            msg.cross_wallet_balance,
                            msg.positions.len()
                        );
                        BinanceFuturesWsStreamsMessage::MarginCall(msg)
                    })
                    .map_err(|e| log::warn!("Failed to parse margin call: {e}"))
                    .ok()
            }
            BinanceWsEventType::AccountConfigUpdate => {
                serde_json::from_value::<BinanceFuturesAccountConfigMsg>(json.clone())
                    .map(|msg| {
                        if let Some(ref lc) = msg.leverage_config {
                            log::debug!(
                                "Account config update: symbol={}, leverage={}",
                                lc.symbol,
                                lc.leverage
                            );
                        }
                        BinanceFuturesWsStreamsMessage::AccountConfigUpdate(msg)
                    })
                    .map_err(|e| log::warn!("Failed to parse account config update: {e}"))
                    .ok()
            }
            BinanceWsEventType::ListenKeyExpired => {
                if let Ok(msg) =
                    serde_json::from_value::<BinanceFuturesListenKeyExpiredMsg>(json.clone())
                {
                    log::warn!("Listen key expired at {}", msg.event_time);
                }
                Some(BinanceFuturesWsStreamsMessage::ListenKeyExpired)
            }
            BinanceWsEventType::Unknown => {
                log::warn!("Unknown event type in message: {json}");
                None
            }
        }
    }
}
