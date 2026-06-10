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

//! Private Polymarket RTDS feed support.

use std::{
    str::FromStr,
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};

use ahash::AHashMap;
use anyhow::Context;
use nautilus_common::{live::get_runtime, messages::DataEvent};
use nautilus_core::{UnixNanos, time::AtomicTime};
use nautilus_model::{
    data::{CustomData, Data as NautilusData, DataType, custom::CustomDataTrait},
    types::Price,
};
use nautilus_network::websocket::{
    TransportBackend, WebSocketClient, WebSocketConfig, channel_message_handler,
};
use serde::{Deserialize, Serialize};
use serde_json::Number;
use tokio_tungstenite::tungstenite::Message;

use crate::data_types::{PolymarketRtdsCryptoPrice, PolymarketRtdsEquityPrice};

const POLYMARKET_RTDS_HEARTBEAT_SECS: u64 = 5;
const POLYMARKET_RTDS_IDLE_TIMEOUT_MS: u64 = 30_000;
const POLYMARKET_RTDS_RECONNECT_TIMEOUT_MS: u64 = 15_000;
const POLYMARKET_RTDS_RECONNECT_DELAY_INITIAL_MS: u64 = 250;
const POLYMARKET_RTDS_RECONNECT_DELAY_MAX_MS: u64 = 5_000;
const POLYMARKET_RTDS_RECONNECT_JITTER_MS: u64 = 200;
const POLYMARKET_RTDS_CRYPTO_PRICE_TYPE_NAME: &str = "PolymarketRtdsCryptoPrice";
const POLYMARKET_RTDS_EQUITY_PRICE_TYPE_NAME: &str = "PolymarketRtdsEquityPrice";

pub(crate) fn is_supported_rtds_data_type(data_type: &DataType) -> bool {
    matches!(
        data_type.type_name(),
        POLYMARKET_RTDS_CRYPTO_PRICE_TYPE_NAME | POLYMARKET_RTDS_EQUITY_PRICE_TYPE_NAME
    )
}

#[derive(Clone, Debug)]
pub(crate) struct PolymarketRtdsFeed {
    inner: Arc<PolymarketRtdsFeedInner>,
}

#[derive(Debug)]
struct PolymarketRtdsFeedInner {
    url: String,
    transport_backend: TransportBackend,
    clock: &'static AtomicTime,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    subscriptions: dashmap::DashMap<String, TrackedSubscription>,
    ws_client: StdMutex<Option<Arc<WebSocketClient>>>,
    task_handle: StdMutex<Option<tokio::task::JoinHandle<()>>>,
    wire_mutex: tokio::sync::Mutex<()>,
}

#[derive(Clone, Debug)]
struct TrackedSubscription {
    wire: RtdsWireSubscription,
    total_ref_count: usize,
    data_types: AHashMap<String, TrackedDataType>,
}

#[derive(Clone, Debug)]
struct TrackedDataType {
    data_type: DataType,
    ref_count: usize,
}

#[derive(Clone, Debug, Serialize)]
struct RtdsWireRequest {
    action: &'static str,
    subscriptions: Vec<RtdsWireSubscription>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct RtdsWireSubscription {
    topic: &'static str,
    #[serde(rename = "type")]
    msg_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    filters: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RtdsTopic {
    CryptoPrices,
    EquityPrices,
}

impl RtdsTopic {
    fn as_str(self) -> &'static str {
        match self {
            Self::CryptoPrices => "crypto_prices",
            Self::EquityPrices => "equity_prices",
        }
    }
}

#[derive(Clone, Debug)]
struct ParsedSubscription {
    key: String,
    wire: RtdsWireSubscription,
}

#[derive(Debug, Deserialize)]
struct RtdsEnvelope {
    topic: String,
    #[serde(rename = "type")]
    msg_type: String,
    timestamp: u64,
    payload: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct CryptoPayloadRaw {
    symbol: String,
    timestamp: u64,
    value: Number,
}

#[derive(Debug, Deserialize)]
struct CryptoSubscribePayloadRaw {
    symbol: String,
    data: Vec<SnapshotPointRaw>,
}

#[derive(Debug, Deserialize)]
struct EquityPayloadRaw {
    symbol: String,
    value: Number,
    full_accuracy_value: String,
    timestamp: u64,
    #[serde(default)]
    received_at: Option<u64>,
    #[serde(default)]
    is_carried_forward: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct EquitySubscribePayloadRaw {
    symbol: String,
    data: Vec<SnapshotPointRaw>,
}

#[derive(Debug, Deserialize)]
struct SnapshotPointRaw {
    timestamp: u64,
    value: Number,
}

impl PolymarketRtdsFeed {
    pub(crate) fn new(
        url: String,
        transport_backend: TransportBackend,
        clock: &'static AtomicTime,
        data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    ) -> Self {
        Self {
            inner: Arc::new(PolymarketRtdsFeedInner {
                url,
                transport_backend,
                clock,
                data_sender,
                subscriptions: dashmap::DashMap::new(),
                ws_client: StdMutex::new(None),
                task_handle: StdMutex::new(None),
                wire_mutex: tokio::sync::Mutex::new(()),
            }),
        }
    }

