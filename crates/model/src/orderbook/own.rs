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

//! An `OwnBookOrder` for use with tracking own/user orders in L3 order books.
//! It organizes orders into bid and ask ladders, maintains timestamps for state changes,
//! and provides various methods for adding, updating, deleting, and querying orders.

use std::{
    cmp::Ordering,
    collections::{BTreeMap, HashMap},
    fmt::{Debug, Display},
    hash::{Hash, Hasher},
};

use indexmap::IndexMap;
use nautilus_core::UnixNanos;
use rust_decimal::Decimal;

use super::display::pprint_own_book;
use crate::{
    enums::{OrderSideSpecified, OrderStatus, OrderType, TimeInForce},
    identifiers::{ClientOrderId, InstrumentId},
    orderbook::BookPrice,
    types::{Price, Quantity},
};

/// Represents an own/user order for a book.
///
/// This struct models an order that may be in-flight to the trading venue or actively working,
/// depending on the value of the `status` field.
#[repr(C)]
#[derive(Clone, Copy, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct OwnBookOrder {
    /// The client order ID.
    pub client_order_id: ClientOrderId,
    /// The specified order side (BUY or SELL).
    pub side: OrderSideSpecified,
    /// The order price.
    pub price: Price,
    /// The order size.
    pub size: Quantity,
    /// The order type.
    pub order_type: OrderType,
    /// The order time in force.
    pub time_in_force: TimeInForce,
    /// The current order status (SUBMITTED/ACCEPTED/CANCELED/FILLED).
    pub status: OrderStatus,
    /// UNIX timestamp (nanoseconds) when the last event occurred for this order.
    pub ts_last: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the order was initialized.
    pub ts_init: UnixNanos,
}

impl OwnBookOrder {
    /// Creates a new [`OwnBookOrder`] instance.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        client_order_id: ClientOrderId,
        side: OrderSideSpecified,
        price: Price,
        size: Quantity,
        order_type: OrderType,
        time_in_force: TimeInForce,
        status: OrderStatus,
        ts_last: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            client_order_id,
            side,
            price,
            size,
            order_type,
            time_in_force,
            status,
            ts_last,
            ts_init,
        }
    }

    /// Returns a [`BookPrice`] from this order.
    #[must_use]
    pub fn to_book_price(&self) -> BookPrice {
        BookPrice::new(self.price, self.side)
    }

    /// Returns the order exposure as an `f64`.
    #[must_use]
    pub fn exposure(&self) -> f64 {
        self.price.as_f64() * self.size.as_f64()
    }

    /// Returns the signed order exposure as an `f64`.
    #[must_use]
    pub fn signed_size(&self) -> f64 {
        match self.side {
            OrderSideSpecified::Buy => self.size.as_f64(),
            OrderSideSpecified::Sell => -(self.size.as_f64()),
        }
    }
}

impl Ord for OwnBookOrder {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Compare solely based on ts_init.
        self.ts_init.cmp(&other.ts_init)
    }
}

impl PartialOrd for OwnBookOrder {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for OwnBookOrder {
    fn eq(&self, other: &Self) -> bool {
        self.client_order_id == other.client_order_id && self.ts_init == other.ts_init
    }
}

impl Hash for OwnBookOrder {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.client_order_id.hash(state);
    }
}

impl Debug for OwnBookOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(client_order_id={}, side={}, price={}, size={}, order_type={}, time_in_force={}, ts_init={})",
            stringify!(OwnBookOrder),
            self.client_order_id,
            self.side,
            self.price,
            self.size,
            self.order_type,
            self.time_in_force,
            self.ts_init,
        )
    }
}

impl Display for OwnBookOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{},{},{},{},{}",
            self.client_order_id,
            self.side,
            self.price,
            self.size,
            self.order_type,
            self.time_in_force,
            self.ts_init,
        )
    }
}

#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct OwnOrderBook {
    /// The instrument ID for the order book.
    pub instrument_id: InstrumentId,
    /// The timestamp of the last event applied to the order book.
    pub ts_last: UnixNanos,
    /// The current count of events applied to the order book.
    pub event_count: u64,
    pub(crate) bids: OwnBookLadder,
    pub(crate) asks: OwnBookLadder,
}

impl PartialEq for OwnOrderBook {
    fn eq(&self, other: &Self) -> bool {
        self.instrument_id == other.instrument_id
    }
}

impl Display for OwnOrderBook {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(instrument_id={}, event_count={})",
            stringify!(OwnOrderBook),
            self.instrument_id,
            self.event_count,
        )
    }
}

