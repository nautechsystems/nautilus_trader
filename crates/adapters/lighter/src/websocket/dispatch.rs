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

//! Per-client WebSocket dispatch state and pure translation helpers.
//!
//! Owns the cloid translation tables, the optimistic nonce manager, and the
//! cached `AccountState` snapshot that backs `query_account` replays. Pure
//! helpers (cloid translation, terminal-state eviction, tick conversions) live
//! alongside the state so the execution-client lifecycle code stays focused on
//! `ExecutionClient` trait wiring.

use std::{
    collections::VecDeque,
    hash::{BuildHasher, Hasher},
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use ahash::{AHashMap, AHashSet, RandomState};
use anyhow::Context;
use dashmap::{DashMap, DashSet};
use nautilus_core::{AtomicTime, MUTEX_POISONED, UnixNanos};
use nautilus_model::{
    enums::{OrderSide, OrderStatus, OrderType, TimeInForce},
    events::AccountState,
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TradeId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    orders::OrderAny,
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{Price, Quantity},
};
use rust_decimal::{Decimal, prelude::ToPrimitive};

use crate::{
    common::{
        credential::{Credential, scrub_auth},
        enums::{LighterOrderType, LighterTimeInForce},
        symbol::MarketRegistry,
    },
    http::{
        client::{LIGHTER_REST_PAGE_SIZE, LighterHttpClient},
        models::LighterOrder,
        query::{LighterAccountActiveOrdersQuery, LighterAccountInactiveOrdersQuery},
    },
    signing::{auth_token::build_auth_token_for, nonce::NonceManager},
    websocket::parse::parse_ws_order_status_report,
};

/// Default GTC / Day order lifetime when the caller did not specify an
/// explicit expire-time. Lighter rejects `OrderExpiry = -1` for GTC limits
/// with `21711 invalid expiry`, so the adapter substitutes a 28-day window
/// (matches the upstream venue convention).
pub(crate) const ORDER_EXPIRY_DEFAULT_GTC_MS: i64 = 28 * 24 * 60 * 60 * 1_000;

/// Venue minimum GTD lifetime plus one second for signing and transport.
pub(crate) const ORDER_EXPIRY_MIN_GTD_MS: i64 = 5 * 60 * 1_000 + 1_000;

/// Venue maximum GTD lifetime.
pub(crate) const ORDER_EXPIRY_MAX_GTD_MS: i64 = 30 * 24 * 60 * 60 * 1_000;

/// Sentinel used in `OrderInfo.order_expiry` for IOC orders, per the Lighter
/// Go signer's documented contract: `0` means "no expiry tracking, IOC
/// semantics".
pub(crate) const ORDER_EXPIRY_IOC: i64 = 0;

/// Defensive cap for authenticated reconciliation pagination.
pub(crate) const MAX_RECONCILIATION_PAGES: usize = 1_000;

/// Order identity context captured at submit time.
///
/// Used by the consumption loop to construct typed `OrderEventAny` variants
/// (`OrderAccepted`, `OrderFilled`, etc.) for tracked orders without a Cache
/// round-trip. Fields are immutable for the lifetime of an order.
#[derive(Debug, Clone)]
pub(crate) struct OrderIdentity {
    pub(crate) instrument_id: InstrumentId,
    pub(crate) strategy_id: StrategyId,
    pub(crate) order_side: OrderSide,
    pub(crate) order_type: OrderType,
    pub(crate) client_order_index: i64,
    venue_order_id: Arc<Mutex<Option<VenueOrderId>>>,
    accepted_emitted: Arc<AtomicBool>,
}

impl OrderIdentity {
    pub(crate) fn new(
        instrument_id: InstrumentId,
        strategy_id: StrategyId,
        order_side: OrderSide,
        order_type: OrderType,
        client_order_index: i64,
    ) -> Self {
        Self {
            instrument_id,
            strategy_id,
            order_side,
            order_type,
            client_order_index,
            venue_order_id: Arc::new(Mutex::new(None)),
            accepted_emitted: Arc::new(AtomicBool::new(false)),
        }
    }

    fn bind_venue_order_id(&self, venue_order_id: VenueOrderId) -> bool {
        let mut current = self.venue_order_id.lock().expect(MUTEX_POISONED);
        match *current {
            Some(existing) => existing == venue_order_id,
            None => {
                *current = Some(venue_order_id);
                true
            }
        }
    }

    fn matches_venue_order_id(&self, venue_order_id: VenueOrderId) -> bool {
        *self.venue_order_id.lock().expect(MUTEX_POISONED) == Some(venue_order_id)
    }

    fn accepted_was_emitted(&self) -> bool {
        self.accepted_emitted.load(Ordering::Acquire)
    }

    fn claim_accepted_emission(&self) -> bool {
        !self.accepted_emitted.swap(true, Ordering::AcqRel)
    }
}

/// In-flight sendTx awaiting a venue response.
///
/// Every signed sendTx (create, cancel, modify, update_leverage) enqueues an
/// entry keyed by the signed `tx_hash`, which the venue echoes in its ACK.
/// Responses that carry the hash remove their matching entry directly;
/// responses without one fall back to FIFO-head attribution, so the queue
/// order still matches the send order. The `kind` records whether the entry
/// has an originating Nautilus order that should receive a typed
/// order event on a venue rejection.
#[derive(Debug, Clone)]
pub(crate) struct PendingSendTx {
    pub(crate) kind: PendingSendTxKind,
    pub(crate) submitted_at: UnixNanos,
    pub(crate) nonce: i64,
    pub(crate) api_key_index: u8,
    pub(crate) tx_hash: String,
}

/// What kind of sendTx is sitting at this queue position, and whether the
/// consumption loop has anything cloid-bound to roll back on rejection.
#[derive(Debug, Clone)]
pub(crate) enum PendingSendTxKind {
    /// Create-order submit. On rejection: emit `OrderRejected`, evict the
    /// cloid_map slot, and forget the order identity.
    Create {
        order: Box<OrderAny>,
        client_order_index: i64,
    },
    /// Cancel-order submit. On rejection: emit `OrderCancelRejected`.
    Cancel {
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
    },
    /// Modify-order submit. On rejection: emit `OrderModifyRejected`.
    Modify {
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
    },
    /// Non-order sendTx, such as update-leverage. Tracked for FIFO alignment
    /// so the venue's ACK or rejection pops the correct head.
    Other,
}

/// Max probing attempts when [`WsDispatchState::register_cloid`] detects a
/// collision on the derived `client_order_index`. The 31-bit space makes a
/// collision improbable at session scale; bounding the probe ensures that a
/// degenerate seed never spins indefinitely while still leaving headroom.
pub(crate) const CLOID_INDEX_PROBE_LIMIT: usize = 16;

/// Maximum terminal orders and trade ids retained for reconnect replay.
const REPLAY_CACHE_CAPACITY: usize = 100_000;

#[derive(Debug, Clone)]
struct RetiredOrderIdentity {
    cloid: ClientOrderId,
    identity: OrderIdentity,
}

#[derive(Debug)]
pub(crate) struct RetiredOrderCache {
    inner: Mutex<RetiredOrderCacheInner>,
    capacity: usize,
}

#[derive(Debug)]
struct RetiredOrderCacheInner {
    by_index: AHashMap<i64, (RetiredOrderIdentity, u64)>,
    index_by_cloid: AHashMap<ClientOrderId, i64>,
    queue: VecDeque<(i64, u64)>,
    next_seq: u64,
}

impl RetiredOrderCache {
    fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(RetiredOrderCacheInner {
                by_index: AHashMap::with_capacity(capacity),
                index_by_cloid: AHashMap::with_capacity(capacity),
                queue: VecDeque::with_capacity(capacity),
                next_seq: 0,
            }),
            capacity,
        }
    }

    fn insert(&self, cloid: ClientOrderId, identity: OrderIdentity) {
        let index = identity.client_order_index;
        let mut inner = self.inner.lock().expect(MUTEX_POISONED);
        let seq = inner.next_seq;
        inner.next_seq = inner.next_seq.wrapping_add(1);
        inner.index_by_cloid.insert(cloid, index);
        inner
            .by_index
            .insert(index, (RetiredOrderIdentity { cloid, identity }, seq));
        inner.queue.push_back((index, seq));

        while inner.queue.len() > self.capacity
            && let Some((old_index, old_seq)) = inner.queue.pop_front()
        {
            if inner.by_index.get(&old_index).map(|(_, seq)| *seq) == Some(old_seq)
                && let Some((old, _)) = inner.by_index.remove(&old_index)
            {
                inner.index_by_cloid.remove(&old.cloid);
                log::warn!(
                    "Evicting retired Lighter order identity at replay-cache capacity: cloid={}, client_order_index={old_index}",
                    old.cloid,
                );
            }
        }
    }

    fn cloid_for_index(&self, index: i64) -> Option<ClientOrderId> {
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .by_index
            .get(&index)
            .map(|(retired, _)| retired.cloid)
    }

    fn identity_for_cloid(&self, cloid: &ClientOrderId) -> Option<OrderIdentity> {
        let inner = self.inner.lock().expect(MUTEX_POISONED);
        let index = inner.index_by_cloid.get(cloid)?;
        inner
            .by_index
            .get(index)
            .map(|(retired, _)| retired.identity.clone())
    }

    fn contains_index(&self, index: i64) -> bool {
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .by_index
            .contains_key(&index)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TradeDedupSource {
    Live,
    Reconciliation,
}

#[derive(Debug)]
pub(crate) struct TradeDedupCache {
    inner: Mutex<TradeDedupCacheInner>,
    capacity: usize,
}

#[derive(Debug)]
struct TradeDedupCacheInner {
    entries: AHashMap<TradeId, (TradeDedupSource, u64)>,
    queue: VecDeque<(TradeId, u64)>,
    next_seq: u64,
}

impl TradeDedupCache {
    fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(TradeDedupCacheInner {
                entries: AHashMap::with_capacity(capacity),
                queue: VecDeque::with_capacity(capacity),
                next_seq: 0,
            }),
            capacity,
        }
    }

    fn insert(&self, trade_id: TradeId, source: TradeDedupSource) -> Option<TradeDedupSource> {
        let mut inner = self.inner.lock().expect(MUTEX_POISONED);
        if let Some((existing, _)) = inner.entries.get(&trade_id) {
            return Some(*existing);
        }

        let seq = inner.next_seq;
        inner.next_seq = inner.next_seq.wrapping_add(1);
        inner.entries.insert(trade_id, (source, seq));
        inner.queue.push_back((trade_id, seq));
        while inner.queue.len() > self.capacity
            && let Some((old_trade_id, old_seq)) = inner.queue.pop_front()
        {
            if inner.entries.get(&old_trade_id).map(|(_, seq)| *seq) == Some(old_seq) {
                inner.entries.remove(&old_trade_id);
                log::warn!(
                    "Evicting Lighter trade id at replay-cache capacity: trade_id={old_trade_id}",
                );
            }
        }
        None
    }

    fn remove(&self, trade_id: &TradeId) {
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .entries
            .remove(trade_id);
    }

    #[cfg(test)]
    pub(crate) fn contains(&self, trade_id: &TradeId) -> bool {
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .entries
            .contains_key(trade_id)
    }
}

/// Per-client WebSocket dispatch state.
///
/// Threaded into the consumption loop and the order-action methods; cloned
/// freely thanks to interior `Arc` sharing on each field.
#[derive(Debug, Clone)]
pub(crate) struct WsDispatchState {
    /// Maps the venue-side `client_order_index` (i64) we derive at submit
    /// time back to the originating Nautilus [`ClientOrderId`]. The venue
    /// echoes the index on `account_*` order frames; the consumption loop
    /// uses this map to substitute the original cloid before forwarding.
    pub(crate) cloid_map: Arc<DashMap<i64, ClientOrderId>>,
    /// Terminal identity retained for late account-trade frames and reconnect
    /// replay. Active indices are never reused while their retired entry is
    /// inside this bounded process replay horizon.
    pub(crate) retired_orders: Arc<RetiredOrderCache>,
    /// Maps Nautilus [`ClientOrderId`] to the venue-assigned
    /// [`VenueOrderId`]. Populated by the consumption loop on the first
    /// `OrderStatusReport` and consumed by `cancel_order` / `modify_order`.
    pub(crate) venue_id_map: Arc<DashMap<ClientOrderId, VenueOrderId>>,
    /// Optimistic nonce allocator keyed by `(account_index, api_key_index)`.
    pub(crate) nonce_manager: Arc<NonceManager>,
    /// Last [`AccountState`] received from the WebSocket account stream,
    /// used to back `query_account` since the venue does not currently
    /// expose a REST account snapshot endpoint.
    pub(crate) last_account_state: Arc<Mutex<Option<AccountState>>>,
    /// Set of account-active `market_index` values surfaced by account
    /// streams or reconciliation reports. Mass-status reconciliation
    /// iterates over this set because Lighter's `accountActiveOrders` is
    /// per-market and the venue's REST quota would make a full-market
    /// fan-out prohibitively slow.
    pub(crate) active_markets: Arc<DashSet<i16>>,
    /// WS-driven position cache backing `generate_position_status_reports`
    /// (Lighter has no REST equivalent). `Mutex` not `DashMap` so a reader
    /// never lands between `replace_positions`' clear and repopulate.
    pub(crate) last_positions: Arc<Mutex<AHashMap<InstrumentId, PositionStatusReport>>>,
    /// Identity context for orders this client submitted. Keyed on the
    /// originating [`ClientOrderId`]; populated by the execution client at
    /// submit time, consumed by the consumption loop to decide whether an
    /// inbound venue frame should produce a typed `OrderEventAny` or fall
    /// back to a report for an externally-managed order.
    pub(crate) order_identities: Arc<DashMap<ClientOrderId, OrderIdentity>>,
    /// Trade ids already routed to `OrderFilled` / `FillReport`. The venue
    /// can re-emit the same `account_all_trades` payload across reconnects
    /// and HTTP reconciliation seeds this bounded source-aware cache so a
    /// later live replay cannot double-book a reported fill.
    pub(crate) seen_trade_ids: Arc<TradeDedupCache>,
    /// Cloids for which `OrderTriggered` has already been emitted. The
    /// venue keeps surfacing `trigger_status = Ready` on every subsequent
    /// `Open` frame for a conditional order once the trigger fires; the
    /// dedup keeps the engine from receiving phantom `Triggered` repeats.
    pub(crate) triggered_emitted: Arc<DashSet<ClientOrderId>>,
    /// Last known order-shape snapshot per tracked cloid. The consumption
    /// loop diffs incoming `Open` frames against this map to distinguish
    /// a real modify (qty / price / trigger changed) from a venue echo
    /// (snapshot, reconnect replay, partial-fill update). The snapshot
    /// is initialised on the first emitted `OrderAccepted` and refreshed
    /// on every emitted `OrderUpdated`.
    pub(crate) order_snapshots: Arc<DashMap<ClientOrderId, OrderShapeSnapshot>>,
    /// FIFO queue of submits awaiting a venue response. The consumption loop
    /// pops on every `SendTxAck` / `SendTxRejected` so it can attribute a
    /// rejection back to the originating order (sendTx error frames carry no
    /// correlation field). Single-account WS connection, so one global queue.
    pub(crate) pending_sendtx: Arc<Mutex<VecDeque<PendingSendTx>>>,
    /// First-frame readiness flags handed to the WS feed handler so
    /// `connect()` blocks until every account stream has produced a frame.
    /// Cloned cheaply since the inner state is shared via `Arc`.
    pub(crate) account_streams_ready: Arc<AccountStreamsReady>,
}

