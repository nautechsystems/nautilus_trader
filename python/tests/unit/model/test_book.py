# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model import BookAction
from nautilus_trader.model import BookOrder
from nautilus_trader.model import BookType
from nautilus_trader.model import OrderBook
from nautilus_trader.model import OrderBookDelta
from nautilus_trader.model import OrderBookDeltas
from nautilus_trader.model import OrderSide
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity


@pytest.fixture
def bid_order():
    return BookOrder(
        side=OrderSide.BUY,
        price=Price.from_str("100.50"),
        size=Quantity.from_str("10.0"),
        order_id=1,
    )


@pytest.fixture
def ask_order():
    return BookOrder(
        side=OrderSide.SELL,
        price=Price.from_str("100.60"),
        size=Quantity.from_str("5.0"),
        order_id=2,
    )


def test_book_order_construction(bid_order):
    assert bid_order.side == OrderSide.BUY
    assert bid_order.price == Price.from_str("100.50")
    assert bid_order.size == Quantity.from_str("10.0")
    assert bid_order.order_id == 1


def test_book_order_equality():
    order1 = BookOrder(OrderSide.BUY, Price.from_str("100.50"), Quantity.from_str("10.0"), 1)
    order2 = BookOrder(OrderSide.BUY, Price.from_str("100.50"), Quantity.from_str("10.0"), 1)
    order3 = BookOrder(OrderSide.SELL, Price.from_str("100.60"), Quantity.from_str("5.0"), 2)

    assert order1 == order2
    assert order1 != order3


def test_book_order_hash():
    order1 = BookOrder(OrderSide.BUY, Price.from_str("100.50"), Quantity.from_str("10.0"), 1)
    order2 = BookOrder(OrderSide.BUY, Price.from_str("100.50"), Quantity.from_str("10.0"), 1)

    assert hash(order1) == hash(order2)


def test_book_order_repr(bid_order):
    r = repr(bid_order)
    assert "100.50" in r
    assert "10.0" in r


def test_book_order_exposure(bid_order):
    exposure = bid_order.exposure()
    assert exposure == pytest.approx(100.50 * 10.0)


def test_book_order_signed_size():
    buy = BookOrder(OrderSide.BUY, Price.from_str("100.00"), Quantity.from_str("10.0"), 1)
    sell = BookOrder(OrderSide.SELL, Price.from_str("100.00"), Quantity.from_str("10.0"), 2)

    assert buy.signed_size() == pytest.approx(10.0)
    assert sell.signed_size() == pytest.approx(-10.0)


def test_book_order_pickle_roundtrip(bid_order):
    restored = pickle.loads(pickle.dumps(bid_order))  # noqa: S301

    assert restored == bid_order
    assert restored.side == bid_order.side
    assert restored.price == bid_order.price
    assert restored.size == bid_order.size
    assert restored.order_id == bid_order.order_id


def test_book_order_to_dict_and_from_dict(bid_order):
    d = BookOrder.to_dict(bid_order)
    restored = BookOrder.from_dict(d)

    assert restored == bid_order


@pytest.fixture
def delta(audusd_id, bid_order):
    return OrderBookDelta(
        instrument_id=audusd_id,
        action=BookAction.ADD,
        order=bid_order,
        flags=0,
        sequence=1,
        ts_event=1_000_000_000,
        ts_init=1_000_000_001,
    )


def test_order_book_delta_construction(delta, audusd_id):
    assert delta.instrument_id == audusd_id
    assert delta.action == BookAction.ADD
    assert delta.flags == 0
    assert delta.sequence == 1
    assert delta.ts_event == 1_000_000_000
    assert delta.ts_init == 1_000_000_001


def test_order_book_delta_equality(audusd_id, bid_order):
    delta1 = OrderBookDelta(audusd_id, BookAction.ADD, bid_order, 0, 1, 0, 0)
    delta2 = OrderBookDelta(audusd_id, BookAction.ADD, bid_order, 0, 1, 0, 0)

    assert delta1 == delta2


def test_order_book_delta_hash(audusd_id, bid_order):
    delta1 = OrderBookDelta(audusd_id, BookAction.ADD, bid_order, 0, 1, 0, 0)
    delta2 = OrderBookDelta(audusd_id, BookAction.ADD, bid_order, 0, 1, 0, 0)

    assert hash(delta1) == hash(delta2)


def test_order_book_delta_repr(delta):
    r = repr(delta)
    assert "AUD/USD.SIM" in r


def test_order_book_delta_pickle_roundtrip(delta):
    restored = pickle.loads(pickle.dumps(delta))  # noqa: S301

    assert restored == delta
    assert restored.instrument_id == delta.instrument_id
    assert restored.action == delta.action
    assert restored.ts_event == delta.ts_event


def test_order_book_delta_to_dict_and_from_dict(delta):
    d = OrderBookDelta.to_dict(delta)
    restored = OrderBookDelta.from_dict(d)

    assert restored == delta