impl OwnOrderBook {
    /// Creates a new [`OwnOrderBook`] instance.
    #[must_use]
    pub fn new(instrument_id: InstrumentId) -> Self {
        Self {
            instrument_id,
            ts_last: UnixNanos::default(),
            event_count: 0,
            bids: OwnBookLadder::new(OrderSideSpecified::Buy),
            asks: OwnBookLadder::new(OrderSideSpecified::Sell),
        }
    }

    fn increment(&mut self, order: &OwnBookOrder) {
        self.ts_last = order.ts_last;
        self.event_count += 1;
    }

    /// Resets the order book to its initial empty state.
    pub fn reset(&mut self) {
        self.bids.clear();
        self.asks.clear();
        self.ts_last = UnixNanos::default();
        self.event_count = 0;
    }

    /// Adds an own order to the book.
    pub fn add(&mut self, order: OwnBookOrder) {
        self.increment(&order);
        match order.side {
            OrderSideSpecified::Buy => self.bids.add(order),
            OrderSideSpecified::Sell => self.asks.add(order),
        }
    }

    /// Updates an existing own order in the book.
    pub fn update(&mut self, order: OwnBookOrder) -> anyhow::Result<()> {
        self.increment(&order);
        match order.side {
            OrderSideSpecified::Buy => self.bids.update(order),
            OrderSideSpecified::Sell => self.asks.update(order),
        }
    }

    /// Deletes an own order from the book.
    pub fn delete(&mut self, order: OwnBookOrder) -> anyhow::Result<()> {
        self.increment(&order);
        match order.side {
            OrderSideSpecified::Buy => self.bids.delete(order),
            OrderSideSpecified::Sell => self.asks.delete(order),
        }
    }

    /// Clears all orders from both sides of the book.
    pub fn clear(&mut self) {
        self.bids.clear();
        self.asks.clear();
    }

    /// Returns an iterator over bid price levels.
    pub fn bids(&self) -> impl Iterator<Item = &OwnBookLevel> {
        self.bids.levels.values()
    }

    /// Returns an iterator over ask price levels.
    pub fn asks(&self) -> impl Iterator<Item = &OwnBookLevel> {
        self.asks.levels.values()
    }

    /// Returns bid price levels as a map of level price to order list at that level.
    pub fn bids_as_map(&self) -> IndexMap<Decimal, Vec<OwnBookOrder>> {
        self.bids()
            .map(|level| {
                (
                    level.price.value.as_decimal(),
                    level.orders.values().cloned().collect(),
                )
            })
            .collect()
    }

    /// Returns ask price levels as a map of level price to order list at that level.
    pub fn asks_as_map(&self) -> IndexMap<Decimal, Vec<OwnBookOrder>> {
        self.asks()
            .map(|level| {
                (
                    level.price.value.as_decimal(),
                    level.orders.values().cloned().collect(),
                )
            })
            .collect()
    }

    /// Return a formatted string representation of the order book.
    #[must_use]
    pub fn pprint(&self, num_levels: usize) -> String {
        pprint_own_book(&self.bids, &self.asks, num_levels)
    }
}

/// Represents a ladder of price levels for one side of an order book.
pub(crate) struct OwnBookLadder {
    pub side: OrderSideSpecified,
    pub levels: BTreeMap<BookPrice, OwnBookLevel>,
    pub cache: HashMap<ClientOrderId, BookPrice>,
}

impl OwnBookLadder {
    /// Creates a new [`OwnBookLadder`] instance.
    #[must_use]
    pub fn new(side: OrderSideSpecified) -> Self {
        Self {
            side,
            levels: BTreeMap::new(),
            cache: HashMap::new(),
        }
    }

    /// Returns the number of price levels in the ladder.
    #[must_use]
    #[allow(dead_code)] // Used in tests
    pub fn len(&self) -> usize {
        self.levels.len()
    }

