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

//! A common `OrderMatchingCore` for the `OrderMatchingEngine` and other components.

use nautilus_model::{
    enums::{OrderSideSpecified, OrderType},
    identifiers::{ClientOrderId, InstrumentId},
    orders::{Order, OrderError, PassiveOrderAny, StopOrderAny},
    types::Price,
};

/// An action returned by [`OrderMatchingCore::iterate`] when an order matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchAction {
    FillLimit(ClientOrderId),
    TriggerStop(ClientOrderId),
}

/// Lightweight order information for matching/trigger checking.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrderMatchInfo {
    pub client_order_id: ClientOrderId,
    pub order_side: OrderSideSpecified,
    pub order_type: OrderType,
    pub trigger_price: Option<Price>,
    pub limit_price: Option<Price>,
    pub is_activated: bool,
}

impl OrderMatchInfo {
    /// Creates a new [`OrderMatchInfo`] instance.
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

impl From<&PassiveOrderAny> for OrderMatchInfo {
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

/// A generic order matching core.
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
    pub is_bid_initialized: bool,
    pub is_ask_initialized: bool,
    pub is_last_initialized: bool,
    fill_limit_inside_spread: bool,
    orders_bid: Vec<OrderMatchInfo>,
    orders_ask: Vec<OrderMatchInfo>,
}

impl OrderMatchingCore {
    // Creates a new [`OrderMatchingCore`] instance.
    #[must_use]
    pub const fn new(instrument_id: InstrumentId, price_increment: Price) -> Self {
        Self {
            instrument_id,
            price_increment,
            bid: None,
            ask: None,
            last: None,
            is_bid_initialized: false,
            is_ask_initialized: false,
            is_last_initialized: false,
            fill_limit_inside_spread: false,
            orders_bid: Vec::new(),
            orders_ask: Vec::new(),
        }
    }

    #[must_use]
    pub const fn price_precision(&self) -> u8 {
        self.price_increment.precision
    }

    #[must_use]
    pub fn get_order(&self, client_order_id: ClientOrderId) -> Option<&OrderMatchInfo> {
        self.orders_bid
            .iter()
            .find(|o| o.client_order_id == client_order_id)
            .or_else(|| {
                self.orders_ask
                    .iter()
                    .find(|o| o.client_order_id == client_order_id)
            })
    }

    #[must_use]
    pub const fn get_orders_bid(&self) -> &[OrderMatchInfo] {
        self.orders_bid.as_slice()
    }

    #[must_use]
    pub const fn get_orders_ask(&self) -> &[OrderMatchInfo] {
        self.orders_ask.as_slice()
    }

    #[must_use]
    pub fn get_orders(&self) -> Vec<OrderMatchInfo> {
        let mut orders = self.orders_bid.clone();
        orders.extend_from_slice(&self.orders_ask);
        orders
    }

    #[must_use]
    pub fn order_exists(&self, client_order_id: ClientOrderId) -> bool {
        self.orders_bid
            .iter()
            .any(|o| o.client_order_id == client_order_id)
            || self
                .orders_ask
                .iter()
                .any(|o| o.client_order_id == client_order_id)
    }

    pub const fn set_last_raw(&mut self, last: Price) {
        self.last = Some(last);
        self.is_last_initialized = true;
    }

    pub const fn set_bid_raw(&mut self, bid: Price) {
        self.bid = Some(bid);
        self.is_bid_initialized = true;
    }

    pub const fn set_ask_raw(&mut self, ask: Price) {
        self.ask = Some(ask);
        self.is_ask_initialized = true;
    }

    pub fn reset(&mut self) {
        self.bid = None;
        self.ask = None;
        self.last = None;
        self.orders_bid.clear();
        self.orders_ask.clear();
    }

    /// Adds an order to the matching core.
    pub fn add_order(&mut self, order: OrderMatchInfo) {
        match order.order_side {
            OrderSideSpecified::Buy => self.orders_bid.push(order),
            OrderSideSpecified::Sell => self.orders_ask.push(order),
        }
    }

    /// Deletes an order from the matching core by client order ID.
    ///
    /// # Errors
    ///
    /// Returns an [`OrderError::NotFound`] if the order is not present.
    pub fn delete_order(&mut self, client_order_id: ClientOrderId) -> Result<(), OrderError> {
        if let Some(index) = self
            .orders_bid
            .iter()
            .position(|o| o.client_order_id == client_order_id)
        {
            self.orders_bid.remove(index);
            return Ok(());
        }

        if let Some(index) = self
            .orders_ask
            .iter()
            .position(|o| o.client_order_id == client_order_id)
        {
            self.orders_ask.remove(index);
            return Ok(());
        }

        Err(OrderError::NotFound(client_order_id))
    }

