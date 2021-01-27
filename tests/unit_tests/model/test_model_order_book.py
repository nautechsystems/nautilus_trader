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

from decimal import Decimal
import unittest

import numpy as np

from nautilus_trader.model.order_book import OrderBook
from tests.test_kit.providers import TestInstrumentProvider


ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


class OrderBookTests(unittest.TestCase):

    def test_instantiation(self):
        # Arrange
        bids = np.asarray([[1550.15, 0.51], [1580.00, 1.20]])
        asks = np.asarray([[1552.15, 1.51], [1582.00, 2.20]])

        # Act
        order_book = OrderBook(
            symbol=ETHUSDT_BINANCE.symbol,
            level=2,
            price_precision=2,
            size_precision=5,
            bids=bids,
            asks=asks,
            timestamp=0,
        )

        # Assert
        self.assertEqual(ETHUSDT_BINANCE.symbol, order_book.symbol)
        self.assertEqual(2, order_book.level)
        self.assertEqual(2, order_book.price_precision)
        self.assertEqual(5, order_book.size_precision)
        self.assertEqual(0, order_book.timestamp)

    def test_str_and_repr(self):
        # Arrange
        bids = np.asarray([[1550.15, 0.51], [1580.00, 1.20]])
        asks = np.asarray([[1552.15, 1.51], [1582.00, 2.20]])

        # Act
        order_book = OrderBook(
            symbol=ETHUSDT_BINANCE.symbol,
            level=2,
            price_precision=2,
            size_precision=2,
            bids=bids,
            asks=asks,
            timestamp=0,
        )

        # Assert
        self.assertEqual("ETH/USDT.BINANCE, bids_len=2, asks_len=2, timestamp=0", str(order_book))
        self.assertEqual("OrderBook(ETH/USDT.BINANCE, bids_len=2, asks_len=2, timestamp=0)", repr(order_book))

    def test_bids_and_asks(self):
        # Arrange
        bids = np.asarray([[1550.15, 0.51], [1580.00, 1.20]])
        asks = np.asarray([[1552.15, 1.51], [1582.00, 2.20]])

        # Act
        order_book = OrderBook(
            symbol=ETHUSDT_BINANCE.symbol,
            level=2,
            price_precision=2,
            size_precision=2,
            bids=bids,
            asks=asks,
            timestamp=0,
        )

        # Assert
        self.assertEqual(ETHUSDT_BINANCE.symbol, order_book.symbol)
        self.assertEqual(0, order_book.timestamp)
        self.assertEqual([[1550.15, 0.51], [1580.00, 1.20]], order_book.bids())
        self.assertEqual([[1552.15, 1.51], [1582.00, 2.20]], order_book.asks())

    def test_update(self):
        # Arrange
        bids1 = np.asarray([[1550.15, 0.51], [1580.00, 1.20]])
        asks1 = np.asarray([[1552.15, 1.51], [1582.00, 2.20]])

        order_book = OrderBook(
            symbol=ETHUSDT_BINANCE.symbol,
            level=2,
            price_precision=2,
            size_precision=2,
            bids=bids1,
            asks=asks1,
            timestamp=0,
        )

        bids2 = np.asarray([[1551.00, 1.00], [1581.00, 2.00]])
        asks2 = np.asarray([[1553.00, 1.00], [1583.00, 2.00]])

        # Act
        order_book.update(bids2, asks2, 1)

        # Assert
        self.assertEqual(ETHUSDT_BINANCE.symbol, order_book.symbol)
        self.assertEqual(1, order_book.timestamp)
        self.assertEqual([[1551.00, 1.00], [1581.00, 2.00]], order_book.bids())
        self.assertEqual([[1553.00, 1.00], [1583.00, 2.00]], order_book.asks())

    def test_bids_and_asks_as_decimals(self):
        # Arrange
        bids = np.asarray([[1550.15, 0.51], [1580.00, 1.20]])
        asks = np.asarray([[1552.15, 1.51], [1582.00, 2.20]])

        # Act
        order_book = OrderBook(
            symbol=ETHUSDT_BINANCE.symbol,
            level=2,
            price_precision=2,
            size_precision=2,
            bids=bids,
            asks=asks,
            timestamp=0,
        )

        # Assert
        self.assertEqual(ETHUSDT_BINANCE.symbol, order_book.symbol)
        self.assertEqual(0, order_book.timestamp)
        self.assertEqual([Decimal('1550.15'), Decimal('0.51')], order_book.bids_as_decimals()[0])
        self.assertEqual([Decimal('1580.00'), Decimal('1.20')], order_book.bids_as_decimals()[1])
        self.assertEqual([Decimal('1552.15'), Decimal('1.51')], order_book.asks_as_decimals()[0])
        self.assertEqual([Decimal('1582.00'), Decimal('2.20')], order_book.asks_as_decimals()[1])
