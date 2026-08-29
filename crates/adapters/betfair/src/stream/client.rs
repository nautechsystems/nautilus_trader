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

//! Betfair Exchange Stream API client.
//!
//! Connects to the Betfair raw TLS stream (CRLF-delimited JSON), authenticates,
//! and manages market/order subscriptions with automatic clk-based resubscription
//! on reconnection.

use std::{
    sync::{
        Arc, Mutex, MutexGuard, OnceLock, PoisonError,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use bytes::Bytes;
use nautilus_common::live::get_runtime;
use nautilus_network::{
    SocketState, SocketStateSink,
    mode::ReconnectRequestOutcome,
    socket::{
        SocketClient, SocketConfig, SocketHeartbeat, SocketReconnectHandle, SocketReconnectReplay,
        TcpMessageHandler, WriterCommand,
    },
};
use tokio::{sync::watch, task::JoinHandle}; // tokio-import-ok
use tokio_tungstenite::tungstenite::stream::Mode;

use super::{
    config::{
        BETFAIR_STREAM_HEARTBEAT_MAX_MS, BETFAIR_STREAM_HEARTBEAT_MIN_MS, BetfairStreamConfig,
    },
    error::BetfairStreamError,
    messages::{
        Authentication, CricketSubscription, MarketDataFilter, MarketSubscription, OrderFilter,
        OrderSubscription, RaceSubscription, Status, StreamMarketFilter, StreamMessage,
        stream_decode,
    },
};
use crate::common::{
    consts::{
        BETFAIR_STREAM_SERVER_HEARTBEAT_MS, STREAM_OP_MARKET_SUBSCRIPTION,
        STREAM_OP_ORDER_SUBSCRIPTION,
    },
    credential::BetfairCredential,
    enums::{ChangeType, SegmentType, StatusErrorCode},
};

pub(crate) type StreamMessageHandler = Arc<dyn Fn(StreamMessage) + Send + Sync>;

#[derive(Clone, Copy, Debug)]
pub(crate) enum HeartbeatTimeoutSource {
    Outbound,
    Server,
}

const AUTH_REQUEST_ID: u64 = 1;
const STREAM_STATUS_SUCCESS: &str = "SUCCESS";
const STREAM_DEGRADED_STATUS: i32 = 503;
const MARKET_SUBSCRIPTION_REPLAY_KEY: u64 = 1;
const ORDER_SUBSCRIPTION_REPLAY_KEY: u64 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum StreamLifecycleState {
    Disconnected,
    Idle,
    Pending,
    Active,
    Degraded,
    Rejected,
}

impl StreamLifecycleState {
    fn from_atomic(value: &AtomicU8) -> Self {
        match value.load(Ordering::Acquire) {
            1 => Self::Idle,
            2 => Self::Pending,
            3 => Self::Active,
            4 => Self::Degraded,
            5 => Self::Rejected,
            _ => Self::Disconnected,
        }
    }
}

#[derive(Debug)]
struct LifecycleState {
    value: AtomicU8,
    changed: watch::Sender<()>,
}

impl LifecycleState {
    fn new(state: StreamLifecycleState) -> Self {
        Self {
            value: AtomicU8::new(state as u8),
            changed: watch::channel(()).0,
        }
    }

    fn get(&self) -> StreamLifecycleState {
        StreamLifecycleState::from_atomic(&self.value)
    }

    fn set(&self, state: StreamLifecycleState) {
        self.value.store(state as u8, Ordering::Release);
        self.changed.send_replace(());
    }
}

#[derive(Debug)]
struct ProtocolLifecycle {
    transport_connected: AtomicBool,
    authenticated: LifecycleState,
    market: LifecycleState,
    market_was_current: AtomicBool,
    market_requires_image: AtomicBool,
    market_image_tainted: AtomicBool,
    order: LifecycleState,
    order_was_current: AtomicBool,
    order_requires_image: AtomicBool,
    order_image_tainted: AtomicBool,
}

impl Default for ProtocolLifecycle {
    fn default() -> Self {
        Self {
            transport_connected: AtomicBool::new(false),
            authenticated: LifecycleState::new(StreamLifecycleState::Disconnected),
            market: LifecycleState::new(StreamLifecycleState::Idle),
            market_was_current: AtomicBool::new(false),
            market_requires_image: AtomicBool::new(false),
            market_image_tainted: AtomicBool::new(false),
            order: LifecycleState::new(StreamLifecycleState::Idle),
            order_was_current: AtomicBool::new(false),
            order_requires_image: AtomicBool::new(false),
            order_image_tainted: AtomicBool::new(false),
        }
    }
}

impl ProtocolLifecycle {
    fn on_transport(&self, state: SocketState, market_id: u64, order_id: u64) {
        let connected = state == SocketState::Connected;
        self.transport_connected.store(connected, Ordering::Release);
        self.authenticated.set(if connected {
            StreamLifecycleState::Pending
        } else {
            StreamLifecycleState::Disconnected
        });
        self.market
            .set(subscription_transport_state(connected, market_id));
        self.market_was_current.store(false, Ordering::Release);
        self.market_image_tainted.store(false, Ordering::Release);
        self.order
            .set(subscription_transport_state(connected, order_id));
        self.order_was_current.store(false, Ordering::Release);
        self.order_image_tainted.store(false, Ordering::Release);
    }

    fn on_status(&self, status: &Status, market_id: u64, order_id: u64) {
        let Some(id) = status.id else {
            return;
        };
        let next = if status.status_code.as_deref() == Some(STREAM_STATUS_SUCCESS)
            && status.error_code.is_none()
        {
            StreamLifecycleState::Active
        } else {
            StreamLifecycleState::Rejected
        };

        if id == AUTH_REQUEST_ID {
            self.authenticated.set(next);
        } else if id == market_id {
            self.market.set(if next == StreamLifecycleState::Active {
                StreamLifecycleState::Pending
            } else {
                next
            });
        } else if id == order_id {
            self.order.set(if next == StreamLifecycleState::Active {
                StreamLifecycleState::Pending
            } else {
                next
            });
        }
    }

    fn on_change(
        state: &LifecycleState,
        was_current: &AtomicBool,
        requires_image: &AtomicBool,
        status: Option<i32>,
        change_type: Option<ChangeType>,
        segment_type: Option<SegmentType>,
    ) {
        let complete = change_complete(segment_type);
        let initial = change_type == Some(ChangeType::SubImage)
            || (change_type == Some(ChangeType::ResubDelta)
                && !requires_image.load(Ordering::Acquire));

        if status == Some(STREAM_DEGRADED_STATUS) {
            state.set(StreamLifecycleState::Degraded);
            return;
        }

        let current = state.get();

        if status.is_none()
            && complete
            && (initial
                || (current == StreamLifecycleState::Degraded
                    && was_current.load(Ordering::Acquire)))
        {
            if change_type == Some(ChangeType::SubImage) {
                requires_image.store(false, Ordering::Release);
            }
            was_current.store(true, Ordering::Release);
            state.set(StreamLifecycleState::Active);
        }
    }
}

const fn subscription_transport_state(connected: bool, id: u64) -> StreamLifecycleState {
    if !connected {
        StreamLifecycleState::Disconnected
    } else if id == 0 {
        StreamLifecycleState::Idle
    } else {
        StreamLifecycleState::Pending
    }
}

async fn wait_for_lifecycle_state(state: &LifecycleState, expected: StreamLifecycleState) {
    let mut changed_rx = state.changed.subscribe();
    loop {
        if state.get() == expected {
            return;
        }
        changed_rx
            .changed()
            .await
            .expect("lifecycle sender lives as long as the borrowed client");
    }
}

/// Betfair Exchange Stream API client using raw TLS (CRLF-delimited JSON).
///
/// On connect, authenticates immediately. On reconnection, replays authentication
/// and any active subscriptions with the latest `clk` token for delta resumption.
///
/// The auth bytes are stored in a watch channel so the caller can push refreshed
/// session tokens via [`update_auth`](Self::update_auth) after keep-alive or HTTP
/// reconnect. The `closed` flag distinguishes permanent shutdown from transient
/// reconnect.
#[derive(Debug)]
pub struct BetfairStreamClient {
    socket: SocketClient,
    market_sub_tx: watch::Sender<Option<MarketSubscription>>,
    market_clk_tx: watch::Sender<Option<String>>,
    market_initial_clk_tx: watch::Sender<Option<String>>,
    order_sub_tx: watch::Sender<Option<OrderSubscription>>,
    order_clk_tx: watch::Sender<Option<String>>,
    order_initial_clk_tx: watch::Sender<Option<String>>,
    market_active_sub_id: Arc<AtomicU64>,
    order_active_sub_id: Arc<AtomicU64>,
    request_id: Arc<AtomicU64>,
    market_state_lock: Arc<Mutex<()>>,
    order_state_lock: Arc<Mutex<()>>,
    auth_tx: watch::Sender<StreamAuth>,
    reconnect_auth: Arc<ReconnectAuthState>,
    lifecycle: Arc<ProtocolLifecycle>,
    dead_peer_enabled: Arc<AtomicBool>,
    dead_peer_timeout_ms: Arc<AtomicU64>,
    dead_peer_timeout_override: bool,
    dead_peer_task: Option<JoinHandle<()>>,
    closed: AtomicBool,
}

impl BetfairStreamClient {
    /// Connects to the Betfair stream API and authenticates.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails or authentication cannot be sent.
    pub async fn connect(
        credential: &BetfairCredential,
        session_token: String,
        handler: TcpMessageHandler,
        config: BetfairStreamConfig,
    ) -> Result<Self, BetfairStreamError> {
        Self::connect_inner(
            credential,
            session_token,
            StreamHandler::Raw(handler),
            config,
            HeartbeatTimeoutSource::Server,
            None,
        )
        .await
    }

    /// Connects to the Betfair stream API and reports transport availability changes.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails or authentication cannot be sent.
    pub(crate) async fn connect_with_state_sink(
        credential: &BetfairCredential,
        session_token: String,
        handler: StreamMessageHandler,
        config: BetfairStreamConfig,
        heartbeat_timeout_source: HeartbeatTimeoutSource,
        state_sink: Option<SocketStateSink>,
    ) -> Result<Self, BetfairStreamError> {
        Self::connect_inner(
            credential,
            session_token,
            StreamHandler::Decoded(handler),
            config,
            heartbeat_timeout_source,
            state_sink,
        )
        .await
    }

    async fn connect_inner(
        credential: &BetfairCredential,
        session_token: String,
        handler: StreamHandler,
        config: BetfairStreamConfig,
        heartbeat_timeout_source: HeartbeatTimeoutSource,
        state_sink: Option<SocketStateSink>,
    ) -> Result<Self, BetfairStreamError> {
        config
            .validate()
            .map_err(|e| BetfairStreamError::ProtocolError(e.to_string()))?;
        let auth = Authentication::with_id(
            credential.app_key().to_string(),
            session_token,
            AUTH_REQUEST_ID,
        );
        let auth_bytes_vec = serde_json::to_vec(&auth)?;
        let auth_bytes = Bytes::from(auth_bytes_vec.clone());
        let reconnect_auth = Arc::new(ReconnectAuthState::default());
        let (auth_tx, auth_rx) = watch::channel(StreamAuth {
            generation: 0,
            bytes: auth_bytes,
        });
        let mode = if config.use_tls {
            Mode::Tls
        } else {
            Mode::Plain
        };

        let (market_clk_tx, market_clk_rx) = watch::channel(None::<String>);
        let (market_initial_clk_tx, market_initial_clk_rx) = watch::channel(None::<String>);
        let (order_clk_tx, order_clk_rx) = watch::channel(None::<String>);
        let (order_initial_clk_tx, order_initial_clk_rx) = watch::channel(None::<String>);
        let (market_sub_tx, market_sub_rx) = watch::channel(None::<MarketSubscription>);
        let (order_sub_tx, order_sub_rx) = watch::channel(None::<OrderSubscription>);

        // Clone senders for the handler; struct keeps originals to reset on re-subscribe.
        let market_sub_tx_h = market_sub_tx.clone();
        let order_sub_tx_h = order_sub_tx.clone();
        let (market_clk_tx_h, market_initial_clk_tx_h) =
            (market_clk_tx.clone(), market_initial_clk_tx.clone());
        let (order_clk_tx_h, order_initial_clk_tx_h) =
            (order_clk_tx.clone(), order_initial_clk_tx.clone());

        let market_active_sub_id = Arc::new(AtomicU64::new(0));
        let order_active_sub_id = Arc::new(AtomicU64::new(0));
        let request_id = Arc::new(AtomicU64::new(AUTH_REQUEST_ID + 1));
        let request_id_h = Arc::clone(&request_id);
        let market_state_lock = Arc::new(Mutex::new(()));
        let order_state_lock = Arc::new(Mutex::new(()));
        let market_state_lock_h = Arc::clone(&market_state_lock);
        let order_state_lock_h = Arc::clone(&order_state_lock);
        let writer_tx_h = Arc::new(OnceLock::new());
        let writer_tx_handler = Arc::clone(&writer_tx_h);
        let market_active_sub_id_h = Arc::clone(&market_active_sub_id);
        let order_active_sub_id_h = Arc::clone(&order_active_sub_id);
        let reconnect_auth_h = Arc::clone(&reconnect_auth);
        let lifecycle = Arc::new(ProtocolLifecycle::default());
        let lifecycle_h = Arc::clone(&lifecycle);
        let last_inbound = Arc::new(Mutex::new(Instant::now()));
        let last_inbound_h = Arc::clone(&last_inbound);
        let dead_peer_enabled = Arc::new(AtomicBool::new(false));
        let dead_peer_timeout_ms = Arc::new(AtomicU64::new(
            config.dead_peer_timeout_secs().saturating_mul(1_000),
        ));
        let dead_peer_timeout_ms_h = Arc::clone(&dead_peer_timeout_ms);
        let timeout_override = config.heartbeat_timeout_secs.is_some();

        let message_handler: TcpMessageHandler = Arc::new(move |data: &[u8]| {
            *last_inbound_h.lock().expect("last inbound lock poisoned") = Instant::now();
            let Some(msg) = handler.decode(data) else {
                return;
            };

            match &msg {
                StreamMessage::MarketChange(mcm) => {
                    let _state = lock_stream_state(&market_state_lock_h);
                    let active = market_active_sub_id_h.load(Ordering::SeqCst);
                    let current = active == 0 || mcm.id.is_none_or(|id| id == active);
                    if !current {
                        return;
                    }

                    if mcm.status == Some(STREAM_DEGRADED_STATUS) {
                        if mcm.segment_type.is_some() {
                            lifecycle_h
                                .market_image_tainted
                                .store(true, Ordering::Release);
                        }
                        ProtocolLifecycle::on_change(
                            &lifecycle_h.market,
                            &lifecycle_h.market_was_current,
                            &lifecycle_h.market_requires_image,
                            mcm.status,
                            mcm.ct,
                            mcm.segment_type,
                        );
                        return;
                    }

                    let image_start = mcm.ct == Some(ChangeType::SubImage)
                        && matches!(mcm.segment_type, None | Some(SegmentType::SegStart));
                    let complete = change_complete(mcm.segment_type);
                    if image_start && mcm.status.is_none() {
                        lifecycle_h
                            .market_image_tainted
                            .store(false, Ordering::Release);
                    } else if lifecycle_h.market_image_tainted.load(Ordering::Acquire) {
                        if complete {
                            reissue_market_subscription(
                                &request_id_h,
                                &market_active_sub_id_h,
                                &lifecycle_h,
                                &market_sub_tx_h,
                                &market_clk_tx_h,
                                &market_initial_clk_tx_h,
                                writer_tx_handler.get(),
                            );
                        }
                        return;
                    }

                    let lifecycle_state = lifecycle_h.market.get();
                    if lifecycle_state == StreamLifecycleState::Degraded
                        && mcm.ct != Some(ChangeType::SubImage)
                    {
                        if complete {
                            reissue_market_subscription(
                                &request_id_h,
                                &market_active_sub_id_h,
                                &lifecycle_h,
                                &market_sub_tx_h,
                                &market_clk_tx_h,
                                &market_initial_clk_tx_h,
                                writer_tx_handler.get(),
                            );
                        }
                        return;
                    }

                    if lifecycle_h.market_requires_image.load(Ordering::Acquire)
                        && mcm.ct == Some(ChangeType::ResubDelta)
                    {
                        return;
                    }

                    ProtocolLifecycle::on_change(
                        &lifecycle_h.market,
                        &lifecycle_h.market_was_current,
                        &lifecycle_h.market_requires_image,
                        mcm.status,
                        mcm.ct,
                        mcm.segment_type,
                    );
                    update_stream_state(
                        &mcm.clk,
                        &mcm.initial_clk,
                        mcm.heartbeat_ms,
                        &market_clk_tx_h,
                        &market_initial_clk_tx_h,
                        timeout_override,
                        &dead_peer_timeout_ms_h,
                    );
                    handler.handle(data, msg);
                }
                StreamMessage::OrderChange(ocm) => {
                    let _state = lock_stream_state(&order_state_lock_h);
                    let active = order_active_sub_id_h.load(Ordering::SeqCst);
                    let current = active == 0 || ocm.id.is_none_or(|id| id == active);
                    if !current {
                        return;
                    }

                    if ocm.status == Some(STREAM_DEGRADED_STATUS) {
                        if ocm.segment_type.is_some() {
                            lifecycle_h
                                .order_image_tainted
                                .store(true, Ordering::Release);
                        }
                        ProtocolLifecycle::on_change(
                            &lifecycle_h.order,
                            &lifecycle_h.order_was_current,
                            &lifecycle_h.order_requires_image,
                            ocm.status,
                            ocm.ct,
                            ocm.segment_type,
                        );
                        handler.handle(data, msg);
                        return;
                    }

                    let image_start = ocm.ct == Some(ChangeType::SubImage)
                        && matches!(ocm.segment_type, None | Some(SegmentType::SegStart));
                    let complete = change_complete(ocm.segment_type);
                    if image_start && ocm.status.is_none() {
                        lifecycle_h
                            .order_image_tainted
                            .store(false, Ordering::Release);
                    } else if lifecycle_h.order_image_tainted.load(Ordering::Acquire) {
                        if complete {
                            reissue_order_subscription(
                                &request_id_h,
                                &order_active_sub_id_h,
                                &lifecycle_h,
                                &order_sub_tx_h,
                                &order_clk_tx_h,
                                &order_initial_clk_tx_h,
                                writer_tx_handler.get(),
                            );
                        }
                        return;
                    }

                    let lifecycle_state = lifecycle_h.order.get();
                    if lifecycle_state == StreamLifecycleState::Degraded
                        && lifecycle_h.order_requires_image.load(Ordering::Acquire)
                        && ocm.ct != Some(ChangeType::SubImage)
                    {
                        if complete {
                            reissue_order_subscription(
                                &request_id_h,
                                &order_active_sub_id_h,
                                &lifecycle_h,
                                &order_sub_tx_h,
                                &order_clk_tx_h,
                                &order_initial_clk_tx_h,
                                writer_tx_handler.get(),
                            );
                        }
                        return;
                    }

                    if lifecycle_h.order_requires_image.load(Ordering::Acquire)
                        && ocm.ct == Some(ChangeType::ResubDelta)
                    {
                        return;
                    }

                    ProtocolLifecycle::on_change(
                        &lifecycle_h.order,
                        &lifecycle_h.order_was_current,
                        &lifecycle_h.order_requires_image,
                        ocm.status,
                        ocm.ct,
                        ocm.segment_type,
                    );
                    update_stream_state(
                        &ocm.clk,
                        &ocm.initial_clk,
                        ocm.heartbeat_ms,
                        &order_clk_tx_h,
                        &order_initial_clk_tx_h,
                        timeout_override,
                        &dead_peer_timeout_ms_h,
                    );
                    handler.handle(data, msg);
                }
                StreamMessage::Status(status) => {
                    let _market_state = lock_stream_state(&market_state_lock_h);
                    let _order_state = lock_stream_state(&order_state_lock_h);
                    let market_id = market_active_sub_id_h.load(Ordering::Acquire);
                    let order_id = order_active_sub_id_h.load(Ordering::Acquire);
                    lifecycle_h.on_status(status, market_id, order_id);
                    // Clear rejected clocks so the next reconnect requests a full image
                    if status.error_code == Some(StatusErrorCode::InvalidClock) {
                        if market_id > 0 && status.id == Some(market_id) {
                            let _ = market_clk_tx_h.send(None);
                            let _ = market_initial_clk_tx_h.send(None);
                            lifecycle_h
                                .market_requires_image
                                .store(true, Ordering::Release);
                            lifecycle_h
                                .market_image_tainted
                                .store(false, Ordering::Release);
                            log::warn!(
                                "Betfair market stream INVALID_CLOCK: clocks cleared, \
                                 next reconnect will request a full image",
                            );
                        } else if order_id > 0 && status.id == Some(order_id) {
                            let _ = order_clk_tx_h.send(None);
                            let _ = order_initial_clk_tx_h.send(None);
                            lifecycle_h
                                .order_requires_image
                                .store(true, Ordering::Release);
                            lifecycle_h
                                .order_image_tainted
                                .store(false, Ordering::Release);
                            log::warn!(
                                "Betfair order stream INVALID_CLOCK: clocks cleared, \
                                 next reconnect will request a full image",
                            );
                        }
                    } else if status.connection_closed {
                        log::warn!(
                            "Betfair stream connection closed by server: {:?} - {:?}",
                            status.error_code,
                            status.error_message,
                        );
                    } else if status.error_code.is_some() {
                        log::warn!(
                            "Betfair stream status error: {:?} - {:?}",
                            status.error_code,
                            status.error_message,
                        );
                    }
                    handler.handle(data, msg);
                }
                StreamMessage::Connection(_) => {
                    reconnect_auth_h.request_pending();
                    handler.handle(data, msg);
                }
                _ => {
                    handler.handle(data, msg);
                }
            }
        });

        let auth_reconnect = auth_rx;
        let reconnect_auth_replay = Arc::clone(&reconnect_auth);
        let market_state_replay = Arc::clone(&market_state_lock);
        let order_state_replay = Arc::clone(&order_state_lock);
        let reconnect_replay: SocketReconnectReplay = Arc::new(move || {
            let mut replay = Vec::with_capacity(3);
            let auth = auth_reconnect.borrow().clone();
            reconnect_auth_replay.record_replay(auth.generation);

            replay.push(auth.bytes);

            {
                let _state = lock_stream_state(&market_state_replay);

                if let Some(mut sub) = market_sub_rx.borrow().clone() {
                    sub.clk = market_clk_rx.borrow().clone();
                    sub.initial_clk = market_initial_clk_rx.borrow().clone();
                    if let Ok(sub_bytes) = serde_json::to_vec(&sub) {
                        replay.push(Bytes::from(sub_bytes));
                    }
                }
            }

            {
                let _state = lock_stream_state(&order_state_replay);

                if let Some(mut sub) = order_sub_rx.borrow().clone() {
                    sub.clk = order_clk_rx.borrow().clone();
                    sub.initial_clk = order_initial_clk_rx.borrow().clone();
                    if let Ok(sub_bytes) = serde_json::to_vec(&sub) {
                        replay.push(Bytes::from(sub_bytes));
                    }
                }
            }

            replay
        });

        let url = format!("{}:{}", config.host, config.port);
        let lifecycle_sink = Arc::clone(&lifecycle);
        let market_id_sink = Arc::clone(&market_active_sub_id);
        let order_id_sink = Arc::clone(&order_active_sub_id);
        let last_inbound_sink = Arc::clone(&last_inbound);
        let market_state_sink = Arc::clone(&market_state_lock);
        let order_state_sink = Arc::clone(&order_state_lock);
        let lifecycle_callback = move |state| {
            let _market_state = lock_stream_state(&market_state_sink);
            let _order_state = lock_stream_state(&order_state_sink);
            lifecycle_sink.on_transport(
                state,
                market_id_sink.load(Ordering::Acquire),
                order_id_sink.load(Ordering::Acquire),
            );
            *last_inbound_sink
                .lock()
                .expect("last inbound lock poisoned") = Instant::now();
        };
        let state_sink = match state_sink {
            Some(sink) => sink.with_callback(lifecycle_callback),
            None => SocketStateSink::new(lifecycle_callback),
        };
        let socket_config = SocketConfig {
            url,
            mode,
            suffix: b"\r\n".to_vec(),
            message_handler: Some(message_handler),
            heartbeat: outbound_heartbeat(config.heartbeat_secs),
            connect_timeout_ms: None,
            reconnect_delay_initial_ms: Some(config.reconnect_delay_initial_ms),
            reconnect_delay_max_ms: Some(config.reconnect_delay_max_ms),
            reconnect_backoff_factor: None,
            reconnect_jitter_ms: None,
            connection_max_retries: None,
            reconnect_max_attempts: None,
            heartbeat_timeout_secs: heartbeat_timeout(
                heartbeat_timeout_source,
                config.heartbeat_secs,
                config.heartbeat_timeout_secs,
            ),
            certs_dir: None,
        };

        let socket = SocketClient::builder()
            .config(socket_config)
            .state_sink(state_sink)
            .reconnect_replay(reconnect_replay)
            .connect()
            .await
            .map_err(|e| BetfairStreamError::ConnectionFailed(e.to_string()))?;
        writer_tx_h
            .set(socket.writer_tx.clone())
            .expect("Betfair stream writer must only be initialized once");
        reconnect_auth.set_handle(socket.reconnect_handle());

        let dead_peer_task = if matches!(heartbeat_timeout_source, HeartbeatTimeoutSource::Server) {
            let reconnect = socket.reconnect_handle();
            let enabled = Arc::clone(&dead_peer_enabled);
            let last = Arc::clone(&last_inbound);
            let timeout_ms = Arc::clone(&dead_peer_timeout_ms);

            Some(get_runtime().spawn(async move {
                loop {
                    tokio::time::sleep(Duration::from_millis(100)).await;

                    if !enabled.load(Ordering::Acquire) {
                        continue;
                    }
                    let timeout = Duration::from_millis(timeout_ms.load(Ordering::Acquire));
                    if last.lock().expect("last inbound lock poisoned").elapsed() >= timeout {
                        let _ = reconnect.request_reconnect();
                    }
                }
            }))
        } else {
            None
        };

        socket
            .send_bytes(auth_bytes_vec)
            .await
            .map_err(|e| BetfairStreamError::ConnectionFailed(e.to_string()))?;

        Ok(Self {
            socket,
            market_sub_tx,
            market_clk_tx,
            market_initial_clk_tx,
            order_sub_tx,
            order_clk_tx,
            order_initial_clk_tx,
            market_active_sub_id,
            order_active_sub_id,
            request_id,
            market_state_lock,
            order_state_lock,
            auth_tx,
            reconnect_auth,
            lifecycle,
            dead_peer_enabled,
            dead_peer_timeout_ms,
            dead_peer_timeout_override: timeout_override,
            dead_peer_task,
            closed: AtomicBool::new(false),
        })
    }

    /// Subscribes to market data for the given filter and data fields.
    ///
    /// Stores the subscription for automatic replay on reconnection.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization or sending fails.
    pub async fn subscribe_markets(
        &self,
        market_filter: StreamMarketFilter,
        data_filter: MarketDataFilter,
        heartbeat_ms: Option<u64>,
        conflate_ms: Option<u64>,
    ) -> Result<(), BetfairStreamError> {
        if self.closed.load(Ordering::SeqCst) || self.socket.is_closed() {
            return Err(BetfairStreamError::Disconnected(
                "stream client is closed".to_string(),
            ));
        }
        let heartbeat_ms = heartbeat_ms.unwrap_or(BETFAIR_STREAM_SERVER_HEARTBEAT_MS);
        validate_subscription_heartbeat(heartbeat_ms)?;
        self.update_dead_peer_timeout(heartbeat_ms);
        let _state = lock_stream_state(&self.market_state_lock);
        let id = self.request_id.fetch_add(1, Ordering::Relaxed);
        // Advance the active ID before clearing clocks so that any in-flight MCMs
        // from the previous subscription are immediately rejected by the handler.
        self.market_active_sub_id.store(id, Ordering::SeqCst);
        self.lifecycle.market.set(StreamLifecycleState::Pending);
        self.lifecycle
            .market_was_current
            .store(false, Ordering::Release);
        self.lifecycle
            .market_requires_image
            .store(true, Ordering::Release);
        self.lifecycle
            .market_image_tainted
            .store(false, Ordering::Release);
        self.dead_peer_enabled.store(true, Ordering::Release);
        let sub = MarketSubscription {
            op: STREAM_OP_MARKET_SUBSCRIPTION.to_string(),
            id: Some(id),
            market_filter,
            market_data_filter: data_filter,
            clk: None,
            conflate_ms,
            heartbeat_ms: Some(heartbeat_ms),
            initial_clk: None,
            segmentation_enabled: Some(true),
        };

        // Reset clocks so a disconnect before the first MCM response doesn't replay
        // stale tokens from a previous subscription with different filters.
        let _ = self.market_clk_tx.send(None);
        let _ = self.market_initial_clk_tx.send(None);
        let _ = self.market_sub_tx.send(Some(sub.clone()));

        let data = Bytes::from(serde_json::to_vec(&sub)?);
        self.socket
            .writer_tx
            .send(WriterCommand::SendOrReplay {
                key: MARKET_SUBSCRIPTION_REPLAY_KEY,
                data,
            })
            .map_err(|e| BetfairStreamError::ConnectionFailed(e.to_string()))?;
        Ok(())
    }

    /// Subscribes to order updates.
    ///
    /// Stores the subscription for automatic replay on reconnection.
    ///
    /// # Errors
    ///
    /// Returns an error if serialization or sending fails.
    pub async fn subscribe_orders(
        &self,
        order_filter: Option<OrderFilter>,
        heartbeat_ms: Option<u64>,
    ) -> Result<(), BetfairStreamError> {
        if self.closed.load(Ordering::SeqCst) || self.socket.is_closed() {
            return Err(BetfairStreamError::Disconnected(
                "stream client is closed".to_string(),
            ));
        }
        let heartbeat_ms = heartbeat_ms.unwrap_or(BETFAIR_STREAM_SERVER_HEARTBEAT_MS);
        validate_subscription_heartbeat(heartbeat_ms)?;
        self.update_dead_peer_timeout(heartbeat_ms);
        let _state = lock_stream_state(&self.order_state_lock);
        let id = self.request_id.fetch_add(1, Ordering::Relaxed);
        self.order_active_sub_id.store(id, Ordering::SeqCst);
        self.lifecycle.order.set(StreamLifecycleState::Pending);
        self.lifecycle
            .order_was_current
            .store(false, Ordering::Release);
        self.lifecycle
            .order_requires_image
            .store(true, Ordering::Release);
        self.lifecycle
            .order_image_tainted
            .store(false, Ordering::Release);
        self.dead_peer_enabled.store(true, Ordering::Release);
        let sub = OrderSubscription {
            op: STREAM_OP_ORDER_SUBSCRIPTION.to_string(),
            id: Some(id),
            order_filter,
            clk: None,
            conflate_ms: None,
            heartbeat_ms: Some(heartbeat_ms),
            initial_clk: None,
            segmentation_enabled: Some(true),
        };

        // Reset clocks so a disconnect before the first OCM response doesn't replay
        // stale tokens from a previous subscription with different filters.
        let _ = self.order_clk_tx.send(None);
        let _ = self.order_initial_clk_tx.send(None);
        let _ = self.order_sub_tx.send(Some(sub.clone()));

        let data = Bytes::from(serde_json::to_vec(&sub)?);
        self.socket
            .writer_tx
            .send(WriterCommand::SendOrReplay {
                key: ORDER_SUBSCRIPTION_REPLAY_KEY,
                data,
            })
            .map_err(|e| BetfairStreamError::ConnectionFailed(e.to_string()))?;
        Ok(())
    }

    fn update_dead_peer_timeout(&self, heartbeat_ms: u64) {
        if !self.dead_peer_timeout_override {
            self.dead_peer_timeout_ms
                .store(heartbeat_ms.saturating_mul(2), Ordering::Release);
        }
    }

    /// Returns `true` if the connection is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.socket.is_active()
    }

    #[must_use]
    pub fn authentication_state(&self) -> StreamLifecycleState {
        self.lifecycle.authenticated.get()
    }

    #[must_use]
    pub fn market_subscription_state(&self) -> StreamLifecycleState {
        self.lifecycle.market.get()
    }

    #[must_use]
    pub fn order_subscription_state(&self) -> StreamLifecycleState {
        self.lifecycle.order.get()
    }

    /// Waits for the authentication lifecycle component to equal `expected`.
    ///
    /// Returns immediately if the component is already in the exact expected state;
    /// otherwise waits for a later transition. A transient expected state that is
    /// replaced before this task observes it can be missed because transitions are not
    /// recorded as history. This method has no internal timeout; callers wanting a
    /// bound should wrap it in [`tokio::time::timeout`].
    pub async fn wait_for_authentication_state(&self, expected: StreamLifecycleState) {
        wait_for_lifecycle_state(&self.lifecycle.authenticated, expected).await;
    }

    /// Waits for the market subscription lifecycle component to equal `expected`.
    ///
    /// Returns immediately if the component is already in the exact expected state;
    /// otherwise waits for a later transition. A transient expected state that is
    /// replaced before this task observes it can be missed because transitions are not
    /// recorded as history. This method has no internal timeout; callers wanting a
    /// bound should wrap it in [`tokio::time::timeout`].
    pub async fn wait_for_market_subscription_state(&self, expected: StreamLifecycleState) {
        wait_for_lifecycle_state(&self.lifecycle.market, expected).await;
    }

    /// Waits for the order subscription lifecycle component to equal `expected`.
    ///
    /// Returns immediately if the component is already in the exact expected state;
    /// otherwise waits for a later transition. A transient expected state that is
    /// replaced before this task observes it can be missed because transitions are not
    /// recorded as history. This method has no internal timeout; callers wanting a
    /// bound should wrap it in [`tokio::time::timeout`].
    pub async fn wait_for_order_subscription_state(&self, expected: StreamLifecycleState) {
        wait_for_lifecycle_state(&self.lifecycle.order, expected).await;
    }

    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.socket.is_active() && self.authentication_state() == StreamLifecycleState::Active
    }

    #[must_use]
    pub fn is_market_ready(&self) -> bool {
        self.is_authenticated() && self.market_subscription_state() == StreamLifecycleState::Active
    }

    #[must_use]
    pub fn is_order_ready(&self) -> bool {
        self.is_authenticated() && self.order_subscription_state() == StreamLifecycleState::Active
    }

    /// Pushes refreshed auth bytes so the next reconnection or subscription uses
    /// the current session token instead of the one from initial connect.
    pub fn update_auth(&self, app_key: &str, session_token: String) {
        update_auth_state(
            &self.auth_tx,
            &Authentication::with_id(app_key.to_string(), session_token, AUTH_REQUEST_ID),
        );
    }

    /// Requests replacement of the active stream transport.
    ///
    /// Returns `true` only when this call starts a reconnect. Duplicate requests and requests after
    /// close return `false`.
    #[must_use]
    pub fn request_reconnect(&self) -> bool {
        self.request_reconnect_outcome() == ReconnectRequestOutcome::Accepted
    }

    pub(crate) fn request_reconnect_outcome(&self) -> ReconnectRequestOutcome {
        if self.closed.load(Ordering::SeqCst) {
            return ReconnectRequestOutcome::Closed;
        }
        self.reconnect_auth
            .request(self.auth_tx.borrow().generation)
    }

    /// Closes the stream connection.
    pub async fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.dead_peer_enabled.store(false, Ordering::Release);

        if let Some(task) = &self.dead_peer_task {
            task.abort();
        }
        self.socket.close().await;
    }
}

