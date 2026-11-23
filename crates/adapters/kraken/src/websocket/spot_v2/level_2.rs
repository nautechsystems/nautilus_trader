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

//! Runtime state for Kraken Spot L2 book handling.

use std::sync::{
    Arc, Weak,
    atomic::{AtomicU64, Ordering},
};

use ahash::AHashMap;
use nautilus_common::live::dst::time::{Duration, Instant};
use nautilus_core::UnixNanos;
use nautilus_live::book::{
    BookSequenceOutcome, DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS,
    recovery::{BookRecovery, BookRecoveryOutcome},
};
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas},
    enums::{BookAction, BookType, RecordFlag},
    identifiers::InstrumentId,
    instruments::{Instrument, any::InstrumentAny},
    orderbook::{OrderBook, analysis::book_check_integrity},
};
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{
    checksum::{crc32_ieee, push_scaled},
    messages::KrakenWsBookData,
    parse::parse_book_deltas,
};
use crate::{
    book::sync::BookSyncTracker, common::consts::KRAKEN_PAIR_DECIMALS_KEY,
    websocket::error::KrakenWsError,
};

/// The current book subscription, keyed by venue symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct L2Subscription {
    pub(crate) depth: u32,
    pub(crate) generation: u64,
    pub(crate) latest_request: u64,
    pub(crate) snapshot_epoch: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct L2Depths {
    depths: Arc<Mutex<AHashMap<String, L2Subscription>>>,
    recoveries: Arc<Mutex<AHashMap<Ustr, Weak<BookRecovery<KrakenWsError>>>>>,
    next_generation: Arc<AtomicU64>,
}

impl Default for L2Depths {
    fn default() -> Self {
        Self {
            depths: Arc::new(Mutex::new(AHashMap::new())),
            recoveries: Arc::new(Mutex::new(AHashMap::new())),
            next_generation: Arc::new(AtomicU64::new(1)),
        }
    }
}

impl L2Depths {
    pub(crate) fn get(&self, symbol: &str) -> Option<u32> {
        self.subscription(symbol).map(|s| s.depth)
    }

    pub(crate) fn subscription(&self, symbol: &str) -> Option<L2Subscription> {
        self.depths.lock().get(symbol).copied()
    }

