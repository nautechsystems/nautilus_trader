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

use super::{
    checksum::{crc32_ieee, push_scaled},
    messages::KrakenWsBookData,
    parse::{datetime_to_nanos, parse_book_deltas},
};
use crate::common::consts::KRAKEN_PAIR_DECIMALS_KEY;

/// One logical `book` subscription: its depth and a generation that changes with every
/// resubscription, so a recovery queued for a retired subscription can tell it is retired.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct L2Subscription {
    pub(crate) depth: u32,
    pub(crate) generation: u64,
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

    /// Records a new subscription at `depth` under a fresh generation.
    pub(crate) fn insert(&self, symbol: &str, depth: u32) {
        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed);
        self.depths
            .insert(symbol.to_string(), L2Subscription { depth, generation });
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

/// A resubscription the data client issues after a checksum mismatch.
///
/// `generation` names the subscription the mismatch belonged to, so the recovery leaves a
/// replacement subscription alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct L2ResyncRequest {
    pub(crate) instrument_id: InstrumentId,
    pub(crate) depth: Option<u32>,
    pub(crate) generation: Option<u64>,
}

/// What processing one `book` message produced.
#[derive(Debug, Default)]
pub(crate) struct L2BookOutcome {
    pub(crate) deltas: Option<(OrderBookDeltas, u64)>,
    pub(crate) resync: Option<L2ResyncRequest>,
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
/// `since` is when the latest request for the snapshot went out and `attempts` how many the
/// watchdog has made, so the next one is due `SNAPSHOT_TIMEOUT_NS << attempts` after `since`.
/// A snapshot or a cancelled subscription removes the entry; at `MAX_SNAPSHOT_REQUESTS` it stays
/// without further requests and the book stays cleared until the next subscription change or
/// reconnect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SnapshotWait {
    pub(crate) since: UnixNanos,
    pub(crate) attempts: u32,
}

/// Shadow books for the Spot `book` channel, one per instrument.
///
/// `Default` leaves checksum validation off; the data client enables it from its configuration.
#[derive(Debug)]
pub(crate) struct L2BookState {
    pub(crate) books: AHashMap<InstrumentId, OrderBook>,
    validate_checksum: bool,
    /// Instruments with a cleared book. Their updates are dropped until the snapshot their
    /// subscription owes, since they describe a stream the venue has ended.
    awaiting_snapshot: AHashMap<InstrumentId, SnapshotWait>,
    /// Consecutive mismatches per instrument; a valid message resets the count.
    mismatches: AHashMap<InstrumentId, u32>,
    /// Instruments whose validation is off after too many consecutive mismatches.
    validation_disabled: AHashSet<InstrumentId>,
    /// The subscription generation last seen per instrument; a new one re-enables validation.
    last_generation: AHashMap<InstrumentId, u64>,
}

impl L2BookState {
    pub(crate) fn new(validate_checksum: bool) -> Self {
        Self {
            books: AHashMap::new(),
            validate_checksum,
            awaiting_snapshot: AHashMap::new(),
            mismatches: AHashMap::new(),
            validation_disabled: AHashSet::new(),
            last_generation: AHashMap::new(),
        }
    }

    /// Drops the shadow books and the mismatch counts after a reconnect.
    ///
    /// The replayed subscriptions deliver fresh snapshots and the wait for each of them starts at
    /// `now`, so a replay the venue drops is noticed by [`Self::overdue_snapshots`]. The
    /// replacement stream gets the full allowance of mismatches; an instrument whose validation
    /// is off stays off.
    pub(crate) fn reset_after_reconnect(&mut self, now: UnixNanos) {
        for instrument_id in self.books.keys() {
            self.awaiting_snapshot
                .entry(*instrument_id)
                .or_insert(SnapshotWait {
                    since: now,
                    attempts: 0,
                });
        }

        for wait in self.awaiting_snapshot.values_mut() {
            *wait = SnapshotWait {
                since: now,
                attempts: 0,
            };
        }

        self.books.clear();
        self.mismatches.clear();
    }

