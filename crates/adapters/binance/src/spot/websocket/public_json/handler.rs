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
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use nautilus_network::{
    RECONNECTED,
    websocket::{SubscriptionState, WebSocketClient},
};
use serde::Deserialize;
use serde_json::{Value, value::RawValue};
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use super::messages::{
    BinanceSpotBookTickerMsg, BinanceSpotPartialDepthMsg, BinanceSpotPartialDepthPayload,
    BinanceSpotPublicWsCommand, BinanceSpotPublicWsMessage, BinanceSpotServerShutdownMsg,
    BinanceSpotWsErrorResponse, BinanceSpotWsResponse, BinanceWsSubscription,
};
use crate::common::{
    consts::BINANCE_RATE_LIMIT_KEY_SUBSCRIPTION,
    enums::BinanceWsEventType,
    websocket::{
        PendingSubscriptionRequest, PendingSubscriptionRequests, json_field_present,
        reset_requests_after_reconnect,
    },
};

/// Handler for Binance Spot public JSON WebSocket streams.
pub(super) struct BinanceSpotPublicWsHandler {
    signal: Arc<AtomicBool>,
    cmd_rx: tokio::sync::mpsc::UnboundedReceiver<BinanceSpotPublicWsCommand>,
    raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    inner: Option<WebSocketClient>,
    subscriptions: SubscriptionState,
    request_id_counter: Arc<AtomicU64>,
    pending_requests: PendingSubscriptionRequests,
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
        raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
        subscriptions: SubscriptionState,
        request_id_counter: Arc<AtomicU64>,
    ) -> Self {
        Self {
            signal,
            cmd_rx,
            raw_rx,
            inner: None,
            subscriptions,
            request_id_counter,
            pending_requests: PendingSubscriptionRequests::default(),
        }
    }

    pub(super) async fn next(&mut self) -> Option<BinanceSpotPublicWsMessage> {
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
        for stream in &streams {
            self.subscriptions.mark_subscribe(stream);
        }

        let Some(client) = &self.inner else {
            log::warn!("Cannot subscribe: no client connected");
            return;
        };

        let request_id = self.request_id_counter.fetch_add(1, Ordering::Relaxed);
        let request = BinanceWsSubscription::subscribe(streams.clone(), request_id);
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
                request.mark_failure(&self.subscriptions);
            }
            log::error!("Failed to send subscribe request: {e}");
        }
    }

    async fn send_unsubscribe(&mut self, streams: Vec<String>) {
        for stream in &streams {
            self.subscriptions.mark_unsubscribe(stream);
        }

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

    fn handle_raw_message(&mut self, raw: &[u8]) -> Option<BinanceSpotPublicWsMessage> {
        if raw == RECONNECTED.as_bytes() {
            reset_requests_after_reconnect(&mut self.pending_requests, &self.subscriptions);
            log::debug!("WebSocket reconnected signal received");
            return Some(BinanceSpotPublicWsMessage::Reconnected);
        }

        if let Some(message) = decode_market_data(raw) {
            return Some(message);
        }

        let json: Value = match serde_json::from_slice(raw) {
            Ok(j) => j,
            Err(e) => {
                log::warn!("Failed to parse Spot public JSON message: {e}");
                return None;
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
            return Some(BinanceSpotPublicWsMessage::Error(
                crate::spot::websocket::streams::messages::BinanceWsErrorMsg {
                    code: code as i32,
                    msg,
                },
            ));
        }

        if json.get("result").is_some() || json.get("id").is_some() {
            self.handle_subscription_response(&json);
            return None;
        }

        self.handle_stream_data(json)
    }

    fn handle_subscription_response(&mut self, json: &serde_json::Value) {
        if json.get("result").is_some()
            && let Ok(response) = serde_json::from_value::<BinanceSpotWsResponse>(json.clone())
        {
            if let Some(request) = self.pending_requests.take(response.id) {
                if response.result.is_none() {
                    request.confirm(&self.subscriptions);
                    log::debug!("Subscription request confirmed: request={request:?}");
                } else {
                    request.mark_failure(&self.subscriptions);
                    log::warn!(
                        "Subscription request failed: request={request:?}, result={:?}",
                        response.result
                    );
                }
            }
        } else if let Ok(error) = serde_json::from_value::<BinanceSpotWsErrorResponse>(json.clone())
        {
            if let Some(id) = error.id
                && let Some(request) = self.pending_requests.take(id)
            {
                request.mark_failure(&self.subscriptions);
            }
            log::warn!(
                "WebSocket error response: code={}, msg={}",
                error.code,
                error.msg
            );
        }
    }

    fn handle_stream_data(&self, json: Value) -> Option<BinanceSpotPublicWsMessage> {
        let (stream_name, payload) = split_combined_payload(json);
        if let Some(stream_name) = stream_name.as_deref() {
            if let Ok(parsed) = BinanceSpotPartialDepthPayload::deserialize(&payload) {
                return Some(partial_depth_with_symbol(parsed, stream_name));
            }

            if stream_name.ends_with("@bookTicker") {
                return serde_json::from_value::<BinanceSpotBookTickerMsg>(payload)
                    .map(BinanceSpotPublicWsMessage::BookTicker)
                    .map_err(|e| log::warn!("Failed to parse Spot bookTicker: {e}"))
                    .ok();
            }
        }

        // The shared event enum maps `serverShutdown` to `Unknown`
        if payload.get("e").and_then(Value::as_str) == Some("serverShutdown") {
            return serde_json::from_value::<BinanceSpotServerShutdownMsg>(payload)
                .map(BinanceSpotPublicWsMessage::ServerShutdown)
                .map_err(|e| log::warn!("Failed to parse Spot server shutdown event: {e}"))
                .ok();
        }

        let event_type = payload
            .get("e")
            .and_then(|value| BinanceWsEventType::deserialize(value).ok());

        match event_type {
            Some(BinanceWsEventType::Trade) => serde_json::from_value(payload)
                .map(BinanceSpotPublicWsMessage::Trade)
                .map_err(|e| log::warn!("Failed to parse Spot trade: {e}"))
                .ok(),
            Some(BinanceWsEventType::BookTicker) => serde_json::from_value(payload)
                .map(BinanceSpotPublicWsMessage::BookTicker)
                .map_err(|e| log::warn!("Failed to parse Spot bookTicker: {e}"))
                .ok(),
            Some(BinanceWsEventType::DepthUpdate) => serde_json::from_value(payload)
                .map(BinanceSpotPublicWsMessage::DepthDiff)
                .map_err(|e| log::warn!("Failed to parse Spot depthUpdate: {e}"))
                .ok(),
            Some(BinanceWsEventType::Kline) => serde_json::from_value(payload)
                .map(BinanceSpotPublicWsMessage::Kline)
                .map_err(|e| log::warn!("Failed to parse Spot kline: {e}"))
                .ok(),
            Some(BinanceWsEventType::Ticker24Hr) => serde_json::from_value(payload)
                .map(BinanceSpotPublicWsMessage::Ticker)
                .map_err(|e| log::warn!("Failed to parse Spot ticker: {e}"))
                .ok(),
            _ => Some(BinanceSpotPublicWsMessage::RawJson(payload)),
        }
    }
}

fn split_combined_payload(mut json: Value) -> (Option<String>, Value) {
    if let Some(stream) = json.get("stream").and_then(Value::as_str)
        && json.get("data").is_some()
    {
        let stream = stream.to_owned();
        (Some(stream), json["data"].take())
    } else if let Value::Array(values) = &mut json
        && let [Value::String(stream), data] = values.as_mut_slice()
    {
        (Some(std::mem::take(stream)), data.take())
    } else {
        (None, json)
    }
}

#[derive(Deserialize)]
struct MarketDataRoute<'a> {
    #[serde(borrow)]
    stream: Option<&'a str>,
    #[serde(borrow)]
    data: Option<&'a RawValue>,
    #[serde(rename = "e")]
    event_type: Option<BinanceWsEventType>,
    #[serde(
        rename = "lastUpdateId",
        default,
        deserialize_with = "json_field_present"
    )]
    partial_depth: bool,
    #[serde(default, deserialize_with = "json_field_present")]
    result: bool,
    #[serde(default, deserialize_with = "json_field_present")]
    id: bool,
    #[serde(default, deserialize_with = "json_field_present")]
    code: bool,
}

