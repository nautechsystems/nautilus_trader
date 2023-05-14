# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.data.book import BookOrder
from nautilus_trader.model.data.book import OrderBookDelta
from nautilus_trader.model.data.book import OrderBookDeltas
from nautilus_trader.model.data.book import OrderBookSnapshot
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


AUDUSD = TestIdStubs.audusd_id()


class TestOrderBookSnapshot:
    def test_fully_qualified_name(self):
        # Arrange, Act, Assert
        assert (
            OrderBookSnapshot.fully_qualified_name()
            == "nautilus_trader.model.data.book:OrderBookSnapshot"
        )

    def test_hash_str_and_repr(self):
        # Arrange
        snapshot = OrderBookSnapshot(
            instrument_id=AUDUSD,
            bids=[[1010, 2], [1009, 1]],
            asks=[[1020, 2], [1021, 1]],
            ts_event=0,
            ts_init=0,
        )

        # Act, Assert
        assert isinstance(hash(snapshot), int)
        assert (
            str(snapshot)
            == "OrderBookSnapshot(instrument_id=AUD/USD.SIM, bids=[[1010, 2], [1009, 1]], asks=[[1020, 2], [1021, 1]], sequence=0, ts_event=0, ts_init=0)"  # noqa
        )
        assert (
            repr(snapshot)
            == "OrderBookSnapshot(instrument_id=AUD/USD.SIM, bids=[[1010, 2], [1009, 1]], asks=[[1020, 2], [1021, 1]], sequence=0, ts_event=0, ts_init=0)"  # noqa
        )

    def test_to_dict_returns_expected_dict(self):
        # Arrange
        snapshot = OrderBookSnapshot(
            instrument_id=AUDUSD,
            bids=[[1010, 2], [1009, 1]],
            asks=[[1020, 2], [1021, 1]],
            sequence=123456789,
            ts_event=0,
            ts_init=1_000_000_000,
        )

        # Act
        result = OrderBookSnapshot.to_dict(snapshot)

        # Assert
        assert result == {
            "type": "OrderBookSnapshot",
            "instrument_id": "AUD/USD.SIM",
            "bids": b"[[1010,2],[1009,1]]",
            "asks": b"[[1020,2],[1021,1]]",
            "sequence": 123456789,
            "ts_event": 0,
            "ts_init": 1_000_000_000,
        }

    def test_from_dict_returns_expected_tick(self):
        # Arrange
        snapshot = OrderBookSnapshot(
            instrument_id=AUDUSD,
            bids=[[1010, 2], [1009, 1]],
            asks=[[1020, 2], [1021, 1]],
            sequence=123456789,
            ts_event=0,
            ts_init=1_000_000_000,
        )

        # Act
        result = OrderBookSnapshot.from_dict(OrderBookSnapshot.to_dict(snapshot))

        # Assert
        assert result == snapshot


class TestOrderBookDelta:
    def test_fully_qualified_name(self):
        # Arrange, Act, Assert
        assert (
            OrderBookDelta.fully_qualified_name()
            == "nautilus_trader.model.data.book:OrderBookDelta"
        )

    def test_hash_str_and_repr(self):
        # Arrange
        order = BookOrder(price=10, size=5, side=OrderSide.BUY)
        delta = OrderBookDelta(
            instrument_id=AUDUSD,
            action=BookAction.ADD,
            order=order,
            sequence=123456789,
            ts_event=0,
            ts_init=1_000_000_000,
        )

        # Act, Assert
        assert isinstance(hash(delta), int)
        assert (
            str(delta)
            == f"OrderBookDelta(instrument_id=AUD/USD.SIM, action=ADD, order=BookOrder(10.0, 5.0, BUY, {order.order_id}), sequence=123456789, ts_event=0, ts_init=1000000000)"  # noqa
        )
        assert (
            repr(delta)
            == f"OrderBookDelta(instrument_id=AUD/USD.SIM, action=ADD, order=BookOrder(10.0, 5.0, BUY, {order.order_id}), sequence=123456789, ts_event=0, ts_init=1000000000)"  # noqa
        )

    def test_to_dict_returns_expected_dict(self):
        # Arrange
        order = BookOrder(price=10, size=5, side=OrderSide.BUY, order_id="1")
        delta = OrderBookDelta(
            instrument_id=AUDUSD,
            action=BookAction.ADD,
            order=order,
            ts_event=0,
            ts_init=0,
        )

        # Act
        result = OrderBookDelta.to_dict(delta)

        # Assert
        assert result == {
            "type": "OrderBookDelta",
            "instrument_id": "AUD/USD.SIM",
            "action": "ADD",
            "order_id": "1",
            "price": 10.0,
            "side": "BUY",
            "size": 5.0,
            "sequence": 0,
            "ts_event": 0,
            "ts_init": 0,
        }

    def test_from_dict_returns_expected_delta(self):
        # Arrange
        order = BookOrder(price=10, size=5, side=OrderSide.BUY)
        delta = OrderBookDelta(
            instrument_id=AUDUSD,
            action=BookAction.ADD,
            order=order,
            ts_event=0,
            ts_init=0,
        )

        # Act
        result = OrderBookDelta.from_dict(OrderBookDelta.to_dict(delta))

        # Assert
        assert result == delta

    def test_from_dict_returns_expected_clear(self):
        # Arrange
        delta = OrderBookDelta(
            instrument_id=AUDUSD,
            action=BookAction.CLEAR,
            order=None,
            ts_event=0,
            ts_init=0,
        )

        # Act
        result = OrderBookDelta.from_dict(OrderBookDelta.to_dict(delta))

        # Assert
        assert result == delta


