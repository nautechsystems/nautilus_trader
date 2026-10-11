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

//! Private Polymarket PolyBolt reference-price feed support.
//!
//! PolyBolt (`wss://ws-live-v2.polymarket.com/ws`) serves crypto, equity, and Chainlink TWAP
//! reference prices on authenticated `price.*` channels. The feed authenticates with CLOB API
//! credentials before subscribing, tracks one wire subscription per `(channel, symbol)`, and
//! normalizes snapshot and live envelopes into Polymarket custom data types.

use std::{
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
use nautilus_common::{live::sender::EventSender, messages::DataEvent};
use nautilus_core::{UnixNanos, string::secret::SecretString, time::AtomicTime};
use nautilus_live::{
    SocketControl,
    task::{TaskJoinOutcome, TaskSlot, finish_task},
};
use nautilus_model::data::{CustomData, Data as NautilusData, DataType, custom::CustomDataTrait};
use nautilus_network::{
    RECONNECTED, SocketStateSink,
    http::create_standard_nautilus_headers,
    websocket::{
        AuthTracker, TransportBackend, WebSocketClient, WebSocketConfig, channel_message_handler,
        proxy::ProxyUrl,
    },
};
use parking_lot::Mutex;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;
use tokio_tungstenite::tungstenite::Message;
use zeroize::Zeroize;

use crate::{
    common::{
        credential::Credential,
        parse::{
            deserialize_crypto_twap_value, deserialize_decimal_from_json_number,
            parse_decimal_exact,
        },
    },
    data_types::{PolymarketCryptoPrice, PolymarketCryptoTwap, PolymarketEquityPrice},
    websocket::messages::PolymarketWsAuth,
};

// The venue sends a WebSocket ping every 25 seconds and closes after two missed pongs. The
// shared client answers pings and counts every inbound frame as liveness, so allow two
// missed server pings before treating the connection as dead.
const POLYBOLT_HEARTBEAT_TIMEOUT_SECS: u64 = 60;
const POLYBOLT_AUTH_TIMEOUT_SECS: u64 = 10;
// ponytail: fixed retry delay for transient auth failures; add backoff if the venue ever
// rate-limits auth retries at this cadence (8 auth frames per connection is the hard cap).
const POLYBOLT_AUTH_RETRY_DELAY_SECS: u64 = 5;
const POLYBOLT_RECONNECT_TIMEOUT_MS: u64 = 15_000;
const POLYBOLT_RECONNECT_DELAY_INITIAL_MS: u64 = 250;
const POLYBOLT_RECONNECT_DELAY_MAX_MS: u64 = 5_000;
const POLYBOLT_RECONNECT_JITTER_MS: u64 = 200;
/// Venue limit: active `(channel, filter)` subscriptions per connection.
pub(crate) const POLYBOLT_MAX_SUBSCRIPTIONS: usize = 64;
/// The only TWAP window PolyBolt serves.
const POLYBOLT_TWAP_WINDOW_SECONDS: u32 = 60;
const POLYMARKET_CRYPTO_PRICE_TYPE_NAME: &str = "PolymarketCryptoPrice";
const POLYMARKET_CRYPTO_TWAP_TYPE_NAME: &str = "PolymarketCryptoTwap";
const POLYMARKET_EQUITY_PRICE_TYPE_NAME: &str = "PolymarketEquityPrice";

pub(crate) fn is_supported_polybolt_data_type(data_type: &DataType) -> bool {
    matches!(
        data_type.type_name(),
        POLYMARKET_CRYPTO_PRICE_TYPE_NAME
            | POLYMARKET_CRYPTO_TWAP_TYPE_NAME
            | POLYMARKET_EQUITY_PRICE_TYPE_NAME
    )
}

#[derive(Clone, Debug)]
pub(crate) struct PolymarketPolyBoltFeed {
    inner: Arc<PolymarketPolyBoltFeedInner>,
    _task_owner: Option<Arc<PolyBoltTaskSlots>>,
    task_slots: Weak<PolyBoltTaskSlots>,
}

#[derive(Debug)]
struct PolyBoltTaskSlots {
    message: tokio::sync::Mutex<TaskSlot<()>>,
    reconcile: tokio::sync::Mutex<TaskSlot<()>>,
    shutdown_errors: Mutex<Vec<String>>,
}

struct PolyBoltWebSocketGuard<'a> {
    owner: &'a Mutex<Option<Arc<WebSocketClient>>>,
    ws: Option<Arc<WebSocketClient>>,
}

impl<'a> PolyBoltWebSocketGuard<'a> {
    fn take(owner: &'a Mutex<Option<Arc<WebSocketClient>>>) -> Self {
        let ws = owner.lock().take();
        Self { owner, ws }
    }

    fn clear(&mut self) {
        self.ws = None;
    }
}

impl Drop for PolyBoltWebSocketGuard<'_> {
    fn drop(&mut self) {
        if self.ws.is_some() {
            *self.owner.lock() = self.ws.take();
        }
    }
}

impl PolyBoltTaskSlots {
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
                "Polymarket PolyBolt task shutdown failed: {}",
                errors.join("; ")
            )
        }
    }
}

impl Drop for PolyBoltTaskSlots {
    fn drop(&mut self) {
        self.message.get_mut().abort();
        self.reconcile.get_mut().abort();
    }
}

#[derive(Debug)]
struct PolymarketPolyBoltFeedInner {
    url: String,
    proxy_url: Option<ProxyUrl>,
    transport_backend: TransportBackend,
    clock: &'static AtomicTime,
    data_sender: EventSender<DataEvent>,
    socket_sink: Option<SocketStateSink>,
    socket_control: Option<SocketControl>,
    credential: Option<Credential>,
    auth: AuthTracker,
    // Set when the venue definitively rejects the credentials. Cleared only by an explicit
    // `connect`, so a reconnect loop cannot burn the venue's auth-frame budget.
    auth_rejected: AtomicBool,
    /// Set when the next reconcile attempt must wait out the auth retry delay first.
    auth_retry_pending: AtomicBool,
    subscriptions: dashmap::DashMap<String, TrackedSubscription>,
    last_emitted_timestamps_ms: dashmap::DashMap<String, u64>,
    // Tracks the last venue state we successfully pushed so incremental syncs
    // can send only the delta from desired state to live wire state.
    live_subscriptions: Mutex<AHashMap<String, PolyBoltSubscription>>,
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
    wire: PolyBoltSubscription,
    total_ref_count: usize,
    data_types: AHashMap<String, TrackedDataType>,
    last_twap_fingerprint: Option<TwapReplayFingerprint>,
}

#[derive(Clone, Debug)]
struct TrackedDataType {
    data_type: DataType,
    ref_count: usize,
}

/// Subscribe or unsubscribe request: `{"op": ..., "subscriptions": [...]}`.
#[derive(Debug, Serialize)]
struct PolyBoltSubscribeRequest<'a> {
    op: &'static str,
    subscriptions: &'a [PolyBoltSubscription],
}

/// Authentication request: `{"op": "auth", "auth": {apiKey, secret, passphrase}}`.
#[derive(Debug, Serialize, Zeroize)]
struct PolyBoltAuthRequest {
    #[zeroize(skip)]
    op: &'static str,
    auth: PolymarketWsAuth,
}