    /// Requests the snapshot again for every held subscription whose book is overdue.
    ///
    /// `held` is every `book` subscription the client holds. Per instrument:
    /// - not held: the wait is dropped, since a cancelled subscription delivers nothing;
    /// - held with a shadow book: the snapshot has arrived, nothing is owed (an instrument whose
    ///   validation is off keeps its book, so it is never overdue);
    /// - held without a book and without a wait: a wait starts at `now`, which covers a subscribe
    ///   or a reconnect replay whose snapshot the venue dropped, with at most one tick of slack;
    /// - held without a book, fewer than `MAX_SNAPSHOT_REQUESTS` made: once
    ///   `SNAPSHOT_TIMEOUT_NS << attempts` has passed since `since`, a request carrying the held
    ///   generation and depth is returned and the wait restarts at `now` with one more attempt; the
    ///   request that reaches the cap is still made and logs the error;
    /// - at the cap: no further request, and updates keep being dropped until a snapshot arrives.
    pub(crate) fn overdue_snapshots(
        &mut self,
        now: UnixNanos,
        held: &[(InstrumentId, L2Subscription)],
    ) -> Vec<L2ResyncRequest> {
        self.awaiting_snapshot
            .retain(|instrument_id, _| held.iter().any(|(held_id, _)| held_id == instrument_id));

        let mut requests = Vec::new();

        for (instrument_id, subscription) in held {
            if self.books.contains_key(instrument_id) {
                continue;
            }

            let wait = self
                .awaiting_snapshot
                .entry(*instrument_id)
                .or_insert(SnapshotWait {
                    since: now,
                    attempts: 0,
                });

            if wait.attempts >= MAX_SNAPSHOT_REQUESTS {
                continue;
            }

            let timeout_ns = SNAPSHOT_TIMEOUT_NS << wait.attempts;
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

            requests.push(L2ResyncRequest {
                instrument_id: *instrument_id,
                depth: Some(subscription.depth),
                generation: Some(subscription.generation),
            });
        }

        requests
    }

