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

//! Order matching core shared by the `OrderMatchingEngine` and other components.
//!
//! Evaluates resting orders against a market-price snapshot and returns fill or
//! trigger actions without removing orders from the books.
//!
//! # Book Layout
//!
//! Each side has separate limit and stop books, keyed by price in a `BTreeMap`:
//!
//! - **Limit book**: keyed by limit price. Holds orders with a limit price and no
//!   trigger price, including converted `MARKET_TO_LIMIT` orders and triggered
//!   stop-limit orders whose trigger price has been cleared.
//! - **Stop book**: keyed by trigger price. Holds `STOP_*`, `*_IF_TOUCHED`, and
//!   `TRAILING_STOP_*` orders that require trigger checks.
//!
//! A per-side pending `SmallVec` holds orders with neither price, such as
//! `MARKET_TO_LIMIT` orders before conversion. These orders remain visible in
//! lookups and snapshots but are excluded from matching.
//!
//! # Ordering Invariant
//!
//! [`OrderMatchingCore::iterate`] processes bids before asks. On each side,
//! limits precede stops, with the following price order:
//!
//! - **Bid limits**: highest price first.
//! - **Ask limits**: lowest price first.
//! - **Bid stops**: lowest trigger first, following the crossing order as the
//!   ask rises through buy-stop levels.
//! - **Ask stops**: highest trigger first, following the crossing order as the
//!   bid falls through sell-stop levels.
//!
//! Each price level stores orders in a `SmallVec` in insertion order, preserving
//! time priority (FIFO at the same price). Traversing the `BTreeMap` forward or
//! backward supplies price order without a separate sort. Order snapshots use
//! the same book order and append pending orders after the stops on each side.
//!
//! # Modify Semantics
//!
//! The core has no in-place modify API. To change a resting order, call
//! [`OrderMatchingCore::delete_order`] followed by [`OrderMatchingCore::add_order`].
//! The order joins the back of its price level, even if the price is unchanged.
//! This models loss of queue position on price changes, but also loses position
//! for quantity-only changes. Preserving position for quantity-only changes
//! would require an in-place update API.
//!
//! # Snapshot Ordering Limitation
//!
//! [`OrderMatchingCore::iterate_bids`] and [`OrderMatchingCore::iterate_asks`]
//! emit all matchable limits before triggered stops on their side. This emission
//! order is deterministic, but it does not reconstruct the order in which prices
//! cross levels. A stop can trigger during a price move and then aggress against
//! the limit book; a snapshot alone cannot recover that sequence when both
//! limits and stops are matchable.
//!
//! Callers must not interpret the limits-then-stops sequence as price-path order,
//! particularly when a gap crosses several limit and stop levels on the same
//! side. The engine supplies the snapshot. Replaying crossings would require
//! additional price-path information, such as previous bid/ask values, and an
//! engine/core change to emit fills and triggers in crossing order.
//!
//! # Duplicate Inserts
//!
//! Each `client_order_id` must appear at most once across both sides.
//! [`OrderMatchingCore::add_order`] panics on duplicate IDs when debug assertions
//! are enabled. Without debug assertions, adding the same ID twice without an
//! intervening [`OrderMatchingCore::delete_order`] leaves duplicate entries that
//! can both match.
//!
//! # Performance
//!
//! Each price-level `SmallVec` stores up to four orders inline, covering the
//! common case of 1-3 orders without a per-bucket heap allocation. The bucket
//! spills to the heap when it exceeds that capacity.
//!
//! For L distinct price levels and B orders at a level, insertion requires an
//! O(log L) tree lookup and an amortized O(1) append. A bucket allocation can
//! move O(B) orders. Deletion requires an O(log L) tree lookup and an O(B) scan
//! and shift. Both L and B are expected to be small in typical use.
//!
//! An `AHashMap` maps each `ClientOrderId` to its side and optional book location.
//! [`OrderMatchingCore::order_exists`] uses only this index.
//! [`OrderMatchingCore::get_order`] and [`OrderMatchingCore::delete_order`] use
//! the index to locate the book, then perform a tree lookup and bucket scan.
//! Pending orders require only the per-side pending-bucket scan after the index
//! lookup. The index serves only point queries and is never iterated, so its
//! randomized hash seed does not affect ordering.

use std::collections::BTreeMap;

use ahash::AHashMap;
use nautilus_model::{
    enums::{OrderSide, OrderType, TriggerType},
    identifiers::{ClientOrderId, InstrumentId},
    orders::{Order, OrderError, PassiveOrderAny, StopOrderAny},
    types::Price,
};
use smallvec::SmallVec;

/// Inline capacity for orders at a single price level. Sized to cover the
/// typical 1-3 orders per level; above this the per-bucket `SmallVec` spills
/// to the heap.
const INLINE_ORDERS_PER_LEVEL: usize = 4;

type OrderBucket = SmallVec<[RestingOrder; INLINE_ORDERS_PER_LEVEL]>;

/// An action returned by [`OrderMatchingCore::iterate`] when an order matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchAction {
    FillLimit(ClientOrderId),
    TriggerStop(ClientOrderId),
}

/// A generic order matching core. See module docs for ordering, modify,
/// duplicate, and performance contracts.
#[derive(Clone, Debug)]
pub struct OrderMatchingCore {
    /// The instrument ID for the matching core.
    pub instrument_id: InstrumentId,
    /// The price increment for the matching core.
    pub price_increment: Price,
    /// The current bid price for the matching core.
    pub bid: Option<Price>,
    /// The current ask price for the matching core.
    pub ask: Option<Price>,
    /// The last price for the matching core.
    pub last: Option<Price>,
    fill_limit_inside_spread: bool,
    bid_limits: BTreeMap<Price, OrderBucket>,
    ask_limits: BTreeMap<Price, OrderBucket>,
    bid_stops: BTreeMap<Price, OrderBucket>,
    ask_stops: BTreeMap<Price, OrderBucket>,
    pending_bid: SmallVec<[RestingOrder; 2]>,
    pending_ask: SmallVec<[RestingOrder; 2]>,
    order_index: AHashMap<ClientOrderId, (OrderSide, Option<(BookKind, Price)>)>,
}

impl OrderMatchingCore {
    /// Creates a new [`OrderMatchingCore`] for the given instrument.
    #[must_use]
    pub fn new(instrument_id: InstrumentId, price_increment: Price) -> Self {
        Self {
            instrument_id,
            price_increment,
            bid: None,
            ask: None,
            last: None,
            fill_limit_inside_spread: false,
            bid_limits: BTreeMap::new(),
            ask_limits: BTreeMap::new(),
            bid_stops: BTreeMap::new(),
            ask_stops: BTreeMap::new(),
            pending_bid: SmallVec::new(),
            pending_ask: SmallVec::new(),
            order_index: AHashMap::new(),
        }
    }

    /// Returns the price precision of the instrument's tick size.
    #[must_use]
    pub const fn price_precision(&self) -> u8 {
        self.price_increment.precision
    }

    /// Returns the order with the given `client_order_id`, searching both sides.
    #[must_use]
    pub fn get_order(&self, client_order_id: ClientOrderId) -> Option<&RestingOrder> {
        let (side, location) = self.order_index.get(&client_order_id).copied()?;
        let orders: &[RestingOrder] = if let Some((kind, price)) = location {
            self.book_for(side, kind).get(&price)?
        } else {
            self.pending_for(side)
        };

        orders.iter().find(|o| o.client_order_id == client_order_id)
    }

    /// Iterates the bid-side orders in price-time priority without
    /// allocating: limits best (highest) first, then stops nearest-trigger
    /// (lowest) first, then pending unkeyed orders. Borrowed view; for an
    /// owned snapshot use [`Self::get_orders_bid`].
    pub fn iter_bid_orders(&self) -> impl Iterator<Item = &RestingOrder> {
        self.iter_bid_book_orders().chain(self.pending_bid.iter())
    }

    /// Iterates the ask-side orders in price-time priority without
    /// allocating: limits best (lowest) first, then stops nearest-trigger
    /// (highest) first, then pending unkeyed orders. Borrowed view; for an
    /// owned snapshot use [`Self::get_orders_ask`].
    pub fn iter_ask_orders(&self) -> impl Iterator<Item = &RestingOrder> {
        self.iter_ask_book_orders().chain(self.pending_ask.iter())
    }

    /// Iterates all orders without allocating, bids (best first) then asks
    /// (best first). Borrowed view; for an owned snapshot use
    /// [`Self::get_orders`].
    pub fn iter_orders(&self) -> impl Iterator<Item = &RestingOrder> {
        self.iter_bid_orders().chain(self.iter_ask_orders())
    }