impl Drop for BetfairStreamClient {
    fn drop(&mut self) {
        self.dead_peer_enabled.store(false, Ordering::Release);

        if let Some(task) = &self.dead_peer_task {
            task.abort();
        }
    }
}

fn lock_stream_state(lock: &Mutex<()>) -> MutexGuard<'_, ()> {
    lock.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Betfair race stream client for Total Performance Data (TPD).
///
/// Connects to `sports-data-stream-api.betfair.com` and subscribes to Race Change
/// Messages (RCM) with live GPS tracking data. Simpler than [`BetfairStreamClient`]:
/// no clk-based delta resumption, just auth + raceSubscription on (re)connect.
#[derive(Debug)]
pub struct BetfairRaceStreamClient {
    socket: SocketClient,
    auth_tx: watch::Sender<StreamAuth>,
    reconnect_auth: Arc<ReconnectAuthState>,
    closed: AtomicBool,
}

impl BetfairRaceStreamClient {
    /// Connects to the Betfair race stream and subscribes.
    ///
    /// The `race_fatal_tx` channel receives a signal when the server returns a
    /// fatal status error (e.g. NOT_AUTHORIZED, no TPD entitlement). The caller
    /// should monitor this channel and close the client when it fires.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails or the initial send fails.
    pub async fn connect(
        credential: &BetfairCredential,
        session_token: String,
        handler: TcpMessageHandler,
        config: BetfairStreamConfig,
        race_fatal_tx: tokio::sync::mpsc::UnboundedSender<()>,
    ) -> Result<Self, BetfairStreamError> {
        let subscription = AuxiliaryStreamSubscription::race(race_fatal_tx)?;
        Self::connect_with_subscription(
            credential,
            session_token,
            StreamHandler::Raw(handler),
            config,
            subscription,
            None,
        )
        .await
    }