    pub(crate) fn has_subscriptions(&self) -> bool {
        !self.inner.subscriptions.is_empty()
    }

    pub(crate) fn track_subscribe(
        &self,
        data_type: DataType,
    ) -> anyhow::Result<Option<RtdsWireSubscription>> {
        let parsed = ParsedSubscription::from_data_type(&data_type)?;
        let mut entry = self
            .inner
            .subscriptions
            .entry(parsed.key.clone())
            .or_insert_with(|| TrackedSubscription {
                wire: parsed.wire.clone(),
                total_ref_count: 0,
                data_types: AHashMap::new(),
            });

        let should_send_wire = entry.total_ref_count == 0;
        entry.total_ref_count += 1;

        let data_type_key = data_type.topic().to_string();
        entry
            .data_types
            .entry(data_type_key)
            .and_modify(|tracked| tracked.ref_count += 1)
            .or_insert(TrackedDataType {
                data_type,
                ref_count: 1,
            });

        Ok(should_send_wire.then_some(parsed.wire))
    }

    pub(crate) fn track_unsubscribe(
        &self,
        data_type: &DataType,
    ) -> anyhow::Result<Option<RtdsWireSubscription>> {
        let parsed = ParsedSubscription::from_data_type(data_type)?;
        let mut entry = match self.inner.subscriptions.get_mut(&parsed.key) {
            Some(entry) => entry,
            None => return Ok(None),
        };

        let data_type_key = data_type.topic().to_string();
        let Some(tracked) = entry.data_types.get_mut(&data_type_key) else {
            return Ok(None);
        };

        if tracked.ref_count > 1 {
            tracked.ref_count -= 1;
        } else {
            entry.data_types.remove(&data_type_key);
        }

        if entry.total_ref_count > 1 {
            entry.total_ref_count -= 1;
            return Ok(None);
        }

        let wire = entry.wire.clone();
        drop(entry);
        self.inner.subscriptions.remove(&parsed.key);
        Ok(Some(wire))
    }

    pub(crate) async fn connect(&self) -> anyhow::Result<()> {
        let _guard = self.inner.wire_mutex.lock().await;
        let _fresh_connect = self.ensure_connected_locked().await?;
        Ok(())
    }

    pub(crate) async fn subscribe_live(&self, wire: RtdsWireSubscription) -> anyhow::Result<()> {
        let _guard = self.inner.wire_mutex.lock().await;
        let fresh_connect = self.ensure_connected_locked().await?;
        if fresh_connect {
            return Ok(());
        }

        let Some(ws) = self.current_ws() else {
            anyhow::bail!("RTDS WebSocket client unavailable after connect");
        };

        log::debug!(
            "Subscribing Polymarket RTDS topic={} type={} filters={:?}",
            wire.topic,
            wire.msg_type,
            wire.filters
        );
        self.send_wire_request(&ws, "subscribe", vec![wire]).await
    }

