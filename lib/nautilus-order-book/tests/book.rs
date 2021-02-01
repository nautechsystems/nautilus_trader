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

    #[test]
    fn instantiate_order_book() {
        let order_book = OrderBook::new(
            "BTC/USD".to_string(),
            "USD".to_string(),
            0,
        );

        assert_eq!("BTC/USD", order_book.symbol);
        assert_eq!("USD", order_book.currency);
        assert_eq!(0, order_book.timestamp);
        assert_eq!(0, order_book.last_update_id);
    }

    #[test]
    fn best_bid_price_when_no_entries() {
        let order_book = OrderBook::new(
            "BTC/USD".to_string(),
            "USD".to_string(),
            0,
        );

        let result = order_book.best_bid_price();

        assert_eq!(0.0, result);
    }

    #[test]
    fn best_ask_price_when_no_entries() {
        let order_book = OrderBook::new(
            "BTC/USD".to_string(),
            "USD".to_string(),
            0,
        );

        let result = order_book.best_ask_price();

        assert_eq!(0.0, result);
    }

    #[test]
    fn best_bid_amount_when_no_entries() {
        let order_book = OrderBook::new(
            "BTC/USD".to_string(),
            "USD".to_string(),
            0,
        );

        let result = order_book.best_bid_amount();

        assert_eq!(0.0, result);
    }

    #[test]
    fn best_ask_amount_when_no_entries() {
        let order_book = OrderBook::new(
            "BTC/USD".to_string(),
            "USD".to_string(),
            0,
        );

        let result = order_book.best_ask_amount();

        assert_eq!(0.0, result);
    }

    #[test]
    fn apply_float_diffs() {
        let mut order_book = OrderBook::new(
            "BTC/USD".to_string(),
            "USD".to_string(),
            0,
        );

        order_book.apply_float_diffs(
            vec![[1000.0, 10.0], [999.0, 20.0]],
            vec![[1001.0, 11.0], [1002.0, 21.0]],
            1610000000000,
            1,
        );

        assert_eq!(1000.0, order_book.best_bid_price());
        assert_eq!(10.0, order_book.best_bid_amount());
        assert_eq!(1001.0, order_book.best_ask_price());
        assert_eq!(11.0, order_book.best_ask_amount());
        assert_eq!(1.0, order_book.spread());
        assert_eq!(1610000000000, order_book.timestamp);
        assert_eq!(1, order_book.last_update_id);
    }
}