    pub(crate) async fn connect_decoded(
        credential: &BetfairCredential,
        session_token: String,
        handler: StreamMessageHandler,
        config: BetfairStreamConfig,
        race_fatal_tx: tokio::sync::mpsc::UnboundedSender<()>,
        state_sink: Option<SocketStateSink>,
    ) -> Result<Self, BetfairStreamError> {
        let subscription = AuxiliaryStreamSubscription::race(race_fatal_tx)?;
        Self::connect_with_subscription(
            credential,
            session_token,
            StreamHandler::Decoded(handler),
            config,
            subscription,
            state_sink,
        )
        .await
    }

    /// Connects to the Betfair sports data stream and subscribes to cricket.
    ///
    /// The `cricket_fatal_tx` channel receives a signal when the server returns
    /// a fatal status error.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection fails or the initial send fails.
    pub async fn connect_cricket(
        credential: &BetfairCredential,
        session_token: String,
        handler: TcpMessageHandler,
        config: BetfairStreamConfig,
        cricket_fatal_tx: tokio::sync::mpsc::UnboundedSender<()>,
    ) -> Result<Self, BetfairStreamError> {
        let subscription = AuxiliaryStreamSubscription::cricket(cricket_fatal_tx)?;
        Self::connect_with_subscription(
            credential,
            session_token,
            StreamHandler::Raw(handler),
            config,
            subscription,
            None,
        )
        .await
    }

