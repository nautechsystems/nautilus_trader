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

from nautilus_trader.model.order_book import OrderBook
from tests.test_kit.providers import TestInstrumentProvider


ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


class OrderBookTests(unittest.TestCase):
    def test_instantiation(self):
        # Arrange
        # Act
        order_book = OrderBook(
            instrument_id=ETHUSDT_BINANCE.id,
            level=2,
            depth=25,
            price_precision=2,
            size_precision=5,
            bids=[],
            asks=[],
            update_id=0,
            timestamp=0,
        )

        # Act
        # Assert
        self.assertEqual(2, order_book.level)
        self.assertEqual(25, order_book.depth)
        self.assertEqual(0, order_book.timestamp)
        self.assertEqual(0, order_book.update_id)

    def test_apply_snapshot(self):
        # Arrange
        order_book = OrderBook(
            instrument_id=ETHUSDT_BINANCE.id,
            level=2,
            depth=25,
            price_precision=2,
            size_precision=5,
            bids=[],
            asks=[],
            update_id=0,
            timestamp=0,
        )

        bids = [
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
        ]

        asks = [
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
        ]

        # Act
        order_book.apply_snapshot(bids, asks, 1, 1)

        # Assert
        self.assertEqual(bids, order_book.bids())
        self.assertEqual(asks, order_book.asks())
        self.assertEqual(1.0, order_book.spread())
        self.assertEqual(1002.0, order_book.best_bid_price())
        self.assertEqual(1003.0, order_book.best_ask_price())
        self.assertEqual(10.0, order_book.best_bid_qty())
        self.assertEqual(10.0, order_book.best_ask_qty())