    /// Returns the bid-side orders in price-time priority: limits best
    /// (highest) first, then stops nearest-trigger (lowest) first, then
    /// pending unkeyed orders. Allocates an owned snapshot; for borrowed
    /// iteration use [`Self::iter_bid_orders`].
    #[must_use]
    pub fn get_orders_bid(&self) -> Vec<RestingOrder> {
        self.iter_bid_orders().copied().collect()
    }

    /// Returns the ask-side orders in price-time priority: limits best
    /// (lowest) first, then stops nearest-trigger (highest) first, then
    /// pending unkeyed orders. Allocates an owned snapshot; for borrowed
    /// iteration use [`Self::iter_ask_orders`].
    #[must_use]
    pub fn get_orders_ask(&self) -> Vec<RestingOrder> {
        self.iter_ask_orders().copied().collect()
    }

    /// Returns all orders, bids (best first) then asks (best first).
    /// Allocates an owned snapshot; for borrowed iteration use
    /// [`Self::iter_orders`].
    #[must_use]
    pub fn get_orders(&self) -> Vec<RestingOrder> {
        self.iter_orders().copied().collect()
    }

    /// Returns whether an order with `client_order_id` is present on either side.
    #[must_use]
    pub fn order_exists(&self, client_order_id: ClientOrderId) -> bool {
        self.order_index.contains_key(&client_order_id)
    }

    /// Sets the last traded price.
    pub const fn set_last_raw(&mut self, last: Price) {
        self.last = Some(last);
    }

    /// Sets the best bid price.
    pub const fn set_bid_raw(&mut self, bid: Price) {
        self.bid = Some(bid);
    }

    /// Sets the best ask price.
    pub const fn set_ask_raw(&mut self, ask: Price) {
        self.ask = Some(ask);
    }

    /// Updates the price increment (tick size) for the matching core.
    pub const fn update_price_increment(&mut self, price_increment: Price) {
        self.price_increment = price_increment;
    }

    /// Clears all orders and resets bid/ask/last to uninitialized.
    pub fn reset(&mut self) {
        self.bid = None;
        self.ask = None;
        self.last = None;
        self.bid_limits.clear();
        self.ask_limits.clear();
        self.bid_stops.clear();
        self.ask_stops.clear();
        self.pending_bid.clear();
        self.pending_ask.clear();
        self.order_index.clear();
    }

    /// Adds an order to the matching core.
    ///
    /// # Invariant
    ///
    /// Each `client_order_id` must appear at most once across all books.
    /// To re-add an order under the same ID (e.g. a price-changing modify),
    /// call [`Self::delete_order`] first. Inserting duplicates puts two entries
    /// in the bucket and the order will match twice.
    ///
    /// Routing:
    /// - `is_stop()` orders go to the side's stop book, keyed by trigger price.
    /// - Orders with only a limit price go to the side's limit book, keyed by limit price.
    /// - Orders with neither price (e.g. `MARKET_TO_LIMIT` before conversion)
    ///   go to the per-side pending bucket. They remain visible to `get_order`
    ///   / `order_exists` but `iterate_*` skips them.
    ///
    /// # Panics
    ///
    /// Panics if the invariant is violated and debug assertions are enabled.
    pub fn add_order(&mut self, order: RestingOrder) {
        debug_assert!(
            !self.order_exists(order.client_order_id),
            "duplicate add_order for {}; caller must delete before re-adding",
            order.client_order_id,
        );

        let side = order.order_side;
        let client_order_id = order.client_order_id;
        let location = Self::locate(&order);

        if let Some((kind, price)) = location {
            let book = self.book_for_mut(side, kind);
            book.entry(price).or_default().push(order);
        } else {
            self.pending_for_mut(side).push(order);
        }
        self.order_index.insert(client_order_id, (side, location));
    }

    /// Deletes an order from the matching core by client order ID.
    ///
    /// # Errors
    ///
    /// Returns an [`OrderError::NotFound`] if the order is not present.
    ///
    /// # Panics
    ///
    /// Panics if the index points at a bucket that is missing or no longer
    /// contains the expected order, indicating internal index corruption.
    pub fn delete_order(&mut self, client_order_id: ClientOrderId) -> Result<(), OrderError> {
        let Some((side, location)) = self.order_index.remove(&client_order_id) else {
            return Err(OrderError::NotFound(client_order_id));
        };

        if let Some((kind, price)) = location {
            let book = self.book_for_mut(side, kind);
            let bucket = book
                .get_mut(&price)
                .expect("order_index points to existing bucket");
            let pos = bucket
                .iter()
                .position(|o| o.client_order_id == client_order_id)
                .expect("order_index points to existing slot");

            bucket.remove(pos);
            if bucket.is_empty() {
                book.remove(&price);
            }
        } else {
            let pending = self.pending_for_mut(side);
            let pos = pending
                .iter()
                .position(|o| o.client_order_id == client_order_id)
                .expect("order_index points to existing pending slot");
            pending.remove(pos);
        }
        Ok(())
    }

    /// Matches all bid then ask orders against the current market and returns
    /// the resulting actions in price-time priority.
    pub fn iterate(&self) -> Vec<MatchAction> {
        let mut actions = self.iterate_bids();
        actions.extend(self.iterate_asks());
        actions
    }

    /// Matches bid-side orders: limits best (highest) first, then stops
    /// nearest-trigger (lowest) first. FIFO within each price level.
    pub fn iterate_bids(&self) -> Vec<MatchAction> {
        self.iter_bid_book_orders()
            .filter_map(|order| self.match_order(order))
            .collect()
    }

    /// Matches ask-side orders: limits best (lowest) first, then stops
    /// nearest-trigger (highest) first. FIFO within each price level.
    pub fn iterate_asks(&self) -> Vec<MatchAction> {
        self.iter_ask_book_orders()
            .filter_map(|order| self.match_order(order))
            .collect()
    }

    /// Returns a [`MatchAction`] if the order matches the current market,
    /// or `None` if it does not (or has neither trigger nor limit price).
    pub fn match_order(&self, order: &RestingOrder) -> Option<MatchAction> {
        if order.is_stop() {
            self.match_stop_order(order)
        } else if order.is_limit() {
            self.match_limit_order(order)
        } else {
            None
        }
    }

    /// Returns whether a limit order at `price` would cross the opposite side
    /// (BUY: `ask <= price`, SELL: `bid >= price`).
    #[must_use]
    pub fn is_limit_matched(&self, side: OrderSide, price: Price) -> bool {
        match side {
            OrderSide::Buy => self.ask.is_some_and(|a| a <= price),
            OrderSide::Sell => self.bid.is_some_and(|b| b >= price),
        }
    }

    /// Returns whether a stop trigger at `price` has been reached
    /// (BUY: `ask >= price`, SELL: `bid <= price`).
    #[must_use]
    pub fn is_stop_matched(&self, side: OrderSide, price: Price) -> bool {
        self.is_stop_matched_with_trigger_type(side, price, TriggerType::BidAsk)
    }

    #[must_use]
    pub(crate) fn is_stop_matched_with_trigger_type(
        &self,
        side: OrderSide,
        price: Price,
        trigger_type: TriggerType,
    ) -> bool {
        self.market_price_for_trigger(side, trigger_type)
            .is_some_and(|market_price| match side {
                OrderSide::Buy => market_price >= price,
                OrderSide::Sell => market_price <= price,
            })
    }

    /// Returns whether a touch trigger at `trigger_price` has been reached
    /// (BUY: `ask <= trigger_price`, SELL: `bid >= trigger_price`).
    #[must_use]
    pub fn is_touch_triggered(&self, side: OrderSide, trigger_price: Price) -> bool {
        self.is_touch_triggered_with_trigger_type(side, trigger_price, TriggerType::BidAsk)
    }

    #[must_use]
    pub(crate) fn is_touch_triggered_with_trigger_type(
        &self,
        side: OrderSide,
        trigger_price: Price,
        trigger_type: TriggerType,
    ) -> bool {
        self.market_price_for_trigger(side, trigger_type)
            .is_some_and(|market_price| match side {
                OrderSide::Buy => market_price <= trigger_price,
                OrderSide::Sell => market_price >= trigger_price,
            })
    }

    /// Toggles whether limit orders fill at-or-inside the spread (vs only on cross).
    pub fn set_fill_limit_inside_spread(&mut self, value: bool) {
        self.fill_limit_inside_spread = value;
    }

    /// Returns whether a limit order is fillable at the given price.
    ///
    /// Checks `is_limit_matched` first (crosses the spread). When
    /// `fill_limit_inside_spread` is set, also checks at-or-inside spread
    /// (BUY >= bid, SELL <= ask), requiring both sides initialized.
    #[must_use]
    pub fn is_limit_fillable(&self, side: OrderSide, price: Price) -> bool {
        if self.is_limit_matched(side, price) {
            return true;
        }

        if !self.fill_limit_inside_spread {
            return false;
        }

        if let (Some(bid), Some(ask)) = (self.bid, self.ask) {
            match side {
                OrderSide::Buy => price >= bid,
                OrderSide::Sell => price <= ask,
            }
        } else {
            false
        }
    }

