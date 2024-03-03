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
    enums::OrderSide,
    identifiers::instrument_id::InstrumentId,
    orders::{
        base::{
            GetLimitPrice, GetOrderSide, GetStopPrice, LimitOrderType, PassiveOrderType,
            StopOrderType,
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
    trigger_stop_order: fn(&StopOrderType),
    fill_market_order: fn(&MarketOrder),
    fill_limit_order: fn(&LimitOrderType),
}

impl OrderMatchingCore {
    pub fn new(
        instrument_id: InstrumentId,
        price_increment: Price,
        trigger_stop_order: fn(&StopOrderType),
        fill_market_order: fn(&MarketOrder),
        fill_limit_order: fn(&LimitOrderType),
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

    pub fn price_precision(&self) -> u8 {
        self.price_increment.precision
    }

    pub fn get_orders_bid(&self) -> &[PassiveOrderType] {
        self.orders_bid.as_slice()
    }

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

    pub fn add_order(&mut self, order: PassiveOrderType) {
        match order.get_order_side() {
            OrderSide::Buy => self.orders_bid.push(order),
            OrderSide::Sell => self.orders_ask.push(order),
            _ => panic!("Invalid order side"), // Design-time error
        }
    }

    pub fn delete_order(&mut self, order: &PassiveOrderType) {
        match order.get_order_side() {
            OrderSide::Buy => {
                let index = self
                    .orders_bid
                    .iter()
                    .position(|o| o == order)
                    .expect("Error: order not found");
                self.orders_bid.remove(index);
            }
            OrderSide::Sell => {
                let index = self
                    .orders_ask
                    .iter()
                    .position(|o| o == order)
                    .expect("Error: order {} not found");
                self.orders_ask.remove(index);
            }
            _ => panic!("Invalid order side"), // Design-time error
        }
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

    pub fn is_limit_matched(&self, order: &LimitOrderType) -> bool {
        match order.get_order_side() {
            OrderSide::Buy => self.ask.map_or(false, |a| a <= order.get_limit_px()),
            OrderSide::Sell => self.bid.map_or(false, |b| b >= order.get_limit_px()),
            _ => panic!("Invalid order side"), // Design-time error
        }
    }

    pub fn is_stop_matched(&self, order: &StopOrderType) -> bool {
        match order.get_order_side() {
            OrderSide::Buy => self.ask.map_or(false, |a| a >= order.get_stop_px()),
            OrderSide::Sell => self.bid.map_or(false, |b| b <= order.get_stop_px()),
            _ => panic!("Invalid order side"), // Design-time error
        }
    }
}
