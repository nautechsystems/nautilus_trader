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
    Arc,
    atomic::{AtomicU64, Ordering},
};

use ahash::{AHashMap, AHashSet};
use nautilus_core::{AtomicMap, UnixNanos};
use nautilus_model::{
    data::{BookOrder, OrderBookDelta, OrderBookDeltas},
    enums::{BookAction, BookType, RecordFlag},
    identifiers::InstrumentId,
    instruments::{Instrument, any::InstrumentAny},
    orderbook::OrderBook,
};
use parking_lot::Mutex;
use ustr::Ustr;

use super::{checksum::push_scaled, messages::KrakenWsBookData, parse::parse_book_deltas};
use crate::common::consts::KRAKEN_PAIR_DECIMALS_KEY;

/// One logical `book` subscription as the client records it per venue symbol.
///
/// `generation` changes with every subscription change, so a recovery queued for a replaced
/// subscription can tell it is retired. `latest_request` is the request id of the latest `book`
/// subscribe sent for the symbol: once the venue confirms it, the stream it opens is the only one
/// whose frames are accepted. `snapshot_epoch` moves with every snapshot the data client accepts
/// and every reconnect, so a recovery issued before either finds its snapshot served or the stream
/// replayed and sends nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct L2Subscription {
    pub(crate) depth: u32,
    pub(crate) generation: u64,
    pub(crate) latest_request: u64,
    pub(crate) snapshot_epoch: u64,
}

#[derive(Debug, Clone)]
pub(crate) struct L2Depths {
    depths: Arc<AtomicMap<String, L2Subscription>>,
    next_generation: Arc<AtomicU64>,
}

impl Default for L2Depths {
    fn default() -> Self {
        Self {
            depths: Arc::new(AtomicMap::new()),
            next_generation: Arc::new(AtomicU64::new(1)),
        }
    }
}

impl L2Depths {
    pub(crate) fn get(&self, symbol: &str) -> Option<u32> {
        self.subscription(symbol).map(|s| s.depth)
    }

    pub(crate) fn subscription(&self, symbol: &str) -> Option<L2Subscription> {
        self.depths.load().get(symbol).copied()
    }

    /// Records a new subscription at `depth`, opened by the subscribe `request`, under a fresh
    /// generation, which it returns.
    pub(crate) fn insert(&self, symbol: &str, depth: u32, request: u64) -> u64 {
        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed);
        self.depths.insert(
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

    /// Records `request` as the symbol's latest subscribe when the subscription is still
    /// `generation` and its snapshot epoch is still `epoch`, returning the updated subscription.
    ///
    /// The check and the write are one compare-and-swap, as is [`Self::accept_snapshot`], so of a
    /// recovery and a late snapshot exactly one takes effect: either the snapshot is accepted and
    /// the recovery finds the epoch moved, or the recovery's request becomes the latest and the
    /// snapshot is dropped as a retired stream's.
    pub(crate) fn begin_resync(
        &self,
        symbol: &str,
        generation: u64,
        epoch: u64,
        request: u64,
    ) -> Option<L2Subscription> {
        let mut updated = None;
        self.depths.rcu(|depths| {
            updated = depths
                .get_mut(symbol)
                .filter(|held| held.generation == generation && held.snapshot_epoch == epoch)
                .map(|held| {
                    held.latest_request = request;
                    *held
                });
        });
        updated
    }

    /// Moves the symbol's snapshot epoch when `request` is still its latest subscribe, returning
    /// the updated subscription; `None` means a later request has retired the snapshot's stream.
    pub(crate) fn accept_snapshot(&self, symbol: &str, request: u64) -> Option<L2Subscription> {
        let mut updated = None;
        self.depths.rcu(|depths| {
            updated = depths
                .get_mut(symbol)
                .filter(|held| held.latest_request == request)
                .map(|held| {
                    held.snapshot_epoch += 1;
                    *held
                });
        });
        updated
    }

    /// Moves every symbol's snapshot epoch, so a recovery issued before a reconnect sends nothing
    /// against the replayed stream.
    pub(crate) fn advance_epochs(&self) {
        self.depths.rcu(|depths| {
            for held in depths.values_mut() {
                held.snapshot_epoch += 1;
            }
        });
    }

    /// Every subscription currently held, by venue symbol.
    pub(crate) fn held(&self) -> Vec<(String, L2Subscription)> {
        self.depths
            .load()
            .iter()
            .map(|(symbol, subscription)| (symbol.clone(), *subscription))
            .collect()
    }

    pub(crate) fn remove(&self, symbol: &str) {
        self.depths.rcu(|depths| {
            depths.remove(symbol);
        });
    }

    pub(crate) fn clear(&self) {
        self.depths.store(AHashMap::new());
    }
}

/// A `book` subscribe the client has sent and the venue has not answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct L2BookRequest {
    pub(crate) symbol: Ustr,
}

/// The `book` subscribes in flight, by request id, shared between the client that sends them and
/// the data client that reads the venue's answers.
pub(crate) type L2BookRequests = Arc<Mutex<AHashMap<u64, L2BookRequest>>>;

/// A resubscription the data client issues after a checksum mismatch or an overdue snapshot.
///
/// `generation` names the subscription the request belongs to, so the recovery leaves a
/// replacement subscription alone; the depth resubscribed is the live subscription's. `epoch` is
/// the symbol's snapshot epoch when the request was issued, so a snapshot accepted or a reconnect
/// made before the recovery runs makes it send nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct L2ResyncRequest {
    pub(crate) instrument_id: InstrumentId,
    pub(crate) generation: u64,
    pub(crate) epoch: u64,
}

/// What processing one `book` message produced.
#[derive(Debug, Default)]
pub(crate) struct L2BookOutcome {
    pub(crate) deltas: Option<(OrderBookDeltas, u64)>,
    pub(crate) resync: Option<L2ResyncRequest>,
}

/// What one check of the held subscriptions' snapshots produced.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct L2SnapshotCheck {
    /// The snapshots to request again.
    pub(crate) requests: Vec<L2ResyncRequest>,
    /// Held instruments whose shadow book the check dropped, so the consumer's book is to be
    /// cleared; a book dropped with its canceled subscription has no consumer and is left out.
    pub(crate) cleared: Vec<InstrumentId>,
}

/// What rejecting a `book` subscribe did to the instrument's wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct L2Rejection {
    /// When the snapshot is requested again, or `None` at the request cap.
    pub(crate) next_request_due: Option<UnixNanos>,
    /// Whether a shadow book was dropped, so the consumer's book is to be cleared.
    pub(crate) cleared: bool,
}

/// Checksum mismatches after which an instrument's validation is switched off.
///
/// A book the venue hashes differently from the shadow book would otherwise resubscribe forever.
/// Three with no valid update between them is not a transient gap; a snapshot that validates says
/// nothing about the update path, so it does not reset the count.
pub(crate) const MAX_CONSECUTIVE_CHECKSUM_MISMATCHES: u32 = 3;

/// Base wait for the snapshot a subscription owes; it doubles with every request made for it.
pub(crate) const SNAPSHOT_TIMEOUT_NS: u64 = 10_000_000_000;

/// Snapshot requests per cleared book before the data client stops asking.
///
/// With the doubling wait the requests go out 10, 20, 40, 80 and 160 seconds after the one
/// before; a venue that has not answered in that time is not going to.
pub(crate) const MAX_SNAPSHOT_REQUESTS: u32 = 5;

/// A cleared book waiting for the snapshot its subscription owes.
///
/// `since` is when the latest request for the snapshot went out and `attempts` how many have
/// failed, so the next one is due `SNAPSHOT_TIMEOUT_NS << attempts` after `since`. `generation`
/// is the subscription the wait belongs to: a replacement subscription gets a fresh wait rather
/// than inheriting one capped or counted up under its predecessor. A snapshot or a canceled
/// subscription removes the entry; a subscribe the venue rejects restarts it at the rejection
/// with the rejected request counted; at `MAX_SNAPSHOT_REQUESTS` it stays without further
/// requests and the book stays cleared until the next subscription change or reconnect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SnapshotWait {
    pub(crate) since: UnixNanos,
    pub(crate) attempts: u32,
    pub(crate) generation: u64,
}

impl SnapshotWait {
    const fn fresh(since: UnixNanos, generation: u64) -> Self {
        Self {
            since,
            attempts: 0,
            generation,
        }
    }

    /// How long after `since` the next request is due.
    const fn timeout_ns(&self) -> u64 {
        SNAPSHOT_TIMEOUT_NS << self.attempts
    }

    /// When the next request is due, or `None` at the request cap.
    fn next_request_due(&self) -> Option<UnixNanos> {
        (self.attempts < MAX_SNAPSHOT_REQUESTS)
            .then(|| UnixNanos::new(self.since.as_u64() + self.timeout_ns()))
    }
}

/// A checksum the shadow book failed, with the level counts the warning reports.
#[derive(Debug, Clone, Copy)]
struct ChecksumMismatch {
    local: u32,
    remote: u32,
    bids: usize,
    asks: usize,
}

/// Shadow books for the Spot `book` channel, one per instrument.
///
/// Checksum validation is fixed at construction from the client configuration.
#[derive(Debug)]
pub(crate) struct L2BookState {
    pub(crate) books: AHashMap<InstrumentId, OrderBook>,
    validate_checksum: bool,
    /// Instruments already warned about frames that arrive with no recorded subscription, so a
    /// stream in that state logs once rather than per frame.
    unsubscribed_warned: AHashSet<InstrumentId>,
    /// Instruments with a cleared book. Their updates are dropped until the snapshot their
    /// subscription owes, since they describe a stream the venue has ended.
    awaiting_snapshot: AHashMap<InstrumentId, SnapshotWait>,
    /// Consecutive mismatches per instrument; a valid message resets the count.
    mismatches: AHashMap<InstrumentId, u32>,
    /// Instruments whose validation is off after too many consecutive mismatches.
    validation_disabled: AHashSet<InstrumentId>,
    /// The subscription generation whose stream was last opened per instrument; a new one
    /// re-enables validation.
    last_generation: AHashMap<InstrumentId, u64>,
    /// The subscribe request whose stream the venue has confirmed, per instrument. Frames are
    /// accepted only while it is the symbol's latest request.
    live_request: AHashMap<InstrumentId, u64>,
    /// The checksum string and the per-value scratch, reused across messages.
    checksum_buffer: String,
    checksum_scratch: String,
}

impl L2BookState {
    pub(crate) fn new(validate_checksum: bool) -> Self {
        Self {
            books: AHashMap::new(),
            validate_checksum,
            unsubscribed_warned: AHashSet::new(),
            awaiting_snapshot: AHashMap::new(),
            mismatches: AHashMap::new(),
            validation_disabled: AHashSet::new(),
            last_generation: AHashMap::new(),
            live_request: AHashMap::new(),
            checksum_buffer: String::with_capacity(512),
            checksum_scratch: String::with_capacity(32),
        }
    }

