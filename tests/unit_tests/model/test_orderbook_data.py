# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import pytest

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model import convert_to_raw_int
from nautilus_trader.model.data import NULL_ORDER
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import OrderBookDepth10
from nautilus_trader.model.data import capsule_to_data
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import RecordFlag
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.rust.data_pyo3 import TestDataProviderPyo3
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


AUDUSD = TestIdStubs.audusd_id()


@pytest.mark.parametrize(
    ("side"),
    [
        OrderSide.BUY,
        OrderSide.SELL,
    ],
)
def test_book_order_init(side: OrderSide) -> None:
    # Arrange, Act
    order = BookOrder(
        price=Price.from_str("100"),
        size=Quantity.from_str("10"),
        side=side,
        order_id=1,
    )

    # Assert
    assert order.side == side
    assert order.price == 100
    assert order.size == 10
    assert order.order_id == 1


def test_signed_size():
    order = BookOrder(
        price=Price.from_str("10.0"),
        size=Quantity.from_str("1"),
        side=OrderSide.BUY,
        order_id=1,
    )
    assert order.size == 1
    assert order.signed_size() == 1.0

    order = BookOrder(
        price=Price.from_str("10.0"),
        size=Quantity.from_str("5"),
        side=OrderSide.SELL,
        order_id=2,
    )
    assert order.size == 5
    assert order.signed_size() == -5.0

    order = BookOrder(
        price=Price.from_str("10.0"),
        size=Quantity.zero(),
        side=OrderSide.SELL,
        order_id=3,
    )
    assert order.size == 0.0
    assert order.signed_size() == 0.0


def test_exposure():
    order = BookOrder(
        price=Price.from_str("100.0"),
        size=Quantity.from_str("10"),
        side=OrderSide.BUY,
        order_id=1,
    )
    assert order.exposure() == 1000.0


def test_hash_str_and_repr():
    # Arrange
    order = BookOrder(
        price=Price.from_str("100.0"),
        size=Quantity.from_str("5"),
        side=OrderSide.BUY,
        order_id=1,
    )

    # Act, Assert
    assert isinstance(hash(order), int)
    assert str(order) == "BookOrder(side=BUY, price=100.0, size=5, order_id=1)"
    assert repr(order) == "BookOrder(side=BUY, price=100.0, size=5, order_id=1)"


def test_to_dict_returns_expected_dict():
    # Arrange
    order = BookOrder(
        price=Price.from_str("100.00"),
        size=Quantity.from_str("5"),
        side=OrderSide.BUY,
        order_id=1,
    )

    # Act
    result = BookOrder.to_dict(order)

    # Assert
    assert result == {
        "type": "BookOrder",
        "side": "BUY",
        "price": "100.00",
        "size": "5",
        "order_id": 1,
    }


def test_from_dict_returns_expected_order():
    # Arrange
    order = BookOrder(
        price=Price.from_str("100.0"),
        size=Quantity.from_str("5"),
        side=OrderSide.BUY,
        order_id=1,
    )

    # Act
    result = BookOrder.from_dict(BookOrder.to_dict(order))

    # Assert
    assert result == order


def test_book_order_from_raw() -> None:
    # Arrange
    price = 10.0
    price_precision = 1
    size = 5
    size_precision = 0

    # Act
    order = BookOrder.from_raw(
        side=OrderSide.BUY,
        price_raw=convert_to_raw_int(price, price_precision),
        price_prec=1,
        size_raw=convert_to_raw_int(size, size_precision),
        size_prec=0,
        order_id=1,
    )

    # Assert
    assert str(order) == "BookOrder(side=BUY, price=10.0, size=5, order_id=1)"


def test_delta_fully_qualified_name() -> None:
    # Arrange, Act, Assert
    assert OrderBookDelta.fully_qualified_name() == "nautilus_trader.model.data:OrderBookDelta"


def test_delta_from_raw() -> None:
    # Arrange
    price = 10.0
    price_precision = 1
    size = 5
    size_precision = 0

    # Act
    delta = OrderBookDelta.from_raw(
        instrument_id=AUDUSD,
        action=BookAction.ADD,
        side=OrderSide.BUY,
        price_raw=convert_to_raw_int(price, price_precision),
        price_prec=1,
        size_raw=convert_to_raw_int(size, size_precision),
        size_prec=0,
        order_id=1,
        flags=0,
        sequence=123456789,
        ts_event=5_000_000,
        ts_init=1_000_000_000,
    )

    # Assert
    assert (
        str(delta)
        == "OrderBookDelta(instrument_id=AUD/USD.SIM, action=ADD, order=BookOrder(side=BUY, price=10.0, size=5, order_id=1), flags=0, sequence=123456789, ts_event=5000000, ts_init=1000000000)"
    )