    pub(crate) async fn connect_cricket_decoded(
        credential: &BetfairCredential,
        session_token: String,
        handler: StreamMessageHandler,
        config: BetfairStreamConfig,
        cricket_fatal_tx: tokio::sync::mpsc::UnboundedSender<()>,
        state_sink: Option<SocketStateSink>,
    ) -> Result<Self, BetfairStreamError> {
        let subscription = AuxiliaryStreamSubscription::cricket(cricket_fatal_tx)?;
        Self::connect_with_subscription(
            credential,
            session_token,
            StreamHandler::Decoded(handler),
            config,
            subscription,
            state_sink,
        )
        .await
    }

    async fn connect_with_subscription(
        credential: &BetfairCredential,
        session_token: String,
        handler: StreamHandler,
        config: BetfairStreamConfig,
        subscription: AuxiliaryStreamSubscription,
        state_sink: Option<SocketStateSink>,
    ) -> Result<Self, BetfairStreamError> {
        let AuxiliaryStreamSubscription {
            bytes: sub_bytes,
            label,
            fatal_hint,
            fatal_tx,
        } = subscription;

        let auth = Authentication::new(credential.app_key().to_string(), session_token);
        let auth_bytes_vec = serde_json::to_vec(&auth)?;
        let auth_bytes = Bytes::from(auth_bytes_vec.clone());
        let reconnect_auth = Arc::new(ReconnectAuthState::default());
        let (auth_tx, auth_rx) = watch::channel(StreamAuth {
            generation: 0,
            bytes: auth_bytes,
        });

        let mode = if config.use_tls {
            Mode::Tls
        } else {
            Mode::Plain
        };

        let reconnect_auth_h = Arc::clone(&reconnect_auth);
        let message_handler: TcpMessageHandler = Arc::new(move |data: &[u8]| {
            let Some(msg) = handler.decode(data) else {
                return;
            };

            if let StreamMessage::Status(status) = &msg {
                if let Some(ref code) = status.error_code
                    && code.is_race_stream_fatal()
                {
                    log::error!(
                        "Betfair {label} stream fatal error: {:?} - {:?} ({fatal_hint})",
                        status.error_code,
                        status.error_message,
                    );
                    let _ = fatal_tx.send(());
                    return;
                }

                if status.connection_closed {
                    log::warn!(
                        "Betfair {label} stream closed: {:?} - {:?}",
                        status.error_code,
                        status.error_message,
                    );
                } else if status.error_code.is_some() {
                    log::warn!(
                        "Betfair {label} stream status: {:?} - {:?}",
                        status.error_code,
                        status.error_message,
                    );
                }
            }

            if matches!(msg, StreamMessage::Connection(_)) {
                reconnect_auth_h.request_pending();
            }

            handler.handle(data, msg);
        });

        let auth_reconnect = auth_rx;
        let reconnect_auth_replay = Arc::clone(&reconnect_auth);
        let sub_reconnect = sub_bytes.clone();
        let reconnect_replay: SocketReconnectReplay = Arc::new(move || {
            let auth = auth_reconnect.borrow().clone();
            reconnect_auth_replay.record_replay(auth.generation);
            let mut combined = Vec::with_capacity(auth.bytes.len() + 2 + sub_reconnect.len());
            combined.extend_from_slice(&auth.bytes);
            combined.extend_from_slice(b"\r\n");
            combined.extend_from_slice(&sub_reconnect);
            vec![Bytes::from(combined)]
        });

        let url = format!("{}:{}", config.host, config.port);
        let socket_config = SocketConfig {
            url,
            mode,
            suffix: b"\r\n".to_vec(),
            message_handler: Some(message_handler),
            heartbeat: outbound_heartbeat(config.heartbeat_secs),
            connect_timeout_ms: None,
            reconnect_delay_initial_ms: Some(config.reconnect_delay_initial_ms),
            reconnect_delay_max_ms: Some(config.reconnect_delay_max_ms),
            reconnect_backoff_factor: None,
            reconnect_jitter_ms: None,
            connection_max_retries: None,
            reconnect_max_attempts: None,
            heartbeat_timeout_secs: heartbeat_timeout(
                HeartbeatTimeoutSource::Outbound,
                config.heartbeat_secs,
                config.heartbeat_timeout_secs,
            ),
            certs_dir: None,
        };

        let socket = SocketClient::builder()
            .config(socket_config)
            .maybe_state_sink(state_sink)
            .reconnect_replay(reconnect_replay)
            .connect()
            .await
            .map_err(|e| BetfairStreamError::ConnectionFailed(e.to_string()))?;
        reconnect_auth.set_handle(socket.reconnect_handle());

        let mut combined = Vec::with_capacity(auth_bytes_vec.len() + 2 + sub_bytes.len());
        combined.extend_from_slice(&auth_bytes_vec);
        combined.extend_from_slice(b"\r\n");
        combined.extend_from_slice(&sub_bytes);
        socket
            .send_bytes(combined)
            .await
            .map_err(|e| BetfairStreamError::ConnectionFailed(e.to_string()))?;

        Ok(Self {
            socket,
            auth_tx,
            reconnect_auth,
            closed: AtomicBool::new(false),
        })
    }