/// Compact snapshot of the mutable shape of a tracked order used by the
/// consumption loop to detect real modifies vs unchanged echoes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OrderShapeSnapshot {
    pub(crate) quantity: Quantity,
    pub(crate) price: Option<Price>,
    pub(crate) trigger_price: Option<Price>,
}

/// Fixed seed for the cloid hasher. Pinned at module load so the same
/// `ClientOrderId` always hashes to the same venue-side `client_order_index`
/// across operations, reconnects, AND fresh client instances after a
/// process restart. The seed must not change without coordinated cache
/// invalidation: if the engine restarts and recovers an order whose cloid
/// hashed to index N under the old seed, the new instance must derive the
/// same N to find that order via REST lookup.
static CLOID_HASHER: LazyLock<RandomState> = LazyLock::new(|| {
    RandomState::with_seeds(
        0x4C49_4748_5445_5253, // "LIGHTERS"
        0x434C_4F49_445F_4853, // "CLOID_HS"
        0x4E41_5554_494C_5553, // "NAUTILUS"
        0x5F4C_4F4F_4B5F_5550, // "_LOOK_UP"
    )
});

/// First-frame readiness flags for the four account-scoped WebSocket streams.
///
/// `connect()` blocks until every flag is set so strategies cannot race the
/// venue's initial frames. Lighter has no REST endpoint for account/position
/// state, so the WS frames are the only ground truth: returning before they
/// land risks `venue_id_map` and the position cache being empty on the first
/// strategy action.
#[derive(Debug)]
pub(crate) struct AccountStreamsReady {
    orders: AtomicBool,
    trades: AtomicBool,
    positions: AtomicBool,
    assets: AtomicBool,
    user_stats: AtomicBool,
    notify: tokio::sync::Notify,
}

impl AccountStreamsReady {
    pub(crate) fn new() -> Self {
        Self {
            orders: AtomicBool::new(false),
            trades: AtomicBool::new(false),
            positions: AtomicBool::new(false),
            assets: AtomicBool::new(false),
            user_stats: AtomicBool::new(false),
            notify: tokio::sync::Notify::new(),
        }
    }

    /// Reset every flag so a fresh connect attempt starts with no streams
    /// marked. Use before re-subscribing on a new WebSocket session.
    pub(crate) fn reset(&self) {
        self.orders.store(false, Ordering::Release);
        self.trades.store(false, Ordering::Release);
        self.positions.store(false, Ordering::Release);
        self.assets.store(false, Ordering::Release);
        self.user_stats.store(false, Ordering::Release);
    }

    /// Mark the `account_all_orders` stream as having delivered a frame.
    /// Idempotent: only the first call logs and notifies waiters.
    pub(crate) fn mark_orders(&self) {
        self.mark("orders", &self.orders);
    }

    /// Mark the `account_all_trades` stream as having delivered a frame.
    /// Idempotent: only the first call logs and notifies waiters.
    pub(crate) fn mark_trades(&self) {
        self.mark("trades", &self.trades);
    }

    /// Mark the `account_all_positions` stream as having delivered a frame.
    /// Idempotent: only the first call logs and notifies waiters.
    pub(crate) fn mark_positions(&self) {
        self.mark("positions", &self.positions);
    }

    /// Mark the `account_all_assets` stream as having delivered a frame.
    /// Idempotent: only the first call logs and notifies waiters.
    pub(crate) fn mark_assets(&self) {
        self.mark("assets", &self.assets);
    }

    /// Mark the `user_stats` stream as having delivered a frame.
    /// Idempotent: only the first call logs and notifies waiters.
    pub(crate) fn mark_user_stats(&self) {
        self.mark("user_stats", &self.user_stats);
    }

    fn mark(&self, name: &str, flag: &AtomicBool) {
        if !flag.swap(true, Ordering::AcqRel) {
            log::debug!("Lighter {name}: first frame received");
            self.notify.notify_waiters();
        }
    }

    /// Returns `true` once every account stream has delivered a frame.
    pub(crate) fn all_ready(&self) -> bool {
        self.orders.load(Ordering::Acquire)
            && self.trades.load(Ordering::Acquire)
            && self.positions.load(Ordering::Acquire)
            && self.assets.load(Ordering::Acquire)
            && self.user_stats.load(Ordering::Acquire)
    }

    fn pending(&self) -> Vec<&'static str> {
        let mut pending = Vec::new();
        if !self.orders.load(Ordering::Acquire) {
            pending.push("orders");
        }

        if !self.trades.load(Ordering::Acquire) {
            pending.push("trades");
        }

        if !self.positions.load(Ordering::Acquire) {
            pending.push("positions");
        }

        if !self.assets.load(Ordering::Acquire) {
            pending.push("assets");
        }

        if !self.user_stats.load(Ordering::Acquire) {
            pending.push("user_stats");
        }

        pending
    }

    /// Wait until every account stream has delivered a frame, or `timeout`
    /// elapses. Warns at 5s ticks with the list of pending streams; logs a
    /// success line with the total wait when all four have landed.
    pub(crate) async fn await_all(&self, timeout: Duration) -> anyhow::Result<()> {
        let start = Instant::now();
        let warn_interval = Duration::from_secs(5);
        let mut next_warn = start + warn_interval;

        loop {
            // Register interest before re-checking so a mark between the
            // `all_ready` test and the `.await` is still observed; with
            // `notify_waiters` the registration guarantees future notifies
            // reach us.
            let waiter = self.notify.notified();
            tokio::pin!(waiter);
            waiter.as_mut().enable();

            if self.all_ready() {
                log::debug!(
                    "All Lighter account streams ready in {:.1}s",
                    start.elapsed().as_secs_f64(),
                );
                return Ok(());
            }

            let now = Instant::now();
            let elapsed = now.duration_since(start);
            if elapsed >= timeout {
                anyhow::bail!(
                    "Timeout after {:.1}s awaiting Lighter account streams: pending={:?}",
                    timeout.as_secs_f64(),
                    self.pending(),
                );
            }

            // `elapsed < timeout` is established by the bail above, so the
            // subtraction never underflows. Use `saturating_sub` anyway to
            // satisfy `clippy::unchecked-time-subtraction`.
            let until_timeout = timeout.saturating_sub(elapsed);
            let until_warn = next_warn.saturating_duration_since(now);
            let wait = until_timeout.min(until_warn);

            let _ = tokio::time::timeout(wait, waiter).await;

            if !self.all_ready() && Instant::now() >= next_warn {
                log::warn!(
                    "Still awaiting Lighter account streams after {}s: pending={:?}",
                    start.elapsed().as_secs(),
                    self.pending(),
                );
                next_warn += warn_interval;
            }
        }
    }
}

impl WsDispatchState {
    /// Construct a fresh dispatch state with empty translation tables and a
    /// default-window nonce manager.
    pub(crate) fn new() -> Self {
        Self {
            cloid_map: Arc::new(DashMap::new()),
            retired_orders: Arc::new(RetiredOrderCache::new(REPLAY_CACHE_CAPACITY)),
            venue_id_map: Arc::new(DashMap::new()),
            nonce_manager: Arc::new(NonceManager::default()),
            last_account_state: Arc::new(Mutex::new(None)),
            active_markets: Arc::new(DashSet::new()),
            last_positions: Arc::new(Mutex::new(AHashMap::new())),
            order_identities: Arc::new(DashMap::new()),
            seen_trade_ids: Arc::new(TradeDedupCache::new(REPLAY_CACHE_CAPACITY)),
            triggered_emitted: Arc::new(DashSet::new()),
            order_snapshots: Arc::new(DashMap::new()),
            pending_sendtx: Arc::new(Mutex::new(VecDeque::new())),
            account_streams_ready: Arc::new(AccountStreamsReady::new()),
        }
    }

    /// Append a submit to the FIFO pending-sendTx queue.
    ///
    /// No stale-entry pruning: silently dropping entries would misattribute
    /// a late venue ACK to the next-head. In normal operation ACKs arrive in
    /// milliseconds; sustained queue growth indicates a stuck WS read loop.
    pub(crate) fn enqueue_pending_sendtx(&self, pending: PendingSendTx) {
        self.pending_sendtx
            .lock()
            .expect(MUTEX_POISONED)
            .push_back(pending);
    }

    /// Pop the oldest pending entry unconditionally.
    ///
    /// Tests and reconnect cleanup use this directly. Live hashless response
    /// attribution must use [`Self::pop_pending_sendtx_if_only`].
    #[cfg(test)]
    pub(crate) fn pop_pending_sendtx_head(&self) -> Option<PendingSendTx> {
        self.pending_sendtx
            .lock()
            .expect(MUTEX_POISONED)
            .pop_front()
    }

    /// Pop a pending entry only when it is the sole possible match.
    ///
    /// Lighter does not always echo `tx_hash`. With two or more transactions
    /// in flight, FIFO attribution is unsafe because responses may arrive out
    /// of order.
    pub(crate) fn pop_pending_sendtx_if_only(&self) -> Option<PendingSendTx> {
        let mut queue = self.pending_sendtx.lock().expect(MUTEX_POISONED);
        if queue.len() != 1 {
            return None;
        }
        queue.pop_front()
    }

    /// Pop the sole pending entry only if its `submitted_at` is within
    /// `max_age_ms` of `now`.
    pub(crate) fn pop_pending_sendtx_if_only_within(
        &self,
        now: UnixNanos,
        max_age_ms: u64,
    ) -> Option<PendingSendTx> {
        let cutoff_ns = now.as_u64().saturating_sub(max_age_ms * 1_000_000);
        let mut queue = self.pending_sendtx.lock().expect(MUTEX_POISONED);
        if queue.len() != 1 {
            return None;
        }

        match queue.front() {
            Some(pending) if pending.submitted_at.as_u64() >= cutoff_ns => queue.pop_front(),
            _ => None,
        }
    }

    /// Remove a pending entry by nonce. Used by the spawn-failure path when
    /// `send_tx` errors locally before the venue ever sees the message; the
    /// nonce is unique per submit and reachable from every dispatch path.
    pub(crate) fn remove_pending_sendtx_by_nonce(&self, nonce: i64) -> Option<PendingSendTx> {
        let mut q = self.pending_sendtx.lock().expect(MUTEX_POISONED);
        let pos = q.iter().position(|p| p.nonce == nonce)?;
        q.remove(pos)
    }

    /// Remove a pending entry by its signed transaction hash
    /// (case-insensitive hex comparison, optional `0x` prefix tolerated).
    /// The venue echoes the hash the client computed at signing time, so a
    /// match is exact attribution regardless of queue position.
    pub(crate) fn remove_pending_sendtx_by_hash(&self, tx_hash: &str) -> Option<PendingSendTx> {
        let tx_hash = tx_hash
            .strip_prefix("0x")
            .or_else(|| tx_hash.strip_prefix("0X"))
            .unwrap_or(tx_hash);
        let mut q = self.pending_sendtx.lock().expect(MUTEX_POISONED);
        let pos = q
            .iter()
            .position(|p| p.tx_hash.eq_ignore_ascii_case(tx_hash))?;
        q.remove(pos)
    }

    /// Drain every pending entry. Used on reconnect: responses to txs sent
    /// on the previous connection are lost, so retained entries would only
    /// misattribute responses arriving on the new connection.
    pub(crate) fn drain_pending_sendtx(&self) -> Vec<PendingSendTx> {
        self.pending_sendtx
            .lock()
            .expect(MUTEX_POISONED)
            .drain(..)
            .collect()
    }

    /// Returns the current pending-sendTx queue length. Test-only helper.
    #[cfg(test)]
    pub(crate) fn pending_sendtx_len(&self) -> usize {
        self.pending_sendtx.lock().expect(MUTEX_POISONED).len()
    }

    /// First-time check for an `OrderTriggered` event for `cloid`. Returns
    /// `true` if `cloid` has not yet emitted `Triggered` and inserts it.
    pub(crate) fn mark_triggered_emitted(&self, cloid: ClientOrderId) -> bool {
        self.triggered_emitted.insert(cloid)
    }

    /// Returns `true` if `OrderTriggered` has already fired for `cloid`.
    pub(crate) fn triggered_was_emitted(&self, cloid: &ClientOrderId) -> bool {
        self.triggered_emitted.contains(cloid)
    }

    /// Read the last-known order shape for `cloid`, if any.
    pub(crate) fn snapshot_for(&self, cloid: &ClientOrderId) -> Option<OrderShapeSnapshot> {
        self.order_snapshots.get(cloid).map(|e| e.value().clone())
    }

    /// Replace the stored order-shape snapshot for `cloid`.
    pub(crate) fn store_snapshot(&self, cloid: ClientOrderId, snapshot: OrderShapeSnapshot) {
        self.order_snapshots.insert(cloid, snapshot);
    }

    /// Register identity context for an order the client just dispatched.
    /// The consumption loop reads this to decide whether to emit typed events.
    pub(crate) fn register_order_identity(&self, cloid: ClientOrderId, identity: OrderIdentity) {
        self.order_identities.insert(cloid, identity);
    }

    /// Drop the identity entry for `cloid` after a terminal event or
    /// failed dispatch.
    pub(crate) fn forget_order_identity(&self, cloid: &ClientOrderId) {
        self.order_identities.remove(cloid);
        self.triggered_emitted.remove(cloid);
        self.order_snapshots.remove(cloid);
    }

