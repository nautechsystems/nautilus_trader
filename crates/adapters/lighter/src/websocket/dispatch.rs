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
    hash::{BuildHasher, Hash, Hasher},
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use ahash::{AHashMap, RandomState};
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

/// Sentinel used in `OrderInfo.order_expiry` for IOC orders, per the Lighter
/// Go signer's documented contract: `0` means "no expiry tracking, IOC
/// semantics".
pub(crate) const ORDER_EXPIRY_IOC: i64 = 0;

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
}

/// In-flight sendTx awaiting a venue response.
///
/// Every signed sendTx (create, cancel, modify, update_leverage) enqueues an
/// entry so the queue's FIFO order matches the venue's ACK order. The `kind`
/// records whether the entry has an originating Nautilus order that should
/// receive a typed `OrderRejected` on a venue rejection.
#[derive(Debug, Clone)]
pub(crate) struct PendingSendTx {
    pub(crate) kind: PendingSendTxKind,
    pub(crate) submitted_at: UnixNanos,
    pub(crate) nonce: i64,
    pub(crate) api_key_index: u8,
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
    /// Cancel, modify, or update-leverage submit. Tracked for FIFO alignment
    /// so the venue's ACK or rejection pops the correct head; the consumption
    /// loop logs but does not emit a typed event for these yet.
    Other,
}

/// Max probing attempts when [`WsDispatchState::register_cloid`] detects a
/// collision on the derived `client_order_index`. The 31-bit space makes a
/// collision improbable at session scale; bounding the probe ensures that a
/// degenerate seed never spins indefinitely while still leaving headroom.
const CLOID_INDEX_PROBE_LIMIT: usize = 16;

/// Maximum entries held by lifecycle dedup caches before the oldest marker is
/// evicted.
const DEDUP_CAPACITY: usize = 10_000;

/// Bounded deduplication set with FIFO eviction.
#[derive(Debug)]
pub(crate) struct BoundedDedup<K> {
    inner: Mutex<BoundedDedupInner<K>>,
    capacity: usize,
}

#[derive(Debug)]
struct BoundedDedupInner<K> {
    set: AHashMap<K, u64>,
    queue: VecDeque<(K, u64)>,
    next_seq: u64,
}

impl<K> BoundedDedup<K>
where
    K: Eq + Hash + Clone,
{
    /// Creates a new dedup set with the given maximum capacity.
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            inner: Mutex::new(BoundedDedupInner {
                set: AHashMap::with_capacity(capacity),
                queue: VecDeque::with_capacity(capacity),
                next_seq: 0,
            }),
            capacity,
        }
    }

    /// Returns `true` when the key is present.
    pub(crate) fn contains(&self, key: &K) -> bool {
        self.inner
            .lock()
            .expect(MUTEX_POISONED)
            .set
            .contains_key(key)
    }

    /// Inserts the key and evicts old markers when the cache is full.
    pub(crate) fn insert(&self, key: K) {
        let mut inner = self.inner.lock().expect(MUTEX_POISONED);
        if inner.set.contains_key(&key) {
            return;
        }

        let seq = inner.next_seq;
        inner.next_seq = inner.next_seq.wrapping_add(1);
        inner.set.insert(key.clone(), seq);
        inner.queue.push_back((key, seq));

        while inner.queue.len() > self.capacity
            && let Some((old_key, old_seq)) = inner.queue.pop_front()
        {
            if inner.set.get(&old_key) == Some(&old_seq) {
                inner.set.remove(&old_key);
            }
        }
    }

    /// Removes the key when present.
    pub(crate) fn remove(&self, key: &K) {
        self.inner.lock().expect(MUTEX_POISONED).set.remove(key);
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
    /// Cloids for which an `OrderAccepted` event has already been emitted on
    /// the live path. Drives the modify-as-restate branch: a subsequent
    /// venue `Open` for the same cloid emits `OrderUpdated` rather than
    /// re-emitting `OrderAccepted`. Also lets `ensure_accepted_emitted`
    /// synthesise an `OrderAccepted` for fast-filling orders that skip the
    /// `Open` state. Report generation also seeds this marker when
    /// reconciliation can accept a submitted tracked order before the typed
    /// WebSocket path sees it.
    pub(crate) accepted_emitted: Arc<BoundedDedup<ClientOrderId>>,
    /// Trade ids already routed to `OrderFilled` / `FillReport`. The venue
    /// can re-emit the same `account_all_trades` payload across reconnects
    /// and HTTP reconciliation does not backfill the dedup state, so a
    /// process-lifetime set keeps duplicate fills from double-booking.
    pub(crate) seen_trade_ids: Arc<DashSet<TradeId>>,
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
    notify: tokio::sync::Notify,
}

