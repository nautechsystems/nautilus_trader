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

//! Book sequencing, snapshot replacement, and per-market recovery state.

use ahash::{AHashMap, AHashSet};
use futures_util::stream::FuturesUnordered;
use nautilus_core::nanos::UnixNanos;
use nautilus_live::book::{
    BookSequenceOutcome,
    recovery::{BookRecoveryOutcome, BookRecoveryState},
    snapshot::PendingSnapshot,
};
use nautilus_model::instruments::InstrumentAny;
use rust_decimal::Decimal;

use super::recovery::{BookWork, BookWrite};
use crate::{
    http::models::LighterPriceLevel,
    websocket::{
        error::LighterWsError,
        messages::{LighterWsOrderBook, NautilusWsMessage},
        parse::{parse_ws_order_book_deltas, parse_ws_order_book_depth},
    },
};

#[derive(Default)]
pub(crate) struct BookSyncTracker {
    pub(crate) delta_subs: AHashSet<i64>,
    pub(crate) depth_subs: AHashSet<i64>,
    pub(crate) snapshots_seen: AHashSet<i64>,
    pub(crate) states: AHashMap<i64, CachedOrderBook>,
    pub(crate) recovery: AHashMap<i64, BookRecoveryState<LighterWsError>>,
    pub(crate) work: FuturesUnordered<BookWork>,
    pub(crate) initial: AHashMap<i64, PendingSnapshot>,
    pub(crate) writes: AHashMap<i64, BookWrite>,
    pub(crate) expected: AHashMap<i64, (u64, u64)>,
    pub(crate) trailing: AHashMap<i64, (u64, u64)>,
}

impl BookSyncTracker {
    pub(crate) fn validate_sequence(
        &mut self,
        market_index: i64,
        book: &LighterWsOrderBook,
        is_snapshot: bool,
    ) -> BookSequenceOutcome {
        if !self.delta_subs.contains(&market_index) && !self.depth_subs.contains(&market_index) {
            return BookSequenceOutcome::Suppress;
        }

        if self.recovery.get(&market_index).is_some_and(|state| {
            state.is_failed()
                || state.current().is_some_and(|recovery| {
                    !recovery.is_accepted() && (!is_snapshot || recovery.gate.lock().is_closed())
                })
        }) {
            return BookSequenceOutcome::Suppress;
        }

        // The venue tags the initial book as `subscribed/order_book` and follows
        // up with `update/order_book` for incrementals. An incremental cannot
        // seed the book because it only carries changed levels.
        if !is_snapshot && !self.snapshots_seen.contains(&market_index) {
            log::warn!(
                "Dropping Lighter order_book update before snapshot for market_index={market_index}",
            );
            return BookSequenceOutcome::Suppress;
        }

        if is_snapshot {
            return BookSequenceOutcome::Accept;
        }

        if let Some(cached_nonce) = self.states.get(&market_index).map(|state| state.book.nonce) {
            if book.begin_nonce != cached_nonce {
                log::warn!(
                    "Dropping Lighter order_book update with nonce gap for \
                     market_index={market_index}: begin_nonce={}, cached_nonce={cached_nonce}",
                    book.begin_nonce,
                );
                self.clear_cached_order_book(market_index);
                return BookSequenceOutcome::Recover;
            }
        } else {
            log::warn!(
                "Dropping Lighter order_book update without cached state for \
                 market_index={market_index}",
            );
            self.clear_cached_order_book(market_index);
            return BookSequenceOutcome::Recover;
        }

        BookSequenceOutcome::Accept
    }

    pub(crate) fn apply(
        &mut self,
        market_index: i64,
        instrument: &InstrumentAny,
        book: &LighterWsOrderBook,
        timestamp: u64,
        is_snapshot: bool,
        ts_init: UnixNanos,
    ) -> Vec<NautilusWsMessage> {
        let snapshot_messages = is_snapshot.then(|| {
            self.order_book_messages(market_index, book, instrument, timestamp, true, ts_init)
        });

        if snapshot_messages.as_ref().is_some_and(Vec::is_empty) {
            return Vec::new();
        }

        if is_snapshot {
            if let Some(recovery) = self
                .recovery
                .get(&market_index)
                .and_then(BookRecoveryState::current)
                && !recovery.is_accepted()
                && !recovery.accept()
            {
                return Vec::new();
            }

            self.initial.remove(&market_index);
            self.snapshots_seen.insert(market_index);
            self.states.insert(
                market_index,
                CachedOrderBook {
                    book: book.clone(),
                    timestamp,
                },
            );
        } else if let Some(state) = self.states.get_mut(&market_index) {
            apply_order_book_update(&mut state.book, book);
            state.timestamp = timestamp;
        }

        snapshot_messages.unwrap_or_else(|| {
            self.order_book_messages(market_index, book, instrument, timestamp, false, ts_init)
        })
    }