fn decode_market_data(raw: &[u8]) -> Option<BinanceSpotPublicWsMessage> {
    // Serde also accepts positional arrays for structs
    if raw.trim_ascii_start().first() != Some(&b'{') {
        return None;
    }

    let outer = serde_json::from_slice::<MarketDataRoute<'_>>(raw).ok()?;
    if outer.result || outer.id || outer.code {
        return None;
    }

    let stream_name = outer.stream;
    let (raw, route) = if stream_name.is_some() {
        let data = outer.data?.get().as_bytes();
        (data, serde_json::from_slice(data).ok()?)
    } else {
        (raw, outer)
    };

    if let Some(stream_name) = stream_name {
        if route.partial_depth {
            let parsed = serde_json::from_slice(raw).ok()?;
            return Some(partial_depth_with_symbol(parsed, stream_name));
        }

        if stream_name.ends_with("@bookTicker") {
            return serde_json::from_slice(raw)
                .ok()
                .map(BinanceSpotPublicWsMessage::BookTicker);
        }
    }

    match route.event_type? {
        BinanceWsEventType::Trade => serde_json::from_slice(raw)
            .ok()
            .map(BinanceSpotPublicWsMessage::Trade),
        BinanceWsEventType::BookTicker => serde_json::from_slice(raw)
            .ok()
            .map(BinanceSpotPublicWsMessage::BookTicker),
        BinanceWsEventType::DepthUpdate => serde_json::from_slice(raw)
            .ok()
            .map(BinanceSpotPublicWsMessage::DepthDiff),
        BinanceWsEventType::Kline => serde_json::from_slice(raw)
            .ok()
            .map(BinanceSpotPublicWsMessage::Kline),
        BinanceWsEventType::Ticker24Hr => serde_json::from_slice(raw)
            .ok()
            .map(BinanceSpotPublicWsMessage::Ticker),
        _ => None,
    }
}

