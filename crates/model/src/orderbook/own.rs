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
    collections::{BTreeMap, HashMap, HashSet},
    fmt::{Debug, Display},
    hash::{Hash, Hasher},
};

use indexmap::IndexMap;
use nautilus_core::{UnixNanos, time::nanos_since_unix_epoch};
use rust_decimal::Decimal;

use super::display::pprint_own_book;
use crate::{
    enums::{OrderSideSpecified, OrderStatus, OrderType, TimeInForce},
    identifiers::{ClientOrderId, InstrumentId, TraderId, VenueOrderId},
    orderbook::BookPrice,
    orders::{Order, OrderAny},
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
    /// The trader ID.
    pub trader_id: TraderId,
    /// The client order ID.
    pub client_order_id: ClientOrderId,
    /// The venue order ID (if assigned by the venue).
    pub venue_order_id: Option<VenueOrderId>,
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
    /// The current order status (SUBMITTED/ACCEPTED/PENDING_CANCEL/PENDING_UPDATE/PARTIALLY_FILLED).
    pub status: OrderStatus,
    /// UNIX timestamp (nanoseconds) when the last order event occurred for this order.
    pub ts_last: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the order was accepted (zero unless accepted).
    pub ts_accepted: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the order was submitted (zero unless submitted).
    pub ts_submitted: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the order was initialized.
    pub ts_init: UnixNanos,
}

impl OwnBookOrder {
    /// Creates a new [`OwnBookOrder`] instance.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        trader_id: TraderId,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        side: OrderSideSpecified,
        price: Price,
        size: Quantity,
        order_type: OrderType,
        time_in_force: TimeInForce,
        status: OrderStatus,
        ts_last: UnixNanos,
        ts_accepted: UnixNanos,
        ts_submitted: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            trader_id,
            client_order_id,
            venue_order_id,
            side,
            price,
            size,
            order_type,
            time_in_force,
            status,
            ts_last,
            ts_accepted,
            ts_submitted,
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
        self.client_order_id == other.client_order_id
            && self.status == other.status
            && self.ts_last == other.ts_last
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
            "{}(trader_id={}, client_order_id={}, venue_order_id={:?}, side={}, price={}, size={}, order_type={}, time_in_force={}, status={}, ts_last={}, ts_accepted={}, ts_submitted={}, ts_init={})",
            stringify!(OwnBookOrder),
            self.trader_id,
            self.client_order_id,
            self.venue_order_id,
            self.side,
            self.price,
            self.size,
            self.order_type,
            self.time_in_force,
            self.status,
            self.ts_last,
            self.ts_accepted,
            self.ts_submitted,
            self.ts_init,
        )
    }
}

