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

//! A common order matching core for the `OrderMatchingEngine` and other components.
//!
//! # Book layout
//!
//! Each side has two separate books, mirroring real-venue architecture:
//! - **Limit book**: `BTreeMap<Price, OrderBucket>` keyed by limit price.
//!   Holds plain `LIMIT` orders.
//! - **Stop book**: `BTreeMap<Price, OrderBucket>` keyed by trigger price.
//!   Holds `STOP_*`, `*_IF_TOUCHED`, and `TRAILING_STOP_*` orders that need
//!   trigger checking before matching.
//!
//! Plus a per-side pending `SmallVec` for orders without a key (e.g.
//! `MARKET_TO_LIMIT` before conversion).
//!
//! # Ordering invariant
//!
//! Orders are matched in **price-time priority**, with limits processed
//! before stops on each side:
//! - **Bid limits**: best (highest) price first via `iter().rev()`.
//! - **Ask limits**: best (lowest) price first via `iter()`.
//! - **Bid stops**: closest trigger first via `iter()` (lowest trigger crosses
//!   first as ask climbs through resting buy stops).
//! - **Ask stops**: closest trigger first via `iter().rev()` (highest trigger
//!   crosses first as bid drops through resting sell stops).
//!
//! Within a price level orders are stored in a `SmallVec` in insertion order,
//! preserving time priority (FIFO at the same price). No active sorting
//! happens; the `BTreeMap`'s tree shape gives price ordering for free.
//!
//! # Modify semantics
//!
//! The core does not expose an in-place modify API. Any change to a resting
//! order must call [`OrderMatchingCore::delete_order`] followed by
//! [`OrderMatchingCore::add_order`], which lands the order at the back of
//! its (new or unchanged) price level. This matches real-venue behavior for
//! price-changing modifies but loses queue position on quantity-only
//! modifies. An in-place quantity-update API could be added later if the
//! engine wants to preserve queue position on those.
//!
//! # Known limitation: limits-then-stops emission
//!
//! On each side, [`OrderMatchingCore::iterate_bids`] and
//! [`OrderMatchingCore::iterate_asks`] emit all matchable limits before any
//! triggered stops. In real venues stops trigger as the price crosses them
//! and only then aggress against the limit book, so a snapshot iteration that
//! sees both kinds matchable simultaneously cannot perfectly reconstruct the
//! temporal order. The matching engine drives the snapshot, so a future
//! engine change that feeds the previous bid/ask to the core could replay
//! the price path and emit triggers/fills in cross-time order. Until then,
//! callers that depend on price-path ordering (e.g. multi-level gap
//! scenarios with both matchable limits and matchable stops on the same
//! side) should treat that interleaving as undefined.
//!
//! # Duplicate inserts
//!
//! `add_order` does not deduplicate. Adding the same `client_order_id` twice
//! without an intervening `delete_order` puts two `RestingOrder` entries in
//! the vec and they will both match. Callers must ensure each
//! `client_order_id` appears at most once across both sides.
//!
//! # Performance
//!
//! Per-level buckets are `SmallVec`s with [`INLINE_ORDERS_PER_LEVEL`] inline
//! slots so the common case (1-3 orders per price) avoids heap allocation
//! per bucket. Above that threshold the bucket spills to the heap. Adds and
//! deletes are O(log L) for the `BTreeMap` lookup plus O(B) for the bucket
//! scan/shift, where L is the number of distinct price levels per book and
//! B is orders at that level: both small in practice.
//!
//! An `AHashMap` index from `ClientOrderId` to `(side, BookKind, Price)`
//! makes [`OrderMatchingCore::get_order`], [`OrderMatchingCore::order_exists`],
//! and the lookup portion of [`OrderMatchingCore::delete_order`] hash-fast;
//! the follow-on bucket scan is O(B). The map is used purely for point
//! queries (never iterated), so its randomized seed does not affect
//! determinism.

use std::collections::BTreeMap;

use ahash::AHashMap;
use nautilus_model::{
    enums::{OrderSideSpecified, OrderType},
    identifiers::{ClientOrderId, InstrumentId},
    orders::{Order, OrderError, PassiveOrderAny, StopOrderAny},
    types::Price,
};
use smallvec::SmallVec;