    pub(crate) fn clear_cached_order_book(&mut self, market_index: i64) {
        self.snapshots_seen.remove(&market_index);
        self.states.remove(&market_index);
    }

    pub(crate) fn cancel(&mut self, market_index: i64) {
        self.recovery.remove(&market_index);
        self.expected.remove(&market_index);
        self.writes.remove(&market_index);
        self.initial.remove(&market_index);
    }

    pub(crate) fn reset_on_reconnect(&mut self) {
        self.snapshots_seen.clear();
        self.states.clear();
        self.expected.clear();
        self.trailing.clear();
        self.initial.clear();
        self.writes.clear();

        for state in self.recovery.values_mut() {
            if let Some(recovery) = state.current()
                && !recovery.is_accepted()
                && !recovery.cancellation.is_cancelled()
            {
                recovery.gate.lock().close();
                recovery.outcome.send_replace(BookRecoveryOutcome::Rejected(
                    LighterWsError::Network("socket reconnected during book recovery".into()),
                ));
            } else {
                state.reset();
            }
        }
    }

    pub(crate) fn emit_cached_order_book_deltas_snapshot(
        &self,
        market_index: i64,
        instrument: &InstrumentAny,
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        let cached = self.states.get(&market_index)?;
        match parse_ws_order_book_deltas(&cached.book, instrument, cached.timestamp, true, ts_init)
        {
            Ok(deltas) => Some(NautilusWsMessage::Deltas(deltas)),
            Err(e) => {
                log::error!("Error parsing cached Lighter order_book deltas: {e}");
                None
            }
        }
    }

    pub(crate) fn emit_cached_order_book_depth_snapshot(
        &self,
        market_index: i64,
        instrument: &InstrumentAny,
        ts_init: UnixNanos,
    ) -> Option<NautilusWsMessage> {
        let cached = self.states.get(&market_index)?;
        match parse_ws_order_book_depth(&cached.book, instrument, cached.timestamp, ts_init) {
            Ok(depth) => Some(NautilusWsMessage::Depth(Box::new(depth))),
            Err(e) => {
                log::error!("Error parsing cached Lighter order_book depth: {e}");
                None
            }
        }
    }

    fn order_book_messages(
        &self,
        market_index: i64,
        book: &LighterWsOrderBook,
        instrument: &InstrumentAny,
        timestamp: u64,
        is_snapshot: bool,
        ts_init: UnixNanos,
    ) -> Vec<NautilusWsMessage> {
        let mut messages = Vec::new();

        if self.delta_subs.contains(&market_index) {
            match parse_ws_order_book_deltas(book, instrument, timestamp, is_snapshot, ts_init) {
                Ok(deltas) => messages.push(NautilusWsMessage::Deltas(deltas)),
                Err(e) => log::error!("Error parsing Lighter order_book deltas: {e}"),
            }
        }

        let depth_book = if is_snapshot {
            Some((book, timestamp))
        } else {
            self.states
                .get(&market_index)
                .map(|cached| (&cached.book, cached.timestamp))
        };

        if self.depth_subs.contains(&market_index)
            && let Some((book, timestamp)) = depth_book
        {
            match parse_ws_order_book_depth(book, instrument, timestamp, ts_init) {
                Ok(depth) => messages.push(NautilusWsMessage::Depth(Box::new(depth))),
                Err(e) => log::error!("Error parsing Lighter order_book depth: {e}"),
            }
        }

        messages
    }
}

#[derive(Debug)]
pub(crate) struct CachedOrderBook {
    pub(crate) book: LighterWsOrderBook,
    pub(crate) timestamp: u64,
}

fn apply_order_book_update(state: &mut LighterWsOrderBook, update: &LighterWsOrderBook) {
    apply_book_side_update(&mut state.bids, &update.bids, true);
    apply_book_side_update(&mut state.asks, &update.asks, false);

    state.code = update.code;
    state.offset = update.offset;
    state.nonce = update.nonce;
    state.last_updated_at = update.last_updated_at;
    state.begin_nonce = update.begin_nonce;
}

fn apply_book_side_update(
    levels: &mut Vec<LighterPriceLevel>,
    updates: &[LighterPriceLevel],
    bids: bool,
) {
    for update in updates {
        if update.price == Decimal::ZERO {
            continue;
        }

        match find_book_level(levels, update.price, bids) {
            Ok(index) if update.size == Decimal::ZERO => {
                levels.remove(index);
            }
            Ok(index) => {
                levels[index] = update.clone();
            }
            Err(_) if update.size == Decimal::ZERO => {}
            Err(index) => {
                levels.insert(index, update.clone());
            }
        }
    }
}

fn find_book_level(
    levels: &[LighterPriceLevel],
    price: Decimal,
    bids: bool,
) -> Result<usize, usize> {
    levels.binary_search_by(|level| {
        if bids {
            price.cmp(&level.price)
        } else {
            level.price.cmp(&price)
        }
    })
}