    /// Records a new subscription at `depth`, opened by the subscribe `request`, under a fresh
    /// generation, which it returns.
    pub(crate) fn insert(&self, symbol: &str, depth: u32, request: u64) -> u64 {
        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed);
        self.depths.lock().insert(
            symbol.to_string(),
            L2Subscription {
                depth,
                generation,
                latest_request: request,
                snapshot_epoch: 0,
            },
        );
        generation
    }

    pub(crate) fn begin_resync(
        &self,
        symbol: &str,
        generation: u64,
        epoch: u64,
        request: u64,
    ) -> Option<L2Subscription> {
        self.begin_resync_if(symbol, generation, epoch, request, || true)
    }

    pub(crate) fn begin_recovery(
        &self,
        symbol: Ustr,
        generation: u64,
        epoch: u64,
        request: u64,
        cancellation: &CancellationToken,
    ) -> Option<L2Subscription> {
        self.begin_resync_if(symbol.as_str(), generation, epoch, request, || {
            if cancellation.is_cancelled() {
                return false;
            }

            let recoveries = self.recoveries.lock();

            let Some(recovery) = recoveries
                .get(&symbol)
                .and_then(Weak::upgrade)
                .filter(|recovery| recovery.is_running())
            else {
                return false;
            };

            // A rejection can arrive during token refresh, before this request takes ownership.
            recovery.outcome.send_if_modified(|outcome| {
                if matches!(outcome, BookRecoveryOutcome::Rejected(_)) {
                    *outcome = BookRecoveryOutcome::Pending;
                    true
                } else {
                    false
                }
            });

            true
        })
    }

    fn begin_resync_if(
        &self,
        symbol: &str,
        generation: u64,
        epoch: u64,
        request: u64,
        should_replace: impl FnOnce() -> bool,
    ) -> Option<L2Subscription> {
        let mut depths = self.depths.lock();

        let held = depths.get_mut(symbol).filter(|held| {
            held.generation == generation && held.snapshot_epoch == epoch && should_replace()
        })?;

        held.latest_request = request;
        Some(*held)
    }

    pub(crate) fn accept_snapshot(
        &self,
        symbol: &str,
        request: u64,
        accept: impl FnOnce() -> bool,
    ) -> Option<L2Subscription> {
        let mut depths = self.depths.lock();
        let held = depths
            .get_mut(symbol)
            .filter(|held| held.latest_request == request)?;

        if !accept() {
            return None;
        }

        held.snapshot_epoch += 1;
        Some(*held)
    }

    pub(crate) fn retire_requests(&self, reserved_request: u64) {
        for held in self.depths.lock().values_mut() {
            held.snapshot_epoch += 1;
            held.latest_request = reserved_request;
        }
    }

    pub(crate) fn begin_replay(&self, symbol: &str, req_id: u64) -> Option<()> {
        self.depths.lock().get_mut(symbol)?.latest_request = req_id;
        Some(())
    }

    pub(crate) fn with_current_request<T>(
        &self,
        symbol: &str,
        req_id: u64,
        apply: impl FnOnce(L2Subscription) -> T,
    ) -> Option<T> {
        let depths = self.depths.lock();
        let sub = depths
            .get(symbol)
            .filter(|sub| sub.latest_request == req_id)?;
        Some(apply(*sub))
    }

    /// Every subscription currently held, by venue symbol.
    pub(crate) fn held(&self) -> Vec<(String, L2Subscription)> {
        self.depths
            .lock()
            .iter()
            .map(|(symbol, subscription)| (symbol.clone(), *subscription))
            .collect()
    }

    pub(crate) fn remove(&self, symbol: &str) {
        self.depths.lock().remove(symbol);
        if let Some(symbol) = Ustr::from_existing(symbol)
            && let Some(recovery) = self
                .recoveries
                .lock()
                .remove(&symbol)
                .and_then(|r| r.upgrade())
        {
            recovery.cancellation.cancel();
        }
    }

    pub(crate) fn clear(&self) {
        self.depths.lock().clear();

        for (_, recovery) in self.recoveries.lock().drain() {
            if let Some(recovery) = recovery.upgrade() {
                recovery.cancellation.cancel();
            }
        }
    }

    pub(crate) fn register_recovery(
        &self,
        symbol: Ustr,
        generation: u64,
        recovery: &Arc<BookRecovery<KrakenWsError>>,
    ) {
        let depths = self.depths.lock();
        if depths
            .get(symbol.as_str())
            .is_some_and(|sub| sub.generation == generation)
        {
            self.recoveries
                .lock()
                .insert(symbol, Arc::downgrade(recovery));
        } else {
            recovery.cancellation.cancel();
        }
    }

    pub(crate) fn confirm_recovery(&self, symbol: Ustr, req_id: u64) -> Option<L2Subscription> {
        let depths = self.depths.lock();
        let sub = depths
            .get(symbol.as_str())
            .filter(|sub| sub.latest_request == req_id)?;

        if let Some(recovery) = self.recoveries.lock().get(&symbol).and_then(Weak::upgrade)
            && recovery.is_running()
        {
            recovery.gate.open();
        }

        Some(*sub)
    }
}

/// Book subscription symbols keyed by the request IDs awaiting a venue acknowledgement.
pub(crate) type BookRequests = Arc<Mutex<AHashMap<u64, BookRequest>>>;

#[derive(Debug)]
pub(crate) struct BookRequest {
    pub(crate) symbol: Ustr,
    pub(crate) cancel: CancellationToken,
}

impl BookRequest {
    pub(crate) fn new(symbol: Ustr) -> Self {
        Self {
            symbol,
            cancel: CancellationToken::new(),
        }
    }
}

impl Drop for BookRequest {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

/// Identifies the subscription that requested recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct L2ResyncRequest {
    pub(crate) instrument_id: InstrumentId,
    pub(crate) generation: u64,
}

/// What processing one `book` message produced.
#[derive(Debug, Default)]
pub(crate) struct L2BookOutcome {
    pub(crate) deltas: Option<(OrderBookDeltas, u64, L2Subscription)>,
    pub(crate) resync: Option<L2ResyncRequest>,
}

#[derive(Debug)]
struct ChecksumMismatch {
    local: u32,
    remote: u32,
    bids: usize,
    asks: usize,
}

#[derive(Debug)]
pub(crate) struct L2BookState {
    pub(crate) books: AHashMap<InstrumentId, OrderBook>,
    pub(crate) sync: BookSyncTracker,
    validate_checksum: bool,
    live_request: AHashMap<InstrumentId, u64>,
    checksum_buffer: String,
    checksum_scratch: String,
}

