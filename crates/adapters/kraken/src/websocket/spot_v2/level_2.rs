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
    parse::parse_book_deltas,
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

    /// Records a new subscription at `depth` and returns its generation.
    pub(crate) fn insert(&self, symbol: &str, depth: u32) -> u64 {
        let generation = self.next_generation.fetch_add(1, Ordering::Relaxed);
        self.depths
            .insert(symbol.to_string(), L2Subscription { depth, generation });
        generation
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

/// Shadow books for the Spot `book` channel, one per instrument.
///
/// `Default` leaves checksum validation off; the data client enables it from its configuration.
#[derive(Debug)]
pub(crate) struct L2BookState {
    pub(crate) books: AHashMap<InstrumentId, OrderBook>,
    validate_checksum: bool,
    /// Instruments with a cleared book after a mismatch. Their updates are dropped until the
    /// snapshot the resubscription produces, since they describe a stream the venue has ended.
    awaiting_snapshot: AHashSet<InstrumentId>,
    /// Consecutive mismatches per instrument; a valid message resets the count.
    mismatches: AHashMap<InstrumentId, u32>,
    /// Instruments whose validation is off after too many consecutive mismatches.
    validation_disabled: AHashSet<InstrumentId>,
}

impl Default for L2BookState {
    fn default() -> Self {
        Self::new(false)
    }
}

impl L2BookState {
    pub(crate) fn new(validate_checksum: bool) -> Self {
        Self {
            books: AHashMap::new(),
            validate_checksum,
            awaiting_snapshot: AHashSet::new(),
            mismatches: AHashMap::new(),
            validation_disabled: AHashSet::new(),
        }
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
        let instrument_id = instrument.id();
        let depth = subscription.map(|s| s.depth);

        if is_snapshot {
            self.awaiting_snapshot.remove(&instrument_id);
        } else if self.awaiting_snapshot.contains(&instrument_id) {
            log::debug!(
                "Dropping L2 update for {} while awaiting the snapshot after a checksum mismatch",
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
        } else if let Some(depth) = depth {
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
                self.awaiting_snapshot.insert(instrument_id);

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
                        depth,
                        generation: subscription.map(|s| s.generation),
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
