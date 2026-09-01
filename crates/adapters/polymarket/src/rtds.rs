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
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use ahash::AHashMap;
use anyhow::Context;
#[cfg(test)]
use nautilus_common::live::get_runtime;
use nautilus_common::messages::DataEvent;
use nautilus_core::{UnixNanos, time::AtomicTime};
use nautilus_live::{
    SocketControl,
    task::{TaskJoinOutcome, TaskSlot, finish_task},
};
use nautilus_model::{
    data::{CustomData, Data as NautilusData, DataType, custom::CustomDataTrait},
    types::Price,
};
use nautilus_network::{
    RECONNECTED, SocketStateSink,
    websocket::{
        TransportBackend, WebSocketClient, WebSocketConfig, channel_message_handler,
        proxy::ProxyUrl,
    },
};
use parking_lot::Mutex;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Number;
use tokio_tungstenite::tungstenite::Message;

use crate::{
    common::parse::deserialize_crypto_twap_value,
    data_types::{PolymarketRtdsCryptoPrice, PolymarketRtdsCryptoTwap, PolymarketRtdsEquityPrice},
};

const POLYMARKET_RTDS_HEARTBEAT_SECS: u64 = 5;
// The venue answers each `PING` with a text `PONG`, which refreshes a data-silence
// timer just like real data would. Liveness therefore rests on inbound frames of any
// kind, at six heartbeat cycles.
const POLYMARKET_RTDS_HEARTBEAT_TIMEOUT_SECS: u64 = 30;
const POLYMARKET_RTDS_RECONNECT_TIMEOUT_MS: u64 = 15_000;
const POLYMARKET_RTDS_RECONNECT_DELAY_INITIAL_MS: u64 = 250;
const POLYMARKET_RTDS_RECONNECT_DELAY_MAX_MS: u64 = 5_000;
const POLYMARKET_RTDS_RECONNECT_JITTER_MS: u64 = 200;
const POLYMARKET_RTDS_CRYPTO_PRICE_TYPE_NAME: &str = "PolymarketRtdsCryptoPrice";
const POLYMARKET_RTDS_CRYPTO_TWAP_TYPE_NAME: &str = "PolymarketRtdsCryptoTwap";
const POLYMARKET_RTDS_EQUITY_PRICE_TYPE_NAME: &str = "PolymarketRtdsEquityPrice";

pub(crate) fn is_supported_rtds_data_type(data_type: &DataType) -> bool {
    matches!(
        data_type.type_name(),
        POLYMARKET_RTDS_CRYPTO_PRICE_TYPE_NAME
            | POLYMARKET_RTDS_CRYPTO_TWAP_TYPE_NAME
            | POLYMARKET_RTDS_EQUITY_PRICE_TYPE_NAME
    )
}

#[derive(Clone, Debug)]
pub(crate) struct PolymarketRtdsFeed {
    inner: Arc<PolymarketRtdsFeedInner>,
    _task_owner: Option<Arc<RtdsTaskSlots>>,
    task_slots: Weak<RtdsTaskSlots>,
}

#[derive(Debug)]
struct RtdsTaskSlots {
    message: tokio::sync::Mutex<TaskSlot<()>>,
    reconcile: tokio::sync::Mutex<TaskSlot<()>>,
    shutdown_errors: Mutex<Vec<String>>,
}

struct RtdsWebSocketGuard<'a> {
    owner: &'a Mutex<Option<Arc<WebSocketClient>>>,
    ws: Option<Arc<WebSocketClient>>,
}

impl<'a> RtdsWebSocketGuard<'a> {
    fn take(owner: &'a Mutex<Option<Arc<WebSocketClient>>>) -> Self {
        let ws = owner.lock().take();
        Self { owner, ws }
    }

    fn clear(&mut self) {
        self.ws = None;
    }
}

impl Drop for RtdsWebSocketGuard<'_> {
    fn drop(&mut self) {
        if self.ws.is_some() {
            *self.owner.lock() = self.ws.take();
        }
    }
}

impl RtdsTaskSlots {
    fn push_shutdown_error(&self, error: String) {
        self.shutdown_errors.lock().push(error);
    }

    fn take_shutdown_result(&self) -> anyhow::Result<()> {
        let mut errors = self.shutdown_errors.lock();

        if errors.is_empty() {
            Ok(())
        } else {
            let errors = std::mem::take(&mut *errors);
            anyhow::bail!(
                "Polymarket RTDS task shutdown failed: {}",
                errors.join("; ")
            )
        }
    }
}

impl Drop for RtdsTaskSlots {
    fn drop(&mut self) {
        self.message.get_mut().abort();
        self.reconcile.get_mut().abort();
    }
}

#[derive(Debug)]
struct PolymarketRtdsFeedInner {
    url: String,
    proxy_url: Option<ProxyUrl>,
    transport_backend: TransportBackend,
    clock: &'static AtomicTime,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    socket_sink: Option<SocketStateSink>,
    socket_control: Option<SocketControl>,
    subscriptions: dashmap::DashMap<String, TrackedSubscription>,
    last_emitted_timestamps_ms: dashmap::DashMap<String, u64>,
    // Tracks the last venue state we successfully pushed so incremental syncs
    // can send only the delta from desired state to live wire state.
    live_subscriptions: Mutex<AHashMap<String, RtdsWireSubscription>>,
    ws_client: Mutex<Option<Arc<WebSocketClient>>>,
    wire_mutex: tokio::sync::Mutex<()>,
    reconcile_notify: tokio::sync::Notify,
    reconcile_pending: AtomicBool,
    reset_live_state_pending: AtomicBool,
    closing: AtomicBool,
    shutdown_generation: Mutex<u64>,
}

#[derive(Clone, Copy, Debug)]
struct TwapReplayFingerprint {
    timestamp_ms: u64,
    value: Decimal,
}

#[derive(Clone, Debug)]
struct TrackedSubscription {
    wire: RtdsWireSubscription,
    total_ref_count: usize,
    data_types: AHashMap<String, TrackedDataType>,
    last_twap_fingerprint: Option<TwapReplayFingerprint>,
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
    CryptoPricesTwapThirty,
    CryptoPricesTwapSixty,
    EquityPrices,
}

impl RtdsTopic {
    fn as_str(self) -> &'static str {
        match self {
            Self::CryptoPrices => "crypto_prices",
            Self::CryptoPricesTwapThirty => "crypto_prices_twap_thirty",
            Self::CryptoPricesTwapSixty => "crypto_prices_twap_sixty",
            Self::EquityPrices => "equity_prices",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RtdsCryptoTwapWindow {
    ThirtySeconds,
    SixtySeconds,
}

impl RtdsCryptoTwapWindow {
    const fn seconds(self) -> u32 {
        match self {
            Self::ThirtySeconds => 30,
            Self::SixtySeconds => 60,
        }
    }

    const fn topic(self) -> RtdsTopic {
        match self {
            Self::ThirtySeconds => RtdsTopic::CryptoPricesTwapThirty,
            Self::SixtySeconds => RtdsTopic::CryptoPricesTwapSixty,
        }
    }
}

impl TryFrom<u64> for RtdsCryptoTwapWindow {
    type Error = anyhow::Error;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            30 => Ok(Self::ThirtySeconds),
            60 => Ok(Self::SixtySeconds),
            other => anyhow::bail!(
                "PolymarketRtdsCryptoTwap metadata['window_seconds'] must be 30 or 60, received {other}"
            ),
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
    #[allow(dead_code, reason = "modeled for RTDS envelope conformance")]
    connection_id: Option<String>,
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
struct CryptoTwapPayloadRaw {
    symbol: String,
    timestamp: u64,
    #[serde(rename = "value", deserialize_with = "deserialize_crypto_twap_value")]
    #[allow(
        dead_code,
        reason = "display-only field validated for wire conformance, never published"
    )]
    display_value: Decimal,
    full_accuracy_value: String,
    window_s: u32,
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
    #[serde(default)]
    full_accuracy_value: Option<String>,
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
    #[cfg(test)]
    pub(crate) fn new(
        url: String,
        transport_backend: TransportBackend,
        clock: &'static AtomicTime,
        data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    ) -> Self {
        Self::new_with_proxy(url, transport_backend, clock, data_sender, None)
    }

    #[cfg(test)]
    pub(crate) fn new_with_proxy(
        url: String,
        transport_backend: TransportBackend,
        clock: &'static AtomicTime,
        data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
        proxy_url: Option<ProxyUrl>,
    ) -> Self {
        Self::new_with_proxy_and_state_sink(
            url,
            transport_backend,
            clock,
            data_sender,
            proxy_url,
            None,
        )
    }

    #[cfg(test)]
    pub(crate) fn new_with_proxy_and_state_sink(
        url: String,
        transport_backend: TransportBackend,
        clock: &'static AtomicTime,
        data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
        proxy_url: Option<ProxyUrl>,
        state_sink: Option<SocketStateSink>,
    ) -> Self {
        Self::new_inner(
            url,
            transport_backend,
            clock,
            data_sender,
            proxy_url,
            state_sink,
            None,
        )
    }

    pub(crate) fn new_with_proxy_and_socket_control(
        url: String,
        transport_backend: TransportBackend,
        clock: &'static AtomicTime,
        data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
        proxy_url: Option<ProxyUrl>,
        socket_control: Option<SocketControl>,
    ) -> Self {
        Self::new_inner(
            url,
            transport_backend,
            clock,
            data_sender,
            proxy_url,
            None,
            socket_control,
        )
    }

    fn new_inner(
        url: String,
        transport_backend: TransportBackend,
        clock: &'static AtomicTime,
        data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
        proxy_url: Option<ProxyUrl>,
        socket_sink: Option<SocketStateSink>,
        socket_control: Option<SocketControl>,
    ) -> Self {
        let task_owner = Arc::new(RtdsTaskSlots {
            message: tokio::sync::Mutex::new(TaskSlot::new()),
            reconcile: tokio::sync::Mutex::new(TaskSlot::new()),
            shutdown_errors: Mutex::new(Vec::new()),
        });
        Self {
            inner: Arc::new(PolymarketRtdsFeedInner {
                url,
                proxy_url,
                transport_backend,
                clock,
                data_sender,
                socket_sink,
                socket_control,
                subscriptions: dashmap::DashMap::new(),
                last_emitted_timestamps_ms: dashmap::DashMap::new(),
                live_subscriptions: Mutex::new(AHashMap::new()),
                ws_client: Mutex::new(None),
                wire_mutex: tokio::sync::Mutex::new(()),
                reconcile_notify: tokio::sync::Notify::new(),
                reconcile_pending: AtomicBool::new(false),
                reset_live_state_pending: AtomicBool::new(false),
                closing: AtomicBool::new(false),
                shutdown_generation: Mutex::new(0),
            }),
            _task_owner: Some(Arc::clone(&task_owner)),
            task_slots: Arc::downgrade(&task_owner),
        }
    }

    fn worker(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
            _task_owner: None,
            task_slots: self.task_slots.clone(),
        }
    }

    fn task_slots(&self) -> Option<Arc<RtdsTaskSlots>> {
        self.task_slots.upgrade()
    }

    pub(crate) async fn has_retained_tasks(&self) -> bool {
        let Some(tasks) = self.task_slots() else {
            return false;
        };
        !tasks.message.lock().await.is_none() || !tasks.reconcile.lock().await.is_none()
    }