impl L2BookState {
    pub(crate) fn new(validate_checksum: bool) -> Self {
        Self {
            books: AHashMap::new(),
            sync: BookSyncTracker::new(Duration::from_secs(DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS)),
            validate_checksum,
            live_request: AHashMap::new(),
            checksum_buffer: String::with_capacity(512),
            checksum_scratch: String::with_capacity(32),
        }
    }

    pub(crate) fn with_snapshot_timeout(mut self, timeout: Duration) -> Self {
        self.sync = BookSyncTracker::new(timeout);
        self
    }

    pub(crate) fn reset_after_reconnect(&mut self, now: Instant) {
        self.books.clear();
        self.live_request.clear();
        self.sync.reset_on_reconnect(now);
    }

    pub(crate) fn start_stream(
        &mut self,
        instrument_id: InstrumentId,
        subscription: L2Subscription,
        now: Instant,
    ) {
        self.sync.track(instrument_id, subscription.generation, now);
        if self
            .live_request
            .insert(instrument_id, subscription.latest_request)
            != Some(subscription.latest_request)
        {
            self.books.remove(&instrument_id);
        }
    }

    pub(crate) fn reject_subscription(
        &mut self,
        instrument_id: InstrumentId,
        subscription: L2Subscription,
        now: Instant,
        reason: &str,
    ) -> Option<L2ResyncRequest> {
        self.live_request.remove(&instrument_id);
        self.invalidate(instrument_id, subscription, now, reason)
    }

    pub(crate) fn overdue_snapshots(
        &mut self,
        now: Instant,
        held: &AHashMap<InstrumentId, L2Subscription>,
    ) -> Vec<L2ResyncRequest> {
        self.sync.retain(|id| held.contains_key(&id));
        self.books.retain(|id, _| {
            held.get(id)
                .is_some_and(|sub| self.live_request.get(id) == Some(&sub.latest_request))
        });

        self.live_request.retain(|id, _| held.contains_key(id));
        for (&id, subscription) in held {
            self.sync.track(id, subscription.generation, now);
        }

        self.sync
            .expired(now)
            .into_iter()
            .filter_map(|id| self.request_recovery(id, held[&id], now))
            .collect()
    }

    pub(crate) fn process_book(
        &mut self,
        book: &KrakenWsBookData,
        instrument: &InstrumentAny,
        sequence: u64,
        is_snapshot: bool,
        depths: &L2Depths,
        ts_init: UnixNanos,
    ) -> anyhow::Result<L2BookOutcome> {
        let instrument_id = instrument.id();

        let Some(subscription) = depths.subscription(book.symbol.as_str()) else {
            self.sync.remove(instrument_id);
            self.books.remove(&instrument_id);
            self.live_request.remove(&instrument_id);
            return Ok(L2BookOutcome::default());
        };

        let now = Instant::now();

        if self.live_request.get(&instrument_id) != Some(&subscription.latest_request) {
            return Ok(L2BookOutcome::default());
        }

        let position = i64::try_from(book.timestamp.as_nanosecond())?;
        let sync = self.sync.track(instrument_id, subscription.generation, now);
        match sync.position() {
            Some(previous) if position < *previous => return Ok(L2BookOutcome::default()),
            None if !is_snapshot => {
                let resync = self.request_recovery(instrument_id, subscription, now);
                return Ok(L2BookOutcome {
                    deltas: None,
                    resync,
                });
            }
            _ => {}
        }

        let result = if is_snapshot {
            self.process_snapshot(book, instrument, sequence, subscription, depths, ts_init)
        } else {
            self.process_update(book, instrument, sequence, subscription, ts_init)
        };

        match result {
            _ if !is_snapshot
                && depths.subscription(book.symbol.as_str()) != Some(subscription) =>
            {
                Ok(L2BookOutcome::default())
            }
            Ok(deltas) => {
                if !is_snapshot && deltas.is_some() {
                    self.sync
                        .track(instrument_id, subscription.generation, now)
                        .advance(position, now);
                }

                Ok(L2BookOutcome {
                    deltas: deltas
                        .map(|(batch, next_sequence)| (batch, next_sequence, subscription)),
                    resync: None,
                })
            }
            Err(e) => {
                let resync = depths
                    .with_current_request(book.symbol.as_str(), subscription.latest_request, |_| {
                        log::warn!("Invalid Kraken Spot L2 book for {instrument_id}: {e}");
                        self.invalidate(instrument_id, subscription, now, &e.to_string())
                    })
                    .flatten();

                Ok(L2BookOutcome {
                    deltas: None,
                    resync,
                })
            }
        }
    }

