// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{
    fmt::{Display, Formatter},
    hash::{Hash, Hasher},
};

use nautilus_core::time::UnixNanos;

use super::tick::{QuoteTick, TradeTick};
use crate::{
    enums::{BookAction, OrderSide},
    identifiers::instrument_id::InstrumentId,
    orderbook::{book::BookIntegrityError, ladder::BookPrice},
    types::{price::Price, quantity::Quantity},
};

/// Represents an order in a book.
#[repr(C)]
#[derive(Copy, Clone, Eq, Debug)]
pub struct BookOrder {
    pub side: OrderSide,
    pub price: Price,
    pub size: Quantity,
    pub order_id: u64,
}

impl BookOrder {
    #[must_use]
    pub fn new(side: OrderSide, price: Price, size: Quantity, order_id: u64) -> Self {
        Self {
            side,
            price,
            size,
            order_id,
        }
    }

    #[must_use]
    pub fn to_book_price(&self) -> BookPrice {
        BookPrice::new(self.price, self.side)
    }

    #[must_use]
    pub fn exposure(&self) -> f64 {
        self.price.as_f64() * self.size.as_f64()
    }

    #[must_use]
    pub fn signed_size(&self) -> f64 {
        match self.side {
            OrderSide::Buy => self.size.as_f64(),
            OrderSide::Sell => -(self.size.as_f64()),
            _ => panic!("{}", BookIntegrityError::NoOrderSide),
        }
    }

    #[must_use]
    pub fn from_quote_tick(tick: &QuoteTick, side: OrderSide) -> Self {
        match side {
            OrderSide::Buy => {
                Self::new(OrderSide::Buy, tick.bid, tick.bid_size, tick.bid.raw as u64)
            }
            OrderSide::Sell => Self::new(
                OrderSide::Sell,
                tick.ask,
                tick.ask_size,
                tick.ask.raw as u64,
            ),
            _ => panic!("{}", BookIntegrityError::NoOrderSide),
        }
    }

    #[must_use]
    pub fn from_trade_tick(tick: &TradeTick, side: OrderSide) -> Self {
        match side {
            OrderSide::Buy => {
                Self::new(OrderSide::Buy, tick.price, tick.size, tick.price.raw as u64)
            }
            OrderSide::Sell => Self::new(
                OrderSide::Sell,
                tick.price,
                tick.size,
                tick.price.raw as u64,
            ),
            _ => panic!("{}", BookIntegrityError::NoOrderSide),
        }
    }
}

impl PartialEq for BookOrder {
    fn eq(&self, other: &Self) -> bool {
        self.order_id == other.order_id
    }
}

impl Hash for BookOrder {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.order_id.hash(state);
    }
}

impl Display for BookOrder {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{},{}",
            self.price, self.size, self.side, self.order_id,
        )
    }
}

/// Represents a single change/delta in an order book.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OrderBookDelta {
    pub instrument_id: InstrumentId,
    pub action: BookAction,
    pub order: BookOrder,
    pub flags: u8,
    pub sequence: u64,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

impl OrderBookDelta {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        action: BookAction,
        order: BookOrder,
        flags: u8,
        sequence: u64,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            instrument_id,
            action,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        }
    }
}