    pub(crate) async fn unsubscribe_live(&self, wire: RtdsWireSubscription) -> anyhow::Result<()> {
        let _guard = self.inner.wire_mutex.lock().await;
        let Some(ws) = self.current_ws() else {
            return Ok(());
        };

        if !ws.is_active() {
            return Ok(());
        }

        log::debug!(
            "Unsubscribing Polymarket RTDS topic={} type={} filters={:?}",
            wire.topic,
            wire.msg_type,
            wire.filters
        );
        self.send_wire_request(&ws, "unsubscribe", vec![wire]).await
    }

    pub(crate) async fn disconnect(&self) {
        let _guard = self.inner.wire_mutex.lock().await;
        let ws = self
            .inner
            .ws_client
            .lock()
            .expect("RTDS ws_client mutex poisoned")
            .take();
        let handle = self
            .inner
            .task_handle
            .lock()
            .expect("RTDS task_handle mutex poisoned")
            .take();

        if let Some(ws) = ws {
            ws.disconnect().await;
        }

        if let Some(handle) = handle {
            let abort_handle = handle.abort_handle();
            tokio::select! {
                result = handle => {
                    if let Err(e) = result
                        && !e.is_cancelled()
                    {
                        log::error!("RTDS handler task error: {e:?}");
                    }
                }
                () = tokio::time::sleep(Duration::from_secs(2)) => {
                    abort_handle.abort();
                }
            }
        }
    }

    pub(crate) fn abort(&self) {
        let ws = self
            .inner
            .ws_client
            .lock()
            .expect("RTDS ws_client mutex poisoned")
            .take();

        if let Some(ws) = ws {
            get_runtime().spawn(async move {
                ws.disconnect().await;
            });
        }

        if let Some(handle) = self
            .inner
            .task_handle
            .lock()
            .expect("RTDS task_handle mutex poisoned")
            .take()
        {
            handle.abort();
        }
    }

    #[cfg(test)]
    pub(crate) fn tracked_subscription_count(&self) -> usize {
        self.inner.subscriptions.len()
    }

    #[cfg(test)]
    pub(crate) fn tracked_data_type_count(&self, key: &str) -> usize {
        self.inner
            .subscriptions
            .get(key)
            .map_or(0, |entry| entry.data_types.len())
    }

    #[cfg(test)]
    fn handle_text_for_test(&self, text: &str) {
        self.handle_text_message(text);
    }

    fn current_ws(&self) -> Option<Arc<WebSocketClient>> {
        self.inner
            .ws_client
            .lock()
            .expect("RTDS ws_client mutex poisoned")
            .clone()
    }

    async fn ensure_connected_locked(&self) -> anyhow::Result<bool> {
        if self.current_ws().is_some_and(|ws| !ws.is_disconnected()) {
            return Ok(false);
        }

        let (handler, raw_rx) = channel_message_handler();
        let config = WebSocketConfig {
            url: self.inner.url.clone(),
            headers: vec![],
            heartbeat: Some(POLYMARKET_RTDS_HEARTBEAT_SECS),
            heartbeat_msg: Some("PING".to_string()),
            reconnect_timeout_ms: Some(POLYMARKET_RTDS_RECONNECT_TIMEOUT_MS),
            reconnect_delay_initial_ms: Some(POLYMARKET_RTDS_RECONNECT_DELAY_INITIAL_MS),
            reconnect_delay_max_ms: Some(POLYMARKET_RTDS_RECONNECT_DELAY_MAX_MS),
            reconnect_backoff_factor: Some(2.0),
            reconnect_jitter_ms: Some(POLYMARKET_RTDS_RECONNECT_JITTER_MS),
            reconnect_max_attempts: None,
            idle_timeout_ms: Some(POLYMARKET_RTDS_IDLE_TIMEOUT_MS),
            backend: self.inner.transport_backend,
            proxy_url: None,
        };

        let ws = Arc::new(
            WebSocketClient::connect(config, Some(handler), None, None, vec![], None)
                .await
                .context("failed to connect Polymarket RTDS WebSocket")?,
        );
        log::info!("Polymarket RTDS WebSocket connected: {}", self.inner.url);

        let feed = self.clone();
        let ws_for_task = Arc::clone(&ws);
        let handle = get_runtime().spawn(async move {
            feed.run_message_loop(ws_for_task, raw_rx).await;
        });

        *self
            .inner
            .ws_client
            .lock()
            .expect("RTDS ws_client mutex poisoned") = Some(Arc::clone(&ws));

        if let Some(old_handle) = self
            .inner
            .task_handle
            .lock()
            .expect("RTDS task_handle mutex poisoned")
            .replace(handle)
        {
            old_handle.abort();
        }

        let subscriptions = self.snapshot_wire_subscriptions();
        if !subscriptions.is_empty() {
            self.send_wire_request(&ws, "subscribe", subscriptions)
                .await?;
        }

        Ok(true)
    }

