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

#[cfg(test)]
mod tests {
    use nautilus_order_book::book::OrderBook;
    use nautilus_order_book::entry::OrderBookEntry;

    #[test]
    fn instantiate_order_book() {
        let order_book = OrderBook::new(0);

        assert_eq!(0, order_book.timestamp);
        assert_eq!(0, order_book.last_update_id);
    }

    #[test]
    fn best_bid_price_when_no_entries_returns_min() {
        let order_book = OrderBook::new(0);
        let result = order_book.best_bid_price;

        assert_eq!(f64::MIN, result);
    }

    #[test]
    fn best_ask_price_when_no_entries_returns_max() {
        let order_book = OrderBook::new(0);
        let result = order_book.best_ask_price;

        assert_eq!(f64::MAX, result);
    }

    #[test]
    fn best_bid_qty_when_no_entries_returns_zero() {
        let order_book = OrderBook::new(0);

        let result = order_book.best_bid_qty;

        assert_eq!(0.0, result);
    }

    #[test]
    fn best_ask_qty_when_no_entries_returns_zero() {
        let order_book = OrderBook::new(0);
        let result = order_book.best_ask_qty;

        assert_eq!(0.0, result);
    }

    #[test]
    fn apply_bid_diff() {
        let mut order_book = OrderBook::new(0);

        order_book.apply_bid_diff(OrderBookEntry { price: 1000.0, qty: 10.0, update_id: 1 }, 1610000000001);

        assert_eq!(1000.0, order_book.best_bid_price);
        assert_eq!(10.0, order_book.best_bid_qty);
        assert_eq!(1610000000001, order_book.timestamp);
        assert_eq!(1, order_book.last_update_id);
    }

    #[test]
    fn apply_ask_diff() {
        let mut order_book = OrderBook::new(0);

        order_book.apply_ask_diff(OrderBookEntry { price: 1001.0, qty: 20.0, update_id: 2 }, 1610000000002);

        assert_eq!(1001.0, order_book.best_ask_price);
        assert_eq!(20.0, order_book.best_ask_qty);
        assert_eq!(1610000000002, order_book.timestamp);
        assert_eq!(2, order_book.last_update_id);
    }

    #[test]
    fn apply_bid_then_ask_diffs() {
        let mut order_book = OrderBook::new(0);

        order_book.apply_bid_diff(OrderBookEntry { price: 1000.0, qty: 10.0, update_id: 1 }, 1610000000001);
        order_book.apply_ask_diff(OrderBookEntry { price: 1001.0, qty: 20.0, update_id: 2 }, 1610000000002);

        assert_eq!(1000.0, order_book.best_bid_price);
        assert_eq!(1001.0, order_book.best_ask_price);
        assert_eq!(10.0, order_book.best_bid_qty);
        assert_eq!(20.0, order_book.best_ask_qty);
        assert_eq!(1.0, order_book.spread());
        assert_eq!(1610000000002, order_book.timestamp);
        assert_eq!(2, order_book.last_update_id);
    }
}