def test_order_book_delta_clear(audusd_id):
    null_order = BookOrder(OrderSide.NO_ORDER_SIDE, Price.from_str("0"), Quantity.from_str("0"), 0)
    delta = OrderBookDelta(
        instrument_id=audusd_id,
        action=BookAction.CLEAR,
        order=null_order,
        flags=0,
        sequence=5,
        ts_event=0,
        ts_init=0,
    )

    assert delta.instrument_id == audusd_id
    assert delta.action == BookAction.CLEAR
    assert delta.sequence == 5


def test_order_book_deltas_construction(audusd_id, bid_order, ask_order):
    d1 = OrderBookDelta(audusd_id, BookAction.ADD, bid_order, 0, 1, 0, 0)
    d2 = OrderBookDelta(audusd_id, BookAction.ADD, ask_order, 0, 2, 0, 0)

    deltas = OrderBookDeltas(
        instrument_id=audusd_id,
        deltas=[d1, d2],
    )

    assert deltas.instrument_id == audusd_id
    assert len(deltas.deltas) == 2
    assert deltas.deltas[0].action == BookAction.ADD
    assert deltas.deltas[1].action == BookAction.ADD


def test_order_book_deltas_pickle_roundtrip(audusd_id, bid_order, ask_order):
    d1 = OrderBookDelta(audusd_id, BookAction.ADD, bid_order, 0, 1, 0, 0)
    d2 = OrderBookDelta(audusd_id, BookAction.ADD, ask_order, 0, 2, 0, 0)

    deltas = OrderBookDeltas(
        instrument_id=audusd_id,
        deltas=[d1, d2],
    )

    restored = pickle.loads(pickle.dumps(deltas))  # noqa: S301

    assert restored.instrument_id == deltas.instrument_id
    assert len(restored.deltas) == 2
    assert restored.deltas[0] == d1
    assert restored.deltas[1] == d2


def test_order_book_construction(audusd_id):
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L2_MBP)

    assert book.instrument_id == audusd_id
    assert book.book_type == BookType.L2_MBP
    assert book.update_count == 0


def test_order_book_add_and_query(audusd_id):
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L2_MBP)

    bid = BookOrder(OrderSide.BUY, Price.from_str("100.50"), Quantity.from_str("10.0"), 1)
    ask = BookOrder(OrderSide.SELL, Price.from_str("100.60"), Quantity.from_str("5.0"), 2)

    delta_bid = OrderBookDelta(audusd_id, BookAction.ADD, bid, 0, 1, 0, 0)
    delta_ask = OrderBookDelta(audusd_id, BookAction.ADD, ask, 0, 2, 0, 0)

    book.apply_delta(delta_bid)
    book.apply_delta(delta_ask)

    assert book.best_bid_price() == Price.from_str("100.50")
    assert book.best_ask_price() == Price.from_str("100.60")
    assert book.best_bid_size() == Quantity.from_str("10.0")
    assert book.best_ask_size() == Quantity.from_str("5.0")


def test_order_book_spread(audusd_id):
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L2_MBP)

    bid = BookOrder(OrderSide.BUY, Price.from_str("100.50"), Quantity.from_str("10.0"), 1)
    ask = BookOrder(OrderSide.SELL, Price.from_str("100.60"), Quantity.from_str("5.0"), 2)

    book.apply_delta(OrderBookDelta(audusd_id, BookAction.ADD, bid, 0, 1, 0, 0))
    book.apply_delta(OrderBookDelta(audusd_id, BookAction.ADD, ask, 0, 2, 0, 0))

    assert book.spread() == pytest.approx(0.10, abs=0.001)


def test_order_book_midpoint(audusd_id):
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L2_MBP)

    bid = BookOrder(OrderSide.BUY, Price.from_str("100.00"), Quantity.from_str("10.0"), 1)
    ask = BookOrder(OrderSide.SELL, Price.from_str("101.00"), Quantity.from_str("5.0"), 2)

    book.apply_delta(OrderBookDelta(audusd_id, BookAction.ADD, bid, 0, 1, 0, 0))
    book.apply_delta(OrderBookDelta(audusd_id, BookAction.ADD, ask, 0, 2, 0, 0))

    assert book.midpoint() == pytest.approx(100.50)


def test_order_book_reset(audusd_id):
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L2_MBP)

    bid = BookOrder(OrderSide.BUY, Price.from_str("100.50"), Quantity.from_str("10.0"), 1)
    book.apply_delta(OrderBookDelta(audusd_id, BookAction.ADD, bid, 0, 1, 0, 0))
    book.reset()

    assert book.best_bid_price() is None
    assert book.best_ask_price() is None


def test_order_book_repr(audusd_id):
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L2_MBP)
    r = repr(book)

    assert "OrderBook" in r
    assert "L2_MBP" in r