    fn process_snapshot(
        &mut self,
        book: &KrakenWsBookData,
        instrument: &InstrumentAny,
        sequence: u64,
        subscription: L2Subscription,
        depths: &L2Depths,
        ts_init: UnixNanos,
    ) -> anyhow::Result<Option<(OrderBookDeltas, u64)>> {
        let instrument_id = instrument.id();
        let (shadow, mut deltas, next_sequence) =
            build_snapshot_book(book, instrument, sequence, subscription.depth, ts_init)?;
        self.validate(&shadow, instrument, book.checksum)?;
        let sync = self
            .sync
            .track(instrument_id, subscription.generation, Instant::now());

        let position = i64::try_from(book.timestamp.as_nanosecond())?;

        if depths
            .accept_snapshot(book.symbol.as_str(), subscription.latest_request, || {
                sync.accept_snapshot(position, Instant::now())
            })
            .is_none()
        {
            return Ok(None);
        }

        self.books.insert(instrument_id, shadow);
        set_last_delta_flag(&mut deltas);
        Ok(Some((
            OrderBookDeltas::new(instrument_id, deltas),
            next_sequence,
        )))
    }

    fn process_update(
        &mut self,
        book: &KrakenWsBookData,
        instrument: &InstrumentAny,
        sequence: u64,
        subscription: L2Subscription,
        ts_init: UnixNanos,
    ) -> anyhow::Result<Option<(OrderBookDeltas, u64)>> {
        let instrument_id = instrument.id();

        let Some(shadow) = self.books.get_mut(&instrument_id) else {
            return Ok(None);
        };

        let mut deltas = parse_book_deltas(book, instrument, sequence, false, ts_init)?;
        if deltas.is_empty() {
            return Ok(None);
        }

        let mut next_sequence = sequence + deltas.len() as u64;
        shadow.apply_deltas(&OrderBookDeltas::new(instrument_id, deltas.clone()))?;
        prune_deltas_to_depth(
            shadow,
            subscription.depth,
            false,
            &mut next_sequence,
            ts_init,
            &mut deltas,
        );
        book_check_integrity(shadow)?;
        validate_checksum(
            self.validate_checksum,
            shadow,
            instrument,
            book.checksum,
            &mut self.checksum_buffer,
            &mut self.checksum_scratch,
        )?;
        set_last_delta_flag(&mut deltas);
        Ok(Some((
            OrderBookDeltas::new(instrument_id, deltas),
            next_sequence,
        )))
    }

    fn validate(
        &mut self,
        shadow: &OrderBook,
        instrument: &InstrumentAny,
        checksum: Option<u32>,
    ) -> anyhow::Result<()> {
        book_check_integrity(shadow)?;
        validate_checksum(
            self.validate_checksum,
            shadow,
            instrument,
            checksum,
            &mut self.checksum_buffer,
            &mut self.checksum_scratch,
        )
    }

    fn invalidate(
        &mut self,
        instrument_id: InstrumentId,
        subscription: L2Subscription,
        now: Instant,
        reason: &str,
    ) -> Option<L2ResyncRequest> {
        self.books.remove(&instrument_id);
        let sync = self.sync.track(instrument_id, subscription.generation, now);
        sync.reject(KrakenWsError::SubscriptionError(reason.to_owned()));
        self.request_recovery(instrument_id, subscription, now)
    }

    fn request_recovery(
        &mut self,
        instrument_id: InstrumentId,
        subscription: L2Subscription,
        now: Instant,
    ) -> Option<L2ResyncRequest> {
        let sync = self.sync.track(instrument_id, subscription.generation, now);
        (sync.gap() == BookSequenceOutcome::Recover).then_some(L2ResyncRequest {
            instrument_id,
            generation: subscription.generation,
        })
    }
}

fn validate_checksum(
    enabled: bool,
    shadow: &OrderBook,
    instrument: &InstrumentAny,
    checksum: Option<u32>,
    buffer: &mut String,
    scratch: &mut String,
) -> anyhow::Result<()> {
    if let Some(mismatch) =
        checksum_mismatch(enabled, shadow, instrument, checksum, buffer, scratch)
    {
        anyhow::bail!(
            "Checksum mismatch: local={}, remote={}, bids={}, asks={}",
            mismatch.local,
            mismatch.remote,
            mismatch.bids,
            mismatch.asks,
        );
    }

    Ok(())
}