    fn snapshot_wire_subscriptions(&self) -> Vec<RtdsWireSubscription> {
        self.inner
            .subscriptions
            .iter()
            .map(|entry| entry.wire.clone())
            .collect()
    }

    async fn send_wire_request(
        &self,
        ws: &Arc<WebSocketClient>,
        action: &'static str,
        subscriptions: Vec<RtdsWireSubscription>,
    ) -> anyhow::Result<()> {
        if subscriptions.is_empty() {
            return Ok(());
        }

        let request = RtdsWireRequest {
            action,
            subscriptions,
        };
        let payload = serde_json::to_string(&request)?;
        ws.send_text(payload, None)
            .await
            .map_err(|e| anyhow::anyhow!("failed to send RTDS {action} request: {e}"))
    }

    async fn run_message_loop(
        &self,
        ws: Arc<WebSocketClient>,
        mut raw_rx: tokio::sync::mpsc::UnboundedReceiver<Message>,
    ) {
        loop {
            match raw_rx.recv().await {
                Some(Message::Text(text)) => {
                    if text.as_str() == "reconnected" {
                        log::info!("Polymarket RTDS reconnected");
                        let subscriptions = self.snapshot_wire_subscriptions();
                        if let Err(e) = self
                            .send_wire_request(&ws, "subscribe", subscriptions)
                            .await
                        {
                            log::error!("Failed to replay RTDS subscriptions after reconnect: {e}");
                        }
                        continue;
                    }

                    if text.as_str() == "PONG" {
                        continue;
                    }

                    self.handle_text_message(text.as_str());
                }
                Some(Message::Binary(_)) => {
                    log::debug!("Ignoring binary RTDS message");
                }
                Some(other) => {
                    log::debug!("Ignoring RTDS control message: {other:?}");
                }
                None => {
                    log::debug!("RTDS message channel closed");
                    break;
                }
            }
        }
    }

    fn handle_text_message(&self, text: &str) {
        if text.trim().is_empty() {
            return;
        }

        let envelope: RtdsEnvelope = match serde_json::from_str(text) {
            Ok(envelope) => envelope,
            Err(e) => {
                log::debug!("Ignoring non-RTDS JSON frame: {e}");
                return;
            }
        };

        match (envelope.topic.as_str(), envelope.msg_type.as_str()) {
            ("crypto_prices", "subscribe") => self.handle_crypto_price_subscribe(envelope),
            ("crypto_prices", "update") => self.handle_crypto_price_update(envelope),
            ("equity_prices", "subscribe") => self.handle_equity_price_subscribe(envelope),
            ("equity_prices", "update") => self.handle_equity_price_update(envelope),
            _ => {
                log::debug!(
                    "Ignoring unsupported RTDS message topic={} type={}",
                    envelope.topic,
                    envelope.msg_type,
                );
            }
        }
    }

    fn handle_crypto_price_update(&self, envelope: RtdsEnvelope) {
        let payload: CryptoPayloadRaw = match serde_json::from_value(envelope.payload) {
            Ok(payload) => payload,
            Err(e) => {
                log::error!("Failed to parse RTDS crypto price payload: {e}");
                return;
            }
        };

        let symbol_lower = payload.symbol.to_ascii_lowercase();
        let data_types = self.matching_data_types(RtdsTopic::CryptoPrices, &symbol_lower);
        if data_types.is_empty() {
            return;
        }

        let value = match price_from_json_number("value", &payload.value) {
            Ok(value) => value,
            Err(e) => {
                log::error!("Failed to parse RTDS crypto price value: {e}");
                return;
            }
        };

        let ts_event = UnixNanos::from_millis(payload.timestamp);
        let ts_init = self.inner.clock.get_time_ns();
        let custom_payload = Arc::new(PolymarketRtdsCryptoPrice::new(
            symbol_lower,
            value,
            payload.timestamp,
            envelope.timestamp,
            ts_event,
            ts_init,
        ));

        self.emit_custom_payload(&custom_payload, data_types);
    }

