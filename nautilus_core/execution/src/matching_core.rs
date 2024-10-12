// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use nautilus_model::{
    enums::OrderSideSpecified,
    identifiers::{ClientOrderId, InstrumentId},
    orders::{
        any::{LimitOrderAny, PassiveOrderAny, StopOrderAny},
        base::OrderError,
        market::MarketOrder,
    },
    types::price::Price,
};

/// A generic order matching core.
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
    orders_bid: Vec<PassiveOrderAny>,
    orders_ask: Vec<PassiveOrderAny>,
    trigger_stop_order: Option<fn(&StopOrderAny)>,
    fill_market_order: Option<fn(&MarketOrder)>,
    fill_limit_order: Option<fn(&LimitOrderAny)>,
}

impl OrderMatchingCore {
    // Creates a new [`OrderMatchingCore`] instance.
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        price_increment: Price,
        trigger_stop_order: Option<fn(&StopOrderAny)>,
        fill_market_order: Option<fn(&MarketOrder)>,
        fill_limit_order: Option<fn(&LimitOrderAny)>,
    ) -> Self {
        Self {
            instrument_id,
            price_increment,
            bid: None,
            ask: None,
            last: None,
            is_bid_initialized: false,
            is_ask_initialized: false,
            is_last_initialized: false,
            orders_bid: Vec::new(),
            orders_ask: Vec::new(),
            trigger_stop_order,
            fill_market_order,
            fill_limit_order,
        }
    }

    // -- QUERIES ---------------------------------------------------------------------------------

    #[must_use]
    pub const fn price_precision(&self) -> u8 {
        self.price_increment.precision
    }

    #[must_use]
    pub fn get_orders_bid(&self) -> &[PassiveOrderAny] {
        self.orders_bid.as_slice()
    }

    #[must_use]
    pub fn get_orders_ask(&self) -> &[PassiveOrderAny] {
        self.orders_ask.as_slice()
    }

    #[must_use]
    pub fn order_exists(&self, client_order_id: ClientOrderId) -> bool {
        self.orders_bid
            .iter()
            .any(|o| o.client_order_id() == client_order_id)
            || self
                .orders_ask
                .iter()
                .any(|o| o.client_order_id() == client_order_id)
    }

    // -- COMMANDS --------------------------------------------------------------------------------

    pub fn set_last_raw(&mut self, last: Price) {
        self.last = Some(last);
        self.is_last_initialized = true;
    }

    pub fn set_bid_raw(&mut self, bid: Price) {
        self.bid = Some(bid);
        self.is_bid_initialized = true;
    }

    pub fn set_ask_raw(&mut self, ask: Price) {
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

    pub fn add_order(&mut self, order: PassiveOrderAny) -> Result<(), OrderError> {
        match order.order_side_specified() {
            OrderSideSpecified::Buy => {
                self.orders_bid.push(order);
                Ok(())
            }
            OrderSideSpecified::Sell => {
                self.orders_ask.push(order);
                Ok(())
            }
        }
    }

    pub fn delete_order(&mut self, order: &PassiveOrderAny) -> Result<(), OrderError> {
        match order.order_side_specified() {
            OrderSideSpecified::Buy => {
                let index = self
                    .orders_bid
                    .iter()
                    .position(|o| o == order)
                    .ok_or(OrderError::NotFound(order.client_order_id()))?;
                self.orders_bid.remove(index);
                Ok(())
            }
            OrderSideSpecified::Sell => {
                let index = self
                    .orders_ask
                    .iter()
                    .position(|o| o == order)
                    .ok_or(OrderError::NotFound(order.client_order_id()))?;
                self.orders_ask.remove(index);
                Ok(())
            }
        }
    }

    pub fn iterate(&self) {
        self.iterate_bids();
        self.iterate_asks();
    }

    pub fn iterate_bids(&self) {
        self.iterate_orders(&self.orders_bid);
    }

    pub fn iterate_asks(&self) {
        self.iterate_orders(&self.orders_ask);
    }

    fn iterate_orders(&self, orders: &[PassiveOrderAny]) {
        for order in orders {
            self.match_order(order, false);
        }
    }

    // -- MATCHING --------------------------------------------------------------------------------

    fn match_order(&self, order: &PassiveOrderAny, _initial: bool) {
        match order {
            PassiveOrderAny::Limit(o) => self.match_limit_order(o),
            PassiveOrderAny::Stop(o) => self.match_stop_order(o),
        }
    }

    pub fn match_limit_order(&self, order: &LimitOrderAny) {
        if self.is_limit_matched(order) {
            if let Some(func) = self.fill_limit_order {
                func(order);
            }
        }
    }

    pub fn match_stop_order(&self, order: &StopOrderAny) {
        if self.is_stop_matched(order) {
            if let Some(func) = self.trigger_stop_order {
                func(order);
            }
        }
    }

    #[must_use]
    pub fn is_limit_matched(&self, order: &LimitOrderAny) -> bool {
        match order.order_side_specified() {
            OrderSideSpecified::Buy => self.ask.map_or(false, |a| a <= order.limit_px()),
            OrderSideSpecified::Sell => self.bid.map_or(false, |b| b >= order.limit_px()),
        }
    }

    #[must_use]
    pub fn is_stop_matched(&self, order: &StopOrderAny) -> bool {
        match order.order_side_specified() {
            OrderSideSpecified::Buy => self.ask.map_or(false, |a| a >= order.stop_px()),
            OrderSideSpecified::Sell => self.bid.map_or(false, |b| b <= order.stop_px()),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use nautilus_model::{
        enums::{OrderSide, OrderType},
        orders::builder::OrderTestBuilder,
        types::quantity::Quantity,
    };
    use rstest::rstest;

    use super::*;

    static TRIGGERED_STOPS: Mutex<Vec<StopOrderAny>> = Mutex::new(Vec::new());
    static FILLED_LIMITS: Mutex<Vec<LimitOrderAny>> = Mutex::new(Vec::new());

    fn create_matching_core(
        instrument_id: InstrumentId,
        price_increment: Price,
    ) -> OrderMatchingCore {
        OrderMatchingCore::new(instrument_id, price_increment, None, None, None)
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

        matching_core.add_order(order.clone().into()).unwrap();

        let passive_order: PassiveOrderAny = order.into();
        assert!(matching_core.get_orders_bid().contains(&passive_order));
        assert!(!matching_core.get_orders_ask().contains(&passive_order));
        assert_eq!(matching_core.get_orders_bid().len(), 1);
        assert!(matching_core.get_orders_ask().is_empty());
        assert!(matching_core.order_exists(passive_order.client_order_id()));
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

        matching_core.add_order(order.clone().into()).unwrap();

        let passive_order: PassiveOrderAny = order.into();
        assert!(matching_core.get_orders_ask().contains(&passive_order));
        assert!(!matching_core.get_orders_bid().contains(&passive_order));
        assert_eq!(matching_core.get_orders_ask().len(), 1);
        assert!(matching_core.get_orders_bid().is_empty());
        assert!(matching_core.order_exists(passive_order.client_order_id()));
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

        matching_core.add_order(order.into()).unwrap();
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

        let result = matching_core.delete_order(&order.into());
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

        matching_core.add_order(order.clone().into()).unwrap();
        matching_core.delete_order(&order.into()).unwrap();

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

        let result = matching_core.is_limit_matched(&order.into());
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

        let result = matching_core.is_stop_matched(&order.into());
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(OrderSide::Buy)]
    #[case(OrderSide::Sell)]
    fn test_match_stop_order_when_triggered(#[case] order_side: OrderSide) {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let trigger_price = Price::from("100.00");

        fn trigger_stop_order_handler(order: &StopOrderAny) {
            let order = order;
            TRIGGERED_STOPS.lock().unwrap().push(order.clone());
        }

        let mut matching_core = OrderMatchingCore::new(
            instrument_id,
            Price::from("0.01"),
            Some(trigger_stop_order_handler),
            None,
            None,
        );

        matching_core.bid = Some(Price::from("100.00"));
        matching_core.ask = Some(Price::from("100.00"));

        let order = OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(instrument_id)
            .side(order_side)
            .trigger_price(trigger_price)
            .quantity(Quantity::from("100"))
            .build();

        matching_core.match_stop_order(&order.clone().into());

        let triggered_stops = TRIGGERED_STOPS.lock().unwrap();
        assert_eq!(triggered_stops.len(), 1);
        assert_eq!(triggered_stops[0], order.into());
    }

    #[rstest]
    #[case(OrderSide::Buy)]
    #[case(OrderSide::Sell)]
    fn test_match_limit_order_when_triggered(#[case] order_side: OrderSide) {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let price = Price::from("100.00");

        fn fill_limit_order_handler(order: &LimitOrderAny) {
            FILLED_LIMITS.lock().unwrap().push(order.clone());
        }

        let mut matching_core = OrderMatchingCore::new(
            instrument_id,
            Price::from("0.01"),
            None,
            None,
            Some(fill_limit_order_handler),
        );

        matching_core.bid = Some(Price::from("100.00"));
        matching_core.ask = Some(Price::from("100.00"));

        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument_id)
            .side(order_side)
            .price(price)
            .quantity(Quantity::from("100"))
            .build();

        matching_core.match_limit_order(&order.clone().into());

        let filled_limits = FILLED_LIMITS.lock().unwrap();
        assert_eq!(filled_limits.len(), 1);
        assert_eq!(filled_limits[0], order.into());
    }
}
