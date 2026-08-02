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

use std::{cell::RefCell, num::NonZeroUsize, rc::Rc};

use indexmap::IndexMap;
use nautilus_common::{
    cache::Cache,
    msgbus::{self, Handler, MStr, Topic, switchboard},
    timer::TimeEvent,
};
use nautilus_model::{
    data::{OrderBookDeltas, OrderBookDepth10, QuoteTick},
    enums::InstrumentClass,
    identifiers::{InstrumentId, Venue},
    instruments::Instrument,
    orderbook::OrderBook,
};
use ustr::Ustr;

/// Contains information for creating snapshots of specific order books.
#[derive(Clone, Debug)]
pub struct BookSnapshotInfo {
    pub instrument_id: InstrumentId,
    pub venue: Venue,
    /// Parent expansion components `(root, class)` when this snapshot subscription
    /// targets a parent symbol. `None` for concrete (exact-instrument) subscriptions.
    pub parent: Option<(Ustr, InstrumentClass)>,
    pub topic: MStr<Topic>,
    pub interval_ms: NonZeroUsize,
}

/// Reference-counted map of per-instrument book snapshot descriptors.
///
/// Shared between the engine (which populates it on subscribe) and the
/// [`BookSnapshotter`] timer callback (which iterates it on each tick).
pub(crate) type BookSnapshotInfos = Rc<RefCell<IndexMap<InstrumentId, BookSnapshotInfo>>>;

/// Reference count key for a book snapshot subscription.
pub(crate) type BookSnapshotKey = (InstrumentId, NonZeroUsize);

/// Outcome of decrementing a book snapshot subscription.
pub(crate) enum BookSnapshotUnsubscribeResult {
    /// No matching subscription was found.
    NotSubscribed,
    /// The reference count was decremented but other consumers remain.
    Decremented,
    /// The last consumer was removed; tear down associated state.
    Removed,
}

/// Handles order book updates and delta processing for a specific instrument.
///
/// The `BookUpdater` processes incoming order book deltas and maintains
/// the current state of an order book. It can handle both incremental
/// updates and full snapshots for the instrument it's assigned to.
#[derive(Debug)]
pub struct BookUpdater {
    pub id: Ustr,
    pub instrument_id: InstrumentId,
    pub cache: Rc<RefCell<Cache>>,
    pub emit_quotes_from_book: bool,
}

impl BookUpdater {
    /// Creates a new [`BookUpdater`] instance.
    pub fn new(
        instrument_id: &InstrumentId,
        cache: Rc<RefCell<Cache>>,
        emit_quotes_from_book: bool,
    ) -> Self {
        Self {
            id: Ustr::from(&format!("{}-{}", stringify!(BookUpdater), instrument_id)),
            instrument_id: *instrument_id,
            cache,
            emit_quotes_from_book,
        }
    }
}

impl Handler<OrderBookDeltas> for BookUpdater {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, deltas: &OrderBookDeltas) {
        let mut emit: Option<QuoteTick> = None;
        {
            let mut cache = self.cache.borrow_mut();
            if let Some(book) = cache.order_book_mut(&deltas.instrument_id) {
                if let Err(e) = book.apply_deltas(deltas) {
                    log::error!("Failed to apply deltas: {e}");
                    return;
                }

                if self.emit_quotes_from_book {
                    emit = derive_quote_from_book(book);
                }
            }
        }

        if let Some(quote) = emit {
            publish_quote_if_changed(&self.cache, quote);
        }
    }
}

impl Handler<OrderBookDepth10> for BookUpdater {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, depth: &OrderBookDepth10) {
        let mut emit: Option<QuoteTick> = None;
        {
            let mut cache = self.cache.borrow_mut();
            if let Some(book) = cache.order_book_mut(&depth.instrument_id) {
                if let Err(e) = book.apply_depth(depth) {
                    log::error!("Failed to apply depth: {e}");
                    return;
                }

                if self.emit_quotes_from_book {
                    emit = derive_quote_from_book(book);
                }
            }
        }

        if let Some(quote) = emit {
            publish_quote_if_changed(&self.cache, quote);
        }
    }
}

fn derive_quote_from_book(book: &OrderBook) -> Option<QuoteTick> {
    let bid_price = book.best_bid_price()?;
    let ask_price = book.best_ask_price()?;
    let bid_size = book.best_bid_size()?;
    let ask_size = book.best_ask_size()?;

    if bid_size.raw == 0 || ask_size.raw == 0 {
        return None;
    }

    Some(QuoteTick::new(
        book.instrument_id,
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        book.ts_last,
        book.ts_last,
    ))
}

