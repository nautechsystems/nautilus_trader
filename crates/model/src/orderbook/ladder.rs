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

//! Represents a ladder of price levels for one side of an order book.

use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashMap},
    fmt::{Debug, Display},
};

use nautilus_core::UnixNanos;

use crate::{
    data::order::{BookOrder, OrderId},
    enums::{BookType, OrderSideSpecified, RecordFlag},
    orderbook::BookLevel,
    types::{Price, Quantity},
};

/// Represents a price level with a specified side in an order books ladder.
///
/// # Comparison Semantics
///
/// `BookPrice` instances are only meaningfully compared within the same side
/// (i.e., within a single `BookLadder`). Cross-side comparisons are not expected
/// in normal use, as bid and ask ladders maintain separate `BTreeMap<BookPrice, BookLevel>`
/// collections.
///
/// - Equality requires both `value` and `side` to match.
/// - Ordering is side-dependent: Buy side sorts descending, Sell side ascending.
#[derive(Clone, Copy, Debug, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct BookPrice {
    pub value: Price,
    pub side: OrderSideSpecified,
}

impl BookPrice {
    /// Creates a new [`BookPrice`] instance.
    #[must_use]
    pub fn new(value: Price, side: OrderSideSpecified) -> Self {
        Self { value, side }
    }
}

impl PartialOrd for BookPrice {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for BookPrice {
    fn eq(&self, other: &Self) -> bool {
        self.side == other.side && self.value == other.value
    }
}

impl Ord for BookPrice {
    fn cmp(&self, other: &Self) -> Ordering {
        assert_eq!(
            self.side, other.side,
            "BookPrice compared across sides: {:?} vs {:?}",
            self.side, other.side
        );

        match self.side.cmp(&other.side) {
            Ordering::Equal => match self.side {
                OrderSideSpecified::Buy => other.value.cmp(&self.value),
                OrderSideSpecified::Sell => self.value.cmp(&other.value),
            },
            non_equal => non_equal,
        }
    }
}

impl Display for BookPrice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}

/// Tracks the type of L1 batch currently being accumulated.
///
/// Separating MBP and snapshot batches prevents cross-contamination where
/// stale MBP data could pollute a new snapshot. Without this distinction,
/// an incomplete MBP stream (missing F_LAST) would leave batch state that
/// incorrectly affects subsequent snapshot processing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum L1BatchState {
    /// Not in any batch.
    #[default]
    None,
    /// Accumulating an F_MBP batch (final two deltas accumulate).
    MbpBatch,
    /// Accumulating an F_SNAPSHOT batch (all deltas accumulate).
    SnapshotBatch,
}

/// Represents a ladder of price levels for one side of an order book.
#[derive(Clone, Debug)]
pub(crate) struct BookLadder {
    pub side: OrderSideSpecified,
    pub book_type: BookType,
    pub levels: BTreeMap<BookPrice, BookLevel>,
    pub cache: HashMap<u64, BookPrice>,
    batch_state: L1BatchState,
}

impl BookLadder {
    /// Creates a new [`Ladder`] instance.
    #[must_use]
    pub fn new(side: OrderSideSpecified, book_type: BookType) -> Self {
        Self {
            side,
            book_type,
            levels: BTreeMap::new(),
            cache: HashMap::new(),
            batch_state: L1BatchState::None,
        }
    }

    /// Returns the number of price levels in the ladder.
    #[must_use]
    pub fn len(&self) -> usize {
        self.levels.len()
    }