    /// Drops the shadow books and the mismatch counts after a reconnect, returning the instruments
    /// whose book it dropped so the consumer's books are cleared.
    ///
    /// `held` is every `book` subscription the client holds. The replay re-sends each one's latest
    /// subscribe, whose answer matches no request, so that request is taken as live and the
    /// replayed stream's frames are accepted. Every held instrument gets a wait starting at `now`
    /// under its held generation: an existing wait is restarted, not preserved, since the replay is
    /// a new subscribe with the full allowance of requests, which also covers a wait a rejection
    /// restarted. A replay the venue drops is then noticed by [`Self::overdue_snapshots`]. The
    /// replacement stream gets the full allowance of mismatches; an instrument whose validation is
    /// off stays off unless the held generation is new to it. The unsubscribed-frame warnings are
    /// cleared, since the replay sends every subscribe again.
    pub(crate) fn reset_after_reconnect(
        &mut self,
        now: UnixNanos,
        held: &[(InstrumentId, L2Subscription)],
    ) -> Vec<InstrumentId> {
        self.live_request.clear();

        for (instrument_id, subscription) in held {
            self.live_request
                .insert(*instrument_id, subscription.latest_request);
            self.begin_generation(*instrument_id, subscription.generation);
            self.awaiting_snapshot.insert(
                *instrument_id,
                SnapshotWait::fresh(now, subscription.generation),
            );
        }

        let dropped: Vec<InstrumentId> = self.books.drain().map(|(id, _)| id).collect();
        self.mismatches.clear();
        self.unsubscribed_warned.clear();
        dropped
    }

    /// Opens the stream of `subscription`'s latest request, which the venue has confirmed, and
    /// returns whether a shadow book was dropped, so the consumer's book is cleared.
    ///
    /// The new stream's snapshot replaces whatever the instrument held, so any book is dropped and
    /// the instrument waits for that snapshot; a wait already armed for the generation keeps its
    /// attempts. Under a generation new to the instrument, validation, the mismatch count and the
    /// unsubscribed-frame warning start over. Under the same generation, a recovery's
    /// resubscription, the mismatch count carries over, so a book that keeps mismatching still
    /// reaches the cap. A confirmation of the request already live changes nothing.
    pub(crate) fn start_stream(
        &mut self,
        instrument_id: InstrumentId,
        subscription: L2Subscription,
        now: UnixNanos,
    ) -> bool {
        if self.live_request.get(&instrument_id) == Some(&subscription.latest_request) {
            return false;
        }

        self.live_request
            .insert(instrument_id, subscription.latest_request);
        self.begin_generation(instrument_id, subscription.generation);
        self.arm_snapshot_wait(instrument_id, subscription.generation, now);
        self.books.remove(&instrument_id).is_some()
    }

    /// Records `generation` as the instrument's stream; a generation new to it starts validation
    /// over, so an instrument the cap switched off is validated again once the user resubscribes.
    fn begin_generation(&mut self, instrument_id: InstrumentId, generation: u64) {
        if self.last_generation.insert(instrument_id, generation) != Some(generation) {
            self.validation_disabled.remove(&instrument_id);
            self.mismatches.remove(&instrument_id);
            self.unsubscribed_warned.remove(&instrument_id);
        }
    }

    /// Records that the venue rejected the symbol's latest `book` subscribe, sent for
    /// `generation`.
    ///
    /// The stream that request was to open will not come, so a book the instrument still holds
    /// belongs to a retired stream: it is dropped and reported as cleared. The wait restarts at
    /// `now` with the rejected request counted as one failed attempt: a request the watchdog made
    /// is counted when it goes out, and the subscribe or recovery that opened the wait is counted
    /// here, so the next request is due after the doubled base wait rather than the base wait. A
    /// pair the venue will not serve reaches the request cap after five rejections; a rejection the
    /// venue takes back, such as a rate limit while a reconnect fans out many recoveries, recovers
    /// at the next request.
    pub(crate) fn reject_subscription(
        &mut self,
        instrument_id: InstrumentId,
        generation: u64,
        now: UnixNanos,
    ) -> L2Rejection {
        let cleared = self.books.remove(&instrument_id).is_some();
        let wait = self.arm_snapshot_wait(instrument_id, generation, now);
        wait.since = now;
        wait.attempts = wait.attempts.max(1);

        L2Rejection {
            next_request_due: wait.next_request_due(),
            cleared,
        }
    }

    /// The wait for `generation`'s snapshot: armed at `now` unless one for that generation is
    /// under way, which keeps its attempts. A wait left by another generation is replaced, since
    /// the replacement is a new subscribe with the full allowance of requests.
    fn arm_snapshot_wait(
        &mut self,
        instrument_id: InstrumentId,
        generation: u64,
        now: UnixNanos,
    ) -> &mut SnapshotWait {
        let wait = self
            .awaiting_snapshot
            .entry(instrument_id)
            .or_insert(SnapshotWait::fresh(now, generation));

        if wait.generation != generation {
            *wait = SnapshotWait::fresh(now, generation);
        }

        wait
    }

    /// Requests the snapshot again for every held subscription whose book is overdue.
    ///
    /// `held` is every `book` subscription the client holds. Per instrument:
    /// - not held: its book, wait, generation, live request, mismatch count and validation switch
    ///   are dropped, since a canceled subscription delivers nothing and a later one is a new stream
    ///   that must not inherit its state; the book is not reported as cleared, since no consumer
    ///   holds it; the unsubscribed-frame warning is kept, so a stream in that state warns once;
    /// - held, its latest request live, with a shadow book: the snapshot has arrived, nothing is
    ///   owed (an instrument whose validation is off keeps its book, so it is never overdue);
    /// - held, its latest request not confirmed: the stream that request opens has not started,
    ///   so a book is a retired stream's and is dropped and reported as cleared, and the instrument
    ///   owes a snapshot. A confirmation the venue never sent or the transport lost therefore
    ///   falls to the request ladder below rather than leaving a frozen book in place;
    /// - owing a snapshot without a wait under the held generation: a wait starts at `now`,
    ///   replacing one left by an earlier generation, which covers a subscribe or a reconnect
    ///   replay whose snapshot the venue dropped, with at most one tick of slack;
    /// - owing a snapshot, fewer than `MAX_SNAPSHOT_REQUESTS` made: once
    ///   `SNAPSHOT_TIMEOUT_NS << attempts` has passed since `since`, a request carrying the held
    ///   generation and snapshot epoch is returned and the wait restarts at `now` with one more
    ///   attempt; the request that reaches the cap is still made and logs the error;
    /// - at the cap: no further request, and updates keep being dropped until a snapshot arrives.
    pub(crate) fn overdue_snapshots(
        &mut self,
        now: UnixNanos,
        held: &[(InstrumentId, L2Subscription)],
    ) -> L2SnapshotCheck {
        let held_ids: AHashSet<InstrumentId> = held.iter().map(|(held_id, _)| *held_id).collect();
        self.awaiting_snapshot
            .retain(|instrument_id, _| held_ids.contains(instrument_id));
        self.books
            .retain(|instrument_id, _| held_ids.contains(instrument_id));
        self.last_generation
            .retain(|instrument_id, _| held_ids.contains(instrument_id));
        self.live_request
            .retain(|instrument_id, _| held_ids.contains(instrument_id));
        self.mismatches
            .retain(|instrument_id, _| held_ids.contains(instrument_id));
        self.validation_disabled
            .retain(|instrument_id| held_ids.contains(instrument_id));

        let mut check = L2SnapshotCheck::default();

        for (instrument_id, subscription) in held {
            let generation = subscription.generation;
            let live = self.live_request.get(instrument_id) == Some(&subscription.latest_request);

            if live && self.books.contains_key(instrument_id) {
                continue;
            }

            if !live && self.books.remove(instrument_id).is_some() {
                check.cleared.push(*instrument_id);
            }

            let wait = self.arm_snapshot_wait(*instrument_id, generation, now);

            if wait.attempts >= MAX_SNAPSHOT_REQUESTS {
                continue;
            }

            let timeout_ns = wait.timeout_ns();
            let waited_ns = now.saturating_duration_since(wait.since).as_u64();

            if waited_ns < timeout_ns {
                continue;
            }

            wait.attempts += 1;
            wait.since = now;
            log::warn!(
                "Requesting the L2 snapshot for {instrument_id} again: none arrived within {} s \
                 of the resubscription (request {}/{MAX_SNAPSHOT_REQUESTS})",
                timeout_ns / 1_000_000_000,
                wait.attempts,
            );

            if wait.attempts == MAX_SNAPSHOT_REQUESTS {
                log::error!(
                    "L2 snapshot for {instrument_id} requested {MAX_SNAPSHOT_REQUESTS} times \
                     without an answer; the book stays cleared and its updates are dropped until \
                     the next subscription change or reconnect"
                );
            }

            check.requests.push(L2ResyncRequest {
                instrument_id: *instrument_id,
                generation,
                epoch: subscription.snapshot_epoch,
            });
        }

        check
    }

    /// Processes one `book` frame for `instrument` against the symbol's subscription in `depths`.
    ///
    /// A frame is accepted only from the stream of the symbol's latest subscribe, once the venue
    /// has confirmed it ([`Self::start_stream`]); a frame of any other stream is a retired
    /// stream's and is dropped. A snapshot is parsed, applied to a fresh book and pruned before
    /// any state changes; only then is it accepted against `depths`, which moves the snapshot epoch,
    /// and does its book replace the instrument's and end the wait.
    ///
    /// # Errors
    ///
    /// Returns an error if the frame cannot be parsed. A snapshot that fails leaves the wait, the
    /// live request and the snapshot epoch as they were; a book the instrument held is dropped,
    /// since the venue's state is the snapshot that failed, and its `Clear` is returned with the
    /// error logged instead.
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
            if self.last_generation.contains_key(&instrument_id) {
                // This handler fed the instrument under a subscription that has ended, so the
                // frame predates the cancel and describes an ended stream. The next watchdog tick
                // drops the rest of the instrument's state.
                log::debug!(
                    "Dropping L2 {} for {} after its subscription ended",
                    if is_snapshot { "snapshot" } else { "update" },
                    book.symbol
                );
                return Ok(L2BookOutcome::default());
            }

            // No subscription has ever held the stream, so there is no depth to prune to, no
            // generation to track and nothing to resubscribe: the frame is emitted as received.
            // Dropping it would turn a symbol the client records differently from the venue into
            // a silent outage.
            if self.unsubscribed_warned.insert(instrument_id) {
                log::warn!(
                    "L2 {} for {} arrived with no recorded subscription; emitting it unvalidated",
                    if is_snapshot { "snapshot" } else { "update" },
                    book.symbol
                );
            }

            let mut deltas = parse_book_deltas(book, instrument, sequence, is_snapshot, ts_init)?;
            if deltas.is_empty() {
                return Ok(L2BookOutcome::default());
            }

            let next_sequence = sequence + deltas.len() as u64;
            set_last_delta_flag(&mut deltas);

