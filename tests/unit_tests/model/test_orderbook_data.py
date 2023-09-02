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

import pickle

from nautilus_trader.model.data import NULL_ORDER
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


AUDUSD = TestIdStubs.audusd_id()


def test_book_order_pickle_round_trip():
    # Arrange
    order = BookOrder(
        side=OrderSide.BUY,
        price=Price.from_str("10.0"),
        size=Quantity.from_str("5"),
        order_id=1,
    )

    # Act
    pickled = pickle.dumps(order)
    unpickled = pickle.loads(pickled)  # noqa

    # Assert
    assert order == unpickled


class TestOrderBookDelta:
    def test_fully_qualified_name(self):
        # Arrange, Act, Assert
        assert (
            OrderBookDelta.fully_qualified_name()
            == "nautilus_trader.model.data.book:OrderBookDelta"
        )

    def test_pickle_round_trip(self):
        order = BookOrder(
            side=OrderSide.BUY,
            price=Price.from_str("10.0"),
            size=Quantity.from_str("5"),
            order_id=1,
        )

        delta = OrderBookDelta(
            instrument_id=AUDUSD,
            action=BookAction.ADD,
            order=order,
            flags=0,
            sequence=123456789,
            ts_event=0,
            ts_init=1_000_000_000,
        )

        # Act
        pickled = pickle.dumps(delta)
        unpickled = pickle.loads(pickled)  # noqa

        # Assert
        assert delta == unpickled

    def test_hash_str_and_repr(self):
        # Arrange
        order = BookOrder(
            side=OrderSide.BUY,
            price=Price.from_str("10.0"),
            size=Quantity.from_str("5"),
            order_id=1,
        )

        delta = OrderBookDelta(
            instrument_id=AUDUSD,
            action=BookAction.ADD,
            order=order,
            flags=0,
            sequence=123456789,
            ts_event=0,
            ts_init=1_000_000_000,
        )

        # Act, Assert
        assert isinstance(hash(delta), int)
        assert (
            str(delta)
            == f"OrderBookDelta(instrument_id=AUD/USD.SIM, action=ADD, order=BookOrder {{ side: Buy, price: 10.0, size: 5, order_id: 1 }}, flags=0, sequence=123456789, ts_event=0, ts_init=1000000000)"  # noqa
        )
        assert (
            repr(delta)
            == f"OrderBookDelta(instrument_id=AUD/USD.SIM, action=ADD, order=BookOrder {{ side: Buy, price: 10.0, size: 5, order_id: 1 }}, flags=0, sequence=123456789, ts_event=0, ts_init=1000000000)"  # noqa
        )

    def test_with_null_book_order(self):
        # Arrange
        delta = OrderBookDelta(
            instrument_id=AUDUSD,
            action=BookAction.CLEAR,
            order=NULL_ORDER,
            flags=32,
            sequence=123456789,
            ts_event=0,
            ts_init=1_000_000_000,
        )

        # Act, Assert
        assert isinstance(hash(delta), int)
        assert (
            str(delta)
            == f"OrderBookDelta(instrument_id=AUD/USD.SIM, action=CLEAR, order=BookOrder {{ side: NoOrderSide, price: 0, size: 0, order_id: 0 }}, flags=32, sequence=123456789, ts_event=0, ts_init=1000000000)"  # noqa
        )
        assert (
            repr(delta)
            == f"OrderBookDelta(instrument_id=AUD/USD.SIM, action=CLEAR, order=BookOrder {{ side: NoOrderSide, price: 0, size: 0, order_id: 0 }}, flags=32, sequence=123456789, ts_event=0, ts_init=1000000000)"  # noqa
        )

    def test_clear_delta(self):
        # Arrange, Act
        delta = OrderBookDelta.clear(
            instrument_id=AUDUSD,
            ts_event=0,
            ts_init=1_000_000_000,
            sequence=42,
        )

        # Assert
        assert delta.action == BookAction.CLEAR
        assert delta.sequence == 42
        assert delta.ts_event == 0
        assert delta.ts_init == 1_000_000_000

    def test_to_dict_with_order_returns_expected_dict(self):
        # Arrange
        order = BookOrder(
            side=OrderSide.BUY,
            price=Price.from_str("10.0"),
            size=Quantity.from_str("5"),
            order_id=1,
        )

        delta = OrderBookDelta(
            instrument_id=AUDUSD,
            action=BookAction.ADD,
            order=order,
            flags=0,
            sequence=3,
            ts_event=1,
            ts_init=2,
        )

        # Act
        result = OrderBookDelta.to_dict(delta)

        # Assert
        assert result == {
            "type": "OrderBookDelta",
            "instrument_id": "AUD/USD.SIM",
            "action": "ADD",
            "order": {
                "side": "BUY",
                "price": "10.0",
                "size": "5",
                "order_id": 1,
            },
            "flags": 0,
            "sequence": 3,
            "ts_event": 1,
            "ts_init": 2,
        }

    def test_from_dict_returns_expected_delta(self):
        # Arrange
        order = BookOrder(
            side=OrderSide.BUY,
            price=Price.from_str("10.0"),
            size=Quantity.from_str("5"),
            order_id=1,
        )

        delta = OrderBookDelta(
            instrument_id=AUDUSD,
            action=BookAction.ADD,
            order=order,
            flags=0,
            sequence=3,
            ts_event=1,
            ts_init=2,
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
            flags=0,
            sequence=3,
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
        order1 = BookOrder(
            side=OrderSide.BUY,
            price=Price.from_str("10.0"),
            size=Quantity.from_str("5"),
            order_id=1,
        )

        delta1 = OrderBookDelta(
            instrument_id=AUDUSD,
            action=BookAction.ADD,
            order=order1,
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )

        order2 = BookOrder(
            side=OrderSide.BUY,
            price=Price.from_str("10.0"),
            size=Quantity.from_str("15"),
            order_id=2,
        )

        delta2 = OrderBookDelta(
            instrument_id=AUDUSD,
            action=BookAction.ADD,
            order=order2,
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )

        deltas = OrderBookDeltas(
            instrument_id=AUDUSD,
            deltas=[delta1, delta2],
        )

        # Act, Assert
        assert isinstance(hash(deltas), int)

        # TODO(cs): String format TBD
        # assert (
        #     str(deltas)
        #     == "OrderBookDeltas(instrument_id=AUD/USD.SIM, [OrderBookDelta(instrument_id=AUD/USD.SIM, action=ADD, order=BookOrder(10.0, 5.0, BUY, 1), sequence=0, ts_event=0, ts_init=0), OrderBookDelta(instrument_id=AUD/USD.SIM, action=ADD, order=BookOrder(10.0, 15.0, BUY, 2), sequence=0, ts_event=0, ts_init=0)], sequence=0, ts_event=0, ts_init=0)"  # noqa
        # )
        # assert (
        #     repr(deltas)
        #     == "OrderBookDeltas(instrument_id=AUD/USD.SIM, [OrderBookDelta(instrument_id=AUD/USD.SIM, action=ADD, order=BookOrder(10.0, 5.0, BUY, 1), sequence=0, ts_event=0, ts_init=0), OrderBookDelta(instrument_id=AUD/USD.SIM, action=ADD, order=BookOrder(10.0, 15.0, BUY, 2), sequence=0, ts_event=0, ts_init=0)], sequence=0, ts_event=0, ts_init=0)"  # noqa
        # )

    def test_to_dict(self):
        # Arrange
        order1 = BookOrder(
            side=OrderSide.BUY,
            price=Price.from_str("10.0"),
            size=Quantity.from_str("5"),
            order_id=1,
        )

        delta1 = OrderBookDelta(
            instrument_id=AUDUSD,
            action=BookAction.ADD,
            order=order1,
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )

        order2 = BookOrder(
            side=OrderSide.BUY,
            price=Price.from_str("10.0"),
            size=Quantity.from_str("15"),
            order_id=2,
        )

        delta2 = OrderBookDelta(
            instrument_id=AUDUSD,
            action=BookAction.ADD,
            order=order2,
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )

        deltas = OrderBookDeltas(
            instrument_id=AUDUSD,
            deltas=[delta1, delta2],
        )

        # Act
        result = OrderBookDeltas.to_dict(deltas)

        # Assert
        # TODO(cs): TBD
        assert result
        # assert result == {
        #     "type": "OrderBookDeltas",
        #     "instrument_id": "AUD/USD.SIM",
        #     "deltas": b'[{"type":"OrderBookDelta","instrument_id":"AUD/USD.SIM","action":"ADD","price":10.0,"size":5.0,"side":"BUY","order_id":"1","sequence":0,"ts_event":0,"ts_init":0},{"type":"OrderBookDelta","instrument_id":"AUD/USD.SIM","action":"ADD","price":10.0,"size":15.0,"side":"BUY","order_id":"2","sequence":0,"ts_event":0,"ts_init":0}]',  # noqa
        #     "sequence": 0,
        #     "ts_event": 0,
        #     "ts_init": 0,
        # }

    def test_from_dict_returns_expected_dict(self):
        # Arrange
        order1 = BookOrder(
            side=OrderSide.BUY,
            price=Price.from_str("10.0"),
            size=Quantity.from_str("5"),
            order_id=1,
        )

        delta1 = OrderBookDelta(
            instrument_id=AUDUSD,
            action=BookAction.ADD,
            order=order1,
            flags=0,
            sequence=0,
            ts_event=0,
            ts_init=0,
        )

        order2 = BookOrder(
            side=OrderSide.BUY,
            price=Price.from_str("10.0"),
            size=Quantity.from_str("15"),
            order_id=2,
        )

        delta2 = OrderBookDelta(
            instrument_id=AUDUSD,
            action=BookAction.ADD,
            order=order2,
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )

        deltas = OrderBookDeltas(
            instrument_id=AUDUSD,
            deltas=[delta1, delta2],
        )

        # Act
        result = OrderBookDeltas.from_dict(OrderBookDeltas.to_dict(deltas))

        # Assert
        assert result == deltas
