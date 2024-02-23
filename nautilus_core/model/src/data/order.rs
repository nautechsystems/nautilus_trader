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

use std::{
    fmt::{Display, Formatter},
    hash::{Hash, Hasher},
};

use nautilus_core::serialization::Serializable;
use serde::{Deserialize, Serialize};

use super::{quote::QuoteTick, trade::TradeTick};
use crate::{
    enums::OrderSide,
    orderbook::{book::BookIntegrityError, ladder::BookPrice},
    types::{price::Price, quantity::Quantity},
};

pub type OrderId = u64;

pub const NULL_ORDER: BookOrder = BookOrder {
    side: OrderSide::NoOrderSide,
    price: Price {
        raw: 0,
        precision: 0,
    },
    size: Quantity {
        raw: 0,
        precision: 0,
    },
    order_id: 0,
};

/// Represents an order in a book.
#[repr(C)]
#[derive(Clone, Eq, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
#[cfg_attr(feature = "trivial_copy", derive(Copy))]
pub struct BookOrder {
    /// The order side.
    pub side: OrderSide,
    /// The order price.
    pub price: Price,
    /// The order size.
    pub size: Quantity,
    /// The order ID.
    pub order_id: OrderId,
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
            OrderSide::Buy => Self::new(
                OrderSide::Buy,
                tick.bid_price,
                tick.bid_size,
                tick.bid_price.raw as u64,
            ),
            OrderSide::Sell => Self::new(
                OrderSide::Sell,
                tick.ask_price,
                tick.ask_size,
                tick.ask_price.raw as u64,
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

impl Default for BookOrder {
    fn default() -> Self {
        NULL_ORDER
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

impl Serializable for BookOrder {}

////////////////////////////////////////////////////////////////////////////////
// Stubs
////////////////////////////////////////////////////////////////////////////////
#[cfg(feature = "stubs")]
pub mod stubs {
    use rstest::fixture;

    use super::{BookOrder, OrderSide};
    use crate::types::{price::Price, quantity::Quantity};

    #[fixture]
    pub fn stub_book_order() -> BookOrder {
        let price = Price::from("100.00");
        let size = Quantity::from("10");
        let side = OrderSide::Buy;
        let order_id = 123_456;

        BookOrder::new(side, price, size, order_id)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::{stubs::*, *};
    use crate::{
        enums::AggressorSide,
        identifiers::{instrument_id::InstrumentId, trade_id::TradeId},
    };

    #[rstest]
    fn test_new() {
        let price = Price::from("100.00");
        let size = Quantity::from("10");
        let side = OrderSide::Buy;
        let order_id = 123_456;

        let order = BookOrder::new(side, price, size, order_id);

        assert_eq!(order.price, price);
        assert_eq!(order.size, size);
        assert_eq!(order.side, side);
        assert_eq!(order.order_id, order_id);
    }

    #[rstest]
    fn test_to_book_price() {
        let price = Price::from("100.00");
        let size = Quantity::from("10");
        let side = OrderSide::Buy;
        let order_id = 123_456;

        let order = BookOrder::new(side, price, size, order_id);
        let book_price = order.to_book_price();

        assert_eq!(book_price.value, price);
        assert_eq!(book_price.side, side);
    }

    #[rstest]
    fn test_exposure() {
        let price = Price::from("100.00");
        let size = Quantity::from("10");
        let side = OrderSide::Buy;
        let order_id = 123_456;

        let order = BookOrder::new(side, price, size, order_id);
        let exposure = order.exposure();

        assert_eq!(exposure, price.as_f64() * size.as_f64());
    }

    #[rstest]
    fn test_signed_size() {
        let price = Price::from("100.00");
        let size = Quantity::from("10");
        let order_id = 123_456;

        let order_buy = BookOrder::new(OrderSide::Buy, price, size, order_id);
        let signed_size_buy = order_buy.signed_size();
        assert_eq!(signed_size_buy, size.as_f64());

        let order_sell = BookOrder::new(OrderSide::Sell, price, size, order_id);
        let signed_size_sell = order_sell.signed_size();
        assert_eq!(signed_size_sell, -(size.as_f64()));
    }

    #[rstest]
    fn test_display() {
        let price = Price::from("100.00");
        let size = Quantity::from(10);
        let side = OrderSide::Buy;
        let order_id = 123_456;

        let order = BookOrder::new(side, price, size, order_id);
        let display = format!("{order}");

        let expected = format!("{price},{size},{side},{order_id}");
        assert_eq!(display, expected);
    }

    #[rstest]
    #[case(OrderSide::Buy)]
    #[case(OrderSide::Sell)]
    fn test_from_quote_tick(#[case] side: OrderSide) {
        let tick = QuoteTick::new(
            InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            Price::from("5000.00"),
            Price::from("5001.00"),
            Quantity::from("100.000"),
            Quantity::from("99.000"),
            0,
            0,
        )
        .unwrap();

        let book_order = BookOrder::from_quote_tick(&tick, side);

        assert_eq!(book_order.side, side);
        assert_eq!(
            book_order.price,
            match side {
                OrderSide::Buy => tick.bid_price,
                OrderSide::Sell => tick.ask_price,
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
                OrderSide::Buy => tick.bid_price.raw as u64,
                OrderSide::Sell => tick.ask_price.raw as u64,
                _ => panic!("Invalid test"),
            }
        );
    }

    #[rstest]
    #[case(OrderSide::Buy)]
    #[case(OrderSide::Sell)]
    fn test_from_trade_tick(#[case] side: OrderSide) {
        let tick = TradeTick::new(
            InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            Price::from("5000.00"),
            Quantity::from("100.00"),
            AggressorSide::Buyer,
            TradeId::new("1").unwrap(),
            0,
            0,
        );

        let book_order = BookOrder::from_trade_tick(&tick, side);

        assert_eq!(book_order.side, side);
        assert_eq!(book_order.price, tick.price);
        assert_eq!(book_order.size, tick.size);
        assert_eq!(book_order.order_id, tick.price.raw as u64);
    }

    #[rstest]
    fn test_json_serialization(stub_book_order: BookOrder) {
        let order = stub_book_order;
        let serialized = order.as_json_bytes().unwrap();
        let deserialized = BookOrder::from_json_bytes(serialized).unwrap();
        assert_eq!(deserialized, order);
    }

    #[rstest]
    fn test_msgpack_serialization(stub_book_order: BookOrder) {
        let order = stub_book_order;
        let serialized = order.as_msgpack_bytes().unwrap();
        let deserialized = BookOrder::from_msgpack_bytes(serialized).unwrap();
        assert_eq!(deserialized, order);
    }
}