def test_delta_pickle_round_trip() -> None:
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

    # Act
    pickled = pickle.dumps(delta)
    unpickled = pickle.loads(pickled)  # noqa: S301 (pickle safe here)

    # Assert
    assert delta == unpickled


def test_delta_hash_str_and_repr() -> None:
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
        == "OrderBookDelta(instrument_id=AUD/USD.SIM, action=ADD, order=BookOrder(side=BUY, price=10.0, size=5, order_id=1), flags=0, sequence=123456789, ts_event=0, ts_init=1000000000)"
    )
    assert (
        repr(delta)
        == "OrderBookDelta(instrument_id=AUD/USD.SIM, action=ADD, order=BookOrder(side=BUY, price=10.0, size=5, order_id=1), flags=0, sequence=123456789, ts_event=0, ts_init=1000000000)"
    )


def test_delta_with_null_book_order() -> None:
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
        == "OrderBookDelta(instrument_id=AUD/USD.SIM, action=CLEAR, order=BookOrder(side=NO_ORDER_SIDE, price=0, size=0, order_id=0), flags=32, sequence=123456789, ts_event=0, ts_init=1000000000)"
    )
    assert (
        repr(delta)
        == "OrderBookDelta(instrument_id=AUD/USD.SIM, action=CLEAR, order=BookOrder(side=NO_ORDER_SIDE, price=0, size=0, order_id=0), flags=32, sequence=123456789, ts_event=0, ts_init=1000000000)"
    )


def test_delta_clear() -> None:
    # Arrange, Act
    delta = OrderBookDelta.clear(
        instrument_id=AUDUSD,
        sequence=42,
        ts_event=0,
        ts_init=1_000_000_000,
    )

    # Assert
    assert delta.action == BookAction.CLEAR
    assert delta.sequence == 42
    assert delta.ts_event == 0
    assert delta.ts_init == 1_000_000_000


def test_delta_to_dict_with_order_returns_expected_dict() -> None:
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


def test_delta_from_dict_returns_expected_delta() -> None:
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


def test_delta_from_dict_returns_expected_clear() -> None:
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


def test_deltas_fully_qualified_name() -> None:
    # Arrange, Act, Assert
    assert OrderBookDeltas.fully_qualified_name() == "nautilus_trader.model.data:OrderBookDeltas"


def test_deltas_pickle_round_trip() -> None:
    # Arrange
    deltas = TestDataStubs.order_book_deltas()

    # Act
    pickled = pickle.dumps(deltas)
    unpickled = pickle.loads(pickled)  # noqa: S301 (pickle safe here)

    # Assert
    assert deltas == unpickled
    assert len(deltas.deltas) == len(unpickled.deltas)


def test_deltas_to_pyo3() -> None:
    # Arrange
    deltas = TestDataStubs.order_book_deltas()

    # Act
    pyo3_deltas = deltas.to_pyo3()

    # Assert
    assert isinstance(pyo3_deltas, nautilus_pyo3.OrderBookDeltas)
    assert len(pyo3_deltas.deltas) == len(deltas.deltas)


def test_deltas_capsule_round_trip() -> None:
    # Arrange
    deltas = TestDataStubs.order_book_deltas()

    # Act
    pyo3_deltas = deltas.to_pyo3()
    capsule = pyo3_deltas.as_pycapsule()
    deltas = capsule_to_data(capsule)

    # Assert
    assert isinstance(pyo3_deltas, nautilus_pyo3.OrderBookDeltas)
    assert len(pyo3_deltas.deltas) == len(deltas.deltas)