    fn handle_crypto_price_subscribe(&self, envelope: RtdsEnvelope) {
        let payload: CryptoSubscribePayloadRaw = match serde_json::from_value(envelope.payload) {
            Ok(payload) => payload,
            Err(e) => {
                log::error!("Failed to parse RTDS crypto subscribe payload: {e}");
                return;
            }
        };

        let symbol_lower = payload.symbol.to_ascii_lowercase();
        let data_types = self.matching_data_types(RtdsTopic::CryptoPrices, &symbol_lower);
        if data_types.is_empty() {
            return;
        }

        for point in payload.data {
            let value = match price_from_json_number("value", &point.value) {
                Ok(value) => value,
                Err(e) => {
                    log::error!("Failed to parse RTDS crypto subscribe value: {e}");
                    continue;
                }
            };

            let ts_event = UnixNanos::from_millis(point.timestamp);
            let ts_init = self.inner.clock.get_time_ns();
            let custom_payload = Arc::new(PolymarketRtdsCryptoPrice::new(
                symbol_lower.clone(),
                value,
                point.timestamp,
                envelope.timestamp,
                ts_event,
                ts_init,
            ));

            self.emit_custom_payload(&custom_payload, data_types.clone());
        }
    }

    fn handle_equity_price_update(&self, envelope: RtdsEnvelope) {
        let payload: EquityPayloadRaw = match serde_json::from_value(envelope.payload) {
            Ok(payload) => payload,
            Err(e) => {
                log::error!("Failed to parse RTDS equity price payload: {e}");
                return;
            }
        };

        let symbol_lower = payload.symbol.to_ascii_lowercase();
        let data_types = self.matching_data_types(RtdsTopic::EquityPrices, &symbol_lower);
        if data_types.is_empty() {
            return;
        }

        let value = match price_from_json_number("value", &payload.value) {
            Ok(value) => value,
            Err(e) => {
                log::error!("Failed to parse RTDS equity price value: {e}");
                return;
            }
        };

        let full_accuracy_value =
            match price_from_str("full_accuracy_value", payload.full_accuracy_value.as_str()) {
                Ok(value) => value,
                Err(e) => {
                    log::error!("Failed to parse RTDS equity full_accuracy_value: {e}");
                    return;
                }
            };

        let ts_event = UnixNanos::from_millis(payload.timestamp);
        let ts_init = self.inner.clock.get_time_ns();
        let custom_payload = Arc::new(PolymarketRtdsEquityPrice::new(
            symbol_lower,
            value,
            full_accuracy_value,
            payload.timestamp,
            envelope.timestamp,
            payload.received_at,
            payload.is_carried_forward.unwrap_or(false),
            ts_event,
            ts_init,
        ));

        self.emit_custom_payload(&custom_payload, data_types);
    }

    fn handle_equity_price_subscribe(&self, envelope: RtdsEnvelope) {
        let payload: EquitySubscribePayloadRaw = match serde_json::from_value(envelope.payload) {
            Ok(payload) => payload,
            Err(e) => {
                log::error!("Failed to parse RTDS equity subscribe payload: {e}");
                return;
            }
        };

        let symbol_lower = payload.symbol.to_ascii_lowercase();
        let data_types = self.matching_data_types(RtdsTopic::EquityPrices, &symbol_lower);
        if data_types.is_empty() {
            return;
        }

        for point in payload.data {
            let value = match price_from_json_number("value", &point.value) {
                Ok(value) => value,
                Err(e) => {
                    log::error!("Failed to parse RTDS equity subscribe value: {e}");
                    continue;
                }
            };

            let ts_event = UnixNanos::from_millis(point.timestamp);
            let ts_init = self.inner.clock.get_time_ns();
            let custom_payload = Arc::new(PolymarketRtdsEquityPrice::new(
                symbol_lower.clone(),
                value,
                value,
                point.timestamp,
                envelope.timestamp,
                None,
                false,
                ts_event,
                ts_init,
            ));

            self.emit_custom_payload(&custom_payload, data_types.clone());
        }
    }

