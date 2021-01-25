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
from tests.test_kit.stubs import TestStubs
from tests.test_kit.stubs import UNIX_EPOCH


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy(TestStubs.symbol_audusd())


# TODO: WIP (more tests to add)
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

    def test_tick_str_and_repr(self):
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