/// One `(channel, filter)` wire subscription.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct PolyBoltSubscription {
    channel: &'static str,
    filter: PolyBoltFilter,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct PolyBoltFilter {
    symbol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    window_seconds: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PolyBoltChannel {
    CryptoPrice,
    CryptoTwap,
    EquityPrice,
}

impl PolyBoltChannel {
    const fn as_str(self) -> &'static str {
        match self {
            Self::CryptoPrice => "price.crypto",
            Self::CryptoTwap => "price.crypto.twap",
            Self::EquityPrice => "price.equity",
        }
    }

    fn parse(channel: &str) -> Option<Self> {
        match channel {
            "price.crypto" => Some(Self::CryptoPrice),
            "price.crypto.twap" => Some(Self::CryptoTwap),
            "price.equity" => Some(Self::EquityPrice),
            _ => None,
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

/// Marker for authentication failures the venue asks clients to retry.
#[derive(Debug, thiserror::Error)]
#[error("PolyBolt authentication did not complete: {0}")]
struct AuthRetryable(String);

#[derive(Clone, Debug)]
struct ParsedSubscription {
    key: String,
    wire: PolyBoltSubscription,
}

/// Control frame from the venue, distinguished from data envelopes by `op`.
#[derive(Debug, Deserialize)]
#[serde(tag = "op")]
enum PolyBoltControl {
    #[serde(rename = "authed")]
    Authed { rid: Option<String> },
    #[serde(rename = "subscribed")]
    Subscribed {
        channel: String,
        rid: Option<String>,
        provider: Option<String>,
    },
    #[serde(rename = "unsubscribed")]
    Unsubscribed {
        channel: String,
        rid: Option<String>,
    },
    #[serde(rename = "error")]
    Error {
        code: String,
        channel: Option<String>,
        rid: Option<String>,
    },
    #[serde(rename = "pong")]
    Pong { rid: Option<String> },
    #[serde(other)]
    Unknown,
}

/// Data envelope: `{v, channel, seq, ts, snapshot?, dropped?, payload}`.
#[derive(Debug, Deserialize)]
struct PolyBoltEnvelope {
    #[allow(dead_code, reason = "modeled for PolyBolt envelope conformance")]
    v: u8,
    channel: String,
    #[allow(dead_code, reason = "modeled for PolyBolt envelope conformance")]
    seq: u64,
    ts: u64,
    #[serde(default)]
    snapshot: bool,
    #[serde(default)]
    dropped: Option<u64>,
    payload: Box<RawValue>,
}

/// Live `price.crypto` and `price.equity` payload.
#[derive(Debug, Deserialize)]
struct PricePayloadRaw {
    symbol: String,
    source: String,
    timestamp: u64,
    #[serde(deserialize_with = "deserialize_decimal_from_json_number")]
    value: Decimal,
    #[serde(default)]
    full_accuracy_value: Option<String>,
    #[serde(default)]
    received_at: Option<u64>,
    #[serde(default)]
    is_carried_forward: Option<bool>,
}

/// Live `price.crypto.twap` payload.
#[derive(Debug, Deserialize)]
struct TwapPayloadRaw {
    symbol: String,
    source: String,
    timestamp: u64,
    #[serde(rename = "value", deserialize_with = "deserialize_crypto_twap_value")]
    #[allow(
        dead_code,
        reason = "display-only field validated for wire conformance, never published"
    )]
    display_value: Decimal,
    full_accuracy_value: String,
    window_seconds: u32,
}

/// Snapshot payload shared by every price channel.
#[derive(Debug, Deserialize)]
struct SnapshotPayloadRaw {
    symbol: String,
    source: String,
    #[serde(default)]
    window_seconds: Option<u32>,
    data: Vec<SnapshotPointRaw>,
}

#[derive(Debug, Deserialize)]
struct SnapshotPointRaw {
    timestamp: u64,
    #[serde(deserialize_with = "deserialize_decimal_from_json_number")]
    value: Decimal,
    #[serde(default)]
    full_accuracy_value: Option<String>,
}

impl PolymarketPolyBoltFeed {
    #[cfg(test)]
    pub(crate) fn new(
        url: String,
        transport_backend: TransportBackend,
        clock: &'static AtomicTime,
        data_sender: EventSender<DataEvent>,
    ) -> Self {
        Self::new_with_proxy(url, transport_backend, clock, data_sender, None)
    }

    #[cfg(test)]
    pub(crate) fn new_with_proxy(
        url: String,
        transport_backend: TransportBackend,
        clock: &'static AtomicTime,
        data_sender: EventSender<DataEvent>,
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
        data_sender: EventSender<DataEvent>,
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
            Some(fixture_credential()),
        )
    }

    pub(crate) fn new_with_proxy_and_socket_control(
        url: String,
        transport_backend: TransportBackend,
        clock: &'static AtomicTime,
        data_sender: EventSender<DataEvent>,
        proxy_url: Option<ProxyUrl>,
        socket_control: Option<SocketControl>,
        credential: Option<Credential>,
    ) -> Self {
        Self::new_inner(
            url,
            transport_backend,
            clock,
            data_sender,
            proxy_url,
            None,
            socket_control,
            credential,
        )
    }

    #[allow(clippy::too_many_arguments, reason = "private constructor funnel")]
    fn new_inner(
        url: String,
        transport_backend: TransportBackend,
        clock: &'static AtomicTime,
        data_sender: EventSender<DataEvent>,
        proxy_url: Option<ProxyUrl>,
        socket_sink: Option<SocketStateSink>,
        socket_control: Option<SocketControl>,
        credential: Option<Credential>,
    ) -> Self {
        let task_owner = Arc::new(PolyBoltTaskSlots {
            message: tokio::sync::Mutex::new(TaskSlot::new()),
            reconcile: tokio::sync::Mutex::new(TaskSlot::new()),
            shutdown_errors: Mutex::new(Vec::new()),
        });
        Self {
            inner: Arc::new(PolymarketPolyBoltFeedInner {
                url,
                proxy_url,
                transport_backend,
                clock,
                data_sender,
                socket_sink,
                socket_control,
                credential,
                auth: AuthTracker::new(),
                auth_rejected: AtomicBool::new(false),
                auth_retry_pending: AtomicBool::new(false),
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

    fn task_slots(&self) -> Option<Arc<PolyBoltTaskSlots>> {
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

    #[cfg(test)]
    pub(crate) fn is_authenticated(&self) -> bool {
        self.inner.auth.is_authenticated()
    }

    pub(crate) fn track_subscribe(&self, data_type: DataType) -> anyhow::Result<bool> {
        let parsed = ParsedSubscription::from_data_type(&data_type)?;

        if self.inner.credential.is_none() {
            anyhow::bail!(
                "{} subscriptions require CLOB API credentials: set api_key, api_secret and \
                 passphrase on PolymarketDataClientConfig or POLYMARKET_API_KEY, \
                 POLYMARKET_API_SECRET and POLYMARKET_PASSPHRASE",
                data_type.type_name(),
            );
        }

        // ponytail: one connection; shard across connections if anyone needs more than 64.
        // Checked before taking the entry: `len` locks every shard and the entry holds one.
        if !self.inner.subscriptions.contains_key(&parsed.key)
            && self.inner.subscriptions.len() >= POLYBOLT_MAX_SUBSCRIPTIONS
        {
            anyhow::bail!(
                "PolyBolt allows {POLYBOLT_MAX_SUBSCRIPTIONS} price subscriptions per \
                 connection; cannot add {}",
                parsed.key,
            );
        }

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
                anyhow::bail!("PolyBolt connect was canceled by shutdown");
            }
            self.inner.closing.store(false, Ordering::Release);
            self.inner.auth_rejected.store(false, Ordering::Release);
        }

        self.ensure_reconcile_worker();

        // A venue-side auth outage must not fail the data client connect: the market
        // socket stays up and the reconcile worker retries the handshake after a delay.
        if let Err(e) = self.reconcile_once(false).await {
            if e.downcast_ref::<AuthRetryable>().is_none() {
                return Err(e);
            }

            log::warn!("{e}; retrying in {POLYBOLT_AUTH_RETRY_DELAY_SECS}s");
            self.schedule_auth_retry();
        }

        let current_generation = self.inner.shutdown_generation.lock();
        if *current_generation != generation || self.inner.closing.load(Ordering::Acquire) {
            anyhow::bail!("PolyBolt connect was canceled by shutdown");
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
            anyhow::bail!("PolyBolt task owner was dropped");
        };

        let mut message_slot = tasks.message.lock().await;
        if let Some(outcome) =
            finish_task(&mut message_slot, Duration::ZERO, Duration::from_secs(2)).await
        {
            match outcome {
                TaskJoinOutcome::Completed(()) | TaskJoinOutcome::Aborted => {}
                TaskJoinOutcome::Failed(error) => {
                    tasks.push_shutdown_error(format!("PolyBolt message loop failed: {error}"));
                }
                TaskJoinOutcome::Incomplete => {
                    tasks.push_shutdown_error(
                        "PolyBolt message loop did not stop after abort".to_string(),
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
                    tasks.push_shutdown_error(format!("PolyBolt reconcile worker failed: {error}"));
                }
                TaskJoinOutcome::Incomplete => {
                    tasks.push_shutdown_error(
                        "PolyBolt reconcile worker did not stop after abort".to_string(),
                    );
                }
            }
        }

        tasks.take_shutdown_result()
    }

    fn schedule_auth_retry(&self) {
        self.inner.auth_retry_pending.store(true, Ordering::Release);
        self.inner.reconcile_pending.store(true, Ordering::Release);
        self.inner.reconcile_notify.notify_one();
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
            anyhow::bail!("Polymarket PolyBolt task owner was dropped");
        };
        let mut ws = PolyBoltWebSocketGuard::take(&self.inner.ws_client);

        if let Some(client) = ws.ws.as_ref() {
            client.disconnect().await;
        }

        if let Some(control) = &self.inner.socket_control {
            control.deregister();
        }

        self.inner.live_subscriptions.lock().clear();
        self.inner.auth.invalidate();
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
                    tasks.push_shutdown_error(format!("PolyBolt message loop failed: {error}"));
                }
                TaskJoinOutcome::Incomplete => {
                    tasks.push_shutdown_error(
                        "PolyBolt message loop did not stop after abort".to_string(),
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
                    tasks.push_shutdown_error(format!("PolyBolt reconcile worker failed: {error}"));
                }
                TaskJoinOutcome::Incomplete => {
                    tasks.push_shutdown_error(
                        "PolyBolt reconcile worker did not stop after abort".to_string(),
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
        // Release any reconcile waiting on `authed` so shutdown is not delayed by the auth timeout.
        self.inner.auth.fail("PolyBolt feed shutting down");

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
            log::error!("Failed to start PolyBolt reconcile worker: {e}");
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
                if self.inner.auth_retry_pending.swap(false, Ordering::AcqRel) {
                    tokio::time::sleep(Duration::from_secs(POLYBOLT_AUTH_RETRY_DELAY_SECS)).await;

                    if self.inner.closing.load(Ordering::Acquire) {
                        break;
                    }
                }

                let reset_live_state = self
                    .inner
                    .reset_live_state_pending
                    .swap(false, Ordering::AcqRel);

                let Err(e) = self.reconcile_once(reset_live_state).await else {
                    continue;
                };

                if self.inner.closing.load(Ordering::Acquire) {
                    log::debug!("PolyBolt reconcile stopped by shutdown: {e}");
                    break;
                }

                log::error!("Failed to reconcile PolyBolt custom data subscriptions: {e}");

                if e.downcast_ref::<AuthRetryable>().is_some() {
                    self.inner.auth_retry_pending.store(true, Ordering::Release);
                    self.inner.reconcile_pending.store(true, Ordering::Release);
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
            anyhow::bail!("PolyBolt WebSocket client unavailable after reconcile");
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
                .context("failed to connect Polymarket PolyBolt WebSocket")?,
        );

        if !self.is_generation_open(generation) {
            ws.notify_closed();
            ws.disconnect().await;
            anyhow::bail!("PolyBolt connection was canceled by shutdown");
        }

        log::debug!(
            "Polymarket PolyBolt WebSocket connected: {}",
            self.inner.url
        );
        *self.inner.ws_client.lock() = Some(Arc::clone(&ws));
        self.inner.auth.invalidate();

        // Tokio cancellation is cooperative. Quiesce the previous loop before
        // activating the replacement so an admitted old-loop tail cannot emit
        // after newer data from the new connection.
        let tasks = self
            .task_slots()
            .ok_or_else(|| anyhow::anyhow!("PolyBolt task owner was dropped"))?;
        let mut message_slot = tasks.message.lock().await;
        if !self.is_generation_open(generation) {
            drop(message_slot);
            ws.notify_closed();
            ws.disconnect().await;
            self.clear_ws_if_current(&ws);
            anyhow::bail!("PolyBolt connection was canceled by shutdown");
        }
        message_slot.abort();
        let previous_loop_error = if let Some(outcome) =
            finish_task(&mut message_slot, Duration::ZERO, Duration::from_secs(2)).await
        {
            match outcome {
                TaskJoinOutcome::Completed(()) | TaskJoinOutcome::Aborted => None,
                TaskJoinOutcome::Failed(error) => {
                    Some(format!("previous PolyBolt message loop failed: {error}"))
                }
                TaskJoinOutcome::Incomplete => {
                    Some("previous PolyBolt message loop did not stop after abort".to_string())
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
            anyhow::bail!("PolyBolt connection was canceled by shutdown");
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
                    "Failed to start PolyBolt message loop: {e}; startup rollback failed: \
                     {shutdown_error}"
                ),
                None => format!("Failed to start PolyBolt message loop: {e}"),
            });
        }

        Ok(true)
    }

    fn is_generation_open(&self, generation: u64) -> bool {
        let current_generation = self.inner.shutdown_generation.lock();
        *current_generation == generation && !self.inner.closing.load(Ordering::Acquire)
    }

    fn websocket_config(&self) -> WebSocketConfig {
        let headers = create_standard_nautilus_headers();

        WebSocketConfig {
            url: self.inner.url.clone(),
            headers,
            heartbeat_interval_secs: None,
            heartbeat_payload: None,
            connect_timeout_ms: Some(POLYBOLT_RECONNECT_TIMEOUT_MS),
            reconnect_delay_initial_ms: Some(POLYBOLT_RECONNECT_DELAY_INITIAL_MS),
            reconnect_delay_max_ms: Some(POLYBOLT_RECONNECT_DELAY_MAX_MS),
            reconnect_backoff_factor: Some(2.0),
            reconnect_jitter_ms: Some(POLYBOLT_RECONNECT_JITTER_MS),
            reconnect_max_attempts: None,
            heartbeat_timeout_secs: Some(POLYBOLT_HEARTBEAT_TIMEOUT_SECS),
            idle_timeout_ms: None,
            writer_capacity: None,
            backend: self.inner.transport_backend,
            proxy_url: self
                .inner
                .proxy_url
                .as_ref()
                .map(|url| url.expose().to_string()),
            max_message_size_bytes: None,
            max_frame_size_bytes: None,
        }
    }

    fn snapshot_wire_subscriptions(&self) -> AHashMap<String, PolyBoltSubscription> {
        self.inner
            .subscriptions
            .iter()
            .map(|entry| (entry.key().clone(), entry.wire.clone()))
            .collect()
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
            self.inner.auth.invalidate();
        }

        self.ensure_authenticated_locked(ws).await?;

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
                "Unsubscribing Polymarket PolyBolt delta with {} subscription(s)",
                unsubscribe.len()
            );
            self.send_wire_request(ws, "unsubscribe", &unsubscribe)
                .await?;
        }

        if !subscribe.is_empty() {
            log::debug!(
                "Subscribing Polymarket PolyBolt delta with {} subscription(s)",
                subscribe.len()
            );
            self.send_wire_request(ws, "subscribe", &subscribe).await?;
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

    /// Sends the auth frame and waits for `authed` unless the session is already authenticated.
    async fn ensure_authenticated_locked(&self, ws: &Arc<WebSocketClient>) -> anyhow::Result<()> {
        if self.inner.auth.is_authenticated() {
            return Ok(());
        }

        if self.inner.auth_rejected.load(Ordering::Acquire) {
            anyhow::bail!(
                "PolyBolt rejected the CLOB API credentials; fix api_key, api_secret and \
                 passphrase, then reconnect the data client"
            );
        }

        let Some(credential) = self.inner.credential.as_ref() else {
            anyhow::bail!("PolyBolt price channels require CLOB API credentials");
        };

        let receiver = self.inner.auth.begin();
        let request = PolyBoltAuthRequest {
            op: "auth",
            auth: PolymarketWsAuth {
                api_key: SecretString::from(credential.api_key_str()),
                secret: credential.api_secret(),
                passphrase: SecretString::from(credential.passphrase()),
            },
        };
        let payload = serde_json::to_string(&request)?;
        ws.send_text(payload, None)
            .await
            .map_err(|e| anyhow::anyhow!("failed to send PolyBolt auth request: {e}"))?;

        match self
            .inner
            .auth
            .wait_for_result::<String>(Duration::from_secs(POLYBOLT_AUTH_TIMEOUT_SECS), receiver)
            .await
        {
            Ok(()) => {
                log::debug!("Polymarket PolyBolt authenticated");
                Ok(())
            }
            Err(e) if self.inner.auth_rejected.load(Ordering::Acquire) => {
                anyhow::bail!("PolyBolt rejected the CLOB API credentials: {e}")
            }
            Err(e) => Err(AuthRetryable(e).into()),
        }
    }

    async fn send_wire_request(
        &self,
        ws: &Arc<WebSocketClient>,
        op: &'static str,
        subscriptions: &[PolyBoltSubscription],
    ) -> anyhow::Result<()> {
        if subscriptions.is_empty() {
            return Ok(());
        }

        let request = PolyBoltSubscribeRequest { op, subscriptions };
        let payload = serde_json::to_string(&request)?;
        ws.send_text(payload, None)
            .await
            .map_err(|e| anyhow::anyhow!("failed to send PolyBolt {op} request: {e}"))
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
                        log::info!("Polymarket PolyBolt reconnected");
                        self.inner.auth.invalidate();
                        self.request_reconcile(ReconcileReason::TransportReset);
                        continue;
                    }

                    if let Err(e) = self.handle_text_message(text.as_str()) {
                        log::error!(
                            "Failed to handle subscribed Polymarket PolyBolt message: {e:#}"
                        );
                    }
                }
                Some(Message::Binary(_)) => {
                    log::debug!("Ignoring binary PolyBolt message");
                }
                Some(other) => {
                    log::debug!("Ignoring PolyBolt control message: {other:?}");
                }
                None => {
                    log::debug!("PolyBolt message channel closed");

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

        let envelope_error = match serde_json::from_str::<PolyBoltEnvelope>(text) {
            Ok(envelope) => return self.handle_envelope(&envelope),
            Err(e) => e,
        };

        if let Ok(control) = serde_json::from_str::<PolyBoltControl>(text) {
            self.handle_control(control);
            return Ok(());
        }

        if self.malformed_frame_requires_visible_twap_failure(text) {
            return Err(anyhow::Error::new(envelope_error).context("invalid PolyBolt JSON frame"));
        }

        log::debug!("Ignoring non-PolyBolt JSON frame: {envelope_error}");
        Ok(())
    }

    fn handle_control(&self, control: PolyBoltControl) {
        match control {
            PolyBoltControl::Authed { rid } => {
                log::debug!("PolyBolt authed rid={rid:?}");
                self.inner.auth.succeed();
            }
            PolyBoltControl::Subscribed {
                channel,
                rid,
                provider,
            } => {
                log::debug!(
                    "PolyBolt subscribed channel={channel} provider={provider:?} rid={rid:?}"
                );
            }
            PolyBoltControl::Unsubscribed { channel, rid } => {
                log::debug!("PolyBolt unsubscribed channel={channel} rid={rid:?}");
            }
            PolyBoltControl::Error { code, channel, rid } => match code.as_str() {
                "auth_invalid" | "auth_expired" | "auth_attempts" => {
                    log::error!(
                        "PolyBolt rejected the CLOB API credentials code={code} rid={rid:?}; \
                         fix api_key, api_secret and passphrase, then reconnect the data client"
                    );
                    self.inner.auth_rejected.store(true, Ordering::Release);
                    self.inner.auth.fail(code);
                }
                "auth_unavailable" => {
                    log::warn!("PolyBolt authentication unavailable, retrying later rid={rid:?}");
                    self.inner.auth.fail(code);
                }
                _ => {
                    log::error!("PolyBolt error code={code} channel={channel:?} rid={rid:?}");
                }
            },
            PolyBoltControl::Pong { rid } => {
                log::trace!("PolyBolt pong rid={rid:?}");
            }
            PolyBoltControl::Unknown => {
                log::debug!("Ignoring unsupported PolyBolt control frame");
            }
        }
    }

    fn handle_envelope(&self, envelope: &PolyBoltEnvelope) -> anyhow::Result<()> {
        if let Some(dropped) = envelope.dropped {
            log::warn!(
                "PolyBolt conflated {dropped} frame(s) on channel={} (slow consumer)",
                envelope.channel,
            );
        }

        match (PolyBoltChannel::parse(&envelope.channel), envelope.snapshot) {
            (Some(PolyBoltChannel::CryptoTwap), true) => self.handle_twap_snapshot(envelope),
            (Some(PolyBoltChannel::CryptoTwap), false) => self.handle_twap_update(envelope),
            (Some(channel), true) => {
                self.handle_price_snapshot(envelope, channel);
                Ok(())
            }
            (Some(channel), false) => {
                self.handle_price_update(envelope, channel);
                Ok(())
            }
            (None, _) => {
                log::debug!(
                    "Ignoring unsupported PolyBolt message channel={}",
                    envelope.channel,
                );
                Ok(())
            }
        }
    }

    fn handle_price_update(&self, envelope: &PolyBoltEnvelope, channel: PolyBoltChannel) {
        let payload: PricePayloadRaw = match serde_json::from_str(envelope.payload.get()) {
            Ok(payload) => payload,
            Err(e) => {
                log::warn!("Failed to parse PolyBolt {} payload: {e}", channel.as_str());
                return;
            }
        };

        let symbol_lower = payload.symbol.to_ascii_lowercase();
        let data_types = self.matching_data_types(channel, &symbol_lower);
        if data_types.is_empty() {
            return;
        }

        let value = match exact_value(payload.full_accuracy_value.as_deref(), payload.value) {
            Ok(value) => value,
            Err(e) => {
                log::error!("Failed to parse PolyBolt {} value: {e}", channel.as_str());
                return;
            }
        };

        let ts_event = match unix_nanos_from_millis("payload.timestamp", payload.timestamp) {
            Ok(ts_event) => ts_event,
            Err(e) => {
                log::error!(
                    "Failed to parse PolyBolt {} timestamp: {e}",
                    channel.as_str()
                );
                return;
            }
        };

        if !self.should_emit_timestamp_ms(
            channel,
            &symbol_lower,
            payload.timestamp,
            TimestampGuard::Live,
        ) {
            return;
        }
        let ts_init = self.inner.clock.get_time_ns();

        match channel {
            PolyBoltChannel::EquityPrice => {
                let custom_payload = Arc::new(PolymarketEquityPrice::new(
                    symbol_lower,
                    payload.source,
                    value,
                    payload.timestamp,
                    envelope.ts,
                    payload.received_at,
                    payload.is_carried_forward.unwrap_or(false),
                    ts_event,
                    ts_init,
                ));
                self.emit_custom_payload(&custom_payload, data_types);
            }
            PolyBoltChannel::CryptoPrice | PolyBoltChannel::CryptoTwap => {
                let custom_payload = Arc::new(PolymarketCryptoPrice::new(
                    symbol_lower,
                    payload.source,
                    value,
                    payload.timestamp,
                    envelope.ts,
                    ts_event,
                    ts_init,
                ));
                self.emit_custom_payload(&custom_payload, data_types);
            }
        }
    }

    fn handle_price_snapshot(&self, envelope: &PolyBoltEnvelope, channel: PolyBoltChannel) {
        let payload: SnapshotPayloadRaw = match serde_json::from_str(envelope.payload.get()) {
            Ok(payload) => payload,
            Err(e) => {
                log::warn!(
                    "Failed to parse PolyBolt {} snapshot payload: {e}",
                    channel.as_str()
                );
                return;
            }
        };

        let symbol_lower = payload.symbol.to_ascii_lowercase();
        let data_types = self.matching_data_types(channel, &symbol_lower);
        if data_types.is_empty() {
            return;
        }

        for point in payload.data {
            let value = match exact_value(point.full_accuracy_value.as_deref(), point.value) {
                Ok(value) => value,
                Err(e) => {
                    log::error!(
                        "Failed to parse PolyBolt {} snapshot value: {e}",
                        channel.as_str()
                    );
                    continue;
                }
            };

            let ts_event = match unix_nanos_from_millis("payload.data.timestamp", point.timestamp) {
                Ok(ts_event) => ts_event,
                Err(e) => {
                    log::error!(
                        "Failed to parse PolyBolt {} snapshot timestamp: {e}",
                        channel.as_str()
                    );
                    continue;
                }
            };

            if !self.should_emit_timestamp_ms(
                channel,
                &symbol_lower,
                point.timestamp,
                TimestampGuard::Snapshot,
            ) {
                continue;
            }
            let ts_init = self.inner.clock.get_time_ns();

            match channel {
                PolyBoltChannel::EquityPrice => {
                    let custom_payload = Arc::new(PolymarketEquityPrice::new(
                        symbol_lower.clone(),
                        payload.source.clone(),
                        value,
                        point.timestamp,
                        envelope.ts,
                        None,
                        false,
                        ts_event,
                        ts_init,
                    ));
                    self.emit_custom_payload(&custom_payload, data_types.clone());
                }
                PolyBoltChannel::CryptoPrice | PolyBoltChannel::CryptoTwap => {
                    let custom_payload = Arc::new(PolymarketCryptoPrice::new(
                        symbol_lower.clone(),
                        payload.source.clone(),
                        value,
                        point.timestamp,
                        envelope.ts,
                        ts_event,
                        ts_init,
                    ));
                    self.emit_custom_payload(&custom_payload, data_types.clone());
                }
            }
        }
    }

    fn handle_twap_update(&self, envelope: &PolyBoltEnvelope) -> anyhow::Result<()> {
        let channel = PolyBoltChannel::CryptoTwap;
        if !self.has_channel_subscription(channel) {
            return Ok(());
        }

        let payload: TwapPayloadRaw = serde_json::from_str(envelope.payload.get())
            .map_err(|e| anyhow::anyhow!("invalid PolyBolt crypto TWAP payload: {e}"))?;
        check_twap_window(payload.window_seconds)?;
        let symbol_lower = payload.symbol.to_ascii_lowercase();
        let value = twap_exact_value(payload.full_accuracy_value.as_str())?;
        let ts_event = unix_nanos_from_millis("payload.timestamp", payload.timestamp)?;
        unix_nanos_from_millis("envelope.ts", envelope.ts)?;
        let Some(data_types) =
            self.admit_twap_observation(channel, &symbol_lower, payload.timestamp, value)?
        else {
            return Ok(());
        };

        let ts_init = self.inner.clock.get_time_ns();
        let custom_payload = Arc::new(PolymarketCryptoTwap::new(
            symbol_lower,
            payload.source,
            payload.window_seconds,
            value,
            payload.timestamp,
            envelope.ts,
            ts_event,
            ts_init,
        ));

        self.emit_custom_payload(&custom_payload, data_types);
        Ok(())
    }

    fn handle_twap_snapshot(&self, envelope: &PolyBoltEnvelope) -> anyhow::Result<()> {
        let channel = PolyBoltChannel::CryptoTwap;
        if !self.has_channel_subscription(channel) {
            return Ok(());
        }

        let payload: SnapshotPayloadRaw = serde_json::from_str(envelope.payload.get())
            .map_err(|e| anyhow::anyhow!("invalid PolyBolt crypto TWAP snapshot payload: {e}"))?;
        let window_seconds = payload
            .window_seconds
            .context("invalid PolyBolt crypto TWAP snapshot payload: missing window_seconds")?;
        check_twap_window(window_seconds)?;
        unix_nanos_from_millis("envelope.ts", envelope.ts)?;
        let symbol_lower = payload.symbol.to_ascii_lowercase();

        for point in payload.data {
            let exact = point.full_accuracy_value.as_deref().context(
                "invalid PolyBolt crypto TWAP snapshot point: missing full_accuracy_value",
            )?;
            let value = twap_exact_value(exact)?;
            let ts_event = unix_nanos_from_millis("payload.data.timestamp", point.timestamp)?;
            let Some(data_types) =
                self.admit_twap_observation(channel, &symbol_lower, point.timestamp, value)?
            else {
                continue;
            };

            let ts_init = self.inner.clock.get_time_ns();
            let custom_payload = Arc::new(PolymarketCryptoTwap::new(
                symbol_lower.clone(),
                payload.source.clone(),
                window_seconds,
                value,
                point.timestamp,
                envelope.ts,
                ts_event,
                ts_init,
            ));
            self.emit_custom_payload(&custom_payload, data_types);
        }
        Ok(())
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
                log::error!("Failed to emit PolyBolt custom data: {e}");
            }
        }
    }

    fn matching_data_types(&self, channel: PolyBoltChannel, symbol_lower: &str) -> Vec<DataType> {
        let key = tracked_key(channel, symbol_lower);
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

    fn has_channel_subscription(&self, channel: PolyBoltChannel) -> bool {
        self.inner
            .subscriptions
            .iter()
            .any(|entry| entry.wire.channel == channel.as_str())
    }

    fn malformed_frame_requires_visible_twap_failure(&self, text: &str) -> bool {
        let has_twap = self.has_channel_subscription(PolyBoltChannel::CryptoTwap);

        match serde_json::from_str::<serde_json::Value>(text) {
            Ok(value) => match value.get("channel").and_then(serde_json::Value::as_str) {
                Some(channel) => channel == PolyBoltChannel::CryptoTwap.as_str() && has_twap,
                None => has_twap,
            },
            Err(_) => has_twap,
        }
    }

    fn should_emit_timestamp_ms(
        &self,
        channel: PolyBoltChannel,
        symbol_lower: &str,
        timestamp_ms: u64,
        guard: TimestampGuard,
    ) -> bool {
        let key = tracked_key(channel, symbol_lower);
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
        channel: PolyBoltChannel,
        symbol_lower: &str,
        timestamp_ms: u64,
        value: Decimal,
    ) -> anyhow::Result<Option<Vec<DataType>>> {
        let key = tracked_key(channel, symbol_lower);
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
                        "conflicting PolyBolt TWAP observation channel={} symbol={} ",
                        "timestamp_ms={} prior={} received={}",
                    ),
                    channel.as_str(),
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
        let symbol_lower = normalize_symbol(type_name, symbol_value)?;

        let (channel, window_seconds) = match type_name {
            POLYMARKET_CRYPTO_PRICE_TYPE_NAME => (PolyBoltChannel::CryptoPrice, None),
            POLYMARKET_EQUITY_PRICE_TYPE_NAME => (PolyBoltChannel::EquityPrice, None),
            POLYMARKET_CRYPTO_TWAP_TYPE_NAME => {
                let window_seconds = metadata
                    .get("window_seconds")
                    .and_then(serde_json::Value::as_u64)
                    .context(format!(
                        "{type_name} subscriptions require integer metadata['window_seconds']"
                    ))?;

                if window_seconds != u64::from(POLYBOLT_TWAP_WINDOW_SECONDS) {
                    anyhow::bail!(
                        "{type_name} metadata['window_seconds'] must be \
                         {POLYBOLT_TWAP_WINDOW_SECONDS}, received {window_seconds}; PolyBolt \
                         serves only the 60-second TWAP"
                    );
                }
                (
                    PolyBoltChannel::CryptoTwap,
                    Some(POLYBOLT_TWAP_WINDOW_SECONDS),
                )
            }
            other => anyhow::bail!("Unsupported PolyBolt custom data type: {other}"),
        };

        Ok(Self {
            key: tracked_key(channel, &symbol_lower),
            wire: PolyBoltSubscription {
                channel: channel.as_str(),
                filter: PolyBoltFilter {
                    symbol: symbol_lower,
                    window_seconds,
                },
            },
        })
    }
}