    fn emit_custom_payload<T>(&self, payload: &Arc<T>, data_types: Vec<DataType>)
    where
        T: CustomDataTrait + 'static,
    {
        for data_type in data_types {
            let custom = CustomData::new(payload.clone(), data_type);

            if let Err(e) = self
                .inner
                .data_sender
                .send(DataEvent::Data(NautilusData::Custom(custom)))
            {
                log::error!("Failed to emit RTDS custom data: {e}");
            }
        }
    }

    fn matching_data_types(&self, topic: RtdsTopic, symbol_lower: &str) -> Vec<DataType> {
        let key = tracked_key(topic.as_str(), symbol_lower);
        self.inner
            .subscriptions
            .get(&key)
            .map(|entry| {
                entry
                    .data_types
                    .values()
                    .map(|tracked| tracked.data_type.clone())
                    .collect()
            })
            .unwrap_or_default()
    }
}

impl ParsedSubscription {
    fn from_data_type(data_type: &DataType) -> anyhow::Result<Self> {
        let type_name = data_type.type_name();
        let metadata_binding = data_type.metadata();
        let metadata = metadata_binding.as_ref().context(format!(
            "{type_name} subscriptions require metadata['symbol']"
        ))?;
        let symbol_value = metadata
            .get("symbol")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .context(format!(
                "{type_name} subscriptions require metadata['symbol']"
            ))?;
        let symbol_lower = symbol_value.to_ascii_lowercase();

        match type_name {
            POLYMARKET_RTDS_CRYPTO_PRICE_TYPE_NAME => Ok(Self {
                key: tracked_key(RtdsTopic::CryptoPrices.as_str(), &symbol_lower),
                wire: RtdsWireSubscription {
                    topic: RtdsTopic::CryptoPrices.as_str(),
                    msg_type: "update",
                    // Live RTDS currently accepts the crypto filter in the same JSON-string
                    // shape as equity filters; regression tests lock this wire format in.
                    filters: Some(format!(
                        r#"{{"symbol":"{}"}}"#,
                        symbol_lower.to_ascii_uppercase()
                    )),
                },
            }),
            POLYMARKET_RTDS_EQUITY_PRICE_TYPE_NAME => Ok(Self {
                key: tracked_key(RtdsTopic::EquityPrices.as_str(), &symbol_lower),
                wire: RtdsWireSubscription {
                    topic: RtdsTopic::EquityPrices.as_str(),
                    msg_type: "update",
                    filters: Some(format!(
                        r#"{{"symbol":"{}"}}"#,
                        symbol_lower.to_ascii_uppercase()
                    )),
                },
            }),
            other => anyhow::bail!("Unsupported RTDS custom data type: {other}"),
        }
    }
}

fn tracked_key(topic: &str, symbol_lower: &str) -> String {
    format!("{topic}:{symbol_lower}")
}

fn price_from_json_number(field: &str, number: &Number) -> anyhow::Result<Price> {
    let value = number.to_string();
    price_from_str(field, &value)
}

fn price_from_str(field: &str, value: &str) -> anyhow::Result<Price> {
    Price::from_str(value)
        .map_err(anyhow::Error::msg)
        .with_context(|| format!("invalid price for {field}: {value}"))
}

#[cfg(test)]
mod tests {
    use nautilus_common::messages::DataEvent;
    use nautilus_core::{Params, time::get_atomic_clock_realtime};
    use rstest::rstest;
    use serde_json::json;

    use super::*;

    const RTDS_CRYPTO_UPDATE_FIXTURE: &str =
        include_str!("../test_data/rtds_crypto_prices_update.json");
    const RTDS_CRYPTO_SUBSCRIBE_FIXTURE: &str =
        include_str!("../test_data/rtds_crypto_prices_subscribe.json");
    const RTDS_EQUITY_UPDATE_FIXTURE: &str =
        include_str!("../test_data/rtds_equity_prices_update.json");
    const RTDS_EQUITY_SUBSCRIBE_FIXTURE: &str =
        include_str!("../test_data/rtds_equity_prices_subscribe.json");