    pub(crate) fn process_book(
        &mut self,
        book: &KrakenWsBookData,
        instrument: &InstrumentAny,
        sequence: u64,
        is_snapshot: bool,
        subscription: Option<L2Subscription>,
        ts_init: UnixNanos,
    ) -> anyhow::Result<L2BookOutcome> {
        let Some(subscription) = subscription else {
            // A frame consumed after the user unsubscribed belongs to a stream that is ending.
            log::debug!(
                "Dropping L2 {} for {}: no subscription holds it",
                if is_snapshot { "snapshot" } else { "update" },
                book.symbol
            );
            return Ok(L2BookOutcome::default());
        };
        let instrument_id = instrument.id();
        let depth = subscription.depth;

        // A new subscription is a new stream: its validation starts afresh, so an instrument the
        // cap switched off is validated again once the user resubscribes.
        if self
            .last_generation
            .insert(instrument_id, subscription.generation)
            != Some(subscription.generation)
        {
            self.validation_disabled.remove(&instrument_id);
            self.mismatches.remove(&instrument_id);

            if !is_snapshot {
                // A new stream opens with its snapshot. An update seen first is one queued under
                // the retired subscription and consumed under this one; applying it would put the
                // retired stream's levels into the replacement's book, so the book is cleared
                // and the stream waits for the snapshot.
                log::debug!(
                    "Dropping L2 update for {} from a retired subscription: awaiting the \
                     replacement's snapshot",
                    book.symbol
                );
                self.awaiting_snapshot
                    .entry(instrument_id)
                    .or_insert(SnapshotWait {
                        since: ts_init,
                        attempts: 0,
                    });

                if self.books.remove(&instrument_id).is_some() {
                    let ts_event = datetime_to_nanos(book.timestamp, "book.timestamp")?;
                    let mut clear =
                        OrderBookDelta::clear(instrument_id, sequence, ts_event, ts_init);
                    clear.flags |= RecordFlag::F_LAST as u8;

                    return Ok(L2BookOutcome {
                        deltas: Some((
                            OrderBookDeltas::new(instrument_id, vec![clear]),
                            sequence + 1,
                        )),
                        resync: None,
                    });
                }

                return Ok(L2BookOutcome::default());
            }
        }

        if is_snapshot {
            self.awaiting_snapshot.remove(&instrument_id);
        } else if self.awaiting_snapshot.contains_key(&instrument_id) {
            log::debug!(
                "Dropping L2 update for {} while awaiting the snapshot",
                book.symbol
            );
            return Ok(L2BookOutcome::default());
        }

        let mut deltas = parse_book_deltas(book, instrument, sequence, is_snapshot, ts_init)?;
        if deltas.is_empty() {
            return Ok(L2BookOutcome::default());
        }

        let mut next_sequence = sequence + deltas.len() as u64;
        let book_state = self
            .books
            .entry(instrument_id)
            .or_insert_with(|| OrderBook::new(instrument_id, BookType::L2_MBP));

        if let Err(e) =
            book_state.apply_deltas(&OrderBookDeltas::new(instrument_id, deltas.clone()))
        {
            log::error!("Failed to apply Kraken L2 deltas to shadow book: {e}");
        } else {
            prune_deltas_to_depth(
                book_state,
                depth,
                is_snapshot,
                &mut next_sequence,
                ts_init,
                &mut deltas,
            );
        }

        // The venue hashes its top ten levels per side, which pruning to the subscribed depth
        // leaves intact, so the shadow book is compared after the message has been applied.
        let validate = self.validate_checksum && !self.validation_disabled.contains(&instrument_id);
        let mismatch = match (validate, book.checksum) {
            (true, Some(remote)) => {
                // Scales come from the instrument on every message, so a refreshed definition
                // takes effect at once.
                let local = compute_checksum(
                    book_state,
                    price_wire_scale(instrument),
                    instrument.size_precision(),
                );
                (local != remote).then_some((local, remote))
            }
            _ => None,
        };

        if let Some((local, remote)) = mismatch {
            let bids = book_state.bids(None).count();
            let asks = book_state.asks(None).count();
            let consecutive = self.mismatches.entry(instrument_id).or_insert(0);
            *consecutive += 1;

            if *consecutive >= MAX_CONSECUTIVE_CHECKSUM_MISMATCHES {
                // The shadow book cannot reproduce the venue's hash for this instrument, so another
                // resubscription would only repeat the cycle; keep the book and stop validating it.
                log::error!(
                    "L2 book checksum mismatched {consecutive} times in a row: symbol={}, \
                     local={local}, remote={remote}; validation disabled for this instrument and \
                     the book kept as received",
                    book.symbol,
                );
                self.validation_disabled.insert(instrument_id);
                self.mismatches.remove(&instrument_id);
            } else {
                log::warn!(
                    "L2 book checksum mismatch: symbol={}, local={local}, remote={remote}, \
                     bids={bids}, asks={asks}; clearing the book and resubscribing",
                    book.symbol,
                );
                self.books.remove(&instrument_id);
                self.awaiting_snapshot.insert(
                    instrument_id,
                    SnapshotWait {
                        since: ts_init,
                        attempts: 0,
                    },
                );

                let ts_event = deltas.last().map_or(ts_init, |delta| delta.ts_event);
                let mut clear =
                    OrderBookDelta::clear(instrument_id, next_sequence, ts_event, ts_init);
                next_sequence += 1;
                clear.flags |= RecordFlag::F_LAST as u8;

                return Ok(L2BookOutcome {
                    deltas: Some((
                        OrderBookDeltas::new(instrument_id, vec![clear]),
                        next_sequence,
                    )),
                    resync: Some(L2ResyncRequest {
                        instrument_id,
                        depth: Some(depth),
                        generation: Some(subscription.generation),
                    }),
                });
            }
        } else if validate && book.checksum.is_some() && !is_snapshot {
            self.mismatches.remove(&instrument_id);
        }

        set_last_delta_flag(&mut deltas);
        Ok(L2BookOutcome {
            deltas: Some((OrderBookDeltas::new(instrument_id, deltas), next_sequence)),
            resync: None,
        })
    }
}