    pub fn iterate(&mut self) -> Vec<MatchAction> {
        let mut actions = self.iterate_bids();
        actions.extend(self.iterate_asks());
        actions
    }

    pub fn iterate_bids(&mut self) -> Vec<MatchAction> {
        let orders: Vec<_> = self.orders_bid.clone();
        orders
            .iter()
            .filter_map(|order| self.match_order(order))
            .collect()
    }

    pub fn iterate_asks(&mut self) -> Vec<MatchAction> {
        let orders: Vec<_> = self.orders_ask.clone();
        orders
            .iter()
            .filter_map(|order| self.match_order(order))
            .collect()
    }

    pub fn match_order(&self, order: &OrderMatchInfo) -> Option<MatchAction> {
        if order.is_stop() {
            self.match_stop_order(order)
        } else if order.is_limit() {
            self.match_limit_order(order)
        } else {
            None
        }
    }

    fn match_limit_order(&self, order: &OrderMatchInfo) -> Option<MatchAction> {
        if let Some(limit_price) = order.limit_price
            && self.is_limit_fillable(order.order_side, limit_price)
        {
            Some(MatchAction::FillLimit(order.client_order_id))
        } else {
            None
        }
    }

    fn match_stop_order(&self, order: &OrderMatchInfo) -> Option<MatchAction> {
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

    #[must_use]
    pub fn is_limit_matched(&self, side: OrderSideSpecified, price: Price) -> bool {
        match side {
            OrderSideSpecified::Buy => self.ask.is_some_and(|a| a <= price),
            OrderSideSpecified::Sell => self.bid.is_some_and(|b| b >= price),
        }
    }

    #[must_use]
    pub fn is_stop_matched(&self, side: OrderSideSpecified, price: Price) -> bool {
        match side {
            OrderSideSpecified::Buy => self.ask.is_some_and(|a| a >= price),
            OrderSideSpecified::Sell => self.bid.is_some_and(|b| b <= price),
        }
    }

    #[must_use]
    pub fn is_touch_triggered(&self, side: OrderSideSpecified, trigger_price: Price) -> bool {
        match side {
            OrderSideSpecified::Buy => self.ask.is_some_and(|a| a <= trigger_price),
            OrderSideSpecified::Sell => self.bid.is_some_and(|b| b >= trigger_price),
        }
    }

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
        enums::{OrderSide, OrderType},
        orders::{Order, builder::OrderTestBuilder},
        types::Quantity,
    };
    use rstest::rstest;

    use super::*;

    const fn create_matching_core(
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

        let match_info = OrderMatchInfo::from(&PassiveOrderAny::try_from(order).unwrap());
        matching_core.add_order(match_info.clone());

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

        let match_info = OrderMatchInfo::from(&PassiveOrderAny::try_from(order).unwrap());
        matching_core.add_order(match_info.clone());

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
        let match_info = OrderMatchInfo::from(&PassiveOrderAny::try_from(order).unwrap());
        matching_core.add_order(match_info);
        matching_core.bid = Some(Price::from("100.00"));
        matching_core.ask = Some(Price::from("100.00"));
        matching_core.last = Some(Price::from("100.00"));

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
        let match_info = OrderMatchInfo::from(&PassiveOrderAny::try_from(order).unwrap());
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
        matching_core.bid = Some(Price::from("100.00"));
        matching_core.ask = Some(Price::from("101.00"));

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
        let match_info = OrderMatchInfo::from(&PassiveOrderAny::try_from(order).unwrap());
        matching_core.add_order(match_info);

        let actions = matching_core.iterate();

        assert!(actions.is_empty());
    }

    #[rstest]
    fn test_iterate_returns_fill_limit_for_matched_buy() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut matching_core = create_matching_core(instrument_id, Price::from("0.01"));
        matching_core.ask = Some(Price::from("100.00"));

        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_id)
            .side(OrderSide::Buy)
            .price(Price::from("100.00"))
            .quantity(Quantity::from("100"))
            .build();
        let client_order_id = order.client_order_id();
        let match_info = OrderMatchInfo::from(&PassiveOrderAny::try_from(order).unwrap());
        matching_core.add_order(match_info);