def test_deltas_hash_str_and_repr() -> None:
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
    assert (
        str(deltas)
        == "OrderBookDeltas(instrument_id=AUD/USD.SIM, [OrderBookDelta(instrument_id=AUD/USD.SIM, action=ADD, order=BookOrder(side=BUY, price=10.0, size=5, order_id=1), flags=0, sequence=0, ts_event=0, ts_init=0), OrderBookDelta(instrument_id=AUD/USD.SIM, action=ADD, order=BookOrder(side=BUY, price=10.0, size=15, order_id=2), flags=0, sequence=1, ts_event=0, ts_init=0)], is_snapshot=False, sequence=1, ts_event=0, ts_init=0)"
    )
    assert (
        repr(deltas)
        == "OrderBookDeltas(instrument_id=AUD/USD.SIM, [OrderBookDelta(instrument_id=AUD/USD.SIM, action=ADD, order=BookOrder(side=BUY, price=10.0, size=5, order_id=1), flags=0, sequence=0, ts_event=0, ts_init=0), OrderBookDelta(instrument_id=AUD/USD.SIM, action=ADD, order=BookOrder(side=BUY, price=10.0, size=15, order_id=2), flags=0, sequence=1, ts_event=0, ts_init=0)], is_snapshot=False, sequence=1, ts_event=0, ts_init=0)"
    )


def test_deltas_batching() -> None:
    # Arrange
    delta1 = TestDataStubs.order_book_delta(flags=0)
    delta2 = TestDataStubs.order_book_delta(flags=RecordFlag.F_LAST)
    delta3 = TestDataStubs.order_book_delta(flags=0)
    delta4 = TestDataStubs.order_book_delta(flags=0)
    delta5 = TestDataStubs.order_book_delta(flags=RecordFlag.F_LAST)

    # Act
    batches = OrderBookDeltas.batch(
        [
            delta1,
            delta2,
            delta3,
            delta4,
            delta5,
        ],
    )

    # Assert
    assert len(batches) == 2
    assert isinstance(batches[0], OrderBookDeltas)
    assert isinstance(batches[1], OrderBookDeltas)


def test_deltas_batching_with_remainder() -> None:
    # Arrange
    delta1 = TestDataStubs.order_book_delta(flags=0)
    delta2 = TestDataStubs.order_book_delta(flags=RecordFlag.F_LAST)
    delta3 = TestDataStubs.order_book_delta(flags=0)
    delta4 = TestDataStubs.order_book_delta(flags=0)
    delta5 = TestDataStubs.order_book_delta(flags=RecordFlag.F_LAST)
    delta6 = TestDataStubs.order_book_delta(flags=0)
    delta7 = TestDataStubs.order_book_delta(flags=0)

    # Act
    batches = OrderBookDeltas.batch(
        [
            delta1,
            delta2,
            delta3,
            delta4,
            delta5,
            delta6,
            delta7,
        ],
    )

    # Assert
    assert len(batches) == 3
    assert isinstance(batches[0], OrderBookDeltas)
    assert isinstance(batches[1], OrderBookDeltas)
    assert isinstance(batches[2], OrderBookDeltas)


def test_deltas_to_dict_from_dict_round_trip() -> None:
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
        ts_init=1,
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
        ts_event=2,
        ts_init=3,
    )

    deltas = OrderBookDeltas(
        instrument_id=AUDUSD,
        deltas=[delta1, delta2],
    )

    # Act
    result = OrderBookDeltas.to_dict(deltas)

    # Assert
    assert OrderBookDeltas.from_dict(result) == deltas
    assert result == {
        "type": "OrderBookDeltas",
        "instrument_id": "AUD/USD.SIM",
        "deltas": [
            {
                "type": "OrderBookDelta",
                "instrument_id": "AUD/USD.SIM",
                "action": "ADD",
                "order": {"side": "BUY", "price": "10.0", "size": "5", "order_id": 1},
                "flags": 0,
                "sequence": 0,
                "ts_event": 0,
                "ts_init": 1,
            },
            {
                "type": "OrderBookDelta",
                "instrument_id": "AUD/USD.SIM",
                "action": "ADD",
                "order": {"side": "BUY", "price": "10.0", "size": "15", "order_id": 2},
                "flags": 0,
                "sequence": 1,
                "ts_event": 2,
                "ts_init": 3,
            },
        ],
    }


def test_deltas_from_dict_returns_expected_dict() -> None:
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


def test_deltas_from_pyo3():
    # Arrange
    pyo3_delta = TestDataProviderPyo3.order_book_delta()

    # Act
    delta = OrderBookDelta.from_pyo3(pyo3_delta)

    # Assert
    assert isinstance(delta, OrderBookDelta)


def test_deltas_from_pyo3_list():
    # Arrange
    pyo3_deltas = [TestDataProviderPyo3.order_book_delta()] * 1024

    # Act
    deltas = OrderBookDelta.from_pyo3_list(pyo3_deltas)

    # Assert
    assert len(deltas) == 1024
    assert isinstance(deltas[0], OrderBookDelta)