    /// Move a terminal order from active maps into the bounded replay cache.
    pub(crate) fn retire_order_identity(&self, cloid: &ClientOrderId) {
        let Some((_, identity)) = self.order_identities.remove(cloid) else {
            return;
        };
        self.cloid_map.remove(&identity.client_order_index);
        self.retired_orders.insert(*cloid, identity);
        self.triggered_emitted.remove(cloid);
        self.order_snapshots.remove(cloid);
    }

    /// Resolve active first, then terminal replay identity.
    pub(crate) fn order_identity(&self, cloid: &ClientOrderId) -> Option<OrderIdentity> {
        self.order_identities
            .get(cloid)
            .map(|entry| entry.value().clone())
            .or_else(|| self.retired_orders.identity_for_cloid(cloid))
    }

    /// Returns `true` if an `OrderAccepted` has already been emitted for
    /// `cloid` on the live path. Drives the modify-as-restate branch and
    /// the `ensure_accepted_emitted` synthesis.
    pub(crate) fn accepted_was_emitted(&self, cloid: &ClientOrderId) -> bool {
        self.order_identity(cloid)
            .is_some_and(|identity| identity.accepted_was_emitted())
    }

    /// Atomically claim the right to emit `OrderAccepted` for `cloid`.
    pub(crate) fn claim_accepted_emission(&self, cloid: &ClientOrderId) -> bool {
        self.order_identity(cloid)
            .is_some_and(|identity| identity.claim_accepted_emission())
    }

    /// Record that an `OrderAccepted` has been emitted for `cloid`.
    pub(crate) fn mark_accepted_emitted(&self, cloid: ClientOrderId) {
        let _ = self.claim_accepted_emission(&cloid);
    }

    /// Seed the live accepted marker from a tracked order report.
    ///
    /// Reconciliation can turn any non-rejected tracked report into
    /// `OrderAccepted` for a locally submitted order before the typed
    /// WebSocket path receives a cancel, fill, or open frame. Marking here
    /// keeps that later typed path from synthesising a second `OrderAccepted`.
    pub(crate) fn seed_accepted_from_report(&self, report: &OrderStatusReport) {
        if !matches!(
            report.order_status,
            OrderStatus::Submitted
                | OrderStatus::PendingUpdate
                | OrderStatus::PendingCancel
                | OrderStatus::Accepted
                | OrderStatus::Triggered
                | OrderStatus::PartiallyFilled
                | OrderStatus::Filled
                | OrderStatus::Canceled
                | OrderStatus::Expired
        ) {
            return;
        }

        let Some(cloid) = report.client_order_id else {
            return;
        };

        self.mark_accepted_emitted(cloid);
    }

    /// First-time check for a trade id: returns `true` if `trade_id` is new
    /// and inserts it, returns `false` if it was already routed.
    pub(crate) fn mark_trade_seen(&self, trade_id: TradeId) -> bool {
        self.seen_trade_ids
            .insert(trade_id, TradeDedupSource::Live)
            .is_none()
    }

    /// Record a reconciliation fill and return its prior delivery source.
    pub(crate) fn mark_trade_reconciled(&self, trade_id: TradeId) -> Option<TradeDedupSource> {
        self.seen_trade_ids
            .insert(trade_id, TradeDedupSource::Reconciliation)
    }

    /// Roll back a [`Self::mark_trade_seen`] marker when the trade could not be
    /// parsed, so a later replay of the same `trade_id` is retried instead of
    /// being permanently suppressed by the dedup set.
    pub(crate) fn unmark_trade_seen(&self, trade_id: &TradeId) {
        self.seen_trade_ids.remove(trade_id);
    }

    /// Record a market_index as having reported account activity.
    pub(crate) fn note_active_market(&self, market_index: i16) {
        self.active_markets.insert(market_index);
    }

    /// Snapshot account-active markets for fan-out at reconciliation time.
    pub(crate) fn active_markets_snapshot(&self) -> Vec<i16> {
        let mut markets: Vec<i16> = self.active_markets.iter().map(|m| *m).collect();
        markets.sort_unstable();
        markets
    }

    /// Hash a Nautilus [`ClientOrderId`] into a stable positive `i64` for use
    /// as the venue's `client_order_index`. The high bit is masked off so
    /// every derived value passes Lighter's `client_order_index >= 0` check.
    pub(crate) fn derive_client_order_index(&self, cloid: &ClientOrderId) -> i64 {
        derive_client_order_index_static(cloid)
    }

    /// Register a `(client_order_index, ClientOrderId)` mapping ahead of
    /// dispatch so the venue's later echo can be translated.
    ///
    /// When the derived `client_order_index` collides with an existing
    /// in-flight registration for a *different* cloid the call probes
    /// forward by 1 (wrapping inside the 31-bit venue-safe window) up to
    /// [`CLOID_INDEX_PROBE_LIMIT`] times to find a free slot. The chosen
    /// index is returned so the caller can use it as the venue-side
    /// `client_order_index`. A re-registration of the same cloid against
    /// its already-assigned index is a no-op and returns `index`.
    ///
    /// The 31-bit space is large enough that collisions are improbable at
    /// session scale; the probe protects against rare collisions without
    /// silently re-routing a later order's fill to a prior cloid.
    ///
    /// # Errors
    ///
    /// Returns an error when every bounded probe candidate is occupied. No
    /// existing mapping is changed on failure.
    pub(crate) fn register_cloid(&self, index: i64, cloid: ClientOrderId) -> anyhow::Result<i64> {
        let mut candidate = index;
        for attempt in 0..=CLOID_INDEX_PROBE_LIMIT {
            match self.cloid_map.entry(candidate) {
                dashmap::mapref::entry::Entry::Vacant(entry) => {
                    if self.retired_orders.contains_index(candidate) {
                        candidate = next_probe_index(candidate);
                        continue;
                    }
                    entry.insert(cloid);

                    if attempt > 0 {
                        log::warn!(
                            "Lighter client_order_index collision at {index}: \
                             cloid {cloid} re-derived to {candidate} after {attempt} probe(s)",
                        );
                    }
                    return Ok(candidate);
                }
                dashmap::mapref::entry::Entry::Occupied(entry) => {
                    if *entry.get() == cloid {
                        return Ok(candidate);
                    }
                    candidate = next_probe_index(candidate);
                }
            }
        }
        anyhow::bail!(
            "Lighter client_order_index probe exhausted after {} attempts for cloid {cloid}",
            CLOID_INDEX_PROBE_LIMIT + 1,
        )
    }

    /// Drop a cloid registration (called from the spawn's error branch when
    /// the tx never reaches the wire).
    pub(crate) fn forget_cloid(&self, index: i64) {
        self.cloid_map.remove(&index);
    }

    /// Resolve a venue client-order index across active and replay caches.
    pub(crate) fn resolve_client_order_index(&self, index: i64) -> Option<ClientOrderId> {
        self.cloid_map
            .get(&index)
            .map(|entry| *entry.value())
            .or_else(|| self.retired_orders.cloid_for_index(index))
    }

    /// Resolve and bind a live venue client id across active and replay caches.
    pub(crate) fn resolve_live_cloid(
        &self,
        raw_client_id: &str,
        venue_order_id: VenueOrderId,
    ) -> Option<ClientOrderId> {
        if raw_client_id.is_empty() || raw_client_id == "0" {
            return None;
        }
        let index = raw_client_id.parse::<i64>().ok()?;
        let cloid = self.resolve_client_order_index(index)?;
        self.order_identity(&cloid)?
            .bind_venue_order_id(venue_order_id)
            .then_some(cloid)
    }

    fn resolve_client_order_index_for_venue(
        &self,
        index: i64,
        venue_order_id: VenueOrderId,
    ) -> Option<ClientOrderId> {
        let cloid = self.resolve_client_order_index(index)?;
        self.order_identity(&cloid)?
            .matches_venue_order_id(venue_order_id)
            .then_some(cloid)
    }

    /// Substitute a numeric venue cloid only when its venue order is bound.
    pub(crate) fn translate_order_cloid(&self, mut report: OrderStatusReport) -> OrderStatusReport {
        if let Some(cloid) = report.client_order_id
            && let Ok(index) = cloid.as_str().parse::<i64>()
        {
            let resolved = self
                .resolve_client_order_index_for_venue(index, report.venue_order_id)
                .unwrap_or_else(|| ClientOrderId::new(report.venue_order_id.as_str()));
            report = report.with_client_order_id(resolved);
        }
        report
    }

    /// Substitute a numeric venue cloid only when its venue order is bound.
    pub(crate) fn translate_fill_cloid(&self, mut report: FillReport) -> FillReport {
        if let Some(cloid) = report.client_order_id
            && let Ok(index) = cloid.as_str().parse::<i64>()
        {
            report.client_order_id = Some(
                self.resolve_client_order_index_for_venue(index, report.venue_order_id)
                    .unwrap_or_else(|| ClientOrderId::new(report.venue_order_id.as_str())),
            );
        }
        report
    }

    /// Return the actual collision-probed index used for `cloid`.
    pub(crate) fn client_order_index(&self, cloid: &ClientOrderId) -> Option<i64> {
        self.order_identity(cloid)
            .map(|identity| identity.client_order_index)
    }

    /// Look up the venue-assigned [`VenueOrderId`] for a Nautilus cloid.
    pub(crate) fn lookup_venue_order_id(&self, cloid: &ClientOrderId) -> Option<VenueOrderId> {
        self.venue_id_map.get(cloid).map(|e| *e.value())
    }

    /// Cache the most recent [`AccountState`] from the WS feed so
    /// `query_account` can serve a snapshot synchronously.
    pub(crate) fn cache_account_state(&self, state: AccountState) {
        let mut guard = self.last_account_state.lock().expect(MUTEX_POISONED);
        *guard = Some(state);
    }

    /// Return a clone of the cached [`AccountState`], if any.
    pub(crate) fn snapshot_account_state(&self) -> Option<AccountState> {
        self.last_account_state
            .lock()
            .expect(MUTEX_POISONED)
            .clone()
    }

    /// Drop the cached `AccountState` snapshot. Used at connect time so a
    /// stale prior-session snapshot cannot satisfy the strict-await gate
    /// when an initial venue frame fails to parse or omits balances.
    pub(crate) fn clear_account_state_cache(&self) {
        let mut guard = self.last_account_state.lock().expect(MUTEX_POISONED);
        *guard = None;
    }

    /// Drop the cached position snapshot without emitting flat reports.
    /// Used at connect time so a stale prior-session entry cannot leak past
    /// the strict-await gate before the next `account_all_positions` frame
    /// replaces the cache.
    pub(crate) fn clear_position_cache(&self) {
        self.last_positions.lock().expect(MUTEX_POISONED).clear();
    }

    /// Replace the cache from a complete `account_all_positions` snapshot
    /// and return the instrument ids that were present before but absent
    /// after. The caller is expected to emit a flat
    /// [`PositionStatusReport`] for each removed instrument; otherwise the
    /// execution engine won't observe externally-closed positions.
    /// Instruments absent from `snapshot` are evicted; an empty input
    /// clears the cache entirely.
    pub(crate) fn replace_positions(&self, snapshot: &[PositionStatusReport]) -> Vec<InstrumentId> {
        self.replace_positions_except(snapshot, &[])
    }

    /// Replace the cache from a snapshot while retaining instruments whose
    /// venue rows were skipped and therefore cannot be treated as closed.
    pub(crate) fn replace_positions_except(
        &self,
        snapshot: &[PositionStatusReport],
        retained: &[InstrumentId],
    ) -> Vec<InstrumentId> {
        let mut guard = self.last_positions.lock().expect(MUTEX_POISONED);
        let new_ids: ahash::AHashSet<InstrumentId> =
            snapshot.iter().map(|r| r.instrument_id).collect();
        let retained_ids: ahash::AHashSet<InstrumentId> = retained.iter().copied().collect();
        let removed: Vec<InstrumentId> = guard
            .keys()
            .filter(|id| !new_ids.contains(id) && !retained_ids.contains(id))
            .copied()
            .collect();
        guard.retain(|id, _| retained_ids.contains(id));
        for report in snapshot {
            guard.insert(report.instrument_id, report.clone());
        }
        removed
    }

    /// Snapshot the cached positions, optionally filtered by instrument.
    pub(crate) fn snapshot_positions(
        &self,
        instrument_id: Option<InstrumentId>,
    ) -> Vec<PositionStatusReport> {
        let guard = self.last_positions.lock().expect(MUTEX_POISONED);
        match instrument_id {
            Some(id) => guard.get(&id).cloned().map(|r| vec![r]).unwrap_or_default(),
            None => guard.values().cloned().collect(),
        }
    }
}

/// Standalone derivation so the fixed-seed contract is testable without
/// constructing a full dispatch state, and so the seed lives in one place.
pub(crate) fn derive_client_order_index_static(cloid: &ClientOrderId) -> i64 {
    let mut hasher = CLOID_HASHER.build_hasher();
    hasher.write(cloid.as_str().as_bytes());
    let h = hasher.finish();
    // Mask to 31 positive bits (max ~2.1B). Lighter rejects larger values
    // with `21727 invalid client order index`; the venue's accepted range
    // is not documented but observed empirically. Using a smaller window
    // also keeps collision risk negligible at session scale.
    i64::from(h as u32 & 0x7FFF_FFFF)
}

/// Linear probe: advance the candidate index by 1, wrapping inside the
/// 31-bit venue-safe window. Used by [`WsDispatchState::register_cloid`]
/// when the derived index collides with another in-flight cloid.
fn next_probe_index(candidate: i64) -> i64 {
    let next = candidate.wrapping_add(1);
    if (0..=0x7FFF_FFFF).contains(&next) {
        next
    } else {
        0
    }
}

/// Drops the `ClientOrderId → VenueOrderId` mapping for an order that has
/// reached a terminal status, since cancel/modify can no longer act on it.
///
/// The dispatch path separately moves terminal identity into the bounded
/// replay cache. That cache keeps the probed client index available when a
/// terminal status arrives before its trailing `account_all_trades` frame.
pub(crate) fn evict_terminal_mappings(
    report: &OrderStatusReport,
    venue_id_map: &Arc<DashMap<ClientOrderId, VenueOrderId>>,
) {
    if let Some(cloid) = &report.client_order_id {
        venue_id_map.remove(cloid);
    }
}

/// Process-global instrument cache used by the HTTP report-gen path.
///
/// Avoids threading the live engine cache through every helper; populated by
/// the data and execution clients on bootstrap.
pub(crate) static LIGHTER_INSTRUMENT_CACHE: LazyLock<DashMap<InstrumentId, InstrumentAny>> =
    LazyLock::new(DashMap::new);

