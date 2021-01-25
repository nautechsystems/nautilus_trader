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

from nautilus_trader.model.order_book import OrderBook
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs
from tests.test_kit.stubs import UNIX_EPOCH


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy(TestStubs.symbol_audusd())


class OrderBookTests(unittest.TestCase):

    def test_instantiation(self):
        # Arrange
        bids = [[1550.15, 0.51], [1580.00, 1.20]]
        asks = [[1552.15, 1.51], [1582.00, 2.20]]

        # Act
        order_book = OrderBook(
            symbol=AUDUSD_SIM.symbol,
            level=2,
            bids=bids,
            asks=asks,
            timestamp=UNIX_EPOCH,
        )

        # Assert
        self.assertEqual(AUDUSD_SIM.symbol, order_book.symbol)
        self.assertEqual(UNIX_EPOCH, order_book.timestamp)
        self.assertEqual([1550.15, 0.51], order_book.bids[0])
        self.assertEqual([1552.15, 1.51], order_book.asks[0])
        self.assertEqual([1580.00, 1.20], order_book.bids[1])
        self.assertEqual([1582.00, 2.20], order_book.asks[1])

    def test_str_and_repr(self):
        # Arrange
        bids = [[1550.15, 0.51], [1580.00, 1.20]]
        asks = [[1552.15, 1.51], [1582.00, 2.20]]

        # Act
        order_book = OrderBook(
            symbol=AUDUSD_SIM.symbol,
            level=2,
            bids=bids,
            asks=asks,
            timestamp=UNIX_EPOCH,
        )

        # Assert
        self.assertEqual("AUD/USD.SIM,bids=[[1550.15, 0.51], [1580.0, 1.2]],asks=[[1552.15, 1.51], [1582.0, 2.2]]", str(order_book))
        self.assertEqual("OrderBook(AUD/USD.SIM,bids=[[1550.15, 0.51], [1580.0, 1.2]],asks=[[1552.15, 1.51], [1582.0, 2.2]])", repr(order_book))

    def test_from_floats_given_valid_data_returns_order_book(self):
        # Arrange
        bids = [[1550.15, 0.51], [1580.00, 1.20]]
        asks = [[1552.15, 1.51], [1582.00, 2.20]]

        # Act
        order_book = OrderBook.from_floats_py(
            symbol=AUDUSD_SIM.symbol,
            level=2,
            bids=bids,
            asks=asks,
            price_precision=2,
            size_precision=2,
            timestamp=UNIX_EPOCH,
        )

        # Assert
        self.assertEqual(AUDUSD_SIM.symbol, order_book.symbol)
        self.assertEqual(UNIX_EPOCH, order_book.timestamp)
        self.assertEqual((Decimal('1550.15'), Decimal('0.51')), order_book.bids[0])
        self.assertEqual((Decimal('1552.15'), Decimal('1.51')), order_book.asks[0])
        self.assertEqual((Decimal('1580.00'), Decimal('1.20')), order_book.bids[1])
        self.assertEqual((Decimal('1582.00'), Decimal('2.20')), order_book.asks[1])
