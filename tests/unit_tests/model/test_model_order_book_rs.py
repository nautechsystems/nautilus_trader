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
import numpy as np

from nautilus_trader.model.order_book_2 import OrderBook
from tests.test_kit.providers import TestInstrumentProvider


ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


# TODO: WIP
class OrderBookTests(unittest.TestCase):
    pass

    # def test_instantiation(self):
    #     pass
    #     # Arrange
    #     order_book = OrderBook(2)
    #
    #     print(order_book.spread())
    #     print(order_book.best_bid_price())
    #     print(order_book.best_ask_price())
    #     print(order_book.timestamp())
    #     print(order_book.last_update_id())

        # Act

        # Assert
        # self.assertEqual(ETHUSDT_BINANCE.symbol, order_book.symbol)
        # self.assertEqual(2, order_book.level)
        # self.assertEqual(2, order_book.price_precision)
        # self.assertEqual(5, order_book.size_precision)
        # self.assertEqual(0, order_book.timestamp)

    # TODO: WIP
    # def test_apply_snapshot(self):
    #     # Arrange
    #     order_book = OrderBook(0)
    #
    #     # Act
    #     order_book.apply_snapshot(
    #         np.asarray([[1000.0, 10.0]]),
    #         np.asarray([[1001.0, 20.0]]),
    #         1650000000000,
    #         1,
    #     )