class TestOrderBookDeltas:
    def test_fully_qualified_name(self):
        # Arrange, Act, Assert
        assert (
            OrderBookDeltas.fully_qualified_name()
            == "nautilus_trader.model.data.book:OrderBookDeltas"
        )

    def test_hash_str_and_repr(self):
        # Arrange
        order1 = BookOrder(price=10, size=5, side=OrderSide.BUY, order_id="1")
        delta1 = OrderBookDelta(
            instrument_id=AUDUSD,
            action=BookAction.ADD,
            order=order1,
            ts_event=0,
            ts_init=0,
        )

        order2 = BookOrder(price=10, size=15, side=OrderSide.BUY, order_id="2")
        delta2 = OrderBookDelta(
            instrument_id=AUDUSD,
            action=BookAction.ADD,
            order=order2,
            ts_event=0,
            ts_init=0,
        )

        deltas = OrderBookDeltas(
            instrument_id=AUDUSD,
            deltas=[delta1, delta2],
            ts_event=0,
            ts_init=0,
        )

        # Act, Assert
        assert isinstance(hash(deltas), int)
        assert (
            str(deltas)
            == "OrderBookDeltas(instrument_id=AUD/USD.SIM, [OrderBookDelta(instrument_id=AUD/USD.SIM, action=ADD, order=BookOrder(10.0, 5.0, BUY, 1), sequence=0, ts_event=0, ts_init=0), OrderBookDelta(instrument_id=AUD/USD.SIM, action=ADD, order=BookOrder(10.0, 15.0, BUY, 2), sequence=0, ts_event=0, ts_init=0)], sequence=0, ts_event=0, ts_init=0)"  # noqa
        )
        assert (
            repr(deltas)
            == "OrderBookDeltas(instrument_id=AUD/USD.SIM, [OrderBookDelta(instrument_id=AUD/USD.SIM, action=ADD, order=BookOrder(10.0, 5.0, BUY, 1), sequence=0, ts_event=0, ts_init=0), OrderBookDelta(instrument_id=AUD/USD.SIM, action=ADD, order=BookOrder(10.0, 15.0, BUY, 2), sequence=0, ts_event=0, ts_init=0)], sequence=0, ts_event=0, ts_init=0)"  # noqa
        )

    def test_to_dict_returns_expected_dict(self):
        # Arrange
        order1 = BookOrder(price=10, size=5, side=OrderSide.BUY, order_id="1")
        delta1 = OrderBookDelta(
            instrument_id=AUDUSD,
            action=BookAction.ADD,
            order=order1,
            ts_event=0,
            ts_init=0,
        )

        order2 = BookOrder(price=10, size=15, side=OrderSide.BUY, order_id="2")
        delta2 = OrderBookDelta(
            instrument_id=AUDUSD,
            action=BookAction.ADD,
            order=order2,
            ts_event=0,
            ts_init=0,
        )

        deltas = OrderBookDeltas(
            instrument_id=AUDUSD,
            deltas=[delta1, delta2],
            ts_event=0,
            ts_init=0,
        )

        # Act
        result = OrderBookDeltas.to_dict(deltas)

        # Assert
        assert result == {
            "type": "OrderBookDeltas",
            "instrument_id": "AUD/USD.SIM",
            "deltas": b'[{"type":"OrderBookDelta","instrument_id":"AUD/USD.SIM","action":"ADD","price":10.0,"size":5.0,"side":"BUY","order_id":"1","sequence":0,"ts_event":0,"ts_init":0},{"type":"OrderBookDelta","instrument_id":"AUD/USD.SIM","action":"ADD","price":10.0,"size":15.0,"side":"BUY","order_id":"2","sequence":0,"ts_event":0,"ts_init":0}]',  # noqa
            "sequence": 0,
            "ts_event": 0,
            "ts_init": 0,
        }

    def test_from_dict_returns_expected_tick(self):
        # Arrange
        order1 = BookOrder(price=10, size=5, side=OrderSide.BUY, order_id="1")
        delta1 = OrderBookDelta(
            instrument_id=AUDUSD,
            action=BookAction.ADD,
            order=order1,
            ts_event=0,
            ts_init=0,
        )

        order2 = BookOrder(price=10, size=15, side=OrderSide.BUY, order_id="2")
        delta2 = OrderBookDelta(
            instrument_id=AUDUSD,
            action=BookAction.ADD,
            order=order2,
            ts_event=0,
            ts_init=0,
        )

        deltas = OrderBookDeltas(
            instrument_id=AUDUSD,
            deltas=[delta1, delta2],
            ts_event=0,
            ts_init=0,
        )

        # Act
        result = OrderBookDeltas.from_dict(OrderBookDeltas.to_dict(deltas))

        # Assert
        assert result == deltas