impl Display for OrderBookDelta {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{},{},{},{},{}",
            self.instrument_id,
            self.action,
            self.order,
            self.flags,
            self.sequence,
            self.ts_event,
            self.ts_init
        )
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rstest::rstest;

    use super::*;
    use crate::{enums::AggressorSide, identifiers::trade_id::TradeId};

    #[test]
    fn test_book_order_new() {
        let price = Price::from("100.00");
        let size = Quantity::from("10");
        let side = OrderSide::Buy;
        let order_id = 123456;

        let order = BookOrder::new(side, price.clone(), size.clone(), order_id);

        assert_eq!(order.price, price);
        assert_eq!(order.size, size);
        assert_eq!(order.side, side);
        assert_eq!(order.order_id, order_id);
    }

    #[test]
    fn test_book_order_to_book_price() {
        let price = Price::from("100.00");
        let size = Quantity::from("10");
        let side = OrderSide::Buy;
        let order_id = 123456;

        let order = BookOrder::new(side, price.clone(), size.clone(), order_id);
        let book_price = order.to_book_price();

        assert_eq!(book_price.value, price);
        assert_eq!(book_price.side, side);
    }

    #[test]
    fn test_book_order_exposure() {
        let price = Price::from("100.00");
        let size = Quantity::from("10");
        let side = OrderSide::Buy;
        let order_id = 123456;

        let order = BookOrder::new(side, price.clone(), size.clone(), order_id);
        let exposure = order.exposure();

        assert_eq!(exposure, price.as_f64() * size.as_f64());
    }

    #[test]
    fn test_book_order_signed_size() {
        let price = Price::from("100.00");
        let size = Quantity::from("10");
        let order_id = 123456;

        let order_buy = BookOrder::new(OrderSide::Buy, price.clone(), size.clone(), order_id);
        let signed_size_buy = order_buy.signed_size();
        assert_eq!(signed_size_buy, size.as_f64());

        let order_sell = BookOrder::new(OrderSide::Sell, price.clone(), size.clone(), order_id);
        let signed_size_sell = order_sell.signed_size();
        assert_eq!(signed_size_sell, -(size.as_f64()));
    }

    #[test]
    fn test_book_order_display() {
        let price = Price::from("100.00");
        let size = Quantity::from("10");
        let side = OrderSide::Buy;
        let order_id = 123456;

        let order = BookOrder::new(side, price.clone(), size.clone(), order_id);
        let display = format!("{}", order);

        let expected = format!("{},{},{},{}", price, size, side, order_id);
        assert_eq!(display, expected);
    }

    #[rstest(side, case(OrderSide::Buy), case(OrderSide::Sell))]
    fn book_order_from_quote_tick(side: OrderSide) {
        let tick = QuoteTick::new(
            InstrumentId::from_str("ETHUSDT-PERP.BINANCE").unwrap(),
            Price::new(5000.0, 2),
            Price::new(5001.0, 2),
            Quantity::new(100.0, 3),
            Quantity::new(99.0, 3),
            0,
            0,
        );

        let book_order = BookOrder::from_quote_tick(&tick, side.clone());

        assert_eq!(book_order.side, side);
        assert_eq!(
            book_order.price,
            match side {
                OrderSide::Buy => tick.bid,
                OrderSide::Sell => tick.ask,
                _ => panic!("Invalid test"),
            }
        );
        assert_eq!(
            book_order.size,
            match side {
                OrderSide::Buy => tick.bid_size,
                OrderSide::Sell => tick.ask_size,
                _ => panic!("Invalid test"),
            }
        );
        assert_eq!(
            book_order.order_id,
            match side {
                OrderSide::Buy => tick.bid.raw as u64,
                OrderSide::Sell => tick.ask.raw as u64,
                _ => panic!("Invalid test"),
            }
        );
    }

    #[test]
    fn test_orderbook_delta_new() {
        let instrument_id = InstrumentId::from_str("AAPL.NASDAQ").unwrap();
        let action = BookAction::Add;
        let price = Price::from("100.00");
        let size = Quantity::from("10");
        let side = OrderSide::Buy;
        let order_id = 123456;
        let flags = 0;
        let sequence = 1;
        let ts_event = 1;
        let ts_init = 2;

        let order = BookOrder::new(side, price.clone(), size.clone(), order_id);
        let delta = OrderBookDelta::new(
            instrument_id.clone(),
            action,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        );

        assert_eq!(delta.instrument_id, instrument_id);
        assert_eq!(delta.action, action);
        assert_eq!(delta.order.price, price);
        assert_eq!(delta.order.size, size);
        assert_eq!(delta.order.side, side);
        assert_eq!(delta.order.order_id, order_id);
        assert_eq!(delta.flags, flags);
        assert_eq!(delta.sequence, sequence);
        assert_eq!(delta.ts_event, ts_event);
        assert_eq!(delta.ts_init, ts_init);
    }

    #[test]
    fn test_order_book_delta_display() {
        let instrument_id = InstrumentId::from_str("AAPL.NASDAQ").unwrap();
        let action = BookAction::Add;
        let price = Price::from("100.00");
        let size = Quantity::from("10");
        let side = OrderSide::Buy;
        let order_id = 123456;
        let flags = 0;
        let sequence = 1;
        let ts_event = 1;
        let ts_init = 2;

        let order = BookOrder::new(side, price.clone(), size.clone(), order_id);

        let delta = OrderBookDelta::new(
            instrument_id.clone(),
            action,
            order.clone(),
            flags,
            sequence,
            ts_event,
            ts_init,
        );

        assert_eq!(
            format!("{}", delta),
            "AAPL.NASDAQ,ADD,100.00,10,BUY,123456,0,1,1,2".to_string()
        );
    }

    #[rstest(side, case(OrderSide::Buy), case(OrderSide::Sell))]
    fn book_order_from_trade_tick(side: OrderSide) {
        let tick = TradeTick::new(
            InstrumentId::from_str("ETHUSDT-PERP.BINANCE").unwrap(),
            Price::new(5000.0, 2),
            Quantity::new(100.0, 2),
            AggressorSide::Buyer,
            TradeId::new("1"),
            0,
            0,
        );

        let book_order = BookOrder::from_trade_tick(&tick, side);

        assert_eq!(book_order.side, side);
        assert_eq!(book_order.price, tick.price);
        assert_eq!(book_order.size, tick.size);
        assert_eq!(book_order.order_id, tick.price.raw as u64);
    }
}