/// Publishes the derived `QuoteTick` if top-of-book changed.
///
/// Writes to cache and republishes only when bid/ask price or size differs
/// from the cached quote.
pub(crate) fn publish_quote_if_changed(cache: &Rc<RefCell<Cache>>, quote: QuoteTick) {
    let publish = {
        let cache_ref = cache.borrow();
        match cache_ref.quote(&quote.instrument_id) {
            None => true,
            Some(last) => {
                last.bid_price != quote.bid_price
                    || last.ask_price != quote.ask_price
                    || last.bid_size != quote.bid_size
                    || last.ask_size != quote.ask_size
            }
        }
    };

    if !publish {
        return;
    }

    if let Err(e) = cache.borrow_mut().add_quote(quote) {
        log::error!("Error on cache insert: {e}");
    }

    let topic = switchboard::get_quotes_topic(quote.instrument_id);
    msgbus::publish_quote(topic, &quote);
}

/// Creates periodic snapshots of order books at configured intervals.
///
/// The `BookSnapshotter` generates order book snapshots on timer events,
/// publishing them as market data. This is useful for providing periodic
/// full order book state updates in addition to incremental delta updates.
#[derive(Debug)]
pub struct BookSnapshotter {
    pub timer_name: Ustr,
    pub interval_ms: NonZeroUsize,
    pub snapshot_infos: Rc<RefCell<IndexMap<InstrumentId, BookSnapshotInfo>>>,
    pub cache: Rc<RefCell<Cache>>,
}

impl BookSnapshotter {
    /// Creates a new [`BookSnapshotter`] instance.
    pub fn new(
        interval_ms: NonZeroUsize,
        snapshot_infos: Rc<RefCell<IndexMap<InstrumentId, BookSnapshotInfo>>>,
        cache: Rc<RefCell<Cache>>,
    ) -> Self {
        let timer_name = format!("OrderBookSnapshots|{interval_ms}");

        Self {
            timer_name: Ustr::from(&timer_name),
            interval_ms,
            snapshot_infos,
            cache,
        }
    }

    /// Publishes a snapshot for each subscribed book.
    ///
    /// Books are cloned out of the cache inside a scoped borrow before publishing,
    /// so subscribers can mutably borrow the cache (e.g. a strategy submitting an
    /// order from `on_book`).
    pub fn snapshot(&self, _event: TimeEvent) {
        let snapshot_infos: Vec<BookSnapshotInfo> =
            self.snapshot_infos.borrow().values().cloned().collect();

        log::debug!(
            "BookSnapshotter.snapshot called for {} subscriptions at {}ms",
            snapshot_infos.len(),
            self.interval_ms,
        );

        let books: Vec<(MStr<Topic>, OrderBook)> = {
            let cache = self.cache.borrow();
            let mut books = Vec::new();

            for snap_info in &snapshot_infos {
                self.collect_snapshot(snap_info, &cache, &mut books);
            }

            books
        };

        for (topic, book) in books {
            msgbus::publish_book(topic, &book);
        }
    }

    fn collect_snapshot(
        &self,
        snap_info: &BookSnapshotInfo,
        cache: &Cache,
        books: &mut Vec<(MStr<Topic>, OrderBook)>,
    ) {
        if let Some((root, class)) = snap_info.parent {
            let topic = snap_info.topic;
            for instrument in cache.instruments_by_parent(&snap_info.venue, &root, class) {
                self.collect_order_book(&instrument.id(), topic, cache, books);
            }
        } else {
            self.collect_order_book(&snap_info.instrument_id, snap_info.topic, cache, books);
        }
    }

    fn collect_order_book(
        &self,
        instrument_id: &InstrumentId,
        topic: MStr<Topic>,
        cache: &Cache,
        books: &mut Vec<(MStr<Topic>, OrderBook)>,
    ) {
        let book = match cache.try_order_book(instrument_id) {
            Ok(book) => book,
            Err(e) => {
                log::error!("Cannot publish OrderBook snapshot: {e}");
                return;
            }
        };

        if book.update_count == 0 {
            log::debug!("OrderBook not yet updated for snapshot: {instrument_id}");
            return;
        }
        log::debug!(
            "Publishing OrderBook snapshot for {instrument_id} (update_count={})",
            book.update_count
        );

        books.push((topic, book.clone()));
    }
}

