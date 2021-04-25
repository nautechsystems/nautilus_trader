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

import pandas as pd
import pytest

from nautilus_trader.common.clock import TestClock
from nautilus_trader.model.enums import OrderBookDeltaType
from nautilus_trader.model.enums import OrderBookLevel
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.objects import Price
from nautilus_trader.model.orderbook.book import L1OrderBook
from nautilus_trader.model.orderbook.book import L2OrderBook
from nautilus_trader.model.orderbook.book import L3OrderBook
from nautilus_trader.model.orderbook.book import OrderBook
from nautilus_trader.model.orderbook.book import OrderBookDelta
from nautilus_trader.model.orderbook.book import OrderBookDeltas
from nautilus_trader.model.orderbook.book import OrderBookSnapshot
from nautilus_trader.model.orderbook.ladder import Ladder
from nautilus_trader.model.orderbook.order import Order
from tests.test_kit.stubs import TestStubs


AUDUSD = TestStubs.audusd_id()


@pytest.fixture(scope="function")
def empty_l2_book():
    return L2OrderBook(
        instrument_id=TestStubs.audusd_id(),
        price_precision=5,
        size_precision=0,
    )


@pytest.fixture(scope="function")
def sample_book():
    ob = L3OrderBook(
        instrument_id=TestStubs.audusd_id(),
        price_precision=5,
        size_precision=0,
    )
    orders = [
        Order(price=0.90000, volume=20.0, side=OrderSide.SELL),
        Order(price=0.88700, volume=10.0, side=OrderSide.SELL),
        Order(price=0.88600, volume=5.0, side=OrderSide.SELL),
        Order(price=0.83000, volume=4.0, side=OrderSide.BUY),
        Order(price=0.82000, volume=1.0, side=OrderSide.BUY),
    ]
    for order in orders:
        ob.add(order)
    return ob


@pytest.fixture(scope="function")
def clock():
    return TestClock()


def test_instantiate_base_class_directly_raises_value_error():
    # Arrange
    # Act
    # Assert
    with pytest.raises(RuntimeError):
        OrderBook(
            instrument_id=AUDUSD,
            level=OrderBookLevel.L2,
            price_precision=5,
            size_precision=0,
        )


def test_create_level_1_order_book():
    # Arrange
    # Act
    book = OrderBook.create(
        instrument_id=AUDUSD,
        level=OrderBookLevel.L1,
        price_precision=2,
        size_precision=2,
    )

    # Assert
    assert isinstance(book, L1OrderBook)
    assert book.level == OrderBookLevel.L1
    assert isinstance(book.bids, Ladder) and isinstance(book.asks, Ladder)
    assert book.bids.reverse
    assert not book.asks.reverse
    assert book.timestamp_ns == 0


def test_create_level_2_order_book():
    # Arrange
    # Act
    book = OrderBook.create(
        instrument_id=AUDUSD,
        level=OrderBookLevel.L2,
        price_precision=2,
        size_precision=2,
    )

    # Assert
    assert isinstance(book, L2OrderBook)
    assert book.level == OrderBookLevel.L2
    assert isinstance(book.bids, Ladder) and isinstance(book.asks, Ladder)
    assert book.bids.reverse
    assert not book.asks.reverse


def test_create_level_3_order_book():
    # Arrange
    # Act
    book = OrderBook.create(
        instrument_id=AUDUSD,
        level=OrderBookLevel.L3,
        price_precision=2,
        size_precision=2,
    )

    # Assert
    assert isinstance(book, L3OrderBook)
    assert book.level == OrderBookLevel.L3
    assert isinstance(book.bids, Ladder) and isinstance(book.asks, Ladder)
    assert book.bids.reverse
    assert not book.asks.reverse


def test_create_level_fail():
    # Arrange
    # Act
    # Assert
    with pytest.raises(ValueError):
        OrderBook.create(
            instrument_id=AUDUSD,
            level=0,
            price_precision=2,
            size_precision=2,
        )


def test_best_bid_or_ask_price_with_no_orders_returns_none():
    # Arrange
    book = OrderBook.create(
        instrument_id=AUDUSD,
        level=OrderBookLevel.L2,
        price_precision=2,
        size_precision=2,
    )

    # Act
    # Assert
    assert book.best_bid_price() is None
    assert book.best_ask_price() is None


def test_best_bid_or_ask_qty_with_no_orders_returns_none():
    # Arrange
    book = OrderBook.create(
        instrument_id=AUDUSD,
        level=OrderBookLevel.L2,
        price_precision=2,
        size_precision=2,
    )

    # Act
    # Assert
    assert book.best_bid_qty() is None
    assert book.best_ask_qty() is None