impl AccountStreamsReady {
    pub(crate) fn new() -> Self {
        Self {
            orders: AtomicBool::new(false),
            trades: AtomicBool::new(false),
            positions: AtomicBool::new(false),
            assets: AtomicBool::new(false),
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
            venue_id_map: Arc::new(DashMap::new()),
            nonce_manager: Arc::new(NonceManager::default()),
            last_account_state: Arc::new(Mutex::new(None)),
            active_markets: Arc::new(DashSet::new()),
            last_positions: Arc::new(Mutex::new(AHashMap::new())),
            order_identities: Arc::new(DashMap::new()),
            accepted_emitted: Arc::new(BoundedDedup::new(DEDUP_CAPACITY)),
            seen_trade_ids: Arc::new(DashSet::new()),
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

    /// Pop the oldest pending entry unconditionally. Use for `SendTxAck`
    /// (success or non-200): both are direct responses to our own request
    /// and are always attributable to the head.
    pub(crate) fn pop_pending_sendtx_head(&self) -> Option<PendingSendTx> {
        self.pending_sendtx
            .lock()
            .expect(MUTEX_POISONED)
            .pop_front()
    }

    /// Pop the head only if its submitted_at is within `max_age_ms` of `now`.
    /// Used for bare-error frames: an error arriving long after the head was
    /// submitted is unlikely to belong to it; let the submit-timeout handle.
    pub(crate) fn pop_pending_sendtx_within(
        &self,
        now: UnixNanos,
        max_age_ms: u64,
    ) -> Option<PendingSendTx> {
        let cutoff_ns = now.as_u64().saturating_sub(max_age_ms * 1_000_000);
        let mut q = self.pending_sendtx.lock().expect(MUTEX_POISONED);
        match q.front() {
            Some(front) if front.submitted_at.as_u64() >= cutoff_ns => q.pop_front(),
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
        self.accepted_emitted.remove(cloid);
        self.triggered_emitted.remove(cloid);
        self.order_snapshots.remove(cloid);
    }

    /// Returns `true` if an `OrderAccepted` has already been emitted for
    /// `cloid` on the live path. Drives the modify-as-restate branch and
    /// the `ensure_accepted_emitted` synthesis.
    pub(crate) fn accepted_was_emitted(&self, cloid: &ClientOrderId) -> bool {
        self.accepted_emitted.contains(cloid)
    }

    /// Record that an `OrderAccepted` has been emitted for `cloid`.
    pub(crate) fn mark_accepted_emitted(&self, cloid: ClientOrderId) {
        self.accepted_emitted.insert(cloid);
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

        if self.order_identities.contains_key(&cloid) {
            self.mark_accepted_emitted(cloid);
        }
    }

    /// First-time check for a trade id: returns `true` if `trade_id` is new
    /// and inserts it, returns `false` if it was already routed.
    pub(crate) fn mark_trade_seen(&self, trade_id: TradeId) -> bool {
        self.seen_trade_ids.insert(trade_id)
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
    pub(crate) fn register_cloid(&self, index: i64, cloid: ClientOrderId) -> i64 {
        let mut candidate = index;
        for attempt in 0..=CLOID_INDEX_PROBE_LIMIT {
            match self.cloid_map.entry(candidate) {
                dashmap::mapref::entry::Entry::Vacant(entry) => {
                    entry.insert(cloid);

                    if attempt > 0 {
                        log::warn!(
                            "Lighter client_order_index collision at {index}: \
                             cloid {cloid} re-derived to {candidate} after {attempt} probe(s)",
                        );
                    }
                    return candidate;
                }
                dashmap::mapref::entry::Entry::Occupied(entry) => {
                    if *entry.get() == cloid {
                        return candidate;
                    }
                    candidate = next_probe_index(candidate);
                }
            }
        }
        // Probing exhausted: the venue-safe window has many free slots but
        // we have hit a degenerate pile-up. Overwrite the slot rather than
        // dropping the submit, and surface a loud warn so a real incident
        // is investigable.
        log::warn!(
            "Lighter client_order_index probe exhausted after {CLOID_INDEX_PROBE_LIMIT} attempts: \
             overwriting slot {candidate} with cloid {cloid}",
        );
        self.cloid_map.insert(candidate, cloid);
        candidate
    }

    /// Drop a cloid registration (called from the spawn's error branch when
    /// the tx never reaches the wire).
    pub(crate) fn forget_cloid(&self, index: i64) {
        self.cloid_map.remove(&index);
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
        let mut guard = self.last_positions.lock().expect(MUTEX_POISONED);
        let new_ids: ahash::AHashSet<InstrumentId> =
            snapshot.iter().map(|r| r.instrument_id).collect();
        let removed: Vec<InstrumentId> = guard
            .keys()
            .filter(|id| !new_ids.contains(id))
            .copied()
            .collect();
        guard.clear();
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

/// Translate the venue's i64-string echo back to the originating Nautilus
/// cloid, when the index is one we registered at submit time.
pub(crate) fn translate_order_cloid(
    mut report: OrderStatusReport,
    cloid_map: &Arc<DashMap<i64, ClientOrderId>>,
) -> OrderStatusReport {
    if let Some(cloid_str) = report.client_order_id.as_ref()
        && let Ok(index) = cloid_str.as_str().parse::<i64>()
        && let Some(entry) = cloid_map.get(&index)
    {
        report = report.with_client_order_id(*entry.value());
    }
    report
}

/// Resolve the originating Nautilus [`ClientOrderId`] for a venue-echoed
/// client id string, applying the `cloid_map` reverse-translation.
///
/// Returns `None` for empty / sentinel `"0"` values (the venue's placeholder
/// for an external order that did not carry a `client_order_index`).
/// Returns the mapped Nautilus cloid when the string parses to an `i64`
/// known to [`WsDispatchState::cloid_map`]. Otherwise wraps the raw string
/// as a [`ClientOrderId`] so untracked / externally-managed orders still
/// surface a stable identifier on the report.
pub(crate) fn resolve_cloid(
    raw: &str,
    cloid_map: &Arc<DashMap<i64, ClientOrderId>>,
) -> Option<ClientOrderId> {
    if raw.is_empty() || raw == "0" {
        return None;
    }

    if let Ok(index) = raw.parse::<i64>()
        && let Some(entry) = cloid_map.get(&index)
    {
        return Some(*entry.value());
    }

    Some(ClientOrderId::new(raw))
}

/// Translate the venue-side `client_order_index` echo on a [`FillReport`]
/// back to the originating Nautilus cloid, mirroring [`translate_order_cloid`].
///
/// Lighter exposes the same numeric `client_order_index` in the trade
/// payload's bid/ask client ids that order reports expose. Without
/// translation, a fill that races ahead of the matching order accept is
/// reconciled against the numeric id and would surface as an external order
/// rather than the original [`ClientOrderId`].
pub(crate) fn translate_fill_cloid(
    mut report: FillReport,
    cloid_map: &Arc<DashMap<i64, ClientOrderId>>,
) -> FillReport {
    if let Some(cloid_str) = report.client_order_id.as_ref()
        && let Ok(index) = cloid_str.as_str().parse::<i64>()
        && let Some(entry) = cloid_map.get(&index)
    {
        report.client_order_id = Some(*entry.value());
    }
    report
}

/// Drops the `ClientOrderId → VenueOrderId` mapping for an order that has
/// reached a terminal status, since cancel/modify can no longer act on it.
///
/// `cloid_map` is intentionally NOT evicted here. A terminal-status frame
/// (`Filled` in particular) can land before the matching `account_all_trades`
/// frame; if we drop the cloid → numeric-index mapping at terminal time, the
/// trailing fill loses its Nautilus cloid and surfaces as an external order.
/// The `cloid_map` continues to grow with one entry per submitted order; for
/// long-running sessions that is bounded by submission rate × session
/// length and can be capped by an LRU policy in a follow-up.
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
    cloid_map: &Arc<DashMap<i64, ClientOrderId>>,
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
        Ok(report) => Some(translate_order_cloid(report, cloid_map)),
        Err(e) => {
            log::warn!(
                "parse_http_order_to_report: parse failed for order_index={}: {e}",
                order.order_index,
            );
            None
        }
    }
}

/// Look up a single order via the active+inactive HTTP endpoints, returning
/// the corresponding [`OrderStatusReport`] if found.
///
/// Resolution order: explicit `venue_order_id` > cached `venue_id_map` >
/// derived `client_order_index` from `dispatch.derive_client_order_index`.
/// The third path is what makes `query_order` work between submission and
/// the venue's first `account_*` ack (when `venue_id_map` is still empty).
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
    let target_client_index: Option<i64> =
        client_order_id.map(|cloid| dispatch.derive_client_order_index(cloid));

    let matches_order = |o: &LighterOrder| -> bool {
        if let Some(voi) = target_venue_index
            && o.order_index == voi
        {
            return true;
        }

        if let Some(client_index) = target_client_index
            && o.client_order_index == client_index
        {
            return true;
        }

        false
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
        let mut report =
            parse_http_order_to_report(order, registry, account_id, ts_init, &dispatch.cloid_map)?;
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

    for order in &active.orders {
        if matches_order(order)
            && let Some(report) = finalize(order)
        {
            return Ok(Some(report));
        }
    }

    // Fall back to inactive orders (filled / canceled). Pagination is followed
    // because a single market can hold more than 200 historical inactive
    // orders for a long-running account.
    let mut cursor: Option<String> = None;

    loop {
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
            Some(next) if !next.is_empty() => cursor = Some(next),
            _ => break,
        }
    }

    Ok(None)
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
/// - `Gtd` with an explicit expire_time: the millisecond timestamp.
/// - `Ioc` / `Fok`: `ORDER_EXPIRY_IOC` (`0`): Lighter requires this exact
///   value for IOC semantics; any other value is rejected by the sequencer.
/// - `Gtc` / `Day` / `Gtd` without expiry: `now_ms + ORDER_EXPIRY_DEFAULT_GTC_MS`.
///   The venue rejects `-1` for these TIFs with `21711 invalid expiry`.
pub(crate) fn order_expiry_for(
    order_type: OrderType,
    tif: &TimeInForce,
    expire_time: Option<UnixNanos>,
    now_ms: i64,
) -> i64 {
    if order_type == OrderType::Market {
        return ORDER_EXPIRY_IOC;
    }

    if matches!(tif, TimeInForce::Gtd)
        && let Some(ts) = expire_time
    {
        return (ts.as_u64() / 1_000_000) as i64;
    }

    if is_conditional_order(order_type) && matches!(tif, TimeInForce::Ioc) {
        return now_ms.saturating_add(ORDER_EXPIRY_DEFAULT_GTC_MS);
    }

    if matches!(tif, TimeInForce::Ioc | TimeInForce::Fok) {
        return ORDER_EXPIRY_IOC;
    }

    now_ms.saturating_add(ORDER_EXPIRY_DEFAULT_GTC_MS)
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

        let chosen = state.register_cloid(derived, cid);

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

        let first = state.register_cloid(derived, cid);
        let second = state.register_cloid(derived, cid);

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

        let chosen = state.register_cloid(collision_index, second_cid);

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
    fn resolve_cloid_returns_mapped_cloid_for_known_index() {
        let map: Arc<DashMap<i64, ClientOrderId>> = Arc::new(DashMap::new());
        let original = cloid("MY-ORDER-001");
        map.insert(42, original);

        let resolved = resolve_cloid("42", &map);

        assert_eq!(resolved, Some(original));
    }

    #[rstest]
    fn resolve_cloid_returns_none_for_empty_or_zero() {
        let map: Arc<DashMap<i64, ClientOrderId>> = Arc::new(DashMap::new());

        assert_eq!(resolve_cloid("", &map), None);
        assert_eq!(resolve_cloid("0", &map), None);
    }

    #[rstest]
    fn resolve_cloid_wraps_unmapped_string_as_external_cloid() {
        // An external order (numeric index we don't recognise, or a
        // non-numeric cloid from a third party) should still surface a
        // ClientOrderId so reconciliation can route it.
        let map: Arc<DashMap<i64, ClientOrderId>> = Arc::new(DashMap::new());

        assert_eq!(
            resolve_cloid("9999", &map),
            Some(ClientOrderId::new("9999"))
        );
        assert_eq!(
            resolve_cloid("ext-order", &map),
            Some(ClientOrderId::new("ext-order")),
        );
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
    fn bounded_dedup_evicts_oldest_marker() {
        let dedup = BoundedDedup::new(2);

        dedup.insert(1);
        dedup.insert(2);
        dedup.insert(3);

        assert!(!dedup.contains(&1));
        assert!(dedup.contains(&2));
        assert!(dedup.contains(&3));
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
            OrderIdentity {
                instrument_id: InstrumentId::from("ETH-PERP.LIGHTER"),
                strategy_id: StrategyId::new("S-T"),
                order_side: OrderSide::Buy,
                order_type: OrderType::Limit,
            },
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
            OrderIdentity {
                instrument_id: InstrumentId::from("ETH-PERP.LIGHTER"),
                strategy_id: StrategyId::new("S-T"),
                order_side: OrderSide::Buy,
                order_type: OrderType::Limit,
            },
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
            OrderIdentity {
                instrument_id: InstrumentId::from("ETH-PERP.LIGHTER"),
                strategy_id: StrategyId::new("S-T"),
                order_side: OrderSide::Buy,
                order_type: OrderType::Limit,
            },
        );

        let mut report = stub_open_order_status_report(cid.as_str());
        report.order_status = OrderStatus::Rejected;
        state.seed_accepted_from_report(&report);

        assert!(!state.accepted_was_emitted(&cid));
    }

    #[rstest]
    fn seed_accepted_from_report_marks_submitted_report() {
        let state = WsDispatchState::new();
        let cid = cloid("REPORT-SUBMITTED");
        state.register_order_identity(
            cid,
            OrderIdentity {
                instrument_id: InstrumentId::from("ETH-PERP.LIGHTER"),
                strategy_id: StrategyId::new("S-T"),
                order_side: OrderSide::Buy,
                order_type: OrderType::Limit,
            },
        );

        let mut report = stub_open_order_status_report(cid.as_str());
        report.order_status = OrderStatus::Submitted;
        state.seed_accepted_from_report(&report);

        assert!(state.accepted_was_emitted(&cid));
    }

    #[rstest]
    fn forget_order_identity_clears_snapshot_and_triggered() {
        // A terminal event must clear the snapshot and the triggered
        // dedup. The accepted marker is live-order state and leaves the
        // bounded cache at terminal cleanup.
        let state = WsDispatchState::new();
        let cid = cloid("TERMINAL-CLEANUP");
        let identity = OrderIdentity {
            instrument_id: InstrumentId::from("ETH-PERP.LIGHTER"),
            strategy_id: StrategyId::new("S-T"),
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
        };

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
        let identity = OrderIdentity {
            instrument_id: InstrumentId::from("ETH-PERP.LIGHTER"),
            strategy_id: StrategyId::new("S-T"),
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
        };

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
        let map: Arc<DashMap<i64, ClientOrderId>> = Arc::new(DashMap::new());
        let original = cloid("MY-ORDER-001");
        map.insert(42, original);

        let report = stub_open_order_status_report("42");
        let translated = translate_order_cloid(report, &map);

        assert_eq!(translated.client_order_id, Some(original));
    }

    #[rstest]
    fn translate_order_cloid_passes_through_unknown_index() {
        let map: Arc<DashMap<i64, ClientOrderId>> = Arc::new(DashMap::new());
        let report = stub_open_order_status_report("99");
        let translated = translate_order_cloid(report, &map);

        assert_eq!(
            translated.client_order_id.map(|c| c.to_string()),
            Some("99".to_string()),
        );
    }

    #[rstest]
    fn translate_order_cloid_passes_through_non_integer_cloid() {
        let map: Arc<DashMap<i64, ClientOrderId>> = Arc::new(DashMap::new());
        let report = stub_open_order_status_report("not-an-int");
        let translated = translate_order_cloid(report, &map);

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
    // client id that maps in `cloid_map` substitutes to the originating
    // Nautilus cloid; unmapped numerics and non-integer ids pass through.
    // Without these guards a fill that races ahead of the order accept
    // would surface the venue's numeric `client_order_index` rather than
    // the engine's `ClientOrderId`.
    #[rstest]
    fn translate_fill_cloid_substitutes_known_index() {
        let map: Arc<DashMap<i64, ClientOrderId>> = Arc::new(DashMap::new());
        let original = cloid("MY-ORDER-001");
        map.insert(42, original);

        let report = stub_fill_report("42");
        let translated = translate_fill_cloid(report, &map);

        assert_eq!(translated.client_order_id, Some(original));
    }

    #[rstest]
    fn translate_fill_cloid_passes_through_unknown_index() {
        let map: Arc<DashMap<i64, ClientOrderId>> = Arc::new(DashMap::new());
        let report = stub_fill_report("99");
        let translated = translate_fill_cloid(report, &map);

        assert_eq!(
            translated.client_order_id.map(|c| c.to_string()),
            Some("99".to_string()),
        );
    }

    #[rstest]
    fn translate_fill_cloid_passes_through_non_integer_cloid() {
        let map: Arc<DashMap<i64, ClientOrderId>> = Arc::new(DashMap::new());
        let report = stub_fill_report("not-an-int");
        let translated = translate_fill_cloid(report, &map);

        assert_eq!(
            translated.client_order_id.map(|c| c.to_string()),
            Some("not-an-int".to_string()),
        );
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
        let ts = UnixNanos::from(1_700_000_000_123_000_000u64);
        assert_eq!(
            order_expiry_for(OrderType::Limit, &TimeInForce::Gtd, Some(ts), NOW_MS),
            1_700_000_000_123
        );
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
            order_expiry_for(OrderType::Limit, &tif, expire, NOW_MS),
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
            order_expiry_for(OrderType::Limit, &tif, None, NOW_MS),
            ORDER_EXPIRY_IOC
        );
    }

    #[rstest]
    fn order_expiry_for_market_orders_returns_zero() {
        assert_eq!(
            order_expiry_for(OrderType::Market, &TimeInForce::Gtc, None, NOW_MS),
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
            order_expiry_for(order_type, &TimeInForce::Gtc, None, NOW_MS),
            NOW_MS + ORDER_EXPIRY_DEFAULT_GTC_MS,
        );
    }

    #[rstest]
    #[case(OrderType::StopMarket)]
    #[case(OrderType::MarketIfTouched)]
    fn order_expiry_for_conditional_market_gtd_with_expiry_returns_millis(
        #[case] order_type: OrderType,
    ) {
        let ts = UnixNanos::from(1_700_000_000_456_000_000u64);
        assert_eq!(
            order_expiry_for(order_type, &TimeInForce::Gtd, Some(ts), NOW_MS),
            1_700_000_000_456,
        );
    }

    #[rstest]
    #[case(OrderType::StopLimit)]
    #[case(OrderType::LimitIfTouched)]
    fn order_expiry_for_conditional_limit_ioc_uses_positive_expiry(#[case] order_type: OrderType) {
        assert_eq!(
            order_expiry_for(order_type, &TimeInForce::Ioc, None, NOW_MS),
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
    // `10^16` pushes it to 1e26 — far past i64::MAX (~9.22e18). The
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
        }
    }

    fn stub_pending_other(nonce: i64, submitted_at_ns: u64) -> PendingSendTx {
        PendingSendTx {
            kind: PendingSendTxKind::Other,
            submitted_at: UnixNanos::from(submitted_at_ns),
            nonce,
            api_key_index: 0,
        }
    }

    fn pending_cloid(p: &PendingSendTx) -> Option<ClientOrderId> {
        match &p.kind {
            PendingSendTxKind::Create { order, .. } => Some(order.client_order_id()),
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
        assert!(state.pop_pending_sendtx_within(within, 1_000).is_some());

        state.enqueue_pending_sendtx(stub_pending_create("B", 2, submitted_ns));
        let outside = UnixNanos::from(submitted_ns + 1_500 * 1_000_000);
        assert!(
            state.pop_pending_sendtx_within(outside, 1_000).is_none(),
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
    fn account_streams_ready_starts_pending() {
        let ready = AccountStreamsReady::new();
        assert!(!ready.all_ready());
        assert_eq!(
            ready.pending(),
            vec!["orders", "trades", "positions", "assets"]
        );
    }

    #[rstest]
    fn account_streams_ready_all_marked_is_ready() {
        let ready = AccountStreamsReady::new();
        ready.mark_orders();
        ready.mark_trades();
        ready.mark_positions();
        ready.mark_assets();
        assert!(ready.all_ready());
        assert!(ready.pending().is_empty());
    }

    #[rstest]
    fn account_streams_ready_partial_marks_keep_pending_list() {
        let ready = AccountStreamsReady::new();
        ready.mark_orders();
        ready.mark_positions();
        assert!(!ready.all_ready());
        assert_eq!(ready.pending(), vec!["trades", "assets"]);
    }

    #[tokio::test]
    async fn account_streams_ready_await_all_returns_when_all_marked() {
        let ready = AccountStreamsReady::new();
        ready.mark_orders();
        ready.mark_trades();
        ready.mark_positions();
        ready.mark_assets();
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
        assert!(ready.all_ready());

        ready.reset();
        assert!(!ready.all_ready());
        assert_eq!(
            ready.pending(),
            vec!["orders", "trades", "positions", "assets"]
        );
    }
}
