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

//! Binance Spot public JSON WebSocket handler.

use std::{
    collections::VecDeque,
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
use ustr::Ustr;

use super::messages::{
    BinanceCombinedStreamEvent, BinanceSpotBookTickerMsg, BinanceSpotKlineMsg,
    BinanceSpotPartialDepthMsg, BinanceSpotPartialDepthPayload, BinanceSpotPublicWsCommand,
    BinanceSpotPublicWsMessage, BinanceSpotServerShutdownMsg, BinanceSpotTradeMsg,
    BinanceSpotWsErrorResponse, BinanceSpotWsResponse, BinanceWsSubscription,
};
use crate::common::{consts::BINANCE_RATE_LIMIT_KEY_SUBSCRIPTION, enums::BinanceWsEventType};

/// Handler for Binance Spot public JSON WebSocket streams.
pub(super) struct BinanceSpotPublicWsHandler {
    signal: Arc<AtomicBool>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<BinanceSpotPublicWsCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
    inner: Option<WebSocketClient>,
    pending_messages: VecDeque<BinanceSpotPublicWsMessage>,
    subscriptions: SubscriptionState,
    request_id_counter: Arc<AtomicU64>,
    pending_requests: AHashMap<u64, Vec<String>>,
}

impl Debug for BinanceSpotPublicWsHandler {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BinanceSpotPublicWsHandler))
            .field("pending_requests", &self.pending_requests.len())
            .finish_non_exhaustive()
    }
}

impl BinanceSpotPublicWsHandler {
    pub(super) fn new(
        signal: Arc<AtomicBool>,
        cmd_rx: tokio::sync::mpsc::UnboundedReceiver<BinanceSpotPublicWsCommand>,
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
        subscriptions: SubscriptionState,
        request_id_counter: Arc<AtomicU64>,
    ) -> Self {
        Self {
            signal,
            cmd_rx,
            raw_rx,
            inner: None,
            pending_messages: VecDeque::new(),
            subscriptions,
            request_id_counter,
            pending_requests: AHashMap::new(),
        }
    }