def test_spread_with_no_orders_returns_none():
    # Arrange
    book = OrderBook.create(
        instrument_id=AUDUSD,
        level=OrderBookLevel.L2,
        price_precision=2,
        size_precision=2,
    )

    # Act
    # Assert
    assert book.spread() is None


def test_add_orders_to_book():
    # Arrange
    book = OrderBook.create(
        instrument_id=AUDUSD,
        level=OrderBookLevel.L2,
        price_precision=2,
        size_precision=2,
    )

    # Act
    book.add(Order(price=10.0, volume=5.0, side=OrderSide.BUY))
    book.add(Order(price=11.0, volume=6.0, side=OrderSide.SELL))

    # Assert
    assert book.best_bid_price() == 10.0
    assert book.best_ask_price() == 11.0
    assert book.best_bid_qty() == 5.0
    assert book.best_ask_qty() == 6.0
    assert book.spread() == 1


def test_repr():
    book = OrderBook.create(
        instrument_id=AUDUSD,
        level=OrderBookLevel.L2,
        price_precision=2,
        size_precision=2,
    )

    # Act
    book.add(Order(price=10.0, volume=5.0, side=OrderSide.BUY))
    book.add(Order(price=11.0, volume=6.0, side=OrderSide.SELL))

    # Assert
    assert isinstance(repr(book), str)  # <-- calls pprint internally


def test_pprint_when_no_orders():
    ob = L2OrderBook(
        instrument_id=TestStubs.audusd_id(),
        price_precision=5,
        size_precision=0,
    )
    result = ob.pprint()

    assert "" == result


def test_pprint_full_book(sample_book):
    result = sample_book.pprint()
    print(result)
    expected = """bids     price   asks
------  -------  ------
        0.90000  [20.0]
        0.88700  [10.0]
        0.88600  [5.0]
[4.0]   0.83000
[1.0]   0.82000"""
    assert expected == result


def test_add(empty_l2_book):
    empty_l2_book.add(Order(price=10.0, volume=5.0, side=OrderSide.BUY))
    assert empty_l2_book.bids.top().price == 10.0


def test_add_l1_fails():
    book = OrderBook.create(
        instrument_id=TestStubs.audusd_id(),
        level=OrderBookLevel.L1,
        price_precision=5,
        size_precision=0,
    )
    with pytest.raises(NotImplementedError):
        book.add(TestStubs.order(price=10.0, side=OrderSide.BUY))


def test_delete_l1():
    book = OrderBook.create(
        instrument_id=TestStubs.audusd_id(),
        level=OrderBookLevel.L1,
        price_precision=5,
        size_precision=0,
    )
    order = TestStubs.order(price=10.0, side=OrderSide.BUY)
    book.update(order)
    book.delete(order)


def test_top(empty_l2_book):
    empty_l2_book.add(Order(price=10.0, volume=5.0, side=OrderSide.BUY))
    empty_l2_book.add(Order(price=20.0, volume=5.0, side=OrderSide.BUY))
    empty_l2_book.add(Order(price=5.0, volume=5.0, side=OrderSide.BUY))
    empty_l2_book.add(Order(price=25.0, volume=5.0, side=OrderSide.SELL))
    empty_l2_book.add(Order(price=30.0, volume=5.0, side=OrderSide.SELL))
    empty_l2_book.add(Order(price=21.0, volume=5.0, side=OrderSide.SELL))
    assert empty_l2_book.best_bid_level().price == 20
    assert empty_l2_book.best_ask_level().price == 21


def test_check_integrity_empty(empty_l2_book):
    empty_l2_book.check_integrity()


def test_check_integrity_shallow(empty_l2_book):
    empty_l2_book.add(Order(price=10.0, volume=5.0, side=OrderSide.SELL))
    empty_l2_book.check_integrity()
    empty_l2_book.add(Order(price=20.0, volume=5.0, side=OrderSide.BUY))

    with pytest.raises(AssertionError):
        empty_l2_book.check_integrity()


def test_check_integrity_deep(empty_l2_book):
    empty_l2_book.add(Order(price=10.0, volume=5, side=OrderSide.BUY))
    empty_l2_book.add(Order(price=5.0, volume=5, side=OrderSide.BUY))
    empty_l2_book.check_integrity()


def test_orderbook_snapshot(empty_l2_book):
    snapshot = OrderBookSnapshot(
        instrument_id=empty_l2_book.instrument_id,
        level=OrderBookLevel.L2,
        bids=[[1550.15, 0.51], [1580.00, 1.20]],
        asks=[[1552.15, 1.51], [1582.00, 2.20]],
        timestamp_ns=0,
    )
    empty_l2_book.apply_snapshot(snapshot)
    assert empty_l2_book.best_bid_price() == 1580.0
    assert empty_l2_book.best_ask_price() == 1552.15


