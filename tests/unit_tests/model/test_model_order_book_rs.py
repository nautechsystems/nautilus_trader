# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import unittest

from nautilus_trader.model.order_book_2 import OrderBook
from tests.test_kit.providers import TestInstrumentProvider

import numpy as np


ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


class OrderBookTests(unittest.TestCase):
    pass

    def test_instantiation(self):
        pass
        # Arrange
        # Act
        order_book = OrderBook(0)

        # Act
        # Assert
        self.assertEqual(0, order_book.timestamp())
        self.assertEqual(0, order_book.last_update_id())

    def test_apply_bid_diff(self):
        # Arrange
        order_book = OrderBook(0)

        # Act
        order_book.apply_bid_diff(1000.0, 20.0, 1, 1610000000000)

        # Assert
        self.assertEqual(1000.0, order_book.best_bid_price())
        self.assertEqual(20.0, order_book.best_bid_qty())
        self.assertEqual(1, order_book.last_update_id())
        self.assertEqual(1610000000000, order_book.timestamp())

    def test_apply_ask_diff(self):
        # Arrange
        order_book = OrderBook(0)

        # Act
        order_book.apply_ask_diff(1001.0, 15.0, 2, 1610000000001)

        # Assert
        self.assertEqual(1001.0, order_book.best_ask_price())
        self.assertEqual(15.0, order_book.best_ask_qty())
        self.assertEqual(2, order_book.last_update_id())
        self.assertEqual(1610000000001, order_book.timestamp())

    def test_apply_bid_and_ask_diffs(self):
        # Arrange
        order_book = OrderBook(0)

        # Act
        order_book.apply_bid_diff(1000.0, 20.0, 1, 1610000000000)
        order_book.apply_ask_diff(1001.0, 15.0, 2, 1610000000001)

        # Assert
        self.assertEqual(1000.0, order_book.best_bid_price())
        self.assertEqual(1001.0, order_book.best_ask_price())
        self.assertEqual(20.0, order_book.best_bid_qty())
        self.assertEqual(15.0, order_book.best_ask_qty())
        self.assertEqual(1.0, order_book.spread())
        self.assertEqual(2, order_book.last_update_id())
        self.assertEqual(1610000000001, order_book.timestamp())

    def test_apply_multiple_bid_diffs_results_in_correct_book(self):
        # Arrange
        order_book = OrderBook(0)

        # Act
        order_book.apply_bid_diff(1000.0, 20.0, 1, 1610000000000)
        order_book.apply_bid_diff(1002.0, 30.0, 2, 1610000000001)
        order_book.apply_bid_diff(1000.5, 20.0, 3, 1610000000002)
        order_book.apply_bid_diff(1001.0, 20.0, 4, 1610000000003)
        order_book.apply_bid_diff(999.0, 30.0, 5, 1610000000004)

        # Assert
        self.assertEqual(1002.0, order_book.best_bid_price())
        self.assertEqual(30.0, order_book.best_bid_qty())
        self.assertEqual(5, order_book.last_update_id())
        self.assertEqual(1610000000004, order_book.timestamp())

    def test_apply_multiple_ask_diffs_results_in_correct_book(self):
        # Arrange
        order_book = OrderBook(0)

        # Act
        order_book.apply_ask_diff(999.0, 20.0, 1, 1610000000000)
        order_book.apply_ask_diff(1001.0, 30.0, 2, 1610000000001)
        order_book.apply_ask_diff(997.5, 21.0, 3, 1610000000002)
        order_book.apply_ask_diff(1002.0, 200.0, 4, 1610000000003)
        order_book.apply_ask_diff(1003.0, 300.0, 5, 1610000000004)

        # Assert
        self.assertEqual(997.5, order_book.best_ask_price())
        self.assertEqual(21.0, order_book.best_ask_qty())
        self.assertEqual(5, order_book.last_update_id())
        self.assertEqual(1610000000004, order_book.timestamp())

    def test_apply_snapshot(self):
        # Arrange
        order_book = OrderBook(0)

        bids = np.asarray([
            [1002.0, 10.0],
            [1001.0, 20.0],
            [1000.8, 30.0],
            [1000.7, 40.0],
            [1000.6, 50.0],
            [1000.5, 60.0],
            [1000.4, 70.0],
            [1000.3, 80.0],
            [1000.2, 90.0],
            [1000.1, 100.0],
        ])

        asks = np.asarray([
            [1003.0, 10.0],
            [1004.0, 20.0],
            [1004.1, 30.0],
            [1004.2, 40.0],
            [1004.3, 50.0],
            [1004.4, 60.0],
            [1004.5, 70.0],
            [1004.6, 80.0],
            [1004.7, 90.0],
            [1004.8, 100.0],
        ])

        # Act
        order_book.apply_snapshot(bids, asks, 1, 1)

        # Assert
        self.assertEqual(1.0, order_book.spread())
        self.assertEqual(1002.0, order_book.best_bid_price())
        self.assertEqual(1003.0, order_book.best_ask_price())
        self.assertEqual(10.0, order_book.best_bid_qty())
        self.assertEqual(10.0, order_book.best_ask_qty())
        self.assertEqual(1004.2, order_book.buy_price_for_qty(100.0))
        self.assertEqual(1000.7, order_book.sell_price_for_qty(100.0))