/// Lowercases a venue symbol and rejects the legacy RTDS spellings.
///
/// `btcusdt` and `btc/usd` are not PolyBolt symbols; passing them through would subscribe to
/// nothing (or, for `usdt`, change the quote currency), so they fail loudly instead.
fn normalize_symbol(type_name: &str, symbol: &str) -> anyhow::Result<String> {
    let symbol_lower = symbol.to_ascii_lowercase();

    if symbol_lower.contains('/') || symbol_lower.ends_with("usdt") {
        anyhow::bail!(
            "{type_name} metadata['symbol']='{symbol}' uses the legacy RTDS format; PolyBolt \
             expects lowercase USD-quoted symbols such as `btcusd` or `aapl`"
        );
    }

    Ok(symbol_lower)
}

fn tracked_key(channel: PolyBoltChannel, symbol_lower: &str) -> String {
    format!("{}:{symbol_lower}", channel.as_str())
}

/// Prefers the exact `full_accuracy_value` string and falls back to the display number.
fn exact_value(
    full_accuracy_value: Option<&str>,
    display_value: Decimal,
) -> anyhow::Result<Decimal> {
    match full_accuracy_value {
        Some(exact) => parse_decimal_exact(exact)
            .with_context(|| format!("invalid full_accuracy_value: {exact}")),
        None => Ok(display_value),
    }
}

/// TWAP values are published only from the exact field; the display number is never a fallback.
fn twap_exact_value(full_accuracy_value: &str) -> anyhow::Result<Decimal> {
    parse_decimal_exact(full_accuracy_value)
        .with_context(|| format!("invalid TWAP full_accuracy_value: {full_accuracy_value}"))
}

fn check_twap_window(window_seconds: u32) -> anyhow::Result<()> {
    if window_seconds != POLYBOLT_TWAP_WINDOW_SECONDS {
        anyhow::bail!(
            "PolyBolt crypto TWAP requires window_seconds={POLYBOLT_TWAP_WINDOW_SECONDS}, \
             received {window_seconds}"
        );
    }
    Ok(())
}

fn unix_nanos_from_millis(field: &str, value: u64) -> anyhow::Result<UnixNanos> {
    let millis = i64::try_from(value)
        .with_context(|| format!("millisecond timestamp out of range for {field}: {value}"))?;
    UnixNanos::from_millis_checked(millis)
        .with_context(|| format!("millisecond timestamp overflows UnixNanos for {field}: {value}"))
}

/// Syntactically valid fixture credentials; never sent to a real venue.
#[cfg(test)]
const FIXTURE_API_SECRET_B64: &str = "Zml4dHVyZS1zZWNyZXQ=";

#[cfg(test)]
pub(crate) fn fixture_credential() -> Credential {
    Credential::new(
        SecretString::from("fixture-key"),
        SecretString::from(FIXTURE_API_SECRET_B64),
        SecretString::from("fixture-passphrase"),
    )
    .expect("fixture credential")
}

/// Shared PolyBolt mock-venue helpers for in-crate tests.
#[cfg(test)]
pub(crate) mod testing {
    use serde_json::{Value, json};

    /// Records an inbound client frame with the auth payload redacted.
    pub(crate) fn recorded_payload(payload: &Value) -> Value {
        if payload["op"] == "auth" {
            json!({"op": "auth"})
        } else {
            payload.clone()
        }
    }