    /// Returns true if the ladder has no price levels.
    #[must_use]
    #[allow(dead_code)] // Used in tests
    pub fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }

    /// Removes all orders and price levels from the ladder.
    pub fn clear(&mut self) {
        self.levels.clear();
        self.cache.clear();
    }

    /// Adds an order to the ladder at its price level.
    pub fn add(&mut self, order: OwnBookOrder) {
        let book_price = order.to_book_price();
        self.cache.insert(order.client_order_id, book_price);

        match self.levels.get_mut(&book_price) {
            Some(level) => {
                level.add(order);
            }
            None => {
                let level = OwnBookLevel::from_order(order);
                self.levels.insert(book_price, level);
            }
        }
    }

    /// Updates an existing order in the ladder, moving it to a new price level if needed.
    pub fn update(&mut self, order: OwnBookOrder) -> anyhow::Result<()> {
        let price = self.cache.get(&order.client_order_id).copied();
        if let Some(price) = price {
            if let Some(level) = self.levels.get_mut(&price) {
                if order.price == level.price.value {
                    // Update at current price level
                    level.update(order);
                    return Ok(());
                }

                // Price update: delete and insert at new level
                self.cache.remove(&order.client_order_id);
                level.delete(&order.client_order_id)?;
                if level.is_empty() {
                    self.levels.remove(&price);
                }
            }
        }

        self.add(order);
        Ok(())
    }

    /// Deletes an order from the ladder.
    pub fn delete(&mut self, order: OwnBookOrder) -> anyhow::Result<()> {
        self.remove(order.client_order_id)
    }

    /// Removes an order by its ID from the ladder.
    pub fn remove(&mut self, client_order_id: ClientOrderId) -> anyhow::Result<()> {
        if let Some(price) = self.cache.remove(&client_order_id) {
            if let Some(level) = self.levels.get_mut(&price) {
                level.delete(&client_order_id)?;
                if level.is_empty() {
                    self.levels.remove(&price);
                }
            }
        }

        Ok(())
    }

    /// Returns the total size of all orders in the ladder.
    #[must_use]
    #[allow(dead_code)] // Used in tests
    pub fn sizes(&self) -> f64 {
        self.levels.values().map(OwnBookLevel::size).sum()
    }

    /// Returns the total value exposure (price * size) of all orders in the ladder.
    #[must_use]
    #[allow(dead_code)] // Used in tests
    pub fn exposures(&self) -> f64 {
        self.levels.values().map(OwnBookLevel::exposure).sum()
    }

    /// Returns the best price level in the ladder.
    #[must_use]
    #[allow(dead_code)] // Used in tests
    pub fn top(&self) -> Option<&OwnBookLevel> {
        match self.levels.iter().next() {
            Some((_, l)) => Option::Some(l),
            None => Option::None,
        }
    }
}

impl Debug for OwnBookLadder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(OwnBookLadder))
            .field("side", &self.side)
            .field("levels", &self.levels)
            .finish()
    }
}