def test_orderbook_operation_update(empty_l2_book, clock):
    delta = OrderBookDelta(
        instrument_id=TestStubs.audusd_id(),
        level=OrderBookLevel.L2,
        delta_type=OrderBookDeltaType.UPDATE,
        order=Order(
            0.5814,
            672.45,
            OrderSide.SELL,
            "4a25c3f6-76e7-7584-c5a3-4ec84808e240",
        ),
        timestamp_ns=clock.timestamp(),
    )
    empty_l2_book.apply_delta(delta)
    assert empty_l2_book.best_ask_price() == 0.5814


def test_orderbook_operation_add(empty_l2_book, clock):
    delta = OrderBookDelta(
        instrument_id=TestStubs.audusd_id(),
        level=OrderBookLevel.L2,
        delta_type=OrderBookDeltaType.ADD,
        order=Order(
            0.5900,
            672.45,
            OrderSide.SELL,
            "4a25c3f6-76e7-7584-c5a3-4ec84808e240",
        ),
        timestamp_ns=clock.timestamp(),
    )
    empty_l2_book.apply_delta(delta)
    assert empty_l2_book.best_ask_price() == 0.59


def test_orderbook_operations(empty_l2_book):
    delta = OrderBookDelta(
        instrument_id=TestStubs.audusd_id(),
        level=OrderBookLevel.L2,
        delta_type=OrderBookDeltaType.UPDATE,
        order=Order(
            0.5814,
            672.45,
            OrderSide.SELL,
            "4a25c3f6-76e7-7584-c5a3-4ec84808e240",
        ),
        timestamp_ns=pd.Timestamp.utcnow().timestamp() * 1e9,
    )
    deltas = OrderBookDeltas(
        instrument_id=TestStubs.audusd_id(),
        level=OrderBookLevel.L2,
        deltas=[delta],
        timestamp_ns=pd.Timestamp.utcnow().timestamp() * 1e9,
    )
    empty_l2_book.apply_deltas(deltas)
    assert empty_l2_book.best_ask_price() == 0.5814


def test_apply(empty_l2_book, clock):
    snapshot = OrderBookSnapshot(
        instrument_id=empty_l2_book.instrument_id,
        level=OrderBookLevel.L2,
        bids=[[150.0, 0.51]],
        asks=[[160.0, 1.51]],
        timestamp_ns=0,
    )
    empty_l2_book.apply_snapshot(snapshot)
    assert empty_l2_book.best_ask_price() == 160
    delta = OrderBookDelta(
        instrument_id=TestStubs.audusd_id(),
        level=OrderBookLevel.L2,
        delta_type=OrderBookDeltaType.ADD,
        order=Order(
            155.0,
            672.45,
            OrderSide.SELL,
            "4a25c3f6-76e7-7584-c5a3-4ec84808e240",
        ),
        timestamp_ns=clock.timestamp(),
    )
    empty_l2_book.apply(delta)
    assert empty_l2_book.best_ask_price() == 155


def test_orderbook_midpoint(sample_book):
    assert sample_book.midpoint() == 0.858


def test_orderbook_midpoint_empty(empty_l2_book):
    assert empty_l2_book.midpoint() is None


def test_timestamp_ns(empty_l2_book, clock):
    delta = OrderBookDelta(
        instrument_id=TestStubs.audusd_id(),
        level=OrderBookLevel.L2,
        delta_type=OrderBookDeltaType.ADD,
        order=Order(
            0.5900,
            672.45,
            OrderSide.SELL,
            "4a25c3f6-76e7-7584-c5a3-4ec84808e240",
        ),
        timestamp_ns=clock.timestamp(),
    )
    empty_l2_book.apply_delta(delta)
    assert empty_l2_book.timestamp_ns == delta.timestamp_ns


def test_trade_side(sample_book):
    # Sample book is 0.83 @ 0.8860

    # Trade above the ask
    trade = TestStubs.trade_tick_5decimal(
        instrument_id=sample_book.instrument_id, price=Price("0.88700")
    )
    assert sample_book.trade_side(trade=trade) == OrderSide.SELL

    # Trade below the bid
    trade = TestStubs.trade_tick_5decimal(
        instrument_id=sample_book.instrument_id, price=Price("0.80000")
    )
    assert sample_book.trade_side(trade=trade) == OrderSide.BUY

    # Trade inside the spread
    trade = TestStubs.trade_tick_5decimal(
        instrument_id=sample_book.instrument_id, price=Price("0.85000")
    )
    assert sample_book.trade_side(trade=trade) == 0
