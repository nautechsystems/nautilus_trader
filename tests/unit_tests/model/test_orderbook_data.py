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

from nautilus_trader.model.enums import DeltaType
from nautilus_trader.model.enums import OrderBookLevel
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.orderbook.book import OrderBookDelta
from nautilus_trader.model.orderbook.book import OrderBookSnapshot
from nautilus_trader.model.orderbook.order import Order
from tests.test_kit.stubs import TestStubs


AUDUSD = TestStubs.audusd_id()


class TestOrderBookSnapshot:
    def test_repr(self):
        # Arrange
        snapshot = OrderBookSnapshot(
            instrument_id=AUDUSD,
            level=OrderBookLevel.L2,
            bids=[[1010, 2], [1009, 1]],
            asks=[[1020, 2], [1021, 1]],
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        # Act
        # Assert
        assert (
            repr(snapshot)
            == "OrderBookSnapshot('AUD/USD.SIM', level=L2, bids=[[1010, 2], [1009, 1]], asks=[[1020, 2], [1021, 1]], ts_recv_ns=0)"
        )


class TestOrderBookOperation:
    def test_repr(self):
        # Arrange
        order = Order(price=10, volume=5, side=OrderSide.BUY)
        op = OrderBookDelta(
            instrument_id=AUDUSD,
            level=OrderBookLevel.L2,
            delta_type=DeltaType.ADD,
            order=order,
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        print(repr(op))
        # Act
        # Assert
        assert (
            repr(op)
            == f"OrderBookDelta('AUD/USD.SIM', level=L2, delta_type=ADD, order=Order(10.0, 5.0, BUY, {order.id}), ts_recv_ns=0)"
        )
