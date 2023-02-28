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

use crate::enums::OrderSide;
use crate::orderbook::ladder::BookPrice;
use crate::types::price::Price;
use crate::types::quantity::Quantity;

#[repr(C)]
#[derive(Debug)]
pub struct BookOrder {
    pub price: Price,
    pub size: Quantity,
    pub side: OrderSide,
    pub order_id: u64,
}

impl BookOrder {
    #[must_use]
    pub fn new(price: Price, size: Quantity, side: OrderSide, order_id: u64) -> Self {
        BookOrder {
            price,
            size,
            side,
            order_id,
        }
    }

    pub fn to_book_price(&self) -> BookPrice {
        BookPrice::new(self.price.clone(), self.side)
    }
}

impl From<Vec<&str>> for BookOrder {
    fn from(vec: Vec<&str>) -> Self {
        assert_eq!(vec.len(), 4);
        let price = Price::from(vec[0]);
        let size = Quantity::from(vec[1]);
        let side = match vec[2] {
            "B" => OrderSide::Buy,
            "S" => OrderSide::Sell,
            _ => panic!("Cannot parse side, was {}", vec[2]),
        };
        BookOrder::new(price, size, side, 0)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use crate::enums::OrderSide;
    use crate::orderbook::order::BookOrder;
    use crate::types::price::Price;
    use crate::types::quantity::Quantity;

    #[test]
    fn test_order_from_str_vec() {
        let input = vec!["1.00000", "100", "B", "123"];
        let order = BookOrder::from(input);
        assert_eq!(order.price, Price::new(1.0, 0));
        assert_eq!(order.size, Quantity::new(100.0, 0));
        assert_eq!(order.side, OrderSide::Buy);
        assert_eq!(order.order_id, 0);
    }
}