impl Display for OwnBookOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{:?},{},{},{},{},{},{},{},{},{},{}",
            self.trader_id,
            self.client_order_id,
            self.venue_order_id,
            self.side,
            self.price,
            self.size,
            self.order_type,
            self.time_in_force,
            self.status,
            self.ts_last,
            self.ts_accepted,
            self.ts_submitted,
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
    /// The current count of updates applied to the order book.
    pub update_count: u64,
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
            "{}(instrument_id={}, orders={}, update_count={})",
            stringify!(OwnOrderBook),
            self.instrument_id,
            self.bids.cache.len() + self.asks.cache.len(),
            self.update_count,
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
            update_count: 0,
            bids: OwnBookLadder::new(OrderSideSpecified::Buy),
            asks: OwnBookLadder::new(OrderSideSpecified::Sell),
        }
    }

    fn increment(&mut self, order: &OwnBookOrder) {
        self.ts_last = order.ts_last;
        self.update_count += 1;
    }

    /// Resets the order book to its initial empty state.
    pub fn reset(&mut self) {
        self.bids.clear();
        self.asks.clear();
        self.ts_last = UnixNanos::default();
        self.update_count = 0;
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

    /// Returns the client order IDs currently on the bid side.
    pub fn bid_client_order_ids(&self) -> Vec<ClientOrderId> {
        self.bids.cache.keys().cloned().collect()
    }

    /// Returns the client order IDs currently on the ask side.
    pub fn ask_client_order_ids(&self) -> Vec<ClientOrderId> {
        self.asks.cache.keys().cloned().collect()
    }

    /// Return whether the given client order ID is in the own book.
    pub fn is_order_in_book(&self, client_order_id: &ClientOrderId) -> bool {
        self.asks.cache.contains_key(client_order_id)
            || self.bids.cache.contains_key(client_order_id)
    }

    /// Maps bid price levels to their own orders, excluding empty levels after filtering.
    ///
    /// Filters by `status` if provided. With `accepted_buffer_ns`, only includes orders accepted
    /// at least that many nanoseconds before `ts_now` (defaults to now).
    pub fn bids_as_map(
        &self,
        status: Option<HashSet<OrderStatus>>,
        accepted_buffer_ns: Option<u64>,
        ts_now: Option<u64>,
    ) -> IndexMap<Decimal, Vec<OwnBookOrder>> {
        filter_orders(self.bids(), status.as_ref(), accepted_buffer_ns, ts_now)
    }

    /// Maps ask price levels to their own orders, excluding empty levels after filtering.
    ///
    /// Filters by `status` if provided. With `accepted_buffer_ns`, only includes orders accepted
    /// at least that many nanoseconds before `ts_now` (defaults to now).
    pub fn asks_as_map(
        &self,
        status: Option<HashSet<OrderStatus>>,
        accepted_buffer_ns: Option<u64>,
        ts_now: Option<u64>,
    ) -> IndexMap<Decimal, Vec<OwnBookOrder>> {
        filter_orders(self.asks(), status.as_ref(), accepted_buffer_ns, ts_now)
    }

    /// Aggregates own bid quantities per price level, omitting zero-quantity levels.
    ///
    /// Filters by `status` if provided, including only matching orders. With `accepted_buffer_ns`,
    /// only includes orders accepted at least that many nanoseconds before `ts_now` (defaults to now).
    pub fn bid_quantity(
        &self,
        status: Option<HashSet<OrderStatus>>,
        accepted_buffer_ns: Option<u64>,
        ts_now: Option<u64>,
    ) -> IndexMap<Decimal, Decimal> {
        self.bids_as_map(status, accepted_buffer_ns, ts_now)
            .into_iter()
            .map(|(price, orders)| (price, sum_order_sizes(orders.iter())))
            .filter(|(_, quantity)| *quantity > Decimal::ZERO)
            .collect()
    }

    /// Aggregates own ask quantities per price level, omitting zero-quantity levels.
    ///
    /// Filters by `status` if provided, including only matching orders. With `accepted_buffer_ns`,
    /// only includes orders accepted at least that many nanoseconds before `ts_now` (defaults to now).
    pub fn ask_quantity(
        &self,
        status: Option<HashSet<OrderStatus>>,
        accepted_buffer_ns: Option<u64>,
        ts_now: Option<u64>,
    ) -> IndexMap<Decimal, Decimal> {
        self.asks_as_map(status, accepted_buffer_ns, ts_now)
            .into_iter()
            .map(|(price, orders)| {
                let quantity = sum_order_sizes(orders.iter());
                (price, quantity)
            })
            .filter(|(_, quantity)| *quantity > Decimal::ZERO)
            .collect()
    }

    /// Groups own bid quantities by price into buckets, truncating to a maximum depth.
    ///
    /// Filters by `status` if provided. With `accepted_buffer_ns`, only includes orders accepted
    /// at least that many nanoseconds before `ts_now` (defaults to now).
    pub fn group_bids(
        &self,
        group_size: Decimal,
        depth: Option<usize>,
        status: Option<HashSet<OrderStatus>>,
        accepted_buffer_ns: Option<u64>,
        ts_now: Option<u64>,
    ) -> IndexMap<Decimal, Decimal> {
        let quantities = self.bid_quantity(status, accepted_buffer_ns, ts_now);
        group_quantities(quantities, group_size, depth, true)
    }

    /// Groups own ask quantities by price into buckets, truncating to a maximum depth.
    ///
    /// Filters by `status` if provided. With `accepted_buffer_ns`, only includes orders accepted
    /// at least that many nanoseconds before `ts_now` (defaults to now).
    pub fn group_asks(
        &self,
        group_size: Decimal,
        depth: Option<usize>,
        status: Option<HashSet<OrderStatus>>,
        accepted_buffer_ns: Option<u64>,
        ts_now: Option<u64>,
    ) -> IndexMap<Decimal, Decimal> {
        let quantities = self.ask_quantity(status, accepted_buffer_ns, ts_now);
        group_quantities(quantities, group_size, depth, false)
    }

    /// Return a formatted string representation of the order book.
    #[must_use]
    pub fn pprint(&self, num_levels: usize) -> String {
        pprint_own_book(&self.bids, &self.asks, num_levels)
    }

    pub fn audit_open_orders(&mut self, open_order_ids: &HashSet<ClientOrderId>) {
        log::debug!("Auditing {self}");

        // Audit bids
        let bids_to_remove: Vec<ClientOrderId> = self
            .bids
            .cache
            .keys()
            .filter(|&key| !open_order_ids.contains(key))
            .cloned()
            .collect();

        // Audit asks
        let asks_to_remove: Vec<ClientOrderId> = self
            .asks
            .cache
            .keys()
            .filter(|&key| !open_order_ids.contains(key))
            .cloned()
            .collect();

        for client_order_id in bids_to_remove {
            log_audit_error(&client_order_id);
            if let Err(e) = self.bids.remove(&client_order_id) {
                log::error!("{e}");
            }
        }

        for client_order_id in asks_to_remove {
            log_audit_error(&client_order_id);
            if let Err(e) = self.asks.remove(&client_order_id) {
                log::error!("{e}");
            }
        }
    }
}