    /// Returns `true` if the connection is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.socket.is_active()
    }

    /// Pushes refreshed auth bytes so the next reconnection uses
    /// the current session token instead of the one from initial connect.
    pub fn update_auth(&self, app_key: &str, session_token: String) {
        update_auth_state(
            &self.auth_tx,
            &Authentication::new(app_key.to_string(), session_token),
        );
    }

    /// Requests replacement of the active stream transport.
    ///
    /// Returns `true` only when this call starts a reconnect. Duplicate requests and requests after
    /// close return `false`.
    #[must_use]
    pub fn request_reconnect(&self) -> bool {
        self.request_reconnect_outcome() == ReconnectRequestOutcome::Accepted
    }

    /// Requests transport replacement and returns the exact controller outcome.
    pub(crate) fn request_reconnect_outcome(&self) -> ReconnectRequestOutcome {
        if self.closed.load(Ordering::SeqCst) {
            return ReconnectRequestOutcome::Closed;
        }
        self.reconnect_auth
            .request(self.auth_tx.borrow().generation)
    }

    /// Closes the race stream connection.
    pub async fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.socket.close().await;
    }
}

fn update_auth_state(auth_tx: &watch::Sender<StreamAuth>, auth: &Authentication) {
    let Ok(bytes) = serde_json::to_vec(auth) else {
        return;
    };
    let bytes = Bytes::from(bytes);
    auth_tx.send_if_modified(|current| {
        if current.bytes == bytes {
            return false;
        }
        *current = StreamAuth {
            generation: current.generation.wrapping_add(1),
            bytes,
        };
        true
    });
}

enum StreamHandler {
    Raw(TcpMessageHandler),
    Decoded(StreamMessageHandler),
}

impl StreamHandler {
    fn decode(&self, data: &[u8]) -> Option<StreamMessage> {
        match stream_decode(data) {
            Ok(message) => Some(message),
            Err(e) => {
                match self {
                    Self::Raw(handler) => handler(data),
                    Self::Decoded(_) => log::warn!("Failed to decode stream message: {e}"),
                }
                None
            }
        }
    }

    fn handle(&self, data: &[u8], message: StreamMessage) {
        match self {
            Self::Raw(handler) => handler(data),
            Self::Decoded(handler) => handler(message),
        }
    }
}

const fn change_complete(segment_type: Option<SegmentType>) -> bool {
    matches!(segment_type, None | Some(SegmentType::SegEnd))
}

fn reissue_market_subscription(
    request_id: &AtomicU64,
    active_id: &AtomicU64,
    lifecycle: &ProtocolLifecycle,
    sub_tx: &watch::Sender<Option<MarketSubscription>>,
    clk_tx: &watch::Sender<Option<String>>,
    initial_clk_tx: &watch::Sender<Option<String>>,
    writer_tx: Option<&tokio::sync::mpsc::UnboundedSender<WriterCommand>>,
) {
    let Some(writer_tx) = writer_tx else {
        log::error!("Cannot recover Betfair market stream before writer initialization");
        return;
    };
    let Some(mut sub) = sub_tx.borrow().clone() else {
        log::error!("Cannot recover Betfair market stream without a retained subscription");
        return;
    };
    let id = request_id.fetch_add(1, Ordering::Relaxed);
    sub.id = Some(id);
    sub.clk = None;
    sub.initial_clk = None;
    let data = match serde_json::to_vec(&sub) {
        Ok(data) => Bytes::from(data),
        Err(e) => {
            log::error!("Failed to serialize Betfair market recovery subscription: {e}");
            return;
        }
    };

    active_id.store(id, Ordering::SeqCst);
    lifecycle.market.set(StreamLifecycleState::Pending);
    lifecycle.market_was_current.store(false, Ordering::Release);
    lifecycle
        .market_requires_image
        .store(true, Ordering::Release);
    lifecycle
        .market_image_tainted
        .store(false, Ordering::Release);
    let _ = clk_tx.send(None);
    let _ = initial_clk_tx.send(None);
    let _ = sub_tx.send(Some(sub));

    if let Err(e) = writer_tx.send(WriterCommand::SendOrReplay {
        key: MARKET_SUBSCRIPTION_REPLAY_KEY,
        data,
    }) {
        log::error!("Failed to queue Betfair market recovery subscription: {e}");
    }
}

