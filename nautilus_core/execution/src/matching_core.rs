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

#![allow(dead_code)] // Under development

use nautilus_model::{
    identifiers::instrument_id::InstrumentId,
    orders::{
        base::{
            GetClientOrderId, GetLimitPrice, GetOrderSide, GetStopPrice, LimitOrderType,
            OrderError, OrderSideFixed, PassiveOrderType, StopOrderType,
        },
        market::MarketOrder,
    },
    types::price::Price,
};

/// Provides a generic order matching core.
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
    orders_bid: Vec<PassiveOrderType>,
    orders_ask: Vec<PassiveOrderType>,
    trigger_stop_order: Option<fn(StopOrderType)>,
    fill_market_order: Option<fn(MarketOrder)>,
    fill_limit_order: Option<fn(LimitOrderType)>,
}

impl OrderMatchingCore {
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        price_increment: Price,
        trigger_stop_order: Option<fn(StopOrderType)>,
        fill_market_order: Option<fn(MarketOrder)>,
        fill_limit_order: Option<fn(LimitOrderType)>,
    ) -> Self {
        Self {
            instrument_id,
            price_increment,
            bid: None,
            ask: None,
            last: None,
            orders_bid: Vec::new(),
            orders_ask: Vec::new(),
            trigger_stop_order,
            fill_market_order,
            fill_limit_order,
        }
    }

    // -- QUERIES ---------------------------------------------------------------------------------

    #[must_use]
    pub fn price_precision(&self) -> u8 {
        self.price_increment.precision
    }

    #[must_use]
    pub fn get_orders_bid(&self) -> &[PassiveOrderType] {
        self.orders_bid.as_slice()
    }

    #[must_use]
    pub fn get_orders_ask(&self) -> &[PassiveOrderType] {
        self.orders_ask.as_slice()
    }

    // -- COMMANDS --------------------------------------------------------------------------------

    pub fn reset(&mut self) {
        self.bid = None;
        self.ask = None;
        self.last = None;
        self.orders_bid.clear();
        self.orders_ask.clear();
    }

    pub fn add_order(&mut self, order: PassiveOrderType) -> Result<(), OrderError> {
        match order.get_order_side() {
            OrderSideFixed::Buy => {
                self.orders_bid.push(order);
                Ok(())
            }
            OrderSideFixed::Sell => {
                self.orders_ask.push(order);
                Ok(())
            }
        }
    }

    pub fn delete_order(&mut self, order: &PassiveOrderType) -> Result<(), OrderError> {
        match order.get_order_side() {
            OrderSideFixed::Buy => {
                let index = self
                    .orders_bid
                    .iter()
                    .position(|o| o == order)
                    .ok_or(OrderError::NotFound(order.get_client_order_id()))?;
                self.orders_bid.remove(index);
                Ok(())
            }
            OrderSideFixed::Sell => {
                let index = self
                    .orders_ask
                    .iter()
                    .position(|o| o == order)
                    .ok_or(OrderError::NotFound(order.get_client_order_id()))?;
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

    fn iterate_orders(&self, orders: &[PassiveOrderType]) {
        for order in orders {
            self.match_order(order, false);
        }
    }

    // -- MATCHING --------------------------------------------------------------------------------

    fn match_order(&self, order: &PassiveOrderType, _initial: bool) {
        match order {
            PassiveOrderType::Limit(o) => self.match_limit_order(o),
            PassiveOrderType::Stop(o) => self.match_stop_order(o),
        }
    }

    pub fn match_limit_order(&self, order: &LimitOrderType) {
        if self.is_limit_matched(order) {
            if let Some(func) = self.fill_limit_order {
                func(order.clone()); // TODO: Remove this clone (will need a lifetime)
            }
        }
    }

    pub fn match_stop_order(&self, order: &StopOrderType) {
        if self.is_stop_matched(order) {
            if let Some(func) = self.trigger_stop_order {
                func(order.clone()); // TODO: Remove this clone (will need a lifetime)
            }
        }
    }

    #[must_use]
    pub fn is_limit_matched(&self, order: &LimitOrderType) -> bool {
        match order.get_order_side() {
            OrderSideFixed::Buy => self.ask.map_or(false, |a| a <= order.get_limit_px()),
            OrderSideFixed::Sell => self.bid.map_or(false, |b| b >= order.get_limit_px()),
        }
    }

    #[must_use]
    pub fn is_stop_matched(&self, order: &StopOrderType) -> bool {
        match order.get_order_side() {
            OrderSideFixed::Buy => self.ask.map_or(false, |a| a >= order.get_stop_px()),
            OrderSideFixed::Sell => self.bid.map_or(false, |b| b <= order.get_stop_px()),
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
        enums::OrderSide, orders::stubs::TestOrderStubs, types::quantity::Quantity,
    };
    use rstest::rstest;

    use super::*;

    static TRIGGERED_STOPS: Mutex<Vec<StopOrderType>> = Mutex::new(Vec::new());
    static FILLED_LIMITS: Mutex<Vec<LimitOrderType>> = Mutex::new(Vec::new());

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

        let order = TestOrderStubs::limit_order(
            instrument_id,
            OrderSide::Buy,
            Price::from("100.00"),
            Quantity::from("100"),
            None,
            None,
        );

        let passive_order = PassiveOrderType::Limit(LimitOrderType::Limit(order));
        matching_core.add_order(passive_order.clone()).unwrap();

        assert!(matching_core.get_orders_bid().contains(&passive_order));
        assert!(!matching_core.get_orders_ask().contains(&passive_order));
        assert_eq!(matching_core.get_orders_bid().len(), 1);
        assert!(matching_core.get_orders_ask().is_empty());
    }

    #[rstest]
    fn test_add_order_ask_side() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut matching_core = create_matching_core(instrument_id, Price::from("0.01"));

        let order = TestOrderStubs::limit_order(
            instrument_id,
            OrderSide::Sell,
            Price::from("100.00"),
            Quantity::from("100"),
            None,
            None,
        );

        let passive_order = PassiveOrderType::Limit(LimitOrderType::Limit(order));
        matching_core.add_order(passive_order.clone()).unwrap();

        assert!(matching_core.get_orders_ask().contains(&passive_order));
        assert!(!matching_core.get_orders_bid().contains(&passive_order));
        assert_eq!(matching_core.get_orders_ask().len(), 1);
        assert!(matching_core.get_orders_bid().is_empty());
    }

    #[rstest]
    fn test_reset() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut matching_core = create_matching_core(instrument_id, Price::from("0.01"));

        let order = TestOrderStubs::limit_order(
            instrument_id,
            OrderSide::Sell,
            Price::from("100.00"),
            Quantity::from("100"),
            None,
            None,
        );

        let passive_order = PassiveOrderType::Limit(LimitOrderType::Limit(order));
        matching_core.add_order(passive_order).unwrap();
        matching_core.bid = Some(Price::from("100.00"));
        matching_core.ask = Some(Price::from("100.00"));
        matching_core.last = Some(Price::from("100.00"));

        matching_core.reset();

        assert!(matching_core.bid.is_none());
        assert!(matching_core.ask.is_none());
        assert!(matching_core.last.is_none());
        assert!(matching_core.get_orders_bid().is_empty());
        assert!(matching_core.get_orders_ask().is_empty());
    }

    #[rstest]
    fn test_delete_order_when_not_exists() {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut matching_core = create_matching_core(instrument_id, Price::from("0.01"));

        let order = TestOrderStubs::limit_order(
            instrument_id,
            OrderSide::Buy,
            Price::from("100.00"),
            Quantity::from("100"),
            None,
            None,
        );

        let passive_order = PassiveOrderType::Limit(LimitOrderType::Limit(order));
        let result = matching_core.delete_order(&passive_order);

        assert!(result.is_err());
    }

    #[rstest]
    #[case(OrderSide::Buy)]
    #[case(OrderSide::Sell)]
    fn test_delete_order_when_exists(#[case] order_side: OrderSide) {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let mut matching_core = create_matching_core(instrument_id, Price::from("0.01"));

        let order = TestOrderStubs::limit_order(
            instrument_id,
            order_side,
            Price::from("100.00"),
            Quantity::from("100"),
            None,
            None,
        );

        let passive_order = PassiveOrderType::Limit(LimitOrderType::Limit(order));
        matching_core.add_order(passive_order.clone()).unwrap();
        matching_core.delete_order(&passive_order).unwrap();

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

        let order = TestOrderStubs::limit_order(
            instrument_id,
            order_side,
            price,
            Quantity::from("100"),
            None,
            None,
        );

        let result = matching_core.is_limit_matched(&LimitOrderType::Limit(order));

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

        let order = TestOrderStubs::stop_market_order(
            instrument_id,
            order_side,
            trigger_price,
            Quantity::from("100"),
            None,
            None,
            None,
        );

        let result = matching_core.is_stop_matched(&StopOrderType::StopMarket(order));

        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(OrderSide::Buy)]
    #[case(OrderSide::Sell)]
    fn test_match_stop_order_when_triggered(#[case] order_side: OrderSide) {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let trigger_price = Price::from("100.00");

        fn trigger_stop_order_handler(order: StopOrderType) {
            let order = order;
            TRIGGERED_STOPS.lock().unwrap().push(order);
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

        let order = TestOrderStubs::stop_market_order(
            instrument_id,
            order_side,
            trigger_price,
            Quantity::from("100"),
            None,
            None,
            None,
        );

        matching_core.match_stop_order(&StopOrderType::StopMarket(order.clone()));

        let triggered_stops = TRIGGERED_STOPS.lock().unwrap();
        assert_eq!(triggered_stops.len(), 1);
        assert_eq!(triggered_stops[0], StopOrderType::StopMarket(order));
    }

    #[rstest]
    #[case(OrderSide::Buy)]
    #[case(OrderSide::Sell)]
    fn test_match_limit_order_when_triggered(#[case] order_side: OrderSide) {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let price = Price::from("100.00");

        fn fill_limit_order_handler(order: LimitOrderType) {
            FILLED_LIMITS.lock().unwrap().push(order);
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

        let order = TestOrderStubs::limit_order(
            instrument_id,
            order_side,
            price,
            Quantity::from("100.00"),
            None,
            None,
        );

        matching_core.match_limit_order(&LimitOrderType::Limit(order.clone()));

        let filled_limits = FILLED_LIMITS.lock().unwrap();
        assert_eq!(filled_limits.len(), 1);
        assert_eq!(filled_limits[0], LimitOrderType::Limit(order));
    }
}