/// Populate [`LIGHTER_INSTRUMENT_CACHE`] for downstream report parsers.
pub(crate) fn cache_instruments_for_reports(instruments: &[InstrumentAny]) {
    for instrument in instruments {
        LIGHTER_INSTRUMENT_CACHE.insert(instrument.id(), instrument.clone());
    }
}

/// Convert a Lighter HTTP `LighterOrder` into a Nautilus
/// [`OrderStatusReport`], reusing the WS-side parser once the instrument has
/// been resolved out of the process-global cache.
///
/// Translates the venue's numeric `client_order_index` echo back to the
/// originating Nautilus [`ClientOrderId`] when available, so HTTP-driven
/// reconciliation paths don't surface our own orders as external.
pub(crate) fn parse_http_order_to_report(
    order: &LighterOrder,
    registry: &Arc<MarketRegistry>,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> Option<OrderStatusReport> {
    let instrument_id = registry.instrument_id(order.market_index)?;
    let instrument = match LIGHTER_INSTRUMENT_CACHE.get(&instrument_id) {
        Some(inst) => inst,
        None => {
            log::debug!("parse_http_order_to_report: instrument {instrument_id} not in cache");
            return None;
        }
    };

    match parse_ws_order_status_report(order, &instrument, account_id, ts_init) {
        Ok(report) => Some(report),
        Err(e) => {
            log::warn!(
                "parse_http_order_to_report: parse failed for order_index={}: {e}",
                order.order_index,
            );
            None
        }
    }
}

/// Look up a single order via the active and inactive HTTP endpoints, returning
/// the corresponding [`OrderStatusReport`] if found.
///
/// Resolution order: explicit `venue_order_id` > cached `venue_id_map` >
/// derived `client_order_index` from `dispatch.derive_client_order_index`.
/// The third path is active-order only. It makes `query_order` work between
/// submission and the venue's first `account_*` ack, while avoiding ambiguous
/// terminal history where Lighter can reuse client indexes.
#[expect(
    clippy::too_many_arguments,
    reason = "translation helper that threads context to the parser without a wrapper struct"
)]
pub(crate) async fn lookup_order_status_report(
    http_client: &LighterHttpClient,
    registry: &Arc<MarketRegistry>,
    credential: &Credential,
    account_id: AccountId,
    instrument_id: Option<InstrumentId>,
    client_order_id: Option<&ClientOrderId>,
    venue_order_id: Option<&VenueOrderId>,
    dispatch: &WsDispatchState,
    clock: &'static AtomicTime,
) -> anyhow::Result<Option<OrderStatusReport>> {
    let instrument_id = instrument_id.ok_or_else(|| {
        anyhow::anyhow!("Lighter order lookup requires an instrument_id (per-market REST query)")
    })?;
    let market_index = registry
        .market_index(&instrument_id)
        .ok_or_else(|| anyhow::anyhow!("no Lighter market_index for instrument {instrument_id}"))?;

    // Try, in order: explicit voi, cached voi, derived client_order_index.
    let target_venue_index: Option<i64> = venue_order_id
        .and_then(|voi| voi.as_str().parse::<i64>().ok())
        .or_else(|| {
            client_order_id
                .and_then(|cloid| dispatch.lookup_venue_order_id(cloid))
                .and_then(|voi| voi.as_str().parse::<i64>().ok())
        });
    let target_client_index: Option<i64> = client_order_id.map(|cloid| {
        dispatch
            .client_order_index(cloid)
            .unwrap_or_else(|| dispatch.derive_client_order_index(cloid))
    });

    let matches_order = |o: &LighterOrder| {
        order_matches_lookup(
            o.order_index,
            o.client_order_index,
            target_venue_index,
            target_client_index,
        )
    };

    let auth = mint_auth_token(credential)?;
    let active = http_client
        .get_account_active_orders(&LighterAccountActiveOrdersQuery {
            authorization: None,
            auth: Some(auth.clone()),
            account_index: credential.account_index(),
            market_id: market_index,
        })
        .await
        .context("failed to fetch Lighter active orders")?;

    let ts_init = clock.get_time_ns();
    let supplied_cloid = client_order_id.copied();

    let finalize = |order: &LighterOrder| -> Option<OrderStatusReport> {
        let report = parse_http_order_to_report(order, registry, account_id, ts_init)?;
        let mut report = dispatch.translate_order_cloid(report);
        // Substitute the caller-supplied cloid whenever it positively
        // identifies this order: when the order's
        // `client_order_index` equals the deterministic derivation from
        // `supplied_cloid`. This covers two cases the cloid_map cannot
        // serve after a fresh client instance:
        //   1. The match came via `client_order_index`.
        //   2. The match came via venue order id, but the caller also
        //      supplied the matching cloid.
        // Substituting on the derivation match (rather than which path
        // matched first) avoids leaving the venue numeric cloid on the
        // report whenever the supplied cloid is the right one.
        if let Some(cloid) = supplied_cloid
            && let Some(client_index) = target_client_index
            && order.client_order_index == client_index
            && report.client_order_id != Some(cloid)
        {
            report = report.with_client_order_id(cloid);
        }
        Some(report)
    };

    let mut active_matches = active.orders.iter().filter(|order| matches_order(order));
    let active_match = active_matches.next();

    if target_venue_index.is_none() {
        anyhow::ensure!(
            active_matches.next().is_none(),
            "ambiguous Lighter active-order lookup for client_order_index {}",
            target_client_index.unwrap_or_default(),
        );
    }

    if let Some(order) = active_match
        && let Some(report) = finalize(order)
    {
        return Ok(Some(report));
    }

    if target_venue_index.is_none() {
        return Ok(None);
    }

    // Fall back to inactive orders (filled / canceled). Pagination is followed
    // because a single market can hold more than 200 historical inactive
    // orders for a long-running account.
    let mut cursor: Option<String> = None;
    let mut seen_cursors = AHashSet::new();
    let mut pages = 0_usize;

    loop {
        pages += 1;
        anyhow::ensure!(
            pages <= MAX_RECONCILIATION_PAGES,
            "Lighter inactive-order lookup exceeded {MAX_RECONCILIATION_PAGES} pages",
        );
        let inactive = http_client
            .get_account_inactive_orders(&LighterAccountInactiveOrdersQuery {
                authorization: None,
                auth: Some(auth.clone()),
                account_index: credential.account_index(),
                market_id: Some(market_index),
                ask_filter: None,
                between_timestamps: None,
                cursor: cursor.clone(),
                limit: LIGHTER_REST_PAGE_SIZE,
            })
            .await
            .context("failed to fetch Lighter inactive orders")?;

        for order in &inactive.orders {
            if matches_order(order)
                && let Some(report) = finalize(order)
            {
                return Ok(Some(report));
            }
        }

        match inactive.next_cursor {
            Some(next) if !next.is_empty() => {
                anyhow::ensure!(
                    seen_cursors.insert(next.clone()),
                    "Lighter inactive-order lookup repeated cursor `{next}`",
                );
                cursor = Some(next);
            }
            _ => break,
        }
    }

    Ok(None)
}

fn order_matches_lookup(
    order_index: i64,
    client_order_index: i64,
    target_venue_index: Option<i64>,
    target_client_index: Option<i64>,
) -> bool {
    match target_venue_index {
        Some(target) => order_index == target,
        None => target_client_index == Some(client_order_index),
    }
}

fn mint_auth_token(credential: &Credential) -> anyhow::Result<String> {
    build_auth_token_for(credential).context("failed to mint Lighter auth token for order lookup")
}

/// Translate a Nautilus [`TimeInForce`] into the venue's `LighterTimeInForce`.
///
/// For limit-style orders, `Day` is mapped through `GoodTillTime` because
/// the venue has no `DAY` concept; the engine handles end-of-day expiry on
/// the client side.
///
/// `post_only` overrides the TIF mapping when set: Lighter exposes a
/// dedicated `PostOnly` TIF (slot 2) which the venue treats as a maker-only
/// order, so post-only takes precedence over the order's nominal TIF.
///
/// Plain market orders use IOC on the wire. Conditional market orders
/// (`STOP_MARKET` / `MARKET_IF_TOUCHED`) also use IOC as the post-trigger
/// execution instruction, but their trigger lifetime is controlled by a
/// positive `OrderExpiry`.
///
/// FOK ("fill or kill") is rejected because Lighter has no native
/// fill-or-kill primitive: routing FOK as IOC would let a partial fill
/// satisfy the request, violating the FOK guarantee.
pub(crate) fn nautilus_to_lighter_tif(
    order_type: OrderType,
    tif: TimeInForce,
    post_only: bool,
) -> anyhow::Result<LighterTimeInForce> {
    if post_only {
        return Ok(LighterTimeInForce::PostOnly);
    }

    if order_type == OrderType::Market {
        return match tif {
            TimeInForce::Gtc | TimeInForce::Ioc => Ok(LighterTimeInForce::ImmediateOrCancel),
            TimeInForce::Fok => anyhow::bail!(
                "Lighter has no fill-or-kill TIF; reject FOK at the strategy or use IOC explicitly",
            ),
            other => anyhow::bail!(
                "Lighter market orders support only TimeInForce::Gtc or TimeInForce::Ioc, was TimeInForce::{other:?}",
            ),
        };
    }

    if is_conditional_market_order(order_type) {
        return match tif {
            TimeInForce::Gtc | TimeInForce::Day | TimeInForce::Gtd => {
                Ok(LighterTimeInForce::ImmediateOrCancel)
            }
            TimeInForce::Ioc => anyhow::bail!(
                "Lighter conditional market orders require a positive expiry; Nautilus IOC cannot be represented because the venue uses IOC for post-trigger execution",
            ),
            TimeInForce::Fok => anyhow::bail!(
                "Lighter has no fill-or-kill TIF; reject FOK at the strategy or use IOC explicitly",
            ),
            other => anyhow::bail!(
                "Lighter conditional market orders do not support TimeInForce::{other:?}",
            ),
        };
    }

    match tif {
        TimeInForce::Ioc => Ok(LighterTimeInForce::ImmediateOrCancel),
        TimeInForce::Fok => anyhow::bail!(
            "Lighter has no fill-or-kill TIF; reject FOK at the strategy or use IOC explicitly",
        ),
        TimeInForce::Gtc | TimeInForce::Day | TimeInForce::Gtd => {
            Ok(LighterTimeInForce::GoodTillTime)
        }
        other => anyhow::bail!("Lighter does not support TimeInForce::{other:?}"),
    }
}

/// Translate a Nautilus [`OrderType`] into the venue's [`LighterOrderType`]
/// discriminant for use in `CreateOrder` tx bodies.
pub(crate) fn nautilus_to_lighter_order_type(
    order_type: OrderType,
) -> anyhow::Result<LighterOrderType> {
    LighterOrderType::try_from(order_type)
        .map_err(|e| anyhow::anyhow!("unsupported Nautilus order type for Lighter: {e}"))
}

/// Compute the venue-side `order_expiry` (millis) for a Nautilus order.
///
/// - `MARKET`: `ORDER_EXPIRY_IOC` (`0`) because it has no resting trigger
///   lifetime.
/// - Conditional orders: positive expiry from `GTD` or the default GTC window.
///   Lighter uses `TimeInForce` as the post-trigger execution instruction,
///   while `OrderExpiry` controls how long the trigger can rest.
/// - `Gtd` with an explicit expire_time: the millisecond timestamp, provided
///   it is within the venue's 5-minute to 30-day lifetime range.
/// - `Ioc` / `Fok`: `ORDER_EXPIRY_IOC` (`0`): Lighter requires this exact
///   value for IOC semantics; any other value is rejected by the sequencer.
/// - `Gtc` / `Day` / `Gtd` without expiry: `now_ms + ORDER_EXPIRY_DEFAULT_GTC_MS`.
///   The venue rejects `-1` for these TIFs with `21711 invalid expiry`.
pub(crate) fn order_expiry_for(
    order_type: OrderType,
    tif: &TimeInForce,
    expire_time: Option<UnixNanos>,
    now_ms: i64,
) -> anyhow::Result<i64> {
    if order_type == OrderType::Market {
        return Ok(ORDER_EXPIRY_IOC);
    }

    if matches!(tif, TimeInForce::Gtd)
        && let Some(ts) = expire_time
    {
        let expiry_ms = (ts.as_u64() / 1_000_000) as i64;
        let min_expiry_ms = now_ms.saturating_add(ORDER_EXPIRY_MIN_GTD_MS);
        let max_expiry_ms = now_ms.saturating_add(ORDER_EXPIRY_MAX_GTD_MS);
        anyhow::ensure!(
            expiry_ms >= min_expiry_ms,
            "Lighter GTD expire_time must be at least 5 minutes from now (plus 1 second transport margin)",
        );
        anyhow::ensure!(
            expiry_ms <= max_expiry_ms,
            "Lighter GTD expire_time must be no more than 30 days from now",
        );
        return Ok(expiry_ms);
    }

    if is_conditional_order(order_type) && matches!(tif, TimeInForce::Ioc) {
        return Ok(now_ms.saturating_add(ORDER_EXPIRY_DEFAULT_GTC_MS));
    }

    if matches!(tif, TimeInForce::Ioc | TimeInForce::Fok) {
        return Ok(ORDER_EXPIRY_IOC);
    }

    Ok(now_ms.saturating_add(ORDER_EXPIRY_DEFAULT_GTC_MS))
}

fn is_conditional_market_order(order_type: OrderType) -> bool {
    matches!(
        order_type,
        OrderType::StopMarket | OrderType::MarketIfTouched
    )
}

fn is_conditional_order(order_type: OrderType) -> bool {
    matches!(
        order_type,
        OrderType::StopMarket
            | OrderType::StopLimit
            | OrderType::MarketIfTouched
            | OrderType::LimitIfTouched
    )
}

/// Convert a Nautilus [`Quantity`] to the venue's signed-i64 base-asset tick
/// representation, given the instrument's size precision.
pub(crate) fn quantity_to_ticks(quantity: &Quantity, decimals: u8) -> anyhow::Result<i64> {
    let scaled = quantity.as_decimal() * Decimal::from(10_i64.pow(u32::from(decimals)));
    decimal_trunc_to_i64(scaled)
        .with_context(|| format!("quantity `{quantity}` overflows i64 at precision {decimals}"))
}

/// Convert a Nautilus [`Price`] to the venue's `u32` quote-asset tick
/// representation, given the instrument's price precision.
pub(crate) fn price_to_ticks(price: &Price, decimals: u8) -> anyhow::Result<u32> {
    let scaled = price.as_decimal() * Decimal::from(10_i64.pow(u32::from(decimals)));
    let value = decimal_trunc_to_i64(scaled)
        .with_context(|| format!("price `{price}` overflows i64 at precision {decimals}"))?;
    u32::try_from(value).with_context(|| {
        format!("price `{price}` overflows u32 (Lighter limit) at precision {decimals}")
    })
}