/// The scale the venue sends prices at.
///
/// `AssetPairs` declares it as `pair_decimals`, carried on the instrument only when it differs from
/// the tick-size precision the instrument's prices use.
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
/// wire-scale quantity, per the venue's documented algorithm.
pub(crate) fn compute_checksum(book: &OrderBook, price_scale: u8, qty_scale: u8) -> u32 {
    let mut s = String::with_capacity(512);
    let mut scratch = String::with_capacity(32);

    for level in book.asks(Some(10)).chain(book.bids(Some(10))) {
        push_scaled(
            &mut s,
            &mut scratch,
            level.price.value.as_decimal(),
            price_scale,
        );
        push_scaled(&mut s, &mut scratch, level.size_decimal(), qty_scale);
    }

    crc32_ieee(s.as_bytes())
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

    fn sub(depth: u32) -> L2Subscription {
        L2Subscription {
            depth,
            generation: 7,
        }
    }

    fn book_data(json: &str) -> KrakenWsBookData {
        let message: KrakenWsRawMessage = serde_json::from_str(json).unwrap();
        serde_json::from_str(message.data[0].get()).unwrap()
    }

    fn level(price: Decimal, qty: Decimal) -> KrakenWsBookLevel {
        KrakenWsBookLevel { price, qty }
    }

    #[rstest]
    fn test_guide_snapshot_checksum_matches() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let snapshot = book_data(GUIDE_SNAPSHOT);
        assert_eq!(snapshot.checksum, Some(3_310_070_434));

        let outcome = state
            .process_book(&snapshot, &instrument, 0, true, Some(sub(10)), TS)
            .unwrap();

        assert!(
            outcome.resync.is_none(),
            "the documented example must validate"
        );
        let (deltas, _) = outcome.deltas.expect("snapshot deltas");
        assert_eq!(deltas.deltas.len(), 21);
        let book = &state.books[&instrument.id()];
        assert_eq!(compute_checksum(book, 1, 8), 3_310_070_434);
    }

    #[rstest]
    fn test_update_checksum_is_computed_over_the_shadow_book() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        state
            .process_book(
                &book_data(GUIDE_SNAPSHOT),
                &instrument,
                0,
                true,
                Some(sub(10)),
                TS,
            )
            .unwrap();

        let update = book_data(GUIDE_UPDATE);
        assert_eq!(update.checksum, Some(38_355_977));
        let outcome = state
            .process_book(&update, &instrument, 21, false, Some(sub(10)), TS)
            .unwrap();

        assert!(
            outcome.resync.is_none(),
            "an update validated against the whole book"
        );
        assert_eq!(outcome.deltas.expect("update deltas").0.deltas.len(), 1);
    }

    /// A mismatch clears the book, emits one `Clear`, requests a resubscription, and drops updates
    /// until the next snapshot.
    #[rstest]
    fn test_mismatch_clears_the_book_and_requests_resync() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let mut snapshot = book_data(GUIDE_SNAPSHOT);
        snapshot.checksum = Some(1);

        let outcome = state
            .process_book(&snapshot, &instrument, 0, true, Some(sub(25)), TS)
            .unwrap();

        assert_eq!(
            outcome.resync,
            Some(L2ResyncRequest {
                instrument_id: instrument.id(),
                depth: Some(25),
                generation: Some(7),
            })
        );
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
            .process_book(&update, &instrument, 22, false, Some(sub(25)), TS)
            .unwrap();
        assert!(dropped.deltas.is_none() && dropped.resync.is_none());

        let fresh = state
            .process_book(
                &book_data(GUIDE_SNAPSHOT),
                &instrument,
                22,
                true,
                Some(sub(25)),
                TS,
            )
            .unwrap();
        assert!(fresh.resync.is_none());
        assert!(
            fresh.deltas.is_some(),
            "the next snapshot resumes the stream"
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
        let mut bad = book_data(GUIDE_SNAPSHOT);
        bad.checksum = Some(1);

        for strike in 1..MAX_CONSECUTIVE_CHECKSUM_MISMATCHES {
            let outcome = state
                .process_book(&bad, &instrument, 0, true, Some(sub(10)), TS)
                .unwrap();
            assert!(outcome.resync.is_some(), "strike {strike} resubscribes");
        }

        let final_strike = state
            .process_book(&bad, &instrument, 0, true, Some(sub(10)), TS)
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
            .process_book(&bad, &instrument, 21, true, Some(sub(10)), TS)
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
        let mut bad = book_data(GUIDE_SNAPSHOT);
        bad.checksum = Some(1);

        for _ in 0..MAX_CONSECUTIVE_CHECKSUM_MISMATCHES {
            state
                .process_book(&bad, &instrument, 0, true, Some(sub(10)), TS)
                .unwrap();
        }
        assert!(
            state
                .process_book(&bad, &instrument, 0, true, Some(sub(10)), TS)
                .unwrap()
                .resync
                .is_none(),
            "validation is off for the subscription that struck out"
        );

        let resubscribed = Some(L2Subscription {
            depth: 10,
            generation: 8,
        });
        let outcome = state
            .process_book(&bad, &instrument, 0, true, resubscribed, TS)
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
        let mut bad = book_data(GUIDE_SNAPSHOT);
        bad.checksum = Some(1);

        for _ in 0..(MAX_CONSECUTIVE_CHECKSUM_MISMATCHES - 1) {
            state
                .process_book(&bad, &instrument, 0, true, Some(sub(10)), TS)
                .unwrap();
        }

        state.reset_after_reconnect(TS);

        let outcome = state
            .process_book(&bad, &instrument, 0, true, Some(sub(10)), TS)
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
        let good = book_data(GUIDE_SNAPSHOT);
        let update = book_data(GUIDE_UPDATE);
        let mut bad = good.clone();
        bad.checksum = Some(1);

        for _ in 0..(MAX_CONSECUTIVE_CHECKSUM_MISMATCHES * 2) {
            assert!(
                state
                    .process_book(&bad, &instrument, 0, true, Some(sub(10)), TS)
                    .unwrap()
                    .resync
                    .is_some(),
                "each mismatch after a valid update resubscribes"
            );
            assert!(
                state
                    .process_book(&good, &instrument, 0, true, Some(sub(10)), TS)
                    .unwrap()
                    .resync
                    .is_none()
            );
            assert!(
                state
                    .process_book(&update, &instrument, 21, false, Some(sub(10)), TS)
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
        let good = book_data(GUIDE_SNAPSHOT);
        let mut bad_update = book_data(GUIDE_UPDATE);
        bad_update.checksum = Some(1);

        for strike in 1..MAX_CONSECUTIVE_CHECKSUM_MISMATCHES {
            assert!(
                state
                    .process_book(&good, &instrument, 0, true, Some(sub(10)), TS)
                    .unwrap()
                    .resync
                    .is_none()
            );
            assert!(
                state
                    .process_book(&bad_update, &instrument, 21, false, Some(sub(10)), TS)
                    .unwrap()
                    .resync
                    .is_some(),
                "strike {strike} resubscribes"
            );
        }

        state
            .process_book(&good, &instrument, 0, true, Some(sub(10)), TS)
            .unwrap();
        let final_strike = state
            .process_book(&bad_update, &instrument, 21, false, Some(sub(10)), TS)
            .unwrap();

        assert!(
            final_strike.resync.is_none(),
            "the third mismatch stops resubscribing although each snapshot validated"
        );
        assert!(final_strike.deltas.is_some(), "the update is kept");
    }

    /// `secs` seconds after `TS`.
    fn at(secs: u64) -> UnixNanos {
        UnixNanos::new(TS.as_u64() + secs * 1_000_000_000)
    }

    /// Clears the book with a mismatching snapshot at `TS`, so the wait for the snapshot starts.
    fn mismatch_at_ts(state: &mut L2BookState, instrument: &InstrumentAny) {
        let mut bad = book_data(GUIDE_SNAPSHOT);
        bad.checksum = Some(1);
        let outcome = state
            .process_book(&bad, instrument, 0, true, Some(sub(10)), TS)
            .unwrap();
        assert!(outcome.resync.is_some(), "the mismatch requests a resync");
        assert!(!state.books.contains_key(&instrument.id()));
    }

    /// A book whose snapshot arrives inside the wait is never requested again, before or after
    /// the snapshot.
    #[rstest]
    fn test_a_snapshot_within_the_wait_is_not_requested_again() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let held = [(instrument.id(), sub(10))];
        mismatch_at_ts(&mut state, &instrument);

        assert!(state.overdue_snapshots(at(5), &held).is_empty());

        let fresh = state
            .process_book(
                &book_data(GUIDE_SNAPSHOT),
                &instrument,
                22,
                true,
                Some(sub(10)),
                at(6),
            )
            .unwrap();
        assert!(fresh.deltas.is_some() && fresh.resync.is_none());

        assert!(state.overdue_snapshots(at(60), &held).is_empty());
        assert!(!state.awaiting_snapshot.contains_key(&instrument.id()));
    }

    /// A cleared book whose snapshot is late is requested again under the held generation and
    /// depth, and not again before the doubled wait has passed.
    #[rstest]
    fn test_an_overdue_snapshot_is_requested_again_once_per_doubled_wait() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let held = [(instrument_id, sub(10))];
        mismatch_at_ts(&mut state, &instrument);

        assert!(state.overdue_snapshots(at(9), &held).is_empty());

        let requests = state.overdue_snapshots(at(10), &held);
        assert_eq!(
            requests,
            vec![L2ResyncRequest {
                instrument_id,
                depth: Some(10),
                generation: Some(7),
            }]
        );
        assert_eq!(
            state.awaiting_snapshot[&instrument_id],
            SnapshotWait {
                since: at(10),
                attempts: 1,
            }
        );

        assert!(
            state.overdue_snapshots(at(29), &held).is_empty(),
            "the second wait is twice the first"
        );
        assert_eq!(state.overdue_snapshots(at(30), &held).len(), 1);
        assert_eq!(state.awaiting_snapshot[&instrument_id].attempts, 2);
    }

    /// At the cap no further request goes out; updates are still dropped and a later snapshot
    /// resumes processing.
    #[rstest]
    fn test_the_request_cap_leaves_the_book_cleared_until_a_snapshot() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let held = [(instrument_id, sub(10))];
        mismatch_at_ts(&mut state, &instrument);

        let mut now = TS;
        for attempt in 0..MAX_SNAPSHOT_REQUESTS {
            now = UnixNanos::new(now.as_u64() + (SNAPSHOT_TIMEOUT_NS << attempt));
            assert_eq!(
                state.overdue_snapshots(now, &held).len(),
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
            state.overdue_snapshots(at(100_000), &held).is_empty(),
            "no request beyond the cap"
        );

        let dropped = state
            .process_book(
                &book_data(GUIDE_UPDATE),
                &instrument,
                22,
                false,
                Some(sub(10)),
                at(100_000),
            )
            .unwrap();
        assert!(dropped.deltas.is_none() && dropped.resync.is_none());

        let fresh = state
            .process_book(
                &book_data(GUIDE_SNAPSHOT),
                &instrument,
                22,
                true,
                Some(sub(10)),
                at(100_001),
            )
            .unwrap();
        assert!(fresh.deltas.is_some() && fresh.resync.is_none());
        assert!(!state.awaiting_snapshot.contains_key(&instrument_id));
    }

    /// A late snapshot for a subscription the user replaced is requested under the replacement's
    /// generation and depth, so the recovery acts on the live subscription.
    #[rstest]
    fn test_an_overdue_snapshot_is_requested_for_the_replacement() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        mismatch_at_ts(&mut state, &instrument);

        let replacement = L2Subscription {
            depth: 100,
            generation: 8,
        };
        let requests = state.overdue_snapshots(at(10), &[(instrument_id, replacement)]);

        assert_eq!(
            requests,
            vec![L2ResyncRequest {
                instrument_id,
                depth: Some(100),
                generation: Some(8),
            }]
        );
    }

    /// A cancelled subscription owes no snapshot: its wait is dropped and nothing is requested.
    #[rstest]
    fn test_a_cancelled_subscription_drops_its_wait() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        mismatch_at_ts(&mut state, &instrument);

        assert!(state.overdue_snapshots(at(10), &[]).is_empty());
        assert!(!state.awaiting_snapshot.contains_key(&instrument.id()));
    }

    /// An instrument whose validation is off keeps its book, whether the cap switched it off or the
    /// configuration did, so it is never overdue.
    #[rstest]
    fn test_a_validation_disabled_instrument_is_never_overdue() {
        let instrument = instrument(1, None);
        let held = [(instrument.id(), sub(10))];
        let mut bad = book_data(GUIDE_SNAPSHOT);
        bad.checksum = Some(1);

        let mut struck_out = L2BookState::new(true);
        for _ in 0..MAX_CONSECUTIVE_CHECKSUM_MISMATCHES {
            struck_out
                .process_book(&bad, &instrument, 0, true, Some(sub(10)), TS)
                .unwrap();
        }
        assert!(struck_out.validation_disabled.contains(&instrument.id()));
        assert!(struck_out.books.contains_key(&instrument.id()));
        assert!(struck_out.overdue_snapshots(at(100_000), &held).is_empty());
        assert!(struck_out.awaiting_snapshot.is_empty());

        let mut configured_off = L2BookState::new(false);
        configured_off
            .process_book(&bad, &instrument, 0, true, Some(sub(10)), TS)
            .unwrap();
        assert!(configured_off.books.contains_key(&instrument.id()));
        assert!(
            configured_off
                .overdue_snapshots(at(100_000), &held)
                .is_empty()
        );
        assert!(configured_off.awaiting_snapshot.is_empty());
    }

    /// A held subscription that never produced a book owes a snapshot: the wait starts at the
    /// first check and the request goes out once the timeout has passed.
    #[rstest]
    fn test_a_lost_initial_snapshot_becomes_overdue() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let held = [(instrument_id, sub(10))];

        assert!(state.overdue_snapshots(TS, &held).is_empty());
        assert_eq!(
            state.awaiting_snapshot[&instrument_id],
            SnapshotWait {
                since: TS,
                attempts: 0,
            }
        );
        assert!(state.overdue_snapshots(at(9), &held).is_empty());

        let requests = state.overdue_snapshots(at(10), &held);
        assert_eq!(
            requests,
            vec![L2ResyncRequest {
                instrument_id,
                depth: Some(10),
                generation: Some(7),
            }]
        );
    }

    /// A reconnect arms the wait for every instrument that had a book, so a replay whose snapshot
    /// the venue drops is requested after the timeout.
    #[rstest]
    fn test_a_reconnect_arms_the_wait_for_every_book() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let held = [(instrument_id, sub(10))];
        state
            .process_book(
                &book_data(GUIDE_SNAPSHOT),
                &instrument,
                0,
                true,
                Some(sub(10)),
                TS,
            )
            .unwrap();
        assert!(state.books.contains_key(&instrument_id));

        state.reset_after_reconnect(at(1));

        assert!(state.books.is_empty());
        assert_eq!(
            state.awaiting_snapshot[&instrument_id],
            SnapshotWait {
                since: at(1),
                attempts: 0,
            }
        );
        assert!(state.overdue_snapshots(at(10), &held).is_empty());
        assert_eq!(state.overdue_snapshots(at(11), &held).len(), 1);
    }

    /// A reconnect restarts a wait already under way: the replayed subscription gets the full
    /// allowance of requests, counted from the reconnect.
    #[rstest]
    fn test_a_reconnect_restarts_a_wait_under_way() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let held = [(instrument_id, sub(10))];
        mismatch_at_ts(&mut state, &instrument);
        assert_eq!(state.overdue_snapshots(at(10), &held).len(), 1);
        assert_eq!(state.awaiting_snapshot[&instrument_id].attempts, 1);

        state.reset_after_reconnect(at(15));

        assert_eq!(
            state.awaiting_snapshot[&instrument_id],
            SnapshotWait {
                since: at(15),
                attempts: 0,
            }
        );
        assert!(state.overdue_snapshots(at(24), &held).is_empty());
        assert_eq!(state.overdue_snapshots(at(25), &held).len(), 1);
        assert_eq!(state.awaiting_snapshot[&instrument_id].attempts, 1);
    }

    /// Snapshots that arrive after the watchdog asked again are processed as usual: both the late
    /// one and the requested one apply, request nothing and leave no wait behind.
    #[rstest]
    fn test_snapshots_after_a_watchdog_request_are_processed() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let held = [(instrument_id, sub(10))];
        mismatch_at_ts(&mut state, &instrument);
        assert_eq!(state.overdue_snapshots(at(10), &held).len(), 1);

        for ts in [at(11), at(12)] {
            let outcome = state
                .process_book(
                    &book_data(GUIDE_SNAPSHOT),
                    &instrument,
                    22,
                    true,
                    Some(sub(10)),
                    ts,
                )
                .unwrap();
            assert!(outcome.resync.is_none());
            assert_eq!(
                outcome.deltas.expect("the snapshot applies").0.deltas.len(),
                21
            );
        }

        assert!(state.books.contains_key(&instrument_id));
        assert!(!state.awaiting_snapshot.contains_key(&instrument_id));
        assert!(state.overdue_snapshots(at(100), &held).is_empty());
    }

    /// An update consumed under a generation whose snapshot has not been seen belongs to the
    /// retired stream: it is dropped without validation, the book is cleared downstream, the
    /// instrument waits for the snapshot, and the snapshot opens the stream with validation on.
    #[rstest]
    fn test_an_update_under_a_new_generation_is_dropped_until_its_snapshot() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();
        let retired = L2Subscription {
            depth: 10,
            generation: 1,
        };
        let replacement = L2Subscription {
            depth: 10,
            generation: 2,
        };
        state
            .process_book(
                &book_data(GUIDE_SNAPSHOT),
                &instrument,
                0,
                true,
                Some(retired),
                TS,
            )
            .unwrap();

        let mut bad_update = book_data(GUIDE_UPDATE);
        bad_update.checksum = Some(1);
        let outcome = state
            .process_book(
                &bad_update,
                &instrument,
                21,
                false,
                Some(replacement),
                at(1),
            )
            .unwrap();

        assert!(
            outcome.resync.is_none(),
            "a retired frame must not trigger a recovery against the replacement"
        );
        let (deltas, next_sequence) = outcome.deltas.expect("the downstream book is cleared");
        assert_eq!(deltas.deltas.len(), 1);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert_eq!(deltas.deltas[0].sequence, 21);
        assert!(RecordFlag::F_LAST.matches(deltas.deltas[0].flags));
        assert_eq!(next_sequence, 22);
        assert!(!state.books.contains_key(&instrument_id));
        assert_eq!(
            state.awaiting_snapshot[&instrument_id],
            SnapshotWait {
                since: at(1),
                attempts: 0,
            }
        );

        let snapshot = state
            .process_book(
                &book_data(GUIDE_SNAPSHOT),
                &instrument,
                22,
                true,
                Some(replacement),
                at(2),
            )
            .unwrap();
        assert!(snapshot.resync.is_none());
        assert_eq!(
            snapshot
                .deltas
                .expect("the snapshot applies")
                .0
                .deltas
                .len(),
            21
        );
        assert!(!state.awaiting_snapshot.contains_key(&instrument_id));
        assert!(!state.mismatches.contains_key(&instrument_id));

        let validated = state
            .process_book(
                &bad_update,
                &instrument,
                43,
                false,
                Some(replacement),
                at(3),
            )
            .unwrap();
        assert!(
            validated.resync.is_some(),
            "validation is on for the replacement's stream"
        );
    }

    /// The first frame of a subscription must be its snapshot: an update seen first is dropped and
    /// the instrument waits for the snapshot.
    #[rstest]
    fn test_an_update_as_the_first_frame_is_dropped_and_awaited() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let instrument_id = instrument.id();

        let outcome = state
            .process_book(
                &book_data(GUIDE_UPDATE),
                &instrument,
                0,
                false,
                Some(sub(10)),
                TS,
            )
            .unwrap();

        assert!(outcome.deltas.is_none() && outcome.resync.is_none());
        assert!(!state.books.contains_key(&instrument_id));
        assert_eq!(
            state.awaiting_snapshot[&instrument_id],
            SnapshotWait {
                since: TS,
                attempts: 0,
            }
        );
    }

    /// A frame with no subscription behind it belongs to a stream that is ending: it produces
    /// neither deltas nor a recovery and leaves no state.
    #[rstest]
    fn test_a_frame_without_a_subscription_is_dropped() {
        let mut state = L2BookState::new(true);
        let instrument = instrument(1, None);
        let mut bad = book_data(GUIDE_SNAPSHOT);
        bad.checksum = Some(1);

        let snapshot = state
            .process_book(&bad, &instrument, 0, true, None, TS)
            .unwrap();
        assert!(snapshot.deltas.is_none());
        assert!(snapshot.resync.is_none());

        let update = state
            .process_book(&book_data(GUIDE_UPDATE), &instrument, 0, false, None, TS)
            .unwrap();
        assert!(update.deltas.is_none());
        assert!(update.resync.is_none());

        assert!(state.books.is_empty());
        assert!(state.awaiting_snapshot.is_empty());
        assert!(state.last_generation.is_empty());
    }

    #[rstest]
    fn test_validation_disabled_ignores_a_bad_checksum() {
        let mut state = L2BookState::new(false);
        let instrument = instrument(1, None);
        let mut snapshot = book_data(GUIDE_SNAPSHOT);
        snapshot.checksum = Some(1);

        let outcome = state
            .process_book(&snapshot, &instrument, 0, true, Some(sub(10)), TS)
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

        let mut with_scale = L2BookState::new(true);
        let outcome = with_scale
            .process_book(
                &message,
                &instrument(6, Some(7)),
                0,
                true,
                Some(sub(10)),
                TS,
            )
            .unwrap();
        assert!(
            outcome.resync.is_none(),
            "seven-decimal prices match the venue"
        );

        let mut without_scale = L2BookState::new(true);
        let outcome = without_scale
            .process_book(&message, &instrument(6, None), 0, true, Some(sub(10)), TS)
            .unwrap();
        assert!(
            outcome.resync.is_some(),
            "six-decimal prices cannot reproduce the venue's checksum"
        );
    }
}
