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
from nautilus_trader.model.enums import OrderBookLevel
from nautilus_trader.model.enums import OrderBookOperationType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.orderbook.book import L2OrderBook
from nautilus_trader.model.orderbook.book import L3OrderBook
from nautilus_trader.model.orderbook.book import OrderBookOperation
from nautilus_trader.model.orderbook.book import OrderBookOperations
from nautilus_trader.model.orderbook.book import OrderBookSnapshot
from nautilus_trader.model.orderbook.ladder import Ladder
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
            timestamp_ns=0,
        )

        # Act
        # Assert
        assert (
            repr(snapshot)
            == "OrderBookSnapshot('AUD/USD.SIM', bids=[[1010, 2], [1009, 1]], asks=[[1020, 2], [1021, 1]], timestamp_ns=0)"
        )


class TestOrderBookOperation:
    def test_repr(self):
        # Arrange
        order = Order(price=10, volume=5, side=OrderSide.BUY)
        op = OrderBookOperation(
            op_type=OrderBookOperationType.ADD,
            order=order,
            timestamp_ns=0,
        )

        # Act
        # Assert
        assert (
            repr(op)
            == f"OrderBookOperation(ADD, Order(10.0, 5.0, BUY, {order.id}), timestamp_ns=0)"
        )


@pytest.fixture(scope="function")
def empty_book():
    return L2OrderBook(TestStubs.audusd_id())


@pytest.fixture(scope="function")
def sample_book():
    ob = L3OrderBook(TestStubs.audusd_id())
    orders = [
        Order(price=0.900, volume=20, side=OrderSide.SELL),
        Order(price=0.887, volume=10, side=OrderSide.SELL),
        Order(price=0.886, volume=5, side=OrderSide.SELL),
        Order(price=0.830, volume=4, side=OrderSide.BUY),
        Order(price=0.820, volume=1, side=OrderSide.BUY),
    ]
    for order in orders:
        ob.add(order)
    return ob


def test_init():
    ob = L2OrderBook(TestStubs.audusd_id())
    assert isinstance(ob.bids, Ladder) and isinstance(ob.asks, Ladder)
    assert ob.bids.reverse and not ob.asks.reverse


def test_pprint_when_no_orders():
    ob = L2OrderBook(TestStubs.audusd_id())
    result = ob.pprint()

    assert "" == result


def test_pprint_full_book(sample_book):
    result = sample_book.pprint()
    expected = """bids     price   asks
------  -------  ------
        0.9000   [20.0]
        0.8870   [10.0]
        0.8860   [5.0]
[4.0]   0.8300
[1.0]   0.8200"""
    assert expected == result


def test_add(empty_book):
    empty_book.add(Order(price=10, volume=5, side=OrderSide.BUY))
    assert empty_book.bids.top().price() == 10.0


def test_top(empty_book):
    empty_book.add(Order(price=10, volume=5, side=OrderSide.BUY))
    empty_book.add(Order(price=20, volume=5, side=OrderSide.BUY))
    empty_book.add(Order(price=5, volume=5, side=OrderSide.BUY))
    empty_book.add(Order(price=25, volume=5, side=OrderSide.SELL))
    empty_book.add(Order(price=30, volume=5, side=OrderSide.SELL))
    empty_book.add(Order(price=21, volume=5, side=OrderSide.SELL))
    assert empty_book.best_bid_level().price() == 20
    assert empty_book.best_ask_level().price() == 21


def test_check_integrity_shallow(empty_book):
    empty_book.add(Order(price=10, volume=5, side=OrderSide.SELL))
    empty_book.check_integrity()
    empty_book.add(Order(price=20, volume=5, side=OrderSide.BUY))

    with pytest.raises(AssertionError):
        empty_book.check_integrity()


def test_check_integrity_deep(empty_book):
    empty_book.add(Order(price=10, volume=5, side=OrderSide.BUY))
    empty_book.add(Order(price=5, volume=5, side=OrderSide.BUY))
    empty_book.check_integrity()