/// Truncate a [`Decimal`] toward zero and convert to `i64`, returning an
/// error if the truncated value does not fit. Avoids the
/// `decimal.to_string().split('.').parse()` round-trip the previous
/// implementations used; runs on every exec submit and modify.
fn decimal_trunc_to_i64(value: Decimal) -> anyhow::Result<i64> {
    value
        .trunc()
        .to_i64()
        .ok_or_else(|| anyhow::anyhow!("decimal `{value}` does not fit in i64"))
}

/// Derive a worst-acceptable price (in venue ticks) for `MARKET` /
/// `STOP_MARKET` / `MARKET_IF_TOUCHED` orders. Buys widen `base` upward,
/// sells downward, by `slippage_bps`; the result rounds conservatively at
/// `price_precision` so the venue cap never under-shoots the budget.
pub(crate) fn derive_market_order_price_ticks(
    base: Decimal,
    is_buy: bool,
    price_precision: u8,
    slippage_bps: u32,
) -> anyhow::Result<u32> {
    let slippage = Decimal::new(i64::from(slippage_bps), 4);
    let widened = if is_buy {
        base * (Decimal::ONE + slippage)
    } else {
        base * (Decimal::ONE - slippage)
    };

    let scale = Decimal::from(10_i64.pow(u32::from(price_precision)));
    let scaled = widened * scale;
    let rounded = if is_buy {
        scaled.ceil()
    } else {
        scaled.floor()
    };
    let value = decimal_trunc_to_i64(rounded).with_context(|| {
        format!("derived market price `{widened}` overflows i64 at precision {price_precision}",)
    })?;

    // Lighter rejects `price = 0` as `21702 invalid price`.
    anyhow::ensure!(
        value > 0,
        "derived market price `{widened}` rounds to 0 ticks at precision {price_precision} (slippage_bps={slippage_bps}); reduce slippage or increase price precision",
    );
    u32::try_from(value).with_context(|| {
        format!("derived market price `{widened}` overflows u32 at precision {price_precision}",)
    })
}