fn log_audit_error(client_order_id: &ClientOrderId) {
    log::error!(
        "Audit error - {} cached order already closed, deleting from own book",
        client_order_id
    );
}

fn filter_orders<'a>(
    levels: impl Iterator<Item = &'a OwnBookLevel>,
    status: Option<&HashSet<OrderStatus>>,
    accepted_buffer_ns: Option<u64>,
    ts_now: Option<u64>,
) -> IndexMap<Decimal, Vec<OwnBookOrder>> {
    let accepted_buffer_ns = accepted_buffer_ns.unwrap_or(0);
    let ts_now = ts_now.unwrap_or_else(nanos_since_unix_epoch);
    levels
        .map(|level| {
            let orders = level
                .orders
                .values()
                .filter(|order| status.is_none_or(|f| f.contains(&order.status)))
                .filter(|order| order.ts_accepted + accepted_buffer_ns <= ts_now)
                .cloned()
                .collect::<Vec<OwnBookOrder>>();

            (level.price.value.as_decimal(), orders)
        })
        .filter(|(_, orders)| !orders.is_empty())
        .collect::<IndexMap<Decimal, Vec<OwnBookOrder>>>()
}

fn group_quantities(
    quantities: IndexMap<Decimal, Decimal>,
    group_size: Decimal,
    depth: Option<usize>,
    is_bid: bool,
) -> IndexMap<Decimal, Decimal> {
    let mut grouped = IndexMap::new();
    let depth = depth.unwrap_or(usize::MAX);

    for (price, size) in quantities {
        let grouped_price = if is_bid {
            (price / group_size).floor() * group_size
        } else {
            (price / group_size).ceil() * group_size
        };

        grouped
            .entry(grouped_price)
            .and_modify(|total| *total += size)
            .or_insert(size);

        if grouped.len() > depth {
            if is_bid {
                // For bids, remove the lowest price level
                if let Some((lowest_price, _)) = grouped.iter().min_by_key(|(price, _)| *price) {
                    let lowest_price = *lowest_price;
                    grouped.shift_remove(&lowest_price);
                }
            } else {
                // For asks, remove the highest price level
                if let Some((highest_price, _)) = grouped.iter().max_by_key(|(price, _)| *price) {
                    let highest_price = *highest_price;
                    grouped.shift_remove(&highest_price);
                }
            }
        }
    }

    grouped
}

fn sum_order_sizes<'a, I>(orders: I) -> Decimal
where
    I: Iterator<Item = &'a OwnBookOrder>,
{
    orders.fold(Decimal::ZERO, |total, order| {
        total + order.size.as_decimal()
    })
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
        self.remove(&order.client_order_id)
    }

    /// Removes an order by its ID from the ladder.
    pub fn remove(&mut self, client_order_id: &ClientOrderId) -> anyhow::Result<()> {
        if let Some(price) = self.cache.remove(client_order_id) {
            if let Some(level) = self.levels.get_mut(&price) {
                level.delete(client_order_id)?;
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

pub fn should_handle_own_book_order(order: &OrderAny) -> bool {
    order.has_price()
        && order.time_in_force() != TimeInForce::Ioc
        && order.time_in_force() != TimeInForce::Fok
}