    /// Returns true if the ladder has no price levels.
    #[must_use]
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }

    /// Removes all orders and price levels from the ladder.
    ///
    /// Also resets the batch state to ensure clean handling of subsequent batches.
    pub fn clear(&mut self) {
        self.levels.clear();
        self.cache.clear();
        self.batch_state = L1BatchState::None;
    }

    /// Adds an order to the ladder at its price level.
    ///
    /// For L1_MBP books, behavior depends on flags:
    /// - F_MBP or F_SNAPSHOT (multi-level batch): Retains best after each add to prevent
    ///   accumulation even if F_LAST is never sent.
    /// - F_TOB or no batch flags (single replacement): Clears existing levels first,
    ///   allowing price to degrade.
    pub fn add(&mut self, order: BookOrder, flags: u8) {
        if self.book_type == BookType::L1_MBP && !self.handle_l1_add(&order, flags) {
            return;
        }

        if self.book_type != BookType::L1_MBP && !order.size.is_positive() {
            log::warn!(
                "Attempted to add order with non-positive size: order_id={}, size={}, ignoring",
                order.order_id,
                order.size,
            );
            return;
        }

        let book_price = order.to_book_price();
        self.cache.insert(order.order_id, book_price);

        match self.levels.get_mut(&book_price) {
            Some(level) => {
                level.add(order);
            }
            None => {
                let level = BookLevel::from_order(order);
                self.levels.insert(book_price, level);
            }
        }

        // For L1_MBP with F_MBP or F_SNAPSHOT, always retain best to prevent unbounded
        // accumulation if F_LAST is never sent
        let is_batch = RecordFlag::F_MBP.matches(flags) || RecordFlag::F_SNAPSHOT.matches(flags);
        if self.book_type == BookType::L1_MBP && is_batch {
            self.retain_best_only();
            if RecordFlag::F_LAST.matches(flags) {
                self.batch_state = L1BatchState::None;
            }
        }
    }

    /// Handles L1_MBP-specific add logic.
    ///
    /// Returns `true` to continue with normal add flow, `false` to abort.
    ///
    /// Behavior depends on flags:
    /// - F_SNAPSHOT with F_LAST: End of snapshot batch. If in snapshot batch, accumulate;
    ///   otherwise clear (single-delta snapshot or cross-contamination from MBP).
    /// - F_SNAPSHOT without F_LAST: Start/continue snapshot batch. Clears if not already
    ///   in a snapshot batch (handles stale MBP data).
    /// - F_MBP with F_LAST: End of MBP batch. If in MBP batch, accumulate final two;
    ///   otherwise clear.
    /// - F_MBP without F_LAST: Always clear (streaming mode, prevents stale prices).
    /// - F_TOB or no batch flags: Single replacement (clears first).
    ///
    /// Zero-size orders clear the entire L1 ladder.
    fn handle_l1_add(&mut self, order: &BookOrder, flags: u8) -> bool {
        if !order.size.is_positive() {
            self.clear();
            let side = self.side;
            log::debug!("L1 zero-size add cleared ladder: side={side:?}");
            return false;
        }

        let is_mbp = RecordFlag::F_MBP.matches(flags);
        let is_snapshot = RecordFlag::F_SNAPSHOT.matches(flags);
        let is_last = RecordFlag::F_LAST.matches(flags);

        if is_snapshot && is_last {
            // F_SNAPSHOT|F_LAST: end of snapshot batch
            // Only accumulate if we're in a snapshot batch; otherwise clear to prevent
            // cross-contamination from stale MBP data
            if self.batch_state != L1BatchState::SnapshotBatch {
                self.clear();
            }
        } else if is_snapshot {
            // F_SNAPSHOT without F_LAST: start/continue snapshot batch
            if self.batch_state != L1BatchState::SnapshotBatch {
                self.clear();
                self.batch_state = L1BatchState::SnapshotBatch;
            }
        } else if is_mbp && is_last {
            // F_MBP|F_LAST: end of MBP batch, accumulate if already in MBP batch
            if self.batch_state != L1BatchState::MbpBatch {
                self.clear();
            }
        } else if is_mbp {
            // F_MBP without F_LAST: always clear (streaming mode)
            self.clear();
            self.batch_state = L1BatchState::MbpBatch;
        } else {
            // Non-batch: replacement mode
            self.clear();
        }

        true
    }

    /// Updates an existing order in the ladder, moving it to a new price level if needed.
    pub fn update(&mut self, order: BookOrder, flags: u8) {
        let price = self.cache.get(&order.order_id).copied();
        if let Some(price) = price
            && let Some(level) = self.levels.get_mut(&price)
        {
            if order.price == level.price.value {
                let level_len_before = level.len();
                level.update(order);

                // If level.update removed the order due to zero size, remove from cache too
                if order.size.raw == 0 {
                    self.cache.remove(&order.order_id);
                    debug_assert_eq!(
                        level.len(),
                        level_len_before - 1,
                        "Level should have one less order after zero-size update"
                    );
                } else {
                    debug_assert!(
                        self.cache.contains_key(&order.order_id),
                        "Cache should still contain order {0} after update",
                        order.order_id
                    );
                }

                if level.is_empty() {
                    self.levels.remove(&price);
                    debug_assert!(
                        !self.cache.values().any(|p| *p == price),
                        "Cache should not contain removed price level {price:?}"
                    );
                }

                debug_assert_eq!(
                    self.cache.len(),
                    self.levels.values().map(|level| level.len()).sum::<usize>(),
                    "Cache size should equal total orders across all levels"
                );
                return;
            }

            // Price update: delete and insert at new level
            self.cache.remove(&order.order_id);
            level.delete(&order);

            if level.is_empty() {
                self.levels.remove(&price);
                debug_assert!(
                    !self.cache.values().any(|p| *p == price),
                    "Cache should not contain removed price level {price:?}"
                );
            }
        }

        // Only add if the order has positive size
        if order.size.is_positive() {
            self.add(order, flags);
        }

        // Validate cache consistency after update
        debug_assert_eq!(
            self.cache.len(),
            self.levels.values().map(|level| level.len()).sum::<usize>(),
            "Cache size should equal total orders across all levels"
        );
    }

    /// Deletes an order from the ladder.
    pub fn delete(&mut self, order: BookOrder, sequence: u64, ts_event: UnixNanos) {
        self.remove_order(order.order_id, sequence, ts_event);
    }

    /// Removes an order by its ID from the ladder.
    pub fn remove_order(&mut self, order_id: OrderId, sequence: u64, ts_event: UnixNanos) {
        if let Some(price) = self.cache.get(&order_id).copied()
            && let Some(level) = self.levels.get_mut(&price)
        {
            // Check if order exists in level before modifying cache
            if level.orders.contains_key(&order_id) {
                let level_len_before = level.len();

                // Now safe to remove from cache since we know order exists in level
                self.cache.remove(&order_id);
                level.remove_by_id(order_id, sequence, ts_event);

                debug_assert_eq!(
                    level.len(),
                    level_len_before - 1,
                    "Level should have exactly one less order after removal"
                );

                if level.is_empty() {
                    self.levels.remove(&price);
                    debug_assert!(
                        !self.cache.values().any(|p| *p == price),
                        "Cache should not contain removed price level {price:?}"
                    );
                }
            }
        }

        // Validate cache consistency after removal
        debug_assert_eq!(
            self.cache.len(),
            self.levels.values().map(|level| level.len()).sum::<usize>(),
            "Cache size should equal total orders across all levels"
        );
    }

    /// Removes an entire price level from the ladder and returns it.
    pub fn remove_level(&mut self, price: BookPrice) -> Option<BookLevel> {
        if let Some(level) = self.levels.remove(&price) {
            // Remove all orders in this level from the cache
            for order_id in level.orders.keys() {
                self.cache.remove(order_id);
            }

            debug_assert_eq!(
                self.cache.len(),
                self.levels.values().map(|level| level.len()).sum::<usize>(),
                "Cache size should equal total orders across all levels"
            );

            Some(level)
        } else {
            None
        }
    }

    /// Retains only the best price level, removing all others.
    ///
    /// For L1_MBP books, this ensures only the top-of-book level is kept after
    /// processing multi-level data. The BTreeMap ordering ensures the first
    /// entry is always the best price (highest for bids, lowest for asks).
    fn retain_best_only(&mut self) {
        if self.levels.len() <= 1 {
            return;
        }

        let best_price = match self.levels.keys().next().copied() {
            Some(price) => price,
            None => return,
        };

        // Remove all levels except the best (don't use remove_level as it
        // incorrectly handles cache for L1 where all orders share order_id)
        self.levels.retain(|price, _| *price == best_price);

        // Rebuild cache from remaining level (necessary for L1 where
        // all orders use the same order_id and remove_level would corrupt cache)
        self.cache.clear();
        for (book_price, level) in &self.levels {
            for order_id in level.orders.keys() {
                self.cache.insert(*order_id, *book_price);
            }
        }

        debug_assert!(
            self.levels.len() <= 1,
            "L1 ladder should have at most 1 level after retain_best_only"
        );
        debug_assert_eq!(
            self.cache.len(),
            self.levels.values().map(|l| l.len()).sum::<usize>(),
            "Cache size should equal total orders across all levels"
        );
    }

    /// Returns the total size of all orders in the ladder.
    #[must_use]
    #[allow(dead_code)]
    pub fn sizes(&self) -> f64 {
        self.levels.values().map(BookLevel::size).sum()
    }

    /// Returns the total value exposure (price * size) of all orders in the ladder.
    #[must_use]
    #[allow(dead_code)]
    pub fn exposures(&self) -> f64 {
        self.levels.values().map(BookLevel::exposure).sum()
    }

    /// Returns the best price level in the ladder.
    #[must_use]
    pub fn top(&self) -> Option<&BookLevel> {
        match self.levels.iter().next() {
            Some((_, l)) => Option::Some(l),
            None => Option::None,
        }
    }

    /// Simulates fills for an order against this ladder's liquidity.
    /// Returns a list of (price, size) tuples representing the simulated fills.
    #[must_use]
    pub fn simulate_fills(&self, order: &BookOrder) -> Vec<(Price, Quantity)> {
        let is_reversed = self.side == OrderSideSpecified::Buy;
        let mut fills = Vec::new();
        let mut cumulative_denominator = Quantity::zero(order.size.precision);
        let target = order.size;

        for level in self.levels.values() {
            if (is_reversed && level.price.value < order.price)
                || (!is_reversed && level.price.value > order.price)
            {
                break;
            }

            for book_order in level.orders.values() {
                let current = book_order.size;
                if cumulative_denominator + current >= target {
                    // This order has filled us, add fill and return
                    let remainder = target - cumulative_denominator;
                    if remainder.is_positive() {
                        fills.push((book_order.price, remainder));
                    }
                    return fills;
                }

                // Add this fill and continue
                fills.push((book_order.price, current));
                cumulative_denominator = cumulative_denominator + current;
            }
        }

        fills
    }
}

impl Display for BookLadder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}(side={})", stringify!(BookLadder), self.side)?;
        for (price, level) in &self.levels {
            writeln!(f, "  {} -> {} orders", price, level.len())?;
        }
        Ok(())
    }
}