impl Display for OwnBookLadder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "{}(side={})", stringify!(OwnBookLadder), self.side)?;
        for (price, level) in &self.levels {
            writeln!(f, "  {} -> {} orders", price, level.len())?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct OwnBookLevel {
    pub price: BookPrice,
    pub orders: IndexMap<ClientOrderId, OwnBookOrder>,
}

impl OwnBookLevel {
    /// Creates a new [`OwnBookLevel`] instance.
    #[must_use]
    pub fn new(price: BookPrice) -> Self {
        Self {
            price,
            orders: IndexMap::new(),
        }
    }

    /// Creates a new [`OwnBookLevel`] from an order, using the order's price and side.
    #[must_use]
    pub fn from_order(order: OwnBookOrder) -> Self {
        let mut level = Self {
            price: order.to_book_price(),
            orders: IndexMap::new(),
        };
        level.orders.insert(order.client_order_id, order);
        level
    }

    /// Returns the number of orders at this price level.
    #[must_use]
    pub fn len(&self) -> usize {
        self.orders.len()
    }

    /// Returns true if this price level has no orders.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }

    /// Returns a reference to the first order at this price level in FIFO order.
    #[must_use]
    pub fn first(&self) -> Option<&OwnBookOrder> {
        self.orders.get_index(0).map(|(_key, order)| order)
    }

    /// Returns an iterator over the orders at this price level in FIFO order.
    pub fn iter(&self) -> impl Iterator<Item = &OwnBookOrder> {
        self.orders.values()
    }

    /// Returns all orders at this price level in FIFO insertion order.
    #[must_use]
    pub fn get_orders(&self) -> Vec<OwnBookOrder> {
        self.orders.values().copied().collect()
    }

    /// Returns the total size of all orders at this price level as a float.
    #[must_use]
    pub fn size(&self) -> f64 {
        self.orders.iter().map(|(_, o)| o.size.as_f64()).sum()
    }

    /// Returns the total size of all orders at this price level as a decimal.
    #[must_use]
    pub fn size_decimal(&self) -> Decimal {
        self.orders.iter().map(|(_, o)| o.size.as_decimal()).sum()
    }

    /// Returns the total exposure (price * size) of all orders at this price level as a float.
    #[must_use]
    pub fn exposure(&self) -> f64 {
        self.orders
            .iter()
            .map(|(_, o)| o.price.as_f64() * o.size.as_f64())
            .sum()
    }

    /// Adds multiple orders to this price level in FIFO order. Orders must match the level's price.
    pub fn add_bulk(&mut self, orders: Vec<OwnBookOrder>) {
        for order in orders {
            self.add(order);
        }
    }

    /// Adds an order to this price level. Order must match the level's price.
    pub fn add(&mut self, order: OwnBookOrder) {
        debug_assert_eq!(order.price, self.price.value);

        self.orders.insert(order.client_order_id, order);
    }

    /// Updates an existing order at this price level. Updated order must match the level's price.
    /// Removes the order if size becomes zero.
    pub fn update(&mut self, order: OwnBookOrder) {
        debug_assert_eq!(order.price, self.price.value);

        self.orders[&order.client_order_id] = order;
    }

    /// Deletes an order from this price level.
    pub fn delete(&mut self, client_order_id: &ClientOrderId) -> anyhow::Result<()> {
        if self.orders.shift_remove(client_order_id).is_none() {
            // TODO: Use a generic anyhow result for now pending specific error types
            anyhow::bail!("Order {client_order_id} not found for delete");
        };
        Ok(())
    }
}

impl PartialEq for OwnBookLevel {
    fn eq(&self, other: &Self) -> bool {
        self.price == other.price
    }
}

impl Eq for OwnBookLevel {}

impl PartialOrd for OwnBookLevel {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OwnBookLevel {
    fn cmp(&self, other: &Self) -> Ordering {
        self.price.cmp(&other.price)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use rstest::{fixture, rstest};

    use super::*;

    #[fixture]
    fn own_order() -> OwnBookOrder {
        let client_order_id = ClientOrderId::from("O-123456789");
        let side = OrderSideSpecified::Buy;
        let price = Price::from("100.00");
        let size = Quantity::from("10");
        let order_type = OrderType::Limit;
        let time_in_force = TimeInForce::Gtc;
        let status = OrderStatus::Submitted;
        let ts_last = UnixNanos::default();
        let ts_init = UnixNanos::default();

        OwnBookOrder::new(
            client_order_id,
            side,
            price,
            size,
            order_type,
            time_in_force,
            status,
            ts_last,
            ts_init,
        )
    }

    #[rstest]
    fn test_to_book_price(own_order: OwnBookOrder) {
        let book_price = own_order.to_book_price();
        assert_eq!(book_price.value, Price::from("100.00"));
        assert_eq!(book_price.side, OrderSideSpecified::Buy);
    }

    #[rstest]
    fn test_exposure(own_order: OwnBookOrder) {
        let exposure = own_order.exposure();
        assert_eq!(exposure, 1000.0);
    }

    #[rstest]
    fn test_signed_size(own_order: OwnBookOrder) {
        let own_order_buy = own_order;
        let own_order_sell = OwnBookOrder::new(
            ClientOrderId::from("O-123456789"),
            OrderSideSpecified::Sell,
            Price::from("101.0"),
            Quantity::from("10"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert_eq!(own_order_buy.signed_size(), 10.0);
        assert_eq!(own_order_sell.signed_size(), -10.0);
    }

    #[rstest]
    fn test_debug(own_order: OwnBookOrder) {
        assert_eq!(
            format!("{own_order:?}"),
            "OwnBookOrder(client_order_id=O-123456789, side=BUY, price=100.00, size=10, order_type=LIMIT, time_in_force=GTC, ts_init=0)"
        );
    }

    #[rstest]
    fn test_display(own_order: OwnBookOrder) {
        assert_eq!(
            own_order.to_string(),
            "O-123456789,BUY,100.00,10,LIMIT,GTC,0".to_string()
        );
    }

    #[rstest]
    fn test_own_book_level_size_and_exposure() {
        let mut level = OwnBookLevel::new(BookPrice::new(
            Price::from("100.00"),
            OrderSideSpecified::Buy,
        ));
        let order1 = OwnBookOrder::new(
            ClientOrderId::from("O-1"),
            OrderSideSpecified::Buy,
            Price::from("100.00"),
            Quantity::from("10"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
        );
        let order2 = OwnBookOrder::new(
            ClientOrderId::from("O-2"),
            OrderSideSpecified::Buy,
            Price::from("100.00"),
            Quantity::from("20"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
        );
        level.add(order1);
        level.add(order2);

        assert_eq!(level.len(), 2);
        assert_eq!(level.size(), 30.0);
        assert_eq!(level.exposure(), 3000.0);
    }

    #[rstest]
    fn test_own_book_level_add_update_delete() {
        let mut level = OwnBookLevel::new(BookPrice::new(
            Price::from("100.00"),
            OrderSideSpecified::Buy,
        ));
        let order = OwnBookOrder::new(
            ClientOrderId::from("O-1"),
            OrderSideSpecified::Buy,
            Price::from("100.00"),
            Quantity::from("10"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
        );
        level.add(order);
        assert_eq!(level.len(), 1);

        // Update the order to a new size
        let order_updated = OwnBookOrder::new(
            ClientOrderId::from("O-1"),
            OrderSideSpecified::Buy,
            Price::from("100.00"),
            Quantity::from("15"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
        );
        level.update(order_updated);
        let orders = level.get_orders();
        assert_eq!(orders[0].size, Quantity::from("15"));

        // Delete the order
        level.delete(&ClientOrderId::from("O-1")).unwrap();
        assert!(level.is_empty());
    }

    #[rstest]
    fn test_own_book_ladder_add_update_delete() {
        let mut ladder = OwnBookLadder::new(OrderSideSpecified::Buy);
        let order1 = OwnBookOrder::new(
            ClientOrderId::from("O-1"),
            OrderSideSpecified::Buy,
            Price::from("100.00"),
            Quantity::from("10"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
        );
        let order2 = OwnBookOrder::new(
            ClientOrderId::from("O-2"),
            OrderSideSpecified::Buy,
            Price::from("100.00"),
            Quantity::from("20"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
        );
        ladder.add(order1);
        ladder.add(order2);
        assert_eq!(ladder.len(), 1);
        assert_eq!(ladder.sizes(), 30.0);

        // Update order2 to a larger size
        let order2_updated = OwnBookOrder::new(
            ClientOrderId::from("O-2"),
            OrderSideSpecified::Buy,
            Price::from("100.00"),
            Quantity::from("25"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
        );
        ladder.update(order2_updated).unwrap();
        assert_eq!(ladder.sizes(), 35.0);

        // Delete order1
        ladder.delete(order1).unwrap();
        assert_eq!(ladder.sizes(), 25.0);
    }

    #[rstest]
    fn test_own_order_book_add_update_delete_clear() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut book = OwnOrderBook::new(instrument_id);
        let order_buy = OwnBookOrder::new(
            ClientOrderId::from("O-1"),
            OrderSideSpecified::Buy,
            Price::from("100.00"),
            Quantity::from("10"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
        );
        let order_sell = OwnBookOrder::new(
            ClientOrderId::from("O-2"),
            OrderSideSpecified::Sell,
            Price::from("101.00"),
            Quantity::from("20"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        // Add orders to respective ladders
        book.add(order_buy);
        book.add(order_sell);
        assert!(!book.bids.is_empty());
        assert!(!book.asks.is_empty());

        // Update buy order
        let order_buy_updated = OwnBookOrder::new(
            ClientOrderId::from("O-1"),
            OrderSideSpecified::Buy,
            Price::from("100.00"),
            Quantity::from("15"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
        );
        book.update(order_buy_updated).unwrap();
        book.delete(order_sell).unwrap();

        assert_eq!(book.bids.sizes(), 15.0);
        assert!(book.asks.is_empty());

        // Clear the book
        book.clear();
        assert!(book.bids.is_empty());
        assert!(book.asks.is_empty());
    }

    #[rstest]
    fn test_own_order_book_bids_and_asks_as_map() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut book = OwnOrderBook::new(instrument_id);
        let order1 = OwnBookOrder::new(
            ClientOrderId::from("O-1"),
            OrderSideSpecified::Buy,
            Price::from("100.00"),
            Quantity::from("10"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
        );
        let order2 = OwnBookOrder::new(
            ClientOrderId::from("O-2"),
            OrderSideSpecified::Sell,
            Price::from("101.00"),
            Quantity::from("20"),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            UnixNanos::default(),
            UnixNanos::default(),
        );
        book.add(order1);
        book.add(order2);
        let bids_map = book.bids_as_map();
        let asks_map = book.asks_as_map();

        assert_eq!(bids_map.len(), 1);
        let bid_price = Price::from("100.00").as_decimal();
        let bid_orders = bids_map.get(&bid_price).unwrap();
        assert_eq!(bid_orders.len(), 1);
        assert_eq!(bid_orders[0], order1);

        assert_eq!(asks_map.len(), 1);
        let ask_price = Price::from("101.00").as_decimal();
        let ask_orders = asks_map.get(&ask_price).unwrap();
        assert_eq!(ask_orders.len(), 1);
        assert_eq!(ask_orders[0], order2);
    }
}