    fn book_for(&self, side: OrderSide, kind: BookKind) -> &BTreeMap<Price, OrderBucket> {
        match (side, kind) {
            (OrderSide::Buy, BookKind::Limit) => &self.bid_limits,
            (OrderSide::Buy, BookKind::Stop) => &self.bid_stops,
            (OrderSide::Sell, BookKind::Limit) => &self.ask_limits,
            (OrderSide::Sell, BookKind::Stop) => &self.ask_stops,
        }
    }

    fn book_for_mut(
        &mut self,
        side: OrderSide,
        kind: BookKind,
    ) -> &mut BTreeMap<Price, OrderBucket> {
        match (side, kind) {
            (OrderSide::Buy, BookKind::Limit) => &mut self.bid_limits,
            (OrderSide::Buy, BookKind::Stop) => &mut self.bid_stops,
            (OrderSide::Sell, BookKind::Limit) => &mut self.ask_limits,
            (OrderSide::Sell, BookKind::Stop) => &mut self.ask_stops,
        }
    }

    fn pending_for(&self, side: OrderSide) -> &[RestingOrder] {
        match side {
            OrderSide::Buy => &self.pending_bid,
            OrderSide::Sell => &self.pending_ask,
        }
    }

    fn pending_for_mut(&mut self, side: OrderSide) -> &mut SmallVec<[RestingOrder; 2]> {
        match side {
            OrderSide::Buy => &mut self.pending_bid,
            OrderSide::Sell => &mut self.pending_ask,
        }
    }

    fn iter_bid_book_orders(&self) -> impl Iterator<Item = &RestingOrder> {
        self.bid_limits
            .values()
            .rev()
            .flat_map(|bucket| bucket.iter())
            .chain(self.bid_stops.values().flat_map(|bucket| bucket.iter()))
    }

    fn iter_ask_book_orders(&self) -> impl Iterator<Item = &RestingOrder> {
        self.ask_limits
            .values()
            .flat_map(|bucket| bucket.iter())
            .chain(
                self.ask_stops
                    .values()
                    .rev()
                    .flat_map(|bucket| bucket.iter()),
            )
    }

    fn locate(order: &RestingOrder) -> Option<(BookKind, Price)> {
        if let Some(price) = order.trigger_price {
            Some((BookKind::Stop, price))
        } else {
            order.limit_price.map(|price| (BookKind::Limit, price))
        }
    }

    fn match_limit_order(&self, order: &RestingOrder) -> Option<MatchAction> {
        if let Some(limit_price) = order.limit_price
            && self.is_limit_fillable(order.order_side, limit_price)
        {
            Some(MatchAction::FillLimit(order.client_order_id))
        } else {
            None
        }
    }

    fn match_stop_order(&self, order: &RestingOrder) -> Option<MatchAction> {
        if !order.is_activated {
            return None;
        }

        let trigger_price = order.trigger_price?;

        let is_triggered = match order.order_type {
            OrderType::MarketIfTouched | OrderType::LimitIfTouched => self
                .is_touch_triggered_with_trigger_type(
                    order.order_side,
                    trigger_price,
                    order.trigger_type.unwrap_or(TriggerType::Default),
                ),
            _ => self.is_stop_matched_with_trigger_type(
                order.order_side,
                trigger_price,
                order.trigger_type.unwrap_or(TriggerType::Default),
            ),
        };

        if is_triggered {
            Some(MatchAction::TriggerStop(order.client_order_id))
        } else {
            None
        }
    }

    fn market_price_for_trigger(
        &self,
        side: OrderSide,
        trigger_type: TriggerType,
    ) -> Option<Price> {
        let quote_price = match side {
            OrderSide::Buy => self.ask,
            OrderSide::Sell => self.bid,
        };

        match trigger_type {
            TriggerType::LastPrice => self.last,
            TriggerType::LastOrBidAsk => self.last.or(quote_price),
            _ => quote_price,
        }
    }
}

/// Lightweight order information for matching/trigger checking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RestingOrder {
    pub client_order_id: ClientOrderId,
    pub order_side: OrderSide,
    pub order_type: OrderType,
    pub trigger_type: Option<TriggerType>,
    pub trigger_price: Option<Price>,
    pub limit_price: Option<Price>,
    pub is_activated: bool,
}

impl RestingOrder {
    /// Creates a new [`RestingOrder`] instance.
    ///
    /// `MARKET_TO_LIMIT` orders may legitimately be constructed with both
    /// `trigger_price` and `limit_price` set to `None` until they convert to
    /// a limit at execution time; [`OrderMatchingCore::match_order`] returns
    /// `None` for such orders.
    #[must_use]
    pub const fn new(
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        trigger_price: Option<Price>,
        limit_price: Option<Price>,
        is_activated: bool,
    ) -> Self {
        Self::new_with_trigger_type(
            client_order_id,
            order_side,
            order_type,
            match trigger_price {
                Some(_) => Some(TriggerType::Default),
                None => None,
            },
            trigger_price,
            limit_price,
            is_activated,
        )
    }

    #[must_use]
    pub(crate) const fn new_with_trigger_type(
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        trigger_type: Option<TriggerType>,
        trigger_price: Option<Price>,
        limit_price: Option<Price>,
        is_activated: bool,
    ) -> Self {
        Self {
            client_order_id,
            order_side,
            order_type,
            trigger_type,
            trigger_price,
            limit_price,
            is_activated,
        }
    }

    /// Returns whether a trigger price is set.
    #[must_use]
    pub const fn is_stop(&self) -> bool {
        self.trigger_price.is_some()
    }

    /// Returns whether a limit price is set without a trigger price.
    #[must_use]
    pub const fn is_limit(&self) -> bool {
        self.limit_price.is_some() && self.trigger_price.is_none()
    }
}

impl From<&PassiveOrderAny> for RestingOrder {
    fn from(order: &PassiveOrderAny) -> Self {
        match order {
            PassiveOrderAny::Limit(limit) => Self {
                client_order_id: limit.client_order_id(),
                order_side: limit.order_side(),
                order_type: limit.order_type(),
                trigger_type: None,
                trigger_price: None,
                limit_price: Some(limit.limit_px()),
                is_activated: true,
            },
            PassiveOrderAny::Stop(stop) => {
                let limit_price = match stop {
                    StopOrderAny::LimitIfTouched(o) => Some(o.price),
                    StopOrderAny::StopLimit(o) => Some(o.price),
                    StopOrderAny::TrailingStopLimit(o) => o.price,
                    StopOrderAny::MarketIfTouched(_)
                    | StopOrderAny::StopMarket(_)
                    | StopOrderAny::TrailingStopMarket(_) => None,
                };
                let is_activated = match stop {
                    StopOrderAny::TrailingStopMarket(o) => o.is_activated,
                    StopOrderAny::TrailingStopLimit(o) => o.is_activated,
                    _ => true,
                };
                Self {
                    client_order_id: stop.client_order_id(),
                    order_side: stop.order_side(),
                    order_type: stop.order_type(),
                    trigger_type: Some(stop.trigger_type().unwrap_or(TriggerType::Default)),
                    trigger_price: stop.stop_px(),
                    limit_price,
                    is_activated,
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BookKind {
    Limit,
    Stop,
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::{OrderSide, OrderType, TrailingOffsetType, TriggerType},
        events::{OrderEventAny, OrderInitialized, order::spec::OrderInitializedSpec},
        orders::{Order, OrderAny, builder::OrderTestBuilder},
        types::Quantity,
    };
    use rstest::rstest;
    use rust_decimal::Decimal;

    use super::*;

    fn create_matching_core(
        instrument_id: InstrumentId,
        price_increment: Price,
    ) -> OrderMatchingCore {
        OrderMatchingCore::new(instrument_id, price_increment)
    }

    #[rstest]
    fn test_add_order_bid_side() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut matching_core = create_matching_core(instrument_id, Price::from("0.01"));

        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_id)
            .side(OrderSide::Buy)
            .price(Price::from("100.00"))
            .quantity(Quantity::from("100"))
            .build();

        let match_info = RestingOrder::from(&PassiveOrderAny::try_from(order).unwrap());
        matching_core.add_order(match_info);

        assert!(matching_core.get_orders_bid().contains(&match_info));
        assert!(!matching_core.get_orders_ask().contains(&match_info));
        assert_eq!(matching_core.get_orders_bid().len(), 1);
        assert!(matching_core.get_orders_ask().is_empty());
        assert!(matching_core.order_exists(match_info.client_order_id));
    }

    #[rstest]
    fn test_add_order_ask_side() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut matching_core = create_matching_core(instrument_id, Price::from("0.01"));

        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_id)
            .side(OrderSide::Sell)
            .price(Price::from("100.00"))
            .quantity(Quantity::from("100"))
            .build();

        let match_info = RestingOrder::from(&PassiveOrderAny::try_from(order).unwrap());
        matching_core.add_order(match_info);

        assert!(matching_core.get_orders_ask().contains(&match_info));
        assert!(!matching_core.get_orders_bid().contains(&match_info));
        assert_eq!(matching_core.get_orders_ask().len(), 1);
        assert!(matching_core.get_orders_bid().is_empty());
        assert!(matching_core.order_exists(match_info.client_order_id));
    }

