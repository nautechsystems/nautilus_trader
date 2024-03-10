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
    trigger_stop_order: Option<fn(&StopOrderType)>,
    fill_market_order: Option<fn(&MarketOrder)>,
    fill_limit_order: Option<fn(&LimitOrderType)>,
}

impl OrderMatchingCore {
    pub fn new(
        instrument_id: InstrumentId,
        price_increment: Price,
        trigger_stop_order: Option<fn(&StopOrderType)>,
        fill_market_order: Option<fn(&MarketOrder)>,
        fill_limit_order: Option<fn(&LimitOrderType)>,
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
            // self.fill_limit_order.call(o)
        }
    }

    pub fn match_stop_order(&self, order: &StopOrderType) {
        if self.is_stop_matched(order) {
            // self.fill_stop_order.call(o)
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
    use nautilus_model::{
        enums::OrderSide, orders::stubs::TestOrderStubs, types::quantity::Quantity,
    };
    use rstest::rstest;

    use super::*;

    fn create_matching_core(
        instrument_id: InstrumentId,
        price_increment: Price,
    ) -> OrderMatchingCore {
        OrderMatchingCore::new(instrument_id, price_increment, None, None, None)
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
        Price::from("101.00"),  // <-- Price at ask
        OrderSide::Buy,
        true
    )]
    #[case(
        Some(Price::from("100.00")),
        Some(Price::from("101.00")),
        Price::from("102.00"),  // <-- Price higher than ask (marketable)
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
}