        let actions = matching_core.iterate();

        assert_eq!(actions, vec![MatchAction::FillLimit(client_order_id)]);
    }

    #[rstest]
    fn test_iterate_returns_fill_limit_for_matched_sell() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut matching_core = create_matching_core(instrument_id, Price::from("0.01"));
        matching_core.bid = Some(Price::from("100.00"));

        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_id)
            .side(OrderSide::Sell)
            .price(Price::from("100.00"))
            .quantity(Quantity::from("100"))
            .build();
        let client_order_id = order.client_order_id();
        let match_info = OrderMatchInfo::from(&PassiveOrderAny::try_from(order).unwrap());
        matching_core.add_order(match_info);

        let actions = matching_core.iterate();

        assert_eq!(actions, vec![MatchAction::FillLimit(client_order_id)]);
    }

    #[rstest]
    fn test_iterate_returns_no_fill_for_unmatched_limit() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut matching_core = create_matching_core(instrument_id, Price::from("0.01"));
        matching_core.ask = Some(Price::from("101.00"));

        // Buy limit at 100 with ask at 101 — not matched
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_id)
            .side(OrderSide::Buy)
            .price(Price::from("100.00"))
            .quantity(Quantity::from("100"))
            .build();
        let match_info = OrderMatchInfo::from(&PassiveOrderAny::try_from(order).unwrap());
        matching_core.add_order(match_info);

        let actions = matching_core.iterate();

        assert!(actions.is_empty());
    }

    #[rstest]
    fn test_iterate_returns_trigger_stop_for_matched_buy() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut matching_core = create_matching_core(instrument_id, Price::from("0.01"));
        matching_core.ask = Some(Price::from("101.00"));

        let order = OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(instrument_id)
            .side(OrderSide::Buy)
            .trigger_price(Price::from("101.00"))
            .quantity(Quantity::from("100"))
            .build();
        let client_order_id = order.client_order_id();
        let match_info = OrderMatchInfo::from(&PassiveOrderAny::try_from(order).unwrap());
        matching_core.add_order(match_info);

        let actions = matching_core.iterate();

        assert_eq!(actions, vec![MatchAction::TriggerStop(client_order_id)]);
    }

    #[rstest]
    fn test_iterate_returns_trigger_stop_for_matched_sell() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut matching_core = create_matching_core(instrument_id, Price::from("0.01"));
        matching_core.bid = Some(Price::from("99.00"));

        let order = OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(instrument_id)
            .side(OrderSide::Sell)
            .trigger_price(Price::from("99.00"))
            .quantity(Quantity::from("100"))
            .build();
        let client_order_id = order.client_order_id();
        let match_info = OrderMatchInfo::from(&PassiveOrderAny::try_from(order).unwrap());
        matching_core.add_order(match_info);

        let actions = matching_core.iterate();

        assert_eq!(actions, vec![MatchAction::TriggerStop(client_order_id)]);
    }

    #[rstest]
    fn test_iterate_skips_unactivated_stop_order() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut matching_core = create_matching_core(instrument_id, Price::from("0.01"));
        matching_core.ask = Some(Price::from("110.00"));

        // Manually create an unactivated stop (simulates trailing stop)
        let match_info = OrderMatchInfo::new(
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
        matching_core.ask = Some(Price::from("110.00"));

        let client_order_id = ClientOrderId::from("O-001");
        let match_info = OrderMatchInfo::new(
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
        matching_core.bid = Some(Price::from("99.00"));
        matching_core.ask = Some(Price::from("101.00"));

        // Buy limit at 101 — matches (ask <= price)
        let buy_limit = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_id)
            .side(OrderSide::Buy)
            .price(Price::from("101.00"))
            .quantity(Quantity::from("100"))
            .build();
        let buy_limit_id = buy_limit.client_order_id();
        matching_core.add_order(OrderMatchInfo::from(
            &PassiveOrderAny::try_from(buy_limit).unwrap(),
        ));

        // Sell stop at 99 — matches (bid <= trigger)
        let sell_stop = OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(instrument_id)
            .side(OrderSide::Sell)
            .trigger_price(Price::from("99.00"))
            .quantity(Quantity::from("50"))
            .build();
        let sell_stop_id = sell_stop.client_order_id();
        matching_core.add_order(OrderMatchInfo::from(
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
        let match_info = OrderMatchInfo::from(&PassiveOrderAny::try_from(order).unwrap());
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
}