    /// Venue acknowledgements for a client frame: `authed`, or one ack per subscription.
    pub(crate) fn mock_replies(payload: &Value) -> Vec<Value> {
        match payload["op"].as_str() {
            Some("auth") => vec![json!({"op": "authed", "rid": payload["rid"]})],
            Some(op @ ("subscribe" | "unsubscribe")) => payload["subscriptions"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|subscription| {
                    json!({
                        "op": format!("{op}d"),
                        "channel": subscription["channel"],
                        "rid": payload["rid"],
                    })
                })
                .collect(),
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::SocketAddr,
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
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
    use serde_json::{Value, json};

    use super::{testing::*, *};
    use crate::common::consts::{POLYMARKET_CLIENT_ID, POLYMARKET_VENUE};

    // Live captures from the production venue; see `test_data/polybolt_fixtures.source.md`.
    const CRYPTO_UPDATE_FIXTURE: &str =
        include_str!("../test_data/polybolt_crypto_price_update.json");
    const CRYPTO_SNAPSHOT_FIXTURE: &str =
        include_str!("../test_data/polybolt_crypto_price_snapshot.json");
    const TWAP_UPDATE_FIXTURE: &str = include_str!("../test_data/polybolt_crypto_twap_update.json");
    const TWAP_SNAPSHOT_FIXTURE: &str =
        include_str!("../test_data/polybolt_crypto_twap_snapshot.json");
    const EQUITY_UPDATE_FIXTURE: &str =
        include_str!("../test_data/polybolt_equity_price_update.json");
    const EQUITY_SNAPSHOT_FIXTURE: &str =
        include_str!("../test_data/polybolt_equity_price_snapshot.json");
    const AUTHED_FIXTURE: &str = include_str!("../test_data/polybolt_authed.json");
    const SUBSCRIBED_FIXTURE: &str = include_str!("../test_data/polybolt_subscribed.json");
    const ERROR_AUTH_REQUIRED_FIXTURE: &str =
        include_str!("../test_data/polybolt_error_auth_required.json");
    const ERROR_AUTH_INVALID_FIXTURE: &str =
        include_str!("../test_data/polybolt_error_auth_invalid.json");

    const TWAP_OBSERVATION_TS: u64 = 1791355667000;

    fn crypto_data_type(symbol: &str) -> DataType {
        let mut metadata = Params::new();
        metadata.insert("symbol".to_string(), json!(symbol));
        DataType::new(POLYMARKET_CRYPTO_PRICE_TYPE_NAME, Some(metadata), None)
    }

    fn equity_data_type(symbol: &str) -> DataType {
        let mut metadata = Params::new();
        metadata.insert("symbol".to_string(), json!(symbol));
        DataType::new(POLYMARKET_EQUITY_PRICE_TYPE_NAME, Some(metadata), None)
    }

    fn crypto_twap_data_type(symbol: &str, window_seconds: u64) -> DataType {
        let mut metadata = Params::new();
        metadata.insert("symbol".to_string(), json!(symbol));
        metadata.insert("window_seconds".to_string(), json!(window_seconds));
        DataType::new(POLYMARKET_CRYPTO_TWAP_TYPE_NAME, Some(metadata), None)
    }

    fn crypto_twap_data_type_without_window(symbol: &str) -> DataType {
        let mut metadata = Params::new();
        metadata.insert("symbol".to_string(), json!(symbol));
        DataType::new(POLYMARKET_CRYPTO_TWAP_TYPE_NAME, Some(metadata), None)
    }

    fn make_feed() -> (
        PolymarketPolyBoltFeed,
        tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let feed = PolymarketPolyBoltFeed::new(
            "ws://localhost/polybolt".to_string(),
            TransportBackend::default(),
            get_atomic_clock_realtime(),
            tx.into(),
        );
        (feed, rx)
    }

    fn fixture_json(fixture: &str) -> Value {
        serde_json::from_str(fixture).expect("parse fixture")
    }

    fn twap_update() -> Value {
        fixture_json(TWAP_UPDATE_FIXTURE)
    }

    fn next_custom(rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>) -> CustomData {
        let DataEvent::Data(NautilusData::Custom(custom)) =
            rx.try_recv().expect("custom data event")
        else {
            panic!("expected custom data event");
        };
        custom
    }

    fn crypto_payload(custom: &CustomData) -> &PolymarketCryptoPrice {
        custom
            .data
            .as_any()
            .downcast_ref::<PolymarketCryptoPrice>()
            .expect("PolymarketCryptoPrice")
    }

    fn twap_payload(custom: &CustomData) -> &PolymarketCryptoTwap {
        custom
            .data
            .as_any()
            .downcast_ref::<PolymarketCryptoTwap>()
            .expect("PolymarketCryptoTwap")
    }

    fn equity_payload(custom: &CustomData) -> &PolymarketEquityPrice {
        custom
            .data
            .as_any()
            .downcast_ref::<PolymarketEquityPrice>()
            .expect("PolymarketEquityPrice")
    }

    fn subscription_pairs(payload: &Value) -> Vec<(String, String)> {
        let mut pairs = payload["subscriptions"]
            .as_array()
            .expect("subscriptions array")
            .iter()
            .map(|subscription| {
                (
                    subscription["channel"].as_str().unwrap().to_string(),
                    subscription["filter"]["symbol"]
                        .as_str()
                        .unwrap()
                        .to_string(),
                )
            })
            .collect::<Vec<_>>();
        pairs.sort();
        pairs
    }

    fn ops(payloads: &[Value]) -> Vec<&str> {
        payloads
            .iter()
            .map(|payload| payload["op"].as_str().unwrap_or("?"))
            .collect()
    }

    // ---- wire format -------------------------------------------------------------------------

    #[rstest]
    fn test_subscribe_request_matches_venue_wire_format() {
        let subscriptions = vec![
            ParsedSubscription::from_data_type(&crypto_data_type("BTCUSD"))
                .unwrap()
                .wire,
            ParsedSubscription::from_data_type(&crypto_twap_data_type("btcusd", 60))
                .unwrap()
                .wire,
            ParsedSubscription::from_data_type(&equity_data_type("AAPL"))
                .unwrap()
                .wire,
        ];
        let request = PolyBoltSubscribeRequest {
            op: "subscribe",
            subscriptions: &subscriptions,
        };

        assert_eq!(
            serde_json::to_value(request).unwrap(),
            json!({
                "op": "subscribe",
                "subscriptions": [
                    {"channel": "price.crypto", "filter": {"symbol": "btcusd"}},
                    {"channel": "price.crypto.twap", "filter": {"symbol": "btcusd", "window_seconds": 60}},
                    {"channel": "price.equity", "filter": {"symbol": "aapl"}},
                ],
            }),
        );
    }

    #[rstest]
    fn test_auth_request_matches_venue_wire_format() {
        let credential = fixture_credential();
        let request = PolyBoltAuthRequest {
            op: "auth",
            auth: PolymarketWsAuth {
                api_key: SecretString::from(credential.api_key_str()),
                secret: credential.api_secret(),
                passphrase: SecretString::from(credential.passphrase()),
            },
        };

        assert_eq!(
            serde_json::to_value(request).unwrap(),
            json!({
                "op": "auth",
                "auth": {
                    "apiKey": "fixture-key",
                    "secret": FIXTURE_API_SECRET_B64,
                    "passphrase": "fixture-passphrase",
                },
            }),
        );
    }

    #[rstest]
    fn test_envelope_captured_fields() {
        let envelope: PolyBoltEnvelope =
            serde_json::from_str(CRYPTO_UPDATE_FIXTURE).expect("captured envelope");

        assert_eq!(envelope.v, 1);
        assert_eq!(envelope.channel, "price.crypto");
        assert_eq!(envelope.seq, 2);
        assert_eq!(envelope.ts, 1791355667000);
        assert!(!envelope.snapshot);
        assert_eq!(envelope.dropped, None);
        assert_eq!(
            serde_json::from_str::<Value>(envelope.payload.get()).unwrap(),
            json!({
                "full_accuracy_value": "84290.833784311415570432",
                "source": "chainlink",
                "symbol": "btcusd",
                "timestamp": 1791355667000_u64,
                "value": 84290.83378431141,
            })
        );
    }

    #[rstest]
    fn test_snapshot_envelope_captured_fields() {
        let envelope: PolyBoltEnvelope =
            serde_json::from_str(TWAP_SNAPSHOT_FIXTURE).expect("captured snapshot envelope");

        assert!(envelope.snapshot);
        assert_eq!(envelope.seq, 1);
        let payload: Value = serde_json::from_str(envelope.payload.get()).unwrap();
        assert_eq!(payload["symbol"], "btcusd");
        assert_eq!(payload["window_seconds"], 60);
        assert_eq!(payload["source"], "chainlink");
        assert_eq!(payload["data"].as_array().map(Vec::len), Some(3));
    }

    #[rstest]
    #[case::authed(AUTHED_FIXTURE)]
    #[case::subscribed(SUBSCRIBED_FIXTURE)]
    #[case::auth_required(ERROR_AUTH_REQUIRED_FIXTURE)]
    #[case::auth_invalid(ERROR_AUTH_INVALID_FIXTURE)]
    fn test_control_fixtures_parse_as_control_frames(#[case] fixture: &str) {
        assert!(serde_json::from_str::<PolyBoltEnvelope>(fixture).is_err());
        let control: PolyBoltControl = serde_json::from_str(fixture).expect("control frame");
        assert!(!matches!(control, PolyBoltControl::Unknown));
    }

    #[rstest]
    fn feed_retains_proxy_url_without_debug_exposure() {
        const PROXY_URL: &str = "http://polybolt-user:polybolt-proxy-secret@127.0.0.1:18089";
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let feed = PolymarketPolyBoltFeed::new_with_proxy(
            "ws://polybolt.example/ws".to_string(),
            TransportBackend::Tungstenite,
            get_atomic_clock_realtime(),
            tx.into(),
            Some(ProxyUrl::parse(PROXY_URL).unwrap()),
        );
        let config = feed.websocket_config();
        let debug = format!("{feed:?}");

        assert_eq!(feed.proxy_url().unwrap().expose(), PROXY_URL);
        assert_eq!(config.url, "ws://polybolt.example/ws");
        assert_eq!(config.heartbeat_interval_secs, None);
        assert_eq!(config.heartbeat_payload, None);
        assert_eq!(
            config.connect_timeout_ms,
            Some(POLYBOLT_RECONNECT_TIMEOUT_MS)
        );
        assert_eq!(
            config.reconnect_delay_initial_ms,
            Some(POLYBOLT_RECONNECT_DELAY_INITIAL_MS)
        );
        assert_eq!(
            config.reconnect_delay_max_ms,
            Some(POLYBOLT_RECONNECT_DELAY_MAX_MS)
        );
        assert_eq!(config.reconnect_backoff_factor, Some(2.0));
        assert_eq!(
            config.reconnect_jitter_ms,
            Some(POLYBOLT_RECONNECT_JITTER_MS)
        );
        assert_eq!(config.reconnect_max_attempts, None);
        assert_eq!(
            config.heartbeat_timeout_secs,
            Some(POLYBOLT_HEARTBEAT_TIMEOUT_SECS)
        );
        assert_eq!(config.backend, TransportBackend::Tungstenite);
        assert_eq!(config.proxy_url.as_deref(), Some(PROXY_URL));
        assert!(!debug.contains("polybolt-proxy-secret"));
        assert!(!debug.contains("fixture-passphrase"));
        assert!(!debug.contains(FIXTURE_API_SECRET_B64));
    }

    // ---- subscription tracking ---------------------------------------------------------------

    #[rstest]
    fn test_track_subscribe_reuses_symbol_wire_subscription() {
        let (feed, _rx) = make_feed();
        let first_changed = feed
            .track_subscribe(crypto_data_type("BTCUSD"))
            .expect("track first");
        let second_changed = feed
            .track_subscribe(crypto_data_type("btcusd"))
            .expect("track second");

        assert_eq!(feed.tracked_subscription_count(), 1);
        assert_eq!(
            feed.tracked_data_type_count("price.crypto:btcusd"),
            2,
            "distinct DataType topics should share one wire subscription",
        );
        assert!(first_changed);
        assert!(!second_changed);
    }

    #[rstest]
    fn test_track_subscribe_returns_changed_for_new_symbol() {
        let (feed, _rx) = make_feed();
        feed.track_subscribe(crypto_data_type("btcusd"))
            .expect("track first symbol");

        let changed = feed
            .track_subscribe(crypto_data_type("ethusd"))
            .expect("track second symbol");

        assert!(changed);
        assert_eq!(feed.tracked_subscription_count(), 2);
        let wire = feed.snapshot_wire_subscriptions();
        assert!(wire.contains_key("price.crypto:btcusd"));
        assert!(wire.contains_key("price.crypto:ethusd"));
    }

    #[rstest]
    fn test_track_subscribe_maps_twap_to_sixty_second_channel() {
        let (feed, _rx) = make_feed();
        let sixty = crypto_twap_data_type("BTCUSD", 60);

        assert!(is_supported_polybolt_data_type(&sixty));
        assert!(feed.track_subscribe(sixty).expect("track 60-second TWAP"));

        assert_eq!(feed.tracked_data_type_count("price.crypto.twap:btcusd"), 1);
        let wire = feed.snapshot_wire_subscriptions();
        assert_eq!(
            serde_json::to_value(&wire["price.crypto.twap:btcusd"]).unwrap(),
            json!({"channel": "price.crypto.twap", "filter": {"symbol": "btcusd", "window_seconds": 60}}),
        );
    }

    #[rstest]
    #[case(30)]
    #[case(45)]
    #[case(0)]
    fn test_track_subscribe_rejects_unsupported_twap_window(#[case] window_seconds: u64) {
        let (feed, _rx) = make_feed();

        let error = feed
            .track_subscribe(crypto_twap_data_type("btcusd", window_seconds))
            .expect_err("unsupported TWAP window must fail");

        assert!(error.to_string().contains("must be 60"), "{error}");
        assert_eq!(feed.tracked_subscription_count(), 0);
    }

    #[rstest]
    fn test_track_subscribe_rejects_missing_twap_window() {
        let (feed, _rx) = make_feed();

        let error = feed
            .track_subscribe(crypto_twap_data_type_without_window("btcusd"))
            .expect_err("missing TWAP window must fail");

        assert!(error.to_string().contains("window_seconds"));
        assert_eq!(feed.tracked_subscription_count(), 0);
    }

    #[rstest]
    #[case::binance_usdt("BTCUSDT")]
    #[case::slash("BTC/USD")]
    #[case::lower_slash("btc/usd")]
    fn test_track_subscribe_rejects_legacy_rtds_symbols(#[case] symbol: &str) {
        let (feed, _rx) = make_feed();

        let error = feed
            .track_subscribe(crypto_data_type(symbol))
            .expect_err("legacy symbol must fail");

        assert!(error.to_string().contains("btcusd"), "{error}");
        assert!(error.to_string().contains(symbol), "{error}");
        assert_eq!(feed.tracked_subscription_count(), 0);
    }

    #[rstest]
    fn test_track_subscribe_requires_credentials() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let feed = PolymarketPolyBoltFeed::new_with_proxy_and_socket_control(
            "ws://localhost/polybolt".to_string(),
            TransportBackend::default(),
            get_atomic_clock_realtime(),
            tx.into(),
            None,
            None,
            None,
        );

        let error = feed
            .track_subscribe(crypto_data_type("btcusd"))
            .expect_err("missing credentials must fail");

        assert!(error.to_string().contains("POLYMARKET_API_KEY"), "{error}");
        assert_eq!(feed.tracked_subscription_count(), 0);
    }

    #[rstest]
    fn test_track_subscribe_enforces_venue_subscription_cap() {
        let (feed, _rx) = make_feed();
        for index in 0..POLYBOLT_MAX_SUBSCRIPTIONS {
            feed.track_subscribe(equity_data_type(&format!("sym{index}")))
                .expect("subscription within cap");
        }
        assert!(
            !feed
                .track_subscribe(equity_data_type("sym0"))
                .expect("duplicate symbol stays within cap")
        );

        let error = feed
            .track_subscribe(equity_data_type("one-too-many"))
            .expect_err("65th subscription must fail");

        assert!(error.to_string().contains("64"), "{error}");
        assert_eq!(
            feed.tracked_subscription_count(),
            POLYBOLT_MAX_SUBSCRIPTIONS
        );
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

    // ---- control frames ----------------------------------------------------------------------

    #[rstest]
    fn test_authed_control_marks_session_authenticated() {
        let (feed, _rx) = make_feed();
        let _receiver = feed.inner.auth.begin();

        feed.handle_text_message(AUTHED_FIXTURE)
            .expect("authed frame");

        assert!(feed.is_authenticated());
    }

    #[rstest]
    fn test_auth_invalid_control_latches_rejection() {
        let (feed, _rx) = make_feed();
        let _receiver = feed.inner.auth.begin();

        feed.handle_text_message(ERROR_AUTH_INVALID_FIXTURE)
            .expect("error frame is handled, not raised");

        assert!(!feed.is_authenticated());
        assert!(feed.inner.auth_rejected.load(Ordering::Acquire));
    }

    #[rstest]
    #[case::subscribed(SUBSCRIBED_FIXTURE)]
    #[case::auth_required(ERROR_AUTH_REQUIRED_FIXTURE)]
    #[case::pong(r#"{"op":"pong","rid":"p1"}"#)]
    #[case::unknown_op(r#"{"op":"challenge","nonce":"abc"}"#)]
    fn test_non_auth_control_frames_are_ignored(#[case] frame: &str) {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("btcusd", 60))
            .expect("track TWAP");

        feed.handle_text_message(frame)
            .expect("control frames never fail the loop");

        assert!(rx.try_recv().is_err());
        assert!(!feed.inner.auth_rejected.load(Ordering::Acquire));
    }

    // ---- crypto TWAP -------------------------------------------------------------------------

    #[rstest]
    fn test_handle_twap_update_emits_exact_provider_value_and_three_clocks() {
        let (feed, mut rx) = make_feed();
        let data_type = crypto_twap_data_type("BTCUSD", 60);
        feed.track_subscribe(data_type.clone())
            .expect("track 60-second TWAP");

        feed.handle_text_message(TWAP_UPDATE_FIXTURE)
            .expect("valid TWAP update");

        let custom = next_custom(&mut rx);
        let payload = twap_payload(&custom);
        assert_eq!(custom.data_type, data_type);
        assert_eq!(payload.symbol, "btcusd");
        assert_eq!(payload.source, "chainlink");
        assert_eq!(payload.window_seconds, 60);
        assert_eq!(payload.value, dec!(84291.577380840025358336));
        assert_eq!(payload.observation_timestamp_ms, TWAP_OBSERVATION_TS);
        assert_eq!(payload.message_timestamp_ms, 1791355667000);
        assert_eq!(
            payload.ts_event,
            UnixNanos::from_millis(TWAP_OBSERVATION_TS)
        );
        assert!(payload.ts_init > UnixNanos::default());
    }

    #[rstest]
    fn test_handle_twap_snapshot_emits_points_in_order_and_suppresses_replay() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("btcusd", 60))
            .expect("track 60-second TWAP");

        feed.handle_text_message(TWAP_SNAPSHOT_FIXTURE)
            .expect("valid TWAP snapshot");

        let expected = [
            (1791355549000_u64, dec!(84308.348647635372998656)),
            (1791355550000_u64, dec!(84308.296819943225163776)),
            (1791355666000_u64, dec!(84291.78433204316536832)),
        ];

        for (timestamp, value) in expected {
            let custom = next_custom(&mut rx);
            let payload = twap_payload(&custom);
            assert_eq!(payload.observation_timestamp_ms, timestamp);
            assert_eq!(payload.value, value);
            assert_eq!(payload.message_timestamp_ms, 1791355666000);
            assert_eq!(payload.window_seconds, 60);
        }
        assert!(rx.try_recv().is_err());

        feed.handle_text_message(TWAP_SNAPSHOT_FIXTURE)
            .expect("replayed snapshot after resubscribe");
        assert!(rx.try_recv().is_err(), "snapshot replay must not re-emit");

        feed.handle_text_message(TWAP_UPDATE_FIXTURE)
            .expect("live update after snapshot");
        assert_eq!(
            twap_payload(&next_custom(&mut rx)).observation_timestamp_ms,
            TWAP_OBSERVATION_TS
        );
    }