def test_orderbook_operation(empty_book):
    clock = TestClock()
    op = OrderBookOperation(
        op_type=OrderBookOperationType.UPDATE,
        order=Order(
            0.5814, 672.45, OrderSide.SELL, "4a25c3f6-76e7-7584-c5a3-4ec84808e240"
        ),
        timestamp_ns=clock.timestamp(),
    )
    empty_book.apply_operation(op)
    assert empty_book.best_ask_price() == 0.5814


def test_orderbook_operations(empty_book):
    op = OrderBookOperation(
        op_type=OrderBookOperationType.UPDATE,
        order=Order(
            0.5814, 672.45, OrderSide.SELL, "4a25c3f6-76e7-7584-c5a3-4ec84808e240"
        ),
        timestamp_ns=pd.Timestamp.utcnow().timestamp() * 1e9,
    )
    ops = OrderBookOperations(
        instrument_id=TestStubs.audusd_id(),
        level=OrderBookLevel.L2,
        ops=[op],
        timestamp_ns=pd.Timestamp.utcnow().timestamp() * 1e9,
    )
    empty_book.apply_operations(ops)
    assert empty_book.best_ask_price() == 0.5814


def test_orderbook_midpoint(sample_book):
    assert sample_book.midpoint() == 0.858


# def test_auction_match_match_orders():
#     l1 = Ladder.from_orders(
#         [
#             Order(price=103, volume=5, side=BID),
#             Order(price=102, volume=10, side=BID),
#             Order(price=100, volume=5, side=BID),
#             Order(price=90, volume=5, side=BID),
#         ]
#     )
#     l2 = Ladder.from_orders(
#         [
#             Order(price=100, volume=10, side=ASK),
#             Order(price=101, volume=10, side=ASK),
#             Order(price=105, volume=5, side=ASK),
#             Order(price=110, volume=5, side=ASK),
#         ]
#     )
#     trades = l1.auction_match(l2, on="volume")
#     assert trades
#
#
# def test_insert_remaining():
#     bids = Ladder.from_orders(orders=[Order(price=103, volume=1, side=BID), Order(price=102, volume=1, side=BID)])
#     orderbook = Orderbook(bids=bids)
#
#     order = Order(price=100, volume=3, side=ASK)
#     trades = orderbook.insert(order=order)
#     assert trades[0].price == 103
#     assert trades[0].volume == 1
#     assert trades[1].price == 102
#     assert trades[1].volume == 1
#
#     assert orderbook.asks.top_level.price == 100
#     assert orderbook.asks.top_level.volume == 1
#
#
#
#
# def test_insert_in_cross_order(orderbook):
#     order = Order(price=100, volume=1, side=BID)
#     trades = orderbook.insert(order=order, remove_trades=True)
#     expected = [Order(price=1.2, volume=1.0, side=ASK, order_id="a4")]
#     assert trades == expected
#
#
# def test_exchange_order_ids():
#     book = Orderbook(bids=None, asks=None, exchange_order_ids=True)
#     assert book.exchange_order_ids
#     assert book.bids.exchange_order_ids
#     assert book.asks.exchange_order_ids
#
#
# def test_order_id_side(orderbook):
#     result = orderbook.loads(orderbook.dumps()).order_id_side
#     expected = orderbook.order_id_side
#     assert len(result) == 10
#     assert result == expected
#
#
# def test_orderbook_in_cross():
#     orderbook = Orderbook(bids=Ladder.from_orders(orders=[Order(price=15, volume=1, side=BID)]), asks=None)
#     assert not orderbook.in_cross
#     orderbook = Orderbook(
#         bids=Ladder.from_orders(orders=[Order(price=15, volume=1, side=BID)]),
#         asks=Ladder.from_orders(orders=[Order(price=10, volume=1, side=ASK)]),
#     )
#     assert orderbook.in_cross