/// Parses `book`'s snapshot into a fresh shadow book pruned to `depth`, returning the book, the
/// deltas to emit and the sequence after them. Nothing here touches the instrument's state, so a
/// failure leaves it as it was.
fn build_snapshot_book(
    book: &KrakenWsBookData,
    instrument: &InstrumentAny,
    sequence: u64,
    depth: u32,
    ts_init: UnixNanos,
) -> anyhow::Result<(OrderBook, Vec<OrderBookDelta>, u64)> {
    let instrument_id = instrument.id();
    let mut deltas = parse_book_deltas(book, instrument, sequence, true, ts_init)?;
    let mut shadow = OrderBook::new(instrument_id, BookType::L2_MBP);
    shadow.apply_deltas(&OrderBookDeltas::new(instrument_id, deltas.clone()))?;

    let mut next_sequence = sequence + deltas.len() as u64;
    prune_deltas_to_depth(
        &mut shadow,
        depth,
        true,
        &mut next_sequence,
        ts_init,
        &mut deltas,
    );

    Ok((shadow, deltas, next_sequence))
}

/// Compares `shadow` with the venue's checksum when `validate` holds and the frame carries one.
fn checksum_mismatch(
    validate: bool,
    shadow: &OrderBook,
    instrument: &InstrumentAny,
    remote: Option<u32>,
    buffer: &mut String,
    scratch: &mut String,
) -> Option<ChecksumMismatch> {
    let remote = remote.filter(|_| validate)?;
    // Scales come from the instrument on every message, so a refreshed definition takes effect at
    // once.
    let local = compute_checksum(
        shadow,
        price_wire_scale(instrument),
        instrument.size_precision(),
        buffer,
        scratch,
    );

    (local != remote).then(|| ChecksumMismatch {
        local,
        remote,
        bids: shadow.bids(None).count(),
        asks: shadow.asks(None).count(),
    })
}

/// The scale the venue sends prices at.
///
/// `AssetPairs` declares it as `pair_decimals`, carried on the instrument only when it is finer
/// than the tick-size precision the instrument's prices use.
fn price_wire_scale(instrument: &InstrumentAny) -> u8 {
    instrument
        .info()
        .and_then(|info| info.get(KRAKEN_PAIR_DECIMALS_KEY))
        .and_then(serde_json::Value::as_u64)
        .and_then(|scale| u8::try_from(scale).ok())
        .unwrap_or_else(|| instrument.price_precision())
}

/// Computes Kraken's `book` checksum over the top ten levels of each side of `book`.
///
/// Asks ascending then bids descending, each level as the wire-scale price followed by the
/// wire-scale quantity, per the venue's documented algorithm. `buffer` holds the checksum string
/// and `scratch` one rendered value; both are cleared here and reused across calls.
pub(crate) fn compute_checksum(
    book: &OrderBook,
    price_scale: u8,
    qty_scale: u8,
    buffer: &mut String,
    scratch: &mut String,
) -> u32 {
    buffer.clear();

    for level in book.asks(Some(10)).chain(book.bids(Some(10))) {
        push_scaled(buffer, scratch, level.price.value.as_decimal(), price_scale);
        push_scaled(buffer, scratch, level.size_decimal(), qty_scale);
    }

    crc32_ieee(buffer.as_bytes())
}

fn prune_deltas_to_depth(
    book: &mut OrderBook,
    depth: u32,
    is_snapshot: bool,
    next_sequence: &mut u64,
    ts_init: UnixNanos,
    deltas: &mut Vec<OrderBookDelta>,
) {
    if depth == 0 {
        return;
    }

    let prune_orders: Vec<BookOrder> = book
        .bids(None)
        .skip(depth as usize)
        .chain(book.asks(None).skip(depth as usize))
        .filter_map(|level| level.first().copied())
        .collect();

    if prune_orders.is_empty() {
        return;
    }

    let ts_event = deltas.last().map_or(ts_init, |delta| delta.ts_event);
    let mut flags = RecordFlag::F_MBP as u8;
    if is_snapshot {
        flags |= RecordFlag::F_SNAPSHOT as u8;
    }

    for order in prune_orders {
        let delta = OrderBookDelta::new(
            book.instrument_id,
            BookAction::Delete,
            order,
            flags,
            *next_sequence,
            ts_event,
            ts_init,
        );
        *next_sequence += 1;

        if let Err(e) = book.apply_delta(&delta) {
            log::error!("Failed to apply Kraken L2 depth prune delta to shadow book: {e}");
            continue;
        }

        deltas.push(delta);
    }
}