            return Ok(L2BookOutcome {
                deltas: Some((OrderBookDeltas::new(instrument_id, deltas), next_sequence)),
                resync: None,
            });
        };

        if self.live_request.get(&instrument_id) != Some(&subscription.latest_request) {
            log_retired_frame(book.symbol, is_snapshot);
            return Ok(L2BookOutcome::default());
        }

        if is_snapshot {
            self.process_snapshot(book, instrument, sequence, subscription, depths, ts_init)
        } else {
            self.process_update(book, instrument, sequence, subscription, ts_init)
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
    ) -> anyhow::Result<L2BookOutcome> {
        let instrument_id = instrument.id();
        let built = build_snapshot_book(book, instrument, sequence, subscription.depth, ts_init);

        let (shadow, mut deltas, next_sequence) = match built {
            Ok(built) => built,
            Err(e) => {
                self.arm_snapshot_wait(instrument_id, subscription.generation, ts_init);

                if self.books.remove(&instrument_id).is_none() {
                    return Err(e);
                }

                log::error!(
                    "Failed to process the L2 snapshot for {}: {e}; the book is cleared until the \
                     next snapshot",
                    book.symbol
                );
                return Ok(L2BookOutcome {
                    deltas: Some(clear_deltas(instrument_id, sequence, ts_init, ts_init)),
                    resync: None,
                });
            }
        };

        // A recovery may have recorded a later request since the stream check: the swap fails
        // then, and the snapshot is the retired stream's.
        let Some(accepted) =
            depths.accept_snapshot(book.symbol.as_str(), subscription.latest_request)
        else {
            log_retired_frame(book.symbol, true);
            return Ok(L2BookOutcome::default());
        };
        self.awaiting_snapshot.remove(&instrument_id);

        let validate = self.validate_checksum && !self.validation_disabled.contains(&instrument_id);
        let mismatch = checksum_mismatch(
            validate,
            &shadow,
            instrument,
            book.checksum,
            &mut self.checksum_buffer,
            &mut self.checksum_scratch,
        );

        // The recovery carries the epoch this snapshot moved to, so it is not skipped as served.
        if let Some(mismatch) = mismatch {
            let ts_event = deltas.last().map_or(ts_init, |delta| delta.ts_event);

            if let Some(outcome) = self.count_mismatch(
                instrument_id,
                book.symbol,
                mismatch,
                accepted,
                (next_sequence, ts_event),
                ts_init,
            ) {
                return Ok(outcome);
            }
        }

        self.books.insert(instrument_id, shadow);
        set_last_delta_flag(&mut deltas);
        Ok(L2BookOutcome {
            deltas: Some((OrderBookDeltas::new(instrument_id, deltas), next_sequence)),
            resync: None,
        })
    }

    fn process_update(
        &mut self,
        book: &KrakenWsBookData,
        instrument: &InstrumentAny,
        sequence: u64,
        subscription: L2Subscription,
        ts_init: UnixNanos,
    ) -> anyhow::Result<L2BookOutcome> {
        let instrument_id = instrument.id();

        if self.awaiting_snapshot.contains_key(&instrument_id) {
            log::debug!(
                "Dropping L2 update for {} while awaiting the snapshot",
                book.symbol
            );
            return Ok(L2BookOutcome::default());
        }

        let mut deltas = parse_book_deltas(book, instrument, sequence, false, ts_init)?;

        let Some(book_state) = self.books.get_mut(&instrument_id) else {
            // A stream opens with its snapshot, so an update with no book has nothing to apply
            // to; applying it to an empty book would build a book the venue does not hold.
            log::debug!(
                "Dropping L2 update for {}: no snapshot has opened the stream",
                book.symbol
            );
            self.arm_snapshot_wait(instrument_id, subscription.generation, ts_init);
            return Ok(L2BookOutcome::default());
        };

        if deltas.is_empty() {
            return Ok(L2BookOutcome::default());
        }

        let mut next_sequence = sequence + deltas.len() as u64;

        if let Err(e) =
            book_state.apply_deltas(&OrderBookDeltas::new(instrument_id, deltas.clone()))
        {
            log::error!("Failed to apply Kraken L2 deltas to shadow book: {e}");
        } else {
            prune_deltas_to_depth(
                book_state,
                subscription.depth,
                false,
                &mut next_sequence,
                ts_init,
                &mut deltas,
            );
        }

        // The venue hashes its top ten levels per side, which pruning to the subscribed depth
        // leaves intact, so the shadow book is compared after the message has been applied.
        let validate = self.validate_checksum && !self.validation_disabled.contains(&instrument_id);
        let mismatch = checksum_mismatch(
            validate,
            book_state,
            instrument,
            book.checksum,
            &mut self.checksum_buffer,
            &mut self.checksum_scratch,
        );

        match mismatch {
            Some(mismatch) => {
                let ts_event = deltas.last().map_or(ts_init, |delta| delta.ts_event);

                if let Some(outcome) = self.count_mismatch(
                    instrument_id,
                    book.symbol,
                    mismatch,
                    subscription,
                    (next_sequence, ts_event),
                    ts_init,
                ) {
                    return Ok(outcome);
                }
            }
            None if validate && book.checksum.is_some() => {
                self.mismatches.remove(&instrument_id);
            }
            None => {}
        }

        set_last_delta_flag(&mut deltas);
        Ok(L2BookOutcome {
            deltas: Some((OrderBookDeltas::new(instrument_id, deltas), next_sequence)),
            resync: None,
        })
    }

    /// Counts a checksum mismatch on the instrument's book.
    ///
    /// Below the cap the book is dropped and the stream recovered: the wait starts afresh at
    /// `ts_init` and the outcome carries one `Clear` at `clear_at` and a recovery at
    /// `subscription`'s generation and snapshot epoch. At the cap validation is switched off and
    /// `None` is returned, so the frame is kept.
    fn count_mismatch(
        &mut self,
        instrument_id: InstrumentId,
        symbol: Ustr,
        mismatch: ChecksumMismatch,
        subscription: L2Subscription,
        clear_at: (u64, UnixNanos),
        ts_init: UnixNanos,
    ) -> Option<L2BookOutcome> {
        let ChecksumMismatch {
            local,
            remote,
            bids,
            asks,
        } = mismatch;
        let consecutive = self.mismatches.entry(instrument_id).or_insert(0);
        *consecutive += 1;

        if *consecutive >= MAX_CONSECUTIVE_CHECKSUM_MISMATCHES {
            // The shadow book cannot reproduce the venue's hash for this instrument, so another
            // resubscription would only repeat the cycle; keep the book and stop validating it.
            log::error!(
                "L2 book checksum mismatched {consecutive} times in a row: symbol={symbol}, \
                 local={local}, remote={remote}; validation disabled for this instrument and \
                 the book kept as received",
            );
            self.validation_disabled.insert(instrument_id);
            self.mismatches.remove(&instrument_id);
            return None;
        }

        log::warn!(
            "L2 book checksum mismatch: symbol={symbol}, local={local}, remote={remote}, \
             bids={bids}, asks={asks}; clearing the book and resubscribing",
        );
        self.books.remove(&instrument_id);
        self.awaiting_snapshot.insert(
            instrument_id,
            SnapshotWait::fresh(ts_init, subscription.generation),
        );

        let (sequence, ts_event) = clear_at;
        Some(L2BookOutcome {
            deltas: Some(clear_deltas(instrument_id, sequence, ts_event, ts_init)),
            resync: Some(L2ResyncRequest {
                instrument_id,
                generation: subscription.generation,
                epoch: subscription.snapshot_epoch,
            }),
        })
    }
}