#[cfg(test)]
mod tests {
    use nautilus_common::msgbus::TypedHandler;
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        data::BookOrder,
        enums::{BookType, OrderSide},
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn snapshot_skips_missing_order_book() {
        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        let interval_ms = NonZeroUsize::new(100).unwrap();
        let topic = switchboard::get_book_snapshots_topic(instrument_id, interval_ms);
        let snapshot_infos = Rc::new(RefCell::new(IndexMap::new()));

        snapshot_infos.borrow_mut().insert(
            instrument_id,
            BookSnapshotInfo {
                instrument_id,
                venue: Venue::new("SIM"),
                parent: None,
                topic,
                interval_ms,
            },
        );

        let snapshotter = BookSnapshotter::new(
            interval_ms,
            snapshot_infos,
            Rc::new(RefCell::new(Cache::default())),
        );
        let event = TimeEvent::new(
            Ustr::from("TEST"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        snapshotter.snapshot(event);
    }

    #[rstest]
    fn snapshot_allows_subscriber_to_mutably_borrow_cache() {
        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        let interval_ms = NonZeroUsize::new(100).unwrap();
        let topic = switchboard::get_book_snapshots_topic(instrument_id, interval_ms);
        let snapshot_infos = Rc::new(RefCell::new(IndexMap::new()));

        snapshot_infos.borrow_mut().insert(
            instrument_id,
            BookSnapshotInfo {
                instrument_id,
                venue: Venue::new("SIM"),
                parent: None,
                topic,
                interval_ms,
            },
        );

        let cache = Rc::new(RefCell::new(Cache::default()));
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
        book.add(
            BookOrder::new(OrderSide::Buy, Price::from("100.00"), Quantity::from(10), 0),
            0,
            1,
            UnixNanos::default(),
        );
        cache.borrow_mut().add_order_book(book).unwrap();

        let received = Rc::new(RefCell::new(Vec::new()));
        let handler = CacheWritingBookHandler {
            id: Ustr::from("CacheWritingBookHandler"),
            cache: cache.clone(),
            received: received.clone(),
        };
        msgbus::subscribe_book_snapshots(topic.into(), TypedHandler::new(handler), None);

        let snapshotter = BookSnapshotter::new(interval_ms, snapshot_infos, cache);
        let event = TimeEvent::new(
            Ustr::from("TEST"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        snapshotter.snapshot(event);

        let received = received.borrow();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].instrument_id, instrument_id);
        assert_eq!(received[0].best_bid_price(), Some(Price::from("100.00")));
    }

    #[rstest]
    fn snapshot_skips_book_with_no_updates() {
        let instrument_id = InstrumentId::from("AUD/USD.SIM");
        let interval_ms = NonZeroUsize::new(100).unwrap();
        let topic = switchboard::get_book_snapshots_topic(instrument_id, interval_ms);
        let snapshot_infos = Rc::new(RefCell::new(IndexMap::new()));

        snapshot_infos.borrow_mut().insert(
            instrument_id,
            BookSnapshotInfo {
                instrument_id,
                venue: Venue::new("SIM"),
                parent: None,
                topic,
                interval_ms,
            },
        );

        let cache = Rc::new(RefCell::new(Cache::default()));
        cache
            .borrow_mut()
            .add_order_book(OrderBook::new(instrument_id, BookType::L2_MBP))
            .unwrap();

        let received = Rc::new(RefCell::new(Vec::new()));
        let handler = CacheWritingBookHandler {
            id: Ustr::from("CacheWritingBookHandler-NoUpdates"),
            cache: cache.clone(),
            received: received.clone(),
        };
        msgbus::subscribe_book_snapshots(topic.into(), TypedHandler::new(handler), None);

        let snapshotter = BookSnapshotter::new(interval_ms, snapshot_infos, cache);
        let event = TimeEvent::new(
            Ustr::from("TEST"),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        snapshotter.snapshot(event);

        assert!(received.borrow().is_empty());
    }

    struct CacheWritingBookHandler {
        id: Ustr,
        cache: Rc<RefCell<Cache>>,
        received: Rc<RefCell<Vec<OrderBook>>>,
    }

    impl Handler<OrderBook> for CacheWritingBookHandler {
        fn id(&self) -> Ustr {
            self.id
        }

        fn handle(&self, book: &OrderBook) {
            // Mirrors a strategy writing to the cache from `on_book`
            let mut cache = self.cache.borrow_mut();
            let _ = cache.order_book_mut(&book.instrument_id);
            self.received.borrow_mut().push(book.clone());
        }
    }
}