    fn crypto_data_type(symbol: &str) -> DataType {
        let mut metadata = Params::new();
        metadata.insert("symbol".to_string(), json!(symbol));
        DataType::new(POLYMARKET_RTDS_CRYPTO_PRICE_TYPE_NAME, Some(metadata), None)
    }

    fn equity_data_type(symbol: &str) -> DataType {
        let mut metadata = Params::new();
        metadata.insert("symbol".to_string(), json!(symbol));
        DataType::new(POLYMARKET_RTDS_EQUITY_PRICE_TYPE_NAME, Some(metadata), None)
    }

    fn make_feed() -> (
        PolymarketRtdsFeed,
        tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let feed = PolymarketRtdsFeed::new(
            "ws://localhost/rtds".to_string(),
            TransportBackend::default(),
            get_atomic_clock_realtime(),
            tx,
        );
        (feed, rx)
    }

    #[rstest]
    fn test_track_subscribe_reuses_symbol_wire_subscription() {
        let (feed, _rx) = make_feed();
        let first_wire = feed
            .track_subscribe(crypto_data_type("BTCUSDT"))
            .expect("track first");
        let second_wire = feed
            .track_subscribe(crypto_data_type("btcusdt"))
            .expect("track second");

        assert_eq!(feed.tracked_subscription_count(), 1);
        assert_eq!(
            feed.tracked_data_type_count("crypto_prices:btcusdt"),
            2,
            "distinct DataType topics should share one wire subscription",
        );
        assert_eq!(
            first_wire.and_then(|wire| wire.filters),
            Some(r#"{"symbol":"BTCUSDT"}"#.to_string())
        );
        assert!(second_wire.is_none());
    }

    #[rstest]
    fn test_track_subscribe_returns_new_symbol_wire_subscription() {
        let (feed, _rx) = make_feed();
        feed.track_subscribe(crypto_data_type("BTCUSDT"))
            .expect("track first symbol");

        let wire = feed
            .track_subscribe(crypto_data_type("ETHUSDT"))
            .expect("track second symbol")
            .expect("wire subscription");

        assert_eq!(wire.topic, "crypto_prices");
        assert_eq!(wire.msg_type, "update");
        assert_eq!(wire.filters.as_deref(), Some(r#"{"symbol":"ETHUSDT"}"#));
    }

    #[rstest]
    fn test_handle_crypto_price_update_emits_custom_data() {
        let (feed, mut rx) = make_feed();
        let data_type = crypto_data_type("btcusdt");
        feed.track_subscribe(data_type.clone())
            .expect("track subscribe");

        feed.handle_text_for_test(RTDS_CRYPTO_UPDATE_FIXTURE);

        let event = rx.try_recv().expect("custom data event");
        let DataEvent::Data(NautilusData::Custom(custom)) = event else {
            panic!("expected custom data event");
        };
        let payload = custom
            .data
            .as_any()
            .downcast_ref::<PolymarketRtdsCryptoPrice>()
            .expect("PolymarketRtdsCryptoPrice");

        assert_eq!(custom.data_type, data_type);
        assert_eq!(payload.symbol, "btcusdt");
        assert_eq!(payload.value, Price::from("61035.86"));
        assert_eq!(payload.price_timestamp_ms, 1780730269000);
        assert_eq!(payload.message_timestamp_ms, 1780730269142);
    }

    #[rstest]
    fn test_handle_crypto_price_subscribe_emits_snapshot_custom_data() {
        let (feed, mut rx) = make_feed();
        let data_type = crypto_data_type("BTCUSDT");
        feed.track_subscribe(data_type.clone())
            .expect("track subscribe");

        feed.handle_text_for_test(RTDS_CRYPTO_SUBSCRIBE_FIXTURE);

        let first = rx.try_recv().expect("first custom data event");
        let second = rx.try_recv().expect("second custom data event");
        let third = rx.try_recv().expect("third custom data event");

        for (event, expected_ts, expected_value) in [
            (first, 1780726209000_u64, "61164.12"),
            (second, 1780726210000_u64, "61161.07"),
            (third, 1780726211000_u64, "61150.89"),
        ] {
            let DataEvent::Data(NautilusData::Custom(custom)) = event else {
                panic!("expected custom data event");
            };
            let payload = custom
                .data
                .as_any()
                .downcast_ref::<PolymarketRtdsCryptoPrice>()
                .expect("PolymarketRtdsCryptoPrice");

            assert_eq!(custom.data_type, data_type);
            assert_eq!(payload.symbol, "btcusdt");
            assert_eq!(payload.value, Price::from(expected_value));
            assert_eq!(payload.price_timestamp_ms, expected_ts);
            assert_eq!(payload.message_timestamp_ms, 1780726213178);
        }
    }

    #[rstest]
    fn test_handle_equity_price_update_emits_custom_data() {
        let (feed, mut rx) = make_feed();
        let data_type = equity_data_type("AAPL");
        feed.track_subscribe(data_type.clone())
            .expect("track subscribe");

        feed.handle_text_for_test(RTDS_EQUITY_UPDATE_FIXTURE);

        let event = rx.try_recv().expect("custom data event");
        let DataEvent::Data(NautilusData::Custom(custom)) = event else {
            panic!("expected custom data event");
        };
        let payload = custom
            .data
            .as_any()
            .downcast_ref::<PolymarketRtdsEquityPrice>()
            .expect("PolymarketRtdsEquityPrice");

        assert_eq!(custom.data_type, data_type);
        assert_eq!(payload.symbol, "aapl");
        assert_eq!(payload.value, Price::from("198.45"));
        assert_eq!(payload.full_accuracy_value, Price::from("198.4523"));
        assert_eq!(payload.received_at_ms, Some(1711382400005));
        assert!(!payload.is_carried_forward);
    }

    #[rstest]
    fn test_handle_equity_price_subscribe_emits_snapshot_custom_data() {
        let (feed, mut rx) = make_feed();
        let data_type = equity_data_type("AAPL");
        feed.track_subscribe(data_type.clone())
            .expect("track subscribe");

        feed.handle_text_for_test(RTDS_EQUITY_SUBSCRIBE_FIXTURE);

        let first = rx.try_recv().expect("first custom data event");
        let second = rx.try_recv().expect("second custom data event");
        let third = rx.try_recv().expect("third custom data event");

        for (event, expected_ts, expected_value) in [
            (first, 1780907777000_u64, "307.91499"),
            (second, 1780907778000_u64, "307.91578"),
            (third, 1780907779000_u64, "307.91547"),
        ] {
            let DataEvent::Data(NautilusData::Custom(custom)) = event else {
                panic!("expected custom data event");
            };
            let payload = custom
                .data
                .as_any()
                .downcast_ref::<PolymarketRtdsEquityPrice>()
                .expect("PolymarketRtdsEquityPrice");

            assert_eq!(custom.data_type, data_type);
            assert_eq!(payload.symbol, "aapl");
            assert_eq!(payload.value, Price::from(expected_value));
            assert_eq!(payload.full_accuracy_value, Price::from(expected_value));
            assert_eq!(payload.price_timestamp_ms, expected_ts);
            assert_eq!(payload.message_timestamp_ms, 1780907896598);
            assert_eq!(payload.received_at_ms, None);
            assert!(!payload.is_carried_forward);
        }
    }

    #[rstest]
    fn test_track_unsubscribe_removes_last_symbol_reference() {
        let (feed, _rx) = make_feed();
        let data_type = equity_data_type("AAPL");
        feed.track_subscribe(data_type.clone())
            .expect("track subscribe");

        let wire = feed
            .track_unsubscribe(&data_type)
            .expect("track unsubscribe")
            .expect("wire unsubscribe");

        assert_eq!(feed.tracked_subscription_count(), 0);
        assert_eq!(wire.topic, "equity_prices");
        assert_eq!(wire.filters.as_deref(), Some(r#"{"symbol":"AAPL"}"#));
    }
}
