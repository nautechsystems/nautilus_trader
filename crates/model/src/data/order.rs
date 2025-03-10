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

//! A `BookOrder` for use with the `OrderBook` and `OrderBookDelta` data type.

use std::{
    fmt::{Debug, Display},
    hash::{Hash, Hasher},
};

use nautilus_core::serialization::Serializable;
use serde::{Deserialize, Serialize};

use crate::{
    enums::OrderSide,
    orderbook::{BookIntegrityError, BookPrice},
    types::{Price, Quantity},
};

pub type OrderId = u64;

/// Represents a NULL book order (used with the `Clear` action or where an order is not specified).
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
#[derive(Clone, Copy, Eq, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
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
    /// Creates a new [`BookOrder`] instance.
    #[must_use]
    pub fn new(side: OrderSide, price: Price, size: Quantity, order_id: OrderId) -> Self {
        Self {
            side,
            price,
            size,
            order_id,
        }
    }

    /// Returns a [`BookPrice`] from this order.
    #[must_use]
    pub fn to_book_price(&self) -> BookPrice {
        BookPrice::new(self.price, self.side.as_specified())
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
            OrderSide::Buy => self.size.as_f64(),
            OrderSide::Sell => -(self.size.as_f64()),
            _ => panic!("{}", BookIntegrityError::NoOrderSide),
        }
    }
}

impl Default for BookOrder {
    /// Creates a NULL [`BookOrder`] instance.
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

impl Debug for BookOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(side={}, price={}, size={}, order_id={})",
            stringify!(BookOrder),
            self.side,
            self.price,
            self.size,
            self.order_id,
        )
    }
}

impl Display for BookOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{},{}",
            self.side, self.price, self.size, self.order_id,
        )
    }
}

impl Serializable for BookOrder {}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

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
        assert_eq!(book_price.side, side.as_specified());
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
    fn test_debug() {
        let price = Price::from("100.00");
        let size = Quantity::from(10);
        let side = OrderSide::Buy;
        let order_id = 123_456;
        let order = BookOrder::new(side, price, size, order_id);
        let result = format!("{order:?}");
        let expected = "BookOrder(side=BUY, price=100.00, size=10, order_id=123456)";
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_display() {
        let price = Price::from("100.00");
        let size = Quantity::from(10);
        let side = OrderSide::Buy;
        let order_id = 123_456;
        let order = BookOrder::new(side, price, size, order_id);
        let result = format!("{order}");
        let expected = "BUY,100.00,10,123456";
        assert_eq!(result, expected);
    }
}