def test_depth10_fully_qualified_name() -> None:
    # Arrange, Act, Assert
    assert OrderBookDepth10.fully_qualified_name() == "nautilus_trader.model.data:OrderBookDepth10"


def test_depth10_new() -> None:
    # Arrange, Act
    instrument_id = TestIdStubs.aapl_xnas_id()
    depth = TestDataStubs.order_book_depth10(
        instrument_id=instrument_id,
        flags=0,
        sequence=1,
        ts_event=2,
        ts_init=3,
    )

    # Assert
    assert depth.instrument_id == instrument_id
    assert len(depth.bids) == 10
    assert len(depth.asks) == 10
    assert depth.flags == 0
    assert depth.sequence == 1
    assert depth.ts_event == 2
    assert depth.ts_init == 3


def test_depth10_partial_levels() -> None:
    # Arrange, Act
    instrument_id = TestIdStubs.aapl_xnas_id()
    depth = TestDataStubs.order_book_depth10(
        instrument_id=instrument_id,
        flags=0,
        sequence=1,
        ts_event=2,
        ts_init=3,
        levels=3,
    )

    # Assert
    assert depth.instrument_id == instrument_id
    assert len(depth.bids) == 10
    assert len(depth.asks) == 10
    assert depth.flags == 0
    assert depth.sequence == 1
    assert depth.ts_event == 2
    assert depth.ts_init == 3


def test_depth10_pickle_round_trip() -> None:
    # Arrange
    depth = TestDataStubs.order_book_depth10()

    # Act
    pickled = pickle.dumps(depth)
    unpickled = pickle.loads(pickled)  # noqa: S301 (pickle safe here)

    # Assert
    assert depth == unpickled


def test_depth10_hash_str_repr() -> None:
    # Arrange
    depth = TestDataStubs.order_book_depth10(
        flags=0,
        sequence=1,
        ts_event=2,
        ts_init=3,
    )

    # Act, Assert
    assert isinstance(hash(depth), int)
    assert (
        str(depth)
        == "OrderBookDepth10(instrument_id=AAPL.XNAS, bids=[BookOrder(side=BUY, price=99.00, size=100, order_id=1), BookOrder(side=BUY, price=98.00, size=200, order_id=2), BookOrder(side=BUY, price=97.00, size=300, order_id=3), BookOrder(side=BUY, price=96.00, size=400, order_id=4), BookOrder(side=BUY, price=95.00, size=500, order_id=5), BookOrder(side=BUY, price=94.00, size=600, order_id=6), BookOrder(side=BUY, price=93.00, size=700, order_id=7), BookOrder(side=BUY, price=92.00, size=800, order_id=8), BookOrder(side=BUY, price=91.00, size=900, order_id=9), BookOrder(side=BUY, price=90.00, size=1000, order_id=10)], asks=[BookOrder(side=SELL, price=100.00, size=100, order_id=11), BookOrder(side=SELL, price=101.00, size=200, order_id=12), BookOrder(side=SELL, price=102.00, size=300, order_id=13), BookOrder(side=SELL, price=103.00, size=400, order_id=14), BookOrder(side=SELL, price=104.00, size=500, order_id=15), BookOrder(side=SELL, price=105.00, size=600, order_id=16), BookOrder(side=SELL, price=106.00, size=700, order_id=17), BookOrder(side=SELL, price=107.00, size=800, order_id=18), BookOrder(side=SELL, price=108.00, size=900, order_id=19), BookOrder(side=SELL, price=109.00, size=1000, order_id=20)], bid_counts=[1, 1, 1, 1, 1, 1, 1, 1, 1, 1], ask_counts=[1, 1, 1, 1, 1, 1, 1, 1, 1, 1], flags=0, sequence=1, ts_event=2, ts_init=3)"
    )
    assert (
        repr(depth)
        == "OrderBookDepth10(instrument_id=AAPL.XNAS, bids=[BookOrder(side=BUY, price=99.00, size=100, order_id=1), BookOrder(side=BUY, price=98.00, size=200, order_id=2), BookOrder(side=BUY, price=97.00, size=300, order_id=3), BookOrder(side=BUY, price=96.00, size=400, order_id=4), BookOrder(side=BUY, price=95.00, size=500, order_id=5), BookOrder(side=BUY, price=94.00, size=600, order_id=6), BookOrder(side=BUY, price=93.00, size=700, order_id=7), BookOrder(side=BUY, price=92.00, size=800, order_id=8), BookOrder(side=BUY, price=91.00, size=900, order_id=9), BookOrder(side=BUY, price=90.00, size=1000, order_id=10)], asks=[BookOrder(side=SELL, price=100.00, size=100, order_id=11), BookOrder(side=SELL, price=101.00, size=200, order_id=12), BookOrder(side=SELL, price=102.00, size=300, order_id=13), BookOrder(side=SELL, price=103.00, size=400, order_id=14), BookOrder(side=SELL, price=104.00, size=500, order_id=15), BookOrder(side=SELL, price=105.00, size=600, order_id=16), BookOrder(side=SELL, price=106.00, size=700, order_id=17), BookOrder(side=SELL, price=107.00, size=800, order_id=18), BookOrder(side=SELL, price=108.00, size=900, order_id=19), BookOrder(side=SELL, price=109.00, size=1000, order_id=20)], bid_counts=[1, 1, 1, 1, 1, 1, 1, 1, 1, 1], ask_counts=[1, 1, 1, 1, 1, 1, 1, 1, 1, 1], flags=0, sequence=1, ts_event=2, ts_init=3)"
    )


