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

use std::collections::BTreeMap;

use rust_decimal::Decimal;

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
}
