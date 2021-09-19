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

use crate::objects::price::Price;
use crate::orderbook::order::Order;
use std::cmp::Ordering;

#[repr(C)]
#[derive(Debug, Hash)]
pub struct Level {
    pub price: Price,
    pub orders: *mut Order,
    pub len: usize,
    cap: usize,
}

impl Level {
    pub fn new(price: Price, orders: Vec<Order>) -> Self {
        let (ptr, len, cap) = orders.into_raw_parts();
        Level {
            price,
            orders: ptr,
            len,
            cap,
        }
    }

    unsafe fn _update_orders(&mut self, orders: Vec<Order>) {
        let (ptr, len, cap) = orders.into_raw_parts();
        self.orders = ptr;
        self.len = len;
        self.cap = cap;
    }

    pub unsafe fn as_vec(&self) -> Vec<Order> {
        Vec::from_raw_parts(self.orders, self.len, self.cap)
    }

    pub unsafe fn add(&mut self, order: Order) {
        assert_eq!(order.price, self.price); // Confirm order for this level
        let mut orders = self.as_vec();
        orders.push(order);
        self._update_orders(orders);
    }

    pub unsafe fn update(&mut self, order: Order) {
        assert_eq!(order.price, self.price); // Confirm order for this level

        if order.size.value == 0 {
            self.delete(order)
        } else {
            let mut orders = self.as_vec();
            let index = orders
                .iter()
                .position(|o| o.id == order.id)
                .expect("Cannot update order: order not found");
            orders[index] = order;
            self._update_orders(orders);
        }
    }

    pub unsafe fn delete(&mut self, order: Order) {
        assert_eq!(order.price, self.price); // Confirm order for this level

        let mut orders = self.as_vec();
        let index = orders
            .iter()
            .position(|o| o.id == order.id)
            .expect("Cannot delete order: order not found");
        orders.remove(index);
        self._update_orders(orders);
    }

    pub unsafe fn volume(&self) -> f64 {
        let orders = self.as_vec();
        let mut sum: f64 = 0.0;
        for o in orders {
            sum += o.size.as_f64()
        }
        sum
    }

    pub unsafe fn exposure(&self) -> f64 {
        let orders = self.as_vec();
        let mut sum: f64 = 0.0;
        for o in orders {
            sum += o.price.as_f64() * o.size.as_f64()
        }
        sum
    }
}

impl PartialEq for Level {
    fn eq(&self, other: &Self) -> bool {
        self.price == other.price
    }

    fn ne(&self, other: &Self) -> bool {
        self.price != other.price
    }
}

impl Eq for Level {}

impl PartialOrd for Level {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.price.partial_cmp(&other.price)
    }

    fn lt(&self, other: &Self) -> bool {
        self.price.lt(&other.price)
    }

    fn le(&self, other: &Self) -> bool {
        self.price.le(&other.price)
    }

    fn gt(&self, other: &Self) -> bool {
        self.price.gt(&other.price)
    }

    fn ge(&self, other: &Self) -> bool {
        self.price.ge(&other.price)
    }
}

impl Ord for Level {
    fn cmp(&self, other: &Self) -> Ordering {
        self.price.cmp(&other.price)
    }
}

#[cfg(test)]
mod tests {
    use crate::enums::OrderSide;
    use crate::objects::price::Price;
    use crate::objects::quantity::Quantity;
    use crate::orderbook::level::Level;
    use crate::orderbook::order::Order;

    #[test]
    fn level_equality() {
        let level0 = Level::new(Price::new(1.00, 2), vec![]);
        let level1 = Level::new(Price::new(1.01, 2), vec![]);

        assert_eq!(level0, level0);
        assert!(level0 <= level0);
        assert!(level0 < level1);
    }

    #[test]
    fn level_add_one_order() {
        let mut level = Level::new(Price::new(1.00, 2), vec![]);
        let order = Order::new(
            Price::new(1.00, 2),
            Quantity::new(10.0, 0),
            OrderSide::Buy,
            0,
        );

        unsafe { level.add(order) };

        assert_eq!(level.len, 1);
        assert_eq!(level.cap, 4);
        unsafe { assert_eq!(level.volume(), 10.0) }
    }

    #[test]
    fn level_add_multiple_orders() {
        let mut level = Level::new(Price::new(1.00, 2), vec![]);
        let order1 = Order::new(
            Price::new(1.00, 2),
            Quantity::new(10.0, 0),
            OrderSide::Buy,
            0,
        );
        let order2 = Order::new(
            Price::new(1.00, 2),
            Quantity::new(10.0, 0),
            OrderSide::Buy,
            0,
        );

        unsafe { level.add(order1) };
        unsafe { level.add(order2) };

        assert_eq!(level.len, 2);
        unsafe { assert_eq!(level.volume(), 20.0) }
    }

    #[test]
    fn level_update_order() {
        let mut level = Level::new(Price::new(1.00, 2), vec![]);
        let order1 = Order::new(
            Price::new(1.00, 2),
            Quantity::new(10.0, 0),
            OrderSide::Buy,
            0,
        );
        let order2 = Order::new(
            Price::new(1.00, 2),
            Quantity::new(20.0, 0),
            OrderSide::Buy,
            0,
        );

        unsafe { level.add(order1) };
        unsafe { level.update(order2) };

        assert_eq!(level.len, 1);
        unsafe { assert_eq!(level.volume(), 20.0) }
    }

    #[test]
    fn level_update_order_with_zero_size_deletes() {
        let mut level = Level::new(Price::new(1.00, 2), vec![]);
        let order1 = Order::new(
            Price::new(1.00, 2),
            Quantity::new(10.0, 0),
            OrderSide::Buy,
            0,
        );
        let order2 = Order::new(
            Price::new(1.00, 2),
            Quantity::new(0.0, 0),
            OrderSide::Buy,
            0,
        );

        unsafe { level.add(order1) };
        unsafe { level.update(order2) };

        assert_eq!(level.len, 0);
        unsafe { assert_eq!(level.volume(), 0.0) }
    }
}