fn reissue_order_subscription(
    request_id: &AtomicU64,
    active_id: &AtomicU64,
    lifecycle: &ProtocolLifecycle,
    sub_tx: &watch::Sender<Option<OrderSubscription>>,
    clk_tx: &watch::Sender<Option<String>>,
    initial_clk_tx: &watch::Sender<Option<String>>,
    writer_tx: Option<&tokio::sync::mpsc::UnboundedSender<WriterCommand>>,
) {
    let Some(writer_tx) = writer_tx else {
        log::error!("Cannot recover Betfair order stream before writer initialization");
        return;
    };
    let Some(mut sub) = sub_tx.borrow().clone() else {
        log::error!("Cannot recover Betfair order stream without a retained subscription");
        return;
    };
    let id = request_id.fetch_add(1, Ordering::Relaxed);
    sub.id = Some(id);
    sub.clk = None;
    sub.initial_clk = None;
    let data = match serde_json::to_vec(&sub) {
        Ok(data) => Bytes::from(data),
        Err(e) => {
            log::error!("Failed to serialize Betfair order recovery subscription: {e}");
            return;
        }
    };

    active_id.store(id, Ordering::SeqCst);
    lifecycle.order.set(StreamLifecycleState::Pending);
    lifecycle.order_was_current.store(false, Ordering::Release);
    lifecycle
        .order_requires_image
        .store(true, Ordering::Release);
    lifecycle
        .order_image_tainted
        .store(false, Ordering::Release);
    let _ = clk_tx.send(None);
    let _ = initial_clk_tx.send(None);
    let _ = sub_tx.send(Some(sub));

    if let Err(e) = writer_tx.send(WriterCommand::SendOrReplay {
        key: ORDER_SUBSCRIPTION_REPLAY_KEY,
        data,
    }) {
        log::error!("Failed to queue Betfair order recovery subscription: {e}");
    }
}

fn update_stream_state(
    clk: &Option<String>,
    initial_clk: &Option<String>,
    heartbeat_ms: Option<u64>,
    clk_tx: &watch::Sender<Option<String>>,
    initial_clk_tx: &watch::Sender<Option<String>>,
    timeout_override: bool,
    dead_peer_timeout_ms: &AtomicU64,
) {
    if clk.is_some() {
        let _ = clk_tx.send(clk.clone());
    }

    if initial_clk.is_some() {
        let _ = initial_clk_tx.send(initial_clk.clone());
    }
    update_negotiated_heartbeat(heartbeat_ms, timeout_override, dead_peer_timeout_ms);
}

fn update_negotiated_heartbeat(
    interval_ms: Option<u64>,
    timeout_override: bool,
    dead_peer_timeout_ms: &AtomicU64,
) {
    if !timeout_override
        && let Some(interval_ms) = interval_ms
        && (BETFAIR_STREAM_HEARTBEAT_MIN_MS..=BETFAIR_STREAM_HEARTBEAT_MAX_MS)
            .contains(&interval_ms)
    {
        dead_peer_timeout_ms.store(interval_ms.saturating_mul(2), Ordering::Release);
    }
}

fn outbound_heartbeat(interval_secs: Option<u64>) -> Option<SocketHeartbeat> {
    interval_secs.map(|interval_secs| SocketHeartbeat {
        interval_secs,
        payload: b"{\"op\":\"heartbeat\"}".to_vec(),
    })
}

fn heartbeat_timeout(
    source: HeartbeatTimeoutSource,
    interval_secs: Option<u64>,
    timeout_secs: Option<u64>,
) -> Option<u64> {
    match source {
        HeartbeatTimeoutSource::Outbound => {
            interval_secs.map(|interval| timeout_secs.unwrap_or(interval.saturating_mul(2)))
        }
        HeartbeatTimeoutSource::Server => None,
    }
}

fn validate_subscription_heartbeat(heartbeat_ms: u64) -> Result<(), BetfairStreamError> {
    if !(BETFAIR_STREAM_HEARTBEAT_MIN_MS..=BETFAIR_STREAM_HEARTBEAT_MAX_MS).contains(&heartbeat_ms)
    {
        return Err(BetfairStreamError::ProtocolError(format!(
            "subscription heartbeat must be in range [{BETFAIR_STREAM_HEARTBEAT_MIN_MS}, \
             {BETFAIR_STREAM_HEARTBEAT_MAX_MS}] ms, was {heartbeat_ms} ms",
        )));
    }

    Ok(())
}

struct AuxiliaryStreamSubscription {
    bytes: Bytes,
    label: &'static str,
    fatal_hint: &'static str,
    fatal_tx: tokio::sync::mpsc::UnboundedSender<()>,
}

impl AuxiliaryStreamSubscription {
    fn race(fatal_tx: tokio::sync::mpsc::UnboundedSender<()>) -> Result<Self, serde_json::Error> {
        Ok(Self {
            bytes: Bytes::from(serde_json::to_vec(&RaceSubscription::new(1))?),
            label: "race",
            fatal_hint: "check TPD entitlement on your Betfair app key",
            fatal_tx,
        })
    }

    fn cricket(
        fatal_tx: tokio::sync::mpsc::UnboundedSender<()>,
    ) -> Result<Self, serde_json::Error> {
        Ok(Self {
            bytes: Bytes::from(serde_json::to_vec(&CricketSubscription::new(1))?),
            label: "cricket",
            fatal_hint: "check cricket data entitlement on your Betfair app key",
            fatal_tx,
        })
    }
}

#[derive(Clone, Debug)]
struct StreamAuth {
    generation: u64,
    bytes: Bytes,
}

#[derive(Debug, Default)]
struct ReconnectAuthState {
    replay_generation: AtomicU64,
    pending_generation: AtomicU64,
    reconnect_handle: OnceLock<SocketReconnectHandle>,
}

impl ReconnectAuthState {
    fn set_handle(&self, handle: SocketReconnectHandle) {
        let result = self.reconnect_handle.set(handle);
        debug_assert!(result.is_ok(), "reconnect handle is set only once");
    }