/// Degrade an `Err` sub-report to an empty `Vec` after logging the full
/// chain at WARN. Deliberate: a transient REST failure on one category
/// must not blank out the others. Visibility comes from the `{e:#}` log,
/// not from the returned `ExecutionMassStatus`.
pub(crate) fn unwrap_reports_or_warn<T>(label: &str, result: anyhow::Result<Vec<T>>) -> Vec<T> {
    match result {
        Ok(reports) => reports,
        Err(e) => {
            log::warn!(
                "Lighter mass-status: {label} reports failed: {}",
                scrub_auth(&format!("{e:#}")),
            );
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use nautilus_core::UUID4;
    use nautilus_model::{
        enums::{
            AccountType, LiquiditySide, OrderSide, OrderStatus, OrderType, PositionSideSpecified,
        },
        identifiers::{AccountId, StrategyId, TradeId},
        orders::Order,
        reports::FillReport,
        types::Money,
    };
    use rstest::rstest;

    use super::*;

    fn cloid(s: &str) -> ClientOrderId {
        ClientOrderId::new(s)
    }

    fn voi(s: &str) -> VenueOrderId {
        VenueOrderId::new(s)
    }

    fn stub_open_order_status_report(client_order_id_str: &str) -> OrderStatusReport {
        OrderStatusReport::new(
            AccountId::from("LIGHTER-TEST"),
            InstrumentId::from("ETH-PERP.LIGHTER"),
            Some(ClientOrderId::new(client_order_id_str)),
            VenueOrderId::new("281476929510110"),
            OrderSide::Sell,
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            Quantity::from("0.01"),
            Quantity::from("0"),
            UnixNanos::from(1),
            UnixNanos::from(2),
            UnixNanos::from(3),
            None,
        )
    }

    fn stub_canceled_status_report(client_order_id_str: &str) -> OrderStatusReport {
        let mut r = stub_open_order_status_report(client_order_id_str);
        r.order_status = OrderStatus::Canceled;
        r
    }

    fn stub_position_report(instrument: &str, qty: &str) -> PositionStatusReport {
        PositionStatusReport::new(
            AccountId::from("LIGHTER-TEST"),
            InstrumentId::from(instrument),
            PositionSideSpecified::Long,
            Quantity::from(qty),
            UnixNanos::from(1),
            UnixNanos::from(2),
            None,
            None,
            None,
        )
    }

    /// `snapshots` are applied in order; the cache is queried with `filter`
    /// after the last and compared against `expected`.
    #[rstest]
    #[case::empty_unfiltered(vec![vec![]], None, vec![])]
    #[case::empty_filtered(vec![vec![]], Some("ETH-PERP.LIGHTER"), vec![])]
    #[case::single_unfiltered(
        vec![vec![("ETH-PERP.LIGHTER", "1.5")]],
        None,
        vec![("ETH-PERP.LIGHTER", "1.5")],
    )]
    #[case::single_filtered_matching(
        vec![vec![("ETH-PERP.LIGHTER", "1.5")]],
        Some("ETH-PERP.LIGHTER"),
        vec![("ETH-PERP.LIGHTER", "1.5")],
    )]
    #[case::single_filtered_nonmatching(
        vec![vec![("ETH-PERP.LIGHTER", "1.5")]],
        Some("DOGE-PERP.LIGHTER"),
        vec![],
    )]
    #[case::successive_snapshots_overwrite_same_instrument(
        vec![
            vec![("ETH-PERP.LIGHTER", "1.5")],
            vec![("ETH-PERP.LIGHTER", "2.5")],
        ],
        None,
        vec![("ETH-PERP.LIGHTER", "2.5")],
    )]
    #[case::multi_instrument_filter_matches_one(
        vec![vec![("ETH-PERP.LIGHTER", "1.0"), ("BTC-PERP.LIGHTER", "0.1")]],
        Some("BTC-PERP.LIGHTER"),
        vec![("BTC-PERP.LIGHTER", "0.1")],
    )]
    #[case::closed_position_evicted_by_subsequent_snapshot(
        vec![
            vec![("ETH-PERP.LIGHTER", "1.0"), ("BTC-PERP.LIGHTER", "0.1")],
            vec![("BTC-PERP.LIGHTER", "0.1")],
        ],
        None,
        vec![("BTC-PERP.LIGHTER", "0.1")],
    )]
    #[case::all_positions_closed_by_empty_snapshot(
        vec![
            vec![("ETH-PERP.LIGHTER", "1.0")],
            vec![],
        ],
        None,
        vec![],
    )]
    fn replace_positions_matrix(
        #[case] snapshots: Vec<Vec<(&str, &str)>>,
        #[case] filter: Option<&str>,
        #[case] expected: Vec<(&str, &str)>,
    ) {
        let state = WsDispatchState::new();

        for snapshot in snapshots {
            let frame: Vec<PositionStatusReport> = snapshot
                .into_iter()
                .map(|(instrument, qty)| stub_position_report(instrument, qty))
                .collect();
            state.replace_positions(&frame);
        }

        let result = state.snapshot_positions(filter.map(InstrumentId::from));

        let mut actual: Vec<(String, String)> = result
            .into_iter()
            .map(|r| (r.instrument_id.to_string(), r.quantity.to_string()))
            .collect();
        let mut expected_owned: Vec<(String, String)> = expected
            .into_iter()
            .map(|(i, q)| (i.to_string(), q.to_string()))
            .collect();
        actual.sort();
        expected_owned.sort();
        assert_eq!(actual, expected_owned);
    }

    #[rstest]
    fn replace_positions_with_empty_input_clears_cache() {
        // Anchors the contract the consumption loop relies on for the
        // `Reconnected` and `connect()` cache-drop paths.
        let state = WsDispatchState::new();
        state.replace_positions(&[stub_position_report("ETH-PERP.LIGHTER", "1.0")]);
        assert_eq!(state.snapshot_positions(None).len(), 1);

        state.replace_positions(&[]);

        assert!(state.snapshot_positions(None).is_empty());
    }

    #[rstest]
    fn replace_positions_except_keeps_only_retained_absent_positions() {
        let state = WsDispatchState::new();
        state.replace_positions(&[
            stub_position_report("ETH-PERP.LIGHTER", "1.0"),
            stub_position_report("BTC-PERP.LIGHTER", "2.0"),
            stub_position_report("DOGE-PERP.LIGHTER", "4.0"),
        ]);

        let removed = state.replace_positions_except(
            &[stub_position_report("ETH-PERP.LIGHTER", "3.0")],
            &[InstrumentId::from("BTC-PERP.LIGHTER")],
        );

        let mut actual: Vec<(String, String)> = state
            .snapshot_positions(None)
            .into_iter()
            .map(|r| (r.instrument_id.to_string(), r.quantity.to_string()))
            .collect();
        actual.sort();

        assert_eq!(removed, vec![InstrumentId::from("DOGE-PERP.LIGHTER")]);
        assert_eq!(
            actual,
            vec![
                ("BTC-PERP.LIGHTER".to_string(), "2.0".to_string()),
                ("ETH-PERP.LIGHTER".to_string(), "3.0".to_string()),
            ],
        );
    }

    #[rstest]
    fn unwrap_reports_or_warn_returns_inner_on_ok() {
        let result: anyhow::Result<Vec<i32>> = Ok(vec![1, 2, 3]);
        assert_eq!(unwrap_reports_or_warn("orders", result), vec![1, 2, 3]);
    }

    #[rstest]
    fn unwrap_reports_or_warn_returns_empty_on_err() {
        let result: anyhow::Result<Vec<i32>> = Err(anyhow::anyhow!("boom"));
        let out: Vec<i32> = unwrap_reports_or_warn("orders", result);
        assert!(out.is_empty());
    }

    #[rstest]
    fn derive_client_order_index_is_deterministic_within_state() {
        let state = WsDispatchState::new();
        let cid = cloid("MY-ORDER-001");
        let a = state.derive_client_order_index(&cid);
        let b = state.derive_client_order_index(&cid);
        assert_eq!(a, b);
        assert!(a >= 0, "derived index must be non-negative");
    }

    #[rstest]
    fn derive_client_order_index_is_stable_across_instances() {
        // The hasher uses a fixed seed so a fresh client (after a process
        // restart) derives the same `client_order_index` for the same
        // ClientOrderId. Without this, REST-based query_order cannot
        // recover orders submitted by a prior instance.
        let cid = cloid("RESTART-RECOVERY-ORDER");
        let a = WsDispatchState::new().derive_client_order_index(&cid);
        let b = WsDispatchState::new().derive_client_order_index(&cid);
        assert_eq!(a, b);
    }

    #[rstest]
    fn derive_client_order_index_separates_distinct_cloids() {
        let state = WsDispatchState::new();
        let a = state.derive_client_order_index(&cloid("ORDER-A"));
        let b = state.derive_client_order_index(&cloid("ORDER-B"));
        assert_ne!(a, b, "distinct cloids should map to distinct indexes");
    }

    #[rstest]
    fn register_cloid_returns_index_on_first_registration() {
        let state = WsDispatchState::new();
        let cid = cloid("ORDER-A");
        let derived = state.derive_client_order_index(&cid);

        let chosen = state.register_cloid(derived, cid).unwrap();

        assert_eq!(chosen, derived);
        assert_eq!(state.cloid_map.get(&chosen).map(|e| *e.value()), Some(cid));
    }

    #[rstest]
    fn register_cloid_is_idempotent_for_same_cloid() {
        // A retry of the same submit must reuse the assigned slot, not
        // probe forward (which would waste 31-bit space and break the
        // reverse lookup for the venue's echo).
        let state = WsDispatchState::new();
        let cid = cloid("ORDER-A");
        let derived = state.derive_client_order_index(&cid);

        let first = state.register_cloid(derived, cid).unwrap();
        let second = state.register_cloid(derived, cid).unwrap();

        assert_eq!(first, second);
        assert_eq!(state.cloid_map.len(), 1);
    }

    #[rstest]
    fn register_cloid_probes_forward_on_collision() {
        // Two distinct cloids with the same derived index: the second must
        // probe to a different slot rather than overwrite the first.
        let state = WsDispatchState::new();
        let first_cid = cloid("ORDER-A");
        let second_cid = cloid("ORDER-B");
        // Force a collision by inserting at the same index the second
        // registration will derive.
        let collision_index = 42;
        state.cloid_map.insert(collision_index, first_cid);

        let chosen = state.register_cloid(collision_index, second_cid).unwrap();

        assert_ne!(
            chosen, collision_index,
            "collided second cloid must land in a distinct slot",
        );
        assert_eq!(
            state.cloid_map.get(&collision_index).map(|e| *e.value()),
            Some(first_cid),
        );
        assert_eq!(
            state.cloid_map.get(&chosen).map(|e| *e.value()),
            Some(second_cid)
        );
    }

    #[rstest]
    fn register_cloid_probe_exhaustion_preserves_existing_mappings() {
        let state = WsDispatchState::new();
        let initial_index = 42;
        let existing: Vec<_> = (0..=CLOID_INDEX_PROBE_LIMIT)
            .map(|attempt| {
                let index = initial_index + i64::try_from(attempt).unwrap();
                let cid = cloid(&format!("EXISTING-{attempt}"));
                state.cloid_map.insert(index, cid);
                (index, cid)
            })
            .collect();

        let result = state.register_cloid(initial_index, cloid("NEW-ORDER"));

        assert!(result.is_err());
        assert_eq!(state.cloid_map.len(), existing.len());
        for (index, cid) in existing {
            assert_eq!(
                state.cloid_map.get(&index).map(|entry| *entry.value()),
                Some(cid),
            );
        }
    }

    #[rstest]
    fn store_snapshot_is_idempotent_for_same_shape() {
        // The dispatcher compares against the stored snapshot to decide
        // whether an Open frame is a modify ack. Storing the same shape
        // twice must surface no diff.
        let state = WsDispatchState::new();
        let cid = cloid("SNAPSHOT-CLOID");
        let shape = OrderShapeSnapshot {
            quantity: Quantity::from("0.01"),
            price: Some(Price::from("2352.74")),
            trigger_price: None,
        };

        state.store_snapshot(cid, shape.clone());

        assert_eq!(state.snapshot_for(&cid).as_ref(), Some(&shape));
    }

    #[rstest]
    fn store_snapshot_replaces_on_modify() {
        let state = WsDispatchState::new();
        let cid = cloid("SNAPSHOT-CLOID-2");
        let first = OrderShapeSnapshot {
            quantity: Quantity::from("0.01"),
            price: Some(Price::from("2352.74")),
            trigger_price: None,
        };
        let second = OrderShapeSnapshot {
            quantity: Quantity::from("0.02"),
            price: Some(Price::from("2400.00")),
            trigger_price: None,
        };

        state.store_snapshot(cid, first);
        state.store_snapshot(cid, second.clone());

        assert_eq!(state.snapshot_for(&cid).as_ref(), Some(&second));
    }

    #[rstest]
    fn triggered_emitted_dedupes_repeats() {
        let state = WsDispatchState::new();
        let cid = cloid("TRIGGER-CLOID");

        assert!(state.mark_triggered_emitted(cid), "first mark inserts");
        assert!(
            !state.mark_triggered_emitted(cid),
            "second mark is suppressed",
        );
        assert!(state.triggered_was_emitted(&cid));
    }

    #[rstest]
    fn trade_dedup_cache_evicts_oldest_marker() {
        let dedup = TradeDedupCache::new(2);
        let first = TradeId::new("1");
        let second = TradeId::new("2");
        let third = TradeId::new("3");

        dedup.insert(first, TradeDedupSource::Live);
        dedup.insert(second, TradeDedupSource::Live);
        dedup.insert(third, TradeDedupSource::Live);

        assert!(!dedup.contains(&first));
        assert!(dedup.contains(&second));
        assert!(dedup.contains(&third));
    }

    #[rstest]
    fn retired_order_cache_evicts_oldest_identity_and_index() {
        let cache = RetiredOrderCache::new(2);
        let identities = [
            (cloid("RETIRED-1"), 1),
            (cloid("RETIRED-2"), 2),
            (cloid("RETIRED-3"), 3),
        ];

        for (cid, index) in identities {
            cache.insert(
                cid,
                OrderIdentity::new(
                    InstrumentId::from("ETH-PERP.LIGHTER"),
                    StrategyId::new("S-T"),
                    OrderSide::Buy,
                    OrderType::Limit,
                    index,
                ),
            );
        }

        assert_eq!(cache.cloid_for_index(1), None);
        assert!(cache.identity_for_cloid(&cloid("RETIRED-1")).is_none());
        assert_eq!(cache.cloid_for_index(2), Some(cloid("RETIRED-2")));
        assert_eq!(cache.cloid_for_index(3), Some(cloid("RETIRED-3")));
    }

    #[rstest]
    fn register_cloid_does_not_reuse_retired_index() {
        let state = WsDispatchState::new();
        let retired = cloid("RETIRED");
        let replacement = cloid("REPLACEMENT");
        let retired_index = 42;

        state.register_cloid(retired_index, retired).unwrap();
        state.register_order_identity(
            retired,
            OrderIdentity::new(
                InstrumentId::from("ETH-PERP.LIGHTER"),
                StrategyId::new("S-T"),
                OrderSide::Buy,
                OrderType::Limit,
                retired_index,
            ),
        );
        state.retire_order_identity(&retired);

        let chosen = state.register_cloid(retired_index, replacement).unwrap();

        assert_ne!(chosen, retired_index);
        assert_eq!(
            state.resolve_client_order_index(retired_index),
            Some(retired)
        );
        assert_eq!(state.resolve_client_order_index(chosen), Some(replacement));
    }

    #[rstest]
    fn seed_accepted_from_report_requires_tracked_identity() {
        let state = WsDispatchState::new();
        let cid = cloid("REPORT-ACCEPTED");
        let report = stub_open_order_status_report(cid.as_str());

        state.seed_accepted_from_report(&report);
        assert!(!state.accepted_was_emitted(&cid));

        state.register_order_identity(
            cid,
            OrderIdentity::new(
                InstrumentId::from("ETH-PERP.LIGHTER"),
                StrategyId::new("S-T"),
                OrderSide::Buy,
                OrderType::Limit,
                1,
            ),
        );
        state.seed_accepted_from_report(&report);

        assert!(state.accepted_was_emitted(&cid));
    }

    #[rstest]
    #[case::submitted(OrderStatus::Submitted)]
    #[case::pending_update(OrderStatus::PendingUpdate)]
    #[case::pending_cancel(OrderStatus::PendingCancel)]
    #[case::accepted(OrderStatus::Accepted)]
    #[case::triggered(OrderStatus::Triggered)]
    #[case::partially_filled(OrderStatus::PartiallyFilled)]
    #[case::filled(OrderStatus::Filled)]
    #[case::canceled(OrderStatus::Canceled)]
    #[case::expired(OrderStatus::Expired)]
    fn seed_accepted_from_report_marks_accepted_lifecycle_statuses(#[case] status: OrderStatus) {
        let state = WsDispatchState::new();
        let cid = ClientOrderId::new(format!("REPORT-{status:?}"));
        state.register_order_identity(
            cid,
            OrderIdentity::new(
                InstrumentId::from("ETH-PERP.LIGHTER"),
                StrategyId::new("S-T"),
                OrderSide::Buy,
                OrderType::Limit,
                1,
            ),
        );

        let mut report = stub_open_order_status_report(cid.as_str());
        report.order_status = status;
        state.seed_accepted_from_report(&report);

        assert!(state.accepted_was_emitted(&cid));
    }

    #[rstest]
    fn seed_accepted_from_report_skips_rejected_report() {
        let state = WsDispatchState::new();
        let cid = cloid("REPORT-REJECTED");
        state.register_order_identity(
            cid,
            OrderIdentity::new(
                InstrumentId::from("ETH-PERP.LIGHTER"),
                StrategyId::new("S-T"),
                OrderSide::Buy,
                OrderType::Limit,
                1,
            ),
        );

        let mut report = stub_open_order_status_report(cid.as_str());
        report.order_status = OrderStatus::Rejected;
        state.seed_accepted_from_report(&report);

        assert!(!state.accepted_was_emitted(&cid));
    }

    #[rstest]
    fn accepted_state_survives_more_than_ten_thousand_newer_orders() {
        let state = WsDispatchState::new();
        let early = cloid("EARLY-RESTING-ORDER");
        state.register_order_identity(
            early,
            OrderIdentity::new(
                InstrumentId::from("ETH-PERP.LIGHTER"),
                StrategyId::new("S-T"),
                OrderSide::Buy,
                OrderType::Limit,
                1,
            ),
        );
        assert!(state.claim_accepted_emission(&early));

        for index in 2..=10_002 {
            let newer = ClientOrderId::new(format!("NEWER-{index}"));
            state.register_order_identity(
                newer,
                OrderIdentity::new(
                    InstrumentId::from("ETH-PERP.LIGHTER"),
                    StrategyId::new("S-T"),
                    OrderSide::Buy,
                    OrderType::Limit,
                    index,
                ),
            );
            assert!(state.claim_accepted_emission(&newer));
        }

        assert!(state.accepted_was_emitted(&early));
        assert!(
            !state.claim_accepted_emission(&early),
            "the still-active early order must not emit a second OrderAccepted",
        );
    }

    #[rstest]
    fn seed_accepted_from_report_marks_submitted_report() {
        let state = WsDispatchState::new();
        let cid = cloid("REPORT-SUBMITTED");
        state.register_order_identity(
            cid,
            OrderIdentity::new(
                InstrumentId::from("ETH-PERP.LIGHTER"),
                StrategyId::new("S-T"),
                OrderSide::Buy,
                OrderType::Limit,
                1,
            ),
        );

        let mut report = stub_open_order_status_report(cid.as_str());
        report.order_status = OrderStatus::Submitted;
        state.seed_accepted_from_report(&report);

        assert!(state.accepted_was_emitted(&cid));
    }

    #[rstest]
    fn forget_order_identity_clears_snapshot_and_triggered() {
        // Failed dispatch cleanup clears all per-order state. Terminal venue
        // events use `retire_order_identity` instead so trailing fills retain
        // their identity and accepted marker.
        let state = WsDispatchState::new();
        let cid = cloid("TERMINAL-CLEANUP");
        let identity = OrderIdentity::new(
            InstrumentId::from("ETH-PERP.LIGHTER"),
            StrategyId::new("S-T"),
            OrderSide::Buy,
            OrderType::Limit,
            1,
        );

        state.register_order_identity(cid, identity);
        state.mark_accepted_emitted(cid);
        state.mark_triggered_emitted(cid);
        state.store_snapshot(
            cid,
            OrderShapeSnapshot {
                quantity: Quantity::from("0.01"),
                price: Some(Price::from("2352.74")),
                trigger_price: None,
            },
        );

        state.forget_order_identity(&cid);

        assert!(state.snapshot_for(&cid).is_none());
        assert!(!state.triggered_was_emitted(&cid));
        assert!(!state.accepted_was_emitted(&cid));
    }

    #[rstest]
    fn mark_trade_seen_dedupes_repeats() {
        let state = WsDispatchState::new();
        let trade_id = TradeId::new("19209006902");

        let first = state.mark_trade_seen(trade_id);
        let second = state.mark_trade_seen(trade_id);

        assert!(first, "first observation is new");
        assert!(!second, "repeat observation is suppressed");
    }

    #[rstest]
    fn order_identity_lifecycle_register_then_forget() {
        let state = WsDispatchState::new();
        let cid = cloid("ORDER-LIFECYCLE");
        let identity = OrderIdentity::new(
            InstrumentId::from("ETH-PERP.LIGHTER"),
            StrategyId::new("S-T"),
            OrderSide::Buy,
            OrderType::Limit,
            1,
        );

        state.register_order_identity(cid, identity);
        assert!(state.order_identities.contains_key(&cid));

        state.mark_accepted_emitted(cid);
        assert!(state.accepted_was_emitted(&cid));

        state.forget_order_identity(&cid);
        assert!(!state.order_identities.contains_key(&cid));
        assert!(!state.accepted_was_emitted(&cid));
    }

    #[rstest]
    fn translate_order_cloid_substitutes_known_index() {
        let state = WsDispatchState::new();
        let original = cloid("MY-ORDER-001");
        state.register_cloid(42, original).unwrap();
        state.register_order_identity(
            original,
            OrderIdentity::new(
                InstrumentId::from("ETH-PERP.LIGHTER"),
                StrategyId::new("S-T"),
                OrderSide::Buy,
                OrderType::Limit,
                42,
            ),
        );

        let report = stub_open_order_status_report("42");
        assert_eq!(
            state.resolve_live_cloid("42", report.venue_order_id),
            Some(original),
        );
        let translated = state.translate_order_cloid(report);

        assert_eq!(translated.client_order_id, Some(original));
    }

    #[rstest]
    fn translate_order_cloid_uses_venue_id_for_unknown_index() {
        let state = WsDispatchState::new();
        let report = stub_open_order_status_report("99");
        let translated = state.translate_order_cloid(report);

        assert_eq!(
            translated.client_order_id.map(|c| c.to_string()),
            Some("281476929510110".to_string()),
        );
    }

    #[rstest]
    fn translate_order_cloid_passes_through_non_integer_cloid() {
        let state = WsDispatchState::new();
        let report = stub_open_order_status_report("not-an-int");
        let translated = state.translate_order_cloid(report);

        assert_eq!(
            translated.client_order_id.map(|c| c.to_string()),
            Some("not-an-int".to_string()),
        );
    }

    fn stub_fill_report(client_order_id_str: &str) -> FillReport {
        FillReport::new(
            AccountId::from("LIGHTER-TEST"),
            InstrumentId::from("ETH-PERP.LIGHTER"),
            VenueOrderId::new("281476929510102"),
            TradeId::new("19209006902"),
            OrderSide::Buy,
            Quantity::from("0.1336"),
            Price::from("2352.73"),
            Money::from("0.000196 USDC"),
            LiquiditySide::Taker,
            Some(ClientOrderId::new(client_order_id_str)),
            None,
            UnixNanos::from(1),
            UnixNanos::from(2),
            Some(UUID4::new()),
        )
    }

    // Fill-side cloid translation mirrors the order-side path: a numeric
    // client id substitutes to the originating Nautilus cloid only when the
    // venue order id matches its bound identity. Unknown numerics use the
    // venue order id because Lighter can reuse client indexes over time.
    #[rstest]
    fn translate_fill_cloid_substitutes_known_index() {
        let state = WsDispatchState::new();
        let original = cloid("MY-ORDER-001");
        state.register_cloid(42, original).unwrap();
        state.register_order_identity(
            original,
            OrderIdentity::new(
                InstrumentId::from("ETH-PERP.LIGHTER"),
                StrategyId::new("S-T"),
                OrderSide::Buy,
                OrderType::Limit,
                42,
            ),
        );

        let report = stub_fill_report("42");
        assert_eq!(
            state.resolve_live_cloid("42", report.venue_order_id),
            Some(original),
        );
        let translated = state.translate_fill_cloid(report);

        assert_eq!(translated.client_order_id, Some(original));
    }

    #[rstest]
    fn translate_fill_cloid_uses_venue_id_for_unknown_index() {
        let state = WsDispatchState::new();
        let report = stub_fill_report("99");
        let translated = state.translate_fill_cloid(report);

        assert_eq!(
            translated.client_order_id.map(|c| c.to_string()),
            Some("281476929510102".to_string()),
        );
    }

    #[rstest]
    fn translate_fill_cloid_passes_through_non_integer_cloid() {
        let state = WsDispatchState::new();
        let report = stub_fill_report("not-an-int");
        let translated = state.translate_fill_cloid(report);

        assert_eq!(
            translated.client_order_id.map(|c| c.to_string()),
            Some("not-an-int".to_string()),
        );
    }

    #[rstest]
    fn reused_unknown_index_produces_distinct_external_cloids() {
        let state = WsDispatchState::new();
        let first = state.translate_order_cloid(stub_open_order_status_report("99"));
        let mut second_report = stub_open_order_status_report("99");
        second_report.venue_order_id = VenueOrderId::new("281476929510111");
        let second = state.translate_order_cloid(second_report);

        assert_eq!(
            first.client_order_id,
            Some(ClientOrderId::new("281476929510110")),
        );
        assert_eq!(
            second.client_order_id,
            Some(ClientOrderId::new("281476929510111")),
        );
        assert_ne!(first.client_order_id, second.client_order_id);
    }

    #[rstest]
    fn known_index_with_wrong_venue_id_uses_external_cloid() {
        let state = WsDispatchState::new();
        let original = cloid("MY-ORDER-001");
        state.register_cloid(42, original).unwrap();
        state.register_order_identity(
            original,
            OrderIdentity::new(
                InstrumentId::from("ETH-PERP.LIGHTER"),
                StrategyId::new("S-T"),
                OrderSide::Buy,
                OrderType::Limit,
                42,
            ),
        );
        assert_eq!(
            state.resolve_live_cloid("42", VenueOrderId::new("281476929510109")),
            Some(original),
        );

        let translated = state.translate_order_cloid(stub_open_order_status_report("42"));

        assert_eq!(
            translated.client_order_id,
            Some(ClientOrderId::new("281476929510110")),
        );
    }

    #[rstest]
    fn explicit_venue_lookup_does_not_fall_through_to_client_index() {
        assert!(!order_matches_lookup(100, 42, Some(101), Some(42)));
        assert!(order_matches_lookup(101, 99, Some(101), Some(42)));
        assert!(order_matches_lookup(100, 42, None, Some(42)));
    }

    #[rstest]
    fn evict_terminal_mappings_drops_venue_id_map_only() {
        // cloid_map is intentionally retained so a trailing
        // account_all_trades frame can still translate its numeric
        // client_order_index back to the original cloid even after the
        // terminal-status frame arrived first.
        let cloid_map: Arc<DashMap<i64, ClientOrderId>> = Arc::new(DashMap::new());
        let venue_id_map: Arc<DashMap<ClientOrderId, VenueOrderId>> = Arc::new(DashMap::new());
        let original = cloid("MY-ORDER-001");
        cloid_map.insert(42, original);
        venue_id_map.insert(original, voi("281476929510110"));

        let report = stub_canceled_status_report("MY-ORDER-001");
        evict_terminal_mappings(&report, &venue_id_map);

        assert!(
            cloid_map.get(&42).is_some(),
            "cloid_map must survive terminal status to translate trailing fills",
        );
        assert!(venue_id_map.get(&original).is_none());
    }

    #[rstest]
    fn evict_terminal_mappings_no_op_for_missing_cloid() {
        let venue_id_map: Arc<DashMap<ClientOrderId, VenueOrderId>> = Arc::new(DashMap::new());
        let mut report = stub_canceled_status_report("MY-ORDER-001");
        report.client_order_id = None;

        evict_terminal_mappings(&report, &venue_id_map);
        assert_eq!(venue_id_map.len(), 0);
    }

    #[rstest]
    #[case(TimeInForce::Ioc, LighterTimeInForce::ImmediateOrCancel)]
    #[case(TimeInForce::Gtc, LighterTimeInForce::GoodTillTime)]
    #[case(TimeInForce::Day, LighterTimeInForce::GoodTillTime)]
    #[case(TimeInForce::Gtd, LighterTimeInForce::GoodTillTime)]
    fn nautilus_to_lighter_tif_supported_variants(
        #[case] input: TimeInForce,
        #[case] expected: LighterTimeInForce,
    ) {
        assert_eq!(
            nautilus_to_lighter_tif(OrderType::Limit, input, false).unwrap(),
            expected
        );
    }

    #[rstest]
    fn nautilus_to_lighter_tif_market_orders_use_ioc() {
        assert_eq!(
            nautilus_to_lighter_tif(OrderType::Market, TimeInForce::Gtc, false).unwrap(),
            LighterTimeInForce::ImmediateOrCancel,
        );
        assert_eq!(
            nautilus_to_lighter_tif(OrderType::Market, TimeInForce::Ioc, false).unwrap(),
            LighterTimeInForce::ImmediateOrCancel,
        );
    }

    #[rstest]
    #[case(TimeInForce::Day)]
    #[case(TimeInForce::Gtd)]
    fn nautilus_to_lighter_tif_market_orders_reject_resting_tifs(#[case] tif: TimeInForce) {
        let err = nautilus_to_lighter_tif(OrderType::Market, tif, false).unwrap_err();
        assert!(err.to_string().contains("market orders"));
    }

    #[rstest]
    #[case(OrderType::StopMarket)]
    #[case(OrderType::MarketIfTouched)]
    fn nautilus_to_lighter_tif_conditional_market_orders_use_ioc_wire_tif(
        #[case] order_type: OrderType,
    ) {
        for tif in [TimeInForce::Gtc, TimeInForce::Day, TimeInForce::Gtd] {
            assert_eq!(
                nautilus_to_lighter_tif(order_type, tif, false).unwrap(),
                LighterTimeInForce::ImmediateOrCancel,
            );
        }
    }

    #[rstest]
    fn nautilus_to_lighter_tif_conditional_market_orders_reject_nautilus_ioc() {
        let err =
            nautilus_to_lighter_tif(OrderType::StopMarket, TimeInForce::Ioc, false).unwrap_err();
        assert!(err.to_string().contains("positive expiry"));
    }

    #[rstest]
    #[case(OrderType::StopLimit)]
    #[case(OrderType::LimitIfTouched)]
    fn nautilus_to_lighter_tif_conditional_limit_orders_allow_ioc(#[case] order_type: OrderType) {
        assert_eq!(
            nautilus_to_lighter_tif(order_type, TimeInForce::Ioc, false).unwrap(),
            LighterTimeInForce::ImmediateOrCancel,
        );
    }

    #[rstest]
    #[case(TimeInForce::Gtc)]
    #[case(TimeInForce::Gtd)]
    #[case(TimeInForce::Ioc)]
    fn nautilus_to_lighter_tif_post_only_overrides_base_tif(#[case] tif: TimeInForce) {
        // post_only=true must take precedence regardless of the nominal TIF
        // because Lighter exposes a dedicated PostOnly slot.
        assert_eq!(
            nautilus_to_lighter_tif(OrderType::Limit, tif, true).unwrap(),
            LighterTimeInForce::PostOnly,
        );
    }

    #[rstest]
    fn nautilus_to_lighter_tif_rejects_fok() {
        // Lighter has no fill-or-kill primitive; mapping FOK to IOC would
        // let a partial fill satisfy the order. Reject explicitly.
        let err = nautilus_to_lighter_tif(OrderType::Limit, TimeInForce::Fok, false).unwrap_err();
        assert!(err.to_string().contains("fill-or-kill"));
    }

    #[rstest]
    #[case(TimeInForce::AtTheOpen)]
    #[case(TimeInForce::AtTheClose)]
    fn nautilus_to_lighter_tif_unsupported_variants_error(#[case] tif: TimeInForce) {
        let err = nautilus_to_lighter_tif(OrderType::Limit, tif, false).unwrap_err();
        assert!(err.to_string().contains("does not support"));
    }

    const NOW_MS: i64 = 1_700_000_000_000;

    #[rstest]
    fn order_expiry_for_gtd_with_expiry_returns_millis() {
        let expiry_ms = NOW_MS + ORDER_EXPIRY_MIN_GTD_MS + 123;
        let ts = UnixNanos::from((expiry_ms as u64) * 1_000_000);
        assert_eq!(
            order_expiry_for(OrderType::Limit, &TimeInForce::Gtd, Some(ts), NOW_MS).unwrap(),
            expiry_ms,
        );
    }

    #[rstest]
    #[case::too_short(ORDER_EXPIRY_MIN_GTD_MS - 1, "at least 5 minutes")]
    #[case::too_long(ORDER_EXPIRY_MAX_GTD_MS + 1, "no more than 30 days")]
    fn order_expiry_for_rejects_gtd_outside_venue_range(
        #[case] offset_ms: i64,
        #[case] expected: &str,
    ) {
        let ts = UnixNanos::from(((NOW_MS + offset_ms) as u64) * 1_000_000);
        let error =
            order_expiry_for(OrderType::Limit, &TimeInForce::Gtd, Some(ts), NOW_MS).unwrap_err();

        assert!(error.to_string().contains(expected));
    }

    #[rstest]
    #[case(TimeInForce::Gtc, None)]
    #[case(TimeInForce::Day, None)]
    #[case(TimeInForce::Gtd, None)]
    fn order_expiry_for_default_returns_now_plus_28d(
        #[case] tif: TimeInForce,
        #[case] expire: Option<UnixNanos>,
    ) {
        assert_eq!(
            order_expiry_for(OrderType::Limit, &tif, expire, NOW_MS).unwrap(),
            NOW_MS + ORDER_EXPIRY_DEFAULT_GTC_MS,
        );
    }

    #[rstest]
    #[case(TimeInForce::Ioc)]
    #[case(TimeInForce::Fok)]
    fn order_expiry_for_ioc_returns_zero(#[case] tif: TimeInForce) {
        // Lighter requires `0` for IOC semantics; -1 is rejected as an
        // invalid expiry timestamp by the sequencer.
        assert_eq!(
            order_expiry_for(OrderType::Limit, &tif, None, NOW_MS).unwrap(),
            ORDER_EXPIRY_IOC
        );
    }

    #[rstest]
    fn order_expiry_for_market_orders_returns_zero() {
        assert_eq!(
            order_expiry_for(OrderType::Market, &TimeInForce::Gtc, None, NOW_MS).unwrap(),
            ORDER_EXPIRY_IOC
        );
    }

    #[rstest]
    #[case(OrderType::StopMarket)]
    #[case(OrderType::MarketIfTouched)]
    fn order_expiry_for_conditional_market_orders_uses_positive_expiry(
        #[case] order_type: OrderType,
    ) {
        assert_eq!(
            order_expiry_for(order_type, &TimeInForce::Gtc, None, NOW_MS).unwrap(),
            NOW_MS + ORDER_EXPIRY_DEFAULT_GTC_MS,
        );
    }

    #[rstest]
    #[case(OrderType::StopMarket)]
    #[case(OrderType::MarketIfTouched)]
    fn order_expiry_for_conditional_market_gtd_with_expiry_returns_millis(
        #[case] order_type: OrderType,
    ) {
        let expiry_ms = NOW_MS + ORDER_EXPIRY_MIN_GTD_MS + 456;
        let ts = UnixNanos::from((expiry_ms as u64) * 1_000_000);
        assert_eq!(
            order_expiry_for(order_type, &TimeInForce::Gtd, Some(ts), NOW_MS).unwrap(),
            expiry_ms,
        );
    }

    #[rstest]
    #[case(OrderType::StopLimit)]
    #[case(OrderType::LimitIfTouched)]
    fn order_expiry_for_conditional_limit_ioc_uses_positive_expiry(#[case] order_type: OrderType) {
        assert_eq!(
            order_expiry_for(order_type, &TimeInForce::Ioc, None, NOW_MS).unwrap(),
            NOW_MS + ORDER_EXPIRY_DEFAULT_GTC_MS,
        );
    }

    fn position_at(instrument: &str) -> PositionStatusReport {
        stub_position_report(instrument, "1")
    }

    #[rstest]
    #[case::empty_to_single(&[], &["ETH-PERP.LIGHTER"], &[])]
    #[case::one_removed(
        &["ETH-PERP.LIGHTER", "BTC-PERP.LIGHTER"],
        &["ETH-PERP.LIGHTER"],
        &["BTC-PERP.LIGHTER"],
    )]
    #[case::all_closed(&["ETH-PERP.LIGHTER"], &[], &["ETH-PERP.LIGHTER"])]
    #[case::two_removed(
        &["ETH-PERP.LIGHTER", "BTC-PERP.LIGHTER", "DOGE-PERP.LIGHTER"],
        &["DOGE-PERP.LIGHTER"],
        &["BTC-PERP.LIGHTER", "ETH-PERP.LIGHTER"],
    )]
    #[case::full_swap(
        &["ETH-PERP.LIGHTER"],
        &["BTC-PERP.LIGHTER"],
        &["ETH-PERP.LIGHTER"],
    )]
    fn replace_positions_returns_removed_ids(
        #[case] prior: &[&str],
        #[case] next: &[&str],
        #[case] expected_removed: &[&str],
    ) {
        // Pins the contract the consumption loop relies on to emit flat
        // PositionStatusReports for externally-closed positions.
        // Regression to `Vec::new()` would silently swallow closures.
        let state = WsDispatchState::new();
        let prior_reports: Vec<PositionStatusReport> =
            prior.iter().map(|i| position_at(i)).collect();
        state.replace_positions(&prior_reports);

        let next_reports: Vec<PositionStatusReport> = next.iter().map(|i| position_at(i)).collect();
        let mut removed = state.replace_positions(&next_reports);
        removed.sort();
        let mut expected: Vec<InstrumentId> = expected_removed
            .iter()
            .map(|i| InstrumentId::from(*i))
            .collect();
        expected.sort();

        assert_eq!(removed, expected);
    }

    #[rstest]
    fn derive_client_order_index_fits_in_31_bits() {
        // Venue rejects values above 2^31-1 with `21727 invalid client
        // order index`. Property-style: derive a wide range of distinct
        // cloids and assert each result stays inside the venue-safe
        // window. A mask widening regression would fail here even with
        // a single value out of bounds.
        let state = WsDispatchState::new();
        for n in 0..512u32 {
            let cid = ClientOrderId::new(format!("ORDER-{n}").as_str());
            let derived = state.derive_client_order_index(&cid);
            assert!(derived >= 0, "negative derived index: {derived}");
            assert!(
                derived <= 0x7FFF_FFFF,
                "index {derived} exceeds 31-bit venue cap",
            );
        }
    }

    #[rstest]
    fn quantity_to_ticks_scales_by_decimals() {
        let qty = Quantity::from("0.1336");
        assert_eq!(quantity_to_ticks(&qty, 4).unwrap(), 1_336);
    }

    // Sanity check: an i64-overflowing quantity must surface the i64-stage
    // error rather than silently truncating. The Decimal multiplication
    // pushes well past i64::MAX so `decimal_trunc_to_i64` short-circuits.
    // A precision-0 quantity at 1e10 is well inside the Nautilus
    // `Quantity` raw cap (~3.4e29 with high-precision), but scaling by
    // `10^16` pushes it to 1e26 - far past i64::MAX (~9.22e18). The
    // wrapping `with_context` must surface the typed overflow message
    // rather than silently truncating.
    #[rstest]
    fn quantity_to_ticks_rejects_i64_overflow() {
        let qty = Quantity::from_decimal_dp(Decimal::from(10_000_000_000_i64), 0).unwrap();
        let err = quantity_to_ticks(&qty, 16).unwrap_err();
        assert!(
            err.to_string().contains("overflows i64"),
            "expected i64 overflow error, was: {err}",
        );
    }

    #[rstest]
    fn price_to_ticks_scales_by_decimals() {
        let price = Price::from("2352.74");
        assert_eq!(price_to_ticks(&price, 2).unwrap(), 235_274);
    }

    #[rstest]
    fn price_to_ticks_rejects_overflow_above_u32() {
        let price = Price::from("100000000.00");
        let err = price_to_ticks(&price, 2).unwrap_err();
        assert!(err.to_string().contains("overflows u32"));
    }

    // Pins `decimal_trunc_to_i64` semantics directly so the helper's trunc
    // (toward zero) and overflow contract is asserted independently of any
    // caller that happens to feed it integer-valued Decimals.
    #[rstest]
    #[case::positive_fractional_truncs_toward_zero("3.9", 3)]
    #[case::negative_fractional_truncs_toward_zero("-3.9", -3)]
    #[case::integer_passes_through("42", 42)]
    #[case::zero("0", 0)]
    fn decimal_trunc_to_i64_truncates_toward_zero(#[case] input: &str, #[case] expected: i64) {
        let d = Decimal::from_str(input).unwrap();
        assert_eq!(decimal_trunc_to_i64(d).unwrap(), expected);
    }

    #[rstest]
    fn decimal_trunc_to_i64_rejects_above_i64_max() {
        // i64::MAX is 9223372036854775807; 9.3e18 is above it.
        let d = Decimal::from_str("9300000000000000000").unwrap();
        let err = decimal_trunc_to_i64(d).unwrap_err();
        assert!(
            err.to_string().contains("does not fit in i64"),
            "expected i64 fit error, was: {err}",
        );
    }

    #[rstest]
    #[case::buy_widen(Decimal::new(10_000, 2), true, 2, 50, 10_050)]
    #[case::sell_widen(Decimal::new(10_000, 2), false, 2, 50, 9_950)]
    #[case::buy_ceil(Decimal::new(7_915_055, 2), true, 2, 1, 7_915_847)]
    #[case::sell_floor(Decimal::new(7_915_055, 2), false, 2, 1, 7_914_263)]
    #[case::zero_bps_buy(Decimal::new(123_456, 2), true, 2, 0, 123_456)]
    #[case::zero_bps_sell(Decimal::new(123_456, 2), false, 2, 0, 123_456)]
    fn derive_market_order_price_ticks_cases(
        #[case] base: Decimal,
        #[case] is_buy: bool,
        #[case] price_precision: u8,
        #[case] slippage_bps: u32,
        #[case] expected: u32,
    ) {
        let ticks =
            derive_market_order_price_ticks(base, is_buy, price_precision, slippage_bps).unwrap();
        assert_eq!(ticks, expected);
    }

    #[rstest]
    #[case::excess_sell_slippage(Decimal::new(10_000, 2), false, 2, 10_000)]
    #[case::underflow_at_precision(Decimal::new(5, 6), false, 4, 50)]
    fn derive_market_order_price_ticks_rejects_zero_cap(
        #[case] base: Decimal,
        #[case] is_buy: bool,
        #[case] price_precision: u8,
        #[case] slippage_bps: u32,
    ) {
        let err = derive_market_order_price_ticks(base, is_buy, price_precision, slippage_bps)
            .unwrap_err();
        assert!(err.to_string().contains("rounds to 0 ticks"));
    }

    fn stub_pending_create(
        client_order_id: &str,
        nonce: i64,
        submitted_at_ns: u64,
    ) -> PendingSendTx {
        use nautilus_model::orders::builder::OrderTestBuilder;

        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(InstrumentId::from("ETH-PERP.LIGHTER"))
            .client_order_id(ClientOrderId::new(client_order_id))
            .quantity(Quantity::from("0.01"))
            .build();
        PendingSendTx {
            kind: PendingSendTxKind::Create {
                order: Box::new(order),
                client_order_index: nonce,
            },
            submitted_at: UnixNanos::from(submitted_at_ns),
            nonce,
            api_key_index: 0,
            tx_hash: format!("hash{nonce:02x}"),
        }
    }

    fn stub_pending_other(nonce: i64, submitted_at_ns: u64) -> PendingSendTx {
        PendingSendTx {
            kind: PendingSendTxKind::Other,
            submitted_at: UnixNanos::from(submitted_at_ns),
            nonce,
            api_key_index: 0,
            tx_hash: format!("hash{nonce:02x}"),
        }
    }

    fn pending_cloid(p: &PendingSendTx) -> Option<ClientOrderId> {
        match &p.kind {
            PendingSendTxKind::Create { order, .. } => Some(order.client_order_id()),
            PendingSendTxKind::Cancel {
                client_order_id, ..
            }
            | PendingSendTxKind::Modify {
                client_order_id, ..
            } => Some(*client_order_id),
            PendingSendTxKind::Other => None,
        }
    }

    #[rstest]
    fn enqueue_then_pop_head_is_fifo_across_kinds() {
        // Pins FIFO order across mixed kinds: cancel/modify/leverage entries
        // share the queue with creates so the venue ACK order is preserved.
        // Without this, a non-create ACK would pop a pending create and the
        // real create rejection would land unattributed.
        let state = WsDispatchState::new();
        let now = UnixNanos::from(1_000_000_000);

        state.enqueue_pending_sendtx(stub_pending_create("A", 10, now.as_u64()));
        state.enqueue_pending_sendtx(stub_pending_other(11, now.as_u64() + 1));
        state.enqueue_pending_sendtx(stub_pending_create("B", 12, now.as_u64() + 2));

        let first = state.pop_pending_sendtx_head().expect("head present");
        assert_eq!(pending_cloid(&first), Some(cloid("A")));
        let second = state.pop_pending_sendtx_head().expect("second present");
        assert!(matches!(second.kind, PendingSendTxKind::Other));
        let third = state.pop_pending_sendtx_head().expect("third present");
        assert_eq!(pending_cloid(&third), Some(cloid("B")));
        assert!(state.pop_pending_sendtx_head().is_none());
    }

    #[rstest]
    fn pop_within_window_attributes_only_recent_head() {
        let state = WsDispatchState::new();
        let submitted_ns = 1_000_000_000_u64;
        state.enqueue_pending_sendtx(stub_pending_create("A", 1, submitted_ns));

        let within = UnixNanos::from(submitted_ns + 500 * 1_000_000);
        assert!(
            state
                .pop_pending_sendtx_if_only_within(within, 1_000)
                .is_some(),
        );

        state.enqueue_pending_sendtx(stub_pending_create("B", 2, submitted_ns));
        let outside = UnixNanos::from(submitted_ns + 1_500 * 1_000_000);
        assert!(
            state
                .pop_pending_sendtx_if_only_within(outside, 1_000)
                .is_none(),
            "outside the attribution window the head must not pop",
        );
        assert_eq!(state.pending_sendtx_len(), 1, "head must remain queued");
    }

    #[rstest]
    fn enqueue_does_not_prune_stale_entries() {
        // A stale head must be preserved so a late ACK / rejection still pops
        // the entry it belongs to.
        let state = WsDispatchState::new();
        state.enqueue_pending_sendtx(stub_pending_create("stale", 1, 0));
        state.enqueue_pending_sendtx(stub_pending_create("fresh", 2, 600_000 * 1_000_000));

        assert_eq!(
            state.pending_sendtx_len(),
            2,
            "stale head must be preserved"
        );
        let head = state.pop_pending_sendtx_head().expect("stale at head");
        assert_eq!(pending_cloid(&head), Some(cloid("stale")));
    }

    #[rstest]
    fn remove_pending_by_nonce_targets_the_matching_entry() {
        // Nonce-based removal works regardless of kind (cancel/modify have
        // no cloid to remove by; only the captured nonce is unique).
        let state = WsDispatchState::new();
        let now = UnixNanos::from(1_000_000_000);
        state.enqueue_pending_sendtx(stub_pending_create("A", 10, now.as_u64()));
        state.enqueue_pending_sendtx(stub_pending_other(11, now.as_u64() + 1));

        let removed = state
            .remove_pending_sendtx_by_nonce(11)
            .expect("nonce 11 removed");
        assert!(matches!(removed.kind, PendingSendTxKind::Other));
        assert_eq!(removed.nonce, 11);
        assert_eq!(state.pending_sendtx_len(), 1);

        let head = state.pop_pending_sendtx_head().expect("A still queued");
        assert_eq!(pending_cloid(&head), Some(cloid("A")));
    }

    #[rstest]
    fn remove_pending_by_hash_targets_the_matching_entry() {
        // Hash removal must attribute regardless of queue position so an
        // out-of-order venue response never pops the wrong entry.
        let state = WsDispatchState::new();
        let now = UnixNanos::from(1_000_000_000);
        state.enqueue_pending_sendtx(stub_pending_create("A", 10, now.as_u64()));
        state.enqueue_pending_sendtx(stub_pending_other(11, now.as_u64() + 1));
        state.enqueue_pending_sendtx(stub_pending_create("B", 12, now.as_u64() + 2));

        let removed = state
            .remove_pending_sendtx_by_hash("hash0b")
            .expect("hash for nonce 11 removed");
        assert_eq!(removed.nonce, 11);
        assert_eq!(state.pending_sendtx_len(), 2);

        assert!(
            state.remove_pending_sendtx_by_hash("hash0b").is_none(),
            "removed hash must not match again",
        );

        let upper = state
            .remove_pending_sendtx_by_hash("0xHASH0C")
            .expect("hash comparison is case-insensitive and prefix-tolerant");
        assert_eq!(pending_cloid(&upper), Some(cloid("B")));

        let head = state.pop_pending_sendtx_head().expect("A still queued");
        assert_eq!(pending_cloid(&head), Some(cloid("A")));
    }

    #[rstest]
    fn drain_pending_returns_all_entries_in_order() {
        let state = WsDispatchState::new();
        let now = UnixNanos::from(1_000_000_000);
        state.enqueue_pending_sendtx(stub_pending_create("A", 10, now.as_u64()));
        state.enqueue_pending_sendtx(stub_pending_other(11, now.as_u64() + 1));

        let drained = state.drain_pending_sendtx();

        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].nonce, 10);
        assert_eq!(drained[1].nonce, 11);
        assert_eq!(state.pending_sendtx_len(), 0);
        assert!(state.drain_pending_sendtx().is_empty());
    }

    #[rstest]
    fn account_streams_ready_starts_pending() {
        let ready = AccountStreamsReady::new();
        assert!(!ready.all_ready());
        assert_eq!(
            ready.pending(),
            vec!["orders", "trades", "positions", "assets", "user_stats"]
        );
    }

    #[rstest]
    fn account_streams_ready_all_marked_is_ready() {
        let ready = AccountStreamsReady::new();
        ready.mark_orders();
        ready.mark_trades();
        ready.mark_positions();
        ready.mark_assets();
        ready.mark_user_stats();
        assert!(ready.all_ready());
        assert!(ready.pending().is_empty());
    }

    #[rstest]
    fn account_streams_ready_partial_marks_keep_pending_list() {
        let ready = AccountStreamsReady::new();
        ready.mark_orders();
        ready.mark_positions();
        assert!(!ready.all_ready());
        assert_eq!(ready.pending(), vec!["trades", "assets", "user_stats"]);
    }

    #[tokio::test]
    async fn account_streams_ready_await_all_returns_when_all_marked() {
        let ready = AccountStreamsReady::new();
        ready.mark_orders();
        ready.mark_trades();
        ready.mark_positions();
        ready.mark_assets();
        ready.mark_user_stats();
        ready
            .await_all(Duration::from_millis(50))
            .await
            .expect("await_all should return immediately when all flags are set");
    }

    #[tokio::test]
    async fn account_streams_ready_await_all_wakes_when_streams_arrive() {
        // Pins the Notify wiring: marks landing after await_all has parked
        // must wake the waiter rather than wait for the next 5s tick.
        let ready = std::sync::Arc::new(AccountStreamsReady::new());
        let producer = std::sync::Arc::clone(&ready);

        let waker = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            producer.mark_orders();
            producer.mark_trades();
            producer.mark_positions();
            producer.mark_assets();
            producer.mark_user_stats();
        });

        ready
            .await_all(Duration::from_secs(2))
            .await
            .expect("await_all should observe the marks");
        waker.await.unwrap();
    }

    #[tokio::test]
    async fn account_streams_ready_await_all_times_out_with_pending_list() {
        let ready = AccountStreamsReady::new();
        ready.mark_orders();
        ready.mark_trades();

        let err = ready
            .await_all(Duration::from_millis(20))
            .await
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("positions"),
            "should list pending streams: {msg}"
        );
        assert!(msg.contains("assets"), "should list pending streams: {msg}");
    }

    #[rstest]
    fn account_streams_ready_mark_is_idempotent() {
        let ready = AccountStreamsReady::new();
        ready.mark_orders();
        // Second call must not panic and must not change readiness state.
        ready.mark_orders();
        assert!(!ready.all_ready());
        assert!(!ready.pending().contains(&"orders"));
    }

    #[rstest]
    fn clear_position_cache_drops_entries_without_emitting() {
        // Pins the connect-time clear used to keep stale prior-session
        // positions from leaking past the strict-await gate when the
        // venue's initial `account_all_positions` frame is empty.
        let state = WsDispatchState::new();
        state.replace_positions(&[stub_position_report("ETH-PERP.LIGHTER", "1.0")]);
        assert!(!state.snapshot_positions(None).is_empty());

        state.clear_position_cache();

        assert!(state.snapshot_positions(None).is_empty());
    }

    #[rstest]
    fn clear_account_state_cache_drops_snapshot() {
        // Pins the connect-time clear that prevents a stale account state
        // from satisfying `query_account` after the new session's initial
        // assets frame failed to parse.
        let state = WsDispatchState::new();
        let account_state = AccountState::new(
            AccountId::from("LIGHTER-TEST"),
            AccountType::Margin,
            vec![],
            vec![],
            true,
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(2),
            None,
        );
        state.cache_account_state(account_state);
        assert!(state.snapshot_account_state().is_some());

        state.clear_account_state_cache();

        assert!(state.snapshot_account_state().is_none());
    }

    #[rstest]
    fn account_streams_ready_reset_clears_flags() {
        // Pins the contract `connect()` relies on for retry / reconnect:
        // a fully-marked handle must clear back to pending so a new WS
        // session does not short-circuit the gate with stale state.
        let ready = AccountStreamsReady::new();
        ready.mark_orders();
        ready.mark_trades();
        ready.mark_positions();
        ready.mark_assets();
        ready.mark_user_stats();
        assert!(ready.all_ready());

        ready.reset();
        assert!(!ready.all_ready());
        assert_eq!(
            ready.pending(),
            vec!["orders", "trades", "positions", "assets", "user_stats"]
        );
    }
}
