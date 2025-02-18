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

//! An `OwnBookOrder` for use with tracking own/user orders in L3 order books.

use std::{
    fmt::{Debug, Display},
    hash::{Hash, Hasher},
};

use nautilus_core::UnixNanos;

use crate::{
    enums::{OrderSideSpecified, OrderType, TimeInForce},
    identifiers::ClientOrderId,
    orderbook::BookPrice,
    types::{Price, Quantity},
};

/// Represents an own order in a book.
#[repr(C)]
#[derive(Clone, Copy, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct OwnBookOrder {
    /// The client order ID.
    pub client_order_id: ClientOrderId,
    /// The specified order side (BUY or SELL).
    pub side: OrderSideSpecified,
    /// The order price.
    pub price: Price,
    /// The order size.
    pub size: Quantity,
    /// The order type.
    pub order_type: OrderType,
    /// The order time in force.
    pub time_in_force: TimeInForce,
    /// If the order is currently in-flight to the venue.
    pub is_inflight: bool,
    /// UNIX timestamp (nanoseconds) when the order was initialized.
    pub ts_init: UnixNanos,
}

impl OwnBookOrder {
    /// Creates a new [`OwnBookOrder`] instance.
    #[must_use]
    pub fn new(
        client_order_id: ClientOrderId,
        side: OrderSideSpecified,
        price: Price,
        size: Quantity,
        order_type: OrderType,
        time_in_force: TimeInForce,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            client_order_id,
            side,
            price,
            size,
            order_type,
            time_in_force,
            is_inflight: true,
            ts_init,
        }
    }

    /// Returns a [`BookPrice`] from this order.
    #[must_use]
    pub fn to_book_price(&self) -> BookPrice {
        BookPrice::new(self.price, self.side.as_order_side())
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
            OrderSideSpecified::Buy => self.size.as_f64(),
            OrderSideSpecified::Sell => -(self.size.as_f64()),
        }
    }
}

impl PartialEq for OwnBookOrder {
    fn eq(&self, other: &Self) -> bool {
        self.client_order_id == other.client_order_id
    }
}

impl Hash for OwnBookOrder {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.client_order_id.hash(state);
    }
}

impl Debug for OwnBookOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(client_order_id={}, side={}, price={}, size={}, order_type={}, time_in_force={}, ts_init={})",
            stringify!(OwnBookOrder),
            self.client_order_id,
            self.side,
            self.price,
            self.size,
            self.order_type,
            self.time_in_force,
            self.ts_init,
        )
    }
}

impl Display for OwnBookOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{},{},{},{},{}",
            self.client_order_id,
            self.side,
            self.price,
            self.size,
            self.order_type,
            self.time_in_force,
            self.ts_init,
        )
    }
}

#[cfg(test)]
mod tests {
    use rstest::{fixture, rstest};

    use super::*;
    use crate::enums::OrderSide;

    #[fixture]
    fn user_order() -> OwnBookOrder {
        let client_order_id = ClientOrderId::from("O-123456789");
        let side = OrderSideSpecified::Buy;
        let price = Price::from("100.00");
        let size = Quantity::from("10");
        let order_type = OrderType::Limit;
        let time_in_force = TimeInForce::Gtc;
        let ts_init = UnixNanos::default();

        OwnBookOrder::new(
            client_order_id,
            side,
            price,
            size,
            order_type,
            time_in_force,
            ts_init,
        )
    }

    #[rstest]
    fn test_to_book_price(user_order: OwnBookOrder) {
        let book_price = user_order.to_book_price();
        assert_eq!(book_price.value, Price::from("100.00"));
        assert_eq!(book_price.side, OrderSide::Buy);
    }

    #[rstest]
    fn test_exposure(user_order: OwnBookOrder) {
        let exposure = user_order.exposure();
        assert_eq!(exposure, 1000.0);
    }

    #[rstest]
    fn test_signed_size(user_order: OwnBookOrder) {
        let user_order_buy = user_order;
        let user_order_sell = OwnBookOrder::new(
            ClientOrderId::from("O-123456789"),
            OrderSideSpecified::Sell,
            Price::from("101.0"),
            Quantity::from("10"),
            OrderType::Limit,
            TimeInForce::Gtc,
            UnixNanos::default(),
        );

        assert_eq!(user_order_buy.signed_size(), 10.0);
        assert_eq!(user_order_sell.signed_size(), -10.0);
    }

    #[rstest]
    fn test_debug(user_order: OwnBookOrder) {
        assert_eq!(format!("{user_order:?}"), "OwnBookOrder(client_order_id=O-123456789, side=BUY, price=100.00, size=10, order_type=LIMIT, time_in_force=GTC, ts_init=0)");
    }

    #[rstest]
    fn test_display(user_order: OwnBookOrder) {
        assert_eq!(
            user_order.to_string(),
            "O-123456789,BUY,100.00,10,LIMIT,GTC,0".to_string()
        );
    }
}