    #[rstest]
    fn test_handle_twap_snapshot_requires_exact_value_per_point() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("btcusd", 60))
            .expect("track 60-second TWAP");
        let mut snapshot = fixture_json(TWAP_SNAPSHOT_FIXTURE);
        snapshot["payload"]["data"][1]
            .as_object_mut()
            .unwrap()
            .remove("full_accuracy_value");

        let error = feed
            .handle_text_message(&snapshot.to_string())
            .expect_err("snapshot point without exact value must be visible");

        assert!(error.to_string().contains("full_accuracy_value"), "{error}");
        assert_eq!(
            twap_payload(&next_custom(&mut rx)).observation_timestamp_ms,
            1791355549000,
            "points before the malformed one are still emitted",
        );
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_twap_update_uses_exact_field_not_display_value() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("btcusd", 60))
            .expect("track 60-second TWAP");
        let mut update = twap_update();
        update["payload"]["value"] = json!(1);

        feed.handle_text_message(&update.to_string())
            .expect("display value must not be authoritative");

        assert_eq!(
            twap_payload(&next_custom(&mut rx)).value,
            dec!(84291.577380840025358336)
        );
    }

    #[rstest]
    #[case("84291.58")]
    #[case("8.429158e4")]
    fn test_handle_twap_update_accepts_decimal_string_display_value(#[case] display_value: &str) {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("btcusd", 60))
            .expect("track 60-second TWAP");
        let mut update = twap_update();
        update["payload"]["value"] = json!(display_value);

        feed.handle_text_message(&update.to_string())
            .expect("decimal-string display value should satisfy the wire contract");

        assert_eq!(
            twap_payload(&next_custom(&mut rx)).value,
            dec!(84291.577380840025358336)
        );
    }

    #[rstest]
    #[case::missing(None)]
    #[case::null(Some(json!(null)))]
    #[case::boolean(Some(json!(true)))]
    #[case::object(Some(json!({"future": "format"})))]
    #[case::array(Some(json!([1, 2])))]
    #[case::nonnumeric(Some(json!("not-a-number")))]
    #[case::empty_string(Some(json!("")))]
    fn test_handle_twap_update_rejects_invalid_display_value(#[case] display_value: Option<Value>) {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("btcusd", 60))
            .expect("track 60-second TWAP");
        let mut update = twap_update();
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
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_twap_update_preserves_adjacent_exact_values() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("btcusd", 60))
            .expect("track 60-second TWAP");
        let first = twap_update();
        let mut second = first.clone();
        second["payload"]["full_accuracy_value"] = json!("84291.577380840025358337");
        second["payload"]["timestamp"] = json!(TWAP_OBSERVATION_TS + 1);

        feed.handle_text_message(&first.to_string())
            .expect("first exact TWAP update");
        feed.handle_text_message(&second.to_string())
            .expect("adjacent exact TWAP update");

        let values = [
            twap_payload(&next_custom(&mut rx)).value,
            twap_payload(&next_custom(&mut rx)).value,
        ];
        assert_eq!(values[0], dec!(84291.577380840025358336));
        assert_eq!(values[1], dec!(84291.577380840025358337));
        assert_ne!(values[0], values[1]);
    }

    #[rstest]
    fn test_handle_twap_update_preserves_negative_exact_value() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("btcusd", 60))
            .expect("track 60-second TWAP");
        let mut update = twap_update();
        update["payload"]["full_accuracy_value"] = json!("-1.234567890000000000");

        feed.handle_text_message(&update.to_string())
            .expect("valid negative exact update");

        assert_eq!(
            twap_payload(&next_custom(&mut rx)).value,
            dec!(-1.234567890000000000)
        );
    }

    #[rstest]
    fn test_twap_replay_fingerprints_are_isolated_by_symbol() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("ALPHAUSD", 60))
            .expect("track first 60-second TWAP");
        feed.track_subscribe(crypto_twap_data_type("BETAUSD", 60))
            .expect("track second 60-second TWAP");
        let mut first = twap_update();
        first["payload"]["symbol"] = json!("alphausd");
        first["payload"]["full_accuracy_value"] = json!("1.000000000000000000");
        let mut second = first.clone();
        second["payload"]["symbol"] = json!("betausd");
        second["payload"]["timestamp"] = json!(TWAP_OBSERVATION_TS - 1);
        second["payload"]["full_accuracy_value"] = json!("2.000000000000000000");

        feed.handle_text_message(&first.to_string())
            .expect("first TWAP update");
        feed.handle_text_message(&second.to_string())
            .expect("older second-symbol update must use an independent replay guard");

        let mut observations = Vec::new();

        for _ in 0..2 {
            let custom = next_custom(&mut rx);
            let payload = twap_payload(&custom);
            observations.push((payload.symbol.clone(), payload.observation_timestamp_ms));
        }
        assert_eq!(
            observations,
            vec![
                ("alphausd".to_string(), TWAP_OBSERVATION_TS),
                ("betausd".to_string(), TWAP_OBSERVATION_TS - 1),
            ]
        );
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_twap_update_drops_equal_timestamp_redelivery() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("btcusd", 60))
            .expect("track 60-second TWAP");

        feed.handle_text_message(TWAP_UPDATE_FIXTURE)
            .expect("first exact TWAP update");
        feed.handle_text_message(TWAP_UPDATE_FIXTURE)
            .expect("equal-timestamp redelivery should be ignored");

        next_custom(&mut rx);
        assert!(
            rx.try_recv().is_err(),
            "an equal-timestamp TWAP redelivery must not emit twice"
        );
    }

    #[rstest]
    fn test_twap_conflict_does_not_advance_replay_guard() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("btcusd", 60))
            .expect("track 60-second TWAP");
        let original = twap_update();
        let mut older = original.clone();
        older["payload"]["timestamp"] = json!(TWAP_OBSERVATION_TS - 1);
        older["payload"]["full_accuracy_value"] = json!("84291.577380840025358335");
        let mut conflict = original.clone();
        conflict["payload"]["full_accuracy_value"] = json!("84291.577380840025358337");

        feed.handle_text_message(&original.to_string())
            .expect("first exact TWAP update");
        next_custom(&mut rx);
        feed.handle_text_message(&older.to_string())
            .expect("older TWAP update should be ignored");

        let error = feed
            .handle_text_message(&conflict.to_string())
            .expect_err("equal-timestamp value conflict must be visible");
        assert_eq!(
            error.to_string(),
            concat!(
                "conflicting PolyBolt TWAP observation channel=price.crypto.twap ",
                "symbol=btcusd timestamp_ms=1791355667000 ",
                "prior=84291.577380840025358336 received=84291.577380840025358337",
            ),
        );
        assert!(rx.try_recv().is_err());

        feed.handle_text_message(&original.to_string())
            .expect("the retained exact observation must remain a replay after conflict");
        assert!(rx.try_recv().is_err());

        let mut newer = conflict;
        newer["payload"]["timestamp"] = json!(TWAP_OBSERVATION_TS + 1);
        feed.handle_text_message(&newer.to_string())
            .expect("newer exact TWAP update");
        assert_eq!(
            twap_payload(&next_custom(&mut rx)).value,
            dec!(84291.577380840025358337)
        );
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_twap_inflight_tail_cannot_poison_resubscribe_replay() {
        let (feed, mut rx) = make_feed();
        let data_type = crypto_twap_data_type("btcusd", 60);
        let envelope: PolyBoltEnvelope =
            serde_json::from_str(TWAP_UPDATE_FIXTURE).expect("parse TWAP envelope");
        let payload: TwapPayloadRaw =
            serde_json::from_str(envelope.payload.get()).expect("parse TWAP payload");
        let exact_value =
            twap_exact_value(&payload.full_accuracy_value).expect("parse exact TWAP value");
        feed.track_subscribe(data_type.clone())
            .expect("track 60-second TWAP");

        let captured_data_types = feed.matching_data_types(PolyBoltChannel::CryptoTwap, "btcusd");
        assert_eq!(captured_data_types, vec![data_type.clone()]);

        assert!(
            feed.track_unsubscribe(&data_type)
                .expect("unsubscribe final TWAP reference")
        );
        let _tail_admission = feed
            .admit_twap_observation(
                PolyBoltChannel::CryptoTwap,
                "btcusd",
                payload.timestamp,
                exact_value,
            )
            .expect("complete captured handler admission tail");

        assert!(
            feed.track_subscribe(data_type.clone())
                .expect("resubscribe 60-second TWAP")
        );
        feed.handle_text_message(TWAP_UPDATE_FIXTURE)
            .expect("first replay after resubscribe");

        let custom = next_custom(&mut rx);
        let emitted = twap_payload(&custom);
        assert_eq!(custom.data_type, data_type);
        assert_eq!(emitted.value, exact_value);
        assert_eq!(emitted.observation_timestamp_ms, payload.timestamp);
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_twap_update_rejects_window_mismatch_without_advancing_guard() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("btcusd", 60))
            .expect("track 60-second TWAP");
        let mut mismatch = twap_update();
        mismatch["payload"]["window_seconds"] = json!(30);

        let error = feed
            .handle_text_message(&mismatch.to_string())
            .expect_err("window mismatch must be visible");
        assert!(
            error.to_string().contains("requires window_seconds=60"),
            "{error}"
        );
        assert!(rx.try_recv().is_err());

        feed.handle_text_message(TWAP_UPDATE_FIXTURE)
            .expect("same-timestamp valid update must still emit");
        assert!(rx.try_recv().is_ok());
    }

    #[rstest]
    #[case::exact_value("full_accuracy_value")]
    #[case::window("window_seconds")]
    fn test_handle_twap_update_rejects_missing_required_field(#[case] field: &str) {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("btcusd", 60))
            .expect("track 60-second TWAP");
        let mut update = twap_update();
        update["payload"]
            .as_object_mut()
            .expect("payload object")
            .remove(field);

        let error = feed
            .handle_text_message(&update.to_string())
            .expect_err("missing required TWAP field must fail visibly");
        assert!(error.to_string().contains(field), "{error}");
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    #[case("79228162514264337593543950336")]
    #[case("")]
    #[case("not-a-number")]
    #[case("1e400")]
    fn test_handle_twap_update_rejects_invalid_exact_value(#[case] value: &str) {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("btcusd", 60))
            .expect("track 60-second TWAP");
        let mut update = twap_update();
        update["payload"]["full_accuracy_value"] = json!(value);

        let error = feed
            .handle_text_message(&update.to_string())
            .expect_err("invalid exact value must fail");
        assert!(error.to_string().contains("full_accuracy_value"), "{error}");
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_twap_update_validates_untracked_symbol_on_active_channel() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("btcusd", 60))
            .expect("track 60-second TWAP");
        let mut update = twap_update();
        update["payload"]["symbol"] = json!("ethusd");
        update["payload"]["full_accuracy_value"] = json!("not-a-number");

        let error = feed
            .handle_text_message(&update.to_string())
            .expect_err("malformed frame on active channel must fail visibly");
        assert!(error.to_string().contains("full_accuracy_value"), "{error}");
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_twap_update_rejects_timestamp_overflow_without_advancing_guard() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("btcusd", 60))
            .expect("track 60-second TWAP");
        let mut update = twap_update();
        update["payload"]["timestamp"] = json!(u64::MAX);

        let error = feed
            .handle_text_message(&update.to_string())
            .expect_err("overflowing observation timestamp must fail visibly");
        assert!(error.to_string().contains("payload.timestamp"), "{error}");
        feed.handle_text_message(TWAP_UPDATE_FIXTURE)
            .expect("a later valid update must not be suppressed");

        next_custom(&mut rx);
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_twap_update_rejects_envelope_timestamp_overflow_without_advancing_guard() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("btcusd", 60))
            .expect("track 60-second TWAP");
        let valid = twap_update();
        let mut invalid = valid.clone();
        invalid["ts"] = json!(u64::MAX);

        let error = feed
            .handle_text_message(&invalid.to_string())
            .expect_err("overflowing envelope timestamp must fail visibly");
        assert!(error.to_string().contains("envelope.ts"), "{error}");
        assert!(rx.try_recv().is_err());

        feed.handle_text_message(&valid.to_string())
            .expect("a later valid update must not be suppressed");
        next_custom(&mut rx);
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_text_rejects_malformed_envelope_while_twap_channel_is_active() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("btcusd", 60))
            .expect("track 60-second TWAP");
        let frame = json!({
            "v": 1,
            "channel": "price.crypto.twap",
            "seq": 3,
            "ts": 1791355667000_u64
        });

        let error = feed
            .handle_text_message(&frame.to_string())
            .expect_err("malformed envelope on active TWAP channel must be visible");

        assert!(
            error.to_string().contains("invalid PolyBolt JSON frame"),
            "{error}"
        );
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_text_rejects_unclassifiable_json_while_twap_channel_is_active() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("btcusd", 60))
            .expect("track 60-second TWAP");
        let frame = json!({"ts": 1791355667000_u64, "payload": {}});

        let error = feed
            .handle_text_message(&frame.to_string())
            .expect_err("unclassifiable JSON must be visible while TWAP is active");

        assert!(
            error.to_string().contains("invalid PolyBolt JSON frame"),
            "{error}"
        );
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_text_ignores_malformed_unrelated_channel_while_twap_channel_is_active() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_twap_data_type("btcusd", 60))
            .expect("track 60-second TWAP");
        let frame = json!({"v": 1, "channel": "price.polymarket", "ts": 1791355667000_u64});

        feed.handle_text_message(&frame.to_string())
            .expect("malformed unrelated channel remains ignorable");

        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_text_ignores_unrelated_unsupported_channel() {
        let (feed, mut rx) = make_feed();
        let frame = json!({
            "v": 1,
            "channel": "price.polymarket",
            "seq": 1,
            "ts": 1791355667000_u64,
            "payload": {}
        });

        feed.handle_text_message(&frame.to_string())
            .expect("unrelated unsupported channel remains ignorable");

        assert!(rx.try_recv().is_err());
    }

    // ---- crypto spot -------------------------------------------------------------------------

    #[rstest]
    fn test_handle_crypto_update_emits_exact_custom_data() {
        let (feed, mut rx) = make_feed();
        let data_type = crypto_data_type("btcusd");
        feed.track_subscribe(data_type.clone())
            .expect("track subscribe");

        feed.handle_text_message(CRYPTO_UPDATE_FIXTURE)
            .expect("valid crypto update");

        let custom = next_custom(&mut rx);
        let payload = crypto_payload(&custom);
        assert_eq!(custom.data_type, data_type);
        assert_eq!(payload.symbol, "btcusd");
        assert_eq!(payload.source, "chainlink");
        assert_eq!(payload.value, dec!(84290.833784311415570432));
        assert_eq!(payload.price_timestamp_ms, 1791355667000);
        assert_eq!(payload.message_timestamp_ms, 1791355667000);
        assert_eq!(payload.ts_event, UnixNanos::from_millis(1791355667000));
    }

    #[rstest]
    fn test_handle_crypto_update_falls_back_to_display_value_when_exact_absent() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_data_type("btcusd"))
            .expect("track subscribe");
        let mut update = fixture_json(CRYPTO_UPDATE_FIXTURE);
        update["payload"]
            .as_object_mut()
            .unwrap()
            .remove("full_accuracy_value");

        feed.handle_text_message(&update.to_string())
            .expect("valid crypto update");

        assert_eq!(
            crypto_payload(&next_custom(&mut rx)).value,
            dec!(84290.83378431141)
        );
    }

    #[rstest]
    fn test_handle_crypto_update_drops_malformed_exact_value_without_emitting() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_data_type("btcusd"))
            .expect("track subscribe");
        let mut update = fixture_json(CRYPTO_UPDATE_FIXTURE);
        update["payload"]["full_accuracy_value"] = json!("not-a-number");

        feed.handle_text_message(&update.to_string())
            .expect("spot channels log and continue");

        assert!(rx.try_recv().is_err());
        feed.handle_text_message(CRYPTO_UPDATE_FIXTURE)
            .expect("next valid update");
        assert!(rx.try_recv().is_ok());
    }

    #[rstest]
    fn test_handle_crypto_update_routes_by_symbol() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_data_type("btcusd"))
            .expect("track BTC");
        let eth_data_type = crypto_data_type("ethusd");
        feed.track_subscribe(eth_data_type.clone())
            .expect("track ETH");
        let mut update = fixture_json(CRYPTO_UPDATE_FIXTURE);
        update["payload"]["symbol"] = json!("ethusd");
        update["payload"]["full_accuracy_value"] = json!("2450.110000000000000000");
        update["payload"]["timestamp"] = json!(1791355670000_u64);
        update["ts"] = json!(1791355670004_u64);

        feed.handle_text_message(&update.to_string())
            .expect("valid crypto update");

        let custom = next_custom(&mut rx);
        let payload = crypto_payload(&custom);
        assert_eq!(custom.data_type, eth_data_type);
        assert_eq!(payload.symbol, "ethusd");
        assert_eq!(payload.value, dec!(2450.110000000000000000));
        assert_eq!(payload.price_timestamp_ms, 1791355670000);
        assert_eq!(payload.message_timestamp_ms, 1791355670004);
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_crypto_update_emits_distinct_same_millisecond_points() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_data_type("btcusd"))
            .expect("track subscribe");

        // Two distinct live updates sharing one millisecond timestamp must both emit;
        // only replayed snapshots collapse equal timestamps.
        feed.handle_text_message(CRYPTO_UPDATE_FIXTURE)
            .expect("valid first crypto update");
        let mut second = fixture_json(CRYPTO_UPDATE_FIXTURE);
        second["payload"]["full_accuracy_value"] = json!("84291.000000000000000000");
        feed.handle_text_message(&second.to_string())
            .expect("valid second crypto update");

        assert_eq!(
            crypto_payload(&next_custom(&mut rx)).value,
            dec!(84290.833784311415570432)
        );
        assert_eq!(
            crypto_payload(&next_custom(&mut rx)).value,
            dec!(84291.000000000000000000)
        );
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_crypto_update_with_overflowing_timestamp_does_not_mute_symbol() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_data_type("btcusd"))
            .expect("track subscribe");
        let mut bad = fixture_json(CRYPTO_UPDATE_FIXTURE);
        bad["payload"]["timestamp"] = json!(u64::MAX);

        // The bad frame is dropped without panicking and without advancing the
        // high-water mark, so the next valid frame still emits.
        feed.handle_text_message(&bad.to_string())
            .expect("overflowing timestamp is handled, not fatal");
        assert!(rx.try_recv().is_err());

        feed.handle_text_message(CRYPTO_UPDATE_FIXTURE)
            .expect("valid crypto update");

        assert_eq!(crypto_payload(&next_custom(&mut rx)).symbol, "btcusd");
    }

    #[rstest]
    fn test_handle_crypto_update_with_dropped_counter_still_emits() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(crypto_data_type("btcusd"))
            .expect("track subscribe");
        let mut update = fixture_json(CRYPTO_UPDATE_FIXTURE);
        update["dropped"] = json!(3);

        feed.handle_text_message(&update.to_string())
            .expect("conflated frame is still data");

        assert_eq!(crypto_payload(&next_custom(&mut rx)).symbol, "btcusd");
    }

    #[rstest]
    fn test_handle_crypto_snapshot_emits_points_then_skips_replay() {
        let (feed, mut rx) = make_feed();
        let data_type = crypto_data_type("BTCUSD");
        feed.track_subscribe(data_type.clone())
            .expect("track subscribe");

        feed.handle_text_message(CRYPTO_SNAPSHOT_FIXTURE)
            .expect("valid crypto snapshot");

        let expected = [
            (1791355549000_u64, dec!(84307.27064946722471936)),
            (1791355550000_u64, dec!(84307.023637929491693568)),
            (1791355666000_u64, dec!(84290.764561459730972672)),
        ];

        for (timestamp, value) in expected {
            let custom = next_custom(&mut rx);
            let payload = crypto_payload(&custom);
            assert_eq!(custom.data_type, data_type);
            assert_eq!(payload.symbol, "btcusd");
            assert_eq!(payload.source, "chainlink");
            assert_eq!(payload.value, value);
            assert_eq!(payload.price_timestamp_ms, timestamp);
            assert_eq!(payload.message_timestamp_ms, 1791355666000);
        }
        assert!(rx.try_recv().is_err());

        feed.handle_text_message(CRYPTO_SNAPSHOT_FIXTURE)
            .expect("valid replayed crypto snapshot");
        assert!(rx.try_recv().is_err());
    }

    // ---- equity ------------------------------------------------------------------------------

    #[rstest]
    fn test_handle_equity_update_emits_custom_data() {
        let (feed, mut rx) = make_feed();
        let data_type = equity_data_type("AAPL");
        feed.track_subscribe(data_type.clone())
            .expect("track subscribe");

        feed.handle_text_message(EQUITY_UPDATE_FIXTURE)
            .expect("valid equity update");

        let custom = next_custom(&mut rx);
        let payload = equity_payload(&custom);
        assert_eq!(custom.data_type, data_type);
        assert_eq!(payload.symbol, "aapl");
        assert_eq!(payload.source, "pyth");
        assert_eq!(payload.value, dec!(333.81));
        assert_eq!(payload.price_timestamp_ms, 1791355668000);
        assert_eq!(payload.message_timestamp_ms, 1791355668000);
        assert_eq!(payload.received_at_ms, Some(1791355668000));
        assert!(!payload.is_carried_forward);
    }

    #[rstest]
    fn test_handle_equity_update_reads_carry_forward_flag_and_exact_value() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(equity_data_type("aapl"))
            .expect("track subscribe");
        let mut update = fixture_json(EQUITY_UPDATE_FIXTURE);
        update["payload"]["full_accuracy_value"] = json!("333.8123");
        update["payload"]["is_carried_forward"] = json!(true);

        feed.handle_text_message(&update.to_string())
            .expect("valid equity update");

        let custom = next_custom(&mut rx);
        let payload = equity_payload(&custom);
        assert_eq!(payload.value, dec!(333.8123));
        assert!(payload.is_carried_forward);
    }

    #[rstest]
    fn test_handle_equity_update_falls_back_when_exact_value_is_absent() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(equity_data_type("AAPL"))
            .expect("track subscribe");
        let mut update = fixture_json(EQUITY_UPDATE_FIXTURE);
        update["payload"]
            .as_object_mut()
            .expect("payload object")
            .remove("full_accuracy_value");
        update["payload"]["value"] = json!(333.8);

        feed.handle_text_message(&update.to_string())
            .expect("valid equity update");

        assert_eq!(equity_payload(&next_custom(&mut rx)).value, dec!(333.8));
    }

    #[rstest]
    fn test_handle_equity_update_routes_by_symbol() {
        let (feed, mut rx) = make_feed();
        feed.track_subscribe(equity_data_type("AAPL"))
            .expect("track AAPL");
        let msft_data_type = equity_data_type("MSFT");
        feed.track_subscribe(msft_data_type.clone())
            .expect("track MSFT");
        let mut update = fixture_json(EQUITY_UPDATE_FIXTURE);
        update["payload"]["symbol"] = json!("msft");
        update["payload"]["full_accuracy_value"] = json!("432.1537");
        update["payload"]["timestamp"] = json!(1791355669000_u64);
        update["payload"]["received_at"] = json!(1791355669007_u64);
        update["ts"] = json!(1791355669020_u64);

        feed.handle_text_message(&update.to_string())
            .expect("valid equity update");

        let custom = next_custom(&mut rx);
        let payload = equity_payload(&custom);
        assert_eq!(custom.data_type, msft_data_type);
        assert_eq!(payload.symbol, "msft");
        assert_eq!(payload.value, dec!(432.1537));
        assert_eq!(payload.price_timestamp_ms, 1791355669000);
        assert_eq!(payload.message_timestamp_ms, 1791355669020);
        assert_eq!(payload.received_at_ms, Some(1791355669007));
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_equity_snapshot_emits_points_then_skips_replay() {
        let (feed, mut rx) = make_feed();
        let data_type = equity_data_type("AAPL");
        feed.track_subscribe(data_type.clone())
            .expect("track subscribe");

        feed.handle_text_message(EQUITY_SNAPSHOT_FIXTURE)
            .expect("valid equity snapshot");

        for timestamp in [1791355548200_u64, 1791355548400, 1791355667800] {
            let custom = next_custom(&mut rx);
            let payload = equity_payload(&custom);
            assert_eq!(custom.data_type, data_type);
            assert_eq!(payload.symbol, "aapl");
            assert_eq!(payload.source, "pyth");
            assert_eq!(payload.value, dec!(333.81));
            assert_eq!(payload.price_timestamp_ms, timestamp);
            assert_eq!(payload.message_timestamp_ms, 1791355667800);
            assert_eq!(payload.received_at_ms, None);
            assert!(!payload.is_carried_forward);
        }
        assert!(rx.try_recv().is_err());

        feed.handle_text_message(EQUITY_SNAPSHOT_FIXTURE)
            .expect("valid replayed equity snapshot");
        assert!(rx.try_recv().is_err());
    }

    // ---- mock venue --------------------------------------------------------------------------

    #[derive(Clone, Default)]
    struct TestServerState {
        received_payloads: Arc<tokio::sync::Mutex<Vec<Value>>>,
        connection_count: Arc<tokio::sync::Mutex<usize>>,
        control_txs:
            Arc<tokio::sync::Mutex<Vec<tokio::sync::mpsc::UnboundedSender<TestServerCommand>>>>,
        reject_auth: Arc<AtomicBool>,
        hold_auth: Arc<AtomicBool>,
        unavailable_auth_once: Arc<AtomicBool>,
        auth_count: Arc<AtomicUsize>,
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

        async fn payloads(&self) -> Vec<Value> {
            self.received_payloads.lock().await.clone()
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

    async fn handle_polybolt_upgrade(
        ws: WebSocketUpgrade,
        State(state): State<TestServerState>,
    ) -> Response {
        ws.on_upgrade(move |socket| handle_polybolt_socket(socket, state))
    }

    async fn handle_polybolt_socket(mut socket: WebSocket, state: TestServerState) {
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
                    let Ok(payload) = serde_json::from_str::<Value>(&text) else {
                        continue;
                    };
                    state
                        .received_payloads
                        .lock()
                        .await
                        .push(recorded_payload(&payload));

                    let replies = if payload["op"] == "auth" {
                        state.auth_count.fetch_add(1, Ordering::Relaxed);
                        if state.reject_auth.load(Ordering::Relaxed) {
                            vec![
                                json!({"op": "error", "code": "auth_invalid", "rid": payload["rid"]}),
                            ]
                        } else if state.hold_auth.load(Ordering::Relaxed) {
                            Vec::new()
                        } else if state.unavailable_auth_once.swap(false, Ordering::Relaxed) {
                            vec![
                                json!({"op": "error", "code": "auth_unavailable", "rid": payload["rid"]}),
                            ]
                        } else {
                            mock_replies(&payload)
                        }
                    } else {
                        mock_replies(&payload)
                    };

                    for reply in replies {
                        if socket
                            .send(AxumWsMessage::Text(reply.to_string().into()))
                            .await
                            .is_err()
                        {
                            return;
                        }
                    }
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

    async fn start_polybolt_server(state: TestServerState) -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("failed to bind PolyBolt test server");
        let addr = listener
            .local_addr()
            .expect("missing PolyBolt test server address");
        let router = Router::new()
            .route("/polybolt", get(handle_polybolt_upgrade))
            .with_state(state);

        tokio::spawn(async move {
            axum::serve(listener, router)
                .await
                .expect("PolyBolt test server failed");
        });

        tokio::time::sleep(Duration::from_millis(50)).await;
        addr
    }

    fn make_server_feed(
        addr: SocketAddr,
    ) -> (
        PolymarketPolyBoltFeed,
        tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let feed = PolymarketPolyBoltFeed::new(
            format!("ws://{addr}/polybolt"),
            TransportBackend::default(),
            get_atomic_clock_realtime(),
            tx.into(),
        );
        (feed, rx)
    }

    async fn wait_for_op(state: &TestServerState, op: &'static str) {
        wait_until_async(
            || {
                let state = state.clone();
                async move {
                    state
                        .received_payloads
                        .lock()
                        .await
                        .iter()
                        .any(|payload| payload["op"] == op)
                }
            },
            Duration::from_secs(5),
        )
        .await;
    }

    fn build_crypto_update(
        symbol: &str,
        exact_value: &str,
        price_timestamp_ms: u64,
        message_timestamp_ms: u64,
    ) -> String {
        let mut update = fixture_json(CRYPTO_UPDATE_FIXTURE);
        update["payload"]["symbol"] = json!(symbol);
        update["payload"]["full_accuracy_value"] = json!(exact_value);
        update["payload"]["timestamp"] = json!(price_timestamp_ms);
        update["ts"] = json!(message_timestamp_ms);
        update.to_string()
    }

    fn collect_crypto_symbols(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
        expected_count: usize,
    ) -> Vec<String> {
        let mut symbols = Vec::with_capacity(expected_count);
        for _ in 0..expected_count {
            let custom = next_custom(rx);
            symbols.push(crypto_payload(&custom).symbol.clone());
        }
        symbols.sort_unstable();
        symbols
    }

    async fn connect_test_ws(url: String) -> Arc<WebSocketClient> {
        let (handler, _raw_rx) = channel_message_handler();
        Arc::new(
            WebSocketClient::builder()
                .config(WebSocketConfig {
                    url,
                    headers: vec![],
                    heartbeat_interval_secs: None,
                    heartbeat_payload: None,
                    connect_timeout_ms: Some(POLYBOLT_RECONNECT_TIMEOUT_MS),
                    reconnect_delay_initial_ms: Some(POLYBOLT_RECONNECT_DELAY_INITIAL_MS),
                    reconnect_delay_max_ms: Some(POLYBOLT_RECONNECT_DELAY_MAX_MS),
                    reconnect_backoff_factor: Some(2.0),
                    reconnect_jitter_ms: Some(POLYBOLT_RECONNECT_JITTER_MS),
                    reconnect_max_attempts: None,
                    heartbeat_timeout_secs: Some(POLYBOLT_HEARTBEAT_TIMEOUT_SECS),
                    idle_timeout_ms: None,
                    writer_capacity: None,
                    backend: TransportBackend::default(),
                    proxy_url: None,
                    max_message_size_bytes: None,
                    max_frame_size_bytes: None,
                })
                .message_handler(handler)
                .connect()
                .await
                .expect("connect test ws"),
        )
    }

    // ---- authentication ----------------------------------------------------------------------

    #[rstest]
    #[tokio::test]
    async fn test_connect_authenticates_before_subscribing() {
        let state = TestServerState::default();
        let addr = start_polybolt_server(state.clone()).await;
        let (feed, _rx) = make_server_feed(addr);
        feed.track_subscribe(crypto_data_type("btcusd"))
            .expect("track BTC");
        feed.track_subscribe(crypto_twap_data_type("btcusd", 60))
            .expect("track TWAP");

        feed.connect().await.expect("connect feed");
        wait_for_op(&state, "subscribe").await;

        let payloads = state.payloads().await;
        assert_eq!(ops(&payloads), vec!["auth", "subscribe"]);
        assert_eq!(
            subscription_pairs(&payloads[1]),
            vec![
                ("price.crypto".to_string(), "btcusd".to_string()),
                ("price.crypto.twap".to_string(), "btcusd".to_string()),
            ]
        );
        assert!(feed.is_authenticated());
        feed.disconnect().await.expect("disconnect feed");
    }

    #[rstest]
    #[tokio::test]
    async fn test_subscribe_waits_for_authed_acknowledgement() {
        let state = TestServerState::default();
        state.hold_auth.store(true, Ordering::Relaxed);
        let addr = start_polybolt_server(state.clone()).await;
        let (feed, _rx) = make_server_feed(addr);
        feed.track_subscribe(crypto_data_type("btcusd"))
            .expect("track BTC");

        let connect_task = tokio::spawn({
            let feed = feed.clone();
            async move { feed.connect().await }
        });
        wait_for_op(&state, "auth").await;
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert_eq!(ops(&state.payloads().await), vec!["auth"]);
        assert!(!feed.is_authenticated());

        state.send_text_to_all(AUTHED_FIXTURE.to_string()).await;
        connect_task
            .await
            .expect("join connect task")
            .expect("connect after late authed");
        wait_for_op(&state, "subscribe").await;

        assert_eq!(ops(&state.payloads().await), vec!["auth", "subscribe"]);
        feed.disconnect().await.expect("disconnect feed");
    }

    #[rstest]
    #[tokio::test]
    async fn test_unavailable_auth_keeps_connect_alive_and_retries_later() {
        let state = TestServerState::default();
        state.unavailable_auth_once.store(true, Ordering::Relaxed);
        let addr = start_polybolt_server(state.clone()).await;
        let (feed, _rx) = make_server_feed(addr);
        feed.track_subscribe(crypto_data_type("btcusd"))
            .expect("track BTC");

        feed.connect()
            .await
            .expect("a venue-side auth outage must not fail connect");

        assert!(!feed.is_authenticated());
        assert_eq!(ops(&state.payloads().await), vec!["auth"]);

        // The reconcile worker waits out the retry delay, re-authenticates, then subscribes.
        wait_until_async(
            || {
                let state = state.clone();
                async move { ops(&state.payloads().await) == vec!["auth", "auth", "subscribe"] }
            },
            Duration::from_secs(POLYBOLT_AUTH_RETRY_DELAY_SECS + 5),
        )
        .await;

        assert!(feed.is_authenticated());
        assert_eq!(state.auth_count.load(Ordering::Relaxed), 2);
        feed.disconnect().await.expect("disconnect feed");
    }

    #[rstest]
    #[tokio::test]
    async fn test_rejected_credentials_fail_connect_and_stop_auth_retries() {
        let state = TestServerState::default();
        state.reject_auth.store(true, Ordering::Relaxed);
        let addr = start_polybolt_server(state.clone()).await;
        let (feed, _rx) = make_server_feed(addr);
        feed.track_subscribe(crypto_data_type("btcusd"))
            .expect("track BTC");

        let error = feed
            .connect()
            .await
            .expect_err("rejected credentials must fail connect");
        assert!(error.to_string().contains("rejected"), "{error}");

        feed.request_reconcile(ReconcileReason::DesiredChanged);
        feed.request_reconcile(ReconcileReason::TransportReset);
        tokio::time::sleep(Duration::from_millis(300)).await;

        assert_eq!(state.auth_count.load(Ordering::Relaxed), 1);
        assert_eq!(ops(&state.payloads().await), vec!["auth"]);
        assert!(!feed.is_authenticated());
        feed.disconnect().await.expect("disconnect feed");
    }

    #[rstest]
    #[tokio::test]
    async fn test_server_disconnect_reauthenticates_and_resubscribes() {
        let state = TestServerState::default();
        let addr = start_polybolt_server(state.clone()).await;
        let (feed, _rx) = make_server_feed(addr);
        feed.track_subscribe(equity_data_type("aapl"))
            .expect("track AAPL");
        feed.connect().await.expect("connect feed");
        wait_for_op(&state, "subscribe").await;

        state.clear_received_payloads().await;
        state.close_all_connections().await;
        wait_until_async(
            || {
                let state = state.clone();
                async move { state.connection_count().await >= 2 }
            },
            Duration::from_secs(10),
        )
        .await;
        wait_for_op(&state, "subscribe").await;

        let payloads = state.payloads().await;
        assert_eq!(ops(&payloads), vec!["auth", "subscribe"]);
        assert_eq!(
            subscription_pairs(&payloads[1]),
            vec![("price.equity".to_string(), "aapl".to_string())]
        );
        assert_eq!(state.auth_count.load(Ordering::Relaxed), 2);
        feed.disconnect().await.expect("disconnect feed");
    }

    // ---- socket state and reconnect ----------------------------------------------------------

    #[rstest]
    #[tokio::test]
    async fn test_socket_state_events_use_polybolt_endpoint() {
        let state = TestServerState::default();
        let addr = start_polybolt_server(state).await;
        let (data_tx, _data_rx) = tokio::sync::mpsc::unbounded_channel();
        let (system_tx, mut system_rx) = tokio::sync::mpsc::unbounded_channel();
        replace_system_event_sender(system_tx);
        let registry = SocketReconnectRegistry::default();
        let socket_factory = nautilus_live::SocketControlFactory::with_registry(
            *POLYMARKET_CLIENT_ID,
            Some(*POLYMARKET_VENUE),
            &registry,
        );
        let feed = PolymarketPolyBoltFeed::new_with_proxy_and_socket_control(
            format!("ws://{addr}/polybolt"),
            TransportBackend::default(),
            get_atomic_clock_realtime(),
            data_tx.into(),
            None,
            Some(socket_factory.control(crate::websocket::POLYBOLT_STREAMS_ENDPOINT)),
            Some(fixture_credential()),
        );
        assert!(
            feed.track_subscribe(crypto_data_type("btcusd"))
                .expect("track PolyBolt subscription")
        );

        feed.connect().await.expect("connect PolyBolt feed");

        let event = tokio::time::timeout(Duration::from_secs(5), system_rx.recv())
            .await
            .expect("wait for socket state change")
            .expect("system event channel closed");
        let SystemEvent::SocketState(change) = event;
        let endpoint = ustr::Ustr::from("polymarket-polybolt-streams");
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
        let addr = start_polybolt_server(state.clone()).await;
        let (data_tx, mut data_rx) = tokio::sync::mpsc::unbounded_channel();
        let states = Arc::new(parking_lot::Mutex::new(Vec::new()));
        let states_callback = Arc::clone(&states);
        let state_sink = SocketStateSink::new(move |state| {
            states_callback.lock().push(state);
        });
        let feed = PolymarketPolyBoltFeed::new_with_proxy_and_state_sink(
            format!("ws://{addr}/polybolt"),
            TransportBackend::default(),
            get_atomic_clock_realtime(),
            data_tx.into(),
            None,
            Some(state_sink),
        );
        assert!(
            feed.track_subscribe(crypto_twap_data_type("btcusd", 60))
                .expect("track TWAP subscription")
        );

        feed.connect().await.expect("connect PolyBolt feed");
        wait_for_op(&state, "subscribe").await;
        state
            .send_text_to_all(TWAP_UPDATE_FIXTURE.to_string())
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
        wait_for_op(&state, "subscribe").await;

        let mut conflict = twap_update();
        conflict["payload"]["full_accuracy_value"] = json!("84291.577380840025358337");
        let error = feed
            .handle_text_message(&conflict.to_string())
            .expect_err("equal-timestamp value conflict must be visible after reconnect");
        assert!(error.to_string().contains("conflict"));

        feed.handle_text_message(TWAP_UPDATE_FIXTURE)
            .expect("the retained exact observation must remain a replay after conflict");
        assert!(
            tokio::time::timeout(Duration::from_millis(250), data_rx.recv())
                .await
                .is_err(),
            "reconnect must not re-emit the last equal-timestamp TWAP observation"
        );

        let mut next = twap_update();
        next["payload"]["timestamp"] = json!(TWAP_OBSERVATION_TS + 1);
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

    #[rstest]
    #[tokio::test]
    async fn test_reconnect_quiesces_old_twap_tail_before_new_loop_delivery() {
        let state = TestServerState::default();
        let addr = start_polybolt_server(state.clone()).await;
        let (feed, mut data_rx) = make_server_feed(addr);
        feed.track_subscribe(crypto_twap_data_type("btcusd", 60))
            .expect("track 60-second TWAP");

        let envelope: PolyBoltEnvelope =
            serde_json::from_str(TWAP_UPDATE_FIXTURE).expect("parse TWAP envelope");
        let message_timestamp_ms = envelope.ts;
        let payload: TwapPayloadRaw =
            serde_json::from_str(envelope.payload.get()).expect("parse TWAP payload");
        let symbol_lower = payload.symbol.to_ascii_lowercase();
        let observation_timestamp_ms = payload.timestamp;
        let exact_value =
            twap_exact_value(&payload.full_accuracy_value).expect("parse exact TWAP value");
        let ts_event = unix_nanos_from_millis("payload.timestamp", observation_timestamp_ms)
            .expect("parse TWAP event timestamp");

        let (admitted_tx, admitted_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let old_feed = feed.clone();
        let old_symbol = symbol_lower.clone();

        let old_handle = get_runtime().spawn(async move {
            let data_types = old_feed
                .admit_twap_observation(
                    PolyBoltChannel::CryptoTwap,
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

            let custom_payload = Arc::new(PolymarketCryptoTwap::new(
                old_symbol,
                "chainlink".to_string(),
                POLYBOLT_TWAP_WINDOW_SECONDS,
                exact_value,
                observation_timestamp_ms,
                message_timestamp_ms,
                ts_event,
                old_feed.inner.clock.get_time_ns(),
            ));
            old_feed.emit_custom_payload(&custom_payload, data_types);
        });
        feed.task_slots()
            .expect("PolyBolt task owner")
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

        let mut newer = twap_update();
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
            .expect("join PolyBolt connect task")
            .expect("connect PolyBolt feed");

        let observation_timestamp = |event| {
            let DataEvent::Data(NautilusData::Custom(custom)) = event else {
                panic!("expected custom data event");
            };
            twap_payload(&custom).observation_timestamp_ms
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

    // ---- incremental sync --------------------------------------------------------------------

    #[rstest]
    #[tokio::test]
    async fn test_incremental_sync_subscribes_only_new_symbol_while_connected() {
        let state = TestServerState::default();
        let addr = start_polybolt_server(state.clone()).await;
        let (feed, _rx) = make_server_feed(addr);
        feed.track_subscribe(crypto_data_type("btcusd"))
            .expect("track BTC");
        feed.connect().await.expect("connect feed");
        wait_for_op(&state, "subscribe").await;
        state.clear_received_payloads().await;

        assert!(
            feed.track_subscribe(crypto_data_type("ethusd"))
                .expect("track ETH")
        );
        feed.reconcile_once(false).await.expect("reconcile live");
        wait_for_op(&state, "subscribe").await;

        let payloads = state.payloads().await;
        assert_eq!(
            ops(&payloads),
            vec!["subscribe"],
            "no re-auth, no unsubscribe"
        );
        assert_eq!(
            subscription_pairs(&payloads[0]),
            vec![("price.crypto".to_string(), "ethusd".to_string())]
        );
        feed.disconnect().await.expect("disconnect feed");
    }

    #[rstest]
    #[tokio::test]
    async fn test_incremental_sync_unsubscribes_only_removed_symbol() {
        let state = TestServerState::default();
        let addr = start_polybolt_server(state.clone()).await;
        let (feed, _rx) = make_server_feed(addr);
        let aapl = equity_data_type("aapl");
        let msft = equity_data_type("msft");
        assert!(feed.track_subscribe(aapl).expect("track AAPL"));
        assert!(feed.track_subscribe(msft.clone()).expect("track MSFT"));
        feed.connect().await.expect("connect feed");
        wait_for_op(&state, "subscribe").await;
        state.clear_received_payloads().await;

        assert!(feed.track_unsubscribe(&msft).expect("track unsubscribe"));
        feed.reconcile_once(false).await.expect("reconcile live");
        wait_for_op(&state, "unsubscribe").await;

        let payloads = state.payloads().await;
        assert_eq!(ops(&payloads), vec!["unsubscribe"]);
        assert_eq!(
            subscription_pairs(&payloads[0]),
            vec![("price.equity".to_string(), "msft".to_string())]
        );
        feed.disconnect().await.expect("disconnect feed");
    }

    #[rstest]
    #[tokio::test]
    async fn test_reconcile_worker_coalesces_multiple_desired_changes() {
        let state = TestServerState::default();
        let addr = start_polybolt_server(state.clone()).await;
        let (feed, _rx) = make_server_feed(addr);
        feed.track_subscribe(crypto_data_type("btcusd"))
            .expect("track BTC");
        feed.connect().await.expect("connect feed");
        wait_for_op(&state, "subscribe").await;
        state.clear_received_payloads().await;

        let wire_guard = feed.inner.wire_mutex.lock().await;
        assert!(
            feed.track_subscribe(crypto_data_type("ethusd"))
                .expect("track ETH")
        );
        feed.request_reconcile(ReconcileReason::DesiredChanged);
        assert!(
            feed.track_subscribe(crypto_data_type("solusd"))
                .expect("track SOL")
        );
        feed.request_reconcile(ReconcileReason::DesiredChanged);
        drop(wire_guard);

        wait_for_op(&state, "subscribe").await;
        tokio::time::sleep(Duration::from_millis(200)).await;

        let payloads = state.payloads().await;
        assert_eq!(
            ops(&payloads),
            vec!["subscribe"],
            "coalesced desired-state changes should produce one subscribe frame",
        );
        assert_eq!(
            subscription_pairs(&payloads[0]),
            vec![
                ("price.crypto".to_string(), "ethusd".to_string()),
                ("price.crypto".to_string(), "solusd".to_string()),
            ]
        );
        feed.disconnect().await.expect("disconnect feed");
    }

    #[rstest]
    #[tokio::test]
    async fn test_reconnected_control_replays_retained_subscriptions() {
        let state = TestServerState::default();
        let addr = start_polybolt_server(state.clone()).await;
        let (feed, _rx) = make_server_feed(addr);
        feed.track_subscribe(crypto_data_type("btcusd"))
            .expect("track BTC");
        feed.track_subscribe(crypto_data_type("ethusd"))
            .expect("track ETH");
        feed.track_subscribe(equity_data_type("aapl"))
            .expect("track AAPL");
        feed.track_subscribe(crypto_twap_data_type("btcusd", 60))
            .expect("track 60-second TWAP");

        let ws = connect_test_ws(format!("ws://{addr}/polybolt")).await;
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

        wait_for_op(&state, "subscribe").await;
        loop_handle.await.expect("join PolyBolt loop");

        let payloads = state.payloads().await;
        let replay = payloads
            .iter()
            .find(|payload| payload["op"] == "subscribe")
            .expect("replay payload");
        assert_eq!(
            subscription_pairs(replay),
            vec![
                ("price.crypto".to_string(), "btcusd".to_string()),
                ("price.crypto".to_string(), "ethusd".to_string()),
                ("price.crypto.twap".to_string(), "btcusd".to_string()),
                ("price.equity".to_string(), "aapl".to_string()),
            ]
        );
        assert!(
            ops(&payloads).contains(&"auth"),
            "replay after reconnect must re-authenticate first"
        );
        feed.disconnect().await.expect("disconnect feed");
    }

    #[rstest]
    #[tokio::test]
    async fn test_channel_close_self_heals_with_retained_subscriptions() {
        let state = TestServerState::default();
        let addr = start_polybolt_server(state.clone()).await;
        let (feed, _rx) = make_server_feed(addr);
        feed.track_subscribe(crypto_data_type("btcusd"))
            .expect("track BTC");

        let ws = connect_test_ws(format!("ws://{addr}/polybolt")).await;
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

        wait_for_op(&state, "subscribe").await;
        loop_handle.await.expect("join PolyBolt loop");

        assert!(
            feed.current_ws()
                .is_some_and(|current| !Arc::ptr_eq(&current, &ws)),
            "channel close should replace the dead PolyBolt client when retained subscriptions exist",
        );

        let payloads = state.payloads().await;
        assert_eq!(ops(&payloads), vec!["auth", "subscribe"]);
        feed.disconnect().await.expect("disconnect feed");
    }

    #[rstest]
    #[tokio::test]
    async fn test_server_disconnect_reconnects_and_resumes_multi_symbol_updates() {
        let state = TestServerState::default();
        let addr = start_polybolt_server(state.clone()).await;
        let (feed, mut rx) = make_server_feed(addr);
        feed.track_subscribe(crypto_data_type("btcusd"))
            .expect("track BTC");
        feed.track_subscribe(crypto_data_type("ethusd"))
            .expect("track ETH");
        feed.connect().await.expect("connect feed");
        wait_for_op(&state, "subscribe").await;

        state
            .send_text_to_all(build_crypto_update(
                "btcusd",
                "61035.860000000000000000",
                1791355670000,
                1791355670142,
            ))
            .await;
        state
            .send_text_to_all(build_crypto_update(
                "ethusd",
                "2450.110000000000000000",
                1791355671000,
                1791355671142,
            ))
            .await;

        wait_until_async(|| async { rx.len() >= 2 }, Duration::from_secs(2)).await;
        assert_eq!(
            collect_crypto_symbols(&mut rx, 2),
            vec!["btcusd".to_string(), "ethusd".to_string()],
            "both retained symbols should emit before disconnect",
        );

        state.clear_received_payloads().await;
        state.close_all_connections().await;

        wait_until_async(
            || {
                let state = state.clone();
                async move { state.connection_count().await >= 2 }
            },
            Duration::from_secs(10),
        )
        .await;
        wait_for_op(&state, "subscribe").await;

        state
            .send_text_to_all(build_crypto_update(
                "btcusd",
                "61040.120000000000000000",
                1791355672000,
                1791355672142,
            ))
            .await;
        state
            .send_text_to_all(build_crypto_update(
                "ethusd",
                "2455.550000000000000000",
                1791355673000,
                1791355673142,
            ))
            .await;

        wait_until_async(|| async { rx.len() >= 2 }, Duration::from_secs(2)).await;
        assert_eq!(
            collect_crypto_symbols(&mut rx, 2),
            vec!["btcusd".to_string(), "ethusd".to_string()],
            "both retained symbols should resume after server-side reconnect",
        );

        feed.disconnect().await.expect("disconnect feed");
    }

    #[rstest]
    #[tokio::test]
    async fn test_server_disconnect_does_not_restore_unsubscribed_symbol() {
        let state = TestServerState::default();
        let addr = start_polybolt_server(state.clone()).await;
        let (feed, mut rx) = make_server_feed(addr);
        let btc = crypto_data_type("btcusd");
        let eth = crypto_data_type("ethusd");
        feed.track_subscribe(btc.clone()).expect("track BTC");
        feed.track_subscribe(eth.clone()).expect("track ETH");
        feed.connect().await.expect("connect feed");
        wait_for_op(&state, "subscribe").await;

        assert!(feed.track_unsubscribe(&eth).expect("unsubscribe ETH"));
        feed.reconcile_once(false)
            .await
            .expect("reconcile after unsubscribe");
        wait_for_op(&state, "unsubscribe").await;

        state.clear_received_payloads().await;
        state.close_all_connections().await;

        wait_until_async(
            || {
                let state = state.clone();
                async move { state.connection_count().await >= 2 }
            },
            Duration::from_secs(10),
        )
        .await;
        wait_for_op(&state, "subscribe").await;

        let payloads = state.payloads().await;
        let replay = payloads
            .iter()
            .find(|payload| payload["op"] == "subscribe")
            .expect("replay payload");
        assert_eq!(
            subscription_pairs(replay),
            vec![("price.crypto".to_string(), "btcusd".to_string())]
        );

        state
            .send_text_to_all(build_crypto_update(
                "btcusd",
                "61050.010000000000000000",
                1791355674000,
                1791355674142,
            ))
            .await;
        state
            .send_text_to_all(build_crypto_update(
                "ethusd",
                "2460.010000000000000000",
                1791355675000,
                1791355675142,
            ))
            .await;

        wait_until_async(|| async { !rx.is_empty() }, Duration::from_secs(2)).await;
        assert_eq!(
            collect_crypto_symbols(&mut rx, 1),
            vec!["btcusd".to_string()],
            "reconnect should restore only still-retained symbols",
        );
        assert!(
            rx.try_recv().is_err(),
            "ETH should not be replayed after unsubscribe"
        );

        feed.disconnect().await.expect("disconnect feed");
    }

    // ---- shutdown races ----------------------------------------------------------------------

    #[rstest]
    #[tokio::test]
    async fn test_reconcile_request_does_not_reconnect_while_closing() {
        let state = TestServerState::default();
        let addr = start_polybolt_server(state.clone()).await;
        let (feed, _rx) = make_server_feed(addr);
        feed.track_subscribe(crypto_data_type("btcusd"))
            .expect("track BTC");
        feed.inner.closing.store(true, Ordering::Release);

        feed.request_reconcile(ReconcileReason::DesiredChanged);
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert!(
            feed.current_ws().is_none(),
            "closing feed should not reconnect"
        );
        assert!(
            state.payloads().await.is_empty(),
            "closing feed should not replay subscriptions",
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_connect_without_subscriptions_clears_closing_for_later_subscribe() {
        let state = TestServerState::default();
        let addr = start_polybolt_server(state.clone()).await;
        let (feed, _rx) = make_server_feed(addr);

        feed.disconnect().await.expect("disconnect feed");
        feed.connect().await.expect("connect feed");

        assert!(
            feed.current_ws().is_none(),
            "connect without retained subscriptions should not open a socket",
        );

        feed.track_subscribe(crypto_data_type("btcusd"))
            .expect("track BTC");
        feed.request_reconcile(ReconcileReason::DesiredChanged);
        wait_for_op(&state, "subscribe").await;

        assert_eq!(ops(&state.payloads().await), vec!["auth", "subscribe"]);
        feed.disconnect().await.expect("disconnect feed");
    }

    #[rstest]
    #[tokio::test]
    async fn test_ensure_connected_without_retained_subscriptions_does_not_connect() {
        let state = TestServerState::default();
        let addr = start_polybolt_server(state.clone()).await;
        let (feed, _rx) = make_server_feed(addr);

        feed.request_reconcile(ReconcileReason::EnsureConnected);
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert!(
            feed.current_ws().is_none(),
            "ensure-connected without retained subscriptions should not connect",
        );
        assert!(
            state.payloads().await.is_empty(),
            "ensure-connected without retained subscriptions should not send wire frames",
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_channel_close_does_not_self_heal_while_closing() {
        let state = TestServerState::default();
        let addr = start_polybolt_server(state.clone()).await;
        let (feed, _rx) = make_server_feed(addr);
        feed.track_subscribe(crypto_data_type("btcusd"))
            .expect("track BTC");

        let ws = connect_test_ws(format!("ws://{addr}/polybolt")).await;
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
        loop_handle.await.expect("join PolyBolt loop");
        tokio::time::sleep(Duration::from_millis(200)).await;

        assert!(
            feed.current_ws().is_none(),
            "closing feed should not replace a dead PolyBolt client",
        );
        assert!(
            state.payloads().await.is_empty(),
            "closing feed should not issue recovery frames",
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_disconnect_cancels_reconcile_worker() {
        let (feed, _rx) = make_feed();
        feed.track_subscribe(crypto_data_type("btcusd"))
            .expect("track BTC");

        feed.request_reconcile(ReconcileReason::DesiredChanged);
        wait_until_async(
            || {
                let feed = feed.clone();
                async move {
                    feed.task_slots()
                        .expect("PolyBolt task owner")
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
                .expect("PolyBolt task owner")
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
            .expect("PolyBolt task owner")
            .message
            .lock()
            .await
            .insert(tokio::spawn(async move {
                let _notify = NotifyOnDrop(Some(message_tx));
                std::future::pending::<()>().await;
            }));
        feed.task_slots()
            .expect("PolyBolt task owner")
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
        feed.track_subscribe(crypto_data_type("btcusd"))
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
                        .expect("PolyBolt task owner")
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
        let addr = start_polybolt_server(state.clone()).await;
        let (feed, _rx) = make_server_feed(addr);
        feed.track_subscribe(crypto_data_type("btcusd"))
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
            "disconnect should prevent a pending reconcile from creating a new PolyBolt client",
        );
        assert!(
            state.payloads().await.is_empty(),
            "disconnect should prevent a pending reconcile from sending wire frames",
        );
    }
}
