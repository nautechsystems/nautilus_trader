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

//! Order book levels rebuilt from raw venue frames, independent of adapter parsing.

use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    sync::Arc,
};

use parking_lot::Mutex;
use rust_decimal::Decimal;

const VIEWS_MAX: usize = 2048;

/// Price levels a harness oracle rebuilds from raw venue frames.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct WireBook {
    /// Bid sizes by price.
    pub(crate) bids: BTreeMap<Decimal, Decimal>,
    /// Ask sizes by price.
    pub(crate) asks: BTreeMap<Decimal, Decimal>,
}

impl WireBook {
    /// Sets each `(price, size)` level, removing a level whose size is zero.
    pub(crate) fn apply(&mut self, bids: &[(Decimal, Decimal)], asks: &[(Decimal, Decimal)]) {
        for (levels, updates) in [(&mut self.bids, bids), (&mut self.asks, asks)] {
            for (price, size) in updates {
                if size.is_zero() {
                    levels.remove(price);
                } else {
                    levels.insert(*price, *size);
                }
            }
        }
    }

    /// Returns the best `depth` levels on each side.
    #[must_use]
    pub(crate) fn top(&self, depth: usize) -> Self {
        Self {
            bids: self
                .bids
                .iter()
                .rev()
                .take(depth)
                .map(|(price, size)| (*price, *size))
                .collect(),
            asks: self
                .asks
                .iter()
                .take(depth)
                .map(|(price, size)| (*price, *size))
                .collect(),
        }
    }
}

/// The best levels of a harness oracle's book after one raw venue frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WireView {
    /// Proxied connection that carried the frame, counting from one.
    pub(crate) epoch: usize,
    /// Venue sequence after the frame.
    pub(crate) sequence: u64,
    /// Venue event time in nanoseconds.
    pub(crate) timestamp: u64,
    /// Best levels after the frame.
    pub(crate) book: WireBook,
}

/// Recent oracle views by fault key, shared by every proxied connection.
#[derive(Debug, Clone, Default)]
pub(crate) struct WireViews {
    views: Arc<Mutex<HashMap<String, VecDeque<WireView>>>>,
}

impl WireViews {
    /// Records a view for `key`, evicting that key's oldest view beyond a fixed bound.
    pub(crate) fn record(&self, key: &str, view: WireView) {
        let mut views = self.views.lock();
        let views = views.entry(key.to_string()).or_default();
        views.push_back(view);

        if views.len() > VIEWS_MAX {
            views.pop_front();
        }
    }

    /// Returns the newest view for `key` at `sequence` and `timestamp`.
    #[must_use]
    pub(crate) fn find(&self, key: &str, sequence: u64, timestamp: u64) -> Option<WireView> {
        self.views
            .lock()
            .get(key)?
            .iter()
            .rev()
            .find(|view| view.sequence == sequence && view.timestamp == timestamp)
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    #[rstest]
    fn apply_sets_updates_and_removes_levels() {
        let mut book = WireBook::default();
        book.apply(
            &[(dec!(10), dec!(2)), (dec!(9), dec!(3))],
            &[(dec!(11), dec!(4)), (dec!(12), dec!(5))],
        );

        book.apply(
            &[(dec!(10), dec!(0)), (dec!(9), dec!(7))],
            &[(dec!(11), dec!(8)), (dec!(13), dec!(6))],
        );

        assert_eq!(
            book,
            WireBook {
                bids: BTreeMap::from([(dec!(9), dec!(7))]),
                asks: BTreeMap::from([
                    (dec!(11), dec!(8)),
                    (dec!(12), dec!(5)),
                    (dec!(13), dec!(6)),
                ]),
            }
        );
    }

    #[rstest]
    fn top_keeps_highest_bids_and_lowest_asks() {
        let book = WireBook {
            bids: (1..=4)
                .map(|n| (Decimal::from(n), Decimal::from(n + 40)))
                .collect(),
            asks: (5..=8)
                .map(|n| (Decimal::from(n), Decimal::from(n + 70)))
                .collect(),
        };

        let top = book.top(2);

        assert_eq!(
            top,
            WireBook {
                bids: BTreeMap::from([(dec!(3), dec!(43)), (dec!(4), dec!(44))]),
                asks: BTreeMap::from([(dec!(5), dec!(75)), (dec!(6), dec!(76))]),
            }
        );
    }

    fn view(epoch: usize, sequence: u64, timestamp: u64, bid: Decimal) -> WireView {
        WireView {
            epoch,
            sequence,
            timestamp,
            book: WireBook {
                bids: BTreeMap::from([(bid, dec!(1))]),
                asks: BTreeMap::new(),
            },
        }
    }

    #[rstest]
    fn find_returns_the_newest_view_at_sequence_and_timestamp() {
        let views = WireViews::default();
        views.record("A", view(1, 7, 50, dec!(10)));
        views.record("A", view(2, 7, 50, dec!(11)));
        views.record("A", view(2, 8, 60, dec!(12)));
        views.record("B", view(3, 7, 50, dec!(13)));

        assert_eq!(views.find("A", 7, 50), Some(view(2, 7, 50, dec!(11))));
        assert_eq!(views.find("A", 8, 60), Some(view(2, 8, 60, dec!(12))));
        assert_eq!(views.find("B", 7, 50), Some(view(3, 7, 50, dec!(13))));
        assert_eq!(views.find("A", 7, 60), None);
        assert_eq!(views.find("C", 7, 50), None);
    }

    #[rstest]
    fn record_keeps_the_latest_views_per_key() {
        let views = WireViews::default();

        for sequence in 0..=VIEWS_MAX as u64 {
            views.record("A", view(1, sequence, sequence, dec!(10)));
        }

        views.record("B", view(1, 0, 0, dec!(20)));

        assert_eq!(views.find("A", 0, 0), None);
        assert_eq!(views.find("A", 1, 1), Some(view(1, 1, 1, dec!(10))));
        assert_eq!(
            views.find("A", VIEWS_MAX as u64, VIEWS_MAX as u64),
            Some(view(1, VIEWS_MAX as u64, VIEWS_MAX as u64, dec!(10)))
        );
        assert_eq!(views.find("B", 0, 0), Some(view(1, 0, 0, dec!(20))));
    }
}