    pub(super) async fn next(&mut self) -> Option<BinanceSpotPublicWsMessage> {
        loop {
            if let Some(msg) = self.pending_messages.pop_front() {
                return Some(msg);
            }

            if self.signal.load(Ordering::Relaxed) {
                return None;
            }

            tokio::select! {
                Some(cmd) = self.cmd_rx.recv() => {
                    self.handle_command(cmd).await;
                }
                Some(raw) = self.raw_rx.recv() => {
                    let out = self.handle_raw_message(raw).await;
                    if !out.is_empty() {
                        let mut iter = out.into_iter();
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

    async fn handle_command(&mut self, cmd: BinanceSpotPublicWsCommand) {
        match cmd {
            BinanceSpotPublicWsCommand::SetClient(client) => {
                self.inner = Some(client);
            }
            BinanceSpotPublicWsCommand::Disconnect => {
                if let Some(client) = &self.inner {
                    let () = client.disconnect().await;
                }
                self.inner = None;
            }
            BinanceSpotPublicWsCommand::Subscribe { streams } => {
                self.send_subscribe(streams).await;
            }
            BinanceSpotPublicWsCommand::Unsubscribe { streams } => {
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
            self.subscriptions.mark_subscribe(stream);
        }

        let request = BinanceWsSubscription::subscribe(streams, request_id);
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

        let request = BinanceWsSubscription::unsubscribe(streams.clone(), request_id);
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
            self.subscriptions.mark_unsubscribe(stream);
            self.subscriptions.confirm_unsubscribe(stream);
        }
    }

    async fn handle_raw_message(&mut self, raw: Vec<u8>) -> Vec<BinanceSpotPublicWsMessage> {
        if let Ok(text) = std::str::from_utf8(&raw)
            && text == RECONNECTED
        {
            log::info!("WebSocket reconnected signal received");
            return vec![BinanceSpotPublicWsMessage::Reconnected];
        }

        let json: serde_json::Value = match serde_json::from_slice(&raw) {
            Ok(j) => j,
            Err(e) => {
                log::warn!("Failed to parse Spot public JSON message: {e}");
                return vec![];
            }
        };

        if let Some(code) = json.get("code")
            && let Some(code) = code.as_i64()
        {
            self.handle_subscription_response(&json);
            let msg = json
                .get("msg")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error")
                .to_string();
            return vec![BinanceSpotPublicWsMessage::Error(
                crate::spot::websocket::streams::messages::BinanceWsErrorMsg {
                    code: code as i32,
                    msg,
                },
            )];
        }

        if json.get("result").is_some() || json.get("id").is_some() {
            self.handle_subscription_response(&json);
            return vec![];
        }

        self.handle_stream_data(&json)
    }

    fn handle_subscription_response(&mut self, json: &serde_json::Value) {
        if json.get("result").is_some()
            && let Ok(response) = serde_json::from_value::<BinanceSpotWsResponse>(json.clone())
        {
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
            }
        } else if let Ok(error) = serde_json::from_value::<BinanceSpotWsErrorResponse>(json.clone())
        {
            if let Some(id) = error.id
                && let Some(streams) = self.pending_requests.remove(&id)
            {
                for stream in &streams {
                    self.subscriptions.mark_failure(stream);
                }
            }
            log::warn!(
                "WebSocket error response: code={}, msg={}",
                error.code,
                error.msg
            );
        }
    }

    fn handle_stream_data(&self, json: &serde_json::Value) -> Vec<BinanceSpotPublicWsMessage> {
        let (stream_name, payload) = split_combined_payload(json);

        if let Some(depth) = parse_partial_depth_with_symbol(&payload, stream_name.as_deref()) {
            return vec![BinanceSpotPublicWsMessage::DepthSnapshot(depth)];
        }

        if let Some(stream_name) = stream_name.as_deref()
            && stream_name.ends_with("@bookTicker")
        {
            return serde_json::from_value::<BinanceSpotBookTickerMsg>(payload)
                .map(BinanceSpotPublicWsMessage::BookTicker)
                .map_err(|e| log::warn!("Failed to parse Spot bookTicker: {e}"))
                .ok()
                .into_iter()
                .collect();
        }

        // `serverShutdown` is not a `BinanceWsEventType` variant (it deserializes to
        // `Unknown` via `#[serde(other)]`), so detect it from the raw `e` field before
        // enum dispatch, mirroring the SBE streams handler.
        if payload.get("e").and_then(|v| v.as_str()) == Some("serverShutdown") {
            return serde_json::from_value::<BinanceSpotServerShutdownMsg>(payload)
                .map(BinanceSpotPublicWsMessage::ServerShutdown)
                .map_err(|e| log::warn!("Failed to parse Spot server shutdown event: {e}"))
                .ok()
                .into_iter()
                .collect();
        }

        let Some(event_type) = extract_event_type(&payload) else {
            return vec![BinanceSpotPublicWsMessage::RawJson(payload)];
        };

        match event_type {
            BinanceWsEventType::Trade => serde_json::from_value::<BinanceSpotTradeMsg>(payload)
                .map(BinanceSpotPublicWsMessage::Trade)
                .map_err(|e| log::warn!("Failed to parse Spot trade: {e}"))
                .ok()
                .into_iter()
                .collect(),
            BinanceWsEventType::BookTicker => {
                serde_json::from_value::<BinanceSpotBookTickerMsg>(payload)
                    .map(BinanceSpotPublicWsMessage::BookTicker)
                    .map_err(|e| log::warn!("Failed to parse Spot bookTicker: {e}"))
                    .ok()
                    .into_iter()
                    .collect()
            }
            BinanceWsEventType::Kline => serde_json::from_value::<BinanceSpotKlineMsg>(payload)
                .map(BinanceSpotPublicWsMessage::Kline)
                .map_err(|e| log::warn!("Failed to parse Spot kline: {e}"))
                .ok()
                .into_iter()
                .collect(),
            _ => vec![BinanceSpotPublicWsMessage::RawJson(payload)],
        }
    }
}

fn split_combined_payload(json: &serde_json::Value) -> (Option<String>, serde_json::Value) {
    if let Ok(wrapper) = serde_json::from_value::<BinanceCombinedStreamEvent>(json.clone()) {
        (Some(wrapper.stream), wrapper.data)
    } else {
        (None, json.clone())
    }
}

fn extract_event_type(json: &serde_json::Value) -> Option<BinanceWsEventType> {
    json.get("e")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
}

fn parse_partial_depth_with_symbol(
    payload: &serde_json::Value,
    stream_name: Option<&str>,
) -> Option<BinanceSpotPartialDepthMsg> {
    let parsed = serde_json::from_value::<BinanceSpotPartialDepthPayload>(payload.clone()).ok()?;

    let symbol = stream_name
        .and_then(|stream| stream.split('@').next())
        .map(|s| Ustr::from(s.to_uppercase().as_str()))?;

    Some(BinanceSpotPartialDepthMsg {
        symbol,
        last_update_id: parsed.last_update_id,
        bids: parsed.bids,
        asks: parsed.asks,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64},
    };

    use nautilus_network::{RECONNECTED, websocket::SubscriptionState};
    use rstest::rstest;
    use serde_json::json;
    use ustr::Ustr;

    use super::*;

    #[rstest]
    fn test_parse_partial_depth_with_symbol_uppercases_symbol_from_stream_name() {
        let payload = json!({
            "lastUpdateId": 12345,
            "bids": [["42000.1", "0.5"]],
            "asks": [["42000.2", "0.8"]]
        });

        let parsed = parse_partial_depth_with_symbol(&payload, Some("btcusdt@depth20"))
            .expect("depth payload should parse");

        assert_eq!(parsed.symbol, Ustr::from("BTCUSDT"));
        assert_eq!(parsed.last_update_id, 12345);
        assert_eq!(parsed.bids.len(), 1);
        assert_eq!(parsed.asks.len(), 1);
    }

    #[tokio::test]
    async fn test_handle_raw_message_emits_reconnected_signal() {
        let signal = Arc::new(AtomicBool::new(false));
        let request_id_counter = Arc::new(AtomicU64::new(1));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let subscriptions = SubscriptionState::new('@');

        let mut handler = BinanceSpotPublicWsHandler::new(
            signal,
            cmd_rx,
            raw_rx,
            subscriptions,
            request_id_counter,
        );

        let out = handler
            .handle_raw_message(RECONNECTED.as_bytes().to_vec())
            .await;
        assert_eq!(out.len(), 1);
        assert!(matches!(out[0], BinanceSpotPublicWsMessage::Reconnected));
    }

    #[tokio::test]
    async fn test_handle_raw_message_error_with_id_emits_error() {
        let signal = Arc::new(AtomicBool::new(false));
        let request_id_counter = Arc::new(AtomicU64::new(2));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let subscriptions = SubscriptionState::new('@');

        let mut handler = BinanceSpotPublicWsHandler::new(
            signal,
            cmd_rx,
            raw_rx,
            subscriptions,
            request_id_counter,
        );
        handler
            .pending_requests
            .insert(1, vec!["btcusdt@trade".to_string()]);

        let payload = json!({
            "code": 2,
            "msg": "Invalid request",
            "id": 1
        });

        let out = handler
            .handle_raw_message(payload.to_string().into_bytes())
            .await;
        assert_eq!(out.len(), 1);
        match &out[0] {
            BinanceSpotPublicWsMessage::Error(err) => {
                assert_eq!(err.code, 2);
                assert_eq!(err.msg, "Invalid request");
            }
            other => panic!("expected Error variant, was {other:?}"),
        }
        assert!(handler.pending_requests.is_empty());
    }

    #[rstest]
    fn test_handle_stream_data_parses_book_ticker_without_event_type() {
        let signal = Arc::new(AtomicBool::new(false));
        let request_id_counter = Arc::new(AtomicU64::new(1));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let subscriptions = SubscriptionState::new('@');

        let handler = BinanceSpotPublicWsHandler::new(
            signal,
            cmd_rx,
            raw_rx,
            subscriptions,
            request_id_counter,
        );

        let payload = json!({
            "stream": "btcusdt@bookTicker",
            "data": {
                "u": 94528182161_u64,
                "s": "BTCUSDT",
                "b": "73650.51000000",
                "B": "2.95126000",
                "a": "73650.52000000",
                "A": "1.38108000"
            }
        });

        let out = handler.handle_stream_data(&payload);
        assert_eq!(out.len(), 1);
        assert!(matches!(out[0], BinanceSpotPublicWsMessage::BookTicker(_)));
    }

    #[rstest]
    fn test_handle_stream_data_emits_server_shutdown() {
        let signal = Arc::new(AtomicBool::new(false));
        let request_id_counter = Arc::new(AtomicU64::new(1));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let subscriptions = SubscriptionState::new('@');

        let handler = BinanceSpotPublicWsHandler::new(
            signal,
            cmd_rx,
            raw_rx,
            subscriptions,
            request_id_counter,
        );

        // `serverShutdown` is not a BinanceWsEventType variant, so it must be
        // recognized from the raw `e` field rather than dropped as RawJson.
        let payload = json!({"e": "serverShutdown", "E": 1_700_000_000_000_i64});

        let out = handler.handle_stream_data(&payload);
        assert_eq!(out.len(), 1);
        assert!(matches!(
            out[0],
            BinanceSpotPublicWsMessage::ServerShutdown(_)
        ));
    }
}
