// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
use crate::objects::price::Price;
use crate::objects::quantity::Quantity;


#[derive(Debug, Hash)]
pub struct Order {
    pub price: Price,
    pub size: Quantity,
    pub side: OrderSide,
    pub id: String,
}

impl Order {
    pub fn new(price: Price, size: Quantity, side: OrderSide, id: String) -> Self {
        Order {
            price,
            size,
            side,
            id,
        }
    }

    pub fn from_str_vec(input_vec: Vec<&str>) -> Self {
        assert_eq!(input_vec.len(), 4);
        Order {
            price: Price::new_from_str(&input_vec[0]),
            size: Quantity::new_from_str(&input_vec[1]),
            side: match input_vec[2] {
                "B" => OrderSide::Buy,
                "S" => OrderSide::Sell,
                _ => panic!("Cannot parse side, was {}", input_vec[2]),
            },
            id: String::from(input_vec[3]),
        }
    }
}

#[test]
fn order_from_str_vec() {
    let input = vec![
        "1.00000",
        "100",
        "B",
        "123"
    ];
    let order = Order::from_str_vec(input);

    assert_eq!(order.price, Price::new(1.0, 0));
    assert_eq!(order.size, Quantity::new(100.0, 0));
    assert_eq!(order.side, OrderSide::Buy);
    assert_eq!(order.id, String::from("123"));
}
