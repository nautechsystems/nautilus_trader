// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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
    any::Any,
    cell::{Ref, RefCell},
    num::NonZeroUsize,
    rc::Rc,
};

use nautilus_common::{
    cache::Cache,
    msgbus::{self, MStr, Topic, handler::MessageHandler},
    timer::TimeEvent,
};
use nautilus_model::{
    data::Data,
    identifiers::{InstrumentId, Venue},
    instruments::Instrument,
};
use ustr::Ustr;

/// Contains information for creating snapshots of specific order books.
#[derive(Clone, Debug)]
pub struct BookSnapshotInfo {
    pub instrument_id: InstrumentId,
    pub venue: Venue,
    pub is_composite: bool,
    pub root: Ustr,
    pub topic: MStr<Topic>,
    pub interval_ms: NonZeroUsize,
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
}

impl BookUpdater {
    /// Creates a new [`BookUpdater`] instance.
    pub fn new(instrument_id: &InstrumentId, cache: Rc<RefCell<Cache>>) -> Self {
        Self {
            id: Ustr::from(&format!("{}-{}", stringify!(BookUpdater), instrument_id)),
            instrument_id: *instrument_id,
            cache,
        }
    }
}

impl MessageHandler for BookUpdater {
    fn id(&self) -> Ustr {
        self.id
    }

    fn handle(&self, message: &dyn Any) {
        // TODO: Temporary handler implementation (this will be removed soon)
        if let Some(data) = message.downcast_ref::<Data>()
            && let Some(book) = self
                .cache
                .borrow_mut()
                .order_book_mut(&data.instrument_id())
        {
            match data {
                Data::Delta(delta) => {
                    if let Err(e) = book.apply_delta(delta) {
                        log::error!("Failed to apply delta: {e}");
                    }
                }
                Data::Deltas(deltas) => {
                    if let Err(e) = book.apply_deltas(deltas) {
                        log::error!("Failed to apply deltas: {e}");
                    }
                }
                Data::Depth10(depth) => book.apply_depth(depth),
                _ => log::error!("Invalid data type for book update, was {data:?}"),
            }
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// Creates periodic snapshots of order books at configured intervals.
///
/// The `BookSnapshotter` generates order book snapshots on timer events,
/// publishing them as market data. This is useful for providing periodic
/// full order book state updates in addition to incremental delta updates.
#[derive(Debug)]
pub struct BookSnapshotter {
    pub id: Ustr,
    pub timer_name: Ustr,
    pub snap_info: BookSnapshotInfo,
    pub cache: Rc<RefCell<Cache>>,
}

impl BookSnapshotter {
    /// Creates a new [`BookSnapshotter`] instance.
    pub fn new(snap_info: BookSnapshotInfo, cache: Rc<RefCell<Cache>>) -> Self {
        let id_str = format!(
            "{}-{}",
            stringify!(BookSnapshotter),
            snap_info.instrument_id
        );
        let timer_name = format!(
            "OrderBook|{}|{}",
            snap_info.instrument_id, snap_info.interval_ms
        );

        Self {
            id: Ustr::from(&id_str),
            timer_name: Ustr::from(&timer_name),
            snap_info,
            cache,
        }
    }

    pub fn snapshot(&self, _event: TimeEvent) {
        let cache = self.cache.borrow();

        if self.snap_info.is_composite {
            let topic = self.snap_info.topic;
            let underlying = self.snap_info.root;
            for instrument in cache.instruments(&self.snap_info.venue, Some(&underlying)) {
                self.publish_order_book(&instrument.id(), topic, &cache);
            }
        } else {
            self.publish_order_book(&self.snap_info.instrument_id, self.snap_info.topic, &cache);
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
            log::debug!("OrderBook for {instrument_id} not yet updated for snapshot");
            return;
        }

        msgbus::publish(topic, book as &dyn Any);
    }
}
