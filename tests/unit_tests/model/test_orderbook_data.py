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

from nautilus_trader.model.enums import BookLevel
from nautilus_trader.model.enums import DeltaType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.orderbook.data import Order
from nautilus_trader.model.orderbook.data import OrderBookDelta
from nautilus_trader.model.orderbook.data import OrderBookDeltas
from nautilus_trader.model.orderbook.data import OrderBookSnapshot
from tests.test_kit.stubs import TestStubs


AUDUSD = TestStubs.audusd_id()


class TestOrderBookSnapshot:
    def test_hash_str_and_repr(self):
        # Arrange
        snapshot = OrderBookSnapshot(
            instrument_id=AUDUSD,
            level=BookLevel.L2,
            bids=[[1010, 2], [1009, 1]],
            asks=[[1020, 2], [1021, 1]],
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        # Act, Assert
        assert isinstance(hash(snapshot), int)
        assert (
            str(snapshot)
            == "OrderBookSnapshot('AUD/USD.SIM', level=L2, bids=[[1010, 2], [1009, 1]], asks=[[1020, 2], [1021, 1]], ts_recv_ns=0)"
        )
        assert (
            repr(snapshot)
            == "OrderBookSnapshot('AUD/USD.SIM', level=L2, bids=[[1010, 2], [1009, 1]], asks=[[1020, 2], [1021, 1]], ts_recv_ns=0)"
        )

    def test_to_dict_returns_expected_dict(self):
        # Arrange
        snapshot = OrderBookSnapshot(
            instrument_id=AUDUSD,
            level=BookLevel.L2,
            bids=[[1010, 2], [1009, 1]],
            asks=[[1020, 2], [1021, 1]],
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        # Act
        result = OrderBookSnapshot.to_dict(snapshot)

        # Assert
        assert result == {
            "type": "OrderBookSnapshot",
            "instrument_id": "AUD/USD.SIM",
            "level": "L2",
            "bids": b"[[1010,2],[1009,1]]",
            "asks": b"[[1020,2],[1021,1]]",
            "ts_event_ns": 0,
            "ts_recv_ns": 0,
        }

    def test_from_dict_returns_expected_tick(self):
        # Arrange
        snapshot = OrderBookSnapshot(
            instrument_id=AUDUSD,
            level=BookLevel.L2,
            bids=[[1010, 2], [1009, 1]],
            asks=[[1020, 2], [1021, 1]],
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        # Act
        result = OrderBookSnapshot.from_dict(OrderBookSnapshot.to_dict(snapshot))

        # Assert
        assert result == snapshot


class TestOrderBookDelta:
    def test_hash_str_and_repr(self):
        # Arrange
        order = Order(price=10, size=5, side=OrderSide.BUY)
        delta = OrderBookDelta(
            instrument_id=AUDUSD,
            level=BookLevel.L2,
            delta_type=DeltaType.ADD,
            order=order,
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        # Act, Assert
        assert isinstance(hash(delta), int)
        assert (
            str(delta)
            == f"OrderBookDelta('AUD/USD.SIM', level=L2, delta_type=ADD, order=Order(10.0, 5.0, BUY, {order.id}), ts_recv_ns=0)"
        )
        assert (
            repr(delta)
            == f"OrderBookDelta('AUD/USD.SIM', level=L2, delta_type=ADD, order=Order(10.0, 5.0, BUY, {order.id}), ts_recv_ns=0)"
        )

    def test_to_dict_returns_expected_dict(self):
        # Arrange
        order = Order(price=10, size=5, side=OrderSide.BUY, id="1")
        delta = OrderBookDelta(
            instrument_id=AUDUSD,
            level=BookLevel.L2,
            delta_type=DeltaType.ADD,
            order=order,
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        # Act
        result = OrderBookDelta.to_dict(delta)

        # Assert
        assert result == {
            "type": "OrderBookDelta",
            "instrument_id": "AUD/USD.SIM",
            "level": "L2",
            "delta_type": "ADD",
            "order_id": "1",
            "order_price": 10.0,
            "order_side": "BUY",
            "order_size": 5.0,
            "ts_event_ns": 0,
            "ts_recv_ns": 0,
        }

    def test_from_dict_returns_expected_delta(self):
        # Arrange
        order = Order(price=10, size=5, side=OrderSide.BUY)
        delta = OrderBookDelta(
            instrument_id=AUDUSD,
            level=BookLevel.L2,
            delta_type=DeltaType.ADD,
            order=order,
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        # Act
        result = OrderBookDelta.from_dict(OrderBookDelta.to_dict(delta))

        # Assert
        assert result == delta

    def test_from_dict_returns_expected_clear(self):
        # Arrange
        delta = OrderBookDelta(
            instrument_id=AUDUSD,
            level=BookLevel.L2,
            delta_type=DeltaType.CLEAR,
            order=None,
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        # Act
        result = OrderBookDelta.from_dict(OrderBookDelta.to_dict(delta))

        # Assert
        assert result == delta


class TestOrderBookDeltas:
    def test_hash_str_and_repr(self):
        # Arrange
        order1 = Order(price=10, size=5, side=OrderSide.BUY, id="1")
        delta1 = OrderBookDelta(
            instrument_id=AUDUSD,
            level=BookLevel.L2,
            delta_type=DeltaType.ADD,
            order=order1,
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        order2 = Order(price=10, size=15, side=OrderSide.BUY, id="2")
        delta2 = OrderBookDelta(
            instrument_id=AUDUSD,
            level=BookLevel.L2,
            delta_type=DeltaType.ADD,
            order=order2,
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        deltas = OrderBookDeltas(
            instrument_id=AUDUSD,
            level=BookLevel.L2,
            deltas=[delta1, delta2],
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        # Act, Assert
        assert isinstance(hash(deltas), int)
        assert (
            str(deltas)
            == "OrderBookDeltas('AUD/USD.SIM', level=L2, [OrderBookDelta('AUD/USD.SIM', level=L2, delta_type=ADD, order=Order(10.0, 5.0, BUY, 1), ts_recv_ns=0), OrderBookDelta('AUD/USD.SIM', level=L2, delta_type=ADD, order=Order(10.0, 15.0, BUY, 2), ts_recv_ns=0)], ts_recv_ns=0)"  # noqa
        )
        assert (
            repr(deltas)
            == "OrderBookDeltas('AUD/USD.SIM', level=L2, [OrderBookDelta('AUD/USD.SIM', level=L2, delta_type=ADD, order=Order(10.0, 5.0, BUY, 1), ts_recv_ns=0), OrderBookDelta('AUD/USD.SIM', level=L2, delta_type=ADD, order=Order(10.0, 15.0, BUY, 2), ts_recv_ns=0)], ts_recv_ns=0)"  # noqa
        )

    def test_to_dict_returns_expected_dict(self):
        # Arrange
        order1 = Order(price=10, size=5, side=OrderSide.BUY, id="1")
        delta1 = OrderBookDelta(
            instrument_id=AUDUSD,
            level=BookLevel.L2,
            delta_type=DeltaType.ADD,
            order=order1,
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        order2 = Order(price=10, size=15, side=OrderSide.BUY, id="2")
        delta2 = OrderBookDelta(
            instrument_id=AUDUSD,
            level=BookLevel.L2,
            delta_type=DeltaType.ADD,
            order=order2,
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        deltas = OrderBookDeltas(
            instrument_id=AUDUSD,
            level=BookLevel.L2,
            deltas=[delta1, delta2],
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        # Act
        result = OrderBookDeltas.to_dict(deltas)

        # Assert
        assert result == {
            "type": "OrderBookDeltas",
            "instrument_id": "AUD/USD.SIM",
            "level": "L2",
            "deltas": b'[{"type":"OrderBookDelta","instrument_id":"AUD/USD.SIM","level":"L2","delta_type":"ADD","order_price":10.0,"order_size":5.0,"order_side":"BUY","order_id":"1","ts_event_ns":0,"ts_recv_ns":0},{"type":"OrderBookDelta","instrument_id":"AUD/USD.SIM","level":"L2","delta_type":"ADD","order_price":10.0,"order_size":15.0,"order_side":"BUY","order_id":"2","ts_event_ns":0,"ts_recv_ns":0}]',  # noqa
            "ts_event_ns": 0,
            "ts_recv_ns": 0,
        }

    def test_from_dict_returns_expected_tick(self):
        # Arrange
        order1 = Order(price=10, size=5, side=OrderSide.BUY, id="1")
        delta1 = OrderBookDelta(
            instrument_id=AUDUSD,
            level=BookLevel.L2,
            delta_type=DeltaType.ADD,
            order=order1,
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        order2 = Order(price=10, size=15, side=OrderSide.BUY, id="2")
        delta2 = OrderBookDelta(
            instrument_id=AUDUSD,
            level=BookLevel.L2,
            delta_type=DeltaType.ADD,
            order=order2,
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        deltas = OrderBookDeltas(
            instrument_id=AUDUSD,
            level=BookLevel.L2,
            deltas=[delta1, delta2],
            ts_event_ns=0,
            ts_recv_ns=0,
        )

        # Act
        result = OrderBookDeltas.from_dict(OrderBookDeltas.to_dict(deltas))

        # Assert
        assert result == deltas