def test_depth10_to_dict_from_dict_round_trip() -> None:
    # Arrange
    depth = TestDataStubs.order_book_depth10(
        flags=0,
        sequence=1,
        ts_event=2,
        ts_init=3,
    )

    # Act
    result = OrderBookDepth10.to_dict(depth)

    # Assert
    assert OrderBookDepth10.from_dict(result) == depth
    assert result == {
        "type": "OrderBookDepth10",
        "instrument_id": "AAPL.XNAS",
        "bids": [
            {"type": "BookOrder", "side": "BUY", "price": "99.00", "size": "100", "order_id": 1},
            {"type": "BookOrder", "side": "BUY", "price": "98.00", "size": "200", "order_id": 2},
            {"type": "BookOrder", "side": "BUY", "price": "97.00", "size": "300", "order_id": 3},
            {"type": "BookOrder", "side": "BUY", "price": "96.00", "size": "400", "order_id": 4},
            {"type": "BookOrder", "side": "BUY", "price": "95.00", "size": "500", "order_id": 5},
            {"type": "BookOrder", "side": "BUY", "price": "94.00", "size": "600", "order_id": 6},
            {"type": "BookOrder", "side": "BUY", "price": "93.00", "size": "700", "order_id": 7},
            {"type": "BookOrder", "side": "BUY", "price": "92.00", "size": "800", "order_id": 8},
            {"type": "BookOrder", "side": "BUY", "price": "91.00", "size": "900", "order_id": 9},
            {"type": "BookOrder", "side": "BUY", "price": "90.00", "size": "1000", "order_id": 10},
        ],
        "asks": [
            {"type": "BookOrder", "side": "SELL", "price": "100.00", "size": "100", "order_id": 11},
            {"type": "BookOrder", "side": "SELL", "price": "101.00", "size": "200", "order_id": 12},
            {"type": "BookOrder", "side": "SELL", "price": "102.00", "size": "300", "order_id": 13},
            {"type": "BookOrder", "side": "SELL", "price": "103.00", "size": "400", "order_id": 14},
            {"type": "BookOrder", "side": "SELL", "price": "104.00", "size": "500", "order_id": 15},
            {"type": "BookOrder", "side": "SELL", "price": "105.00", "size": "600", "order_id": 16},
            {"type": "BookOrder", "side": "SELL", "price": "106.00", "size": "700", "order_id": 17},
            {"type": "BookOrder", "side": "SELL", "price": "107.00", "size": "800", "order_id": 18},
            {"type": "BookOrder", "side": "SELL", "price": "108.00", "size": "900", "order_id": 19},
            {
                "type": "BookOrder",
                "side": "SELL",
                "price": "109.00",
                "size": "1000",
                "order_id": 20,
            },
        ],
        "bid_counts": [1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
        "ask_counts": [1, 1, 1, 1, 1, 1, 1, 1, 1, 1],
        "flags": 0,
        "sequence": 1,
        "ts_event": 2,
        "ts_init": 3,
    }


def test_depth10_from_pyo3():
    # Arrange
    pyo3_depth = nautilus_pyo3.OrderBookDepth10.get_stub()

    # Act
    depth = OrderBookDepth10.from_pyo3(pyo3_depth)

    # Assert
    assert isinstance(depth, OrderBookDepth10)