fn set_last_delta_flag(deltas: &mut [OrderBookDelta]) {
    for delta in deltas.iter_mut() {
        delta.flags &= !(RecordFlag::F_LAST as u8);
    }

    if let Some(last) = deltas.last_mut() {
        last.flags |= RecordFlag::F_LAST as u8;
    }
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;
    use nautilus_core::params::Params;
    use nautilus_model::{
        identifiers::Symbol,
        instruments::currency_pair::CurrencyPair,
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use ustr::Ustr;

    use super::*;

    #[rstest]
    fn test_replay_retires_the_previous_request_even_after_a_snapshot() {
        let depths = L2Depths::default();
        let generation = depths.insert("BTC/USD", 25, 17);
        let old = depths.accept_snapshot("BTC/USD", 17, || true).unwrap();
        depths.retire_requests(18);
        assert_eq!(depths.accept_snapshot("BTC/USD", 17, || true), None);
        assert_eq!(depths.begin_replay("BTC/USD", 19), Some(()));

        assert_eq!(old.snapshot_epoch, 1);
        assert_eq!(
            depths.with_current_request("BTC/USD", 17, |_| panic!("retired request applied")),
            None::<()>
        );
        assert_eq!(
            depths.with_current_request("BTC/USD", 19, |_| "current request"),
            Some("current request")
        );
        assert_eq!(
            depths.subscription("BTC/USD"),
            Some(L2Subscription {
                depth: 25,
                generation,
                latest_request: 19,
                snapshot_epoch: 2
            })
        );
    }

    #[rstest]
    fn test_spot_book_recovery_clears_rejection_when_it_replaces_the_request() {
        let depths = L2Depths::default();
        let symbol = Ustr::from("BTC/USD");
        let generation = depths.insert(symbol.as_str(), 25, 17);
        let recovery = Arc::new(BookRecovery::default());
        depths.register_recovery(symbol, generation, &recovery);

        let reject = |request, reason: &str| {
            depths.with_current_request(symbol.as_str(), request, |_| {
                recovery.outcome.send_replace(BookRecoveryOutcome::Rejected(
                    KrakenWsError::SubscriptionError(reason.into()),
                ));
            })
        };

        assert_eq!(reject(17, "old request"), Some(()));
        assert_eq!(
            depths
                .begin_recovery(symbol, generation, 0, 19, &CancellationToken::new())
                .map(|request| request.latest_request),
            Some(19)
        );
        assert!(matches!(
            *recovery.outcome.borrow(),
            BookRecoveryOutcome::Pending
        ));
        assert_eq!(reject(17, "late old request"), None);
        assert_eq!(reject(19, "current request"), Some(()));
        assert!(matches!(
            &*recovery.outcome.borrow(),
            BookRecoveryOutcome::Rejected(KrakenWsError::SubscriptionError(reason))
                if reason == "current request"
        ));
    }
    use crate::{
        common::consts::KRAKEN_VENUE,
        websocket::spot_v2::messages::{KrakenWsBookLevel, KrakenWsRawMessage},
    };

    const TS: UnixNanos = UnixNanos::new(1_700_000_000_000_000_000);
    const SYMBOL: &str = "BTC/USD";

    /// Kraken's documented `book` checksum example: BTC/USD, ten levels a side, 3310070434.
    const GUIDE_SNAPSHOT: &str = include_str!("../../../test_data/ws_book_snapshot.json");
    /// The guide snapshot with the best bid's quantity changed to 0.2; checksum from an independent
    /// implementation of the documented algorithm.
    const GUIDE_UPDATE: &str = include_str!("../../../test_data/ws_book_update.json");

    fn instrument(price_precision: u8, pair_decimals: Option<u8>) -> InstrumentAny {
        let info = pair_decimals.map(|scale| {
            let mut map = IndexMap::new();
            map.insert(
                KRAKEN_PAIR_DECIMALS_KEY.to_string(),
                serde_json::Value::from(u64::from(scale)),
            );
            Params::new(map)
        });
        InstrumentAny::CurrencyPair(
            CurrencyPair::builder()
                .instrument_id(InstrumentId::new(Symbol::new("BTC/USD"), *KRAKEN_VENUE))
                .raw_symbol(Symbol::new("XXBTZUSD"))
                .base_currency(Currency::BTC())
                .quote_currency(Currency::USD())
                .price_precision(price_precision)
                .size_precision(8)
                .price_increment(Price::new(
                    10_f64.powi(-i32::from(price_precision)),
                    price_precision,
                ))
                .size_increment(Quantity::from("0.00000001"))
                .maybe_info(info)
                .ts_event(TS)
                .ts_init(TS)
                .build()
                .unwrap(),
        )
    }

    fn book_data(json: &str) -> KrakenWsBookData {
        let message: KrakenWsRawMessage = serde_json::from_str(json).unwrap();
        serde_json::from_str(message.data[0].get()).unwrap()
    }

    fn level(price: Decimal, qty: Decimal) -> KrakenWsBookLevel {
        KrakenWsBookLevel { price, qty }
    }

    /// The guide snapshot with a timestamp before the epoch, which fails to parse.
    fn confirmed(state: &mut L2BookState, instrument: &InstrumentAny, depth: u32) -> L2Depths {
        let depths = L2Depths::default();
        depths.insert(SYMBOL, depth, 1);
        let subscription = depths.subscription(SYMBOL).unwrap();
        state.start_stream(instrument.id(), subscription, Instant::now());
        depths
    }

    fn feed(
        state: &mut L2BookState,
        instrument: &InstrumentAny,
        depths: &L2Depths,
        sequence: u64,
        now: UnixNanos,
    ) {
        let outcome = state
            .process_book(
                &book_data(GUIDE_SNAPSHOT),
                instrument,
                sequence,
                true,
                depths,
                now,
            )
            .unwrap();
        assert!(outcome.deltas.is_some() && outcome.resync.is_none());
        assert!(state.books.contains_key(&instrument.id()));
    }

    #[rstest]
    fn test_guide_snapshot_checksum_matches() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let depths = confirmed(&mut state, &instrument, 10);
        let snapshot = book_data(GUIDE_SNAPSHOT);
        assert_eq!(snapshot.checksum, Some(3_310_070_434));

        let outcome = state
            .process_book(&snapshot, &instrument, 0, true, &depths, TS)
            .unwrap();

        assert!(
            outcome.resync.is_none(),
            "the documented example must validate"
        );
        let (deltas, _, _) = outcome.deltas.expect("snapshot deltas");
        assert_eq!(deltas.deltas.len(), 21);
        let book = &state.books[&instrument.id()];
        let (mut buffer, mut scratch) = (String::new(), String::new());
        assert_eq!(
            compute_checksum(book, 1, 8, &mut buffer, &mut scratch),
            3_310_070_434
        );
    }

    #[rstest]
    fn test_update_checksum_is_computed_over_the_shadow_book() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let depths = confirmed(&mut state, &instrument, 10);
        feed(&mut state, &instrument, &depths, 0, TS);

        let update = book_data(GUIDE_UPDATE);
        assert_eq!(update.checksum, Some(38_355_977));
        let outcome = state
            .process_book(&update, &instrument, 21, false, &depths, TS)
            .unwrap();

        assert!(
            outcome.resync.is_none(),
            "an update validated against the whole book"
        );
        assert_eq!(outcome.deltas.expect("update deltas").0.deltas.len(), 1);
    }

    #[rstest]
    fn test_checksum_gap_quarantines_updates_until_a_valid_snapshot() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let id = instrument.id();
        let depths = confirmed(&mut state, &instrument, 10);
        feed(&mut state, &instrument, &depths, 0, TS);
        let mut corrupt = book_data(GUIDE_UPDATE);
        corrupt.checksum = Some(1);

        let outcome = state
            .process_book(&corrupt, &instrument, 21, false, &depths, TS)
            .unwrap();
        assert!(outcome.deltas.is_none());
        assert_eq!(outcome.resync.unwrap().instrument_id, id);
        let episode = state.sync.get_mut(id).unwrap().claim().unwrap();
        assert!(episode.begin_replacement());

        let update = book_data(GUIDE_UPDATE);
        assert!(
            state
                .process_book(&update, &instrument, 21, false, &depths, TS)
                .unwrap()
                .deltas
                .is_none()
        );
        let snapshot = book_data(GUIDE_SNAPSHOT);
        assert!(
            state
                .process_book(&snapshot, &instrument, 21, true, &depths, TS)
                .unwrap()
                .deltas
                .is_none()
        );
        assert!(!episode.is_accepted());

        episode.gate.open();
        feed(&mut state, &instrument, &depths, 21, TS);
        assert!(episode.is_accepted());
        assert!(
            state
                .process_book(&update, &instrument, 42, false, &depths, TS)
                .unwrap()
                .deltas
                .is_some()
        );
    }

    #[rstest]
    #[case::disabled(0, false)]
    #[case::enabled(1, true)]
    fn test_missing_initial_snapshot_deadline(#[case] timeout: u64, #[case] expected: bool) {
        let now = Instant::now();
        let mut state = L2BookState::new(true).with_snapshot_timeout(Duration::from_secs(timeout));
        let instrument = instrument(1, None);
        let id = instrument.id();
        let depths = L2Depths::default();
        depths.insert(SYMBOL, 10, 1);
        let held = AHashMap::from([(id, depths.subscription(SYMBOL).unwrap())]);
        assert!(state.overdue_snapshots(now, &held).is_empty());
        assert_eq!(
            state
                .overdue_snapshots(now + Duration::from_secs(1), &held)
                .len(),
            usize::from(expected)
        );
    }

    #[rstest]
    fn test_empty_snapshot_replaces_the_book() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let depths = confirmed(&mut state, &instrument, 10);
        feed(&mut state, &instrument, &depths, 0, TS);
        let mut snapshot = book_data(GUIDE_SNAPSHOT);
        snapshot.bids = Some(Vec::new());
        snapshot.asks = Some(Vec::new());
        snapshot.checksum = Some(0);

        let outcome = state
            .process_book(&snapshot, &instrument, 21, true, &depths, TS)
            .unwrap();
        let (deltas, next_sequence, _) = outcome.deltas.unwrap();
        assert_eq!(next_sequence, 22);
        assert_eq!(deltas.deltas.len(), 1);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert!(RecordFlag::F_SNAPSHOT.matches(deltas.deltas[0].flags));
        assert!(RecordFlag::F_LAST.matches(deltas.deltas[0].flags));
        assert_eq!(state.books[&instrument.id()].best_bid_price(), None);
        assert_eq!(state.books[&instrument.id()].best_ask_price(), None);
        assert!(outcome.resync.is_none());
    }

    #[rstest]
    fn test_retiring_a_subscription_cancels_its_recovery() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let id = instrument.id();
        let depths = confirmed(&mut state, &instrument, 10);
        let episode = state.sync.get_mut(id).unwrap().claim().unwrap();

        depths.remove(SYMBOL);
        let requests = state.overdue_snapshots(Instant::now(), &AHashMap::new());

        assert!(requests.is_empty());
        assert!(episode.cancellation.is_cancelled());
        assert!(state.sync.get_mut(id).is_none());
        assert!(state.live_request.is_empty());
    }

    #[rstest]
    fn test_validation_disabled_ignores_a_bad_checksum() {
        let mut state = L2BookState::new(false);
        let instrument = instrument(1, None);
        let depths = confirmed(&mut state, &instrument, 10);
        let mut snapshot = book_data(GUIDE_SNAPSHOT);
        snapshot.checksum = Some(1);

        let outcome = state
            .process_book(&snapshot, &instrument, 0, true, &depths, TS)
            .unwrap();

        assert!(outcome.resync.is_none());
        assert_eq!(outcome.deltas.expect("deltas").0.deltas.len(), 21);
    }

    /// Prices are hashed at the wire scale, which `pair_decimals` gives when it differs from the
    /// tick precision; hashing at the instrument's precision would mismatch on every message.
    #[rstest]
    fn test_price_scale_comes_from_pair_decimals() {
        let message = KrakenWsBookData {
            symbol: Ustr::from("BTC/USD"),
            bids: Some(vec![level(dec!(0.000122), dec!(75))]),
            asks: Some(vec![
                level(dec!(0.000123), dec!(100)),
                level(dec!(0.000124), dec!(50)),
            ]),
            // Over "0.0001230", "0.0001240", "0.0001220" and eight-decimal quantities.
            checksum: Some(2_896_240_975),
            timestamp: book_data(GUIDE_SNAPSHOT).timestamp,
        };

        let scaled = instrument(6, Some(7));
        let mut with_scale = L2BookState::new(true);
        let depths = confirmed(&mut with_scale, &scaled, 10);
        let outcome = with_scale
            .process_book(&message, &scaled, 0, true, &depths, TS)
            .unwrap();
        assert!(
            outcome.resync.is_none(),
            "seven-decimal prices match the venue"
        );

        let unscaled = instrument(6, None);
        let mut without_scale = L2BookState::new(true);
        let depths = confirmed(&mut without_scale, &unscaled, 10);
        let outcome = without_scale
            .process_book(&message, &unscaled, 0, true, &depths, TS)
            .unwrap();
        assert!(outcome.deltas.is_none());
    }
}