    fn record_replay(&self, generation: u64) {
        self.replay_generation.store(generation, Ordering::SeqCst);
        let _ = self
            .pending_generation
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |pending| {
                (pending != 0 && pending <= generation).then_some(0)
            });
    }

    fn request(&self, auth_generation: u64) -> ReconnectRequestOutcome {
        let Some(handle) = self.reconnect_handle.get() else {
            return ReconnectRequestOutcome::Unsupported;
        };

        let outcome = handle.request_reconnect();
        if outcome == ReconnectRequestOutcome::AlreadyReconnecting
            && auth_generation > self.replay_generation.load(Ordering::SeqCst)
        {
            self.pending_generation
                .fetch_max(auth_generation, Ordering::SeqCst);
        }

        outcome
    }

    fn request_pending(&self) {
        let pending_generation = self.pending_generation.load(Ordering::SeqCst);
        if pending_generation == 0
            || pending_generation <= self.replay_generation.load(Ordering::SeqCst)
        {
            return;
        }

        let Some(handle) = self.reconnect_handle.get() else {
            return;
        };

        match handle.request_reconnect() {
            ReconnectRequestOutcome::Accepted => {
                let _ = self.pending_generation.compare_exchange(
                    pending_generation,
                    0,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                );
            }
            ReconnectRequestOutcome::AlreadyReconnecting => {}
            ReconnectRequestOutcome::Disconnected
            | ReconnectRequestOutcome::Closed
            | ReconnectRequestOutcome::Unsupported => {
                self.pending_generation.store(0, Ordering::SeqCst);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use nautilus_network::SocketState;
    use rstest::rstest;

    use super::*;
    use crate::stream::messages::{
        Authentication, CricketSubscription, MarketDataFilter, RaceSubscription, StreamMarketFilter,
    };

    #[rstest]
    #[case::no_source(HeartbeatTimeoutSource::Outbound, None, None, None)]
    #[case::outbound_override(HeartbeatTimeoutSource::Outbound, Some(5), Some(60), Some(60))]
    #[case::outbound_derived(HeartbeatTimeoutSource::Outbound, Some(5), None, Some(10))]
    #[case::server(HeartbeatTimeoutSource::Server, None, None, None)]
    fn test_heartbeat_timeout(
        #[case] source: HeartbeatTimeoutSource,
        #[case] interval_secs: Option<u64>,
        #[case] timeout_secs: Option<u64>,
        #[case] expected: Option<u64>,
    ) {
        assert_eq!(
            heartbeat_timeout(source, interval_secs, timeout_secs),
            expected
        );
    }

    #[rstest]
    fn test_reissue_before_writer_initialization_stays_fail_closed() {
        let request_id = AtomicU64::new(17);
        let market_active_id = AtomicU64::new(11);
        let order_active_id = AtomicU64::new(13);
        let lifecycle = ProtocolLifecycle::default();
        lifecycle.market.set(StreamLifecycleState::Degraded);
        lifecycle.order.set(StreamLifecycleState::Degraded);
        let (market_sub_tx, _market_sub_rx) = watch::channel(None::<MarketSubscription>);
        let (order_sub_tx, _order_sub_rx) = watch::channel(None::<OrderSubscription>);
        let (market_clk_tx, _market_clk_rx) = watch::channel(None::<String>);
        let (market_initial_clk_tx, _market_initial_clk_rx) = watch::channel(None::<String>);
        let (order_clk_tx, _order_clk_rx) = watch::channel(None::<String>);
        let (order_initial_clk_tx, _order_initial_clk_rx) = watch::channel(None::<String>);

        reissue_market_subscription(
            &request_id,
            &market_active_id,
            &lifecycle,
            &market_sub_tx,
            &market_clk_tx,
            &market_initial_clk_tx,
            None,
        );
        reissue_order_subscription(
            &request_id,
            &order_active_id,
            &lifecycle,
            &order_sub_tx,
            &order_clk_tx,
            &order_initial_clk_tx,
            None,
        );

        assert_eq!(
            (
                request_id.load(Ordering::Acquire),
                market_active_id.load(Ordering::Acquire),
                order_active_id.load(Ordering::Acquire),
                lifecycle.market.get(),
                lifecycle.order.get(),
            ),
            (
                17,
                11,
                13,
                StreamLifecycleState::Degraded,
                StreamLifecycleState::Degraded,
            ),
        );
    }

    #[rstest]
    fn test_invalid_clock_status_resets_clocks() {
        let (market_clk_tx, market_clk_rx) = watch::channel(Some("old-market-clk".to_string()));
        let (market_initial_clk_tx, market_initial_clk_rx) =
            watch::channel(Some("old-market-iclk".to_string()));
        let (order_clk_tx, order_clk_rx) = watch::channel(Some("old-order-clk".to_string()));
        let (order_initial_clk_tx, order_initial_clk_rx) =
            watch::channel(Some("old-order-iclk".to_string()));

        let handler: TcpMessageHandler = Arc::new(move |data: &[u8]| {
            if let Ok(msg) = stream_decode(data)
                && let StreamMessage::Status(status) = &msg
                && status.error_code == Some(StatusErrorCode::InvalidClock)
            {
                let _ = market_clk_tx.send(None);
                let _ = market_initial_clk_tx.send(None);
                let _ = order_clk_tx.send(None);
                let _ = order_initial_clk_tx.send(None);
            }
        });

        handler(
            br#"{"op":"status","statusCode":"503","errorCode":"INVALID_CLOCK","connectionClosed":true}"#,
        );

        assert!(
            market_clk_rx.borrow().is_none(),
            "market clk must be cleared"
        );
        assert!(
            market_initial_clk_rx.borrow().is_none(),
            "market initialClk must be cleared"
        );
        assert!(order_clk_rx.borrow().is_none(), "order clk must be cleared");
        assert!(
            order_initial_clk_rx.borrow().is_none(),
            "order initialClk must be cleared"
        );
    }

    #[rstest]
    fn test_auth_message_serialization() {
        let auth = Authentication::new("my-app-key".to_string(), "my-session".to_string());
        let json = serde_json::to_string(&auth).unwrap();
        assert!(json.contains("\"op\":\"authentication\""));
        assert!(json.contains("\"appKey\":\"my-app-key\""));
        assert!(json.contains("\"session\":\"my-session\""));
    }

    #[rstest]
    #[case::exchange(true)]
    #[case::auxiliary(false)]
    fn test_update_auth_state_changes_once_per_distinct_payload(#[case] with_id: bool) {
        let make_auth = |session: &str| {
            if with_id {
                Authentication::with_id(
                    "test-app-key".to_string(),
                    session.to_string(),
                    AUTH_REQUEST_ID,
                )
            } else {
                Authentication::new("test-app-key".to_string(), session.to_string())
            }
        };
        let initial = make_auth("initial");
        let initial_bytes = Bytes::from(serde_json::to_vec(&initial).unwrap());
        let (auth_tx, auth_rx) = watch::channel(StreamAuth {
            generation: 7,
            bytes: initial_bytes.clone(),
        });

        update_auth_state(&auth_tx, &initial);
        assert_eq!(auth_rx.borrow().generation, 7);
        assert_eq!(auth_rx.borrow().bytes, initial_bytes);

        let replacement = make_auth("replacement");
        let replacement_bytes = Bytes::from(serde_json::to_vec(&replacement).unwrap());
        update_auth_state(&auth_tx, &replacement);
        assert_eq!(auth_rx.borrow().generation, 8);
        assert_eq!(auth_rx.borrow().bytes, replacement_bytes);

        update_auth_state(&auth_tx, &replacement);
        assert_eq!(auth_rx.borrow().generation, 8);
    }

    #[rstest]
    fn test_clk_is_updated_from_mcm() {
        let (market_clk_tx, market_clk_rx) = watch::channel(None::<String>);
        let (market_initial_clk_tx, market_initial_clk_rx) = watch::channel(None::<String>);
        let (order_clk_tx, order_clk_rx) = watch::channel(None::<String>);
        let (order_initial_clk_tx, order_initial_clk_rx) = watch::channel(None::<String>);
        let market_active_sub_id = Arc::new(AtomicU64::new(5));
        let order_active_sub_id = Arc::new(AtomicU64::new(6));

        let handler: TcpMessageHandler = Arc::new(move |data: &[u8]| {
            if let Ok(msg) = stream_decode(data) {
                match &msg {
                    StreamMessage::MarketChange(mcm) => {
                        let active = market_active_sub_id.load(Ordering::SeqCst);
                        if active > 0 && mcm.id.is_none_or(|id| id == active) {
                            if mcm.clk.is_some() {
                                let _ = market_clk_tx.send(mcm.clk.clone());
                            }

                            if mcm.initial_clk.is_some() {
                                let _ = market_initial_clk_tx.send(mcm.initial_clk.clone());
                            }
                        }
                    }
                    StreamMessage::OrderChange(ocm) => {
                        let active = order_active_sub_id.load(Ordering::SeqCst);
                        if active > 0 && ocm.id.is_none_or(|id| id == active) {
                            if ocm.clk.is_some() {
                                let _ = order_clk_tx.send(ocm.clk.clone());
                            }

                            if ocm.initial_clk.is_some() {
                                let _ = order_initial_clk_tx.send(ocm.initial_clk.clone());
                            }
                        }
                    }
                    _ => {}
                }
            }
        });

        // MCM/OCM with matching subscription id update clocks.
        handler(br#"{"op":"mcm","id":5,"pt":1000,"initialClk":"mcm-iclk","clk":"mcm-clk"}"#);
        handler(br#"{"op":"ocm","id":6,"pt":2000,"initialClk":"ocm-iclk","clk":"ocm-clk"}"#);

        assert_eq!(market_clk_rx.borrow().as_deref(), Some("mcm-clk"));
        assert_eq!(market_initial_clk_rx.borrow().as_deref(), Some("mcm-iclk"));
        assert_eq!(order_clk_rx.borrow().as_deref(), Some("ocm-clk"));
        assert_eq!(order_initial_clk_rx.borrow().as_deref(), Some("ocm-iclk"));

        // MCM without an id (e.g. heartbeat) is accepted for the active subscription.
        handler(br#"{"op":"mcm","pt":1001,"clk":"hb-clk"}"#);
        assert_eq!(market_clk_rx.borrow().as_deref(), Some("hb-clk"));

        // MCM from a stale subscription (explicit wrong id) must not overwrite stored clocks.
        handler(br#"{"op":"mcm","id":4,"pt":1002,"clk":"stale-clk"}"#);
        assert_eq!(market_clk_rx.borrow().as_deref(), Some("hb-clk"));
    }

    #[rstest]
    fn test_reconnect_callback_sends_auth_and_subscription() {
        let (market_clk_tx, market_clk_rx) = watch::channel(Some("mcm-clk1".to_string()));
        let (market_initial_clk_tx, market_initial_clk_rx) =
            watch::channel(Some("mcm-iclk1".to_string()));
        let (order_clk_tx, order_clk_rx) = watch::channel(Some("ocm-clk1".to_string()));
        let (order_initial_clk_tx, order_initial_clk_rx) =
            watch::channel(Some("ocm-iclk1".to_string()));
        let (market_sub_tx, market_sub_rx) = watch::channel(None::<MarketSubscription>);
        let (order_sub_tx, order_sub_rx) = watch::channel(None::<OrderSubscription>);

        let auth = Authentication::new("key".to_string(), "token".to_string());
        let auth_bytes = Bytes::from(serde_json::to_vec(&auth).unwrap());

        let _ = market_sub_tx.send(Some(MarketSubscription {
            op: STREAM_OP_MARKET_SUBSCRIPTION.to_string(),
            id: Some(1),
            market_filter: StreamMarketFilter::default(),
            market_data_filter: MarketDataFilter::default(),
            clk: None,
            conflate_ms: None,
            heartbeat_ms: Some(BETFAIR_STREAM_HEARTBEAT_MAX_MS),
            initial_clk: None,
            segmentation_enabled: Some(true),
        }));
        let _ = order_sub_tx.send(Some(OrderSubscription {
            op: STREAM_OP_ORDER_SUBSCRIPTION.to_string(),
            id: Some(2),
            order_filter: None,
            clk: None,
            conflate_ms: None,
            heartbeat_ms: Some(BETFAIR_STREAM_HEARTBEAT_MAX_MS),
            initial_clk: None,
            segmentation_enabled: Some(true),
        }));

        let auth_bytes_reconnect = auth_bytes;
        let reconnect_replay: SocketReconnectReplay = Arc::new(move || {
            let mut replay = Vec::with_capacity(3);
            let market_sub = market_sub_rx.borrow().clone();
            let order_sub = order_sub_rx.borrow().clone();

            replay.push(auth_bytes_reconnect.clone());

            if let Some(mut sub) = market_sub {
                sub.clk = market_clk_rx.borrow().clone();
                sub.initial_clk = market_initial_clk_rx.borrow().clone();
                if let Ok(sub_bytes) = serde_json::to_vec(&sub) {
                    replay.push(Bytes::from(sub_bytes));
                }
            }

            if let Some(mut sub) = order_sub {
                sub.clk = order_clk_rx.borrow().clone();
                sub.initial_clk = order_initial_clk_rx.borrow().clone();
                if let Ok(sub_bytes) = serde_json::to_vec(&sub) {
                    replay.push(Bytes::from(sub_bytes));
                }
            }

            replay
        });

        drop(market_clk_tx);
        drop(market_initial_clk_tx);
        drop(order_clk_tx);
        drop(order_initial_clk_tx);

        let replay = reconnect_replay();
        let [auth_bytes, market_bytes, order_bytes] = replay.as_slice() else {
            panic!("expected auth, market, and order replay messages");
        };

        let auth_str = std::str::from_utf8(auth_bytes).unwrap();
        let market_str = std::str::from_utf8(market_bytes).unwrap();
        let order_str = std::str::from_utf8(order_bytes).unwrap();

        assert!(auth_str.contains("\"op\":\"authentication\""));
        assert!(market_str.contains("\"op\":\"marketSubscription\""));
        // Both clk and initialClk must be injected into each resubscription
        assert!(market_str.contains("\"clk\":\"mcm-clk1\""));
        assert!(market_str.contains("\"initialClk\":\"mcm-iclk1\""));

        assert!(order_str.contains("\"op\":\"orderSubscription\""));
        assert!(order_str.contains("\"clk\":\"ocm-clk1\""));
        assert!(order_str.contains("\"initialClk\":\"ocm-iclk1\""));
    }

    #[rstest]
    #[tokio::test]
    async fn test_auth_update_after_replay_snapshot_requests_follow_up() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let (read_half, _write_half) = socket.into_split();
            let mut reader = BufReader::new(read_half);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            line.clear();
            reader.read_line(&mut line).await.unwrap();

            let (socket, _) = listener.accept().await.unwrap();
            let (read_half, mut write_half) = socket.into_split();
            let mut reader = BufReader::new(read_half);
            line.clear();
            reader.read_line(&mut line).await.unwrap();
            let auth: serde_json::Value = serde_json::from_str(&line).unwrap();
            assert_eq!(auth["session"], "replacement-1");
            line.clear();
            reader.read_line(&mut line).await.unwrap();
            write_half
                .write_all(b"{\"op\":\"connection\",\"connectionId\":\"replacement-1\"}\r\n")
                .await
                .unwrap();

            let (socket, _) = listener.accept().await.unwrap();
            let (read_half, _write_half) = socket.into_split();
            let mut reader = BufReader::new(read_half);
            line.clear();
            reader.read_line(&mut line).await.unwrap();
            let auth: serde_json::Value = serde_json::from_str(&line).unwrap();
            assert_eq!(auth["session"], "replacement-2");
            line.clear();
            reader.read_line(&mut line).await.unwrap();
            let subscription: serde_json::Value = serde_json::from_str(&line).unwrap();
            assert_eq!(subscription["op"], "orderSubscription");
        });

        let credential = BetfairCredential::new(
            "testuser".to_string(),
            "testpass".to_string(),
            "test-app-key".to_string(),
        );
        let config = BetfairStreamConfig {
            host: "127.0.0.1".to_string(),
            port,
            heartbeat_secs: None,
            heartbeat_timeout_secs: Some(60),
            reconnect_delay_initial_ms: 200,
            reconnect_delay_max_ms: 1_000,
            use_tls: false,
        };
        let client = BetfairStreamClient::connect(
            &credential,
            "initial".to_string(),
            Arc::new(|_| {}),
            config,
        )
        .await
        .unwrap();
        client.subscribe_orders(None, Some(5_000)).await.unwrap();

        client.update_auth("test-app-key", "replacement-1".to_string());
        assert!(client.request_reconnect());
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            while client
                .reconnect_auth
                .replay_generation
                .load(Ordering::SeqCst)
                < 1
            {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();

        client.update_auth("test-app-key", "replacement-2".to_string());
        assert!(!client.request_reconnect());
        assert_eq!(
            client
                .reconnect_auth
                .pending_generation
                .load(Ordering::SeqCst),
            2,
        );

        tokio::time::timeout(std::time::Duration::from_secs(5), server)
            .await
            .unwrap()
            .unwrap();
        client.close().await;
    }

    #[rstest]
    fn test_race_subscription_serialization() {
        let sub = RaceSubscription::new(42);
        let json = serde_json::to_string(&sub).unwrap();
        assert!(json.contains("\"op\":\"raceSubscription\""));
        assert!(json.contains("\"id\":42"));
    }

    #[rstest]
    fn test_cricket_subscription_serialization() {
        let sub = CricketSubscription::new(42);
        let json = serde_json::to_string(&sub).unwrap();
        assert!(json.contains("\"op\":\"cricketSubscription\""));
        assert!(json.contains("\"id\":42"));
    }

    #[rstest]
    #[case::race(false, "raceSubscription")]
    #[case::cricket(true, "cricketSubscription")]
    #[tokio::test]
    async fn test_auxiliary_stream_state_and_controller_reconnect(
        #[case] cricket: bool,
        #[case] subscription_op: &'static str,
    ) {
        use std::{sync::Mutex, time::Duration};

        use tokio::io::{AsyncBufReadExt, BufReader};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (initial_tx, initial_rx) = tokio::sync::oneshot::channel();
        let (replacement_tx, replacement_rx) = tokio::sync::oneshot::channel();
        let (done_tx, done_rx) = tokio::sync::oneshot::channel();

        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let (read_half, initial_write_half) = socket.into_split();
            let mut initial_reader = BufReader::new(read_half);
            let mut auth = String::new();
            let mut subscription = String::new();
            initial_reader.read_line(&mut auth).await.unwrap();
            initial_reader.read_line(&mut subscription).await.unwrap();
            let auth: serde_json::Value = serde_json::from_str(&auth).unwrap();
            let subscription: serde_json::Value = serde_json::from_str(&subscription).unwrap();
            assert_eq!(auth["session"], "test-session");
            assert_eq!(subscription["op"], subscription_op);
            initial_tx.send(()).unwrap();

            let (socket, _) = listener.accept().await.unwrap();
            let (read_half, replacement_write_half) = socket.into_split();
            let mut replacement_reader = BufReader::new(read_half);
            let mut replay_auth = String::new();
            let mut replay_subscription = String::new();
            replacement_reader
                .read_line(&mut replay_auth)
                .await
                .unwrap();
            replacement_reader
                .read_line(&mut replay_subscription)
                .await
                .unwrap();
            let replay_auth: serde_json::Value = serde_json::from_str(&replay_auth).unwrap();
            let replay_subscription: serde_json::Value =
                serde_json::from_str(&replay_subscription).unwrap();
            assert_eq!(replay_auth, auth);
            assert_eq!(replay_subscription, subscription);
            replacement_tx.send(()).unwrap();

            let _initial_connection = (initial_reader, initial_write_half);
            let _replacement_connection = (replacement_reader, replacement_write_half);
            let _ = done_rx.await;
        });

        let states = Arc::new(Mutex::new(Vec::new()));
        let states_sink = Arc::clone(&states);
        let state_sink = SocketStateSink::new(move |state| {
            states_sink.lock().unwrap().push(state);
        });
        let credential = BetfairCredential::new(
            "testuser".to_string(),
            "testpass".to_string(),
            "test-app-key".to_string(),
        );
        let config = BetfairStreamConfig {
            host: "127.0.0.1".to_string(),
            port,
            heartbeat_secs: Some(5),
            heartbeat_timeout_secs: Some(60),
            reconnect_delay_initial_ms: 100,
            reconnect_delay_max_ms: 500,
            use_tls: false,
        };
        let (fatal_tx, _fatal_rx) = tokio::sync::mpsc::unbounded_channel();
        let client = if cricket {
            BetfairRaceStreamClient::connect_cricket_decoded(
                &credential,
                "test-session".to_string(),
                Arc::new(|_| {}),
                config,
                fatal_tx,
                Some(state_sink),
            )
            .await
            .unwrap()
        } else {
            BetfairRaceStreamClient::connect_decoded(
                &credential,
                "test-session".to_string(),
                Arc::new(|_| {}),
                config,
                fatal_tx,
                Some(state_sink),
            )
            .await
            .unwrap()
        };

        initial_rx.await.unwrap();
        assert_eq!(
            client.request_reconnect_outcome(),
            ReconnectRequestOutcome::Accepted,
        );
        tokio::time::timeout(Duration::from_secs(5), replacement_rx)
            .await
            .unwrap()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            while states.lock().unwrap().len() < 3 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        client.close().await;

        assert_eq!(
            *states.lock().unwrap(),
            vec![
                SocketState::Connected,
                SocketState::Disconnected,
                SocketState::Connected,
            ],
        );
        assert_eq!(
            client.request_reconnect_outcome(),
            ReconnectRequestOutcome::Closed,
        );

        let _ = done_tx.send(());
        server.await.unwrap();
    }

    #[rstest]
    fn test_race_stream_reconnect_replays_auth_and_subscription() {
        let auth = Authentication::new("key".to_string(), "token".to_string());
        let auth_bytes = Bytes::from(serde_json::to_vec(&auth).unwrap());
        let race_sub = RaceSubscription::new(1);
        let race_sub_bytes = Bytes::from(serde_json::to_vec(&race_sub).unwrap());

        let auth_reconnect = auth_bytes;
        let sub_reconnect = race_sub_bytes;
        let reconnect_replay: SocketReconnectReplay = Arc::new(move || {
            let mut combined = Vec::with_capacity(auth_reconnect.len() + 2 + sub_reconnect.len());
            combined.extend_from_slice(&auth_reconnect);
            combined.extend_from_slice(b"\r\n");
            combined.extend_from_slice(&sub_reconnect);
            vec![Bytes::from(combined)]
        });

        let replay = reconnect_replay();
        let [bytes] = replay.as_slice() else {
            panic!("expected one combined replay message");
        };

        let text = std::str::from_utf8(bytes).unwrap();
        let (auth_part, sub_part) = text
            .split_once("\r\n")
            .expect("CRLF separator in combined message");

        assert!(auth_part.contains("\"op\":\"authentication\""));
        assert!(sub_part.contains("\"op\":\"raceSubscription\""));
    }

    #[rstest]
    fn test_race_stream_handler_fatal_status_sends_kill_signal() {
        let (race_fatal_tx, mut race_fatal_rx) = tokio::sync::mpsc::unbounded_channel::<()>();
        let inner_handler: TcpMessageHandler = Arc::new(|_data: &[u8]| {});

        let handler: TcpMessageHandler = Arc::new(move |data: &[u8]| {
            if let Ok(StreamMessage::Status(status)) = stream_decode(data)
                && let Some(ref code) = status.error_code
                && code.is_race_stream_fatal()
            {
                let _ = race_fatal_tx.send(());
                return;
            }
            inner_handler(data);
        });

        // Fatal: NOT_AUTHORIZED
        handler(
            br#"{"op":"status","statusCode":"503","errorCode":"NOT_AUTHORIZED","connectionClosed":true}"#,
        );
        assert!(
            race_fatal_rx.try_recv().is_ok(),
            "fatal error must send kill signal"
        );

        // Non-fatal: INVALID_CLOCK
        handler(
            br#"{"op":"status","statusCode":"503","errorCode":"INVALID_CLOCK","connectionClosed":true}"#,
        );
        assert!(
            race_fatal_rx.try_recv().is_err(),
            "non-fatal error must not send kill signal"
        );
    }
}