/// Inline capacity for orders at a single price level. Sized to cover the
/// typical 1-3 orders per level; above this the per-bucket `SmallVec` spills
/// to the heap.
pub const INLINE_ORDERS_PER_LEVEL: usize = 4;

type OrderBucket = SmallVec<[RestingOrder; INLINE_ORDERS_PER_LEVEL]>;

/// Identifies which per-side book a [`RestingOrder`] lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BookKind {
    /// Plain `LIMIT` order in the limit book, keyed by limit price.
    Limit,
    /// Stop-style order in the stop book, keyed by trigger price. Includes
    /// `STOP_*`, `*_IF_TOUCHED`, and `TRAILING_STOP_*` order types.
    Stop,
}

/// An action returned by [`OrderMatchingCore::iterate`] when an order matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchAction {
    FillLimit(ClientOrderId),
    TriggerStop(ClientOrderId),
}

/// Lightweight order information for matching/trigger checking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RestingOrder {
    pub client_order_id: ClientOrderId,
    pub order_side: OrderSideSpecified,
    pub order_type: OrderType,
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
    /// `None` for such orders. This is a known coverage gap and not a bug
    /// in the constructor.
    #[must_use]
    pub const fn new(
        client_order_id: ClientOrderId,
        order_side: OrderSideSpecified,
        order_type: OrderType,
        trigger_price: Option<Price>,
        limit_price: Option<Price>,
        is_activated: bool,
    ) -> Self {
        Self {
            client_order_id,
            order_side,
            order_type,
            trigger_price,
            limit_price,
            is_activated,
        }
    }

    /// Returns true if this is a stop order type that needs trigger checking.
    #[must_use]
    pub const fn is_stop(&self) -> bool {
        self.trigger_price.is_some()
    }

    /// Returns true if this is a limit order type that needs fill checking.
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
                order_side: limit.order_side_specified(),
                order_type: limit.order_type(),
                trigger_price: None,
                limit_price: Some(limit.limit_px()),
                is_activated: true,
            },
            PassiveOrderAny::Stop(stop) => {
                let limit_price = match stop {
                    StopOrderAny::LimitIfTouched(o) => Some(o.price),
                    StopOrderAny::StopLimit(o) => Some(o.price),
                    StopOrderAny::TrailingStopLimit(o) => Some(o.price),
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
                    order_side: stop.order_side_specified(),
                    order_type: stop.order_type(),
                    trigger_price: Some(stop.stop_px()),
                    limit_price,
                    is_activated,
                }
            }
        }
    }
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
    order_index: AHashMap<ClientOrderId, (OrderSideSpecified, Option<(BookKind, Price)>)>,
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
        if let Some((kind, price)) = location {
            self.book_for(side, kind)
                .get(&price)?
                .iter()
                .find(|o| o.client_order_id == client_order_id)
        } else {
            self.pending_for(side)
                .iter()
                .find(|o| o.client_order_id == client_order_id)
        }
    }

    /// Iterates the bid-side orders in price-time priority without
    /// allocating: limits best (highest) first, then stops nearest-trigger
    /// (lowest) first, then pending unkeyed orders. Borrowed view; for an
    /// owned snapshot use [`Self::get_orders_bid`].
    pub fn iter_bid_orders(&self) -> impl Iterator<Item = &RestingOrder> {
        self.bid_limits
            .values()
            .rev()
            .flat_map(|b| b.iter())
            .chain(self.bid_stops.values().flat_map(|b| b.iter()))
            .chain(self.pending_bid.iter())
    }

    /// Iterates the ask-side orders in price-time priority without
    /// allocating: limits best (lowest) first, then stops nearest-trigger
    /// (highest) first, then pending unkeyed orders. Borrowed view; for an
    /// owned snapshot use [`Self::get_orders_ask`].
    pub fn iter_ask_orders(&self) -> impl Iterator<Item = &RestingOrder> {
        self.ask_limits
            .values()
            .flat_map(|b| b.iter())
            .chain(self.ask_stops.values().rev().flat_map(|b| b.iter()))
            .chain(self.pending_ask.iter())
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

    /// Returns the per-side book for the given `(side, kind)`.
    fn book_for(&self, side: OrderSideSpecified, kind: BookKind) -> &BTreeMap<Price, OrderBucket> {
        match (side, kind) {
            (OrderSideSpecified::Buy, BookKind::Limit) => &self.bid_limits,
            (OrderSideSpecified::Buy, BookKind::Stop) => &self.bid_stops,
            (OrderSideSpecified::Sell, BookKind::Limit) => &self.ask_limits,
            (OrderSideSpecified::Sell, BookKind::Stop) => &self.ask_stops,
        }
    }

    /// Returns the per-side pending bucket.
    fn pending_for(&self, side: OrderSideSpecified) -> &[RestingOrder] {
        match side {
            OrderSideSpecified::Buy => &self.pending_bid,
            OrderSideSpecified::Sell => &self.pending_ask,
        }
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

    /// Returns the (book kind, key) for an order, or `None` if the order has
    /// neither limit nor trigger price (e.g. `MARKET_TO_LIMIT` pre-conversion).
    fn locate(order: &RestingOrder) -> Option<(BookKind, Price)> {
        if order.is_stop() {
            // is_stop() == trigger_price.is_some()
            Some((BookKind::Stop, order.trigger_price.unwrap()))
        } else {
            order.limit_price.map(|p| (BookKind::Limit, p))
        }
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
    /// - Pure `LIMIT` orders go to the side's limit book, keyed by limit price.
    /// - Orders with neither price (e.g. `MARKET_TO_LIMIT` before conversion)
    ///   go to the per-side pending bucket. They remain visible to `get_order`
    ///   / `order_exists` but `iterate_*` skips them.
    ///
    /// # Panics
    ///
    /// Panics in debug builds if the invariant is violated.
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
            let book = match (side, kind) {
                (OrderSideSpecified::Buy, BookKind::Limit) => &mut self.bid_limits,
                (OrderSideSpecified::Buy, BookKind::Stop) => &mut self.bid_stops,
                (OrderSideSpecified::Sell, BookKind::Limit) => &mut self.ask_limits,
                (OrderSideSpecified::Sell, BookKind::Stop) => &mut self.ask_stops,
            };
            book.entry(price).or_default().push(order);
        } else {
            match side {
                OrderSideSpecified::Buy => self.pending_bid.push(order),
                OrderSideSpecified::Sell => self.pending_ask.push(order),
            }
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
            let book = match (side, kind) {
                (OrderSideSpecified::Buy, BookKind::Limit) => &mut self.bid_limits,
                (OrderSideSpecified::Buy, BookKind::Stop) => &mut self.bid_stops,
                (OrderSideSpecified::Sell, BookKind::Limit) => &mut self.ask_limits,
                (OrderSideSpecified::Sell, BookKind::Stop) => &mut self.ask_stops,
            };
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
            let pending = match side {
                OrderSideSpecified::Buy => &mut self.pending_bid,
                OrderSideSpecified::Sell => &mut self.pending_ask,
            };
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
        self.bid_limits
            .iter()
            .rev()
            .flat_map(|(_, b)| b.iter())
            .chain(self.bid_stops.values().flat_map(|b| b.iter()))
            .filter_map(|order| self.match_order(order))
            .collect()
    }

    /// Matches ask-side orders: limits best (lowest) first, then stops
    /// nearest-trigger (highest) first. FIFO within each price level.
    pub fn iterate_asks(&self) -> Vec<MatchAction> {
        self.ask_limits
            .values()
            .flat_map(|b| b.iter())
            .chain(self.ask_stops.iter().rev().flat_map(|(_, b)| b.iter()))
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

        if let Some(trigger_price) = order.trigger_price
            && self.is_stop_matched(order.order_side, trigger_price)
        {
            Some(MatchAction::TriggerStop(order.client_order_id))
        } else {
            None
        }
    }

    /// Returns whether a limit order at `price` would cross the opposite side
    /// (BUY: `ask <= price`, SELL: `bid >= price`).
    #[must_use]
    pub fn is_limit_matched(&self, side: OrderSideSpecified, price: Price) -> bool {
        match side {
            OrderSideSpecified::Buy => self.ask.is_some_and(|a| a <= price),
            OrderSideSpecified::Sell => self.bid.is_some_and(|b| b >= price),
        }
    }

    /// Returns whether a stop trigger at `price` has been reached
    /// (BUY: `ask >= price`, SELL: `bid <= price`).
    #[must_use]
    pub fn is_stop_matched(&self, side: OrderSideSpecified, price: Price) -> bool {
        match side {
            OrderSideSpecified::Buy => self.ask.is_some_and(|a| a >= price),
            OrderSideSpecified::Sell => self.bid.is_some_and(|b| b <= price),
        }
    }

    /// Returns whether a touch trigger at `trigger_price` has been reached
    /// (BUY: `ask <= trigger_price`, SELL: `bid >= trigger_price`).
    #[must_use]
    pub fn is_touch_triggered(&self, side: OrderSideSpecified, trigger_price: Price) -> bool {
        match side {
            OrderSideSpecified::Buy => self.ask.is_some_and(|a| a <= trigger_price),
            OrderSideSpecified::Sell => self.bid.is_some_and(|b| b >= trigger_price),
        }
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
    pub fn is_limit_fillable(&self, side: OrderSideSpecified, price: Price) -> bool {
        if self.is_limit_matched(side, price) {
            return true;
        }

        if !self.fill_limit_inside_spread {
            return false;
        }

        // Require both quotes present since fill simulation needs best bid and ask
        if let (Some(bid), Some(ask)) = (self.bid, self.ask) {
            match side {
                OrderSideSpecified::Buy => price >= bid,
                OrderSideSpecified::Sell => price <= ask,
            }
        } else {
            false
        }
    }
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

        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_id)
            .side(OrderSide::Sell)
            .price(Price::from("100.00"))
            .quantity(Quantity::from("100"))
            .build();

        let client_order_id = order.client_order_id();
        let match_info = RestingOrder::from(&PassiveOrderAny::try_from(order).unwrap());
        matching_core.add_order(match_info);
        matching_core.set_bid_raw(Price::from("100.00"));
        matching_core.set_ask_raw(Price::from("100.00"));
        matching_core.set_last_raw(Price::from("100.00"));

        matching_core.reset();

        assert!(matching_core.bid.is_none());
        assert!(matching_core.ask.is_none());
        assert!(matching_core.last.is_none());
        assert!(matching_core.get_orders_bid().is_empty());
        assert!(matching_core.get_orders_ask().is_empty());
        assert!(!matching_core.order_exists(client_order_id));
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
        Price::from("100.00"),  // <-- Price below ask
        OrderSide::Buy,
        false
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("101.00"),  // <-- Price at ask
        OrderSide::Buy,
        true
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("102.00"),  // <-- Price above ask (marketable)
        OrderSide::Buy,
        true
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("101.00"), // <-- Price above bid
        OrderSide::Sell,
        false
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("100.00"),  // <-- Price at bid
        OrderSide::Sell,
        true
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("99.00"),  // <-- Price below bid (marketable)
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

        let result =
            matching_core.is_limit_matched(order.order_side_specified(), order.price().unwrap());
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(None, None, Price::from("100.00"), OrderSide::Buy, false)]
    #[case(None, None, Price::from("100.00"), OrderSide::Sell, false)]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("102.00"),  // <-- Trigger above ask
        OrderSide::Buy,
        false
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("101.00"),  // <-- Trigger at ask
        OrderSide::Buy,
        true
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("100.00"),  // <-- Trigger below ask
        OrderSide::Buy,
        true
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("99.00"),  // Trigger below bid
        OrderSide::Sell,
        false
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("100.00"),  // <-- Trigger at bid
        OrderSide::Sell,
        true
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("101.00"),  // <-- Trigger above bid
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

        let result = matching_core
            .is_stop_matched(order.order_side_specified(), order.trigger_price().unwrap());
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

        // Buy limit at 100 with ask at 101 — not matched
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

        // Manually create an unactivated stop (simulates trailing stop)
        let match_info = RestingOrder::new(
            ClientOrderId::from("O-001"),
            OrderSideSpecified::Buy,
            OrderType::TrailingStopMarket,
            Some(Price::from("105.00")),
            None,
            false, // not activated
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
            OrderSideSpecified::Buy,
            OrderType::TrailingStopMarket,
            Some(Price::from("105.00")),
            None,
            true, // activated
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

        // Buy limit at 101 — matches (ask <= price)
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

        // Sell stop at 99 — matches (bid <= trigger)
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

        // Bids processed first, then asks
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

        assert!(core.is_limit_fillable(OrderSideSpecified::Buy, Price::from("101.00")));
        assert!(!core.is_limit_fillable(OrderSideSpecified::Buy, Price::from("100.00")));
        assert!(core.is_limit_fillable(OrderSideSpecified::Sell, Price::from("100.00")));
        assert!(!core.is_limit_fillable(OrderSideSpecified::Sell, Price::from("101.00")));
    }

    #[rstest]
    fn test_is_limit_fillable_inside_spread_buy_at_bid() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut core = create_matching_core(instrument_id, Price::from("0.01"));
        core.set_bid_raw(Price::from("100.00"));
        core.set_ask_raw(Price::from("101.00"));
        core.set_fill_limit_inside_spread(true);

        assert!(core.is_limit_fillable(OrderSideSpecified::Buy, Price::from("100.00")));
        assert!(core.is_limit_fillable(OrderSideSpecified::Buy, Price::from("100.50")));
        assert!(!core.is_limit_fillable(OrderSideSpecified::Buy, Price::from("99.00")));
    }

    #[rstest]
    fn test_is_limit_fillable_inside_spread_sell_at_ask() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut core = create_matching_core(instrument_id, Price::from("0.01"));
        core.set_bid_raw(Price::from("100.00"));
        core.set_ask_raw(Price::from("101.00"));
        core.set_fill_limit_inside_spread(true);

        assert!(core.is_limit_fillable(OrderSideSpecified::Sell, Price::from("101.00")));
        assert!(core.is_limit_fillable(OrderSideSpecified::Sell, Price::from("100.50")));
        assert!(!core.is_limit_fillable(OrderSideSpecified::Sell, Price::from("102.00")));
    }

    #[rstest]
    fn test_is_limit_fillable_inside_spread_requires_both_quotes_present() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut core = create_matching_core(instrument_id, Price::from("0.01"));
        core.set_fill_limit_inside_spread(true);

        core.set_bid_raw(Price::from("100.00"));
        assert!(!core.is_limit_fillable(OrderSideSpecified::Buy, Price::from("100.00")));

        let mut core2 = create_matching_core(instrument_id, Price::from("0.01"));
        core2.set_fill_limit_inside_spread(true);
        core2.set_ask_raw(Price::from("101.00"));
        assert!(!core2.is_limit_fillable(OrderSideSpecified::Sell, Price::from("101.00")));

        // Ask cleared after both were set
        let mut core3 = create_matching_core(instrument_id, Price::from("0.01"));
        core3.set_fill_limit_inside_spread(true);
        core3.set_bid_raw(Price::from("100.00"));
        core3.set_ask_raw(Price::from("101.00"));
        core3.ask = None;
        assert!(!core3.is_limit_fillable(OrderSideSpecified::Buy, Price::from("100.00")));
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
        Price::from("102.00"),  // <-- Ask below trigger
        OrderSide::Buy,
        true
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("101.00"),  // <-- Ask at trigger
        OrderSide::Buy,
        true
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("100.00"),  // <-- Ask above trigger
        OrderSide::Buy,
        false
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("99.00"),  // <-- Bid above trigger
        OrderSide::Sell,
        true
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("100.00"),  // <-- Bid at trigger
        OrderSide::Sell,
        true
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("101.00"),  // <-- Bid below trigger
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

        let result = matching_core.is_touch_triggered(order_side.as_specified(), trigger_price);
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
        // MARKET_TO_LIMIT and any caller-built `RestingOrder::new` with both
        // prices `None` must no-op rather than dispatch to a match function.
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut core = create_matching_core(instrument_id, Price::from("0.01"));
        core.set_bid_raw(Price::from("100.00"));
        core.set_ask_raw(Price::from("101.00"));

        let info = RestingOrder::new(
            ClientOrderId::from("O-NEITHER"),
            OrderSideSpecified::Buy,
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
                .trigger_type(TriggerType::Default)
                .build(),
        );

        let info = RestingOrder::from(&PassiveOrderAny::try_from(order).unwrap());

        assert_eq!(info.trigger_price, Some(Price::from("100.00")));
        assert_eq!(info.limit_price, Some(Price::from("99.00")));
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
        // TrailingStopMarket starts unactivated until the trigger has been seen.
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

    // -- Book layout & iteration ordering ---------------------------------

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

        // Add intentionally out-of-price-order to verify the BTreeMap re-sorts.
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
        // Codex regression: ask climbs from 100 to 106. BUY stops at 101 and
        // 105 should both trigger, but the 101 stop must fire first because
        // the ask crossed it before reaching 105.
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
        // Symmetric to the BUY case: bid drops from 100 to 94. SELL stops at
        // 99 and 95 should both trigger, but 99 must fire first because the
        // bid crossed it before reaching 95.
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
        // Both must match: ask=106 fills BUY limit at 110 (106 <= 110) AND
        // triggers BUY stop at 101 (106 >= 101). Limits emit before stops.
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
        // Both must match: bid=94 fills SELL limit at 90 (94 >= 90) AND
        // triggers SELL stop at 99 (94 <= 99). Limits emit before stops.
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
        // STOP_LIMIT has both prices set. is_stop() is true (because
        // trigger_price.is_some()), so it must live in the stop book and be
        // keyed by trigger_price for trigger-priority iteration.
        let mut core = create_matching_core(InstrumentId::from("AAPL.XNAS"), Price::from("0.01"));
        core.set_ask_raw(Price::from("106.00"));

        // Two STOP_LIMIT BUYs at different triggers; the closer trigger
        // (101) must fire first regardless of limit prices.
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
        // Both sides matchable simultaneously requires limits priced beyond
        // the touch and stops nearer to the touch.
        // Bid: ask=106 -> BUY limits at 110/107 fill (106 <= each), BUY stops
        // at 101/105 trigger (106 >= each).
        // Ask: bid=94 -> SELL limits at 90/93 fill (94 >= each), SELL stops
        // at 95/99 trigger (94 <= each).
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
                // bids: limits high-to-low, then stops near-to-far
                MatchAction::FillLimit(ClientOrderId::from("O-B-LMT-HIGH")),
                MatchAction::FillLimit(ClientOrderId::from("O-B-LMT-LOW")),
                MatchAction::TriggerStop(ClientOrderId::from("O-B-STP-NEAR")),
                MatchAction::TriggerStop(ClientOrderId::from("O-B-STP-FAR")),
                // asks: limits low-to-high, then stops near-to-far
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

        // Real orders.
        core.add_order(limit_order(OrderSide::Buy, "100.00", "O-LMT"));

        // A pending (no key) order.
        let pending = RestingOrder::new(
            ClientOrderId::from("O-PENDING"),
            OrderSideSpecified::Buy,
            OrderType::MarketToLimit,
            None,
            None,
            true,
        );
        core.add_order(pending);

        // iterate sees only the limit; the pending order has no price to match.
        assert_eq!(
            core.iterate_bids(),
            vec![MatchAction::FillLimit(ClientOrderId::from("O-LMT"))],
        );

        // get_orders sees both: bucketed first, pending appended.
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
        // A price-changing modify is delete + add; the re-added order must
        // land at the back of the new price level (queue-position loss),
        // matching real-venue behavior.
        let mut core = create_matching_core(InstrumentId::from("AAPL.XNAS"), Price::from("0.01"));
        core.set_ask_raw(Price::from("99.00"));

        core.add_order(limit_order(OrderSide::Buy, "100.00", "O-A"));
        core.add_order(limit_order(OrderSide::Buy, "100.00", "O-B"));
        core.add_order(limit_order(OrderSide::Buy, "100.00", "O-C"));

        // O-A modifies its price to 100.50 (better): moves to a new level.
        core.delete_order(ClientOrderId::from("O-A")).unwrap();
        core.add_order(limit_order(OrderSide::Buy, "100.50", "O-A"));

        // O-B then modifies to 100.00 in place (price unchanged via re-add):
        // loses queue position to O-C at the same level.
        core.delete_order(ClientOrderId::from("O-B")).unwrap();
        core.add_order(limit_order(OrderSide::Buy, "100.00", "O-B"));

        let actions = core.iterate_bids();
        assert_eq!(
            actions,
            vec![
                MatchAction::FillLimit(ClientOrderId::from("O-A")), // 100.50 best
                MatchAction::FillLimit(ClientOrderId::from("O-C")), // 100.00 oldest
                MatchAction::FillLimit(ClientOrderId::from("O-B")), // 100.00 newest
            ],
        );
    }

    #[rstest]
    fn test_delete_unknown_order_returns_not_found() {
        let mut core = create_matching_core(InstrumentId::from("AAPL.XNAS"), Price::from("0.01"));
        let result = core.delete_order(ClientOrderId::from("O-MISSING"));
        assert!(matches!(result, Err(OrderError::NotFound(_))));
    }
}