    #[rstest]
    fn test_reset() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut matching_core = create_matching_core(instrument_id, Price::from("0.01"));

        let orders = [
            limit_order(OrderSide::Buy, "110.00", "O-B-LMT"),
            stop_order(OrderSide::Buy, "101.00", "O-B-STP"),
            pending_order(OrderSide::Buy, "O-B-PENDING"),
            limit_order(OrderSide::Sell, "90.00", "O-A-LMT"),
            stop_order(OrderSide::Sell, "99.00", "O-A-STP"),
            pending_order(OrderSide::Sell, "O-A-PENDING"),
        ];

        for order in orders {
            matching_core.add_order(order);
        }
        matching_core.set_bid_raw(Price::from("94.00"));
        matching_core.set_ask_raw(Price::from("106.00"));
        matching_core.set_last_raw(Price::from("100.00"));

        assert_eq!(matching_core.get_orders(), orders);

        matching_core.reset();

        assert!(matching_core.bid.is_none());
        assert!(matching_core.ask.is_none());
        assert!(matching_core.last.is_none());
        assert!(matching_core.get_orders_bid().is_empty());
        assert!(matching_core.get_orders_ask().is_empty());
        assert!(matching_core.get_orders().is_empty());
        for order in orders {
            assert_eq!(matching_core.get_order(order.client_order_id), None);
            assert!(!matching_core.order_exists(order.client_order_id));
        }

        matching_core.set_bid_raw(Price::from("94.00"));
        matching_core.set_ask_raw(Price::from("106.00"));

        assert!(matching_core.iterate().is_empty());

        for order in orders {
            matching_core.add_order(order);
        }