fn partial_depth_with_symbol(
    parsed: BinanceSpotPartialDepthPayload,
    stream_name: &str,
) -> BinanceSpotPublicWsMessage {
    let symbol = stream_name
        .split_once('@')
        .map_or(stream_name, |(symbol, _)| symbol);
    let symbol = Ustr::from(symbol.to_uppercase().as_str());
    BinanceSpotPublicWsMessage::DepthSnapshot(BinanceSpotPartialDepthMsg {
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
    use rstest::{fixture, rstest};
    use serde_json::json;
    use ustr::Ustr;

    use super::*;

    const TRADE_STREAM: &str =
        include_str!("../../../../test_data/spot/market_data_json/trade_stream.json");

    #[rstest]
    #[case::combined(true)]
    #[case::raw(false)]
    fn test_decode_trade(#[case] combined: bool) {
        let frame = if combined {
            TRADE_STREAM.to_owned()
        } else {
            let frame: Value = serde_json::from_str(TRADE_STREAM).unwrap();
            frame["data"].to_string()
        };

        let Some(BinanceSpotPublicWsMessage::Trade(trade)) = decode_market_data(frame.as_bytes())
        else {
            panic!("expected a Spot trade");
        };

        assert_eq!(trade.event_type, "trade");
        assert_eq!(trade.event_time, 1672515782138);
        assert_eq!(trade.symbol, Ustr::from("BTCUSDT"));
        assert_eq!(trade.trade_id, 12345);
        assert_eq!(trade.price, "42000.12000000");
        assert_eq!(trade.quantity, "0.00510000");
        assert_eq!(trade.trade_time, 1672515782136);
        assert!(trade.is_buyer_maker);
    }

    #[rstest]
    fn test_handle_raw_message_preserves_unknown_array(mut handler: BinanceSpotPublicWsHandler) {
        let frame: Value = serde_json::from_str(TRADE_STREAM).unwrap();
        let array = json!([frame["stream"], frame["data"], null]);
        let bytes = serde_json::to_vec(&array).unwrap();

        let Some(BinanceSpotPublicWsMessage::RawJson(parsed)) = handler.handle_raw_message(&bytes)
        else {
            panic!("expected a raw JSON array");
        };

        assert_eq!(parsed, array);
    }

    #[rstest]
    #[case(json!(["btcusdt@depth20", {
        "lastUpdateId": 12345,
        "bids": [["42000.1", "0.5"]],
        "asks": [["42000.2", "0.8"]]
    }]))]
    #[case(json!({"stream": "btcusdt@depth20", "data": [
        12345,
        [["42000.1", "0.5"]],
        [["42000.2", "0.8"]]
    ]}))]
    fn test_handle_raw_message_preserves_positional_depth(
        #[case] frame: Value,
        mut handler: BinanceSpotPublicWsHandler,
    ) {
        let bytes = serde_json::to_vec(&frame).unwrap();

        let Some(BinanceSpotPublicWsMessage::DepthSnapshot(parsed)) =
            handler.handle_raw_message(&bytes)
        else {
            panic!("expected a partial depth snapshot");
        };

        assert_eq!(parsed.symbol, Ustr::from("BTCUSDT"));
        assert_eq!(parsed.last_update_id, 12345);
        assert_eq!(parsed.bids, [["42000.1", "0.5"]]);
        assert_eq!(parsed.asks, [["42000.2", "0.8"]]);
    }

    #[fixture]
    fn handler() -> BinanceSpotPublicWsHandler {
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();

        BinanceSpotPublicWsHandler::new(
            Arc::new(AtomicBool::new(false)),
            cmd_rx,
            raw_rx,
            SubscriptionState::new('@'),
            Arc::new(AtomicU64::new(1)),
        )
    }

    #[rstest]
    #[case("result", Value::Null)]
    #[case("id", Value::Null)]
    #[case("code", Value::from(-1))]
    fn test_combined_market_data_defers_control_fields(#[case] key: &str, #[case] value: Value) {
        let fixture = include_bytes!(
            "../../../../test_data/futures/market_data_json/book_ticker_stream.json"
        );
        let data: Value = serde_json::from_slice(fixture).unwrap();
        let mut frame = json!({"stream": "btcusdt@bookTicker", "data": data});
        let bytes = serde_json::to_vec(&frame).unwrap();
        assert!(matches!(
            decode_market_data(&bytes),
            Some(BinanceSpotPublicWsMessage::BookTicker(_))
        ));

        frame[key] = value;
        let bytes = serde_json::to_vec(&frame).unwrap();
        assert!(decode_market_data(&bytes).is_none());
    }

    #[rstest]
    fn test_decode_partial_depth_uppercases_stream_symbol() {
        let payload = json!({
            "lastUpdateId": 12345,
            "bids": [["42000.1", "0.5"]],
            "asks": [["42000.2", "0.8"]]
        });

        let frame = json!({"stream": "btcusdt@depth20", "data": payload});

        let Some(BinanceSpotPublicWsMessage::DepthSnapshot(parsed)) =
            decode_market_data(&serde_json::to_vec(&frame).unwrap())
        else {
            panic!("expected a partial depth snapshot");
        };

        assert_eq!(parsed.symbol, Ustr::from("BTCUSDT"));
        assert_eq!(parsed.last_update_id, 12345);
        assert_eq!(parsed.bids, [["42000.1", "0.5"]]);
        assert_eq!(parsed.asks, [["42000.2", "0.8"]]);
    }

    #[rstest]
    fn test_handle_raw_message_emits_reconnected_signal() {
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

        let out = handler.handle_raw_message(RECONNECTED.as_bytes());
        assert!(matches!(out, Some(BinanceSpotPublicWsMessage::Reconnected)));
    }

    #[rstest]
    #[tokio::test]
    async fn test_subscription_intent_is_preserved_without_active_client() {
        let signal = Arc::new(AtomicBool::new(false));
        let request_id_counter = Arc::new(AtomicU64::new(1));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let subscriptions = SubscriptionState::new('@');
        let subscribe_topic = "btcusdt@trade";
        let unsubscribe_topic = "ethusdt@trade";
        subscriptions.mark_subscribe(unsubscribe_topic);
        subscriptions.confirm_subscribe(unsubscribe_topic);

        let mut handler = BinanceSpotPublicWsHandler::new(
            signal,
            cmd_rx,
            raw_rx,
            subscriptions.clone(),
            request_id_counter,
        );

        handler
            .send_subscribe(vec![subscribe_topic.to_string()])
            .await;
        handler
            .send_unsubscribe(vec![unsubscribe_topic.to_string()])
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
    fn test_handle_raw_message_error_with_id_emits_error() {
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
        handler.pending_requests.insert(
            1,
            PendingSubscriptionRequest::subscribe(vec!["btcusdt@trade".to_string()]),
        );

        let payload = json!({
            "code": 2,
            "msg": "Invalid request",
            "id": 1
        });

        let out = handler.handle_raw_message(&serde_json::to_vec(&payload).unwrap());
        match out {
            Some(BinanceSpotPublicWsMessage::Error(err)) => {
                assert_eq!(err.code, 2);
                assert_eq!(err.msg, "Invalid request");
            }
            other => panic!("expected Error variant, was {other:?}"),
        }
        assert_eq!(handler.pending_requests.len(), 0);
    }

    #[rstest]
    fn test_subscription_responses_preserve_unsubscribe_intent_until_acknowledged() {
        let signal = Arc::new(AtomicBool::new(false));
        let request_id_counter = Arc::new(AtomicU64::new(3));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let subscriptions = SubscriptionState::new('@');
        let topic = "btcusdt@trade";

        let mut handler = BinanceSpotPublicWsHandler::new(
            signal,
            cmd_rx,
            raw_rx,
            subscriptions.clone(),
            request_id_counter,
        );
        subscriptions.mark_subscribe(topic);
        handler.pending_requests.insert(
            1,
            PendingSubscriptionRequest::subscribe(vec![topic.to_string()]),
        );
        subscriptions.mark_unsubscribe(topic);
        handler.pending_requests.insert(
            2,
            PendingSubscriptionRequest::unsubscribe(vec![topic.to_string()]),
        );

        handler.handle_subscription_response(&json!({"result": null, "id": 1}));

        assert_eq!(
            subscriptions.pending_unsubscribe_topics(),
            [topic.to_string()]
        );
        assert_eq!(subscriptions.len(), 0);
        assert_eq!(handler.pending_requests.len(), 1);

        handler.handle_subscription_response(&json!({"result": null, "id": 2}));

        assert!(subscriptions.is_empty());
        assert_eq!(handler.pending_requests.len(), 0);
    }

    #[rstest]
    fn test_stale_responses_do_not_mutate_latest_subscription_request() {
        let signal = Arc::new(AtomicBool::new(false));
        let request_id_counter = Arc::new(AtomicU64::new(4));
        let (_cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (_raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();
        let subscriptions = SubscriptionState::new('@');
        let topic = "btcusdt@trade";

        let mut handler = BinanceSpotPublicWsHandler::new(
            signal,
            cmd_rx,
            raw_rx,
            subscriptions.clone(),
            request_id_counter,
        );
        subscriptions.mark_subscribe(topic);
        handler.pending_requests.insert(
            1,
            PendingSubscriptionRequest::subscribe(vec![topic.to_string()]),
        );
        subscriptions.mark_unsubscribe(topic);
        handler.pending_requests.insert(
            2,
            PendingSubscriptionRequest::unsubscribe(vec![topic.to_string()]),
        );
        subscriptions.mark_subscribe(topic);
        handler.pending_requests.insert(
            3,
            PendingSubscriptionRequest::subscribe(vec![topic.to_string()]),
        );

        handler.handle_subscription_response(&json!({"result": null, "id": 3}));
        handler.handle_subscription_response(&json!({"code": 2, "msg": "Stale failure", "id": 1}));
        handler.handle_subscription_response(&json!({"result": null, "id": 2}));

        assert_eq!(subscriptions.len(), 1);
        assert_eq!(subscriptions.all_topics(), [topic]);
        assert!(subscriptions.pending_subscribe_topics().is_empty());
        assert!(subscriptions.pending_unsubscribe_topics().is_empty());
        assert_eq!(handler.pending_requests.len(), 0);
    }

    #[rstest]
    #[case::direct(true)]
    #[case::fallback(false)]
    fn test_parses_book_ticker_without_event_type(#[case] direct: bool) {
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

        let out = if direct {
            decode_market_data(&serde_json::to_vec(&payload).unwrap())
        } else {
            handler.handle_stream_data(payload)
        };

        let Some(BinanceSpotPublicWsMessage::BookTicker(out)) = out else {
            panic!("expected book ticker");
        };

        assert_eq!(out.book_update_id, 94528182161);
        assert_eq!(out.symbol, "BTCUSDT");
        assert_eq!(out.best_bid_price, "73650.51000000");
        assert_eq!(out.best_bid_qty, "2.95126000");
        assert_eq!(out.best_ask_price, "73650.52000000");
        assert_eq!(out.best_ask_qty, "1.38108000");
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

        let out = handler.handle_stream_data(payload);
        assert!(matches!(
            out,
            Some(BinanceSpotPublicWsMessage::ServerShutdown(_))
        ));
    }
}