    pub(crate) fn has_subscriptions(&self) -> bool {
        !self.inner.subscriptions.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn proxy_url(&self) -> Option<&ProxyUrl> {
        self.inner.proxy_url.as_ref()
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
                last_twap_fingerprint: None,
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
        let mut entry = match self.inner.subscriptions.entry(parsed.key.clone()) {
            dashmap::mapref::entry::Entry::Occupied(entry) => entry,
            dashmap::mapref::entry::Entry::Vacant(_) => return Ok(false),
        };

        let data_type_key = data_type.topic().to_string();
        {
            let subscription = entry.get_mut();
            let Some(tracked) = subscription.data_types.get_mut(&data_type_key) else {
                return Ok(false);
            };

            if tracked.ref_count > 1 {
                tracked.ref_count -= 1;
            } else {
                subscription.data_types.remove(&data_type_key);
            }

            if subscription.total_ref_count > 1 {
                subscription.total_ref_count -= 1;
                return Ok(false);
            }
        }

        self.inner.last_emitted_timestamps_ms.remove(&parsed.key);
        entry.remove();
        Ok(true)
    }

    pub(crate) async fn connect(&self) -> anyhow::Result<()> {
        let generation = *self.inner.shutdown_generation.lock();

        if self.inner.closing.load(Ordering::Acquire) {
            self.finish_retained_tasks().await?;
        }

        {
            let current_generation = self.inner.shutdown_generation.lock();
            if *current_generation != generation {
                anyhow::bail!("RTDS connect was canceled by shutdown");
            }
            self.inner.closing.store(false, Ordering::Release);
        }

        self.ensure_reconcile_worker();
        self.reconcile_once(false).await?;

        let current_generation = self.inner.shutdown_generation.lock();
        if *current_generation != generation || self.inner.closing.load(Ordering::Acquire) {
            anyhow::bail!("RTDS connect was canceled by shutdown");
        }
        Ok(())
    }

    async fn finish_retained_tasks(&self) -> anyhow::Result<()> {
        let Some(tasks) = self.task_slots() else {
            if let Some(ws) = self.current_ws() {
                ws.notify_closed();
                ws.disconnect().await;
                self.clear_ws_if_current(&ws);
            }
            anyhow::bail!("RTDS task owner was dropped");
        };

        let mut message_slot = tasks.message.lock().await;
        if let Some(outcome) =
            finish_task(&mut message_slot, Duration::ZERO, Duration::from_secs(2)).await
        {
            match outcome {
                TaskJoinOutcome::Completed(()) | TaskJoinOutcome::Aborted => {}
                TaskJoinOutcome::Failed(error) => {
                    tasks.push_shutdown_error(format!("RTDS message loop failed: {error}"));
                }
                TaskJoinOutcome::Incomplete => {
                    tasks.push_shutdown_error(
                        "RTDS message loop did not stop after abort".to_string(),
                    );
                }
            }
        }
        drop(message_slot);

        let mut reconcile_slot = tasks.reconcile.lock().await;
        if let Some(outcome) =
            finish_task(&mut reconcile_slot, Duration::ZERO, Duration::from_secs(2)).await
        {
            match outcome {
                TaskJoinOutcome::Completed(()) | TaskJoinOutcome::Aborted => {}
                TaskJoinOutcome::Failed(error) => {
                    tasks.push_shutdown_error(format!("RTDS reconcile worker failed: {error}"));
                }
                TaskJoinOutcome::Incomplete => {
                    tasks.push_shutdown_error(
                        "RTDS reconcile worker did not stop after abort".to_string(),
                    );
                }
            }
        }

        tasks.take_shutdown_result()
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

    pub(crate) async fn disconnect(&self) -> anyhow::Result<()> {
        self.begin_shutdown();
        let _guard = self.inner.wire_mutex.lock().await;

        self.inner.reconcile_pending.store(false, Ordering::Release);
        self.inner
            .reset_live_state_pending
            .store(false, Ordering::Release);
        self.inner.reconcile_notify.notify_waiters();

        let Some(tasks) = self.task_slots() else {
            anyhow::bail!("Polymarket RTDS task owner was dropped");
        };
        let mut ws = RtdsWebSocketGuard::take(&self.inner.ws_client);

        if let Some(client) = ws.ws.as_ref() {
            client.disconnect().await;
        }

        if let Some(control) = &self.inner.socket_control {
            control.deregister();
        }

        self.inner.live_subscriptions.lock().clear();
        drop(_guard);

        let mut message_slot = tasks.message.lock().await;
        if let Some(outcome) = finish_task(
            &mut message_slot,
            Duration::from_secs(2),
            Duration::from_secs(2),
        )
        .await
        {
            match outcome {
                TaskJoinOutcome::Completed(()) | TaskJoinOutcome::Aborted => {}
                TaskJoinOutcome::Failed(error) => {
                    tasks.push_shutdown_error(format!("RTDS message loop failed: {error}"));
                }
                TaskJoinOutcome::Incomplete => {
                    tasks.push_shutdown_error(
                        "RTDS message loop did not stop after abort".to_string(),
                    );
                }
            }
        }
        let message_stopped = message_slot.is_none();
        drop(message_slot);

        let mut reconcile_slot = tasks.reconcile.lock().await;
        if let Some(outcome) = finish_task(
            &mut reconcile_slot,
            Duration::from_secs(2),
            Duration::from_secs(2),
        )
        .await
        {
            match outcome {
                TaskJoinOutcome::Completed(()) | TaskJoinOutcome::Aborted => {}
                TaskJoinOutcome::Failed(error) => {
                    tasks.push_shutdown_error(format!("RTDS reconcile worker failed: {error}"));
                }
                TaskJoinOutcome::Incomplete => {
                    tasks.push_shutdown_error(
                        "RTDS reconcile worker did not stop after abort".to_string(),
                    );
                }
            }
        }

        if message_stopped && reconcile_slot.is_none() {
            ws.clear();
        }

        tasks.take_shutdown_result()
    }

    pub(crate) fn begin_shutdown(&self) {
        let mut generation = self.inner.shutdown_generation.lock();
        *generation = generation.wrapping_add(1);
        self.inner.closing.store(true, Ordering::Release);
        self.inner.reconcile_pending.store(false, Ordering::Release);
        self.inner
            .reset_live_state_pending
            .store(false, Ordering::Release);
        self.inner.reconcile_notify.notify_waiters();

        if let Some(ws) = self.inner.ws_client.lock().as_ref() {
            ws.notify_closed();
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

    fn current_ws(&self) -> Option<Arc<WebSocketClient>> {
        self.inner.ws_client.lock().clone()
    }

    fn clear_ws_if_current(&self, ws: &Arc<WebSocketClient>) -> bool {
        let mut guard = self.inner.ws_client.lock();
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
        let _generation = self.inner.shutdown_generation.lock();

        if self.inner.closing.load(Ordering::Acquire) {
            return;
        }

        let Some(tasks) = self.task_slots() else {
            return;
        };
        let Ok(mut slot) = tasks.reconcile.try_lock() else {
            return;
        };

        if slot.is_some() {
            return;
        }

        let feed = self.worker();
        if let Err(e) = slot.spawn(async move {
            feed.run_reconcile_loop().await;
        }) {
            log::error!("Failed to start RTDS reconcile worker: {e}");
        }
    }

    async fn run_reconcile_loop(&self) {
        loop {
            if self.inner.closing.load(Ordering::Acquire) {
                break;
            }

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
        let generation = {
            let generation = self.inner.shutdown_generation.lock();

            if self.inner.closing.load(Ordering::Acquire) {
                return Ok(false);
            }
            *generation
        };

        if self.current_ws().is_some_and(|ws| !ws.is_disconnected()) {
            return Ok(false);
        }

        let (handler, raw_rx) = channel_message_handler();
        let config = self.websocket_config();

        let ws = Arc::new(
            WebSocketClient::builder()
                .config(config)
                .message_handler(handler)
                .maybe_state_sink(
                    self.inner
                        .socket_control
                        .as_ref()
                        .map(SocketControl::sink)
                        .or_else(|| self.inner.socket_sink.clone()),
                )
                .connect()
                .await
                .context("failed to connect Polymarket RTDS WebSocket")?,
        );

        if !self.is_generation_open(generation) {
            ws.notify_closed();
            ws.disconnect().await;
            anyhow::bail!("RTDS connection was canceled by shutdown");
        }

        log::debug!("Polymarket RTDS WebSocket connected: {}", self.inner.url);
        *self.inner.ws_client.lock() = Some(Arc::clone(&ws));

        // Tokio cancellation is cooperative. Quiesce the previous loop before
        // activating the replacement so an admitted old-loop tail cannot emit
        // after newer data from the new connection.
        let tasks = self
            .task_slots()
            .ok_or_else(|| anyhow::anyhow!("RTDS task owner was dropped"))?;
        let mut message_slot = tasks.message.lock().await;
        if !self.is_generation_open(generation) {
            drop(message_slot);
            ws.notify_closed();
            ws.disconnect().await;
            self.clear_ws_if_current(&ws);
            anyhow::bail!("RTDS connection was canceled by shutdown");
        }
        message_slot.abort();
        let previous_loop_error = if let Some(outcome) =
            finish_task(&mut message_slot, Duration::ZERO, Duration::from_secs(2)).await
        {
            match outcome {
                TaskJoinOutcome::Completed(()) | TaskJoinOutcome::Aborted => None,
                TaskJoinOutcome::Failed(error) => {
                    Some(format!("previous RTDS message loop failed: {error}"))
                }
                TaskJoinOutcome::Incomplete => {
                    Some("previous RTDS message loop did not stop after abort".to_string())
                }
            }
        } else {
            None
        };

        if let Some(error) = previous_loop_error {
            drop(message_slot);
            ws.notify_closed();
            ws.disconnect().await;
            self.clear_ws_if_current(&ws);
            anyhow::bail!(error);
        }

        let spawn_result = {
            let generation_guard = self.inner.shutdown_generation.lock();

            if *generation_guard != generation || self.inner.closing.load(Ordering::Acquire) {
                None
            } else {
                *self.inner.ws_client.lock() = Some(Arc::clone(&ws));

                let feed = self.worker();
                let ws_for_task = Arc::clone(&ws);
                let spawn_result = message_slot.spawn(async move {
                    feed.run_message_loop(ws_for_task, raw_rx).await;
                });

                if spawn_result.is_ok()
                    && let Some(control) = &self.inner.socket_control
                {
                    let handle = ws.reconnect_handle();
                    control.register(move || handle.request_reconnect());
                }
                Some(spawn_result)
            }
        };
        let Some(spawn_result) = spawn_result else {
            drop(message_slot);
            ws.notify_closed();
            ws.disconnect().await;
            self.clear_ws_if_current(&ws);
            anyhow::bail!("RTDS connection was canceled by shutdown");
        };

        if let Err(e) = spawn_result {
            ws.disconnect().await;
            self.clear_ws_if_current(&ws);
            let shutdown_error = match finish_task(
                &mut message_slot,
                Duration::ZERO,
                Duration::from_secs(2),
            )
            .await
            {
                Some(TaskJoinOutcome::Failed(error)) => {
                    Some(format!("message loop task failed: {error}"))
                }
                Some(TaskJoinOutcome::Incomplete) => {
                    Some("message loop task did not stop after abort".to_string())
                }
                None | Some(TaskJoinOutcome::Completed(()) | TaskJoinOutcome::Aborted) => None,
            };
            anyhow::bail!(match shutdown_error {
                Some(shutdown_error) => format!(
                    "Failed to start RTDS message loop: {e}; startup rollback failed: \
                     {shutdown_error}"
                ),
                None => format!("Failed to start RTDS message loop: {e}"),
            });
        }

        Ok(true)
    }

    fn is_generation_open(&self, generation: u64) -> bool {
        let current_generation = self.inner.shutdown_generation.lock();
        *current_generation == generation && !self.inner.closing.load(Ordering::Acquire)
    }

    fn websocket_config(&self) -> WebSocketConfig {
        WebSocketConfig {
            url: self.inner.url.clone(),
            headers: vec![],
            heartbeat_interval_secs: Some(POLYMARKET_RTDS_HEARTBEAT_SECS),
            heartbeat_payload: Some("PING".to_string()),
            connect_timeout_ms: Some(POLYMARKET_RTDS_RECONNECT_TIMEOUT_MS),
            reconnect_delay_initial_ms: Some(POLYMARKET_RTDS_RECONNECT_DELAY_INITIAL_MS),
            reconnect_delay_max_ms: Some(POLYMARKET_RTDS_RECONNECT_DELAY_MAX_MS),
            reconnect_backoff_factor: Some(2.0),
            reconnect_jitter_ms: Some(POLYMARKET_RTDS_RECONNECT_JITTER_MS),
            reconnect_max_attempts: None,
            heartbeat_timeout_secs: Some(POLYMARKET_RTDS_HEARTBEAT_TIMEOUT_SECS),
            idle_timeout_ms: None,
            backend: self.inner.transport_backend,
            proxy_url: self
                .inner
                .proxy_url
                .as_ref()
                .map(|url| url.expose().to_string()),
        }
    }

    fn snapshot_wire_subscriptions(&self) -> AHashMap<String, RtdsWireSubscription> {
        let mut snapshot = AHashMap::new();

        for entry in &self.inner.subscriptions {
            snapshot
                .entry(entry.wire.topic.to_string())
                .or_insert_with(|| RtdsWireSubscription {
                    topic: entry.wire.topic,
                    msg_type: entry.wire.msg_type,
                    filters: None,
                });
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
            self.inner.live_subscriptions.lock().clear();
        }

        let desired = self.snapshot_wire_subscriptions();
        let (unsubscribe, subscribe) = {
            let live = self.inner.live_subscriptions.lock();
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

        {
            let mut live = self.inner.live_subscriptions.lock();
            live.retain(|key, _| desired.contains_key(key));
            for (key, wire) in desired {
                live.insert(key, wire);
            }
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

                    if let Err(e) = self.handle_text_message(text.as_str()) {
                        log::error!("Failed to handle subscribed Polymarket RTDS message: {e:#}");
                    }
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

    fn handle_text_message(&self, text: &str) -> anyhow::Result<()> {
        if text.trim().is_empty() {
            return Ok(());
        }

        let envelope: RtdsEnvelope = match serde_json::from_str(text) {
            Ok(envelope) => envelope,
            Err(e) => {
                if self.malformed_frame_requires_visible_twap_failure(text) {
                    return Err(anyhow::Error::new(e).context("invalid RTDS JSON frame"));
                }
                log::debug!("Ignoring non-RTDS JSON frame: {e}");
                return Ok(());
            }
        };

        self.handle_envelope(envelope)
    }

    fn handle_envelope(&self, envelope: RtdsEnvelope) -> anyhow::Result<()> {
        match (envelope.topic.as_str(), envelope.msg_type.as_str()) {
            ("crypto_prices", "subscribe") => {
                self.handle_crypto_price_subscribe(envelope);
            }
            ("crypto_prices", "update") => {
                self.handle_crypto_price_update(envelope);
            }
            ("crypto_prices_twap_thirty", "update") => {
                self.handle_crypto_twap_update(envelope, RtdsCryptoTwapWindow::ThirtySeconds)?;
            }
            ("crypto_prices_twap_sixty", "update") => {
                self.handle_crypto_twap_update(envelope, RtdsCryptoTwapWindow::SixtySeconds)?;
            }
            ("equity_prices", "subscribe") => {
                self.handle_equity_price_subscribe(envelope);
            }
            ("equity_prices", "update") => {
                self.handle_equity_price_update(envelope);
            }
            (topic @ ("crypto_prices_twap_thirty" | "crypto_prices_twap_sixty"), msg_type)
                if self.has_topic_subscription(topic) =>
            {
                anyhow::bail!("unsupported subscribed RTDS message topic={topic} type={msg_type}");
            }
            _ => {
                log::debug!(
                    "Ignoring unsupported RTDS message topic={} type={}",
                    envelope.topic,
                    envelope.msg_type,
                );
            }
        }
        Ok(())
    }

    fn handle_crypto_price_update(&self, envelope: RtdsEnvelope) {
        let payload: CryptoPayloadRaw = match serde_json::from_value(envelope.payload) {
            Ok(payload) => payload,
            Err(e) => {
                log::warn!("Failed to parse RTDS crypto price payload: {e}");
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
                log::warn!("Failed to parse RTDS crypto subscribe payload: {e}");
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

    fn handle_crypto_twap_update(
        &self,
        envelope: RtdsEnvelope,
        window: RtdsCryptoTwapWindow,
    ) -> anyhow::Result<()> {
        let topic = window.topic();
        if !self.has_topic_subscription(topic.as_str()) {
            return Ok(());
        }

        let payload: CryptoTwapPayloadRaw = serde_json::from_value(envelope.payload)
            .map_err(|e| anyhow::anyhow!("invalid RTDS crypto TWAP payload: {e}"))?;
        if payload.window_s != window.seconds() {
            anyhow::bail!(
                "RTDS TWAP topic {:?} requires window_s={}, received {}",
                topic.as_str(),
                window.seconds(),
                payload.window_s,
            );
        }
        let symbol_lower = payload.symbol.to_ascii_lowercase();
        let value =
            decimal_from_signed_e18("full_accuracy_value", payload.full_accuracy_value.as_str())?;
        let ts_event = unix_nanos_from_millis("payload.timestamp", payload.timestamp)?;
        unix_nanos_from_millis("envelope.timestamp", envelope.timestamp)?;
        let Some(data_types) =
            self.admit_twap_observation(topic, &symbol_lower, payload.timestamp, value)?
        else {
            return Ok(());
        };

        let ts_init = self.inner.clock.get_time_ns();
        let custom_payload = Arc::new(PolymarketRtdsCryptoTwap::new(
            symbol_lower,
            window.seconds(),
            value,
            payload.timestamp,
            envelope.timestamp,
            ts_event,
            ts_init,
        ));

        self.emit_custom_payload(&custom_payload, data_types);
        Ok(())
    }

    fn handle_equity_price_update(&self, envelope: RtdsEnvelope) {
        let payload: EquityPayloadRaw = match serde_json::from_value(envelope.payload) {
            Ok(payload) => payload,
            Err(e) => {
                log::warn!("Failed to parse RTDS equity price payload: {e}");
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

        let full_accuracy_value = match payload.full_accuracy_value {
            Some(full_accuracy_value) => {
                match price_from_str("full_accuracy_value", full_accuracy_value.as_str()) {
                    Ok(value) => value,
                    Err(e) => {
                        log::error!("Failed to parse RTDS equity full_accuracy_value: {e}");
                        return;
                    }
                }
            }
            None => value,
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
                log::warn!("Failed to parse RTDS equity subscribe payload: {e}");
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

    fn has_topic_subscription(&self, topic: &str) -> bool {
        self.inner
            .subscriptions
            .iter()
            .any(|entry| entry.wire.topic == topic)
    }

    fn has_twap_topic_subscription(&self) -> bool {
        self.has_topic_subscription(RtdsTopic::CryptoPricesTwapThirty.as_str())
            || self.has_topic_subscription(RtdsTopic::CryptoPricesTwapSixty.as_str())
    }

    fn malformed_frame_requires_visible_twap_failure(&self, text: &str) -> bool {
        match serde_json::from_str::<serde_json::Value>(text) {
            Ok(value) => match value.get("topic").and_then(serde_json::Value::as_str) {
                Some(topic) => {
                    (topic == RtdsTopic::CryptoPricesTwapThirty.as_str()
                        || topic == RtdsTopic::CryptoPricesTwapSixty.as_str())
                        && self.has_topic_subscription(topic)
                }
                None => self.has_twap_topic_subscription(),
            },
            Err(_) => self.has_twap_topic_subscription(),
        }
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

    fn admit_twap_observation(
        &self,
        topic: RtdsTopic,
        symbol_lower: &str,
        timestamp_ms: u64,
        value: Decimal,
    ) -> anyhow::Result<Option<Vec<DataType>>> {
        let key = tracked_key(topic.as_str(), symbol_lower);
        let Some(mut subscription) = self.inner.subscriptions.get_mut(&key) else {
            return Ok(None);
        };
        let data_types = subscription
            .data_types
            .values()
            .map(|tracked| tracked.data_type.clone())
            .collect::<Vec<_>>();

        if data_types.is_empty() {
            return Ok(None);
        }

        if let Some(previous) = subscription.last_twap_fingerprint {
            if timestamp_ms < previous.timestamp_ms {
                return Ok(None);
            }

            if timestamp_ms == previous.timestamp_ms {
                if value == previous.value {
                    return Ok(None);
                }

                anyhow::bail!(
                    concat!(
                        "conflicting RTDS TWAP observation topic={} symbol={} ",
                        "timestamp_ms={} prior={} received={}",
                    ),
                    topic.as_str(),
                    symbol_lower,
                    previous.timestamp_ms,
                    previous.value,
                    value,
                );
            }
        }

        subscription.last_twap_fingerprint = Some(TwapReplayFingerprint {
            timestamp_ms,
            value,
        });
        Ok(Some(data_types))
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
                    filters: None,
                },
            }),
            POLYMARKET_RTDS_CRYPTO_TWAP_TYPE_NAME => {
                let window_seconds = metadata
                    .get("window_seconds")
                    .and_then(serde_json::Value::as_u64)
                    .context(format!(
                        "{type_name} subscriptions require integer metadata['window_seconds']"
                    ))?;
                let window = RtdsCryptoTwapWindow::try_from(window_seconds)?;
                let topic = window.topic();
                Ok(Self {
                    key: tracked_key(topic.as_str(), &symbol_lower),
                    wire: RtdsWireSubscription {
                        topic: topic.as_str(),
                        msg_type: "update",
                        filters: None,
                    },
                })
            }
            POLYMARKET_RTDS_EQUITY_PRICE_TYPE_NAME => Ok(Self {
                key: tracked_key(RtdsTopic::EquityPrices.as_str(), &symbol_lower),
                wire: RtdsWireSubscription {
                    topic: RtdsTopic::EquityPrices.as_str(),
                    msg_type: "update",
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

fn decimal_from_signed_e18(field: &str, value: &str) -> anyhow::Result<Decimal> {
    let digits = value.strip_prefix('-').unwrap_or(value);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        anyhow::bail!("invalid signed E18 integer for {field}: {value}");
    }

    let mantissa = value
        .parse::<i128>()
        .with_context(|| format!("signed E18 integer out of range for {field}: {value}"))?;
    Decimal::try_from_i128_with_scale(mantissa, 18)
        .with_context(|| format!("signed E18 value out of Decimal range for {field}: {value}"))
}

fn unix_nanos_from_millis(field: &str, value: u64) -> anyhow::Result<UnixNanos> {
    let millis = i64::try_from(value)
        .with_context(|| format!("millisecond timestamp out of range for {field}: {value}"))?;
    UnixNanos::from_millis_checked(millis)
        .with_context(|| format!("millisecond timestamp overflows UnixNanos for {field}: {value}"))
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
    use nautilus_common::{
        live::runner::replace_system_event_sender,
        messages::{DataEvent, SystemEvent, system::SocketState},
        testing::wait_until_async,
    };
    use nautilus_core::{Params, time::get_atomic_clock_realtime};
    use nautilus_live::{SocketReconnectRegistry, SocketReconnectRequestOutcome};
    use rstest::rstest;
    use rust_decimal_macros::dec;
    use serde_json::json;

    use super::*;
    use crate::common::consts::{POLYMARKET_CLIENT_ID, POLYMARKET_VENUE};

    // Existing RTDS spot update is a sanitized capture; other fixtures are protocol cases.
    const RTDS_CRYPTO_UPDATE_FIXTURE: &str =
        include_str!("../test_data/rtds_crypto_prices_update.json");
    const RTDS_CRYPTO_SUBSCRIBE_FIXTURE: &str =
        include_str!("../test_data/rtds_crypto_prices_subscribe.json");
    const RTDS_EQUITY_UPDATE_FIXTURE: &str =
        include_str!("../test_data/rtds_equity_prices_update.json");
    const RTDS_EQUITY_SUBSCRIBE_FIXTURE: &str =
        include_str!("../test_data/rtds_equity_prices_subscribe.json");
    // Constructed from the official Polymarket SDK regression vector; see its source sidecar.
    const RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE: &str =
        include_str!("../test_data/rtds_crypto_twap_sixty_update.json");

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

    fn crypto_twap_data_type(symbol: &str, window_seconds: u64) -> DataType {
        let mut metadata = Params::new();
        metadata.insert("symbol".to_string(), json!(symbol));
        metadata.insert("window_seconds".to_string(), json!(window_seconds));
        DataType::new(POLYMARKET_RTDS_CRYPTO_TWAP_TYPE_NAME, Some(metadata), None)
    }

    fn crypto_twap_data_type_without_window(symbol: &str) -> DataType {
        let mut metadata = Params::new();
        metadata.insert("symbol".to_string(), json!(symbol));
        DataType::new(POLYMARKET_RTDS_CRYPTO_TWAP_TYPE_NAME, Some(metadata), None)
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
    fn test_rtds_envelope_captured_fields() {
        let envelope: RtdsEnvelope =
            serde_json::from_str(RTDS_CRYPTO_UPDATE_FIXTURE).expect("captured RTDS envelope");

        assert_eq!(
            envelope.connection_id.as_deref(),
            Some("11111111-2222-3333-4444-555555555555")
        );
        assert_eq!(envelope.topic, "crypto_prices");
        assert_eq!(envelope.msg_type, "update");
        assert_eq!(envelope.timestamp, 1786179814147);
        assert_eq!(
            envelope.payload,
            json!({
                "full_accuracy_value": "64997.81000000",
                "symbol": "btcusdt",
                "timestamp": 1786179814000_u64,
                "value": 64997.81,
            })
        );
    }

    #[rstest]
    fn test_rtds_envelope_without_connection_id() {
        let envelope: RtdsEnvelope =
            serde_json::from_str(RTDS_CRYPTO_SUBSCRIBE_FIXTURE).expect("legacy RTDS envelope");

        assert!(envelope.connection_id.is_none());
        assert_eq!(envelope.topic, "crypto_prices");
        assert_eq!(envelope.msg_type, "subscribe");
        assert_eq!(envelope.timestamp, 1780726213178);
        assert_eq!(envelope.payload["symbol"], "btcusdt");
        assert_eq!(envelope.payload["data"].as_array().map(Vec::len), Some(3));
    }

    #[rstest]
    fn feed_retains_proxy_url_without_debug_exposure() {
        const PROXY_URL: &str = "http://rtds-user:rtds-proxy-secret@127.0.0.1:18089";
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let feed = PolymarketRtdsFeed::new_with_proxy(
            "ws://rtds.example/ws".to_string(),
            TransportBackend::Tungstenite,
            get_atomic_clock_realtime(),
            tx,
            Some(ProxyUrl::parse(PROXY_URL).unwrap()),
        );
        let config = feed.websocket_config();
        let debug = format!("{feed:?}");

        assert_eq!(feed.proxy_url().unwrap().expose(), PROXY_URL);
        assert_eq!(config.url, "ws://rtds.example/ws");
        assert_eq!(config.headers, Vec::<(String, String)>::new());
        assert_eq!(
            config.heartbeat_interval_secs,
            Some(POLYMARKET_RTDS_HEARTBEAT_SECS)
        );
        assert_eq!(config.heartbeat_payload.as_deref(), Some("PING"));
        assert_eq!(
            config.connect_timeout_ms,
            Some(POLYMARKET_RTDS_RECONNECT_TIMEOUT_MS)
        );
        assert_eq!(
            config.reconnect_delay_initial_ms,
            Some(POLYMARKET_RTDS_RECONNECT_DELAY_INITIAL_MS)
        );
        assert_eq!(
            config.reconnect_delay_max_ms,
            Some(POLYMARKET_RTDS_RECONNECT_DELAY_MAX_MS)
        );
        assert_eq!(config.reconnect_backoff_factor, Some(2.0));
        assert_eq!(
            config.reconnect_jitter_ms,
            Some(POLYMARKET_RTDS_RECONNECT_JITTER_MS)
        );
        assert_eq!(config.reconnect_max_attempts, None);
        assert_eq!(
            config.heartbeat_timeout_secs,
            Some(POLYMARKET_RTDS_HEARTBEAT_TIMEOUT_SECS)
        );
        assert_eq!(config.backend, TransportBackend::Tungstenite);
        assert_eq!(config.proxy_url.as_deref(), Some(PROXY_URL));
        assert!(!debug.contains("rtds-proxy-secret"));
    }

    #[derive(Clone, Default)]
    struct TestServerState {
        received_payloads: Arc<tokio::sync::Mutex<Vec<serde_json::Value>>>,
        connection_count: Arc<tokio::sync::Mutex<usize>>,
        control_txs:
            Arc<tokio::sync::Mutex<Vec<tokio::sync::mpsc::UnboundedSender<TestServerCommand>>>>,
    }

    #[derive(Debug)]
    enum TestServerCommand {
        SendText(String),
        Close,
    }

    impl TestServerState {
        async fn clear_received_payloads(&self) {
            self.received_payloads.lock().await.clear();
        }

        async fn connection_count(&self) -> usize {
            *self.connection_count.lock().await
        }

        async fn send_text_to_all(&self, text: String) {
            let senders = self.control_txs.lock().await.clone();
            for tx in senders {
                let _ = tx.send(TestServerCommand::SendText(text.clone()));
            }
        }

        async fn close_all_connections(&self) {
            let senders = self.control_txs.lock().await.clone();
            for tx in senders {
                let _ = tx.send(TestServerCommand::Close);
            }
        }
    }

    async fn handle_rtds_upgrade(
        ws: WebSocketUpgrade,
        State(state): State<TestServerState>,
    ) -> Response {
        ws.on_upgrade(move |socket| handle_rtds_socket(socket, state))
    }

    async fn handle_rtds_socket(mut socket: WebSocket, state: TestServerState) {
        *state.connection_count.lock().await += 1;

        let (control_tx, mut control_rx) = tokio::sync::mpsc::unbounded_channel();
        state.control_txs.lock().await.push(control_tx);

        loop {
            let result = tokio::select! {
                command = control_rx.recv() => {
                    match command {
                        Some(TestServerCommand::SendText(text)) => {
                            if socket.send(AxumWsMessage::Text(text.into())).await.is_err() {
                                break;
                            }
                            continue;
                        }
                        Some(TestServerCommand::Close) => {
                            let _ = socket.send(AxumWsMessage::Close(None)).await;
                            break;
                        }
                        None => break,
                    }
                }
                result = socket.next() => result
            };

            let Some(result) = result else { break };
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

    fn build_crypto_update(
        symbol: &str,
        value: &str,
        price_timestamp_ms: u64,
        message_timestamp_ms: u64,
    ) -> String {
        let mut update: serde_json::Value =
            serde_json::from_str(RTDS_CRYPTO_UPDATE_FIXTURE).expect("parse crypto update fixture");
        update["payload"]["symbol"] = json!(symbol);
        update["payload"]["value"] =
            serde_json::from_str(value).expect("crypto update value should be a JSON number");
        update["payload"]["timestamp"] = json!(price_timestamp_ms);
        update["timestamp"] = json!(message_timestamp_ms);
        update.to_string()
    }

    fn collect_crypto_symbols(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
        expected_count: usize,
    ) -> Vec<String> {
        let mut symbols = Vec::with_capacity(expected_count);
        for _ in 0..expected_count {
            let event = rx.try_recv().expect("custom data event");
            let DataEvent::Data(NautilusData::Custom(custom)) = event else {
                panic!("expected custom data event");
            };
            let payload = custom
                .data
                .as_any()
                .downcast_ref::<PolymarketRtdsCryptoPrice>()
                .expect("PolymarketRtdsCryptoPrice");
            symbols.push(payload.symbol.clone());
        }
        symbols.sort_unstable();
        symbols
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

    #[rstest]
    #[tokio::test]
    async fn test_socket_state_events_use_rtds_endpoint() {
        let state = TestServerState::default();
        let addr = start_rtds_server(state).await;
        let (data_tx, _data_rx) = tokio::sync::mpsc::unbounded_channel();
        let (system_tx, mut system_rx) = tokio::sync::mpsc::unbounded_channel();
        replace_system_event_sender(system_tx);
        let registry = SocketReconnectRegistry::default();
        let socket_factory = nautilus_live::SocketControlFactory::with_registry(
            *POLYMARKET_CLIENT_ID,
            Some(*POLYMARKET_VENUE),
            &registry,
        );
        let feed = PolymarketRtdsFeed::new_with_proxy_and_socket_control(
            format!("ws://{addr}/rtds"),
            TransportBackend::default(),
            get_atomic_clock_realtime(),
            data_tx,
            None,
            Some(socket_factory.control(crate::websocket::RTDS_STREAMS_ENDPOINT)),
        );
        assert!(
            feed.track_subscribe(crypto_data_type("BTC"))
                .expect("track RTDS subscription")
        );

        feed.connect().await.expect("connect RTDS feed");

        let event = tokio::time::timeout(Duration::from_secs(5), system_rx.recv())
            .await
            .expect("wait for socket state change")
            .expect("system event channel closed");
        let SystemEvent::SocketState(change) = event;
        let endpoint = ustr::Ustr::from("polymarket-rtds-streams");
        let handle = registry.handle(*POLYMARKET_CLIENT_ID, endpoint).unwrap();

        assert_eq!(change.client_id, *POLYMARKET_CLIENT_ID);
        assert_eq!(change.venue, Some(*POLYMARKET_VENUE));
        assert_eq!(change.endpoint, endpoint);
        assert_eq!(change.state, SocketState::Connected);
        assert_eq!(
            handle.request_reconnect(),
            SocketReconnectRequestOutcome::Accepted
        );

        let event = tokio::time::timeout(Duration::from_secs(5), system_rx.recv())
            .await
            .expect("wait for socket state change")
            .expect("system event channel closed");
        let SystemEvent::SocketState(change) = event;
        assert_eq!(change.client_id, *POLYMARKET_CLIENT_ID);
        assert_eq!(change.venue, Some(*POLYMARKET_VENUE));
        assert_eq!(change.endpoint, endpoint);
        assert_eq!(change.state, SocketState::Disconnected);

        feed.disconnect().await.expect("disconnect feed");
        assert!(registry.handle(*POLYMARKET_CLIENT_ID, endpoint).is_none());
    }

    #[rstest]
    #[tokio::test]
    async fn test_server_disconnect_preserves_twap_replay_fingerprint() {
        let state = TestServerState::default();
        let addr = start_rtds_server(state.clone()).await;
        let (data_tx, mut data_rx) = tokio::sync::mpsc::unbounded_channel();
        let states = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let states_callback = Arc::clone(&states);
        let state_sink = SocketStateSink::new(move |state| {
            states_callback.lock().push(state);
        });
        let feed = PolymarketRtdsFeed::new_with_proxy_and_state_sink(
            format!("ws://{addr}/rtds"),
            TransportBackend::default(),
            get_atomic_clock_realtime(),
            data_tx,
            None,
            Some(state_sink),
        );
        assert!(
            feed.track_subscribe(crypto_twap_data_type("BTC/USD", 60))
                .expect("track RTDS TWAP subscription")
        );

        feed.connect().await.expect("connect RTDS feed");
        wait_until_async(
            || {
                let state = state.clone();
                async move {
                    state.received_payloads.lock().await.iter().any(|payload| {
                        payload["subscriptions"]
                            .as_array()
                            .is_some_and(|subscriptions| {
                                subscriptions.iter().any(|subscription| {
                                    subscription["topic"].as_str()
                                        == Some(RtdsTopic::CryptoPricesTwapSixty.as_str())
                                })
                            })
                    })
                }
            },
            Duration::from_secs(5),
        )
        .await;
        state
            .send_text_to_all(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE.to_string())
            .await;
        tokio::time::timeout(Duration::from_secs(5), data_rx.recv())
            .await
            .expect("initial TWAP observation timeout")
            .expect("initial TWAP data channel closed");

        state.clear_received_payloads().await;
        state.close_all_connections().await;

        wait_until_async(
            || {
                let states = Arc::clone(&states);
                async move {
                    states.lock().as_slice()
                        == [
                            nautilus_network::SocketState::Connected,
                            nautilus_network::SocketState::Disconnected,
                            nautilus_network::SocketState::Connected,
                        ]
                }
            },
            Duration::from_secs(10),
        )
        .await;

        wait_until_async(
            || {
                let state = state.clone();
                async move {
                    state.received_payloads.lock().await.iter().any(|payload| {
                        payload["subscriptions"]
                            .as_array()
                            .is_some_and(|subscriptions| {
                                subscriptions.iter().any(|subscription| {
                                    subscription["topic"].as_str()
                                        == Some(RtdsTopic::CryptoPricesTwapSixty.as_str())
                                })
                            })
                    })
                }
            },
            Duration::from_secs(5),
        )
        .await;

        let mut conflict: serde_json::Value =
            serde_json::from_str(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE)
                .expect("parse TWAP fixture");
        conflict["payload"]["full_accuracy_value"] = json!("65000123456789012345679");
        let error = feed
            .handle_text_message(&conflict.to_string())
            .expect_err("equal-timestamp value conflict must be visible after reconnect");
        assert!(error.to_string().contains("conflict"));

        feed.handle_text_message(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE)
            .expect("the retained exact observation must remain a replay after conflict");
        assert!(
            tokio::time::timeout(Duration::from_millis(250), data_rx.recv())
                .await
                .is_err(),
            "reconnect must not re-emit the last equal-timestamp TWAP observation"
        );

        let mut next: serde_json::Value =
            serde_json::from_str(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE)
                .expect("parse TWAP fixture");
        next["payload"]["timestamp"] = json!(
            next["payload"]["timestamp"]
                .as_u64()
                .expect("fixture observation timestamp")
                + 1
        );
        state.send_text_to_all(next.to_string()).await;
        tokio::time::timeout(Duration::from_secs(5), data_rx.recv())
            .await
            .expect("post-reconnect TWAP observation timeout")
            .expect("post-reconnect TWAP data channel closed");

        assert_eq!(
            states.lock().as_slice(),
            [
                nautilus_network::SocketState::Connected,
                nautilus_network::SocketState::Disconnected,
                nautilus_network::SocketState::Connected,
            ]
        );

        feed.disconnect().await.expect("disconnect feed");
    }

    async fn connect_test_ws(url: String) -> Arc<WebSocketClient> {
        let (handler, _raw_rx) = channel_message_handler();
        Arc::new(
            WebSocketClient::builder()
                .config(WebSocketConfig {
                    url,
                    headers: vec![],
                    heartbeat_interval_secs: Some(POLYMARKET_RTDS_HEARTBEAT_SECS),
                    heartbeat_payload: Some("PING".to_string()),
                    connect_timeout_ms: Some(POLYMARKET_RTDS_RECONNECT_TIMEOUT_MS),
                    reconnect_delay_initial_ms: Some(POLYMARKET_RTDS_RECONNECT_DELAY_INITIAL_MS),
                    reconnect_delay_max_ms: Some(POLYMARKET_RTDS_RECONNECT_DELAY_MAX_MS),
                    reconnect_backoff_factor: Some(2.0),
                    reconnect_jitter_ms: Some(POLYMARKET_RTDS_RECONNECT_JITTER_MS),
                    reconnect_max_attempts: None,
                    heartbeat_timeout_secs: Some(POLYMARKET_RTDS_HEARTBEAT_TIMEOUT_SECS),
                    idle_timeout_ms: None,
                    backend: TransportBackend::default(),
                    proxy_url: None,
                })
                .message_handler(handler)
                .connect()
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
    fn test_track_subscribe_maps_twap_window_to_exact_topic() {
        let (feed, _rx) = make_feed();
        let thirty = crypto_twap_data_type("BTC/USD", 30);
        let sixty = crypto_twap_data_type("BTC/USD", 60);

        assert!(is_supported_rtds_data_type(&thirty));
        assert!(is_supported_rtds_data_type(&sixty));
        assert!(feed.track_subscribe(thirty).expect("track 30-second TWAP"));
        assert!(feed.track_subscribe(sixty).expect("track 60-second TWAP"));

        assert_eq!(
            feed.tracked_data_type_count("crypto_prices_twap_thirty:btc/usd"),
            1
        );
        assert_eq!(
            feed.tracked_data_type_count("crypto_prices_twap_sixty:btc/usd"),
            1
        );
        let wire = feed.snapshot_wire_subscriptions();
        assert_eq!(wire.len(), 2);
        assert!(wire.contains_key("crypto_prices_twap_thirty"));
        assert!(wire.contains_key("crypto_prices_twap_sixty"));
    }

    #[rstest]
    #[case(45)]
    #[case(0)]
    fn test_track_subscribe_rejects_unsupported_twap_window(#[case] window_seconds: u64) {
        let (feed, _rx) = make_feed();

        let error = feed
            .track_subscribe(crypto_twap_data_type("BTC/USD", window_seconds))
            .expect_err("unsupported TWAP window must fail");

        assert!(error.to_string().contains("30 or 60"));
        assert_eq!(feed.tracked_subscription_count(), 0);
    }

    #[rstest]
    fn test_track_subscribe_rejects_missing_twap_window() {
        let (feed, _rx) = make_feed();

        let error = feed
            .track_subscribe(crypto_twap_data_type_without_window("BTC/USD"))
            .expect_err("missing TWAP window must fail");

        assert!(error.to_string().contains("window_seconds"));
        assert_eq!(feed.tracked_subscription_count(), 0);
    }

    #[rstest]
    fn test_handle_crypto_twap_update_emits_exact_provider_value_and_three_clocks() {
        let (feed, mut rx) = make_feed();
        let data_type = crypto_twap_data_type("BTC/USD", 60);
        feed.track_subscribe(data_type.clone())
            .expect("track 60-second TWAP");

        feed.handle_text_message(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE)
            .expect("valid TWAP update");

        let event = rx.try_recv().expect("custom data event");
        let DataEvent::Data(NautilusData::Custom(custom)) = event else {
            panic!("expected custom data event");
        };
        let payload = custom
            .data
            .as_any()
            .downcast_ref::<PolymarketRtdsCryptoTwap>()
            .expect("PolymarketRtdsCryptoTwap");

        assert_eq!(custom.data_type, data_type);
        assert_eq!(payload.symbol, "btc/usd");
        assert_eq!(payload.window_seconds, 60);
        assert_eq!(payload.value, dec!(65000.123456789012345678));
        assert_eq!(payload.observation_timestamp_ms, 1772752581815);
        assert_eq!(payload.message_timestamp_ms, 1772752582004);
        assert_eq!(payload.ts_event, UnixNanos::from_millis(1772752581815));
        assert!(payload.ts_init > UnixNanos::default());
    }

    #[rstest]
    fn test_handle_crypto_twap_update_accepts_thirty_second_topic_only_for_thirty_window() {
        let (feed, mut rx) = make_feed();
        let data_type = crypto_twap_data_type("BTC/USD", 30);
        feed.track_subscribe(data_type.clone())
            .expect("track 30-second TWAP");
        let mut update: serde_json::Value =
            serde_json::from_str(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE)
                .expect("parse TWAP fixture");
        update["topic"] = json!("crypto_prices_twap_thirty");
        update["payload"]["window_s"] = json!(30);

        feed.handle_text_message(&update.to_string())
            .expect("valid 30-second TWAP update");

        let DataEvent::Data(NautilusData::Custom(custom)) =
            rx.try_recv().expect("custom data event")
        else {
            panic!("expected custom data event");
        };
        let payload = custom
            .data
            .as_any()
            .downcast_ref::<PolymarketRtdsCryptoTwap>()
            .expect("PolymarketRtdsCryptoTwap");
        assert_eq!(custom.data_type, data_type);
        assert_eq!(payload.window_seconds, 30);
        assert_eq!(payload.value, dec!(65000.123456789012345678));
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_crypto_twap_update_uses_exact_field_not_display_value() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("BTC/USD", 60))
            .expect("track 60-second TWAP");
        let mut update: serde_json::Value =
            serde_json::from_str(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE)
                .expect("parse TWAP fixture");
        update["payload"]["value"] = json!(1);

        feed.handle_text_message(&update.to_string())
            .expect("display value must not be authoritative");

        let event = rx.try_recv().expect("custom data event");
        let DataEvent::Data(NautilusData::Custom(custom)) = event else {
            panic!("expected custom data event");
        };
        let payload = custom
            .data
            .as_any()
            .downcast_ref::<PolymarketRtdsCryptoTwap>()
            .expect("PolymarketRtdsCryptoTwap");
        assert_eq!(payload.value, dec!(65000.123456789012345678));
    }

    #[rstest]
    #[case("64997.81")]
    #[case("6.499781e4")]
    fn test_handle_crypto_twap_update_accepts_decimal_string_display_value(
        #[case] display_value: &str,
    ) {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("BTC/USD", 60))
            .expect("track 60-second TWAP");
        let mut update: serde_json::Value =
            serde_json::from_str(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE)
                .expect("parse TWAP fixture");
        update["payload"]["value"] = json!(display_value);

        feed.handle_text_message(&update.to_string())
            .expect("decimal-string display value should satisfy the wire contract");

        let DataEvent::Data(NautilusData::Custom(custom)) =
            rx.try_recv().expect("custom data event")
        else {
            panic!("expected custom data event");
        };
        let payload = custom
            .data
            .as_any()
            .downcast_ref::<PolymarketRtdsCryptoTwap>()
            .expect("PolymarketRtdsCryptoTwap");
        assert_eq!(payload.value, dec!(65000.123456789012345678));
    }

    #[rstest]
    #[case::missing(None)]
    #[case::null(Some(json!(null)))]
    #[case::boolean(Some(json!(true)))]
    #[case::object(Some(json!({"future": "format"})))]
    #[case::array(Some(json!([1, 2])))]
    #[case::nonnumeric(Some(json!("not-a-number")))]
    #[case::empty_string(Some(json!("")))]
    fn test_handle_crypto_twap_update_rejects_invalid_display_value(
        #[case] display_value: Option<serde_json::Value>,
    ) {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("BTC/USD", 60))
            .expect("track 60-second TWAP");
        let mut update: serde_json::Value =
            serde_json::from_str(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE)
                .expect("parse TWAP fixture");
        let payload = update["payload"].as_object_mut().expect("payload object");

        match display_value {
            Some(value) => {
                payload.insert("value".to_string(), value);
            }
            None => {
                payload.remove("value");
            }
        }

        let error = feed
            .handle_text_message(&update.to_string())
            .expect_err("invalid display value must fail visibly");
        let message = error.to_string();

        assert!(message.contains("`value`"), "{message}");
        assert!(!message.contains("full_accuracy_value"), "{message}");
        assert!(!message.contains("signed E18 integer"), "{message}");
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_crypto_twap_update_validates_display_value_before_full_accuracy_value() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("BTC/USD", 60))
            .expect("track 60-second TWAP");
        let mut update: serde_json::Value =
            serde_json::from_str(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE)
                .expect("parse TWAP fixture");
        update["payload"]["value"] = json!({"future": "format"});
        update["payload"]["full_accuracy_value"] = json!("invalid");

        let error = feed
            .handle_text_message(&update.to_string())
            .expect_err("display value must be validated before the exact value");
        let message = error.to_string();

        assert!(message.contains("`value`"), "{message}");
        assert!(!message.contains("full_accuracy_value"), "{message}");
        assert!(!message.contains("signed E18 integer"), "{message}");
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_crypto_twap_update_preserves_adjacent_e18_values() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("BTC/USD", 60))
            .expect("track 60-second TWAP");
        let first: serde_json::Value = serde_json::from_str(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE)
            .expect("parse TWAP fixture");
        let mut second = first.clone();
        second["payload"]["full_accuracy_value"] = json!("65000123456789012345679");
        second["payload"]["timestamp"] = json!(
            first["payload"]["timestamp"]
                .as_u64()
                .expect("fixture observation timestamp")
                + 1
        );

        feed.handle_text_message(&first.to_string())
            .expect("first exact TWAP update");
        feed.handle_text_message(&second.to_string())
            .expect("adjacent exact TWAP update");

        let mut values = Vec::new();

        for _ in 0..2 {
            let DataEvent::Data(NautilusData::Custom(custom)) =
                rx.try_recv().expect("custom data event")
            else {
                panic!("expected custom data event");
            };
            let payload = custom
                .data
                .as_any()
                .downcast_ref::<PolymarketRtdsCryptoTwap>()
                .expect("PolymarketRtdsCryptoTwap");
            values.push(payload.value);
        }

        assert_eq!(values[0], dec!(65000.123456789012345678));
        assert_eq!(values[1], dec!(65000.123456789012345679));
        assert_ne!(values[0], values[1]);
    }

    #[rstest]
    fn test_twap_replay_fingerprints_are_isolated_by_symbol() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("ALPHA/USD", 60))
            .expect("track first 60-second TWAP");
        feed.track_subscribe(crypto_twap_data_type("BETA/USD", 60))
            .expect("track second 60-second TWAP");
        let mut first: serde_json::Value =
            serde_json::from_str(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE)
                .expect("parse TWAP fixture");
        first["payload"]["symbol"] = json!("alpha/usd");
        first["payload"]["full_accuracy_value"] = json!("1000000000000000000");
        let first_timestamp = first["payload"]["timestamp"]
            .as_u64()
            .expect("fixture observation timestamp");
        let mut second = first.clone();
        second["payload"]["symbol"] = json!("beta/usd");
        second["payload"]["timestamp"] = json!(first_timestamp - 1);
        second["payload"]["full_accuracy_value"] = json!("2000000000000000000");

        feed.handle_text_message(&first.to_string())
            .expect("first TWAP update");
        feed.handle_text_message(&second.to_string())
            .expect("older second-symbol update must use an independent replay guard");

        let mut observations = Vec::new();

        for _ in 0..2 {
            let DataEvent::Data(NautilusData::Custom(custom)) =
                rx.try_recv().expect("custom data event")
            else {
                panic!("expected custom data event");
            };
            let payload = custom
                .data
                .as_any()
                .downcast_ref::<PolymarketRtdsCryptoTwap>()
                .expect("PolymarketRtdsCryptoTwap");
            observations.push((payload.symbol.clone(), payload.observation_timestamp_ms));
        }

        assert_eq!(
            observations,
            vec![
                ("alpha/usd".to_string(), first_timestamp),
                ("beta/usd".to_string(), first_timestamp - 1),
            ]
        );
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_crypto_twap_update_drops_equal_timestamp_redelivery() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("BTC/USD", 60))
            .expect("track 60-second TWAP");

        feed.handle_text_message(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE)
            .expect("first exact TWAP update");
        feed.handle_text_message(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE)
            .expect("equal-timestamp redelivery should be ignored");

        let DataEvent::Data(NautilusData::Custom(_)) =
            rx.try_recv().expect("first custom data event")
        else {
            panic!("expected custom data event");
        };
        assert!(
            rx.try_recv().is_err(),
            "an equal-timestamp TWAP redelivery must not emit twice"
        );
    }

    #[rstest]
    fn test_twap_conflict_does_not_advance_replay_guard() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("BTC/USD", 60))
            .expect("track 60-second TWAP");
        let original: serde_json::Value =
            serde_json::from_str(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE)
                .expect("parse TWAP fixture");
        let mut older = original.clone();
        older["payload"]["timestamp"] = json!(
            original["payload"]["timestamp"]
                .as_u64()
                .expect("fixture observation timestamp")
                - 1
        );
        older["payload"]["full_accuracy_value"] = json!("65000123456789012345677");
        let mut conflict = original.clone();
        conflict["payload"]["full_accuracy_value"] = json!("65000123456789012345679");

        feed.handle_text_message(&original.to_string())
            .expect("first exact TWAP update");
        let DataEvent::Data(NautilusData::Custom(_)) =
            rx.try_recv().expect("first custom data event")
        else {
            panic!("expected custom data event");
        };
        feed.handle_text_message(&older.to_string())
            .expect("older TWAP update should be ignored");

        let error = feed
            .handle_text_message(&conflict.to_string())
            .expect_err("equal-timestamp value conflict must be visible");
        assert_eq!(
            error.to_string(),
            concat!(
                "conflicting RTDS TWAP observation topic=crypto_prices_twap_sixty ",
                "symbol=btc/usd timestamp_ms=1772752581815 ",
                "prior=65000.123456789012345678 received=65000.123456789012345679",
            ),
        );
        assert!(rx.try_recv().is_err());

        feed.handle_text_message(&original.to_string())
            .expect("the retained exact observation must remain a replay after conflict");
        assert!(rx.try_recv().is_err());

        let mut newer = conflict;
        newer["payload"]["timestamp"] = json!(
            original["payload"]["timestamp"]
                .as_u64()
                .expect("fixture observation timestamp")
                + 1
        );
        feed.handle_text_message(&newer.to_string())
            .expect("newer exact TWAP update");
        let DataEvent::Data(NautilusData::Custom(custom)) =
            rx.try_recv().expect("newer custom data event")
        else {
            panic!("expected custom data event");
        };
        let payload = custom
            .data
            .as_any()
            .downcast_ref::<PolymarketRtdsCryptoTwap>()
            .expect("PolymarketRtdsCryptoTwap");
        assert_eq!(payload.value, dec!(65000.123456789012345679));
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_twap_inflight_tail_cannot_poison_resubscribe_replay() {
        let (feed, mut rx) = make_feed();
        let data_type = crypto_twap_data_type("BTC/USD", 60);
        let envelope: RtdsEnvelope = serde_json::from_str(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE)
            .expect("parse TWAP envelope");
        let payload: CryptoTwapPayloadRaw =
            serde_json::from_value(envelope.payload).expect("parse TWAP payload");
        let observation_timestamp_ms = payload.timestamp;
        let exact_value =
            decimal_from_signed_e18("full_accuracy_value", &payload.full_accuracy_value)
                .expect("parse exact TWAP value");
        feed.track_subscribe(data_type.clone())
            .expect("track 60-second TWAP");

        let captured_data_types =
            feed.matching_data_types(RtdsTopic::CryptoPricesTwapSixty, "btc/usd");
        assert_eq!(captured_data_types.len(), 1);
        assert_eq!(captured_data_types[0], data_type);

        assert!(
            feed.track_unsubscribe(&data_type)
                .expect("unsubscribe final TWAP reference")
        );
        let _tail_admission = feed
            .admit_twap_observation(
                RtdsTopic::CryptoPricesTwapSixty,
                "btc/usd",
                observation_timestamp_ms,
                exact_value,
            )
            .expect("complete captured handler admission tail");

        assert!(
            feed.track_subscribe(data_type.clone())
                .expect("resubscribe 60-second TWAP")
        );
        feed.handle_text_message(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE)
            .expect("first replay after resubscribe");

        let DataEvent::Data(NautilusData::Custom(custom)) = rx
            .try_recv()
            .expect("first replay must emit after resubscribe")
        else {
            panic!("expected custom data event");
        };
        let payload = custom
            .data
            .as_any()
            .downcast_ref::<PolymarketRtdsCryptoTwap>()
            .expect("PolymarketRtdsCryptoTwap");
        assert_eq!(custom.data_type, data_type);
        assert_eq!(payload.value, exact_value);
        assert_eq!(payload.observation_timestamp_ms, observation_timestamp_ms);
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    #[tokio::test]
    async fn test_reconnect_quiesces_old_twap_tail_before_new_loop_delivery() {
        let state = TestServerState::default();
        let addr = start_rtds_server(state.clone()).await;
        let (data_tx, mut data_rx) = tokio::sync::mpsc::unbounded_channel();
        let feed = PolymarketRtdsFeed::new(
            format!("ws://{addr}/rtds"),
            TransportBackend::default(),
            get_atomic_clock_realtime(),
            data_tx,
        );
        let data_type = crypto_twap_data_type("BTC/USD", 60);
        feed.track_subscribe(data_type)
            .expect("track 60-second TWAP");

        let envelope: RtdsEnvelope = serde_json::from_str(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE)
            .expect("parse TWAP envelope");
        let message_timestamp_ms = envelope.timestamp;
        let payload: CryptoTwapPayloadRaw =
            serde_json::from_value(envelope.payload).expect("parse TWAP payload");
        let symbol_lower = payload.symbol.to_ascii_lowercase();
        let observation_timestamp_ms = payload.timestamp;
        let exact_value =
            decimal_from_signed_e18("full_accuracy_value", &payload.full_accuracy_value)
                .expect("parse exact TWAP value");
        let ts_event = unix_nanos_from_millis("payload.timestamp", observation_timestamp_ms)
            .expect("parse TWAP event timestamp");

        let (admitted_tx, admitted_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let old_feed = feed.clone();
        let old_symbol = symbol_lower.clone();

        let old_handle = get_runtime().spawn(async move {
            let data_types = old_feed
                .admit_twap_observation(
                    RtdsTopic::CryptoPricesTwapSixty,
                    &old_symbol,
                    observation_timestamp_ms,
                    exact_value,
                )
                .expect("admit old-loop TWAP tail")
                .expect("old-loop TWAP tail should be selected");
            admitted_tx
                .send(())
                .expect("signal old-loop TWAP admission");
            release_rx
                .recv()
                .expect("release old-loop TWAP emission tail");

            let custom_payload = Arc::new(PolymarketRtdsCryptoTwap::new(
                old_symbol,
                RtdsCryptoTwapWindow::SixtySeconds.seconds(),
                exact_value,
                observation_timestamp_ms,
                message_timestamp_ms,
                ts_event,
                old_feed.inner.clock.get_time_ns(),
            ));
            old_feed.emit_custom_payload(&custom_payload, data_types);
        });
        feed.task_slots()
            .expect("RTDS task owner")
            .message
            .lock()
            .await
            .insert(old_handle);
        admitted_rx
            .await
            .expect("old-loop TWAP admission signal dropped");

        let connect_task = tokio::spawn({
            let feed = feed.clone();
            async move { feed.connect().await }
        });
        wait_until_async(
            || {
                let state = state.clone();
                async move { state.connection_count().await >= 1 }
            },
            Duration::from_secs(5),
        )
        .await;

        let mut newer: serde_json::Value =
            serde_json::from_str(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE)
                .expect("parse newer TWAP frame");
        newer["payload"]["timestamp"] = json!(observation_timestamp_ms + 1);
        state.send_text_to_all(newer.to_string()).await;

        let early_event = tokio::time::timeout(Duration::from_millis(250), data_rx.recv())
            .await
            .ok()
            .flatten();
        release_tx
            .send(())
            .expect("release old-loop TWAP emission tail");
        connect_task
            .await
            .expect("join RTDS connect task")
            .expect("connect RTDS feed");

        let observation_timestamp = |event| {
            let DataEvent::Data(NautilusData::Custom(custom)) = event else {
                panic!("expected custom data event");
            };
            custom
                .data
                .as_any()
                .downcast_ref::<PolymarketRtdsCryptoTwap>()
                .expect("PolymarketRtdsCryptoTwap")
                .observation_timestamp_ms
        };
        let mut timestamps = Vec::new();
        if let Some(event) = early_event {
            timestamps.push(observation_timestamp(event));
        }

        while timestamps.len() < 2 {
            let event = tokio::time::timeout(Duration::from_secs(5), data_rx.recv())
                .await
                .expect("TWAP delivery timeout")
                .expect("TWAP data channel closed");
            timestamps.push(observation_timestamp(event));
        }

        assert_eq!(
            timestamps,
            [observation_timestamp_ms, observation_timestamp_ms + 1],
            "a replacement message loop must not overtake an admitted old-loop TWAP tail",
        );
        assert!(data_rx.try_recv().is_err());
        feed.disconnect().await.expect("disconnect feed");
    }

    #[rstest]
    fn test_handle_crypto_twap_update_preserves_negative_signed_e18_value() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("BTC/USD", 60))
            .expect("track 60-second TWAP");
        let mut update: serde_json::Value =
            serde_json::from_str(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE)
                .expect("parse TWAP fixture");
        update["payload"]["full_accuracy_value"] = json!("-1234567890000000000");

        feed.handle_text_message(&update.to_string())
            .expect("valid negative signed E18 update");

        let DataEvent::Data(NautilusData::Custom(custom)) =
            rx.try_recv().expect("custom data event")
        else {
            panic!("expected custom data event");
        };
        let payload = custom
            .data
            .as_any()
            .downcast_ref::<PolymarketRtdsCryptoTwap>()
            .expect("PolymarketRtdsCryptoTwap");
        assert_eq!(payload.value, dec!(-1.234567890000000000));
    }

    #[rstest]
    fn test_handle_crypto_twap_update_rejects_topic_window_mismatch_without_advancing_guard() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("BTC/USD", 60))
            .expect("track 60-second TWAP");
        let mut mismatch: serde_json::Value =
            serde_json::from_str(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE)
                .expect("parse TWAP fixture");
        mismatch["payload"]["window_s"] = json!(30);

        let error = feed
            .handle_text_message(&mismatch.to_string())
            .expect_err("topic/window mismatch must be visible");
        assert!(error.to_string().contains("requires window_s=60"));
        assert!(rx.try_recv().is_err());

        feed.handle_text_message(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE)
            .expect("same-timestamp valid update must still emit");
        assert!(rx.try_recv().is_ok());
    }

    #[rstest]
    fn test_handle_crypto_twap_update_rejects_missing_exact_value_without_fallback() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("BTC/USD", 60))
            .expect("track 60-second TWAP");
        let mut update: serde_json::Value =
            serde_json::from_str(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE)
                .expect("parse TWAP fixture");
        update["payload"]
            .as_object_mut()
            .expect("payload object")
            .remove("full_accuracy_value");

        let error = feed
            .handle_text_message(&update.to_string())
            .expect_err("display value must not be a fallback");
        assert!(error.to_string().contains("full_accuracy_value"));
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_crypto_twap_update_rejects_missing_window() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("BTC/USD", 60))
            .expect("track 60-second TWAP");
        let mut update: serde_json::Value =
            serde_json::from_str(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE)
                .expect("parse TWAP fixture");
        update["payload"]
            .as_object_mut()
            .expect("payload object")
            .remove("window_s");

        let error = feed
            .handle_text_message(&update.to_string())
            .expect_err("missing window must fail visibly");
        assert!(error.to_string().contains("window_s"));
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_crypto_twap_update_rejects_out_of_decimal_range_value() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("BTC/USD", 60))
            .expect("track 60-second TWAP");
        let mut update: serde_json::Value =
            serde_json::from_str(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE)
                .expect("parse TWAP fixture");
        update["payload"]["full_accuracy_value"] = json!("79228162514264337593543950336");

        let error = feed
            .handle_text_message(&update.to_string())
            .expect_err("out-of-range exact value must fail visibly");
        assert!(error.to_string().contains("Decimal range"));
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_crypto_twap_update_validates_untracked_symbol_on_active_topic() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("BTC/USD", 60))
            .expect("track 60-second TWAP");
        let mut update: serde_json::Value =
            serde_json::from_str(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE)
                .expect("parse TWAP fixture");
        update["payload"]["symbol"] = json!("eth/usd");
        update["payload"]["full_accuracy_value"] = json!("not-an-integer");

        let error = feed
            .handle_text_message(&update.to_string())
            .expect_err("malformed frame on active topic must fail visibly");
        assert!(error.to_string().contains("signed E18 integer"));
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    #[case("+64997810000000000000000")]
    #[case("64997.81")]
    #[case("")]
    #[case("not-an-integer")]
    fn test_handle_crypto_twap_update_rejects_non_integer_exact_value(#[case] value: &str) {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("BTC/USD", 60))
            .expect("track 60-second TWAP");
        let mut update: serde_json::Value =
            serde_json::from_str(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE)
                .expect("parse TWAP fixture");
        update["payload"]["full_accuracy_value"] = json!(value);

        let error = feed
            .handle_text_message(&update.to_string())
            .expect_err("non-integer exact value must fail");
        assert!(error.to_string().contains("signed E18 integer"));
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_crypto_twap_update_rejects_timestamp_overflow_without_advancing_guard() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("BTC/USD", 60))
            .expect("track 60-second TWAP");
        let mut update: serde_json::Value =
            serde_json::from_str(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE)
                .expect("parse TWAP fixture");
        update["payload"]["timestamp"] = json!(u64::MAX);

        let error = feed
            .handle_text_message(&update.to_string())
            .expect_err("overflowing observation timestamp must fail visibly");
        assert!(error.to_string().contains("payload.timestamp"));
        feed.handle_text_message(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE)
            .expect("a later valid update must not be suppressed");

        let DataEvent::Data(NautilusData::Custom(_)) =
            rx.try_recv().expect("valid custom data event")
        else {
            panic!("expected custom data event");
        };
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_crypto_twap_update_rejects_publisher_timestamp_overflow_without_advancing_guard()
    {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("ALPHA/USD", 60))
            .expect("track 60-second TWAP");
        let mut valid: serde_json::Value =
            serde_json::from_str(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE)
                .expect("parse TWAP fixture");
        valid["payload"]["symbol"] = json!("alpha/usd");
        valid["payload"]["full_accuracy_value"] = json!("1234567890123456789");
        let mut invalid = valid.clone();
        invalid["timestamp"] = json!(u64::MAX);

        let error = feed
            .handle_text_message(&invalid.to_string())
            .expect_err("overflowing publisher timestamp must fail visibly");
        assert!(error.to_string().contains("envelope.timestamp"));
        assert!(rx.try_recv().is_err());

        feed.handle_text_message(&valid.to_string())
            .expect("a later valid update must not be suppressed");
        let DataEvent::Data(NautilusData::Custom(_)) =
            rx.try_recv().expect("valid custom data event")
        else {
            panic!("expected custom data event");
        };
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_crypto_twap_update_rejects_wrong_type_on_active_topic() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("BTC/USD", 60))
            .expect("track 60-second TWAP");
        let mut update: serde_json::Value =
            serde_json::from_str(RTDS_CRYPTO_TWAP_SIXTY_UPDATE_FIXTURE)
                .expect("parse TWAP fixture");
        update["type"] = json!("subscribe");

        let error = feed
            .handle_text_message(&update.to_string())
            .expect_err("unexpected type on subscribed topic must be visible");
        assert!(
            error
                .to_string()
                .contains("unsupported subscribed RTDS message")
        );
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_text_rejects_malformed_envelope_while_twap_topic_is_active() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("BTC/USD", 60))
            .expect("track 60-second TWAP");
        let frame = json!({
            "topic": "crypto_prices_twap_sixty",
            "type": "update",
            "timestamp": 1786179814147_u64
        });

        let error = feed
            .handle_text_message(&frame.to_string())
            .expect_err("malformed envelope on active TWAP topic must be visible");

        assert!(error.to_string().contains("invalid RTDS JSON frame"));
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_text_rejects_unclassifiable_json_while_twap_topic_is_active() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("BTC/USD", 60))
            .expect("track 60-second TWAP");
        let frame = json!({
            "type": "update",
            "timestamp": 1786179814147_u64,
            "payload": {}
        });

        let error = feed
            .handle_text_message(&frame.to_string())
            .expect_err("unclassifiable JSON must be visible while TWAP is active");

        assert!(error.to_string().contains("invalid RTDS JSON frame"));
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_text_ignores_malformed_unrelated_topic_while_twap_topic_is_active() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("BTC/USD", 60))
            .expect("track 60-second TWAP");
        let frame = json!({
            "topic": "unrelated_topic",
            "type": "update",
            "timestamp": 1786179814147_u64
        });

        feed.handle_text_message(&frame.to_string())
            .expect("malformed unrelated topic remains ignorable");

        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_text_ignores_unrelated_unsupported_topic() {
        let (feed, mut rx) = make_feed();
        let frame = json!({
            "topic": "unrelated_topic",
            "type": "update",
            "timestamp": 1786179814147_u64,
            "payload": {}
        });

        feed.handle_text_message(&frame.to_string())
            .expect("unrelated unsupported topic remains ignorable");

        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_crypto_price_update_emits_custom_data() {
        let (feed, mut rx) = make_feed();
        let data_type = crypto_data_type("btcusdt");
        feed.track_subscribe(data_type.clone())
            .expect("track subscribe");

        feed.handle_text_message(RTDS_CRYPTO_UPDATE_FIXTURE)
            .expect("valid crypto update");

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
        assert_eq!(payload.value, Price::from("64997.81"));
        assert_eq!(payload.price_timestamp_ms, 1786179814000);
        assert_eq!(payload.message_timestamp_ms, 1786179814147);
    }

    #[rstest]
    fn test_handle_crypto_price_update_emits_custom_data_for_new_symbol_on_shared_topic() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_data_type("btcusdt"))
            .expect("track BTC");
        let eth_data_type = crypto_data_type("ethusdt");
        feed.track_subscribe(eth_data_type.clone())
            .expect("track ETH");

        let mut update: serde_json::Value =
            serde_json::from_str(RTDS_CRYPTO_UPDATE_FIXTURE).expect("parse fixture");
        update["payload"]["symbol"] = json!("ethusdt");
        update["payload"]["value"] = json!(2450.11);
        update["payload"]["timestamp"] = json!(1780730270000_u64);
        update["timestamp"] = json!(1780730270142_u64);

        feed.handle_text_message(&update.to_string())
            .expect("valid crypto update");

        let event = rx.try_recv().expect("ETH custom data event");
        let DataEvent::Data(NautilusData::Custom(custom)) = event else {
            panic!("expected custom data event");
        };
        let payload = custom
            .data
            .as_any()
            .downcast_ref::<PolymarketRtdsCryptoPrice>()
            .expect("PolymarketRtdsCryptoPrice");

        assert_eq!(custom.data_type, eth_data_type);
        assert_eq!(payload.symbol, "ethusdt");
        assert_eq!(payload.value, Price::from("2450.11"));
        assert_eq!(payload.price_timestamp_ms, 1780730270000);
        assert_eq!(payload.message_timestamp_ms, 1780730270142);
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_crypto_price_update_emits_distinct_same_millisecond_points() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_data_type("btcusdt"))
            .expect("track subscribe");

        // Two distinct live updates sharing one millisecond timestamp must both emit;
        // only replayed snapshots collapse equal timestamps.
        feed.handle_text_message(RTDS_CRYPTO_UPDATE_FIXTURE)
            .expect("valid first crypto update");

        let mut second: serde_json::Value =
            serde_json::from_str(RTDS_CRYPTO_UPDATE_FIXTURE).expect("parse fixture");
        second["payload"]["value"] = json!(65000.12);
        feed.handle_text_message(&second.to_string())
            .expect("valid second crypto update");

        let first_event = rx.try_recv().expect("first custom data event");
        let second_event = rx.try_recv().expect("second custom data event");
        assert!(rx.try_recv().is_err());

        for (event, expected_value) in [(first_event, "64997.81"), (second_event, "65000.12")] {
            let DataEvent::Data(NautilusData::Custom(custom)) = event else {
                panic!("expected custom data event");
            };
            let payload = custom
                .data
                .as_any()
                .downcast_ref::<PolymarketRtdsCryptoPrice>()
                .expect("PolymarketRtdsCryptoPrice");

            assert_eq!(payload.value, Price::from(expected_value));
            assert_eq!(payload.price_timestamp_ms, 1786179814000);
        }
    }

    #[rstest]
    fn test_handle_crypto_price_subscribe_emits_snapshot_custom_data() {
        let (feed, mut rx) = make_feed();
        let data_type = crypto_data_type("BTCUSDT");
        feed.track_subscribe(data_type.clone())
            .expect("track subscribe");

        feed.handle_text_message(RTDS_CRYPTO_SUBSCRIBE_FIXTURE)
            .expect("valid crypto snapshot");

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

        feed.handle_text_message(RTDS_CRYPTO_SUBSCRIBE_FIXTURE)
            .expect("valid initial crypto snapshot");

        while rx.try_recv().is_ok() {}

        feed.handle_text_message(RTDS_CRYPTO_SUBSCRIBE_FIXTURE)
            .expect("valid replayed crypto snapshot");

        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_equity_price_update_emits_custom_data() {
        let (feed, mut rx) = make_feed();
        let data_type = equity_data_type("AAPL");
        feed.track_subscribe(data_type.clone())
            .expect("track subscribe");

        feed.handle_text_message(RTDS_EQUITY_UPDATE_FIXTURE)
            .expect("valid equity update");

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
    fn test_handle_equity_price_update_falls_back_when_full_accuracy_value_is_absent() {
        let (feed, mut rx) = make_feed();
        let data_type = equity_data_type("AAPL");
        feed.track_subscribe(data_type).expect("track subscribe");
        let mut update: serde_json::Value =
            serde_json::from_str(RTDS_EQUITY_UPDATE_FIXTURE).expect("parse fixture");
        update["payload"]
            .as_object_mut()
            .expect("payload object")
            .remove("full_accuracy_value");

        feed.handle_text_message(&update.to_string())
            .expect("valid equity update");

        let event = rx.try_recv().expect("custom data event");
        let DataEvent::Data(NautilusData::Custom(custom)) = event else {
            panic!("expected custom data event");
        };
        let payload = custom
            .data
            .as_any()
            .downcast_ref::<PolymarketRtdsEquityPrice>()
            .expect("PolymarketRtdsEquityPrice");

        assert_eq!(payload.value, Price::from("198.45"));
        assert_eq!(payload.full_accuracy_value, Price::from("198.45"));
    }

    #[rstest]
    fn test_handle_equity_price_update_emits_custom_data_for_new_symbol_on_shared_topic() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(equity_data_type("AAPL"))
            .expect("track AAPL");
        let msft_data_type = equity_data_type("MSFT");
        feed.track_subscribe(msft_data_type.clone())
            .expect("track MSFT");

        let mut update: serde_json::Value =
            serde_json::from_str(RTDS_EQUITY_UPDATE_FIXTURE).expect("parse fixture");
        update["payload"]["symbol"] = json!("msft");
        update["payload"]["value"] = json!(432.15);
        update["payload"]["full_accuracy_value"] = json!("432.1537");
        update["payload"]["timestamp"] = json!(1711382401000_u64);
        update["payload"]["received_at"] = json!(1711382401007_u64);
        update["timestamp"] = json!(1711382401020_u64);

        feed.handle_text_message(&update.to_string())
            .expect("valid equity update");

        let event = rx.try_recv().expect("MSFT custom data event");
        let DataEvent::Data(NautilusData::Custom(custom)) = event else {
            panic!("expected custom data event");
        };
        let payload = custom
            .data
            .as_any()
            .downcast_ref::<PolymarketRtdsEquityPrice>()
            .expect("PolymarketRtdsEquityPrice");

        assert_eq!(custom.data_type, msft_data_type);
        assert_eq!(payload.symbol, "msft");
        assert_eq!(payload.value, Price::from("432.15"));
        assert_eq!(payload.full_accuracy_value, Price::from("432.1537"));
        assert_eq!(payload.price_timestamp_ms, 1711382401000);
        assert_eq!(payload.message_timestamp_ms, 1711382401020);
        assert_eq!(payload.received_at_ms, Some(1711382401007));
        assert!(!payload.is_carried_forward);
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_equity_price_subscribe_emits_snapshot_custom_data() {
        let (feed, mut rx) = make_feed();
        let data_type = equity_data_type("AAPL");
        feed.track_subscribe(data_type.clone())
            .expect("track subscribe");

        feed.handle_text_message(RTDS_EQUITY_SUBSCRIBE_FIXTURE)
            .expect("valid equity snapshot");

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

        feed.handle_text_message(RTDS_EQUITY_SUBSCRIBE_FIXTURE)
            .expect("valid initial equity snapshot");

        while rx.try_recv().is_ok() {}

        feed.handle_text_message(RTDS_EQUITY_SUBSCRIBE_FIXTURE)
            .expect("valid replayed equity snapshot");

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
        feed.track_subscribe(crypto_twap_data_type("BTC/USD", 30))
            .expect("track 30-second TWAP");
        feed.track_subscribe(crypto_twap_data_type("BTC/USD", 60))
            .expect("track 60-second TWAP");

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
        let replay = payloads
            .iter()
            .find(|payload| {
                payload["action"].as_str() == Some("subscribe")
                    && payload["subscriptions"]
                        .as_array()
                        .is_some_and(|subscriptions| {
                            subscriptions.iter().any(|subscription| {
                                subscription["topic"].as_str()
                                    == Some(RtdsTopic::CryptoPrices.as_str())
                                    && subscription["filters"].is_null()
                            }) && subscriptions.iter().any(|subscription| {
                                subscription["topic"].as_str()
                                    == Some(RtdsTopic::EquityPrices.as_str())
                                    && subscription["filters"].is_null()
                            }) && subscriptions.iter().any(|subscription| {
                                subscription["topic"].as_str()
                                    == Some(RtdsTopic::CryptoPricesTwapThirty.as_str())
                                    && subscription["filters"].is_null()
                            }) && subscriptions.iter().any(|subscription| {
                                subscription["topic"].as_str()
                                    == Some(RtdsTopic::CryptoPricesTwapSixty.as_str())
                                    && subscription["filters"].is_null()
                            })
                        })
            })
            .expect("topic-level replay payload");

        let subscriptions = replay["subscriptions"]
            .as_array()
            .expect("subscriptions array");
        assert_eq!(subscriptions.len(), 4);
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
        *feed.inner.ws_client.lock() = Some(ws.clone());

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
    async fn test_server_disconnect_reconnects_and_resumes_multi_symbol_updates() {
        let state = TestServerState::default();
        let addr = start_rtds_server(state.clone()).await;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
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
        feed.connect().await.expect("connect feed");

        wait_until_async(
            || {
                let state = state.clone();
                async move {
                    state.connection_count().await >= 1
                        && !state.received_payloads.lock().await.is_empty()
                }
            },
            Duration::from_secs(5),
        )
        .await;

        state
            .send_text_to_all(build_crypto_update(
                "btcusdt",
                "61035.86",
                1780730269000,
                1780730269142,
            ))
            .await;
        state
            .send_text_to_all(build_crypto_update(
                "ethusdt",
                "2450.11",
                1780730270000,
                1780730270142,
            ))
            .await;

        wait_until_async(|| async { rx.len() >= 2 }, Duration::from_secs(2)).await;
        assert_eq!(
            collect_crypto_symbols(&mut rx, 2),
            vec!["btcusdt".to_string(), "ethusdt".to_string()],
            "both retained symbols should emit before disconnect",
        );

        state.clear_received_payloads().await;
        state.close_all_connections().await;

        wait_until_async(
            || {
                let state = state.clone();
                async move {
                    state.connection_count().await >= 2
                        && !state.received_payloads.lock().await.is_empty()
                }
            },
            Duration::from_secs(10),
        )
        .await;

        state
            .send_text_to_all(build_crypto_update(
                "btcusdt",
                "61040.12",
                1780730271000,
                1780730271142,
            ))
            .await;
        state
            .send_text_to_all(build_crypto_update(
                "ethusdt",
                "2455.55",
                1780730272000,
                1780730272142,
            ))
            .await;

        wait_until_async(|| async { rx.len() >= 2 }, Duration::from_secs(2)).await;
        assert_eq!(
            collect_crypto_symbols(&mut rx, 2),
            vec!["btcusdt".to_string(), "ethusdt".to_string()],
            "both retained symbols should resume after server-side reconnect",
        );

        feed.disconnect().await.expect("disconnect feed");
    }

    #[rstest]
    #[tokio::test]
    async fn test_server_disconnect_does_not_restore_unsubscribed_symbol() {
        let state = TestServerState::default();
        let addr = start_rtds_server(state.clone()).await;
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let feed = PolymarketRtdsFeed::new(
            format!("ws://{addr}/rtds"),
            TransportBackend::default(),
            get_atomic_clock_realtime(),
            tx,
        );

        let btc = crypto_data_type("BTCUSDT");
        let eth = crypto_data_type("ETHUSDT");
        feed.track_subscribe(btc.clone()).expect("track BTC");
        feed.track_subscribe(eth.clone()).expect("track ETH");
        feed.connect().await.expect("connect feed");

        wait_until_async(
            || {
                let state = state.clone();
                async move {
                    state.connection_count().await >= 1
                        && !state.received_payloads.lock().await.is_empty()
                }
            },
            Duration::from_secs(5),
        )
        .await;

        assert!(feed.track_unsubscribe(&eth).expect("unsubscribe ETH"));
        feed.reconcile_once(false)
            .await
            .expect("reconcile after unsubscribe");

        state.clear_received_payloads().await;
        state.close_all_connections().await;

        wait_until_async(
            || {
                let state = state.clone();
                async move {
                    state.connection_count().await >= 2
                        && !state.received_payloads.lock().await.is_empty()
                }
            },
            Duration::from_secs(10),
        )
        .await;

        state
            .send_text_to_all(build_crypto_update(
                "btcusdt",
                "61050.01",
                1780730273000,
                1780730273142,
            ))
            .await;
        state
            .send_text_to_all(build_crypto_update(
                "ethusdt",
                "2460.01",
                1780730274000,
                1780730274142,
            ))
            .await;

        wait_until_async(|| async { !rx.is_empty() }, Duration::from_secs(2)).await;
        assert_eq!(
            collect_crypto_symbols(&mut rx, 1),
            vec!["btcusdt".to_string()],
            "reconnect should restore only still-retained symbols",
        );
        assert!(
            rx.try_recv().is_err(),
            "ETH should not be replayed after unsubscribe"
        );

        feed.disconnect().await.expect("disconnect feed");
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
    async fn test_connect_without_subscriptions_clears_closing_for_later_subscribe() {
        let state = TestServerState::default();
        let addr = start_rtds_server(state.clone()).await;
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let feed = PolymarketRtdsFeed::new(
            format!("ws://{addr}/rtds"),
            TransportBackend::default(),
            get_atomic_clock_realtime(),
            tx,
        );

        feed.disconnect().await.expect("disconnect feed");
        feed.connect().await.expect("connect feed");

        assert!(
            feed.current_ws().is_none(),
            "connect without retained subscriptions should not open a socket",
        );

        feed.track_subscribe(crypto_data_type("BTCUSDT"))
            .expect("track BTC");
        feed.request_reconcile(ReconcileReason::DesiredChanged);

        wait_until_async(
            || {
                let state = state.clone();
                async move { !state.received_payloads.lock().await.is_empty() }
            },
            Duration::from_secs(2),
        )
        .await;

        let payloads = state.received_payloads.lock().await.clone();
        let subscribe = payloads.last().expect("subscribe payload");
        assert_eq!(subscribe["action"].as_str(), Some("subscribe"));
        feed.disconnect().await.expect("disconnect feed");
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
        *feed.inner.ws_client.lock() = Some(ws.clone());
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
                    feed.task_slots()
                        .expect("RTDS task owner")
                        .reconcile
                        .lock()
                        .await
                        .as_ref()
                        .is_some_and(|handle| !handle.is_finished())
                }
            },
            Duration::from_secs(2),
        )
        .await;

        feed.disconnect().await.expect("disconnect feed");

        assert!(
            feed.task_slots()
                .expect("RTDS task owner")
                .reconcile
                .lock()
                .await
                .is_none(),
            "disconnect should clear the reconcile worker handle",
        );
    }

    #[tokio::test]
    async fn test_last_feed_owner_drop_aborts_worker_tasks() {
        struct NotifyOnDrop(Option<tokio::sync::oneshot::Sender<()>>);

        impl Drop for NotifyOnDrop {
            fn drop(&mut self) {
                if let Some(tx) = self.0.take() {
                    let _ = tx.send(());
                }
            }
        }

        let (feed, _rx) = make_feed();
        let (message_tx, mut message_rx) = tokio::sync::oneshot::channel();
        let (reconcile_tx, mut reconcile_rx) = tokio::sync::oneshot::channel();
        feed.task_slots()
            .expect("RTDS task owner")
            .message
            .lock()
            .await
            .insert(tokio::spawn(async move {
                let _notify = NotifyOnDrop(Some(message_tx));
                std::future::pending::<()>().await;
            }));
        feed.task_slots()
            .expect("RTDS task owner")
            .reconcile
            .lock()
            .await
            .insert(tokio::spawn(async move {
                let _notify = NotifyOnDrop(Some(reconcile_tx));
                std::future::pending::<()>().await;
            }));
        tokio::task::yield_now().await;
        let clone = feed.clone();

        drop(feed);

        assert!(matches!(
            message_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));
        assert!(matches!(
            reconcile_rx.try_recv(),
            Err(tokio::sync::oneshot::error::TryRecvError::Empty)
        ));

        drop(clone);

        tokio::time::timeout(Duration::from_secs(1), &mut message_rx)
            .await
            .expect("message task dropped")
            .expect("message task drop signal");
        tokio::time::timeout(Duration::from_secs(1), &mut reconcile_rx)
            .await
            .expect("reconcile task dropped")
            .expect("reconcile task drop signal");
    }

    #[rstest]
    #[tokio::test]
    async fn test_reconcile_worker_exits_when_shutdown_arrives_during_reconcile() {
        let (feed, _rx) = make_feed();
        feed.track_subscribe(crypto_data_type("BTCUSDT"))
            .expect("track BTC");

        let guard = feed.inner.wire_mutex.lock().await;
        feed.request_reconcile(ReconcileReason::DesiredChanged);

        wait_until_async(
            || {
                let feed = feed.clone();
                async move { !feed.inner.reconcile_pending.load(Ordering::Acquire) }
            },
            Duration::from_secs(2),
        )
        .await;

        feed.inner.closing.store(true, Ordering::Release);
        feed.inner.reconcile_notify.notify_waiters();

        drop(guard);

        wait_until_async(
            || {
                let feed = feed.clone();
                async move {
                    feed.task_slots()
                        .expect("RTDS task owner")
                        .reconcile
                        .lock()
                        .await
                        .as_ref()
                        .is_some_and(tokio::task::JoinHandle::is_finished)
                }
            },
            Duration::from_secs(2),
        )
        .await;
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
                feed.disconnect().await.expect("disconnect feed");
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