        assert_eq!(matching_core.get_orders(), orders);
    }

    #[rstest]
    fn test_delete_order_when_not_exists() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut matching_core = create_matching_core(instrument_id, Price::from("0.01"));

        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_id)
            .side(OrderSide::Buy)
            .price(Price::from("100.00"))
            .quantity(Quantity::from("100"))
            .build();

        let result = matching_core.delete_order(order.client_order_id());
        assert!(result.is_err());
    }

    #[rstest]
    #[case(OrderSide::Buy)]
    #[case(OrderSide::Sell)]
    fn test_delete_order_when_exists(#[case] order_side: OrderSide) {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut matching_core = create_matching_core(instrument_id, Price::from("0.01"));

        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_id)
            .side(order_side)
            .price(Price::from("100.00"))
            .quantity(Quantity::from("100"))
            .build();

        let client_order_id = order.client_order_id();
        let match_info = RestingOrder::from(&PassiveOrderAny::try_from(order).unwrap());
        matching_core.add_order(match_info);
        matching_core.delete_order(client_order_id).unwrap();

        assert!(matching_core.get_orders_ask().is_empty());
        assert!(matching_core.get_orders_bid().is_empty());
    }

    #[rstest]
    #[case(None, None, Price::from("100.00"), OrderSide::Buy, false)]
    #[case(None, None, Price::from("100.00"), OrderSide::Sell, false)]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("100.00"),
        OrderSide::Buy,
        false
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("101.00"),
        OrderSide::Buy,
        true
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("102.00"),
        OrderSide::Buy,
        true
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("101.00"),
        OrderSide::Sell,
        false
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("100.00"),
        OrderSide::Sell,
        true
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("99.00"),
        OrderSide::Sell,
        true
    )]
    fn test_is_limit_matched(
        #[case] bid: Option<Price>,
        #[case] ask: Option<Price>,
        #[case] price: Price,
        #[case] order_side: OrderSide,
        #[case] expected: bool,
    ) {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut matching_core = create_matching_core(instrument_id, Price::from("0.01"));
        matching_core.bid = bid;
        matching_core.ask = ask;

        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_id)
            .side(order_side)
            .price(price)
            .quantity(Quantity::from("100"))
            .build();

        let result = matching_core.is_limit_matched(order.order_side(), order.price().unwrap());
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(None, None, Price::from("100.00"), OrderSide::Buy, false)]
    #[case(None, None, Price::from("100.00"), OrderSide::Sell, false)]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("102.00"),
        OrderSide::Buy,
        false
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("101.00"),
        OrderSide::Buy,
        true
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("100.00"),
        OrderSide::Buy,
        true
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("99.00"),
        OrderSide::Sell,
        false
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("100.00"),
        OrderSide::Sell,
        true
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("101.00"),
        OrderSide::Sell,
        true
    )]
    fn test_is_stop_matched(
        #[case] bid: Option<Price>,
        #[case] ask: Option<Price>,
        #[case] trigger_price: Price,
        #[case] order_side: OrderSide,
        #[case] expected: bool,
    ) {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut matching_core = create_matching_core(instrument_id, Price::from("0.01"));
        matching_core.bid = bid;
        matching_core.ask = ask;

        let order = OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(instrument_id)
            .side(order_side)
            .trigger_price(trigger_price)
            .quantity(Quantity::from("100"))
            .build();

        let result =
            matching_core.is_stop_matched(order.order_side(), order.trigger_price().unwrap());
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case::last_price_below_trigger(Some(Price::from("99.00")), TriggerType::LastPrice, false)]
    #[case::last_price_at_trigger(Some(Price::from("100.00")), TriggerType::LastPrice, true)]
    #[case::last_price_unavailable(None, TriggerType::LastPrice, false)]
    #[case::last_or_bid_ask_prefers_last(
        Some(Price::from("99.00")),
        TriggerType::LastOrBidAsk,
        false
    )]
    #[case::last_or_bid_ask_falls_back_to_quote(None, TriggerType::LastOrBidAsk, true)]
    #[case::bid_ask_uses_quote(Some(Price::from("99.00")), TriggerType::BidAsk, true)]
    fn test_is_stop_matched_uses_trigger_type(
        #[case] last: Option<Price>,
        #[case] trigger_type: TriggerType,
        #[case] expected: bool,
    ) {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut matching_core = create_matching_core(instrument_id, Price::from("0.01"));
        matching_core.ask = Some(Price::from("101.00"));
        matching_core.last = last;

        let result = matching_core.is_stop_matched_with_trigger_type(
            OrderSide::Buy,
            Price::from("100.00"),
            trigger_type,
        );

        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_iterate_returns_empty_when_no_orders() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut matching_core = create_matching_core(instrument_id, Price::from("0.01"));
        matching_core.set_bid_raw(Price::from("100.00"));
        matching_core.set_ask_raw(Price::from("101.00"));

        let actions = matching_core.iterate();

        assert!(actions.is_empty());
    }

    #[rstest]
    fn test_iterate_returns_empty_when_no_market_data() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut matching_core = create_matching_core(instrument_id, Price::from("0.01"));

        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_id)
            .side(OrderSide::Buy)
            .price(Price::from("100.00"))
            .quantity(Quantity::from("100"))
            .build();
        let match_info = RestingOrder::from(&PassiveOrderAny::try_from(order).unwrap());
        matching_core.add_order(match_info);

        let actions = matching_core.iterate();

        assert!(actions.is_empty());
    }

    #[rstest]
    fn test_iterate_returns_fill_limit_for_matched_buy() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut matching_core = create_matching_core(instrument_id, Price::from("0.01"));
        matching_core.set_ask_raw(Price::from("100.00"));

        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_id)
            .side(OrderSide::Buy)
            .price(Price::from("100.00"))
            .quantity(Quantity::from("100"))
            .build();
        let client_order_id = order.client_order_id();
        let match_info = RestingOrder::from(&PassiveOrderAny::try_from(order).unwrap());
        matching_core.add_order(match_info);

        let actions = matching_core.iterate();

        assert_eq!(actions, vec![MatchAction::FillLimit(client_order_id)]);
    }

    #[rstest]
    fn test_iterate_returns_fill_limit_for_matched_sell() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut matching_core = create_matching_core(instrument_id, Price::from("0.01"));
        matching_core.set_bid_raw(Price::from("100.00"));

        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_id)
            .side(OrderSide::Sell)
            .price(Price::from("100.00"))
            .quantity(Quantity::from("100"))
            .build();
        let client_order_id = order.client_order_id();
        let match_info = RestingOrder::from(&PassiveOrderAny::try_from(order).unwrap());
        matching_core.add_order(match_info);

        let actions = matching_core.iterate();

        assert_eq!(actions, vec![MatchAction::FillLimit(client_order_id)]);
    }

    #[rstest]
    fn test_iterate_returns_no_fill_for_unmatched_limit() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut matching_core = create_matching_core(instrument_id, Price::from("0.01"));
        matching_core.set_ask_raw(Price::from("101.00"));

        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_id)
            .side(OrderSide::Buy)
            .price(Price::from("100.00"))
            .quantity(Quantity::from("100"))
            .build();
        let match_info = RestingOrder::from(&PassiveOrderAny::try_from(order).unwrap());
        matching_core.add_order(match_info);

        let actions = matching_core.iterate();

        assert!(actions.is_empty());
    }

    #[rstest]
    fn test_iterate_returns_trigger_stop_for_matched_buy() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut matching_core = create_matching_core(instrument_id, Price::from("0.01"));
        matching_core.set_ask_raw(Price::from("101.00"));

        let order = OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(instrument_id)
            .side(OrderSide::Buy)
            .trigger_price(Price::from("101.00"))
            .trigger_type(TriggerType::Default)
            .quantity(Quantity::from("100"))
            .build();
        let client_order_id = order.client_order_id();
        let match_info = RestingOrder::from(&PassiveOrderAny::try_from(order).unwrap());
        matching_core.add_order(match_info);

        let actions = matching_core.iterate();

        assert_eq!(actions, vec![MatchAction::TriggerStop(client_order_id)]);
    }

    #[rstest]
    fn test_iterate_returns_trigger_stop_for_matched_sell() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut matching_core = create_matching_core(instrument_id, Price::from("0.01"));
        matching_core.set_bid_raw(Price::from("99.00"));

        let order = OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(instrument_id)
            .side(OrderSide::Sell)
            .trigger_price(Price::from("99.00"))
            .quantity(Quantity::from("100"))
            .build();
        let client_order_id = order.client_order_id();
        let match_info = RestingOrder::from(&PassiveOrderAny::try_from(order).unwrap());
        matching_core.add_order(match_info);

        let actions = matching_core.iterate();

        assert_eq!(actions, vec![MatchAction::TriggerStop(client_order_id)]);
    }

    #[rstest]
    fn test_iterate_skips_unactivated_stop_order() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut matching_core = create_matching_core(instrument_id, Price::from("0.01"));
        matching_core.set_ask_raw(Price::from("110.00"));

        let match_info = RestingOrder::new(
            ClientOrderId::from("O-001"),
            OrderSide::Buy,
            OrderType::TrailingStopMarket,
            Some(Price::from("105.00")),
            None,
            false,
        );
        matching_core.add_order(match_info);

        let actions = matching_core.iterate();

        assert!(actions.is_empty());
    }

    #[rstest]
    fn test_iterate_triggers_activated_stop_order() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut matching_core = create_matching_core(instrument_id, Price::from("0.01"));
        matching_core.set_ask_raw(Price::from("110.00"));

        let client_order_id = ClientOrderId::from("O-001");
        let match_info = RestingOrder::new(
            client_order_id,
            OrderSide::Buy,
            OrderType::TrailingStopMarket,
            Some(Price::from("105.00")),
            None,
            true,
        );
        matching_core.add_order(match_info);

        let actions = matching_core.iterate();

        assert_eq!(actions, vec![MatchAction::TriggerStop(client_order_id)]);
    }

    #[rstest]
    fn test_iterate_returns_mixed_actions_for_limits_and_stops() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut matching_core = create_matching_core(instrument_id, Price::from("0.01"));
        matching_core.set_bid_raw(Price::from("99.00"));
        matching_core.set_ask_raw(Price::from("101.00"));

        let buy_limit = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_id)
            .side(OrderSide::Buy)
            .price(Price::from("101.00"))
            .quantity(Quantity::from("100"))
            .client_order_id(ClientOrderId::from("O-BUY-LIMIT"))
            .build();
        let buy_limit_id = buy_limit.client_order_id();
        matching_core.add_order(RestingOrder::from(
            &PassiveOrderAny::try_from(buy_limit).unwrap(),
        ));

        let sell_stop = OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(instrument_id)
            .side(OrderSide::Sell)
            .trigger_price(Price::from("99.00"))
            .quantity(Quantity::from("50"))
            .client_order_id(ClientOrderId::from("O-SELL-STOP"))
            .build();
        let sell_stop_id = sell_stop.client_order_id();
        matching_core.add_order(RestingOrder::from(
            &PassiveOrderAny::try_from(sell_stop).unwrap(),
        ));

        let actions = matching_core.iterate();

        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0], MatchAction::FillLimit(buy_limit_id));
        assert_eq!(actions[1], MatchAction::TriggerStop(sell_stop_id));
    }

    #[rstest]
    fn test_is_limit_fillable_delegates_to_is_limit_matched_by_default() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut core = create_matching_core(instrument_id, Price::from("0.01"));
        core.set_bid_raw(Price::from("100.00"));
        core.set_ask_raw(Price::from("101.00"));

        assert!(core.is_limit_fillable(OrderSide::Buy, Price::from("101.00")));
        assert!(!core.is_limit_fillable(OrderSide::Buy, Price::from("100.00")));
        assert!(core.is_limit_fillable(OrderSide::Sell, Price::from("100.00")));
        assert!(!core.is_limit_fillable(OrderSide::Sell, Price::from("101.00")));
    }

    #[rstest]
    fn test_is_limit_fillable_inside_spread_buy_at_bid() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut core = create_matching_core(instrument_id, Price::from("0.01"));
        core.set_bid_raw(Price::from("100.00"));
        core.set_ask_raw(Price::from("101.00"));
        core.set_fill_limit_inside_spread(true);

        assert!(core.is_limit_fillable(OrderSide::Buy, Price::from("100.00")));
        assert!(core.is_limit_fillable(OrderSide::Buy, Price::from("100.50")));
        assert!(!core.is_limit_fillable(OrderSide::Buy, Price::from("99.00")));
    }

    #[rstest]
    fn test_is_limit_fillable_inside_spread_sell_at_ask() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut core = create_matching_core(instrument_id, Price::from("0.01"));
        core.set_bid_raw(Price::from("100.00"));
        core.set_ask_raw(Price::from("101.00"));
        core.set_fill_limit_inside_spread(true);

        assert!(core.is_limit_fillable(OrderSide::Sell, Price::from("101.00")));
        assert!(core.is_limit_fillable(OrderSide::Sell, Price::from("100.50")));
        assert!(!core.is_limit_fillable(OrderSide::Sell, Price::from("102.00")));
    }

    #[rstest]
    fn test_is_limit_fillable_inside_spread_requires_both_quotes_present() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut core = create_matching_core(instrument_id, Price::from("0.01"));
        core.set_fill_limit_inside_spread(true);

        core.set_bid_raw(Price::from("100.00"));
        assert!(!core.is_limit_fillable(OrderSide::Buy, Price::from("100.00")));

        let mut core2 = create_matching_core(instrument_id, Price::from("0.01"));
        core2.set_fill_limit_inside_spread(true);
        core2.set_ask_raw(Price::from("101.00"));
        assert!(!core2.is_limit_fillable(OrderSide::Sell, Price::from("101.00")));

        let mut core3 = create_matching_core(instrument_id, Price::from("0.01"));
        core3.set_fill_limit_inside_spread(true);
        core3.set_bid_raw(Price::from("100.00"));
        core3.set_ask_raw(Price::from("101.00"));
        core3.ask = None;
        assert!(!core3.is_limit_fillable(OrderSide::Buy, Price::from("100.00")));
    }

    #[rstest]
    fn test_iterate_fills_limit_inside_spread_when_enabled() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut core = create_matching_core(instrument_id, Price::from("0.01"));
        core.set_bid_raw(Price::from("100.00"));
        core.set_ask_raw(Price::from("101.00"));
        core.set_fill_limit_inside_spread(true);

        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_id)
            .side(OrderSide::Buy)
            .price(Price::from("100.00"))
            .quantity(Quantity::from("100"))
            .build();
        let client_order_id = order.client_order_id();
        let match_info = RestingOrder::from(&PassiveOrderAny::try_from(order).unwrap());
        core.add_order(match_info);

        let actions = core.iterate();
        assert_eq!(actions, vec![MatchAction::FillLimit(client_order_id)]);
    }

    #[rstest]
    #[case(None, None, Price::from("100.00"), OrderSide::Buy, false)]
    #[case(None, None, Price::from("100.00"), OrderSide::Sell, false)]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("102.00"),
        OrderSide::Buy,
        true
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("101.00"),
        OrderSide::Buy,
        true
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("100.00"),
        OrderSide::Buy,
        false
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("99.00"),
        OrderSide::Sell,
        true
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("100.00"),
        OrderSide::Sell,
        true
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("101.00"),
        OrderSide::Sell,
        false
    )]
    fn test_is_touch_triggered(
        #[case] bid: Option<Price>,
        #[case] ask: Option<Price>,
        #[case] trigger_price: Price,
        #[case] order_side: OrderSide,
        #[case] expected: bool,
    ) {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut matching_core = create_matching_core(instrument_id, Price::from("0.01"));
        matching_core.bid = bid;
        matching_core.ask = ask;

        let result = matching_core.is_touch_triggered(order_side, trigger_price);
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_update_price_increment_updates_increment_and_precision() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut matching_core = create_matching_core(instrument_id, Price::from("0.01"));

        assert_eq!(matching_core.price_increment, Price::from("0.01"));
        assert_eq!(matching_core.price_precision(), 2);

        matching_core.update_price_increment(Price::from("0.001"));

        assert_eq!(matching_core.price_increment, Price::from("0.001"));
        assert_eq!(matching_core.price_precision(), 3);
    }

    fn order_from_init(spec: OrderInitialized) -> OrderAny {
        OrderAny::from_events(vec![OrderEventAny::Initialized(spec)]).unwrap()
    }

    #[rstest]
    fn test_get_order_finds_orders_on_either_side() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut core = create_matching_core(instrument_id, Price::from("0.01"));

        let buy = order_from_init(
            OrderInitializedSpec::builder()
                .instrument_id(instrument_id)
                .client_order_id(ClientOrderId::from("O-BUY"))
                .order_side(OrderSide::Buy)
                .order_type(OrderType::Limit)
                .quantity(Quantity::from("10"))
                .price(Price::from("100.00"))
                .build(),
        );
        let buy_id = buy.client_order_id();
        core.add_order(RestingOrder::from(&PassiveOrderAny::try_from(buy).unwrap()));

        let sell = order_from_init(
            OrderInitializedSpec::builder()
                .instrument_id(instrument_id)
                .client_order_id(ClientOrderId::from("O-SELL"))
                .order_side(OrderSide::Sell)
                .order_type(OrderType::Limit)
                .quantity(Quantity::from("10"))
                .price(Price::from("101.00"))
                .build(),
        );
        let sell_id = sell.client_order_id();
        core.add_order(RestingOrder::from(
            &PassiveOrderAny::try_from(sell).unwrap(),
        ));

        assert_eq!(
            core.get_order(buy_id).map(|o| o.client_order_id),
            Some(buy_id)
        );
        assert_eq!(
            core.get_order(sell_id).map(|o| o.client_order_id),
            Some(sell_id)
        );
        assert!(core.get_order(ClientOrderId::from("O-MISSING")).is_none());
    }

    #[rstest]
    fn test_match_order_returns_none_when_neither_price_set() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut core = create_matching_core(instrument_id, Price::from("0.01"));
        core.set_bid_raw(Price::from("100.00"));
        core.set_ask_raw(Price::from("101.00"));

        let info = RestingOrder::new(
            ClientOrderId::from("O-NEITHER"),
            OrderSide::Buy,
            OrderType::MarketToLimit,
            None,
            None,
            true,
        );
        assert!(core.match_order(&info).is_none());
    }

    #[rstest]
    fn test_from_passive_order_extracts_limit_price_for_stop_limit() {
        let order = order_from_init(
            OrderInitializedSpec::builder()
                .order_type(OrderType::StopLimit)
                .order_side(OrderSide::Buy)
                .quantity(Quantity::from("10"))
                .price(Price::from("101.00"))
                .trigger_price(Price::from("100.00"))
                .trigger_type(TriggerType::Default)
                .build(),
        );

        let info = RestingOrder::from(&PassiveOrderAny::try_from(order).unwrap());

        assert_eq!(info.trigger_price, Some(Price::from("100.00")));
        assert_eq!(info.limit_price, Some(Price::from("101.00")));
        assert!(info.is_activated);
    }

    #[rstest]
    fn test_from_passive_order_extracts_limit_price_for_limit_if_touched() {
        let order = order_from_init(
            OrderInitializedSpec::builder()
                .order_type(OrderType::LimitIfTouched)
                .order_side(OrderSide::Sell)
                .quantity(Quantity::from("10"))
                .price(Price::from("99.00"))
                .trigger_price(Price::from("100.00"))
                .trigger_type(TriggerType::LastPrice)
                .build(),
        );

        let info = RestingOrder::from(&PassiveOrderAny::try_from(order).unwrap());

        assert_eq!(info.trigger_price, Some(Price::from("100.00")));
        assert_eq!(info.limit_price, Some(Price::from("99.00")));
        assert_eq!(info.trigger_type, Some(TriggerType::LastPrice));
        assert!(info.is_activated);
    }

    #[rstest]
    fn test_from_passive_order_extracts_is_activated_for_trailing_stop_market() {
        let order = order_from_init(
            OrderInitializedSpec::builder()
                .order_type(OrderType::TrailingStopMarket)
                .order_side(OrderSide::Buy)
                .quantity(Quantity::from("10"))
                .trigger_price(Price::from("101.00"))
                .trigger_type(TriggerType::Default)
                .trailing_offset(Decimal::from(1))
                .trailing_offset_type(TrailingOffsetType::Price)
                .build(),
        );

        let info = RestingOrder::from(&PassiveOrderAny::try_from(order).unwrap());

        assert_eq!(info.trigger_price, Some(Price::from("101.00")));
        assert_eq!(info.limit_price, None);
        assert!(!info.is_activated);
    }

    #[rstest]
    fn test_from_passive_order_extracts_limit_and_is_activated_for_trailing_stop_limit() {
        let order = order_from_init(
            OrderInitializedSpec::builder()
                .order_type(OrderType::TrailingStopLimit)
                .order_side(OrderSide::Sell)
                .quantity(Quantity::from("10"))
                .price(Price::from("99.00"))
                .trigger_price(Price::from("100.00"))
                .trigger_type(TriggerType::Default)
                .limit_offset(Decimal::from(1))
                .trailing_offset(Decimal::from(1))
                .trailing_offset_type(TrailingOffsetType::Price)
                .build(),
        );

        let info = RestingOrder::from(&PassiveOrderAny::try_from(order).unwrap());

        assert_eq!(info.trigger_price, Some(Price::from("100.00")));
        assert_eq!(info.limit_price, Some(Price::from("99.00")));
        assert!(!info.is_activated);
    }

    fn limit_order(side: OrderSide, price: &str, id: &str) -> RestingOrder {
        let order = order_from_init(
            OrderInitializedSpec::builder()
                .client_order_id(ClientOrderId::from(id))
                .order_type(OrderType::Limit)
                .order_side(side)
                .quantity(Quantity::from("10"))
                .price(Price::from(price))
                .build(),
        );
        RestingOrder::from(&PassiveOrderAny::try_from(order).unwrap())
    }

    fn stop_order(side: OrderSide, trigger: &str, id: &str) -> RestingOrder {
        let order = order_from_init(
            OrderInitializedSpec::builder()
                .client_order_id(ClientOrderId::from(id))
                .order_type(OrderType::StopMarket)
                .order_side(side)
                .quantity(Quantity::from("10"))
                .trigger_price(Price::from(trigger))
                .trigger_type(TriggerType::Default)
                .build(),
        );
        RestingOrder::from(&PassiveOrderAny::try_from(order).unwrap())
    }

    fn stop_limit_order(side: OrderSide, trigger: &str, limit: &str, id: &str) -> RestingOrder {
        let order = order_from_init(
            OrderInitializedSpec::builder()
                .client_order_id(ClientOrderId::from(id))
                .order_type(OrderType::StopLimit)
                .order_side(side)
                .quantity(Quantity::from("10"))
                .price(Price::from(limit))
                .trigger_price(Price::from(trigger))
                .trigger_type(TriggerType::Default)
                .build(),
        );
        RestingOrder::from(&PassiveOrderAny::try_from(order).unwrap())
    }

    #[rstest]
    fn test_iterate_bids_returns_limits_in_descending_price_order() {
        let mut core = create_matching_core(InstrumentId::from("AAPL.XNAS"), Price::from("0.01"));
        core.set_ask_raw(Price::from("99.00"));

        core.add_order(limit_order(OrderSide::Buy, "100.00", "O-MID"));
        core.add_order(limit_order(OrderSide::Buy, "100.50", "O-HIGH"));
        core.add_order(limit_order(OrderSide::Buy, "99.50", "O-LOW"));

        let actions = core.iterate_bids();
        assert_eq!(
            actions,
            vec![
                MatchAction::FillLimit(ClientOrderId::from("O-HIGH")),
                MatchAction::FillLimit(ClientOrderId::from("O-MID")),
                MatchAction::FillLimit(ClientOrderId::from("O-LOW")),
            ],
        );
    }

    #[rstest]
    fn test_iterate_asks_returns_limits_in_ascending_price_order() {
        let mut core = create_matching_core(InstrumentId::from("AAPL.XNAS"), Price::from("0.01"));
        core.set_bid_raw(Price::from("101.00"));

        core.add_order(limit_order(OrderSide::Sell, "100.50", "O-MID"));
        core.add_order(limit_order(OrderSide::Sell, "100.00", "O-LOW"));
        core.add_order(limit_order(OrderSide::Sell, "100.75", "O-HIGH"));

        let actions = core.iterate_asks();
        assert_eq!(
            actions,
            vec![
                MatchAction::FillLimit(ClientOrderId::from("O-LOW")),
                MatchAction::FillLimit(ClientOrderId::from("O-MID")),
                MatchAction::FillLimit(ClientOrderId::from("O-HIGH")),
            ],
        );
    }

    #[rstest]
    fn test_iterate_limits_preserves_fifo_within_same_price() {
        let mut core = create_matching_core(InstrumentId::from("AAPL.XNAS"), Price::from("0.01"));
        core.set_ask_raw(Price::from("99.00"));

        for id in ["O-1", "O-2", "O-3", "O-4"] {
            core.add_order(limit_order(OrderSide::Buy, "100.00", id));
        }

        let actions = core.iterate_bids();
        assert_eq!(
            actions,
            vec![
                MatchAction::FillLimit(ClientOrderId::from("O-1")),
                MatchAction::FillLimit(ClientOrderId::from("O-2")),
                MatchAction::FillLimit(ClientOrderId::from("O-3")),
                MatchAction::FillLimit(ClientOrderId::from("O-4")),
            ],
        );
    }

    #[rstest]
    fn test_buy_stops_trigger_in_ascending_price_order_when_ask_crosses_multiple() {
        let mut core = create_matching_core(InstrumentId::from("AAPL.XNAS"), Price::from("0.01"));
        core.set_ask_raw(Price::from("106.00"));

        core.add_order(stop_order(OrderSide::Buy, "105.00", "O-FAR"));
        core.add_order(stop_order(OrderSide::Buy, "101.00", "O-NEAR"));

        let actions = core.iterate_bids();
        assert_eq!(
            actions,
            vec![
                MatchAction::TriggerStop(ClientOrderId::from("O-NEAR")),
                MatchAction::TriggerStop(ClientOrderId::from("O-FAR")),
            ],
        );
    }

    #[rstest]
    fn test_sell_stops_trigger_in_descending_price_order_when_bid_crosses_multiple() {
        let mut core = create_matching_core(InstrumentId::from("AAPL.XNAS"), Price::from("0.01"));
        core.set_bid_raw(Price::from("94.00"));

        core.add_order(stop_order(OrderSide::Sell, "95.00", "O-FAR"));
        core.add_order(stop_order(OrderSide::Sell, "99.00", "O-NEAR"));

        let actions = core.iterate_asks();
        assert_eq!(
            actions,
            vec![
                MatchAction::TriggerStop(ClientOrderId::from("O-NEAR")),
                MatchAction::TriggerStop(ClientOrderId::from("O-FAR")),
            ],
        );
    }

    #[rstest]
    fn test_iterate_stops_preserves_fifo_within_same_trigger() {
        let mut core = create_matching_core(InstrumentId::from("AAPL.XNAS"), Price::from("0.01"));
        core.set_ask_raw(Price::from("106.00"));

        for id in ["O-S1", "O-S2", "O-S3"] {
            core.add_order(stop_order(OrderSide::Buy, "101.00", id));
        }

        let actions = core.iterate_bids();
        assert_eq!(
            actions,
            vec![
                MatchAction::TriggerStop(ClientOrderId::from("O-S1")),
                MatchAction::TriggerStop(ClientOrderId::from("O-S2")),
                MatchAction::TriggerStop(ClientOrderId::from("O-S3")),
            ],
        );
    }

    #[rstest]
    fn test_iterate_bids_processes_limits_before_stops() {
        let mut core = create_matching_core(InstrumentId::from("AAPL.XNAS"), Price::from("0.01"));
        core.set_ask_raw(Price::from("106.00"));

        core.add_order(limit_order(OrderSide::Buy, "110.00", "O-LMT"));
        core.add_order(stop_order(OrderSide::Buy, "101.00", "O-STP"));

        let actions = core.iterate_bids();
        assert_eq!(
            actions,
            vec![
                MatchAction::FillLimit(ClientOrderId::from("O-LMT")),
                MatchAction::TriggerStop(ClientOrderId::from("O-STP")),
            ],
        );
    }

    #[rstest]
    fn test_iterate_asks_processes_limits_before_stops() {
        let mut core = create_matching_core(InstrumentId::from("AAPL.XNAS"), Price::from("0.01"));
        core.set_bid_raw(Price::from("94.00"));

        core.add_order(limit_order(OrderSide::Sell, "90.00", "O-LMT"));
        core.add_order(stop_order(OrderSide::Sell, "99.00", "O-STP"));

        let actions = core.iterate_asks();
        assert_eq!(
            actions,
            vec![
                MatchAction::FillLimit(ClientOrderId::from("O-LMT")),
                MatchAction::TriggerStop(ClientOrderId::from("O-STP")),
            ],
        );
    }

    #[rstest]
    fn test_stop_limit_routed_to_stop_book_keyed_by_trigger() {
        let mut core = create_matching_core(InstrumentId::from("AAPL.XNAS"), Price::from("0.01"));
        core.set_ask_raw(Price::from("106.00"));

        // Equal limit prices ensure that trigger prices determine priority
        core.add_order(stop_limit_order(
            OrderSide::Buy,
            "105.00",
            "110.00",
            "O-FAR",
        ));
        core.add_order(stop_limit_order(
            OrderSide::Buy,
            "101.00",
            "110.00",
            "O-NEAR",
        ));

        let actions = core.iterate_bids();
        assert_eq!(
            actions,
            vec![
                MatchAction::TriggerStop(ClientOrderId::from("O-NEAR")),
                MatchAction::TriggerStop(ClientOrderId::from("O-FAR")),
            ],
        );
    }

    #[rstest]
    fn test_iterate_full_walk_combines_bids_then_asks_each_with_limits_then_stops() {
        // Prices make limits and stops matchable on both sides in the same snapshot
        let mut core = create_matching_core(InstrumentId::from("AAPL.XNAS"), Price::from("0.01"));
        core.set_bid_raw(Price::from("94.00"));
        core.set_ask_raw(Price::from("106.00"));

        core.add_order(limit_order(OrderSide::Buy, "110.00", "O-B-LMT-HIGH"));
        core.add_order(limit_order(OrderSide::Buy, "107.00", "O-B-LMT-LOW"));
        core.add_order(stop_order(OrderSide::Buy, "105.00", "O-B-STP-FAR"));
        core.add_order(stop_order(OrderSide::Buy, "101.00", "O-B-STP-NEAR"));

        core.add_order(limit_order(OrderSide::Sell, "90.00", "O-A-LMT-LOW"));
        core.add_order(limit_order(OrderSide::Sell, "93.00", "O-A-LMT-HIGH"));
        core.add_order(stop_order(OrderSide::Sell, "95.00", "O-A-STP-FAR"));
        core.add_order(stop_order(OrderSide::Sell, "99.00", "O-A-STP-NEAR"));

        let actions = core.iterate();
        assert_eq!(
            actions,
            vec![
                MatchAction::FillLimit(ClientOrderId::from("O-B-LMT-HIGH")),
                MatchAction::FillLimit(ClientOrderId::from("O-B-LMT-LOW")),
                MatchAction::TriggerStop(ClientOrderId::from("O-B-STP-NEAR")),
                MatchAction::TriggerStop(ClientOrderId::from("O-B-STP-FAR")),
                MatchAction::FillLimit(ClientOrderId::from("O-A-LMT-LOW")),
                MatchAction::FillLimit(ClientOrderId::from("O-A-LMT-HIGH")),
                MatchAction::TriggerStop(ClientOrderId::from("O-A-STP-NEAR")),
                MatchAction::TriggerStop(ClientOrderId::from("O-A-STP-FAR")),
            ],
        );
    }

    #[rstest]
    fn test_pending_orders_skipped_in_iterate_but_visible_in_get_orders() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut core = create_matching_core(instrument_id, Price::from("0.01"));
        core.set_bid_raw(Price::from("99.00"));
        core.set_ask_raw(Price::from("100.00"));

        core.add_order(limit_order(OrderSide::Buy, "100.00", "O-LMT"));

        let pending = RestingOrder::new(
            ClientOrderId::from("O-PENDING"),
            OrderSide::Buy,
            OrderType::MarketToLimit,
            None,
            None,
            true,
        );
        core.add_order(pending);

        assert_eq!(
            core.iterate_bids(),
            vec![MatchAction::FillLimit(ClientOrderId::from("O-LMT"))],
        );

        let bid_ids: Vec<_> = core
            .get_orders_bid()
            .iter()
            .map(|o| o.client_order_id)
            .collect();
        assert_eq!(
            bid_ids,
            vec![
                ClientOrderId::from("O-LMT"),
                ClientOrderId::from("O-PENDING"),
            ],
        );
    }

    #[rstest]
    fn test_modify_then_readd_moves_order_to_back_of_new_level() {
        let mut core = create_matching_core(InstrumentId::from("AAPL.XNAS"), Price::from("0.01"));
        core.set_ask_raw(Price::from("99.00"));

        core.add_order(limit_order(OrderSide::Buy, "100.00", "O-A"));
        core.add_order(limit_order(OrderSide::Buy, "100.00", "O-B"));
        core.add_order(limit_order(OrderSide::Buy, "100.00", "O-C"));

        core.delete_order(ClientOrderId::from("O-A")).unwrap();
        core.add_order(limit_order(OrderSide::Buy, "100.50", "O-A"));

        // Re-adding at the same price still loses queue position to O-C
        core.delete_order(ClientOrderId::from("O-B")).unwrap();
        core.add_order(limit_order(OrderSide::Buy, "100.00", "O-B"));

        let actions = core.iterate_bids();
        assert_eq!(
            actions,
            vec![
                MatchAction::FillLimit(ClientOrderId::from("O-A")),
                MatchAction::FillLimit(ClientOrderId::from("O-C")),
                MatchAction::FillLimit(ClientOrderId::from("O-B")),
            ],
        );
    }

    #[rstest]
    fn test_delete_unknown_order_returns_not_found() {
        let mut core = create_matching_core(InstrumentId::from("AAPL.XNAS"), Price::from("0.01"));
        let result = core.delete_order(ClientOrderId::from("O-MISSING"));
        assert!(matches!(result, Err(OrderError::NotFound(_))));
    }

    #[rstest]
    #[case::buy(OrderSide::Buy)]
    #[case::sell(OrderSide::Sell)]
    fn test_pending_order_lookup_delete_and_conversion(#[case] side: OrderSide) {
        let mut core = create_matching_core(InstrumentId::from("AAPL.XNAS"), Price::from("0.01"));
        core.set_bid_raw(Price::from("100.00"));
        core.set_ask_raw(Price::from("100.00"));
        let first = pending_order(side, "O-FIRST");
        let second = pending_order(side, "O-SECOND");
        core.add_order(first);
        core.add_order(second);

        assert_eq!(core.get_order(first.client_order_id), Some(&first));
        assert_eq!(core.get_order(second.client_order_id), Some(&second));
        assert!(core.order_exists(first.client_order_id));
        assert!(core.order_exists(second.client_order_id));
        assert_eq!(core.get_orders(), vec![first, second]);
        assert!(core.iterate().is_empty());

        core.delete_order(second.client_order_id).unwrap();

        assert_eq!(core.get_order(second.client_order_id), None);
        assert!(!core.order_exists(second.client_order_id));
        assert_eq!(core.get_order(first.client_order_id), Some(&first));
        assert!(core.order_exists(first.client_order_id));
        assert_eq!(core.get_orders(), vec![first]);

        core.delete_order(first.client_order_id).unwrap();

        assert_eq!(core.get_order(first.client_order_id), None);
        assert!(!core.order_exists(first.client_order_id));
        assert!(core.get_orders().is_empty());

        let converted = RestingOrder {
            limit_price: Some(Price::from("100.00")),
            ..first
        };
        core.add_order(converted);

        assert_eq!(core.get_order(first.client_order_id), Some(&converted));
        assert!(core.order_exists(first.client_order_id));
        assert_eq!(core.get_orders(), vec![converted]);
        assert_eq!(
            core.iterate(),
            vec![MatchAction::FillLimit(first.client_order_id)]
        );
    }

    #[rstest]
    fn test_order_views_preserve_price_time_priority() {
        let mut core = create_matching_core(InstrumentId::from("AAPL.XNAS"), Price::from("0.01"));
        let bids = [
            limit_order(OrderSide::Buy, "101.00", "O-B-LMT-1"),
            limit_order(OrderSide::Buy, "101.00", "O-B-LMT-2"),
            limit_order(OrderSide::Buy, "100.00", "O-B-LMT-3"),
            stop_order(OrderSide::Buy, "102.00", "O-B-STP-1"),
            stop_order(OrderSide::Buy, "102.00", "O-B-STP-2"),
            stop_order(OrderSide::Buy, "103.00", "O-B-STP-3"),
            pending_order(OrderSide::Buy, "O-B-PENDING-1"),
            pending_order(OrderSide::Buy, "O-B-PENDING-2"),
        ];
        let asks = [
            limit_order(OrderSide::Sell, "100.00", "O-A-LMT-1"),
            limit_order(OrderSide::Sell, "100.00", "O-A-LMT-2"),
            limit_order(OrderSide::Sell, "101.00", "O-A-LMT-3"),
            stop_order(OrderSide::Sell, "99.00", "O-A-STP-1"),
            stop_order(OrderSide::Sell, "99.00", "O-A-STP-2"),
            stop_order(OrderSide::Sell, "98.00", "O-A-STP-3"),
            pending_order(OrderSide::Sell, "O-A-PENDING-1"),
            pending_order(OrderSide::Sell, "O-A-PENDING-2"),
        ];

        for index in [6, 5, 2, 3, 0, 4, 1, 7] {
            core.add_order(asks[index]);
            core.add_order(bids[index]);
        }
        let orders = [bids, asks].concat();

        assert_eq!(core.iter_bid_orders().copied().collect::<Vec<_>>(), bids);
        assert_eq!(core.get_orders_bid(), bids);
        assert_eq!(core.iter_ask_orders().copied().collect::<Vec<_>>(), asks);
        assert_eq!(core.get_orders_ask(), asks);
        assert_eq!(core.iter_orders().copied().collect::<Vec<_>>(), orders);
        assert_eq!(core.get_orders(), orders);
    }

    #[rstest]
    #[case::buy(OrderSide::Buy, "99.00", "101.00")]
    #[case::sell(OrderSide::Sell, "101.00", "99.00")]
    fn test_touch_order_last_or_bid_ask_prefers_last_then_falls_back(
        #[case] side: OrderSide,
        #[case] crossed_price: &str,
        #[case] uncrossed_price: &str,
        #[values(OrderType::MarketIfTouched, OrderType::LimitIfTouched)] order_type: OrderType,
    ) {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut core = create_matching_core(instrument_id, Price::from("0.01"));
        let trigger_price = Price::from("100.00");
        let crossed = Price::from(crossed_price);
        let uncrossed = Price::from(uncrossed_price);
        let order = OrderTestBuilder::new(order_type)
            .instrument_id(instrument_id)
            .client_order_id(ClientOrderId::from("O-TOUCH"))
            .side(side)
            .price(uncrossed)
            .trigger_price(trigger_price)
            .trigger_type(TriggerType::LastOrBidAsk)
            .quantity(Quantity::from("10"))
            .build();
        let resting = RestingOrder::from(&PassiveOrderAny::try_from(order).unwrap());
        let action = Some(MatchAction::TriggerStop(resting.client_order_id));

        match side {
            OrderSide::Buy => {
                core.set_bid_raw(Price::from("98.00"));
                core.set_ask_raw(crossed);
            }
            OrderSide::Sell => {
                core.set_bid_raw(crossed);
                core.set_ask_raw(Price::from("102.00"));
            }
        }
        core.set_last_raw(uncrossed);

        assert_eq!(core.match_order(&resting), None);

        match side {
            OrderSide::Buy => core.set_ask_raw(uncrossed),
            OrderSide::Sell => core.set_bid_raw(uncrossed),
        }
        core.set_last_raw(trigger_price);

        assert_eq!(core.match_order(&resting), action);

        core.last = None;

        assert_eq!(core.match_order(&resting), None);

        match side {
            OrderSide::Buy => core.set_ask_raw(crossed),
            OrderSide::Sell => core.set_bid_raw(crossed),
        }

        assert_eq!(core.match_order(&resting), action);

        match side {
            OrderSide::Buy => core.ask = None,
            OrderSide::Sell => core.bid = None,
        }

        assert_eq!(core.match_order(&resting), None);
    }

    fn pending_order(side: OrderSide, id: &str) -> RestingOrder {
        RestingOrder::new(
            ClientOrderId::from(id),
            side,
            OrderType::MarketToLimit,
            None,
            None,
            true,
        )
    }
}
