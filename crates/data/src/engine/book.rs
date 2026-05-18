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

use std::{
    cell::{Ref, RefCell},
    num::NonZeroUsize,
    rc::Rc,
};

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

    pub fn snapshot(&self, _event: TimeEvent) {
        let snapshot_infos: Vec<BookSnapshotInfo> =
            self.snapshot_infos.borrow().values().cloned().collect();

        log::debug!(
            "BookSnapshotter.snapshot called for {} subscriptions at {}ms",
            snapshot_infos.len(),
            self.interval_ms,
        );

        let cache = self.cache.borrow();

        for snap_info in snapshot_infos {
            self.publish_snapshot(&snap_info, &cache);
        }
    }

    fn publish_snapshot(&self, snap_info: &BookSnapshotInfo, cache: &Ref<Cache>) {
        if let Some((root, class)) = snap_info.parent {
            let topic = snap_info.topic;
            for instrument in cache.instruments_by_parent(&snap_info.venue, &root, class) {
                self.publish_order_book(&instrument.id(), topic, cache);
            }
        } else {
            self.publish_order_book(&snap_info.instrument_id, snap_info.topic, cache);
        }
    }

    fn publish_order_book(
        &self,
        instrument_id: &InstrumentId,
        topic: MStr<Topic>,
        cache: &Ref<Cache>,
    ) {
        let book = cache
            .order_book(instrument_id)
            .unwrap_or_else(|| panic!("OrderBook for {instrument_id} was not in cache"));

        if book.update_count == 0 {
            log::debug!("OrderBook not yet updated for snapshot: {instrument_id}");
            return;
        }
        log::debug!(
            "Publishing OrderBook snapshot for {instrument_id} (update_count={})",
            book.update_count
        );

        msgbus::publish_book(topic, book);
    }
}