/// Logs a frame dropped because its stream is not the one the symbol's latest confirmed
/// subscribe opened.
fn log_retired_frame(symbol: Ustr, is_snapshot: bool) {
    if is_snapshot {
        // A stream's snapshot follows its confirmation, so a dropped snapshot means overlapping
        // resubscriptions and leaves the book waiting: it is logged where it can be seen.
        log::warn!(
            "Dropping L2 snapshot for {symbol} from a retired stream: awaiting the confirmation \
             of the latest subscribe"
        );
    } else {
        log::debug!("Dropping L2 update for {symbol} from a retired stream");
    }
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

/// One `Clear` delta for `instrument_id` at `sequence`, flagged last, with the sequence after it.
///
/// Every path that drops a shadow book off the frame path tells the consumer with this delta.
pub(crate) fn clear_deltas(
    instrument_id: InstrumentId,
    sequence: u64,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> (OrderBookDeltas, u64) {
    let mut clear = OrderBookDelta::clear(instrument_id, sequence, ts_event, ts_init);
    clear.flags |= RecordFlag::F_LAST as u8;

    (
        OrderBookDeltas::new(instrument_id, vec![clear]),
        sequence + 1,
    )
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

    crc32fast::hash(buffer.as_bytes())
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
        enums::BookAction,
        identifiers::Symbol,
        instruments::currency_pair::CurrencyPair,
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use ustr::Ustr;

    use super::*;
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
            Params::from_index_map(map)
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
                    10f64.powi(-i32::from(price_precision)),
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
    fn unparsable_snapshot() -> KrakenWsBookData {
        let mut snapshot = book_data(GUIDE_SNAPSHOT);
        snapshot.timestamp = "1969-12-31T23:59:59Z".parse().unwrap();
        snapshot
    }

    fn held_subscription(depths: &L2Depths) -> L2Subscription {
        depths.subscription(SYMBOL).expect("a held subscription")
    }

    /// Records a subscription for the symbol at `depth`, opened by `request`, as `subscribe_book`
    /// does.
    fn subscribe(depths: &L2Depths, depth: u32, request: u64) -> L2Subscription {
        depths.insert(SYMBOL, depth, request);
        held_subscription(depths)
    }

    /// Records `request` as a recovery's resubscribe of the held subscription, as `resync_book`
    /// does.
    fn resubscribe(depths: &L2Depths, request: u64) {
        let live = held_subscription(depths);
        depths
            .begin_resync(SYMBOL, live.generation, live.snapshot_epoch, request)
            .expect("the recovery is admitted");
    }

    /// Confirms the symbol's latest request at `now`, as the venue's success answer does, and
    /// returns whether a book was dropped.
    fn confirm(
        state: &mut L2BookState,
        instrument: &InstrumentAny,
        depths: &L2Depths,
        now: UnixNanos,
    ) -> bool {
        state.start_stream(instrument.id(), held_subscription(depths), now)
    }

    /// A subscription at `depth` whose subscribe the venue confirmed at `TS`.
    fn confirmed(state: &mut L2BookState, instrument: &InstrumentAny, depth: u32) -> L2Depths {
        let depths = L2Depths::default();
        subscribe(&depths, depth, 1);
        confirm(state, instrument, &depths, TS);
        depths
    }

    fn held(instrument: &InstrumentAny, depths: &L2Depths) -> Vec<(InstrumentId, L2Subscription)> {
        vec![(instrument.id(), held_subscription(depths))]
    }

    /// The recovery the held subscription calls for: its generation and current snapshot epoch.
    fn recovery(instrument: &InstrumentAny, depths: &L2Depths) -> L2ResyncRequest {
        let live = held_subscription(depths);
        L2ResyncRequest {
            instrument_id: instrument.id(),
            generation: live.generation,
            epoch: live.snapshot_epoch,
        }
    }

    /// Feeds the guide snapshot at `sequence` and asserts it was applied.
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
        let (deltas, _) = outcome.deltas.expect("snapshot deltas");
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

    /// The subscribe the venue confirms opens the stream: the confirmation arms the wait, and the
    /// stream's snapshot is accepted, moves the snapshot epoch and ends the wait.
    #[rstest]
    fn test_the_confirmed_request_opens_the_stream() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let depths = L2Depths::default();
        let subscription = subscribe(&depths, 10, 1);

        assert!(!confirm(&mut state, &instrument, &depths, TS));
        assert_eq!(state.live_request.get(&instrument_id), Some(&1));
        assert_eq!(
            state.awaiting_snapshot[&instrument_id],
            SnapshotWait::fresh(TS, subscription.generation)
        );

        feed(&mut state, &instrument, &depths, 0, at(1));

        assert!(!state.awaiting_snapshot.contains_key(&instrument_id));
        assert_eq!(held_subscription(&depths).snapshot_epoch, 1);
    }

    /// A mismatch clears the book, emits one `Clear`, requests a resubscription carrying the epoch
    /// the snapshot moved to, and drops updates until the resubscription's snapshot.
    #[rstest]
    fn test_mismatch_clears_the_book_and_requests_resync() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let depths = confirmed(&mut state, &instrument, 25);
        let mut snapshot = book_data(GUIDE_SNAPSHOT);
        snapshot.checksum = Some(1);

        let outcome = state
            .process_book(&snapshot, &instrument, 0, true, &depths, TS)
            .unwrap();

        assert_eq!(
            outcome.resync,
            Some(L2ResyncRequest {
                instrument_id: instrument.id(),
                generation: held_subscription(&depths).generation,
                epoch: 1,
            }),
            "the recovery carries the epoch the snapshot moved to"
        );
        assert_eq!(held_subscription(&depths).snapshot_epoch, 1);
        let (deltas, next_sequence) = outcome.deltas.expect("a clear is emitted");
        assert_eq!(deltas.deltas.len(), 1);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert!(RecordFlag::F_LAST.matches(deltas.deltas[0].flags));
        assert_eq!(
            next_sequence, 22,
            "the clear takes the sequence after the applied deltas"
        );
        assert!(!state.books.contains_key(&instrument.id()));

        let update = book_data(GUIDE_UPDATE);
        let dropped = state
            .process_book(&update, &instrument, 22, false, &depths, TS)
            .unwrap();
        assert!(dropped.deltas.is_none() && dropped.resync.is_none());

        resubscribe(&depths, 2);
        assert!(!confirm(&mut state, &instrument, &depths, TS));
        let fresh = state
            .process_book(
                &book_data(GUIDE_SNAPSHOT),
                &instrument,
                22,
                true,
                &depths,
                TS,
            )
            .unwrap();
        assert!(fresh.resync.is_none());
        assert!(
            fresh.deltas.is_some(),
            "the resubscription's snapshot resumes the stream"
        );
    }

    /// Three mismatches with no valid update between them switch validation off for that
    /// instrument alone.
    ///
    /// A book the venue hashes differently would otherwise resubscribe on every snapshot; after the
    /// third, the message is applied and kept and no resync is requested.
    #[rstest]
    fn test_repeated_mismatches_disable_validation_for_the_instrument() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let depths = confirmed(&mut state, &instrument, 10);
        let mut bad = book_data(GUIDE_SNAPSHOT);
        bad.checksum = Some(1);

        for strike in 1..MAX_CONSECUTIVE_CHECKSUM_MISMATCHES {
            let outcome = state
                .process_book(&bad, &instrument, 0, true, &depths, TS)
                .unwrap();
            assert!(outcome.resync.is_some(), "strike {strike} resubscribes");
        }

        let final_strike = state
            .process_book(&bad, &instrument, 0, true, &depths, TS)
            .unwrap();
        assert!(
            final_strike.resync.is_none(),
            "the last strike stops resubscribing"
        );
        assert_eq!(
            final_strike
                .deltas
                .expect("the message is kept")
                .0
                .deltas
                .len(),
            21
        );
        assert!(state.books.contains_key(&instrument.id()));

        let again = state
            .process_book(&bad, &instrument, 21, true, &depths, TS)
            .unwrap();
        assert!(
            again.resync.is_none(),
            "validation stays off for this instrument"
        );
    }

    /// A new subscription re-enables validation for an instrument the cap switched off.
    #[rstest]
    fn test_a_new_subscription_re_enables_validation() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let depths = confirmed(&mut state, &instrument, 10);
        let mut bad = book_data(GUIDE_SNAPSHOT);
        bad.checksum = Some(1);

        for _ in 0..MAX_CONSECUTIVE_CHECKSUM_MISMATCHES {
            state
                .process_book(&bad, &instrument, 0, true, &depths, TS)
                .unwrap();
        }
        assert!(
            state
                .process_book(&bad, &instrument, 0, true, &depths, TS)
                .unwrap()
                .resync
                .is_none(),
            "validation is off for the subscription that struck out"
        );

        subscribe(&depths, 10, 2);
        confirm(&mut state, &instrument, &depths, TS);
        let outcome = state
            .process_book(&bad, &instrument, 0, true, &depths, TS)
            .unwrap();

        assert!(
            outcome.resync.is_some(),
            "the new subscription is validated again"
        );
    }

    /// A reconnect restarts the mismatch count: the replayed stream gets the full allowance.
    #[rstest]
    fn test_a_reconnect_restarts_the_mismatch_count() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let depths = confirmed(&mut state, &instrument, 10);
        let mut bad = book_data(GUIDE_SNAPSHOT);
        bad.checksum = Some(1);

        for _ in 0..(MAX_CONSECUTIVE_CHECKSUM_MISMATCHES - 1) {
            state
                .process_book(&bad, &instrument, 0, true, &depths, TS)
                .unwrap();
        }

        state.reset_after_reconnect(TS, &held(&instrument, &depths));

        let outcome = state
            .process_book(&bad, &instrument, 0, true, &depths, TS)
            .unwrap();
        assert!(
            outcome.resync.is_some(),
            "the first mismatch after a reconnect resubscribes rather than striking out"
        );
        assert!(state.books.is_empty() || !state.validation_disabled.contains(&instrument.id()));
    }

    /// A valid update resets the mismatch count, so sporadic mismatches never add up.
    #[rstest]
    fn test_a_valid_update_resets_the_mismatch_count() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let depths = confirmed(&mut state, &instrument, 10);
        let good = book_data(GUIDE_SNAPSHOT);
        let update = book_data(GUIDE_UPDATE);
        let mut bad = good.clone();
        bad.checksum = Some(1);

        for _ in 0..(MAX_CONSECUTIVE_CHECKSUM_MISMATCHES * 2) {
            assert!(
                state
                    .process_book(&bad, &instrument, 0, true, &depths, TS)
                    .unwrap()
                    .resync
                    .is_some(),
                "each mismatch after a valid update resubscribes"
            );
            assert!(
                state
                    .process_book(&good, &instrument, 0, true, &depths, TS)
                    .unwrap()
                    .resync
                    .is_none()
            );
            assert!(
                state
                    .process_book(&update, &instrument, 21, false, &depths, TS)
                    .unwrap()
                    .resync
                    .is_none()
            );
        }
    }

    /// A snapshot that validates does not reset the count: a shadow book that diverges only on
    /// updates would otherwise resubscribe on every update, forever.
    #[rstest]
    fn test_a_valid_snapshot_does_not_reset_the_mismatch_count() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let depths = confirmed(&mut state, &instrument, 10);
        let good = book_data(GUIDE_SNAPSHOT);
        let mut bad_update = book_data(GUIDE_UPDATE);
        bad_update.checksum = Some(1);

        for strike in 1..MAX_CONSECUTIVE_CHECKSUM_MISMATCHES {
            assert!(
                state
                    .process_book(&good, &instrument, 0, true, &depths, TS)
                    .unwrap()
                    .resync
                    .is_none()
            );
            assert!(
                state
                    .process_book(&bad_update, &instrument, 21, false, &depths, TS)
                    .unwrap()
                    .resync
                    .is_some(),
                "strike {strike} resubscribes"
            );
        }

        state
            .process_book(&good, &instrument, 0, true, &depths, TS)
            .unwrap();
        let final_strike = state
            .process_book(&bad_update, &instrument, 21, false, &depths, TS)
            .unwrap();

        assert!(
            final_strike.resync.is_none(),
            "the third mismatch stops resubscribing although each snapshot validated"
        );
        assert!(final_strike.deltas.is_some(), "the update is kept");
    }

    /// A mismatch on each resubscription's snapshot counts toward one cap within a generation:
    /// the third switches validation off and keeps the book, and the next generation's
    /// confirmation switches it back on.
    #[rstest]
    fn test_a_third_mismatch_across_resyncs_disables_validation_until_a_new_generation() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let depths = confirmed(&mut state, &instrument, 10);
        let mut bad = book_data(GUIDE_SNAPSHOT);
        bad.checksum = Some(1);

        for (strike, request) in [(1, 2), (2, 3)] {
            let outcome = state
                .process_book(&bad, &instrument, 0, true, &depths, TS)
                .unwrap();
            assert!(outcome.resync.is_some(), "strike {strike} resubscribes");
            resubscribe(&depths, request);
            assert!(!confirm(&mut state, &instrument, &depths, TS));
            assert_eq!(state.mismatches.get(&instrument_id), Some(&strike));
        }

        let third = state
            .process_book(&bad, &instrument, 0, true, &depths, TS)
            .unwrap();
        assert!(third.resync.is_none(), "the third mismatch stops the cycle");
        assert!(state.validation_disabled.contains(&instrument_id));
        assert!(state.books.contains_key(&instrument_id));

        subscribe(&depths, 10, 4);
        assert!(
            confirm(&mut state, &instrument, &depths, TS),
            "the kept book is the retired stream's"
        );
        assert!(!state.validation_disabled.contains(&instrument_id));
        assert!(
            state
                .process_book(&bad, &instrument, 0, true, &depths, TS)
                .unwrap()
                .resync
                .is_some(),
            "the new generation is validated"
        );
    }

    /// `secs` seconds after `TS`.
    fn at(secs: u64) -> UnixNanos {
        UnixNanos::new(TS.as_u64() + secs * 1_000_000_000)
    }

    /// Clears the book with a mismatching snapshot at `TS` on a confirmed subscription at depth
    /// 10, so the wait for the snapshot starts, and returns the subscription.
    fn mismatch_at_ts(state: &mut L2BookState, instrument: &InstrumentAny) -> L2Depths {
        let depths = confirmed(state, instrument, 10);
        let mut bad = book_data(GUIDE_SNAPSHOT);
        bad.checksum = Some(1);
        let outcome = state
            .process_book(&bad, instrument, 0, true, &depths, TS)
            .unwrap();
        assert!(outcome.resync.is_some(), "the mismatch requests a resync");
        assert!(!state.books.contains_key(&instrument.id()));
        depths
    }

    /// A book whose snapshot arrives inside the wait is never requested again, before or after
    /// the snapshot.
    #[rstest]
    fn test_a_snapshot_within_the_wait_is_not_requested_again() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let depths = mismatch_at_ts(&mut state, &instrument);

        assert!(
            state
                .overdue_snapshots(at(5), &held(&instrument, &depths))
                .requests
                .is_empty()
        );

        feed(&mut state, &instrument, &depths, 22, at(6));

        assert!(
            state
                .overdue_snapshots(at(60), &held(&instrument, &depths))
                .requests
                .is_empty()
        );
        assert!(!state.awaiting_snapshot.contains_key(&instrument.id()));
    }

    /// A cleared book whose snapshot is late is requested again under the held generation and
    /// epoch, and not again before the doubled wait has passed.
    #[rstest]
    fn test_an_overdue_snapshot_is_requested_again_once_per_doubled_wait() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let depths = mismatch_at_ts(&mut state, &instrument);
        let held = held(&instrument, &depths);

        assert!(state.overdue_snapshots(at(9), &held).requests.is_empty());

        let requests = state.overdue_snapshots(at(10), &held).requests;
        assert_eq!(requests, vec![recovery(&instrument, &depths)]);
        assert_eq!(
            state.awaiting_snapshot[&instrument_id],
            SnapshotWait {
                since: at(10),
                attempts: 1,
                generation: held_subscription(&depths).generation,
            }
        );

        assert!(
            state.overdue_snapshots(at(29), &held).requests.is_empty(),
            "the second wait is twice the first"
        );
        assert_eq!(state.overdue_snapshots(at(30), &held).requests.len(), 1);
        assert_eq!(state.awaiting_snapshot[&instrument_id].attempts, 2);
    }

    /// At the cap no further request goes out; updates are still dropped and a later snapshot
    /// resumes processing.
    #[rstest]
    fn test_the_request_cap_leaves_the_book_cleared_until_a_snapshot() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let depths = mismatch_at_ts(&mut state, &instrument);
        let held = held(&instrument, &depths);

        let mut now = TS;
        for attempt in 0..MAX_SNAPSHOT_REQUESTS {
            now = UnixNanos::new(now.as_u64() + (SNAPSHOT_TIMEOUT_NS << attempt));
            assert_eq!(
                state.overdue_snapshots(now, &held).requests.len(),
                1,
                "request {} is made",
                attempt + 1
            );
        }
        assert_eq!(
            state.awaiting_snapshot[&instrument_id].attempts,
            MAX_SNAPSHOT_REQUESTS
        );

        assert!(
            state
                .overdue_snapshots(at(100_000), &held)
                .requests
                .is_empty(),
            "no request beyond the cap"
        );

        let dropped = state
            .process_book(
                &book_data(GUIDE_UPDATE),
                &instrument,
                22,
                false,
                &depths,
                at(100_000),
            )
            .unwrap();
        assert!(dropped.deltas.is_none() && dropped.resync.is_none());

        feed(&mut state, &instrument, &depths, 22, at(100_001));
        assert!(!state.awaiting_snapshot.contains_key(&instrument_id));
    }

    /// A subscription the user replaced with its snapshot outstanding leaves the replacement a
    /// fresh wait: the replacement's snapshot is requested a full base timeout after the tick
    /// that first saw it, under its own generation, so the recovery acts on the live subscription.
    #[rstest]
    fn test_an_overdue_snapshot_is_requested_for_the_replacement() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let depths = mismatch_at_ts(&mut state, &instrument);

        let replacement = subscribe(&depths, 100, 2);
        let held = held(&instrument, &depths);

        assert!(
            state.overdue_snapshots(at(10), &held).requests.is_empty(),
            "the predecessor's wait is not inherited"
        );
        assert_eq!(
            state.awaiting_snapshot[&instrument_id],
            SnapshotWait::fresh(at(10), replacement.generation)
        );
        assert!(state.overdue_snapshots(at(19), &held).requests.is_empty());

        let requests = state.overdue_snapshots(at(20), &held).requests;
        assert_eq!(requests, vec![recovery(&instrument, &depths)]);
        assert_eq!(requests[0].generation, replacement.generation);
    }

    /// A wait at the request cap belongs to the subscription that struck out: a replacement
    /// subscription gets a fresh wait and its snapshot is requested after the base timeout.
    #[rstest]
    fn test_a_capped_wait_is_reset_for_the_replacement() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let depths = mismatch_at_ts(&mut state, &instrument);
        let struck_out = held(&instrument, &depths);

        let mut now = TS;
        for attempt in 0..MAX_SNAPSHOT_REQUESTS {
            now = UnixNanos::new(now.as_u64() + (SNAPSHOT_TIMEOUT_NS << attempt));
            assert_eq!(state.overdue_snapshots(now, &struck_out).requests.len(), 1);
        }
        assert_eq!(
            state.awaiting_snapshot[&instrument_id].attempts,
            MAX_SNAPSHOT_REQUESTS
        );
        let later = UnixNanos::new(now.as_u64() + 1_000_000_000);
        assert!(
            state
                .overdue_snapshots(later, &struck_out)
                .requests
                .is_empty()
        );

        let replacement = subscribe(&depths, 10, 2);
        let held = held(&instrument, &depths);
        assert!(state.overdue_snapshots(later, &held).requests.is_empty());
        assert_eq!(
            state.awaiting_snapshot[&instrument_id],
            SnapshotWait::fresh(later, replacement.generation)
        );

        let due = UnixNanos::new(later.as_u64() + SNAPSHOT_TIMEOUT_NS);
        assert!(
            state
                .overdue_snapshots(UnixNanos::new(due.as_u64() - 1), &held)
                .requests
                .is_empty(),
            "the replacement waits the base timeout"
        );
        assert_eq!(
            state.overdue_snapshots(due, &held).requests,
            vec![recovery(&instrument, &depths)]
        );
        assert_eq!(state.awaiting_snapshot[&instrument_id].attempts, 1);
    }

    /// A book of an unsubscribed instrument is dropped with the rest of its state on the next
    /// tick, so a later subscription that delivers no frame at all is seen as owing its snapshot:
    /// the tick that first sees it arms a wait, and the request goes out after the timeout under
    /// the new generation.
    #[rstest]
    fn test_an_unsubscribed_book_is_dropped_and_a_frameless_resubscription_is_overdue() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let depths = confirmed(&mut state, &instrument, 10);
        feed(&mut state, &instrument, &depths, 0, TS);

        assert_eq!(
            state.overdue_snapshots(at(1), &[]),
            L2SnapshotCheck::default(),
            "no consumer holds the unsubscribed book, so no clear is reported"
        );
        assert!(
            !state.books.contains_key(&instrument_id),
            "the unsubscribed book is dropped"
        );
        assert!(!state.last_generation.contains_key(&instrument_id));
        assert!(!state.live_request.contains_key(&instrument_id));
        assert!(!state.awaiting_snapshot.contains_key(&instrument_id));

        let resubscribed = subscribe(&depths, 10, 2);
        let held = held(&instrument, &depths);
        assert!(state.overdue_snapshots(at(2), &held).requests.is_empty());
        assert_eq!(
            state.awaiting_snapshot[&instrument_id],
            SnapshotWait::fresh(at(2), resubscribed.generation)
        );
        assert!(state.overdue_snapshots(at(11), &held).requests.is_empty());
        assert_eq!(
            state.overdue_snapshots(at(12), &held).requests,
            vec![recovery(&instrument, &depths)]
        );
    }

    /// A book whose stream a later subscribe has superseded does not count as the latest
    /// request's, even when that request's confirmation never arrives: the tick drops the book,
    /// tells the consumer, and the watchdog asks for the latest request's snapshot rather than
    /// leaving a frozen book in place.
    #[rstest]
    fn test_a_book_of_an_unconfirmed_replacement_is_dropped_and_retried() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let depths = confirmed(&mut state, &instrument, 10);
        feed(&mut state, &instrument, &depths, 0, TS);
        assert!(!state.awaiting_snapshot.contains_key(&instrument_id));

        let replacement = subscribe(&depths, 25, 2);
        let held = held(&instrument, &depths);
        assert_eq!(
            state.overdue_snapshots(at(1), &held),
            L2SnapshotCheck {
                requests: vec![],
                cleared: vec![instrument_id],
            },
            "the superseded stream's book is dropped and the consumer told"
        );
        assert!(!state.books.contains_key(&instrument_id));
        assert_eq!(
            state.awaiting_snapshot[&instrument_id],
            SnapshotWait::fresh(at(1), replacement.generation)
        );

        assert_eq!(
            state.overdue_snapshots(at(11), &held).requests,
            vec![recovery(&instrument, &depths)]
        );
    }

    /// A rejection of the latest request drops a book the instrument still holds, since that
    /// book is a retired stream's, reports it as cleared and counts the rejected request.
    #[rstest]
    fn test_a_rejection_of_the_latest_request_drops_a_fed_book() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let depths = confirmed(&mut state, &instrument, 10);
        feed(&mut state, &instrument, &depths, 0, TS);
        let replacement = subscribe(&depths, 25, 2);

        let rejection = state.reject_subscription(instrument_id, replacement.generation, at(1));

        assert_eq!(
            rejection,
            L2Rejection {
                next_request_due: Some(at(21)),
                cleared: true,
            }
        );
        assert!(!state.books.contains_key(&instrument_id));
        assert_eq!(
            state.awaiting_snapshot[&instrument_id],
            SnapshotWait {
                since: at(1),
                attempts: 1,
                generation: replacement.generation,
            }
        );
    }

    /// The warning about frames with no subscription fires once per instrument: an instrument
    /// in that state is by definition not held, so the tick that prunes unheld state leaves the
    /// warning armed rather than re-arming it every second.
    #[rstest]
    fn test_the_tick_keeps_the_unsubscribed_warning() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();

        state
            .process_book(
                &book_data(GUIDE_SNAPSHOT),
                &instrument,
                0,
                true,
                &L2Depths::default(),
                TS,
            )
            .unwrap();
        assert!(state.unsubscribed_warned.contains(&instrument_id));

        state.overdue_snapshots(at(1), &[]);

        assert!(
            state.unsubscribed_warned.contains(&instrument_id),
            "the tick does not re-arm the warning"
        );
    }

    /// A subscribe the venue rejects counts as one failed request: the next request is due after
    /// the doubled base wait, each later rejection restarts the wait at the attempts the watchdog
    /// has counted, and the fifth rejection reaches the cap.
    #[rstest]
    fn test_a_rejected_subscription_counts_one_failed_request_then_doubles_to_the_cap() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let depths = L2Depths::default();
        let subscription = subscribe(&depths, 10, 1);
        let generation = subscription.generation;
        let held = held(&instrument, &depths);

        let rejection = state.reject_subscription(instrument_id, generation, at(1));

        assert_eq!(
            rejection.next_request_due,
            Some(at(21)),
            "the next request is due after 20 s"
        );
        assert!(!rejection.cleared);
        assert!(!state.books.contains_key(&instrument_id));
        assert_eq!(
            state.awaiting_snapshot[&instrument_id],
            SnapshotWait {
                since: at(1),
                attempts: 1,
                generation,
            }
        );
        assert!(
            state.overdue_snapshots(at(20), &held).requests.is_empty(),
            "no request before the doubled wait"
        );
        assert_eq!(
            state.overdue_snapshots(at(21), &held).requests,
            vec![recovery(&instrument, &depths)]
        );
        assert_eq!(state.awaiting_snapshot[&instrument_id].attempts, 2);

        // Each watchdog request is counted once: its rejection restarts the wait without
        // counting it again, so the waits run 40, 80 and 160 seconds.
        let mut t = 21;
        for (rejections, wait_secs) in [(2, 40), (3, 80), (4, 160)] {
            t += 1;
            let rejection = state.reject_subscription(instrument_id, generation, at(t));
            assert_eq!(
                rejection.next_request_due,
                Some(at(t + wait_secs)),
                "rejection {rejections} is followed by a {wait_secs} s wait"
            );
            assert!(
                state
                    .overdue_snapshots(at(t + wait_secs - 1), &held)
                    .requests
                    .is_empty()
            );
            t += wait_secs;
            assert_eq!(state.overdue_snapshots(at(t), &held).requests.len(), 1);
            assert_eq!(
                state.awaiting_snapshot[&instrument_id].attempts,
                rejections + 1
            );
        }
        assert_eq!(
            state.awaiting_snapshot[&instrument_id].attempts,
            MAX_SNAPSHOT_REQUESTS
        );

        let fifth = state.reject_subscription(instrument_id, generation, at(t + 1));
        assert_eq!(fifth.next_request_due, None, "the cap is reached");
        assert!(
            state
                .overdue_snapshots(at(100_000), &held)
                .requests
                .is_empty(),
            "no request beyond the cap"
        );
        assert_eq!(
            state.awaiting_snapshot[&instrument_id].attempts,
            MAX_SNAPSHOT_REQUESTS
        );
    }

    /// A rejection burdens only the generation it answers: a replacement subscription gets a fresh
    /// wait, and a snapshot of the replacement ends it.
    #[rstest]
    fn test_a_rejected_subscription_leaves_the_replacement_a_fresh_wait() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let depths = mismatch_at_ts(&mut state, &instrument);
        state.reject_subscription(instrument_id, held_subscription(&depths).generation, at(1));

        let replacement = subscribe(&depths, 10, 2);
        let replaced = held(&instrument, &depths);
        assert!(
            state
                .overdue_snapshots(at(2), &replaced)
                .requests
                .is_empty()
        );
        assert_eq!(
            state.awaiting_snapshot[&instrument_id],
            SnapshotWait::fresh(at(2), replacement.generation)
        );
        assert_eq!(
            state.overdue_snapshots(at(12), &replaced).requests,
            vec![recovery(&instrument, &depths)]
        );

        assert!(!confirm(&mut state, &instrument, &depths, at(13)));
        feed(&mut state, &instrument, &depths, 22, at(13));
        assert!(!state.awaiting_snapshot.contains_key(&instrument_id));
    }

    /// The warning about frames with no subscription is armed again once a subscription for the
    /// instrument has been confirmed and dropped, and after a reconnect, so a recurrence is
    /// logged.
    #[rstest]
    fn test_a_subscription_appearing_rearms_the_unsubscribed_warning() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let snapshot = book_data(GUIDE_SNAPSHOT);
        let unheld = L2Depths::default();

        state
            .process_book(&snapshot, &instrument, 0, true, &unheld, TS)
            .unwrap();
        assert!(state.unsubscribed_warned.contains(&instrument_id));

        let depths = L2Depths::default();
        subscribe(&depths, 10, 1);
        confirm(&mut state, &instrument, &depths, at(1));
        assert!(
            !state.unsubscribed_warned.contains(&instrument_id),
            "the subscription's confirmation clears the warning"
        );

        state.overdue_snapshots(at(2), &[]);
        state
            .process_book(&snapshot, &instrument, 42, true, &unheld, at(3))
            .unwrap();
        assert!(state.unsubscribed_warned.contains(&instrument_id));

        state.reset_after_reconnect(at(4), &[]);
        assert!(state.unsubscribed_warned.is_empty());
    }

    /// A frame consumed after its subscription ended is a straggler of that stream: it is dropped
    /// without a warning, since this handler fed the instrument under a subscription, unlike a
    /// frame for an instrument no subscription has ever held.
    #[rstest]
    fn test_a_straggler_of_an_ended_subscription_is_dropped_silently() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let depths = confirmed(&mut state, &instrument, 10);
        feed(&mut state, &instrument, &depths, 0, TS);
        let ended = L2Depths::default();

        let update = state
            .process_book(
                &book_data(GUIDE_UPDATE),
                &instrument,
                21,
                false,
                &ended,
                at(1),
            )
            .unwrap();
        assert!(update.deltas.is_none() && update.resync.is_none());
        assert!(state.unsubscribed_warned.is_empty());

        let snapshot = state
            .process_book(
                &book_data(GUIDE_SNAPSHOT),
                &instrument,
                21,
                true,
                &ended,
                at(2),
            )
            .unwrap();
        assert!(snapshot.deltas.is_none() && snapshot.resync.is_none());
        assert!(state.unsubscribed_warned.is_empty());

        // Control: once the tick has dropped the instrument's state, a frame with no subscription
        // is the key-mismatch cell again and is emitted with the warning.
        state.overdue_snapshots(at(3), &[]);
        let unheld = state
            .process_book(
                &book_data(GUIDE_UPDATE),
                &instrument,
                21,
                false,
                &ended,
                at(4),
            )
            .unwrap();
        assert!(unheld.deltas.is_some());
        assert!(state.unsubscribed_warned.contains(&instrument_id));
    }

    /// The state of a canceled subscription does not survive into its successor: the tick that
    /// sees the instrument unheld drops its mismatch count and validation switch, so a
    /// resubscription is validated with the full allowance before any of its frames is processed.
    #[rstest]
    fn test_an_unsubscribed_instruments_validation_state_is_dropped() {
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let mut bad = book_data(GUIDE_SNAPSHOT);
        bad.checksum = Some(1);

        let mut struck_out = L2BookState::new(true);
        let depths = confirmed(&mut struck_out, &instrument, 10);
        for _ in 0..MAX_CONSECUTIVE_CHECKSUM_MISMATCHES {
            struck_out
                .process_book(&bad, &instrument, 0, true, &depths, TS)
                .unwrap();
        }
        assert!(struck_out.validation_disabled.contains(&instrument_id));

        let mut counting = L2BookState::new(true);
        mismatch_at_ts(&mut counting, &instrument);
        assert_eq!(counting.mismatches.get(&instrument_id), Some(&1));

        struck_out.overdue_snapshots(at(1), &[]);
        counting.overdue_snapshots(at(1), &[]);

        assert!(
            !struck_out.validation_disabled.contains(&instrument_id),
            "the validation switch goes with the subscription"
        );
        assert!(
            !counting.mismatches.contains_key(&instrument_id),
            "the mismatch count goes with the subscription"
        );

        subscribe(&depths, 10, 2);
        confirm(&mut struck_out, &instrument, &depths, at(2));
        assert!(
            struck_out
                .process_book(&bad, &instrument, 0, true, &depths, at(2))
                .unwrap()
                .resync
                .is_some(),
            "the resubscription is validated"
        );
    }

    /// A canceled subscription owes no snapshot: its wait is dropped and nothing is requested.
    #[rstest]
    fn test_a_canceled_subscription_drops_its_wait() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        mismatch_at_ts(&mut state, &instrument);

        assert!(state.overdue_snapshots(at(10), &[]).requests.is_empty());
        assert!(!state.awaiting_snapshot.contains_key(&instrument.id()));
    }

    /// An instrument whose validation is off keeps its book, whether the cap switched it off or the
    /// configuration did, so it is never overdue.
    #[rstest]
    fn test_a_validation_disabled_instrument_is_never_overdue() {
        let instrument = instrument(1, None);
        let mut bad = book_data(GUIDE_SNAPSHOT);
        bad.checksum = Some(1);

        let mut struck_out = L2BookState::new(true);
        let depths = confirmed(&mut struck_out, &instrument, 10);
        for _ in 0..MAX_CONSECUTIVE_CHECKSUM_MISMATCHES {
            struck_out
                .process_book(&bad, &instrument, 0, true, &depths, TS)
                .unwrap();
        }
        assert!(struck_out.validation_disabled.contains(&instrument.id()));
        assert!(struck_out.books.contains_key(&instrument.id()));
        assert!(
            struck_out
                .overdue_snapshots(at(100_000), &held(&instrument, &depths))
                .requests
                .is_empty()
        );
        assert!(struck_out.awaiting_snapshot.is_empty());

        let mut configured_off = L2BookState::new(false);
        let depths = confirmed(&mut configured_off, &instrument, 10);
        configured_off
            .process_book(&bad, &instrument, 0, true, &depths, TS)
            .unwrap();
        assert!(configured_off.books.contains_key(&instrument.id()));
        assert!(
            configured_off
                .overdue_snapshots(at(100_000), &held(&instrument, &depths))
                .requests
                .is_empty()
        );
        assert!(configured_off.awaiting_snapshot.is_empty());
    }

    /// With validation off, frames are still accepted only from the confirmed stream and the
    /// watchdog still asks for a snapshot that does not come.
    #[rstest]
    fn test_frames_are_gated_and_watched_with_validation_off() {
        let mut state = L2BookState::new(false);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let depths = L2Depths::default();
        subscribe(&depths, 10, 1);
        let held = held(&instrument, &depths);

        let unconfirmed = state
            .process_book(
                &book_data(GUIDE_SNAPSHOT),
                &instrument,
                0,
                true,
                &depths,
                TS,
            )
            .unwrap();
        assert!(unconfirmed.deltas.is_none());
        assert!(!state.books.contains_key(&instrument_id));

        assert!(state.overdue_snapshots(TS, &held).requests.is_empty());
        assert_eq!(
            state.overdue_snapshots(at(10), &held).requests,
            vec![recovery(&instrument, &depths)]
        );

        confirm(&mut state, &instrument, &depths, at(11));
        feed(&mut state, &instrument, &depths, 0, at(11));
    }

    /// A held subscription that never produced a book owes a snapshot: the wait starts at the
    /// confirmation and the request goes out once the timeout has passed.
    #[rstest]
    fn test_a_lost_initial_snapshot_becomes_overdue() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let depths = confirmed(&mut state, &instrument, 10);
        let held = held(&instrument, &depths);

        assert!(state.overdue_snapshots(TS, &held).requests.is_empty());
        assert_eq!(
            state.awaiting_snapshot[&instrument_id],
            SnapshotWait::fresh(TS, held_subscription(&depths).generation)
        );
        assert!(state.overdue_snapshots(at(9), &held).requests.is_empty());

        let requests = state.overdue_snapshots(at(10), &held).requests;
        assert_eq!(requests, vec![recovery(&instrument, &depths)]);
    }

    /// An initial subscribe whose confirmation is lost opens no stream: its snapshot is dropped,
    /// and the watchdog asks again on the doubling ladder up to the cap.
    #[rstest]
    fn test_a_lost_initial_confirmation_is_requested_until_the_cap() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let depths = L2Depths::default();
        subscribe(&depths, 10, 1);
        let held = held(&instrument, &depths);

        let snapshot = state
            .process_book(
                &book_data(GUIDE_SNAPSHOT),
                &instrument,
                0,
                true,
                &depths,
                TS,
            )
            .unwrap();
        assert!(snapshot.deltas.is_none() && snapshot.resync.is_none());
        assert_eq!(held_subscription(&depths).snapshot_epoch, 0);

        assert!(state.overdue_snapshots(TS, &held).requests.is_empty());
        let mut now = TS;
        for attempt in 0..MAX_SNAPSHOT_REQUESTS {
            now = UnixNanos::new(now.as_u64() + (SNAPSHOT_TIMEOUT_NS << attempt));
            assert_eq!(
                state.overdue_snapshots(now, &held).requests,
                vec![recovery(&instrument, &depths)],
                "request {} is made",
                attempt + 1
            );
        }
        assert!(
            state
                .overdue_snapshots(at(100_000), &held)
                .requests
                .is_empty()
        );
        assert_eq!(
            state.awaiting_snapshot[&instrument_id].attempts,
            MAX_SNAPSHOT_REQUESTS
        );
    }

    /// A reconnect arms the wait for every held instrument and takes its latest request as live,
    /// so the replayed snapshot is accepted and a replay whose snapshot the venue drops is
    /// requested after the timeout.
    #[rstest]
    fn test_a_reconnect_arms_the_wait_for_every_book() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let depths = confirmed(&mut state, &instrument, 10);
        feed(&mut state, &instrument, &depths, 0, TS);
        let held = held(&instrument, &depths);

        let dropped = state.reset_after_reconnect(at(1), &held);

        assert_eq!(dropped, vec![instrument_id], "the dropped book is reported");
        assert!(state.books.is_empty());
        assert_eq!(
            state.awaiting_snapshot[&instrument_id],
            SnapshotWait::fresh(at(1), held_subscription(&depths).generation)
        );
        assert!(state.overdue_snapshots(at(10), &held).requests.is_empty());
        assert_eq!(state.overdue_snapshots(at(11), &held).requests.len(), 1);

        feed(&mut state, &instrument, &depths, 21, at(12));
    }

    /// A reconnect restarts a wait already under way: the replayed subscription gets the full
    /// allowance of requests, counted from the reconnect.
    #[rstest]
    fn test_a_reconnect_restarts_a_wait_under_way() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let depths = mismatch_at_ts(&mut state, &instrument);
        let held = held(&instrument, &depths);
        assert_eq!(state.overdue_snapshots(at(10), &held).requests.len(), 1);
        assert_eq!(state.awaiting_snapshot[&instrument_id].attempts, 1);

        state.reset_after_reconnect(at(15), &held);

        assert_eq!(
            state.awaiting_snapshot[&instrument_id],
            SnapshotWait::fresh(at(15), held_subscription(&depths).generation)
        );
        assert!(state.overdue_snapshots(at(24), &held).requests.is_empty());
        assert_eq!(state.overdue_snapshots(at(25), &held).requests.len(), 1);
        assert_eq!(state.awaiting_snapshot[&instrument_id].attempts, 1);
    }

    /// Frames consumed before the data client sees a reconnect belong to the stream it has
    /// confirmed: the epoch the reconnect moves does not retire them.
    #[rstest]
    fn test_frames_before_the_reconnect_are_judged_against_the_prior_stream() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let depths = confirmed(&mut state, &instrument, 10);
        feed(&mut state, &instrument, &depths, 0, TS);

        depths.advance_epochs();
        let update = state
            .process_book(
                &book_data(GUIDE_UPDATE),
                &instrument,
                21,
                false,
                &depths,
                at(1),
            )
            .unwrap();

        assert!(update.deltas.is_some() && update.resync.is_none());
    }

    /// A late snapshot of the live stream that is accepted before the watchdog's recovery runs
    /// serves that recovery: the snapshot epoch moves, so the recovery is not admitted and leaves
    /// the recovered stream alone.
    #[rstest]
    fn test_a_late_snapshot_retires_the_watchdog_request() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let depths = mismatch_at_ts(&mut state, &instrument);
        let held = held(&instrument, &depths);
        let requests = state.overdue_snapshots(at(10), &held).requests;
        assert_eq!(requests.len(), 1);

        feed(&mut state, &instrument, &depths, 22, at(11));
        let request = &requests[0];

        assert_eq!(
            depths.begin_resync(SYMBOL, request.generation, request.epoch, 2),
            None,
            "the recovery finds its snapshot served"
        );
        assert_eq!(held_subscription(&depths).latest_request, 1);
        assert!(!state.awaiting_snapshot.contains_key(&instrument_id));
        assert!(state.overdue_snapshots(at(100), &held).requests.is_empty());
    }

    /// A late snapshot of the old stream consumed after a recovery has recorded its resubscribe is
    /// that stream's and is dropped: the wait and the epoch are left as they were, and the
    /// resubscribe's confirmation and snapshot end the wait.
    #[rstest]
    fn test_a_late_snapshot_after_the_recovery_is_recorded_is_dropped() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let depths = mismatch_at_ts(&mut state, &instrument);
        let requests = state
            .overdue_snapshots(at(10), &held(&instrument, &depths))
            .requests;
        let request = &requests[0];
        let wait = state.awaiting_snapshot[&instrument_id];

        assert!(
            depths
                .begin_resync(SYMBOL, request.generation, request.epoch, 2)
                .is_some()
        );
        let late = state
            .process_book(
                &book_data(GUIDE_SNAPSHOT),
                &instrument,
                22,
                true,
                &depths,
                at(11),
            )
            .unwrap();

        assert!(late.deltas.is_none() && late.resync.is_none());
        assert!(!state.books.contains_key(&instrument_id));
        assert_eq!(state.awaiting_snapshot[&instrument_id], wait);
        assert_eq!(held_subscription(&depths).snapshot_epoch, request.epoch);

        assert!(!confirm(&mut state, &instrument, &depths, at(12)));
        feed(&mut state, &instrument, &depths, 22, at(13));
        assert!(!state.awaiting_snapshot.contains_key(&instrument_id));
    }

    /// Of a recovery being admitted and a snapshot being accepted, exactly one takes effect,
    /// whichever comes first and when the two race.
    #[rstest]
    fn test_a_recovery_and_a_snapshot_take_effect_exclusively() {
        let depths = L2Depths::default();
        let subscription = subscribe(&depths, 10, 1);
        assert!(depths.accept_snapshot(SYMBOL, 1).is_some());
        assert_eq!(
            depths.begin_resync(
                SYMBOL,
                subscription.generation,
                subscription.snapshot_epoch,
                2
            ),
            None,
            "an accepted snapshot serves a recovery issued before it"
        );

        let subscription = held_subscription(&depths);
        assert!(
            depths
                .begin_resync(
                    SYMBOL,
                    subscription.generation,
                    subscription.snapshot_epoch,
                    2
                )
                .is_some()
        );
        assert_eq!(
            depths.accept_snapshot(SYMBOL, 1),
            None,
            "an admitted recovery retires the stream the snapshot belongs to"
        );

        for round in 0..200 {
            let depths = L2Depths::default();
            let subscription = subscribe(&depths, 10, 1);
            let barrier = std::sync::Barrier::new(2);
            let (resynced, accepted) = std::thread::scope(|scope| {
                let resync = scope.spawn(|| {
                    barrier.wait();
                    depths
                        .begin_resync(
                            SYMBOL,
                            subscription.generation,
                            subscription.snapshot_epoch,
                            2,
                        )
                        .is_some()
                });

                let accept = scope.spawn(|| {
                    barrier.wait();
                    depths.accept_snapshot(SYMBOL, 1).is_some()
                });
                (resync.join().unwrap(), accept.join().unwrap())
            });
            assert_ne!(resynced, accepted, "round {round}: exactly one wins");
        }
    }

    /// A snapshot consumed after the user's replacement subscribe and before its confirmation is
    /// the retired stream's: it is dropped without satisfying the replacement's wait, the tick
    /// drops the predecessor's book, the confirmation sends no second `Clear`, and the watchdog
    /// asks for the replacement's snapshot when it does not come.
    #[rstest]
    fn test_a_retired_snapshot_does_not_satisfy_the_replacements_wait() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let depths = confirmed(&mut state, &instrument, 10);
        feed(&mut state, &instrument, &depths, 0, TS);

        depths.remove(SYMBOL);
        let replacement = subscribe(&depths, 100, 2);
        let retired = state
            .process_book(
                &book_data(GUIDE_SNAPSHOT),
                &instrument,
                21,
                true,
                &depths,
                at(1),
            )
            .unwrap();
        assert!(retired.deltas.is_none() && retired.resync.is_none());
        assert_eq!(held_subscription(&depths).snapshot_epoch, 0);

        let held = held(&instrument, &depths);
        assert_eq!(
            state.overdue_snapshots(at(1), &held).cleared,
            vec![instrument_id]
        );
        let wait = SnapshotWait::fresh(at(1), replacement.generation);
        assert_eq!(state.awaiting_snapshot[&instrument_id], wait);

        assert!(
            !confirm(&mut state, &instrument, &depths, at(2)),
            "the book is already gone"
        );
        assert_eq!(state.awaiting_snapshot[&instrument_id], wait);
        assert_eq!(
            state.overdue_snapshots(at(11), &held).requests,
            vec![recovery(&instrument, &depths)],
            "the replacement's snapshot is asked for"
        );
    }

    /// An update of a retired stream consumed under a replacement is dropped without validation:
    /// a bad checksum on it triggers no recovery and counts no mismatch.
    #[rstest]
    fn test_a_retired_update_triggers_no_recovery() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let depths = confirmed(&mut state, &instrument, 10);
        feed(&mut state, &instrument, &depths, 0, TS);
        subscribe(&depths, 100, 2);

        let mut bad_update = book_data(GUIDE_UPDATE);
        bad_update.checksum = Some(1);
        let outcome = state
            .process_book(&bad_update, &instrument, 21, false, &depths, at(1))
            .unwrap();

        assert!(outcome.deltas.is_none() && outcome.resync.is_none());
        assert!(!state.mismatches.contains_key(&instrument_id));
    }

    /// A replacement's confirmation drops a fed book once and starts the new generation afresh:
    /// validation back on, no mismatch count, and a fresh wait in place of the predecessor's. A
    /// second confirmation of the live request leaves its book and wait alone.
    #[rstest]
    fn test_a_replacements_confirmation_starts_the_new_generation_afresh() {
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let mut bad = book_data(GUIDE_SNAPSHOT);
        bad.checksum = Some(1);

        let mut struck_out = L2BookState::new(true);
        let depths = confirmed(&mut struck_out, &instrument, 10);
        for _ in 0..MAX_CONSECUTIVE_CHECKSUM_MISMATCHES {
            struck_out
                .process_book(&bad, &instrument, 0, true, &depths, TS)
                .unwrap();
        }
        let replacement = subscribe(&depths, 25, 2);
        assert!(confirm(&mut struck_out, &instrument, &depths, at(1)));
        assert!(!struck_out.books.contains_key(&instrument_id));
        assert!(!struck_out.validation_disabled.contains(&instrument_id));
        assert_eq!(
            struck_out.awaiting_snapshot[&instrument_id],
            SnapshotWait::fresh(at(1), replacement.generation)
        );
        feed(&mut struck_out, &instrument, &depths, 0, at(2));
        assert!(
            !confirm(&mut struck_out, &instrument, &depths, at(3)),
            "a second confirmation of the live request changes nothing"
        );
        assert!(struck_out.books.contains_key(&instrument_id));
        assert!(!struck_out.awaiting_snapshot.contains_key(&instrument_id));

        let mut waiting = L2BookState::new(true);
        let depths = mismatch_at_ts(&mut waiting, &instrument);
        assert_eq!(
            waiting
                .overdue_snapshots(at(10), &held(&instrument, &depths))
                .requests
                .len(),
            1
        );
        let replacement = subscribe(&depths, 25, 2);
        assert!(!confirm(&mut waiting, &instrument, &depths, at(11)));
        assert_eq!(
            waiting.awaiting_snapshot[&instrument_id],
            SnapshotWait::fresh(at(11), replacement.generation)
        );
        assert!(!waiting.mismatches.contains_key(&instrument_id));
    }

    /// A recovery's resubscription stays within its generation: the old stream's frames are
    /// dropped until the confirmation, which sends no `Clear` and keeps the wait's attempts and the
    /// mismatch count.
    #[rstest]
    fn test_a_same_generation_resync_keeps_the_wait_and_the_mismatch_count() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let depths = mismatch_at_ts(&mut state, &instrument);
        assert_eq!(
            state
                .overdue_snapshots(at(10), &held(&instrument, &depths))
                .requests
                .len(),
            1
        );
        let wait = state.awaiting_snapshot[&instrument_id];
        assert_eq!(wait.attempts, 1);

        resubscribe(&depths, 2);
        let old = state
            .process_book(
                &book_data(GUIDE_SNAPSHOT),
                &instrument,
                22,
                true,
                &depths,
                at(11),
            )
            .unwrap();
        assert!(old.deltas.is_none());

        assert!(!confirm(&mut state, &instrument, &depths, at(12)));
        assert_eq!(state.awaiting_snapshot[&instrument_id], wait);
        assert_eq!(state.mismatches.get(&instrument_id), Some(&1));
        feed(&mut state, &instrument, &depths, 22, at(13));
    }

    /// An unsubscribe followed by a resubscribe, before the unsubscribe's answer and with or
    /// without a watchdog tick between, retires the old stream's frames until the new subscribe's
    /// confirmation, which clears a book the tick has not already dropped.
    #[rstest]
    #[case::other_depth_no_tick(25, false)]
    #[case::other_depth_with_tick(25, true)]
    #[case::same_depth_no_tick(10, false)]
    #[case::same_depth_with_tick(10, true)]
    fn test_a_resubscription_retires_the_old_streams_frames(
        #[case] depth: u32,
        #[case] tick_between: bool,
    ) {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let depths = confirmed(&mut state, &instrument, 10);
        feed(&mut state, &instrument, &depths, 0, TS);

        depths.remove(SYMBOL);

        if tick_between {
            state.overdue_snapshots(at(1), &[]);
        }
        subscribe(&depths, depth, 2);

        for (data, is_snapshot) in [(GUIDE_UPDATE, false), (GUIDE_SNAPSHOT, true)] {
            let retired = state
                .process_book(
                    &book_data(data),
                    &instrument,
                    21,
                    is_snapshot,
                    &depths,
                    at(2),
                )
                .unwrap();
            assert!(retired.deltas.is_none() && retired.resync.is_none());
        }

        assert_eq!(
            confirm(&mut state, &instrument, &depths, at(3)),
            !tick_between,
            "the confirmation clears the book only if the tick has not dropped it"
        );
        feed(&mut state, &instrument, &depths, 21, at(4));
    }

    /// A replacement's snapshot that fails to parse changes nothing it would commit: no book, the
    /// wait, the live request, the generation and the epoch as they were, and the watchdog asks
    /// again.
    #[rstest]
    fn test_a_replacement_snapshot_that_fails_to_parse_leaves_no_book_and_the_wait() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let depths = confirmed(&mut state, &instrument, 10);
        feed(&mut state, &instrument, &depths, 0, TS);
        let replacement = subscribe(&depths, 100, 2);
        assert!(confirm(&mut state, &instrument, &depths, at(1)));
        let wait = state.awaiting_snapshot[&instrument_id];

        let result = state.process_book(
            &unparsable_snapshot(),
            &instrument,
            21,
            true,
            &depths,
            at(2),
        );

        assert!(result.is_err(), "the parse error is returned");
        assert!(!state.books.contains_key(&instrument_id));
        assert_eq!(state.awaiting_snapshot[&instrument_id], wait);
        assert_eq!(state.live_request.get(&instrument_id), Some(&2));
        assert_eq!(
            state.last_generation.get(&instrument_id),
            Some(&replacement.generation)
        );
        assert_eq!(held_subscription(&depths).snapshot_epoch, 0);
        assert_eq!(
            state
                .overdue_snapshots(at(11), &held(&instrument, &depths))
                .requests,
            vec![recovery(&instrument, &depths)]
        );
    }

    /// A snapshot that fails while a book is held drops that book, since the venue's state is the
    /// snapshot that failed: one `Clear` reaches the consumer, the epoch stays, and the watchdog
    /// asks again rather than treating the old book as current.
    #[rstest]
    fn test_a_failed_snapshot_drops_the_book_it_would_replace() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let depths = confirmed(&mut state, &instrument, 10);
        feed(&mut state, &instrument, &depths, 0, TS);

        let outcome = state
            .process_book(
                &unparsable_snapshot(),
                &instrument,
                21,
                true,
                &depths,
                at(1),
            )
            .unwrap();

        assert!(outcome.resync.is_none());
        let (deltas, next_sequence) = outcome.deltas.expect("the consumer's book is cleared");
        assert_eq!(deltas.deltas.len(), 1);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert_eq!(deltas.deltas[0].sequence, 21);
        assert_eq!(next_sequence, 22);
        assert!(!state.books.contains_key(&instrument_id));
        assert_eq!(held_subscription(&depths).snapshot_epoch, 1);
        assert_eq!(
            state.awaiting_snapshot[&instrument_id],
            SnapshotWait::fresh(at(1), held_subscription(&depths).generation)
        );
        assert_eq!(
            state
                .overdue_snapshots(at(11), &held(&instrument, &depths))
                .requests,
            vec![recovery(&instrument, &depths)]
        );
    }

    /// An update that fails to parse returns the error and leaves the book as it was.
    #[rstest]
    fn test_an_update_that_fails_to_parse_leaves_the_book() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let depths = confirmed(&mut state, &instrument, 10);
        feed(&mut state, &instrument, &depths, 0, TS);
        let (mut buffer, mut scratch) = (String::new(), String::new());
        let before = compute_checksum(
            &state.books[&instrument_id],
            1,
            8,
            &mut buffer,
            &mut scratch,
        );

        let mut update = book_data(GUIDE_UPDATE);
        update.timestamp = "1969-12-31T23:59:59Z".parse().unwrap();
        let result = state.process_book(&update, &instrument, 21, false, &depths, at(1));

        assert!(result.is_err());
        assert_eq!(
            compute_checksum(
                &state.books[&instrument_id],
                1,
                8,
                &mut buffer,
                &mut scratch
            ),
            before
        );
    }

    /// An update with no book under the live stream is never applied: it is dropped and the
    /// instrument waits for a snapshot.
    #[rstest]
    fn test_an_update_with_no_book_is_dropped_and_awaited() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let depths = confirmed(&mut state, &instrument, 10);
        state.awaiting_snapshot.clear();

        let outcome = state
            .process_book(
                &book_data(GUIDE_UPDATE),
                &instrument,
                0,
                false,
                &depths,
                at(1),
            )
            .unwrap();

        assert!(outcome.deltas.is_none() && outcome.resync.is_none());
        assert!(!state.books.contains_key(&instrument_id));
        assert_eq!(
            state.awaiting_snapshot[&instrument_id],
            SnapshotWait::fresh(at(1), held_subscription(&depths).generation)
        );
    }

    /// A frame with no subscription behind it is emitted as received: no checksum validation, no
    /// pruning, no recovery and no wait, since there is no subscription to recover, and the
    /// instrument is warned about once.
    #[rstest]
    fn test_a_frame_without_a_subscription_is_emitted_unvalidated() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let unheld = L2Depths::default();
        let mut bad = book_data(GUIDE_SNAPSHOT);
        bad.checksum = Some(1);

        let snapshot = state
            .process_book(&bad, &instrument, 0, true, &unheld, TS)
            .unwrap();
        assert!(snapshot.resync.is_none());
        let (deltas, next_sequence) = snapshot.deltas.expect("the snapshot is emitted");
        assert_eq!(deltas.deltas.len(), 21);
        assert!(RecordFlag::F_LAST.matches(deltas.deltas.last().unwrap().flags));
        assert_eq!(next_sequence, 21);
        assert_eq!(state.unsubscribed_warned.len(), 1);
        assert!(state.unsubscribed_warned.contains(&instrument_id));

        let update = state
            .process_book(
                &book_data(GUIDE_UPDATE),
                &instrument,
                21,
                false,
                &unheld,
                TS,
            )
            .unwrap();
        assert!(update.resync.is_none());
        let (deltas, next_sequence) = update.deltas.expect("the update is emitted");
        assert_eq!(deltas.deltas.len(), 1);
        assert!(RecordFlag::F_LAST.matches(deltas.deltas[0].flags));
        assert_eq!(next_sequence, 22);
        assert_eq!(
            state.unsubscribed_warned.len(),
            1,
            "the second frame does not warn again"
        );

        assert!(state.books.is_empty());
        assert!(state.awaiting_snapshot.is_empty());
        assert!(state.last_generation.is_empty());
        assert!(state.live_request.is_empty());
        assert!(state.mismatches.is_empty());
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
        assert!(
            outcome.resync.is_some(),
            "six-decimal prices cannot reproduce the venue's checksum"
        );
    }
}
