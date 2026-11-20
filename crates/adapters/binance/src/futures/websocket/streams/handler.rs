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
use serde::Deserialize;
use tokio_tungstenite::tungstenite::Message;

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
    websocket::{
        PendingSubscriptionRequest, PendingSubscriptionRequests, json_field_present,
        reset_requests_after_reconnect,
    },
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
    raw_rx: RawFrames,
    #[allow(dead_code)]
    out_tx: tokio::sync::mpsc::UnboundedSender<BinanceFuturesWsStreamsMessage>,
    inner: Option<WebSocketClient>,
    subscriptions_state: SubscriptionState,
    request_id_counter: Arc<AtomicU64>,
    pending_requests: PendingSubscriptionRequests,
    unsubscribe_correlations: AHashMap<u64, (u64, Vec<String>)>,
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
        Self::from_frames(
            signal,
            cmd_rx,
            RawFrames::Payloads(raw_rx),
            out_tx,
            subscriptions_state,
            request_id_counter,
        )
    }

    pub(super) fn from_frames(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<BinanceFuturesWsStreamsCommand>,
        raw_rx: RawFrames,
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
            pending_requests: PendingSubscriptionRequests::default(),
            unsubscribe_correlations: AHashMap::new(),
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
                    let data = match &raw {
                        Message::Binary(data) => data.as_ref(),
                        Message::Text(text) => text.as_bytes(),
                        Message::Close(_) => return None,
                        Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
                    };

                    if let Some(msg) = self.handle_raw_message(data) {
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
            BinanceFuturesWsStreamsCommand::Unsubscribe {
                streams,
                correlation,
            } => {
                self.send_unsubscribe(streams, correlation).await;
            }
        }
    }

    async fn send_subscribe(&mut self, streams: Vec<String>) {
        for stream in &streams {
            self.subscriptions_state.mark_subscribe(stream);
        }

        let Some(client) = &self.inner else {
            log::warn!("Cannot subscribe: no client connected");
            return;
        };

        let request_id = self.request_id_counter.fetch_add(1, Ordering::Relaxed);

        let request = BinanceFuturesWsSubscribeRequest {
            method: BinanceWsMethod::Subscribe,
            params: streams.clone(),
            id: request_id,
        };

        let json = match serde_json::to_string(&request) {
            Ok(j) => j,
            Err(e) => {
                log::error!("Failed to serialize subscribe request: {e}");
                return;
            }
        };

        self.pending_requests
            .insert(request_id, PendingSubscriptionRequest::subscribe(streams));

        if let Err(e) = client
            .send_text(json, Some(BINANCE_RATE_LIMIT_KEY_SUBSCRIPTION.as_slice()))
            .await
        {
            if let Some(request) = self.pending_requests.take(request_id) {
                request.mark_failure(&self.subscriptions_state);
            }
            log::error!("Failed to send subscribe request: {e}");
        }
    }

    async fn send_unsubscribe(&mut self, streams: Vec<String>, correlation: Option<u64>) {
        for stream in &streams {
            self.subscriptions_state.mark_unsubscribe(stream);
        }

        let request_id = self.request_id_counter.fetch_add(1, Ordering::Relaxed);

        // Record the correlation even when the wire send cannot happen: the drain it
        // protects must stay resolvable, and only a reconnect can abandon it
        if let Some(correlation) = correlation {
            self.unsubscribe_correlations
                .insert(request_id, (correlation, streams.clone()));
        }

        let Some(client) = &self.inner else {
            log::warn!("Cannot unsubscribe: no client connected");
            return;
        };

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

        self.pending_requests
            .insert(request_id, PendingSubscriptionRequest::unsubscribe(streams));

        if let Err(e) = client
            .send_text(json, Some(BINANCE_RATE_LIMIT_KEY_SUBSCRIPTION.as_slice()))
            .await
        {
            self.pending_requests.take(request_id);
            log::error!("Failed to send unsubscribe request: {e}");
        }
    }

    fn handle_raw_message(&mut self, raw: &[u8]) -> Option<BinanceFuturesWsStreamsMessage> {
        if raw == RECONNECTED.as_bytes() {
            let abandoned = self
                .unsubscribe_correlations
                .values()
                .map(|(correlation, _)| *correlation)
                .collect();
            self.unsubscribe_correlations.clear();
            reset_requests_after_reconnect(&mut self.pending_requests, &self.subscriptions_state);
            log::debug!("WebSocket reconnected signal received");
            return Some(BinanceFuturesWsStreamsMessage::Reconnected(abandoned));
        }

        if let Some(message) = decode_market_data(raw) {
            return Some(message);
        }

        let json: serde_json::Value = match serde_json::from_slice(raw) {
            Ok(j) => j,
            Err(e) => {
                log::warn!("Failed to parse JSON message: {e}");
                return None;
            }
        };

        if json.get("result").is_some() || json.get("id").is_some() {
            return self.handle_subscription_response(&json);
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

        self.handle_stream_data(json)
    }

    fn handle_subscription_response(
        &mut self,
        json: &serde_json::Value,
    ) -> Option<BinanceFuturesWsStreamsMessage> {
        if let Ok(error) = serde_json::from_value::<BinanceFuturesWsErrorResponse>(json.clone()) {
            if let Some(id) = error.id
                && let Some(request) = self.pending_requests.take(id)
            {
                // A rejected unsubscribe yields no confirmation; the drain fails
                // closed, and the ledger entry survives so a reconnect can abandon it
                request.mark_failure(&self.subscriptions_state);
            }
            log::warn!(
                "WebSocket error response: code={}, msg={}",
                error.code,
                error.msg
            );
        } else if let Ok(response) =
            serde_json::from_value::<BinanceFuturesWsSubscribeResponse>(json.clone())
        {
            if response.result.is_none() {
                if let Some(request) = self.pending_requests.take(response.id) {
                    request.confirm(&self.subscriptions_state);
                    log::debug!("Subscription request confirmed: request={request:?}");
                }

                // The venue confirmed the unsubscribe even when a superseding
                // resubscribe dropped the local request, so resolve its drain from
                // the ledger regardless
                if let Some((correlation, streams)) =
                    self.unsubscribe_correlations.remove(&response.id)
                {
                    return Some(BinanceFuturesWsStreamsMessage::Unsubscribed {
                        streams,
                        correlation: Some(correlation),
                    });
                }
            } else if let Some(request) = self.pending_requests.take(response.id) {
                // A rejected unsubscribe yields no confirmation; keep the ledger
                // entry so a reconnect can still abandon the drain
                request.mark_failure(&self.subscriptions_state);
                log::warn!(
                    "Subscription request failed: request={request:?}, result={:?}",
                    response.result
                );
            }
        }

        None
    }

    fn handle_stream_data(
        &self,
        json: serde_json::Value,
    ) -> Option<BinanceFuturesWsStreamsMessage> {
        let event_type = extract_event_type(&json)?;
        match event_type {
            BinanceWsEventType::AggTrade => {
                serde_json::from_value::<BinanceFuturesAggTradeMsg>(json)
                    .map(BinanceFuturesWsStreamsMessage::AggTrade)
                    .map_err(|e| log::warn!("Failed to parse aggregate trade: {e}"))
                    .ok()
            }
            BinanceWsEventType::Trade => serde_json::from_value::<BinanceFuturesTradeMsg>(json)
                .map(BinanceFuturesWsStreamsMessage::Trade)
                .map_err(|e| log::warn!("Failed to parse trade: {e}"))
                .ok(),
            BinanceWsEventType::BookTicker => {
                serde_json::from_value::<BinanceFuturesBookTickerMsg>(json)
                    .map(BinanceFuturesWsStreamsMessage::BookTicker)
                    .map_err(|e| log::warn!("Failed to parse book ticker: {e}"))
                    .ok()
            }
            BinanceWsEventType::DepthUpdate => {
                serde_json::from_value::<BinanceFuturesDepthUpdateMsg>(json)
                    .map(BinanceFuturesWsStreamsMessage::DepthUpdate)
                    .map_err(|e| log::warn!("Failed to parse depth update: {e}"))
                    .ok()
            }
            BinanceWsEventType::MarkPriceUpdate => {
                serde_json::from_value::<BinanceFuturesMarkPriceMsg>(json)
                    .map(BinanceFuturesWsStreamsMessage::MarkPrice)
                    .map_err(|e| log::warn!("Failed to parse mark price: {e}"))
                    .ok()
            }
            BinanceWsEventType::Kline => serde_json::from_value::<BinanceFuturesKlineMsg>(json)
                .map(BinanceFuturesWsStreamsMessage::Kline)
                .map_err(|e| log::warn!("Failed to parse kline: {e}"))
                .ok(),
            BinanceWsEventType::ForceOrder => {
                serde_json::from_value::<BinanceFuturesLiquidationMsg>(json)
                    .map(BinanceFuturesWsStreamsMessage::ForceOrder)
                    .map_err(|e| log::warn!("Failed to parse force order: {e}"))
                    .ok()
            }
            BinanceWsEventType::Ticker24Hr => {
                serde_json::from_value::<BinanceFuturesTickerMsg>(json)
                    .map(BinanceFuturesWsStreamsMessage::Ticker)
                    .map_err(|e| log::warn!("Failed to parse ticker: {e}"))
                    .ok()
            }
            BinanceWsEventType::MiniTicker24Hr => {
                log::debug!("Mini ticker not yet supported, skipping");
                None
            }
            BinanceWsEventType::AccountUpdate => {
                serde_json::from_value::<BinanceFuturesAccountUpdateMsg>(json)
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
                serde_json::from_value::<BinanceFuturesOrderUpdateMsg>(json)
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
                serde_json::from_value::<BinanceFuturesTradeLiteMsg>(json)
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
                serde_json::from_value::<BinanceFuturesAlgoUpdateMsg>(json)
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
                serde_json::from_value::<BinanceFuturesMarginCallMsg>(json)
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
                serde_json::from_value::<BinanceFuturesAccountConfigMsg>(json)
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
                if let Ok(msg) = serde_json::from_value::<BinanceFuturesListenKeyExpiredMsg>(json) {
                    log::debug!("Listen key expired at {}", msg.event_time);
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

pub(super) enum RawFrames {
    Payloads(tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>),
    WebSocket(tokio::sync::mpsc::UnboundedReceiver<Message>),
}

impl RawFrames {
    async fn recv(&mut self) -> Option<Message> {
        match self {
            Self::Payloads(rx) => rx
                .recv()
                .await
                .map(|payload| Message::Binary(payload.into())),
            Self::WebSocket(rx) => rx.recv().await,
        }
    }
}

#[derive(Deserialize)]
struct MarketDataRoute {
    #[serde(rename = "e")]
    event_type: BinanceWsEventType,
    #[serde(default, deserialize_with = "json_field_present")]
    result: bool,
    #[serde(default, deserialize_with = "json_field_present")]
    id: bool,
    #[serde(default, deserialize_with = "json_field_present")]
    code: bool,
}

fn decode_market_data(raw: &[u8]) -> Option<BinanceFuturesWsStreamsMessage> {
    let route = serde_json::from_slice::<MarketDataRoute>(raw).ok()?;
    if route.result || route.id || route.code {
        return None;
    }

    match route.event_type {
        BinanceWsEventType::BookTicker => serde_json::from_slice(raw)
            .ok()
            .map(BinanceFuturesWsStreamsMessage::BookTicker),
        BinanceWsEventType::DepthUpdate => serde_json::from_slice(raw)
            .ok()
            .map(BinanceFuturesWsStreamsMessage::DepthUpdate),
        BinanceWsEventType::AggTrade => serde_json::from_slice(raw)
            .ok()
            .map(BinanceFuturesWsStreamsMessage::AggTrade),
        BinanceWsEventType::Trade => serde_json::from_slice(raw)
            .ok()
            .map(BinanceFuturesWsStreamsMessage::Trade),
        BinanceWsEventType::MarkPriceUpdate => serde_json::from_slice(raw)
            .ok()
            .map(BinanceFuturesWsStreamsMessage::MarkPrice),
        BinanceWsEventType::Kline => serde_json::from_slice(raw)
            .ok()
            .map(BinanceFuturesWsStreamsMessage::Kline),
        BinanceWsEventType::ForceOrder => serde_json::from_slice(raw)
            .ok()
            .map(BinanceFuturesWsStreamsMessage::ForceOrder),
        BinanceWsEventType::Ticker24Hr => serde_json::from_slice(raw)
            .ok()
            .map(BinanceFuturesWsStreamsMessage::Ticker),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case::agg_trade(
        include_str!("../../../../test_data/futures/market_data_json/agg_trade_stream.json"),
        |event| matches!(event, BinanceFuturesWsStreamsMessage::AggTrade(_)),
    )]
    #[case::trade(
        include_str!("../../../../test_data/futures/market_data_json/trade_stream.json"),
        |event| matches!(event, BinanceFuturesWsStreamsMessage::Trade(_)),
    )]
    #[case::book_ticker(
        include_str!("../../../../test_data/futures/market_data_json/book_ticker_stream.json"),
        |event| matches!(event, BinanceFuturesWsStreamsMessage::BookTicker(_)),
    )]
    #[case::depth(
        include_str!("../../../../test_data/futures/market_data_json/depth_update_stream.json"),
        |event| matches!(event, BinanceFuturesWsStreamsMessage::DepthUpdate(_)),
    )]
    #[case::mark_price(
        include_str!("../../../../test_data/futures/market_data_json/mark_price_stream.json"),
        |event| matches!(event, BinanceFuturesWsStreamsMessage::MarkPrice(_)),
    )]
    #[case::kline(
        include_str!("../../../../test_data/futures/market_data_json/kline_stream_open.json"),
        |event| matches!(event, BinanceFuturesWsStreamsMessage::Kline(_)),
    )]
    #[case::liquidation(
        include_str!("../../../../test_data/futures/market_data_json/liquidation_stream.json"),
        |event| matches!(event, BinanceFuturesWsStreamsMessage::ForceOrder(_)),
    )]
    #[case::ticker(
        include_str!("../../../../test_data/futures/market_data_json/ticker_stream.json"),
        |event| matches!(event, BinanceFuturesWsStreamsMessage::Ticker(_)),
    )]
    fn test_decode_market_data_routes_directly(
        #[case] fixture: &str,
        #[case] matches_event: fn(BinanceFuturesWsStreamsMessage) -> bool,
    ) {
        let event = decode_market_data(fixture.as_bytes()).expect("expected a typed message");

        assert!(matches_event(event));
    }

    #[rstest]
    #[case("result", serde_json::Value::Null)]
    #[case("id", serde_json::Value::Null)]
    #[case("code", serde_json::Value::from(-1))]
    fn test_market_data_decode_defers_control_frames(
        #[case] key: &str,
        #[case] value: serde_json::Value,
    ) {
        let fixture = include_bytes!(
            "../../../../test_data/futures/market_data_json/book_ticker_stream.json"
        );
        assert!(matches!(
            decode_market_data(fixture),
            Some(BinanceFuturesWsStreamsMessage::BookTicker(_))
        ));

        let mut frame: serde_json::Value = serde_json::from_slice(fixture).unwrap();
        frame[key] = value;
        let payload = serde_json::to_vec(&frame).unwrap();

        assert!(decode_market_data(&payload).is_none());
    }

    #[rstest]
    #[tokio::test]
    async fn test_subscription_intent_is_preserved_without_active_client() {
        let signal = Arc::new(AtomicBool::new(false));
        let request_id_counter = Arc::new(AtomicU64::new(1));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel();
        let subscriptions = SubscriptionState::new('@');
        let subscribe_topic = "btcusdt@aggTrade";
        let unsubscribe_topic = "ethusdt@aggTrade";
        subscriptions.mark_subscribe(unsubscribe_topic);
        subscriptions.confirm_subscribe(unsubscribe_topic);

        let mut handler = BinanceFuturesDataWsFeedHandler::new(
            signal,
            cmd_rx,
            raw_rx,
            out_tx,
            subscriptions.clone(),
            request_id_counter,
        );

        handler
            .send_subscribe(vec![subscribe_topic.to_string()])
            .await;
        handler
            .send_unsubscribe(vec![unsubscribe_topic.to_string()], None)
            .await;

        assert_eq!(
            subscriptions.pending_subscribe_topics(),
            [subscribe_topic.to_string()]
        );
        assert_eq!(
            subscriptions.pending_unsubscribe_topics(),
            [unsubscribe_topic.to_string()]
        );
        assert_eq!(subscriptions.len(), 0);
        assert_eq!(handler.pending_requests.len(), 0);
    }

    #[rstest]
    #[tokio::test]
    async fn test_correlated_unsubscribe_without_client_is_abandoned_on_reconnect() {
        let signal = Arc::new(AtomicBool::new(false));
        let request_id_counter = Arc::new(AtomicU64::new(1));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel();
        let subscriptions = SubscriptionState::new('@');

        let mut handler = BinanceFuturesDataWsFeedHandler::new(
            signal,
            cmd_rx,
            raw_rx,
            out_tx,
            subscriptions,
            request_id_counter,
        );

        // Without a connected client the wire send cannot happen, but the correlation
        // must survive so the reconnect abandons the drain it protects
        handler
            .send_unsubscribe(vec!["btcusdt@depth5@100ms".to_string()], Some(7))
            .await;

        let msg = handler.handle_raw_message(RECONNECTED.as_bytes());

        let Some(BinanceFuturesWsStreamsMessage::Reconnected(abandoned)) = msg else {
            panic!("expected Reconnected");
        };
        assert_eq!(abandoned, vec![7]);
        assert!(out_rx.try_recv().is_err());
    }

    #[rstest]
    fn test_error_responses_preserve_subscription_intent() {
        let signal = Arc::new(AtomicBool::new(false));
        let request_id_counter = Arc::new(AtomicU64::new(3));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, _out_rx) = tokio::sync::mpsc::unbounded_channel();
        let subscriptions = SubscriptionState::new('@');
        let subscribe_topic = "btcusdt@aggTrade";
        let unsubscribe_topic = "ethusdt@aggTrade";
        subscriptions.mark_subscribe(subscribe_topic);
        subscriptions.mark_subscribe(unsubscribe_topic);
        subscriptions.confirm_subscribe(unsubscribe_topic);
        subscriptions.mark_unsubscribe(unsubscribe_topic);

        let mut handler = BinanceFuturesDataWsFeedHandler::new(
            signal,
            cmd_rx,
            raw_rx,
            out_tx,
            subscriptions.clone(),
            request_id_counter,
        );
        handler.pending_requests.insert(
            1,
            PendingSubscriptionRequest::subscribe(vec![subscribe_topic.to_string()]),
        );
        handler.pending_requests.insert(
            2,
            PendingSubscriptionRequest::unsubscribe(vec![unsubscribe_topic.to_string()]),
        );
        let error = serde_json::json!({"code": 2, "msg": "Invalid request", "id": 1});

        handler.handle_subscription_response(&error);

        assert_eq!(
            subscriptions.pending_subscribe_topics(),
            [subscribe_topic.to_string()]
        );
        assert_eq!(
            subscriptions.pending_unsubscribe_topics(),
            [unsubscribe_topic.to_string()]
        );
        assert_eq!(subscriptions.len(), 0);
        assert_eq!(handler.pending_requests.len(), 1);

        let error = serde_json::json!({"code": 2, "msg": "Invalid request", "id": 2});
        handler.handle_subscription_response(&error);

        assert_eq!(
            subscriptions.pending_subscribe_topics(),
            [subscribe_topic.to_string()]
        );
        assert_eq!(
            subscriptions.pending_unsubscribe_topics(),
            [unsubscribe_topic.to_string()]
        );
        assert_eq!(subscriptions.len(), 0);
        assert_eq!(handler.pending_requests.len(), 0);
    }
}