#[cfg(test)]
impl BookLadder {
    /// Adds multiple orders to the ladder.
    pub fn add_bulk(&mut self, orders: &[BookOrder]) {
        for order in orders {
            self.add(*order, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::{
        data::order::BookOrder,
        enums::{BookType, OrderSide, OrderSideSpecified, RecordFlag},
        orderbook::ladder::{BookLadder, BookPrice},
        types::{Price, Quantity},
    };

    #[rstest]
    fn test_is_empty() {
        let ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L3_MBO);
        assert!(ladder.is_empty(), "A new ladder should be empty");
    }

    #[rstest]
    fn test_is_empty_after_add() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L3_MBO);
        assert!(ladder.is_empty(), "Ladder should start empty");
        let order = BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(100), 1);
        ladder.add(order, 0);
        assert!(
            !ladder.is_empty(),
            "Ladder should not be empty after adding an order"
        );
    }

    #[rstest]
    fn test_add_bulk_empty() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L3_MBO);
        ladder.add_bulk(&[]);
        assert!(
            ladder.is_empty(),
            "Adding an empty vector should leave the ladder empty"
        );
    }

    #[rstest]
    fn test_add_bulk_orders() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L3_MBO);
        let orders = [
            BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(20), 1),
            BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(30), 2),
            BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(50), 3),
        ];
        ladder.add_bulk(&orders);
        // All orders share the same price, so there should be one price level.
        assert_eq!(ladder.len(), 1, "Ladder should have one price level");
        let orders_in_level = ladder.top().unwrap().get_orders();
        assert_eq!(
            orders_in_level.len(),
            3,
            "Price level should contain all bulk orders"
        );
    }

    #[rstest]
    fn test_book_price_bid_sorting() {
        let mut bid_prices = [
            BookPrice::new(Price::from("2.0"), OrderSideSpecified::Buy),
            BookPrice::new(Price::from("4.0"), OrderSideSpecified::Buy),
            BookPrice::new(Price::from("1.0"), OrderSideSpecified::Buy),
            BookPrice::new(Price::from("3.0"), OrderSideSpecified::Buy),
        ];
        bid_prices.sort();
        assert_eq!(bid_prices[0].value, Price::from("4.0"));
    }

    #[rstest]
    fn test_book_price_ask_sorting() {
        let mut ask_prices = [
            BookPrice::new(Price::from("2.0"), OrderSideSpecified::Sell),
            BookPrice::new(Price::from("4.0"), OrderSideSpecified::Sell),
            BookPrice::new(Price::from("1.0"), OrderSideSpecified::Sell),
            BookPrice::new(Price::from("3.0"), OrderSideSpecified::Sell),
        ];

        ask_prices.sort();
        assert_eq!(ask_prices[0].value, Price::from("1.0"));
    }

    #[rstest]
    fn test_add_single_order() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L3_MBO);
        let order = BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(20), 0);

        ladder.add(order, 0);
        assert_eq!(ladder.len(), 1);
        assert_eq!(ladder.sizes(), 20.0);
        assert_eq!(ladder.exposures(), 200.0);
        assert_eq!(ladder.top().unwrap().price.value, Price::from("10.0"));
    }

    #[rstest]
    fn test_add_multiple_buy_orders() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L3_MBO);
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(20), 0);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("9.00"), Quantity::from(30), 1);
        let order3 = BookOrder::new(OrderSide::Buy, Price::from("9.00"), Quantity::from(50), 2);
        let order4 = BookOrder::new(OrderSide::Buy, Price::from("8.00"), Quantity::from(200), 3);

        ladder.add_bulk(&[order1, order2, order3, order4]);
        assert_eq!(ladder.len(), 3);
        assert_eq!(ladder.sizes(), 300.0);
        assert_eq!(ladder.exposures(), 2520.0);
        assert_eq!(ladder.top().unwrap().price.value, Price::from("10.0"));
    }

    #[rstest]
    fn test_add_multiple_sell_orders() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Sell, BookType::L3_MBO);
        let order1 = BookOrder::new(OrderSide::Sell, Price::from("11.00"), Quantity::from(20), 0);
        let order2 = BookOrder::new(OrderSide::Sell, Price::from("12.00"), Quantity::from(30), 1);
        let order3 = BookOrder::new(OrderSide::Sell, Price::from("12.00"), Quantity::from(50), 2);
        let order4 = BookOrder::new(
            OrderSide::Sell,
            Price::from("13.00"),
            Quantity::from(200),
            0,
        );

        ladder.add_bulk(&[order1, order2, order3, order4]);
        assert_eq!(ladder.len(), 3);
        assert_eq!(ladder.sizes(), 300.0);
        assert_eq!(ladder.exposures(), 3780.0);
        assert_eq!(ladder.top().unwrap().price.value, Price::from("11.0"));
    }

    #[rstest]
    fn test_add_to_same_price_level() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L3_MBO);
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(20), 1);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(30), 2);

        ladder.add(order1, 0);
        ladder.add(order2, 0);

        assert_eq!(ladder.len(), 1);
        assert_eq!(ladder.sizes(), 50.0);
        assert_eq!(ladder.exposures(), 500.0);
    }

    #[rstest]
    fn test_add_descending_buy_orders() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L3_MBO);
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("9.00"), Quantity::from(20), 1);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("8.00"), Quantity::from(30), 2);

        ladder.add(order1, 0);
        ladder.add(order2, 0);

        assert_eq!(ladder.top().unwrap().price.value, Price::from("9.00"));
    }

    #[rstest]
    fn test_add_ascending_sell_orders() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Sell, BookType::L3_MBO);
        let order1 = BookOrder::new(OrderSide::Sell, Price::from("8.00"), Quantity::from(20), 1);
        let order2 = BookOrder::new(OrderSide::Sell, Price::from("9.00"), Quantity::from(30), 2);

        ladder.add(order1, 0);
        ladder.add(order2, 0);

        assert_eq!(ladder.top().unwrap().price.value, Price::from("8.00"));
    }

    #[rstest]
    fn test_update_buy_order_price() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L3_MBO);
        let order = BookOrder::new(OrderSide::Buy, Price::from("11.00"), Quantity::from(20), 1);

        ladder.add(order, 0);
        let order = BookOrder::new(OrderSide::Buy, Price::from("11.10"), Quantity::from(20), 1);

        ladder.update(order, 0);
        assert_eq!(ladder.len(), 1);
        assert_eq!(ladder.sizes(), 20.0);
        assert_eq!(ladder.exposures(), 222.0);
        assert_eq!(ladder.top().unwrap().price.value, Price::from("11.1"));
    }

    #[rstest]
    fn test_update_sell_order_price() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Sell, BookType::L3_MBO);
        let order = BookOrder::new(OrderSide::Sell, Price::from("11.00"), Quantity::from(20), 1);

        ladder.add(order, 0);

        let order = BookOrder::new(OrderSide::Sell, Price::from("11.10"), Quantity::from(20), 1);

        ladder.update(order, 0);
        assert_eq!(ladder.len(), 1);
        assert_eq!(ladder.sizes(), 20.0);
        assert_eq!(ladder.exposures(), 222.0);
        assert_eq!(ladder.top().unwrap().price.value, Price::from("11.1"));
    }

    #[rstest]
    fn test_update_buy_order_size() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L3_MBO);
        let order = BookOrder::new(OrderSide::Buy, Price::from("11.00"), Quantity::from(20), 1);

        ladder.add(order, 0);

        let order = BookOrder::new(OrderSide::Buy, Price::from("11.00"), Quantity::from(10), 1);

        ladder.update(order, 0);
        assert_eq!(ladder.len(), 1);
        assert_eq!(ladder.sizes(), 10.0);
        assert_eq!(ladder.exposures(), 110.0);
        assert_eq!(ladder.top().unwrap().price.value, Price::from("11.0"));
    }

    #[rstest]
    fn test_update_sell_order_size() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Sell, BookType::L3_MBO);
        let order = BookOrder::new(OrderSide::Sell, Price::from("11.00"), Quantity::from(20), 1);

        ladder.add(order, 0);

        let order = BookOrder::new(OrderSide::Sell, Price::from("11.00"), Quantity::from(10), 1);

        ladder.update(order, 0);
        assert_eq!(ladder.len(), 1);
        assert_eq!(ladder.sizes(), 10.0);
        assert_eq!(ladder.exposures(), 110.0);
        assert_eq!(ladder.top().unwrap().price.value, Price::from("11.0"));
    }

    #[rstest]
    fn test_delete_non_existing_order() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L3_MBO);
        let order = BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(20), 1);

        ladder.delete(order, 0, 0.into());

        assert_eq!(ladder.len(), 0);
    }

    #[rstest]
    fn test_delete_buy_order() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L3_MBO);
        let order = BookOrder::new(OrderSide::Buy, Price::from("11.00"), Quantity::from(20), 1);

        ladder.add(order, 0);

        let order = BookOrder::new(OrderSide::Buy, Price::from("11.00"), Quantity::from(10), 1);

        ladder.delete(order, 0, 0.into());
        assert_eq!(ladder.len(), 0);
        assert_eq!(ladder.sizes(), 0.0);
        assert_eq!(ladder.exposures(), 0.0);
        assert_eq!(ladder.top(), None);
    }

    #[rstest]
    fn test_delete_sell_order() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Sell, BookType::L3_MBO);
        let order = BookOrder::new(OrderSide::Sell, Price::from("10.00"), Quantity::from(10), 1);

        ladder.add(order, 0);

        let order = BookOrder::new(OrderSide::Sell, Price::from("10.00"), Quantity::from(10), 1);

        ladder.delete(order, 0, 0.into());
        assert_eq!(ladder.len(), 0);
        assert_eq!(ladder.sizes(), 0.0);
        assert_eq!(ladder.exposures(), 0.0);
        assert_eq!(ladder.top(), None);
    }

    #[rstest]
    fn test_ladder_sizes_empty() {
        let ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L3_MBO);
        assert_eq!(
            ladder.sizes(),
            0.0,
            "An empty ladder should have total size 0.0"
        );
    }

    #[rstest]
    fn test_ladder_exposures_empty() {
        let ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L3_MBO);
        assert_eq!(
            ladder.exposures(),
            0.0,
            "An empty ladder should have total exposure 0.0"
        );
    }

    #[rstest]
    fn test_ladder_sizes() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L3_MBO);
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(20), 1);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("9.50"), Quantity::from(30), 2);
        ladder.add(order1, 0);
        ladder.add(order2, 0);

        let expected_size = 20.0 + 30.0;
        assert_eq!(
            ladder.sizes(),
            expected_size,
            "Ladder total size should match the sum of order sizes"
        );
    }

    #[rstest]
    fn test_ladder_exposures() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L3_MBO);
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(20), 1);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("9.50"), Quantity::from(30), 2);
        ladder.add(order1, 0);
        ladder.add(order2, 0);

        let expected_exposure = 10.00 * 20.0 + 9.50 * 30.0;
        assert_eq!(
            ladder.exposures(),
            expected_exposure,
            "Ladder total exposure should match the sum of individual exposures"
        );
    }

    #[rstest]
    fn test_iter_returns_fifo() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L3_MBO);
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(20), 1);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(30), 2);
        ladder.add(order1, 0);
        ladder.add(order2, 0);
        let orders: Vec<BookOrder> = ladder.top().unwrap().iter().copied().collect();
        assert_eq!(
            orders,
            vec![order1, order2],
            "Iterator should return orders in FIFO order"
        );
    }

    #[rstest]
    fn test_update_missing_order_inserts() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L3_MBO);
        let order = BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(20), 1);
        // Call update on an order that hasn't been added yet (upsert behavior)
        ladder.update(order, 0);
        assert_eq!(
            ladder.len(),
            1,
            "Ladder should have one level after upsert update"
        );
        let orders = ladder.top().unwrap().get_orders();
        assert_eq!(
            orders.len(),
            1,
            "Price level should contain the inserted order"
        );
        assert_eq!(orders[0], order, "The inserted order should match");
    }

    #[rstest]
    fn test_cache_consistency_after_operations() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L3_MBO);
        let order1 = BookOrder::new(OrderSide::Buy, Price::from("10.00"), Quantity::from(20), 1);
        let order2 = BookOrder::new(OrderSide::Buy, Price::from("9.00"), Quantity::from(30), 2);
        ladder.add(order1, 0);
        ladder.add(order2, 0);

        // Ensure that each order in the cache is present in the corresponding price level.
        for (order_id, price) in &ladder.cache {
            let level = ladder
                .levels
                .get(price)
                .expect("Every price in the cache should have a corresponding level");
            assert!(
                level.orders.contains_key(order_id),
                "Order id {order_id} should be present in the level for price {price}",
            );
        }
    }

    #[rstest]
    fn test_simulate_fills_with_empty_book() {
        let ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L3_MBO);
        let order = BookOrder::new(OrderSide::Buy, Price::max(2), Quantity::from(500), 1);

        let fills = ladder.simulate_fills(&order);

        assert!(fills.is_empty());
    }

    #[rstest]
    #[case(OrderSide::Buy, Price::max(2), OrderSideSpecified::Sell)]
    #[case(OrderSide::Sell, Price::min(2), OrderSideSpecified::Buy)]
    fn test_simulate_order_fills_with_no_size(
        #[case] side: OrderSide,
        #[case] price: Price,
        #[case] ladder_side: OrderSideSpecified,
    ) {
        let ladder = BookLadder::new(ladder_side, BookType::L3_MBO);
        let order = BookOrder {
            price, // <-- Simulate a MARKET order
            size: Quantity::from(500),
            side,
            order_id: 2,
        };

        let fills = ladder.simulate_fills(&order);

        assert!(fills.is_empty());
    }

    #[rstest]
    #[case(OrderSide::Buy, OrderSideSpecified::Sell, Price::from("60.0"))]
    #[case(OrderSide::Sell, OrderSideSpecified::Buy, Price::from("40.0"))]
    fn test_simulate_order_fills_buy_when_far_from_market(
        #[case] order_side: OrderSide,
        #[case] ladder_side: OrderSideSpecified,
        #[case] ladder_price: Price,
    ) {
        let mut ladder = BookLadder::new(ladder_side, BookType::L3_MBO);

        ladder.add(
            BookOrder {
                price: ladder_price,
                size: Quantity::from(100),
                side: ladder_side.as_order_side(),
                order_id: 1,
            },
            0,
        );

        let order = BookOrder {
            price: Price::from("50.00"),
            size: Quantity::from(500),
            side: order_side,
            order_id: 2,
        };

        let fills = ladder.simulate_fills(&order);

        assert!(fills.is_empty());
    }

    #[rstest]
    fn test_simulate_order_fills_sell_when_far_from_market() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L3_MBO);

        ladder.add(
            BookOrder {
                price: Price::from("100.00"),
                size: Quantity::from(100),
                side: OrderSide::Buy,
                order_id: 1,
            },
            0,
        );

        let order = BookOrder {
            price: Price::from("150.00"), // <-- Simulate a MARKET order
            size: Quantity::from(500),
            side: OrderSide::Buy,
            order_id: 2,
        };

        let fills = ladder.simulate_fills(&order);

        assert!(fills.is_empty());
    }

    #[rstest]
    fn test_simulate_order_fills_buy() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Sell, BookType::L3_MBO);

        ladder.add_bulk(&[
            BookOrder {
                price: Price::from("100.00"),
                size: Quantity::from(100),
                side: OrderSide::Sell,
                order_id: 1,
            },
            BookOrder {
                price: Price::from("101.00"),
                size: Quantity::from(200),
                side: OrderSide::Sell,
                order_id: 2,
            },
            BookOrder {
                price: Price::from("102.00"),
                size: Quantity::from(400),
                side: OrderSide::Sell,
                order_id: 3,
            },
        ]);

        let order = BookOrder {
            price: Price::max(2), // <-- Simulate a MARKET order
            size: Quantity::from(500),
            side: OrderSide::Buy,
            order_id: 4,
        };

        let fills = ladder.simulate_fills(&order);

        assert_eq!(fills.len(), 3);

        let (price1, size1) = fills[0];
        assert_eq!(price1, Price::from("100.00"));
        assert_eq!(size1, Quantity::from(100));

        let (price2, size2) = fills[1];
        assert_eq!(price2, Price::from("101.00"));
        assert_eq!(size2, Quantity::from(200));

        let (price3, size3) = fills[2];
        assert_eq!(price3, Price::from("102.00"));
        assert_eq!(size3, Quantity::from(200));
    }

    #[rstest]
    fn test_simulate_order_fills_sell() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L3_MBO);

        ladder.add_bulk(&[
            BookOrder {
                price: Price::from("102.00"),
                size: Quantity::from(100),
                side: OrderSide::Buy,
                order_id: 1,
            },
            BookOrder {
                price: Price::from("101.00"),
                size: Quantity::from(200),
                side: OrderSide::Buy,
                order_id: 2,
            },
            BookOrder {
                price: Price::from("100.00"),
                size: Quantity::from(400),
                side: OrderSide::Buy,
                order_id: 3,
            },
        ]);

        let order = BookOrder {
            price: Price::min(2), // <-- Simulate a MARKET order
            size: Quantity::from(500),
            side: OrderSide::Sell,
            order_id: 4,
        };

        let fills = ladder.simulate_fills(&order);

        assert_eq!(fills.len(), 3);

        let (price1, size1) = fills[0];
        assert_eq!(price1, Price::from("102.00"));
        assert_eq!(size1, Quantity::from(100));

        let (price2, size2) = fills[1];
        assert_eq!(price2, Price::from("101.00"));
        assert_eq!(size2, Quantity::from(200));

        let (price3, size3) = fills[2];
        assert_eq!(price3, Price::from("100.00"));
        assert_eq!(size3, Quantity::from(200));
    }

    #[rstest]
    fn test_simulate_order_fills_sell_with_size_at_limit_of_precision() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L3_MBO);

        ladder.add_bulk(&[
            BookOrder {
                price: Price::from("102.00"),
                size: Quantity::from("100.000000000"),
                side: OrderSide::Buy,
                order_id: 1,
            },
            BookOrder {
                price: Price::from("101.00"),
                size: Quantity::from("200.000000000"),
                side: OrderSide::Buy,
                order_id: 2,
            },
            BookOrder {
                price: Price::from("100.00"),
                size: Quantity::from("400.000000000"),
                side: OrderSide::Buy,
                order_id: 3,
            },
        ]);

        let order = BookOrder {
            price: Price::min(2),                  // <-- Simulate a MARKET order
            size: Quantity::from("699.999999999"), // <-- Size slightly less than total size in ladder
            side: OrderSide::Sell,
            order_id: 4,
        };

        let fills = ladder.simulate_fills(&order);

        assert_eq!(fills.len(), 3);

        let (price1, size1) = fills[0];
        assert_eq!(price1, Price::from("102.00"));
        assert_eq!(size1, Quantity::from("100.000000000"));

        let (price2, size2) = fills[1];
        assert_eq!(price2, Price::from("101.00"));
        assert_eq!(size2, Quantity::from("200.000000000"));

        let (price3, size3) = fills[2];
        assert_eq!(price3, Price::from("100.00"));
        assert_eq!(size3, Quantity::from("399.999999999"));
    }

    #[rstest]
    fn test_boundary_prices() {
        let max_price = Price::max(1);
        let min_price = Price::min(1);

        let mut ladder_buy = BookLadder::new(OrderSideSpecified::Buy, BookType::L3_MBO);
        let mut ladder_sell = BookLadder::new(OrderSideSpecified::Sell, BookType::L3_MBO);

        let order_buy = BookOrder::new(OrderSide::Buy, min_price, Quantity::from(1), 1);
        let order_sell = BookOrder::new(OrderSide::Sell, max_price, Quantity::from(1), 1);

        ladder_buy.add(order_buy, 0);
        ladder_sell.add(order_sell, 0);

        assert_eq!(ladder_buy.top().unwrap().price.value, min_price);
        assert_eq!(ladder_sell.top().unwrap().price.value, max_price);
    }

    #[rstest]
    fn test_l1_single_delta_batches_replace_each_other() {
        // Test that single-delta batches (each add has F_LAST) replace each other.
        // Each batch represents the current top-of-book, not a running best.
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L1_MBP);
        let side_constant = OrderSide::Buy as u64;

        // Using F_MBP | F_LAST simulates receiving single-delta batches
        let batch_flags = RecordFlag::F_MBP as u8 | RecordFlag::F_LAST as u8;

        // Add first L1 order at price 100.00
        let order1 = BookOrder {
            side: OrderSide::Buy,
            price: Price::from("100.00"),
            size: Quantity::from(50),
            order_id: side_constant,
        };
        ladder.add(order1, batch_flags);

        assert_eq!(ladder.len(), 1, "Should have one level after first add");
        assert_eq!(
            ladder.top().unwrap().price.value,
            Price::from("100.00"),
            "Top level should be at 100.00"
        );

        let order2 = BookOrder {
            side: OrderSide::Buy,
            price: Price::from("101.00"),
            size: Quantity::from(60),
            order_id: side_constant,
        };
        ladder.add(order2, batch_flags);

        assert_eq!(ladder.len(), 1, "Should have only one level");
        assert_eq!(
            ladder.top().unwrap().price.value,
            Price::from("101.00"),
            "Top level should be at 101.00"
        );

        // Price CAN degrade between batches
        let order3 = BookOrder {
            side: OrderSide::Buy,
            price: Price::from("100.50"),
            size: Quantity::from(70),
            order_id: side_constant,
        };
        ladder.add(order3, batch_flags);

        assert_eq!(ladder.len(), 1, "Should have only one level");
        assert_eq!(
            ladder.top().unwrap().price.value,
            Price::from("100.50"),
            "Top level should be at 100.50 (new batch replaced old)"
        );
    }

    #[rstest]
    fn test_l2_orders_not_affected_by_l1_fix() {
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L3_MBO);

        let order1 = BookOrder {
            side: OrderSide::Buy,
            price: Price::from("100.00"),
            size: Quantity::from(50),
            order_id: Price::from("100.00").raw as u64,
        };
        ladder.add(order1, 0);

        let order2 = BookOrder {
            side: OrderSide::Buy,
            price: Price::from("99.00"),
            size: Quantity::from(60),
            order_id: Price::from("99.00").raw as u64,
        };
        ladder.add(order2, 0);

        assert_eq!(ladder.len(), 2, "L2 orders should create multiple levels");
        assert_eq!(
            ladder.top().unwrap().price.value,
            Price::from("100.00"),
            "Top level should be best bid"
        );
    }

    #[rstest]
    fn test_zero_size_l1_order_clears_top() {
        // Venues send Add with size=0 to clear top-of-book
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L1_MBP);
        let side_constant = OrderSide::Buy as u64;

        let order1 = BookOrder {
            side: OrderSide::Buy,
            price: Price::from("100.00"),
            size: Quantity::from(50),
            order_id: side_constant,
        };
        ladder.add(order1, 0);

        assert_eq!(ladder.len(), 1);
        assert_eq!(ladder.top().unwrap().price.value, Price::from("100.00"));
        assert!(ladder.top().unwrap().first().is_some());

        // Try to add zero-size L1 order (venue clearing the book)
        let order2 = BookOrder {
            side: OrderSide::Buy,
            price: Price::from("101.00"),
            size: Quantity::zero(9), // Zero size
            order_id: side_constant,
        };
        ladder.add(order2, 0);

        // L1 zero-size should clear the top of book
        assert_eq!(ladder.len(), 0, "Zero-size L1 add should clear the book");
        assert!(ladder.top().is_none(), "Book should be empty after clear");

        // Cache should be empty
        assert!(
            ladder.cache.is_empty(),
            "Cache should be empty after L1 clear"
        );
    }

    #[rstest]
    fn test_zero_size_order_to_empty_ladder() {
        // Edge case: Adding zero-size L1 order to empty ladder should remain empty
        let mut ladder = BookLadder::new(OrderSideSpecified::Sell, BookType::L1_MBP);
        let side_constant = OrderSide::Sell as u64;

        let order = BookOrder {
            side: OrderSide::Sell,
            price: Price::from("100.00"),
            size: Quantity::zero(9),
            order_id: side_constant,
        };
        ladder.add(order, 0);

        assert_eq!(ladder.len(), 0, "Empty ladder should remain empty");
        assert!(ladder.top().is_none(), "Top should be None");
        assert!(
            ladder.cache.is_empty(),
            "Cache should remain empty for zero-size add"
        );
    }

    #[rstest]
    fn test_l3_order_id_collision_no_ghost_levels() {
        // Regression test: L3 venue order IDs 1 and 2 should not trigger L1 ghost level removal
        // Real L3 feeds routinely use order IDs 1 or 2, which match the side constants
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L3_MBO);

        // Add order with ID 1 at 100.00 (matches Buy side constant)
        let order1 = BookOrder {
            side: OrderSide::Buy,
            price: Price::from("100.00"),
            size: Quantity::from(50),
            order_id: 1, // Matches OrderSide::Buy as u64
        };
        ladder.add(order1, 0);

        assert_eq!(ladder.len(), 1);

        // Add another order with ID 1 at a different price 99.00
        // For L3, this is a DIFFERENT order (different price), should create second level
        let order2 = BookOrder {
            side: OrderSide::Buy,
            price: Price::from("99.00"),
            size: Quantity::from(60),
            order_id: 1, // Same ID, different price - valid in L3
        };
        ladder.add(order2, 0);

        // Should have both levels - L3 allows duplicate order IDs at different prices
        assert_eq!(
            ladder.len(),
            2,
            "L3 should allow order ID 1 at multiple price levels"
        );

        let prices: Vec<Price> = ladder.levels.keys().map(|bp| bp.value).collect();
        assert!(
            prices.contains(&Price::from("100.00")),
            "Level at 100.00 should still exist"
        );
        assert!(
            prices.contains(&Price::from("99.00")),
            "Level at 99.00 should exist"
        );
    }

    #[rstest]
    fn test_l1_vs_l3_different_behavior_same_order_id() {
        // Demonstrates the difference between L1 and L3 behavior for same order ID

        // L1 behavior with replacement (flags=0): successive adds replace
        let mut l1_ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L1_MBP);
        let side_constant = OrderSide::Buy as u64;

        let order1 = BookOrder {
            side: OrderSide::Buy,
            price: Price::from("100.00"),
            size: Quantity::from(50),
            order_id: side_constant,
        };
        l1_ladder.add(order1, 0);

        let order2 = BookOrder {
            side: OrderSide::Buy,
            price: Price::from("101.00"),
            size: Quantity::from(60),
            order_id: side_constant, // Same ID
        };
        l1_ladder.add(order2, 0);

        assert_eq!(l1_ladder.len(), 1, "L1 should have only 1 level");
        assert_eq!(
            l1_ladder.top().unwrap().price.value,
            Price::from("101.00"),
            "L1 should have replaced the old level"
        );

        // L3 behavior: order ID can be reused at different prices (different orders)
        let mut l3_ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L3_MBO);

        let order3 = BookOrder {
            side: OrderSide::Buy,
            price: Price::from("100.00"),
            size: Quantity::from(50),
            order_id: 1, // Happens to match side constant
        };
        l3_ladder.add(order3, 0);

        let order4 = BookOrder {
            side: OrderSide::Buy,
            price: Price::from("101.00"),
            size: Quantity::from(60),
            order_id: 1, // Same ID but different order
        };
        l3_ladder.add(order4, 0);

        assert_eq!(l3_ladder.len(), 2, "L3 should have 2 levels");
    }

    #[rstest]
    #[case::bids_worst_to_best(OrderSideSpecified::Buy, OrderSide::Buy, &["99.00", "100.00", "101.00", "102.00"], "102.00")]
    #[case::bids_best_to_worst(OrderSideSpecified::Buy, OrderSide::Buy, &["102.00", "101.00", "100.00", "99.00"], "100.00")]
    #[case::asks_worst_to_best(OrderSideSpecified::Sell, OrderSide::Sell, &["105.00", "104.00", "103.00", "102.00"], "102.00")]
    #[case::asks_best_to_worst(OrderSideSpecified::Sell, OrderSide::Sell, &["102.00", "103.00", "104.00", "105.00"], "104.00")]
    fn test_l1_multi_delta_batch_keeps_best_of_final_two(
        #[case] side_spec: OrderSideSpecified,
        #[case] side: OrderSide,
        #[case] prices: &[&str],
        #[case] expected_best: &str,
    ) {
        // Multi-delta batch: F_MBP without F_LAST clears each time.
        // Only the delta before F_LAST + F_LAST delta accumulate.
        let mut ladder = BookLadder::new(side_spec, BookType::L1_MBP);

        let batch_size = prices.len();
        for (i, price_str) in prices.iter().enumerate() {
            let order = BookOrder {
                side,
                price: Price::from(*price_str),
                size: Quantity::from((i + 1) as u64 * 10),
                order_id: (i + 100) as u64,
            };
            let flags = if i == batch_size - 1 {
                RecordFlag::F_MBP as u8 | RecordFlag::F_LAST as u8
            } else {
                RecordFlag::F_MBP as u8
            };
            ladder.add(order, flags);
        }

        assert_eq!(ladder.len(), 1, "L1 should have only 1 level");
        assert_eq!(
            ladder.top().unwrap().price.value,
            Price::from(expected_best),
            "Should keep best of final two deltas"
        );
    }

    #[rstest]
    fn test_l1_retain_best_only_cache_consistency() {
        // Verify cache is properly cleaned up when retaining only the best level
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L1_MBP);
        let batch_flags = RecordFlag::F_MBP as u8 | RecordFlag::F_LAST as u8;
        let prices = ["100.00", "101.00", "102.00", "103.00", "104.00"];

        for (i, price_str) in prices.iter().enumerate() {
            let order = BookOrder {
                side: OrderSide::Buy,
                price: Price::from(*price_str),
                size: Quantity::from(10),
                order_id: (i + 1) as u64,
            };
            ladder.add(order, batch_flags);
        }

        assert_eq!(ladder.len(), 1);
        assert_eq!(
            ladder.cache.len(),
            1,
            "Cache should have exactly 1 entry for L1"
        );

        let total_orders: usize = ladder.levels.values().map(|l| l.len()).sum();
        assert_eq!(
            ladder.cache.len(),
            total_orders,
            "Cache should be consistent with levels"
        );
    }

    #[rstest]
    fn test_l1_sequential_replacement_allows_price_degradation() {
        // Test that sequential L1 replacements (without F_MBP) allow price degradation
        // This is the expected behavior for top-of-book feeds like F_TOB
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L1_MBP);
        let side_constant = OrderSide::Buy as u64;

        // Add first L1 order at price 101.00 (best bid)
        let order1 = BookOrder {
            side: OrderSide::Buy,
            price: Price::from("101.00"),
            size: Quantity::from(50),
            order_id: side_constant,
        };
        ladder.add(order1, 0); // flags=0 means replacement mode

        assert_eq!(ladder.len(), 1);
        assert_eq!(
            ladder.top().unwrap().price.value,
            Price::from("101.00"),
            "Should have bid at 101.00"
        );

        // Add second L1 order at worse price 100.00 (replacement mode)
        // This should REPLACE the previous level, allowing price degradation
        let order2 = BookOrder {
            side: OrderSide::Buy,
            price: Price::from("100.00"),
            size: Quantity::from(60),
            order_id: side_constant,
        };
        ladder.add(order2, 0); // flags=0 means replacement mode

        assert_eq!(ladder.len(), 1);
        assert_eq!(
            ladder.top().unwrap().price.value,
            Price::from("100.00"),
            "Sequential replacement should allow price to degrade from 101 to 100"
        );

        // Verify the size was updated too
        assert_eq!(
            ladder.top().unwrap().first().unwrap().size,
            Quantity::from(60),
            "Size should be from the new order"
        );
    }

    #[rstest]
    #[case::bids(OrderSideSpecified::Buy, OrderSide::Buy, &["100.00", "101.00", "102.00"], "102.00", &["97.00", "98.00", "99.00"], "99.00")]
    #[case::asks(OrderSideSpecified::Sell, OrderSide::Sell, &["100.00", "101.00", "102.00"], "101.00", &["103.00", "104.00", "105.00"], "104.00")]
    fn test_l1_consecutive_batches_clear_between(
        #[case] side_spec: OrderSideSpecified,
        #[case] side: OrderSide,
        #[case] batch1_prices: &[&str],
        #[case] expected1: &str,
        #[case] batch2_prices: &[&str],
        #[case] expected2: &str,
    ) {
        // Consecutive batches clear old data when a new batch starts
        let mut ladder = BookLadder::new(side_spec, BookType::L1_MBP);

        // Batch 1
        for (i, price_str) in batch1_prices.iter().enumerate() {
            let order = BookOrder {
                side,
                price: Price::from(*price_str),
                size: Quantity::from(10),
                order_id: (i + 100) as u64,
            };
            let flags = if i == batch1_prices.len() - 1 {
                RecordFlag::F_MBP as u8 | RecordFlag::F_LAST as u8
            } else {
                RecordFlag::F_MBP as u8
            };
            ladder.add(order, flags);
        }

        assert_eq!(ladder.len(), 1);
        assert_eq!(
            ladder.top().unwrap().price.value,
            Price::from(expected1),
            "After batch 1"
        );

        // Batch 2 (worse prices for bids, higher prices for asks)
        for (i, price_str) in batch2_prices.iter().enumerate() {
            let order = BookOrder {
                side,
                price: Price::from(*price_str),
                size: Quantity::from(20),
                order_id: (i + 200) as u64,
            };
            let flags = if i == batch2_prices.len() - 1 {
                RecordFlag::F_MBP as u8 | RecordFlag::F_LAST as u8
            } else {
                RecordFlag::F_MBP as u8
            };
            ladder.add(order, flags);
        }

        assert_eq!(ladder.len(), 1);
        assert_eq!(
            ladder.top().unwrap().price.value,
            Price::from(expected2),
            "After batch 2: batch 1 data cleared"
        );
    }

    #[rstest]
    fn test_l1_zero_size_clears_regardless_of_order_id() {
        // Regression test: Zero-size clears must work even when order_id
        // differs between F_MBP batch (price-hash ID) and clear (side-constant ID)
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L1_MBP);

        // Add order with F_MBP flags (uses price-hash order_id via pre_process_order)
        let batch_flags = RecordFlag::F_MBP as u8 | RecordFlag::F_LAST as u8;
        let order = BookOrder {
            side: OrderSide::Buy,
            price: Price::from("100.00"),
            size: Quantity::from(50),
            order_id: 12345, // Price-hash ID
        };
        ladder.add(order, batch_flags);
        assert_eq!(ladder.len(), 1);

        // Clear with zero-size and different order_id (side-constant)
        let clear_order = BookOrder {
            side: OrderSide::Buy,
            price: Price::from("100.00"),
            size: Quantity::zero(9),
            order_id: OrderSide::Buy as u64, // Side-constant ID (different!)
        };
        ladder.add(clear_order, 0);

        // Should be cleared despite order_id mismatch
        assert_eq!(
            ladder.len(),
            0,
            "Zero-size should clear L1 regardless of order_id"
        );
        assert!(ladder.cache.is_empty(), "Cache should be empty after clear");
    }

    #[rstest]
    fn test_l1_f_mbp_without_f_last_does_not_accumulate() {
        // F_MBP without F_LAST: each message clears, preventing stale prices.
        // This allows prices to degrade when the market moves.
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L1_MBP);
        let flags = RecordFlag::F_MBP as u8; // No F_LAST

        // Prices descending from 100 to 91 (simulates degrading market)
        let prices = [
            "100.00", "99.00", "98.00", "97.00", "96.00", "95.00", "94.00", "93.00", "92.00",
            "91.00",
        ];

        for (i, price_str) in prices.iter().enumerate() {
            let order = BookOrder {
                side: OrderSide::Buy,
                price: Price::from(*price_str),
                size: Quantity::from(10),
                order_id: (i + 100) as u64,
            };
            ladder.add(order, flags);

            assert_eq!(
                ladder.len(),
                1,
                "L1 should always have at most 1 level, iteration {i}"
            );
        }

        // Final price should be 91 (the last added), not 100 (the best ever seen)
        assert_eq!(
            ladder.top().unwrap().price.value,
            Price::from("91.00"),
            "Should show last price (91), allowing degradation"
        );
    }

    #[rstest]
    fn test_l1_f_mbp_two_delta_batch_retains_best() {
        // A 2-delta batch (F_MBP then F_MBP|F_LAST) accumulates both and keeps best
        let mut ladder = BookLadder::new(OrderSideSpecified::Sell, BookType::L1_MBP);

        // Delta 1 (F_MBP only): clears, adds 100, sets in_l1_batch=true
        let order1 = BookOrder {
            side: OrderSide::Sell,
            price: Price::from("100.00"),
            size: Quantity::from(10),
            order_id: 100,
        };
        ladder.add(order1, RecordFlag::F_MBP as u8);

        // Delta 2 (F_MBP|F_LAST): in_l1_batch=true so doesn't clear,
        // adds 101, now has 100+101, retain_best → 100
        let order2 = BookOrder {
            side: OrderSide::Sell,
            price: Price::from("101.00"),
            size: Quantity::from(20),
            order_id: 101,
        };
        ladder.add(order2, RecordFlag::F_MBP as u8 | RecordFlag::F_LAST as u8);

        assert_eq!(ladder.len(), 1);
        assert_eq!(
            ladder.top().unwrap().price.value,
            Price::from("100.00"),
            "2-delta batch keeps best ask (100) from both deltas"
        );
    }

    #[rstest]
    fn test_l1_snapshot_batch_accumulates_all_levels_bids() {
        // F_SNAPSHOT batch accumulates ALL levels and keeps best bid
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L1_MBP);
        let prices = ["98.00", "99.00", "100.00", "101.00"];
        let batch_size = prices.len();

        for (i, price_str) in prices.iter().enumerate() {
            let order = BookOrder {
                side: OrderSide::Buy,
                price: Price::from(*price_str),
                size: Quantity::from(10),
                order_id: (i + 100) as u64,
            };
            let flags = if i == batch_size - 1 {
                RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8
            } else {
                RecordFlag::F_SNAPSHOT as u8
            };
            ladder.add(order, flags);
        }

        assert_eq!(
            ladder.len(),
            1,
            "L1 should have only 1 level after snapshot"
        );
        assert_eq!(
            ladder.top().unwrap().price.value,
            Price::from("101.00"),
            "F_SNAPSHOT batch should keep best bid (101) from ALL deltas"
        );
    }

    #[rstest]
    fn test_l1_snapshot_batch_accumulates_all_levels_asks() {
        // F_SNAPSHOT batch accumulates ALL levels and keeps best ask
        let mut ladder = BookLadder::new(OrderSideSpecified::Sell, BookType::L1_MBP);
        let prices = ["104.00", "103.00", "102.00", "101.00"];
        let batch_size = prices.len();

        for (i, price_str) in prices.iter().enumerate() {
            let order = BookOrder {
                side: OrderSide::Sell,
                price: Price::from(*price_str),
                size: Quantity::from(10),
                order_id: (i + 100) as u64,
            };
            let flags = if i == batch_size - 1 {
                RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8
            } else {
                RecordFlag::F_SNAPSHOT as u8
            };
            ladder.add(order, flags);
        }

        assert_eq!(
            ladder.len(),
            1,
            "L1 should have only 1 level after snapshot"
        );
        assert_eq!(
            ladder.top().unwrap().price.value,
            Price::from("101.00"),
            "F_SNAPSHOT batch should keep best ask (101) from ALL deltas"
        );
    }

    #[rstest]
    fn test_l1_snapshot_vs_mbp_different_accumulation_behavior() {
        // F_SNAPSHOT accumulates all levels, F_MBP only accumulates final two
        let mut mbp_ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L1_MBP);
        let prices = ["98.00", "99.00", "100.00", "101.00"];
        for (i, price_str) in prices.iter().enumerate() {
            let order = BookOrder {
                side: OrderSide::Buy,
                price: Price::from(*price_str),
                size: Quantity::from(10),
                order_id: (i + 100) as u64,
            };
            let flags = if i == prices.len() - 1 {
                RecordFlag::F_MBP as u8 | RecordFlag::F_LAST as u8
            } else {
                RecordFlag::F_MBP as u8
            };
            mbp_ladder.add(order, flags);
        }
        assert_eq!(
            mbp_ladder.top().unwrap().price.value,
            Price::from("101.00"),
            "F_MBP keeps best of final two (100, 101)"
        );

        let mut snapshot_ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L1_MBP);
        for (i, price_str) in prices.iter().enumerate() {
            let order = BookOrder {
                side: OrderSide::Buy,
                price: Price::from(*price_str),
                size: Quantity::from(10),
                order_id: (i + 200) as u64,
            };
            let flags = if i == prices.len() - 1 {
                RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8
            } else {
                RecordFlag::F_SNAPSHOT as u8
            };
            snapshot_ladder.add(order, flags);
        }
        assert_eq!(
            snapshot_ladder.top().unwrap().price.value,
            Price::from("101.00"),
            "F_SNAPSHOT keeps best of ALL deltas (98, 99, 100, 101)"
        );
    }

    #[rstest]
    fn test_l1_snapshot_after_incomplete_mbp_stream() {
        // Snapshot must clear stale state from incomplete F_MBP stream (no F_LAST sent)
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L1_MBP);

        // Incomplete F_MBP stream leaves stale batch state
        let stale_order = BookOrder {
            side: OrderSide::Buy,
            price: Price::from("101.00"),
            size: Quantity::from(10),
            order_id: 100,
        };
        ladder.add(stale_order, RecordFlag::F_MBP as u8);
        assert_eq!(ladder.top().unwrap().price.value, Price::from("101.00"));

        // Snapshot arrives with Clear delta first
        ladder.clear();

        // Snapshot prices worse than stale 101
        for (i, price_str) in ["98.00", "99.00", "100.00"].iter().enumerate() {
            let order = BookOrder {
                side: OrderSide::Buy,
                price: Price::from(*price_str),
                size: Quantity::from(10),
                order_id: (i + 200) as u64,
            };
            let flags = if i == 2 {
                RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8
            } else {
                RecordFlag::F_SNAPSHOT as u8
            };
            ladder.add(order, flags);
        }

        assert_eq!(
            ladder.top().unwrap().price.value,
            Price::from("100.00"),
            "Snapshot replaces stale MBP state: best is 100, not stale 101"
        );
    }

    #[rstest]
    fn test_l1_snapshot_clears_previous_batch() {
        // New F_SNAPSHOT batch clears previous batch
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L1_MBP);

        for (i, price_str) in ["100.00", "101.00", "102.00"].iter().enumerate() {
            let order = BookOrder {
                side: OrderSide::Buy,
                price: Price::from(*price_str),
                size: Quantity::from(10),
                order_id: (i + 100) as u64,
            };
            let flags = if i == 2 {
                RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8
            } else {
                RecordFlag::F_SNAPSHOT as u8
            };
            ladder.add(order, flags);
        }
        assert_eq!(ladder.top().unwrap().price.value, Price::from("102.00"));

        // Second batch with worse prices
        for (i, price_str) in ["95.00", "96.00", "97.00"].iter().enumerate() {
            let order = BookOrder {
                side: OrderSide::Buy,
                price: Price::from(*price_str),
                size: Quantity::from(20),
                order_id: (i + 200) as u64,
            };
            let flags = if i == 2 {
                RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8
            } else {
                RecordFlag::F_SNAPSHOT as u8
            };
            ladder.add(order, flags);
        }
        assert_eq!(
            ladder.top().unwrap().price.value,
            Price::from("97.00"),
            "Second batch clears first: best is 97, not 102"
        );
    }

    #[rstest]
    fn test_l1_single_delta_snapshot_after_mbp_batch() {
        // Single-delta snapshot (F_SNAPSHOT|F_LAST) must clear stale MBP batch state
        let mut ladder = BookLadder::new(OrderSideSpecified::Buy, BookType::L1_MBP);

        let mbp_order1 = BookOrder {
            side: OrderSide::Buy,
            price: Price::from("100.00"),
            size: Quantity::from(10),
            order_id: 1,
        };
        let mbp_order2 = BookOrder {
            side: OrderSide::Buy,
            price: Price::from("101.00"),
            size: Quantity::from(10),
            order_id: 2,
        };
        ladder.add(mbp_order1, RecordFlag::F_MBP as u8);
        ladder.add(
            mbp_order2,
            RecordFlag::F_MBP as u8 | RecordFlag::F_LAST as u8,
        );

        assert_eq!(ladder.top().unwrap().price.value, Price::from("101.00"));

        // Single-delta snapshot at worse price (no preceding Clear)
        let snapshot_order = BookOrder {
            side: OrderSide::Buy,
            price: Price::from("95.00"),
            size: Quantity::from(20),
            order_id: 100,
        };
        ladder.add(
            snapshot_order,
            RecordFlag::F_SNAPSHOT as u8 | RecordFlag::F_LAST as u8,
        );

        assert_eq!(
            ladder.top().unwrap().price.value,
            Price::from("95.00"),
            "Single-delta snapshot clears MBP state: best is 95, not stale 101"
        );
        assert_eq!(ladder.len(), 1);
    }
}
