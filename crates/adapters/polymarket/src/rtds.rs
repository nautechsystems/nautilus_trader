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
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, Ordering},
    },
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
use nautilus_network::RECONNECTED;
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
    last_emitted_timestamps_ms: dashmap::DashMap<String, u64>,
    // Tracks the last venue state we successfully pushed so incremental syncs
    // can send only the delta from desired state to live wire state.
    live_subscriptions: StdMutex<AHashMap<String, RtdsWireSubscription>>,
    ws_client: StdMutex<Option<Arc<WebSocketClient>>>,
    message_task_handle: StdMutex<Option<tokio::task::JoinHandle<()>>>,
    reconcile_task_handle: StdMutex<Option<tokio::task::JoinHandle<()>>>,
    wire_mutex: tokio::sync::Mutex<()>,
    reconcile_notify: tokio::sync::Notify,
    reconcile_pending: AtomicBool,
    reset_live_state_pending: AtomicBool,
    closing: AtomicBool,
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

#[derive(Clone, Copy)]
enum TimestampGuard {
    // Snapshots can replay, so drop points at or before the high-water mark.
    Snapshot,
    // Live updates never replay, so drop only strictly-older points.
    Live,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ReconcileReason {
    DesiredChanged,
    EnsureConnected,
    TransportReset,
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
                last_emitted_timestamps_ms: dashmap::DashMap::new(),
                live_subscriptions: StdMutex::new(AHashMap::new()),
                ws_client: StdMutex::new(None),
                message_task_handle: StdMutex::new(None),
                reconcile_task_handle: StdMutex::new(None),
                wire_mutex: tokio::sync::Mutex::new(()),
                reconcile_notify: tokio::sync::Notify::new(),
                reconcile_pending: AtomicBool::new(false),
                reset_live_state_pending: AtomicBool::new(false),
                closing: AtomicBool::new(false),
            }),
        }
    }

    pub(crate) fn has_subscriptions(&self) -> bool {
        !self.inner.subscriptions.is_empty()
    }

    pub(crate) fn track_subscribe(&self, data_type: DataType) -> anyhow::Result<bool> {
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

        Ok(should_send_wire)
    }

    pub(crate) fn track_unsubscribe(&self, data_type: &DataType) -> anyhow::Result<bool> {
        let parsed = ParsedSubscription::from_data_type(data_type)?;
        let mut entry = match self.inner.subscriptions.get_mut(&parsed.key) {
            Some(entry) => entry,
            None => return Ok(false),
        };

        let data_type_key = data_type.topic().to_string();
        let Some(tracked) = entry.data_types.get_mut(&data_type_key) else {
            return Ok(false);
        };

        if tracked.ref_count > 1 {
            tracked.ref_count -= 1;
        } else {
            entry.data_types.remove(&data_type_key);
        }

        if entry.total_ref_count > 1 {
            entry.total_ref_count -= 1;
            return Ok(false);
        }

        drop(entry);
        self.inner.subscriptions.remove(&parsed.key);
        self.inner.last_emitted_timestamps_ms.remove(&parsed.key);
        Ok(true)
    }

    pub(crate) async fn connect(&self) -> anyhow::Result<()> {
        self.inner.closing.store(false, Ordering::Release);
        self.ensure_reconcile_worker();
        self.reconcile_once(false).await
    }

    pub(crate) fn request_reconcile(&self, reason: ReconcileReason) {
        if self.inner.closing.load(Ordering::Acquire) {
            return;
        }

        if !self.has_subscriptions() && self.current_ws().is_none() {
            return;
        }

        if matches!(reason, ReconcileReason::TransportReset) {
            self.inner
                .reset_live_state_pending
                .store(true, Ordering::Release);
        }

        self.inner.reconcile_pending.store(true, Ordering::Release);
        self.ensure_reconcile_worker();
        self.inner.reconcile_notify.notify_one();
    }

    pub(crate) async fn disconnect(&self) {
        let _guard = self.inner.wire_mutex.lock().await;

        self.inner.closing.store(true, Ordering::Release);
        self.inner.reconcile_pending.store(false, Ordering::Release);
        self.inner
            .reset_live_state_pending
            .store(false, Ordering::Release);
        self.inner.reconcile_notify.notify_waiters();

        let ws = self
            .inner
            .ws_client
            .lock()
            .expect("RTDS ws_client mutex poisoned")
            .take();
        let message_handle = self
            .inner
            .message_task_handle
            .lock()
            .expect("RTDS message_task_handle mutex poisoned")
            .take();
        let reconcile_handle = self
            .inner
            .reconcile_task_handle
            .lock()
            .expect("RTDS reconcile_task_handle mutex poisoned")
            .take();

        if let Some(ws) = ws {
            ws.disconnect().await;
        }

        self.inner
            .live_subscriptions
            .lock()
            .expect("RTDS live_subscriptions mutex poisoned")
            .clear();

        drop(_guard);

        if let Some(handle) = message_handle {
            await_task_shutdown(handle, "RTDS message loop").await;
        }

        if let Some(handle) = reconcile_handle {
            await_task_shutdown(handle, "RTDS reconcile worker").await;
        }
    }

    pub(crate) fn abort(&self) {
        self.inner.closing.store(true, Ordering::Release);
        self.inner.reconcile_pending.store(false, Ordering::Release);
        self.inner
            .reset_live_state_pending
            .store(false, Ordering::Release);

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

        self.inner
            .live_subscriptions
            .lock()
            .expect("RTDS live_subscriptions mutex poisoned")
            .clear();

        if let Some(handle) = self
            .inner
            .message_task_handle
            .lock()
            .expect("RTDS message_task_handle mutex poisoned")
            .take()
        {
            handle.abort();
        }

        if let Some(handle) = self
            .inner
            .reconcile_task_handle
            .lock()
            .expect("RTDS reconcile_task_handle mutex poisoned")
            .take()
        {
            handle.abort();
        }
    }

    pub(crate) fn needs_connection_recovery(&self) -> bool {
        if self.inner.closing.load(Ordering::Acquire) || !self.has_subscriptions() {
            return false;
        }

        match self.current_ws() {
            None => true,
            Some(ws) => ws.is_disconnected(),
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

    fn clear_ws_if_current(&self, ws: &Arc<WebSocketClient>) -> bool {
        let mut guard = self
            .inner
            .ws_client
            .lock()
            .expect("RTDS ws_client mutex poisoned");
        let Some(current) = guard.as_ref() else {
            return false;
        };

        if !Arc::ptr_eq(current, ws) {
            return false;
        }

        *guard = None;
        true
    }

    fn ensure_reconcile_worker(&self) {
        let mut guard = self
            .inner
            .reconcile_task_handle
            .lock()
            .expect("RTDS reconcile_task_handle mutex poisoned");

        if self.inner.closing.load(Ordering::Acquire) {
            return;
        }

        if guard.as_ref().is_some_and(|handle| !handle.is_finished()) {
            return;
        }

        let feed = self.clone();
        *guard = Some(get_runtime().spawn(async move {
            feed.run_reconcile_loop().await;
        }));
    }

    async fn run_reconcile_loop(&self) {
        loop {
            self.inner.reconcile_notify.notified().await;

            if self.inner.closing.load(Ordering::Acquire) {
                break;
            }

            while self.inner.reconcile_pending.swap(false, Ordering::AcqRel) {
                let reset_live_state = self
                    .inner
                    .reset_live_state_pending
                    .swap(false, Ordering::AcqRel);

                if let Err(e) = self.reconcile_once(reset_live_state).await {
                    log::error!("Failed to reconcile RTDS custom data subscriptions: {e}");
                }
            }
        }
    }

    async fn reconcile_once(&self, reset_live_state: bool) -> anyhow::Result<()> {
        let _guard = self.inner.wire_mutex.lock().await;

        if self.inner.closing.load(Ordering::Acquire) {
            return Ok(());
        }

        if !self.has_subscriptions() && self.current_ws().is_none() {
            return Ok(());
        }

        let fresh_connect = self.ensure_connected_locked().await?;
        let Some(ws) = self.current_ws() else {
            anyhow::bail!("RTDS WebSocket client unavailable after reconcile");
        };

        self.reconcile_live_locked(&ws, fresh_connect || reset_live_state)
            .await
    }

    async fn ensure_connected_locked(&self) -> anyhow::Result<bool> {
        if self.inner.closing.load(Ordering::Acquire) {
            return Ok(false);
        }

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
        log::debug!("Polymarket RTDS WebSocket connected: {}", self.inner.url);

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
            .message_task_handle
            .lock()
            .expect("RTDS message_task_handle mutex poisoned")
            .replace(handle)
        {
            old_handle.abort();
        }

        Ok(true)
    }

    fn snapshot_wire_subscriptions(&self) -> AHashMap<String, RtdsWireSubscription> {
        let mut snapshot = AHashMap::new();

        for entry in &self.inner.subscriptions {
            if topic_uses_topic_level_wire_subscription(entry.wire.topic) {
                snapshot
                    .entry(entry.wire.topic.to_string())
                    .or_insert_with(|| RtdsWireSubscription {
                        topic: entry.wire.topic,
                        msg_type: entry.wire.msg_type,
                        filters: None,
                    });
            } else {
                snapshot.insert(entry.key().clone(), entry.wire.clone());
            }
        }

        snapshot
    }

    async fn reconcile_live_locked(
        &self,
        ws: &Arc<WebSocketClient>,
        reset_live_state: bool,
    ) -> anyhow::Result<()> {
        if !ws.is_active() {
            return Ok(());
        }

        if reset_live_state {
            self.inner
                .live_subscriptions
                .lock()
                .expect("RTDS live_subscriptions mutex poisoned")
                .clear();
        }

        let desired = self.snapshot_wire_subscriptions();
        let (unsubscribe, subscribe) = {
            let live = self
                .inner
                .live_subscriptions
                .lock()
                .expect("RTDS live_subscriptions mutex poisoned");
            let unsubscribe = live
                .iter()
                .filter(|(key, _)| !desired.contains_key(*key))
                .map(|(_, wire)| wire.clone())
                .collect::<Vec<_>>();
            let subscribe = desired
                .iter()
                .filter(|(key, _)| !live.contains_key(*key))
                .map(|(_, wire)| wire.clone())
                .collect::<Vec<_>>();
            (unsubscribe, subscribe)
        };

        if !unsubscribe.is_empty() {
            log::debug!(
                "Unsubscribing Polymarket RTDS delta with {} subscription(s)",
                unsubscribe.len()
            );
            self.send_wire_request(ws, "unsubscribe", unsubscribe)
                .await?;
        }

        if !subscribe.is_empty() {
            log::debug!(
                "Subscribing Polymarket RTDS delta with {} subscription(s)",
                subscribe.len()
            );
            self.send_wire_request(ws, "subscribe", subscribe).await?;
        }

        let mut live = self
            .inner
            .live_subscriptions
            .lock()
            .expect("RTDS live_subscriptions mutex poisoned");
        live.retain(|key, _| desired.contains_key(key));
        for (key, wire) in desired {
            live.insert(key, wire);
        }

        Ok(())
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
                    if text.as_str() == RECONNECTED {
                        log::info!("Polymarket RTDS reconnected");
                        self.request_reconcile(ReconcileReason::TransportReset);
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

                    if self.clear_ws_if_current(&ws) {
                        self.request_reconcile(ReconcileReason::TransportReset);
                    }
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

        if !self.should_emit_timestamp_ms(
            RtdsTopic::CryptoPrices,
            &symbol_lower,
            payload.timestamp,
            TimestampGuard::Live,
        ) {
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

            if !self.should_emit_timestamp_ms(
                RtdsTopic::CryptoPrices,
                &symbol_lower,
                point.timestamp,
                TimestampGuard::Snapshot,
            ) {
                continue;
            }

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

        if !self.should_emit_timestamp_ms(
            RtdsTopic::EquityPrices,
            &symbol_lower,
            payload.timestamp,
            TimestampGuard::Live,
        ) {
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

            if !self.should_emit_timestamp_ms(
                RtdsTopic::EquityPrices,
                &symbol_lower,
                point.timestamp,
                TimestampGuard::Snapshot,
            ) {
                continue;
            }

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

    fn should_emit_timestamp_ms(
        &self,
        topic: RtdsTopic,
        symbol_lower: &str,
        timestamp_ms: u64,
        guard: TimestampGuard,
    ) -> bool {
        let key = tracked_key(topic.as_str(), symbol_lower);
        match self.inner.last_emitted_timestamps_ms.get_mut(&key) {
            Some(mut last_seen) => {
                let stale = match guard {
                    TimestampGuard::Snapshot => timestamp_ms <= *last_seen,
                    TimestampGuard::Live => timestamp_ms < *last_seen,
                };

                if stale {
                    false
                } else {
                    if timestamp_ms > *last_seen {
                        *last_seen = timestamp_ms;
                    }
                    true
                }
            }
            None => {
                self.inner
                    .last_emitted_timestamps_ms
                    .insert(key, timestamp_ms);
                true
            }
        }
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
                    // The current RTDS backend reliably supports unfiltered topic-level
                    // crypto subscriptions; we keep logical subscriptions per symbol and
                    // filter locally on emit.
                    filters: None,
                },
            }),
            POLYMARKET_RTDS_EQUITY_PRICE_TYPE_NAME => Ok(Self {
                key: tracked_key(RtdsTopic::EquityPrices.as_str(), &symbol_lower),
                wire: RtdsWireSubscription {
                    topic: RtdsTopic::EquityPrices.as_str(),
                    msg_type: "update",
                    // The live RTDS backend also starves later per-symbol equity
                    // subscriptions, so we keep logical subscriptions per symbol and
                    // aggregate transport state at the topic boundary.
                    filters: None,
                },
            }),
            other => anyhow::bail!("Unsupported RTDS custom data type: {other}"),
        }
    }
}

fn tracked_key(topic: &str, symbol_lower: &str) -> String {
    format!("{topic}:{symbol_lower}")
}

fn topic_uses_topic_level_wire_subscription(topic: &str) -> bool {
    matches!(topic, "crypto_prices" | "equity_prices")
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

async fn await_task_shutdown(handle: tokio::task::JoinHandle<()>, description: &str) {
    let abort_handle = handle.abort_handle();
    tokio::select! {
        result = handle => {
            if let Err(e) = result
                && !e.is_cancelled()
            {
                log::error!("{description} error: {e:?}");
            }
        }
        () = tokio::time::sleep(Duration::from_secs(2)) => {
            abort_handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::SocketAddr,
        sync::{Arc, atomic::Ordering},
        time::Duration,
    };

    use axum::{
        Router,
        extract::{
            State,
            ws::{Message as AxumWsMessage, WebSocket, WebSocketUpgrade},
        },
        response::Response,
        routing::get,
    };
    use futures_util::StreamExt;
    use nautilus_common::{messages::DataEvent, testing::wait_until_async};
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

    #[derive(Clone, Default)]
    struct TestServerState {
        received_payloads: Arc<tokio::sync::Mutex<Vec<serde_json::Value>>>,
    }

    async fn handle_rtds_upgrade(
        ws: WebSocketUpgrade,
        State(state): State<TestServerState>,
    ) -> Response {
        ws.on_upgrade(move |socket| handle_rtds_socket(socket, state))
    }

    async fn handle_rtds_socket(mut socket: WebSocket, state: TestServerState) {
        while let Some(result) = socket.next().await {
            let Ok(message) = result else { break };

            match message {
                AxumWsMessage::Text(text) => {
                    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&text) else {
                        continue;
                    };
                    state.received_payloads.lock().await.push(payload);
                }
                AxumWsMessage::Ping(data) => {
                    if socket.send(AxumWsMessage::Pong(data)).await.is_err() {
                        break;
                    }
                }
                AxumWsMessage::Close(_) => break,
                _ => {}
            }
        }
    }

    async fn start_rtds_server(state: TestServerState) -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("failed to bind RTDS test server");
        let addr = listener
            .local_addr()
            .expect("missing RTDS test server address");
        let router = Router::new()
            .route("/rtds", get(handle_rtds_upgrade))
            .with_state(state);

        tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("RTDS test server failed");
        });

        tokio::time::sleep(Duration::from_millis(50)).await;
        addr
    }

    async fn connect_test_ws(url: String) -> Arc<WebSocketClient> {
        let (handler, _raw_rx) = channel_message_handler();
        Arc::new(
            WebSocketClient::connect(
                WebSocketConfig {
                    url,
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
                    backend: TransportBackend::default(),
                    proxy_url: None,
                },
                Some(handler),
                None,
                None,
                vec![],
                None,
            )
            .await
            .expect("connect test ws"),
        )
    }

    #[rstest]
    fn test_track_subscribe_reuses_symbol_wire_subscription() {
        let (feed, _rx) = make_feed();
        let first_changed = feed
            .track_subscribe(crypto_data_type("BTCUSDT"))
            .expect("track first");
        let second_changed = feed
            .track_subscribe(crypto_data_type("btcusdt"))
            .expect("track second");

        assert_eq!(feed.tracked_subscription_count(), 1);
        assert_eq!(
            feed.tracked_data_type_count("crypto_prices:btcusdt"),
            2,
            "distinct DataType topics should share one wire subscription",
        );
        assert!(first_changed);
        assert!(!second_changed);
    }

    #[rstest]
    fn test_track_subscribe_returns_changed_for_new_symbol() {
        let (feed, _rx) = make_feed();
        feed.track_subscribe(crypto_data_type("BTCUSDT"))
            .expect("track first symbol");

        let changed = feed
            .track_subscribe(crypto_data_type("ETHUSDT"))
            .expect("track second symbol");

        assert!(changed);
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
    fn test_handle_crypto_price_update_emits_distinct_same_millisecond_points() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_data_type("btcusdt"))
            .expect("track subscribe");

        // Two distinct live updates sharing one millisecond timestamp must both emit;
        // only replayed snapshots collapse equal timestamps.
        feed.handle_text_for_test(RTDS_CRYPTO_UPDATE_FIXTURE);

        let mut second: serde_json::Value =
            serde_json::from_str(RTDS_CRYPTO_UPDATE_FIXTURE).expect("parse fixture");
        second["payload"]["value"] = json!(61040.12);
        feed.handle_text_for_test(&second.to_string());

        let first_event = rx.try_recv().expect("first custom data event");
        let second_event = rx.try_recv().expect("second custom data event");
        assert!(rx.try_recv().is_err());

        for (event, expected_value) in [(first_event, "61035.86"), (second_event, "61040.12")] {
            let DataEvent::Data(NautilusData::Custom(custom)) = event else {
                panic!("expected custom data event");
            };
            let payload = custom
                .data
                .as_any()
                .downcast_ref::<PolymarketRtdsCryptoPrice>()
                .expect("PolymarketRtdsCryptoPrice");

            assert_eq!(payload.value, Price::from(expected_value));
            assert_eq!(payload.price_timestamp_ms, 1780730269000);
        }
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
    fn test_handle_crypto_price_subscribe_skips_duplicate_snapshot_points() {
        let (feed, mut rx) = make_feed();
        let data_type = crypto_data_type("BTCUSDT");
        feed.track_subscribe(data_type).expect("track subscribe");

        feed.handle_text_for_test(RTDS_CRYPTO_SUBSCRIBE_FIXTURE);

        while rx.try_recv().is_ok() {}

        feed.handle_text_for_test(RTDS_CRYPTO_SUBSCRIBE_FIXTURE);

        assert!(rx.try_recv().is_err());
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
    fn test_handle_equity_price_subscribe_skips_duplicate_snapshot_points() {
        let (feed, mut rx) = make_feed();
        let data_type = equity_data_type("AAPL");
        feed.track_subscribe(data_type).expect("track subscribe");

        feed.handle_text_for_test(RTDS_EQUITY_SUBSCRIBE_FIXTURE);

        while rx.try_recv().is_ok() {}

        feed.handle_text_for_test(RTDS_EQUITY_SUBSCRIBE_FIXTURE);

        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_track_unsubscribe_removes_last_symbol_reference() {
        let (feed, _rx) = make_feed();
        let data_type = equity_data_type("AAPL");
        assert!(
            feed.track_subscribe(data_type.clone())
                .expect("track subscribe")
        );

        assert!(
            feed.track_unsubscribe(&data_type)
                .expect("track unsubscribe")
        );
        assert_eq!(feed.tracked_subscription_count(), 0);
        assert!(
            !feed
                .track_unsubscribe(&data_type)
                .expect("repeat unsubscribe should no-op")
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_incremental_sync_subscribes_only_new_symbol_while_connected() {
        let state = TestServerState::default();
        let addr = start_rtds_server(state.clone()).await;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let feed = PolymarketRtdsFeed::new(
            format!("ws://{addr}/rtds"),
            TransportBackend::default(),
            get_atomic_clock_realtime(),
            tx,
        );

        feed.track_subscribe(crypto_data_type("BTCUSDT"))
            .expect("track BTC");
        feed.connect().await.expect("connect feed");

        wait_until_async(
            || {
                let state = state.clone();
                async move { !state.received_payloads.lock().await.is_empty() }
            },
            Duration::from_secs(2),
        )
        .await;

        state.received_payloads.lock().await.clear();

        assert!(
            feed.track_subscribe(crypto_data_type("ETHUSDT"))
                .expect("track ETH")
        );
        feed.reconcile_once(false).await.expect("reconcile live");
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert!(
            state.received_payloads.lock().await.is_empty(),
            "adding another crypto symbol should reuse the existing topic-level RTDS subscription",
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_incremental_sync_subscribes_only_new_equity_symbol_while_connected() {
        let state = TestServerState::default();
        let addr = start_rtds_server(state.clone()).await;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let feed = PolymarketRtdsFeed::new(
            format!("ws://{addr}/rtds"),
            TransportBackend::default(),
            get_atomic_clock_realtime(),
            tx,
        );

        feed.track_subscribe(equity_data_type("AAPL"))
            .expect("track AAPL");
        feed.connect().await.expect("connect feed");

        wait_until_async(
            || {
                let state = state.clone();
                async move { !state.received_payloads.lock().await.is_empty() }
            },
            Duration::from_secs(2),
        )
        .await;

        state.received_payloads.lock().await.clear();

        assert!(
            feed.track_subscribe(equity_data_type("MSFT"))
                .expect("track MSFT")
        );
        feed.reconcile_once(false).await.expect("reconcile live");
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert!(
            state.received_payloads.lock().await.is_empty(),
            "adding another equity symbol should reuse the existing topic-level RTDS subscription",
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_incremental_sync_keeps_crypto_topic_subscribed_while_other_symbols_remain() {
        let state = TestServerState::default();
        let addr = start_rtds_server(state.clone()).await;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let feed = PolymarketRtdsFeed::new(
            format!("ws://{addr}/rtds"),
            TransportBackend::default(),
            get_atomic_clock_realtime(),
            tx,
        );

        let btc = crypto_data_type("BTCUSDT");
        let eth = crypto_data_type("ETHUSDT");
        assert!(feed.track_subscribe(btc).expect("track BTC"));
        assert!(feed.track_subscribe(eth.clone()).expect("track ETH"));
        feed.connect().await.expect("connect feed");

        wait_until_async(
            || {
                let state = state.clone();
                async move { !state.received_payloads.lock().await.is_empty() }
            },
            Duration::from_secs(2),
        )
        .await;

        state.received_payloads.lock().await.clear();

        assert!(feed.track_unsubscribe(&eth).expect("track unsubscribe"));
        feed.reconcile_once(false).await.expect("reconcile live");
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert!(
            state.received_payloads.lock().await.is_empty(),
            "removing one crypto symbol should keep the topic-level RTDS subscription alive while BTC remains",
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_incremental_sync_keeps_equity_topic_subscribed_while_other_symbols_remain() {
        let state = TestServerState::default();
        let addr = start_rtds_server(state.clone()).await;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let feed = PolymarketRtdsFeed::new(
            format!("ws://{addr}/rtds"),
            TransportBackend::default(),
            get_atomic_clock_realtime(),
            tx,
        );

        let aapl = equity_data_type("AAPL");
        let msft = equity_data_type("MSFT");
        assert!(feed.track_subscribe(aapl).expect("track AAPL"));
        assert!(feed.track_subscribe(msft.clone()).expect("track MSFT"));
        feed.connect().await.expect("connect feed");

        wait_until_async(
            || {
                let state = state.clone();
                async move { !state.received_payloads.lock().await.is_empty() }
            },
            Duration::from_secs(2),
        )
        .await;

        state.received_payloads.lock().await.clear();

        assert!(feed.track_unsubscribe(&msft).expect("track unsubscribe"));
        feed.reconcile_once(false).await.expect("reconcile live");
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert!(
            state.received_payloads.lock().await.is_empty(),
            "removing one equity symbol should keep the topic-level RTDS subscription alive while AAPL remains",
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_incremental_sync_unsubscribes_crypto_topic_after_last_symbol_removed() {
        let state = TestServerState::default();
        let addr = start_rtds_server(state.clone()).await;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let feed = PolymarketRtdsFeed::new(
            format!("ws://{addr}/rtds"),
            TransportBackend::default(),
            get_atomic_clock_realtime(),
            tx,
        );

        let btc = crypto_data_type("BTCUSDT");
        assert!(feed.track_subscribe(btc.clone()).expect("track BTC"));
        feed.connect().await.expect("connect feed");

        wait_until_async(
            || {
                let state = state.clone();
                async move { !state.received_payloads.lock().await.is_empty() }
            },
            Duration::from_secs(2),
        )
        .await;

        state.received_payloads.lock().await.clear();

        assert!(feed.track_unsubscribe(&btc).expect("track unsubscribe"));
        feed.reconcile_once(false).await.expect("reconcile live");

        wait_until_async(
            || {
                let state = state.clone();
                async move { !state.received_payloads.lock().await.is_empty() }
            },
            Duration::from_secs(2),
        )
        .await;

        let payloads = state.received_payloads.lock().await.clone();
        let unsubscribe = payloads.last().expect("unsubscribe payload");
        assert_eq!(unsubscribe["action"].as_str(), Some("unsubscribe"));
        assert!(
            unsubscribe["subscriptions"][0]["filters"].is_null(),
            "topic-level crypto unsubscribe should omit filters",
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_incremental_sync_unsubscribes_equity_topic_after_last_symbol_removed() {
        let state = TestServerState::default();
        let addr = start_rtds_server(state.clone()).await;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let feed = PolymarketRtdsFeed::new(
            format!("ws://{addr}/rtds"),
            TransportBackend::default(),
            get_atomic_clock_realtime(),
            tx,
        );

        let aapl = equity_data_type("AAPL");
        assert!(feed.track_subscribe(aapl.clone()).expect("track AAPL"));
        feed.connect().await.expect("connect feed");

        wait_until_async(
            || {
                let state = state.clone();
                async move { !state.received_payloads.lock().await.is_empty() }
            },
            Duration::from_secs(2),
        )
        .await;

        state.received_payloads.lock().await.clear();

        assert!(feed.track_unsubscribe(&aapl).expect("track unsubscribe"));
        feed.reconcile_once(false).await.expect("reconcile live");

        wait_until_async(
            || {
                let state = state.clone();
                async move { !state.received_payloads.lock().await.is_empty() }
            },
            Duration::from_secs(2),
        )
        .await;

        let payloads = state.received_payloads.lock().await.clone();
        let unsubscribe = payloads.last().expect("unsubscribe payload");
        assert_eq!(unsubscribe["action"].as_str(), Some("unsubscribe"));
        assert_eq!(
            unsubscribe["subscriptions"][0]["topic"].as_str(),
            Some(RtdsTopic::EquityPrices.as_str()),
        );
        assert!(
            unsubscribe["subscriptions"][0]["filters"].is_null(),
            "topic-level equity unsubscribe should omit filters",
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_reconcile_worker_coalesces_multiple_desired_changes() {
        let state = TestServerState::default();
        let addr = start_rtds_server(state.clone()).await;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let feed = PolymarketRtdsFeed::new(
            format!("ws://{addr}/rtds"),
            TransportBackend::default(),
            get_atomic_clock_realtime(),
            tx,
        );

        feed.track_subscribe(crypto_data_type("BTCUSDT"))
            .expect("track BTC");
        feed.connect().await.expect("connect feed");

        wait_until_async(
            || {
                let state = state.clone();
                async move { !state.received_payloads.lock().await.is_empty() }
            },
            Duration::from_secs(2),
        )
        .await;

        state.received_payloads.lock().await.clear();

        let wire_guard = feed.inner.wire_mutex.lock().await;
        assert!(
            feed.track_subscribe(crypto_data_type("ETHUSDT"))
                .expect("track ETH")
        );
        feed.request_reconcile(ReconcileReason::DesiredChanged);
        assert!(
            feed.track_subscribe(crypto_data_type("SOLUSDT"))
                .expect("track SOL")
        );
        feed.request_reconcile(ReconcileReason::DesiredChanged);
        drop(wire_guard);

        wait_until_async(
            || {
                let state = state.clone();
                async move { state.received_payloads.lock().await.is_empty() }
            },
            Duration::from_secs(2),
        )
        .await;

        tokio::time::sleep(Duration::from_millis(200)).await;

        let payloads = state.received_payloads.lock().await.clone();
        assert!(
            payloads.is_empty(),
            "coalesced crypto desired-state changes should not send a new wire request while the topic-level subscription is already live",
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_reconnected_control_replays_retained_subscriptions() {
        let state = TestServerState::default();
        let addr = start_rtds_server(state.clone()).await;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let feed = PolymarketRtdsFeed::new(
            format!("ws://{addr}/rtds"),
            TransportBackend::default(),
            get_atomic_clock_realtime(),
            tx,
        );

        feed.track_subscribe(crypto_data_type("BTCUSDT"))
            .expect("track BTC");
        feed.track_subscribe(crypto_data_type("ETHUSDT"))
            .expect("track ETH");
        feed.track_subscribe(equity_data_type("AAPL"))
            .expect("track AAPL");
        feed.track_subscribe(equity_data_type("MSFT"))
            .expect("track MSFT");

        let ws = connect_test_ws(format!("ws://{addr}/rtds")).await;
        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();

        let loop_handle = tokio::spawn({
            let feed = feed.clone();
            let ws = ws.clone();
            async move {
                feed.run_message_loop(ws, raw_rx).await;
            }
        });

        raw_tx
            .send(Message::Text(RECONNECTED.into()))
            .expect("send reconnect sentinel");
        drop(raw_tx);

        wait_until_async(
            || {
                let state = state.clone();
                async move { !state.received_payloads.lock().await.is_empty() }
            },
            Duration::from_secs(2),
        )
        .await;

        loop_handle.await.expect("join RTDS loop");

        let payloads = state.received_payloads.lock().await.clone();
        let replay = payloads.last().expect("replay payload");
        let subscriptions = replay["subscriptions"]
            .as_array()
            .expect("subscriptions array");

        assert_eq!(replay["action"].as_str(), Some("subscribe"));
        assert_eq!(subscriptions.len(), 2);

        let mut topics: Vec<_> = subscriptions
            .iter()
            .map(|subscription| {
                (
                    subscription["topic"].as_str().expect("topic"),
                    subscription["filters"].is_null(),
                )
            })
            .collect();
        topics.sort_unstable_by_key(|(topic, _)| *topic);

        assert_eq!(
            topics,
            vec![
                (RtdsTopic::CryptoPrices.as_str(), true),
                (RtdsTopic::EquityPrices.as_str(), true),
            ],
            "reconnect replay should keep both RTDS topics at topic-level subscriptions",
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_channel_close_self_heals_with_retained_subscriptions() {
        let state = TestServerState::default();
        let addr = start_rtds_server(state.clone()).await;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let feed = PolymarketRtdsFeed::new(
            format!("ws://{addr}/rtds"),
            TransportBackend::default(),
            get_atomic_clock_realtime(),
            tx,
        );

        feed.track_subscribe(crypto_data_type("BTCUSDT"))
            .expect("track BTC");

        let ws = connect_test_ws(format!("ws://{addr}/rtds")).await;
        *feed
            .inner
            .ws_client
            .lock()
            .expect("RTDS ws_client mutex poisoned") = Some(ws.clone());

        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();

        let loop_handle = tokio::spawn({
            let feed = feed.clone();
            let ws = ws.clone();
            async move {
                feed.run_message_loop(ws, raw_rx).await;
            }
        });

        drop(raw_tx);

        wait_until_async(
            || {
                let state = state.clone();
                async move { !state.received_payloads.lock().await.is_empty() }
            },
            Duration::from_secs(2),
        )
        .await;

        loop_handle.await.expect("join RTDS loop");

        assert!(
            feed.current_ws()
                .is_some_and(|current| !Arc::ptr_eq(&current, &ws)),
            "channel close should replace the dead RTDS client when retained subscriptions exist",
        );

        let payloads = state.received_payloads.lock().await.clone();
        let replay = payloads.last().expect("recovery payload");
        assert_eq!(replay["action"].as_str(), Some("subscribe"));
    }

    #[rstest]
    #[tokio::test]
    async fn test_reconcile_request_does_not_reconnect_while_closing() {
        let state = TestServerState::default();
        let addr = start_rtds_server(state.clone()).await;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let feed = PolymarketRtdsFeed::new(
            format!("ws://{addr}/rtds"),
            TransportBackend::default(),
            get_atomic_clock_realtime(),
            tx,
        );

        feed.track_subscribe(crypto_data_type("BTCUSDT"))
            .expect("track BTC");
        feed.inner.closing.store(true, Ordering::Release);

        feed.request_reconcile(ReconcileReason::DesiredChanged);
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert!(
            feed.current_ws().is_none(),
            "closing feed should not reconnect"
        );
        assert!(
            state.received_payloads.lock().await.is_empty(),
            "closing feed should not replay subscriptions",
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_ensure_connected_without_retained_subscriptions_does_not_connect() {
        let state = TestServerState::default();
        let addr = start_rtds_server(state.clone()).await;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let feed = PolymarketRtdsFeed::new(
            format!("ws://{addr}/rtds"),
            TransportBackend::default(),
            get_atomic_clock_realtime(),
            tx,
        );

        feed.request_reconcile(ReconcileReason::EnsureConnected);
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert!(
            feed.current_ws().is_none(),
            "ensure-connected without retained subscriptions should not connect",
        );
        assert!(
            state.received_payloads.lock().await.is_empty(),
            "ensure-connected without retained subscriptions should not send wire subscribe",
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_channel_close_does_not_self_heal_while_closing() {
        let state = TestServerState::default();
        let addr = start_rtds_server(state.clone()).await;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let feed = PolymarketRtdsFeed::new(
            format!("ws://{addr}/rtds"),
            TransportBackend::default(),
            get_atomic_clock_realtime(),
            tx,
        );

        feed.track_subscribe(crypto_data_type("BTCUSDT"))
            .expect("track BTC");

        let ws = connect_test_ws(format!("ws://{addr}/rtds")).await;
        *feed
            .inner
            .ws_client
            .lock()
            .expect("RTDS ws_client mutex poisoned") = Some(ws.clone());
        feed.inner.closing.store(true, Ordering::Release);

        let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();

        let loop_handle = tokio::spawn({
            let feed = feed.clone();
            let ws = ws.clone();
            async move {
                feed.run_message_loop(ws, raw_rx).await;
            }
        });

        drop(raw_tx);
        loop_handle.await.expect("join RTDS loop");
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert!(
            feed.current_ws().is_none(),
            "closing feed should not replace a dead RTDS client",
        );
        assert!(
            state.received_payloads.lock().await.is_empty(),
            "closing feed should not issue recovery subscribe",
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_disconnect_cancels_reconcile_worker() {
        let (feed, _rx) = make_feed();
        feed.track_subscribe(crypto_data_type("BTCUSDT"))
            .expect("track BTC");

        feed.request_reconcile(ReconcileReason::DesiredChanged);
        wait_until_async(
            || {
                let feed = feed.clone();
                async move {
                    feed.inner
                        .reconcile_task_handle
                        .lock()
                        .expect("RTDS reconcile_task_handle mutex poisoned")
                        .as_ref()
                        .is_some_and(|handle| !handle.is_finished())
                }
            },
            Duration::from_secs(2),
        )
        .await;

        feed.disconnect().await;

        assert!(
            feed.inner
                .reconcile_task_handle
                .lock()
                .expect("RTDS reconcile_task_handle mutex poisoned")
                .is_none(),
            "disconnect should clear the reconcile worker handle",
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_disconnect_wins_against_pending_reconcile_before_new_ws_is_created() {
        let state = TestServerState::default();
        let addr = start_rtds_server(state.clone()).await;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let feed = PolymarketRtdsFeed::new(
            format!("ws://{addr}/rtds"),
            TransportBackend::default(),
            get_atomic_clock_realtime(),
            tx,
        );

        feed.track_subscribe(crypto_data_type("BTCUSDT"))
            .expect("track BTC");

        let guard = feed.inner.wire_mutex.lock().await;

        let disconnect_task = tokio::spawn({
            let feed = feed.clone();
            async move {
                feed.disconnect().await;
            }
        });

        tokio::time::sleep(Duration::from_millis(50)).await;
        feed.request_reconcile(ReconcileReason::DesiredChanged);
        tokio::time::sleep(Duration::from_millis(50)).await;

        drop(guard);
        disconnect_task.await.expect("join disconnect task");
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert!(
            feed.current_ws().is_none(),
            "disconnect should prevent a pending reconcile from creating a new RTDS client",
        );
        assert!(
            state.received_payloads.lock().await.is_empty(),
            "disconnect should prevent a pending reconcile from sending wire subscriptions",
        );
    }
}
