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
"""
Test book behavior.
"""

import copy
import operator
import pickle
from decimal import Decimal

import pytest

from nautilus_trader.model import AggressorSide
from nautilus_trader.model import BookAction
from nautilus_trader.model import BookLevel
from nautilus_trader.model import BookOrder
from nautilus_trader.model import BookType
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OrderBook
from nautilus_trader.model import OrderBookDelta
from nautilus_trader.model import OrderBookDeltas
from nautilus_trader.model import OrderBookDepth10
from nautilus_trader.model import OrderSide
from nautilus_trader.model import OrderStatus
from nautilus_trader.model import OrderType
from nautilus_trader.model import OwnBookOrder
from nautilus_trader.model import OwnOrderBook
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import RecordFlag
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TradeId
from nautilus_trader.model import TraderId
from nautilus_trader.model import TradeTick
from nautilus_trader.model import update_book_with_quote_tick
from nautilus_trader.model import update_book_with_trade_tick


@pytest.fixture
def bid_order() -> object:
    """
    Bid order.
    """
    return BookOrder(
        side=OrderSide.BUY,
        price=Price.from_str("100.50"),
        size=Quantity.from_str("10.0"),
        order_id=1,
    )


@pytest.fixture
def ask_order() -> object:
    """
    Ask order.
    """
    return BookOrder(
        side=OrderSide.SELL,
        price=Price.from_str("100.60"),
        size=Quantity.from_str("5.0"),
        order_id=2,
    )


def test_book_order_construction(bid_order: object) -> None:
    """
    Test book order construction.
    """
    assert bid_order.side == OrderSide.BUY
    assert bid_order.price == Price.from_str("100.50")
    assert bid_order.size == Quantity.from_str("10.0")
    assert bid_order.order_id == 1


def test_book_order_construction_with_legacy_no_order_side() -> None:
    """
    Test book order construction with the legacy no-order-side alias.
    """
    order = BookOrder(
        OrderSide.NO_ORDER_SIDE,
        Price.from_str("0"),
        Quantity.from_str("0"),
        0,
    )

    assert order.side is None
    assert order.price == Price.from_str("0")
    assert order.size == Quantity.from_str("0")
    assert order.order_id == 0


def test_book_order_equality() -> None:
    """
    Test book order equality.
    """
    order1 = BookOrder(OrderSide.BUY, Price.from_str("100.50"), Quantity.from_str("10.0"), 1)
    order2 = BookOrder(OrderSide.BUY, Price.from_str("100.50"), Quantity.from_str("10.0"), 1)
    order3 = BookOrder(OrderSide.SELL, Price.from_str("100.60"), Quantity.from_str("5.0"), 2)

    assert order1 == order2
    assert order1 != order3


def test_book_order_hash() -> None:
    """
    Test book order hash.
    """
    order1 = BookOrder(OrderSide.BUY, Price.from_str("100.50"), Quantity.from_str("10.0"), 1)
    order2 = BookOrder(OrderSide.BUY, Price.from_str("100.50"), Quantity.from_str("10.0"), 1)

    assert hash(order1) == hash(order2)


def test_book_order_repr(bid_order: object) -> None:
    """
    Test book order repr.
    """
    r = repr(bid_order)
    assert "100.50" in r
    assert "10.0" in r


def test_book_order_exposure(bid_order: object) -> None:
    """
    Test book order exposure.
    """
    exposure = bid_order.exposure()
    assert exposure == pytest.approx(100.50 * 10.0)


def test_book_order_signed_size() -> None:
    """
    Test book order signed size.
    """
    buy = BookOrder(OrderSide.BUY, Price.from_str("100.00"), Quantity.from_str("10.0"), 1)
    sell = BookOrder(OrderSide.SELL, Price.from_str("100.00"), Quantity.from_str("10.0"), 2)

    assert buy.signed_size() == pytest.approx(10.0)
    assert sell.signed_size() == pytest.approx(-10.0)


def test_book_order_pickle_roundtrip(bid_order: object) -> None:
    """
    Test book order pickle roundtrip.
    """
    restored = pickle.loads(pickle.dumps(bid_order))

    assert restored == bid_order
    assert restored.side == bid_order.side
    assert restored.price == bid_order.price
    assert restored.size == bid_order.size
    assert restored.order_id == bid_order.order_id


def test_book_order_to_dict_and_from_dict(bid_order: object) -> None:
    """
    Test book order to dict and from dict.
    """
    d = BookOrder.to_dict(bid_order)
    restored = BookOrder.from_dict(d)

    assert restored == bid_order


@pytest.fixture
def delta(audusd_id: InstrumentId, bid_order: object) -> object:
    """
    Delta.
    """
    return OrderBookDelta(
        instrument_id=audusd_id,
        action=BookAction.ADD,
        order=bid_order,
        flags=0,
        sequence=1,
        ts_event=1_000_000_000,
        ts_init=1_000_000_001,
    )


def test_order_book_delta_construction(delta: object, audusd_id: InstrumentId) -> None:
    """
    Test order book delta construction.
    """
    assert delta.instrument_id == audusd_id
    assert delta.action == BookAction.ADD
    assert delta.flags == 0
    assert delta.sequence == 1
    assert delta.ts_event == 1_000_000_000
    assert delta.ts_init == 1_000_000_001


def test_order_book_delta_equality(audusd_id: InstrumentId, bid_order: object) -> None:
    """
    Test order book delta equality.
    """
    delta1 = OrderBookDelta(audusd_id, BookAction.ADD, bid_order, 0, 1, 0, 0)
    delta2 = OrderBookDelta(audusd_id, BookAction.ADD, bid_order, 0, 1, 0, 0)

    assert delta1 == delta2


def test_order_book_delta_hash(audusd_id: InstrumentId, bid_order: object) -> None:
    """
    Test order book delta hash.
    """
    delta1 = OrderBookDelta(audusd_id, BookAction.ADD, bid_order, 0, 1, 0, 0)
    delta2 = OrderBookDelta(audusd_id, BookAction.ADD, bid_order, 0, 1, 0, 0)

    assert hash(delta1) == hash(delta2)


def test_order_book_delta_repr(delta: object) -> None:
    """
    Test order book delta repr.
    """
    r = repr(delta)
    assert "AUD/USD.SIM" in r


def test_order_book_delta_pickle_roundtrip(delta: object) -> None:
    """
    Test order book delta pickle roundtrip.
    """
    restored = pickle.loads(pickle.dumps(delta))

    assert restored == delta
    assert restored.instrument_id == delta.instrument_id
    assert restored.action == delta.action
    assert restored.ts_event == delta.ts_event


def test_order_book_delta_to_dict_and_from_dict(delta: object) -> None:
    """
    Test order book delta to dict and from dict.
    """
    d = OrderBookDelta.to_dict(delta)
    restored = OrderBookDelta.from_dict(d)

    assert restored == delta


def test_order_book_delta_clear(audusd_id: InstrumentId) -> None:
    """
    Test order book delta clear.
    """
    delta = OrderBookDelta.clear(audusd_id, sequence=5, ts_event=0, ts_init=0)

    assert delta.instrument_id == audusd_id
    assert delta.action == BookAction.CLEAR
    assert delta.order.side is None
    assert delta.sequence == 5


@pytest.mark.parametrize(
    ("action", "expected"),
    [
        pytest.param(BookAction.ADD, (True, False, False, False), id="add"),
        pytest.param(BookAction.UPDATE, (False, True, False, False), id="update"),
        pytest.param(BookAction.DELETE, (False, False, True, False), id="delete"),
        pytest.param(BookAction.CLEAR, (False, False, False, True), id="clear"),
    ],
)
def test_order_book_delta_action_properties(
    audusd_id: InstrumentId,
    bid_order: object,
    action: BookAction,
    expected: tuple[bool, bool, bool, bool],
) -> None:
    """
    Test order book delta action properties.
    """
    delta = OrderBookDelta(audusd_id, action, bid_order, 0, 1, 0, 0)
    actual = (delta.is_add, delta.is_update, delta.is_delete, delta.is_clear)

    assert actual == expected
    assert tuple(type(value) for value in actual) == (bool, bool, bool, bool)


@pytest.mark.parametrize("property_name", ["is_add", "is_update", "is_delete", "is_clear"])
def test_order_book_delta_action_properties_are_read_only(
    delta: object,
    property_name: str,
) -> None:
    """
    Test order book delta action properties are read-only.
    """
    with pytest.raises(
        AttributeError,
        match=rf"attribute '{property_name}'.*not writable",
    ):
        setattr(delta, property_name, False)


def test_order_book_deltas_construction(
    audusd_id: InstrumentId,
    bid_order: object,
    ask_order: object,
) -> None:
    """
    Test order book deltas construction.
    """
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


@pytest.mark.parametrize(
    ("actions_and_flags", "expected"),
    [
        pytest.param(
            [(BookAction.ADD, RecordFlag.F_SNAPSHOT.value)],
            True,
            id="snapshot",
        ),
        pytest.param(
            [
                (
                    BookAction.ADD,
                    RecordFlag.F_SNAPSHOT.value | RecordFlag.F_LAST.value,
                ),
            ],
            True,
            id="combined-flags",
        ),
        pytest.param(
            [(BookAction.ADD, RecordFlag.F_MBP.value)],
            False,
            id="not-snapshot",
        ),
        pytest.param(
            [(BookAction.CLEAR, 0), (BookAction.ADD, 0)],
            False,
            id="clear-without-snapshot",
        ),
    ],
)
def test_order_book_deltas_is_snapshot(
    audusd_id: InstrumentId,
    bid_order: object,
    actions_and_flags: list[tuple[BookAction, int]],
    expected: bool,
) -> None:
    """
    Test order book deltas snapshot property.
    """
    deltas = [
        OrderBookDelta(audusd_id, action, bid_order, flags, 1, 0, 0)
        for action, flags in actions_and_flags
    ]

    assert OrderBookDeltas(audusd_id, deltas).is_snapshot is expected


def test_order_book_deltas_is_snapshot_is_read_only(
    audusd_id: InstrumentId,
    bid_order: object,
) -> None:
    """
    Test order book deltas snapshot property is read-only.
    """
    delta = OrderBookDelta(audusd_id, BookAction.ADD, bid_order, 0, 1, 0, 0)
    deltas = OrderBookDeltas(audusd_id, [delta])

    with pytest.raises(
        AttributeError,
        match=r"attribute 'is_snapshot'.*not writable",
    ):
        deltas.is_snapshot = False


def test_order_book_deltas_pickle_roundtrip(
    audusd_id: InstrumentId,
    bid_order: object,
    ask_order: object,
) -> None:
    """
    Test order book deltas pickle roundtrip.
    """
    d1 = OrderBookDelta(audusd_id, BookAction.ADD, bid_order, 0, 1, 0, 0)
    d2 = OrderBookDelta(audusd_id, BookAction.ADD, ask_order, 0, 2, 0, 0)

    deltas = OrderBookDeltas(
        instrument_id=audusd_id,
        deltas=[d1, d2],
    )

    restored = pickle.loads(pickle.dumps(deltas))

    assert restored.instrument_id == deltas.instrument_id
    assert len(restored.deltas) == 2
    assert restored.deltas[0] == d1
    assert restored.deltas[1] == d2


def test_order_book_construction(audusd_id: InstrumentId) -> None:
    """
    Test order book construction.
    """
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L2_MBP)

    assert book.instrument_id == audusd_id
    assert book.book_type == BookType.L2_MBP
    assert book.update_count == 0


def create_populated_l3_book(instrument_id: InstrumentId) -> tuple[OrderBook, list[BookOrder]]:
    """
    Create a populated L3 order book with multiple levels and FIFO orders.
    """
    orders = [
        BookOrder(
            OrderSide.BUY,
            Price.from_str("100.50"),
            Quantity.from_str("10.25"),
            11,
        ),
        BookOrder(
            OrderSide.BUY,
            Price.from_str("100.50"),
            Quantity.from_str("5.75"),
            12,
        ),
        BookOrder(
            OrderSide.BUY,
            Price.from_str("100.40"),
            Quantity.from_str("4.50"),
            13,
        ),
        BookOrder(
            OrderSide.SELL,
            Price.from_str("100.60"),
            Quantity.from_str("8.125"),
            21,
        ),
    ]
    book = OrderBook(instrument_id=instrument_id, book_type=BookType.L3_MBO)
    for sequence, order in enumerate(orders, start=41):
        book.add(order, flags=0, sequence=sequence, ts_event=sequence + 100)

    return book, orders


def create_aggregate_overflow_book(
    instrument_id: InstrumentId,
) -> tuple[OrderBook, Price, int]:
    """
    Create an order book whose aggregated level exceeds the quantity domain.
    """
    try:
        size = Quantity.from_str("20000000000000")
    except ValueError:
        size = Quantity.from_str("10000000000")

    book = OrderBook(instrument_id=instrument_id, book_type=BookType.L3_MBO)
    price = Price.from_str("100.60")
    book.add(BookOrder(OrderSide.SELL, price, size, 1), flags=0, sequence=1, ts_event=1)
    book.add(BookOrder(OrderSide.SELL, price, size, 2), flags=0, sequence=2, ts_event=2)

    return book, price, size.precision


def assert_order_book_state(actual: OrderBook, expected: OrderBook) -> None:
    """
    Assert all observable order book state, including nested order fields.
    """
    assert actual.instrument_id == expected.instrument_id
    assert actual.book_type == expected.book_type
    assert actual.sequence == expected.sequence
    assert actual.ts_last == expected.ts_last
    assert actual.update_count == expected.update_count

    actual_levels = actual.bids() + actual.asks()
    expected_levels = expected.bids() + expected.asks()
    assert len(actual_levels) == len(expected_levels)

    for actual_level, expected_level in zip(actual_levels, expected_levels, strict=True):
        assert actual_level.side == expected_level.side
        assert actual_level.price.raw == expected_level.price.raw
        assert actual_level.price.precision == expected_level.price.precision
        assert actual_level.len() == expected_level.len()

        actual_orders = actual_level.get_orders()
        expected_orders = expected_level.get_orders()
        assert len(actual_orders) == len(expected_orders)

        for actual_order, expected_order in zip(actual_orders, expected_orders, strict=True):
            assert actual_order.side == expected_order.side
            assert actual_order.price.raw == expected_order.price.raw
            assert actual_order.price.precision == expected_order.price.precision
            assert actual_order.size.raw == expected_order.size.raw
            assert actual_order.size.precision == expected_order.size.precision
            assert actual_order.order_id == expected_order.order_id


def test_order_book_pickle_roundtrip_preserves_complete_state(audusd_id: InstrumentId) -> None:
    """
    Test order book pickle roundtrip preserves complete state.
    """
    book, _ = create_populated_l3_book(audusd_id)

    restored = pickle.loads(pickle.dumps(book))

    assert restored is not book
    assert_order_book_state(restored, book)


def test_order_book_pickle_roundtrip_preserves_l2_state(audusd_id: InstrumentId) -> None:
    """
    Test order book pickle roundtrip preserves L2 price-based order IDs.
    """
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L2_MBP)
    orders = [
        BookOrder(OrderSide.BUY, Price.from_str("100.50"), Quantity.from_str("1.25"), 11),
        BookOrder(OrderSide.BUY, Price.from_str("100.40"), Quantity.from_str("2.50"), 12),
        BookOrder(OrderSide.SELL, Price.from_str("100.60"), Quantity.from_str("3.75"), 21),
    ]

    for sequence, order in enumerate(orders, start=1):
        book.add(order, flags=0, sequence=sequence, ts_event=sequence + 10)

    restored = pickle.loads(pickle.dumps(book))

    assert restored is not book
    assert_order_book_state(restored, book)


@pytest.mark.parametrize(
    "flags",
    [
        pytest.param(RecordFlag.F_TOB.value, id="top-of-book"),
        pytest.param(RecordFlag.F_MBP.value, id="market-by-price"),
    ],
)
def test_order_book_pickle_roundtrip_preserves_flag_normalized_l3_state(
    audusd_id: InstrumentId,
    flags: int,
) -> None:
    """
    Test order book pickle roundtrip preserves flag-normalized L3 order IDs.
    """
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L3_MBO)
    orders = [
        BookOrder(OrderSide.BUY, Price.from_str("100.50"), Quantity.from_str("1.25"), 11),
        BookOrder(OrderSide.BUY, Price.from_str("100.40"), Quantity.from_str("2.50"), 12),
        BookOrder(OrderSide.SELL, Price.from_str("100.60"), Quantity.from_str("3.75"), 21),
    ]

    for sequence, order in enumerate(orders, start=1):
        book.add(order, flags=flags, sequence=sequence, ts_event=sequence + 10)

    stored_orders = [order for level in book.bids() + book.asks() for order in level.get_orders()]
    restored = pickle.loads(pickle.dumps(book))

    assert stored_orders
    assert all(order.order_id not in {11, 12, 21} for order in stored_orders)
    assert restored is not book
    assert_order_book_state(restored, book)


def test_order_book_deepcopy_is_independent(audusd_id: InstrumentId) -> None:
    """
    Test order book deepcopy is independent.
    """
    book, orders = create_populated_l3_book(audusd_id)

    copied = copy.deepcopy(book)
    copied.delete(orders[0], flags=0, sequence=99, ts_event=999)

    assert copied is not book
    assert [order.order_id for order in book.bids()[0].get_orders()] == [11, 12]
    assert [order.order_id for order in copied.bids()[0].get_orders()] == [12]
    assert book.sequence == 44
    assert book.ts_last == 144
    assert book.update_count == 4
    assert copied.sequence == 99
    assert copied.ts_last == 999
    assert copied.update_count == 5


@pytest.mark.parametrize("batch_flag", [RecordFlag.F_MBP.value, RecordFlag.F_SNAPSHOT.value])
@pytest.mark.parametrize("side", [OrderSide.BUY, OrderSide.SELL])
@pytest.mark.parametrize("copy_method", ["pickle", "deepcopy"])
def test_order_book_copy_preserves_unfinished_l1_batch(
    audusd_id: InstrumentId,
    batch_flag: int,
    side: OrderSide,
    copy_method: str,
) -> None:
    """
    Test order book copies preserve unfinished L1 batch state.
    """
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L1_MBP)
    initial_price = Price.from_str("100.00")
    worse_price = Price.from_str("99.00" if side == OrderSide.BUY else "101.00")
    initial = BookOrder(side, initial_price, Quantity.from_str("10"), 1)
    terminal = BookOrder(side, worse_price, Quantity.from_str("20"), 1)
    book.add(initial, flags=batch_flag, sequence=1, ts_event=101)

    restored = pickle.loads(pickle.dumps(book)) if copy_method == "pickle" else copy.deepcopy(book)

    terminal_flags = batch_flag | RecordFlag.F_LAST.value
    book.add(terminal, flags=terminal_flags, sequence=2, ts_event=102)
    restored.add(terminal, flags=terminal_flags, sequence=2, ts_event=102)

    assert_order_book_state(restored, book)
    if side == OrderSide.BUY:
        assert restored.best_bid_price() == initial_price
        assert restored.best_ask_price() is None
    else:
        assert restored.best_ask_price() == initial_price
        assert restored.best_bid_price() is None


def test_order_book_setstate_rejects_identity_mismatch_without_mutation(
    audusd_id: InstrumentId,
) -> None:
    """
    Test order book setstate rejects identity mismatch without mutation.
    """
    book, _ = create_populated_l3_book(audusd_id)
    state = list(book.__getstate__())
    state[0] = InstrumentId.from_str("GBP/USD.SIM")

    with pytest.raises(ValueError, match="does not match instance instrument ID"):
        book.__setstate__(tuple(state))

    expected, _ = create_populated_l3_book(audusd_id)
    assert_order_book_state(book, expected)


def test_order_book_setstate_rejects_book_type_mismatch_without_mutation(
    audusd_id: InstrumentId,
) -> None:
    """
    Test order book setstate rejects book type mismatch without mutation.
    """
    book, _ = create_populated_l3_book(audusd_id)
    state = list(book.__getstate__())
    state[1] = "L2_MBP"

    with pytest.raises(ValueError, match="does not match instance book type"):
        book.__setstate__(tuple(state))

    expected, _ = create_populated_l3_book(audusd_id)
    assert_order_book_state(book, expected)


def test_order_book_setstate_rejects_order_without_side_without_mutation(
    audusd_id: InstrumentId,
) -> None:
    """
    Test order book setstate rejects order without side without mutation.
    """
    book, _ = create_populated_l3_book(audusd_id)
    state = list(book.__getstate__())
    state[5] = [BookOrder(None, Price.from_str("1.0"), Quantity.from_str("1.0"), 1)]

    with pytest.raises(ValueError, match="contains an order with no side"):
        book.__setstate__(tuple(state))

    expected, _ = create_populated_l3_book(audusd_id)
    assert_order_book_state(book, expected)


@pytest.mark.parametrize(
    ("batch_state", "message"),
    [
        (1, "Cannot restore L1 batch state"),
        (3, "Invalid L1 batch state code"),
    ],
)
def test_order_book_setstate_rejects_invalid_batch_state_without_mutation(
    audusd_id: InstrumentId,
    batch_state: int,
    message: str,
) -> None:
    """
    Test order book setstate rejects invalid batch state without mutation.
    """
    book, _ = create_populated_l3_book(audusd_id)
    state = list(book.__getstate__())
    state[6] = batch_state

    with pytest.raises(ValueError, match=message):
        book.__setstate__(tuple(state))

    expected, _ = create_populated_l3_book(audusd_id)
    assert_order_book_state(book, expected)


def test_order_book_add_and_query(audusd_id: InstrumentId) -> None:
    """
    Test order book add and query.
    """
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


def test_order_book_spread(audusd_id: InstrumentId) -> None:
    """
    Test order book spread.
    """
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L2_MBP)

    bid = BookOrder(OrderSide.BUY, Price.from_str("100.50"), Quantity.from_str("10.0"), 1)
    ask = BookOrder(OrderSide.SELL, Price.from_str("100.60"), Quantity.from_str("5.0"), 2)

    book.apply_delta(OrderBookDelta(audusd_id, BookAction.ADD, bid, 0, 1, 0, 0))
    book.apply_delta(OrderBookDelta(audusd_id, BookAction.ADD, ask, 0, 2, 0, 0))

    assert book.spread() == pytest.approx(0.10, abs=0.001)


def test_order_book_midpoint(audusd_id: InstrumentId) -> None:
    """
    Test order book midpoint.
    """
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L2_MBP)

    bid = BookOrder(OrderSide.BUY, Price.from_str("100.00"), Quantity.from_str("10.0"), 1)
    ask = BookOrder(OrderSide.SELL, Price.from_str("101.00"), Quantity.from_str("5.0"), 2)

    book.apply_delta(OrderBookDelta(audusd_id, BookAction.ADD, bid, 0, 1, 0, 0))
    book.apply_delta(OrderBookDelta(audusd_id, BookAction.ADD, ask, 0, 2, 0, 0))

    assert book.midpoint() == pytest.approx(100.50)


def test_update_book_with_quote_tick(audusd_id: InstrumentId) -> None:
    """
    Test update book with quote tick.
    """
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L1_MBP)
    quote = QuoteTick(
        instrument_id=audusd_id,
        bid_price=Price.from_str("100.50"),
        ask_price=Price.from_str("100.60"),
        bid_size=Quantity.from_str("10"),
        ask_size=Quantity.from_str("5"),
        ts_event=1,
        ts_init=2,
    )

    update_book_with_quote_tick(book, quote)

    assert book.best_bid_price() == Price.from_str("100.50")
    assert book.best_ask_price() == Price.from_str("100.60")
    assert book.best_bid_size() == Quantity.from_str("10")
    assert book.best_ask_size() == Quantity.from_str("5")
    assert book.update_count == 1


def test_update_book_with_trade_tick(audusd_id: InstrumentId) -> None:
    """
    Test update book with trade tick.
    """
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L1_MBP)
    trade = TradeTick(
        instrument_id=audusd_id,
        price=Price.from_str("100.55"),
        size=Quantity.from_str("7"),
        aggressor_side=AggressorSide.BUY,
        trade_id=TradeId("TRADE-001"),
        ts_event=1,
        ts_init=2,
    )

    update_book_with_trade_tick(book, trade)

    assert book.best_bid_price() == Price.from_str("100.55")
    assert book.best_ask_price() == Price.from_str("100.55")
    assert book.best_bid_size() == Quantity.from_str("7")
    assert book.best_ask_size() == Quantity.from_str("7")
    assert book.update_count == 1


def test_order_book_reset(audusd_id: InstrumentId) -> None:
    """
    Test order book reset.
    """
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L2_MBP)

    bid = BookOrder(OrderSide.BUY, Price.from_str("100.50"), Quantity.from_str("10.0"), 1)
    book.apply_delta(OrderBookDelta(audusd_id, BookAction.ADD, bid, 0, 1, 0, 0))
    book.reset()

    assert book.best_bid_price() is None
    assert book.best_ask_price() is None


def test_order_book_repr(audusd_id: InstrumentId) -> None:
    """
    Test order book repr.
    """
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L2_MBP)
    r = repr(book)

    assert "OrderBook" in r
    assert "L2_MBP" in r


@pytest.fixture
def depth10() -> object:
    """
    Depth10.
    """
    return OrderBookDepth10.get_stub()


def test_depth10_get_stub(depth10: object) -> None:
    """
    Test depth10 get stub.
    """
    assert depth10.instrument_id == InstrumentId.from_str("AAPL.XNAS")
    assert len(depth10.bids) == 10
    assert len(depth10.asks) == 10
    assert len(depth10.bid_counts) == 10
    assert len(depth10.ask_counts) == 10


def test_depth10_properties(depth10: object) -> None:
    """
    Test depth10 properties.
    """
    assert depth10.flags == 0
    assert depth10.sequence == 0
    assert depth10.ts_event == 1
    assert depth10.ts_init == 2


def test_depth10_bid_ask_structure(depth10: object) -> None:
    """
    Test depth10 bid ask structure.
    """
    for bid in depth10.bids:
        assert bid.side == OrderSide.BUY
    for ask in depth10.asks:
        assert ask.side == OrderSide.SELL

    assert depth10.bids[0].price > depth10.bids[1].price
    assert depth10.asks[0].price < depth10.asks[1].price


def test_depth10_hash(depth10: object) -> None:
    """
    Test depth10 hash.
    """
    assert isinstance(hash(depth10), int)


def test_depth10_str_and_repr(depth10: object) -> None:
    """
    Test depth10 str and repr.
    """
    assert "AAPL.XNAS" in str(depth10)
    assert "OrderBookDepth10" in repr(depth10)


def test_depth10_to_dict_and_from_dict_roundtrip(depth10: object) -> None:
    """
    Test depth10 to dict and from dict roundtrip.
    """
    d = depth10.to_dict()
    restored = OrderBookDepth10.from_dict(d)

    assert d["instrument_id"] == "AAPL.XNAS"
    assert len(d["bids"]) == 10
    assert len(d["asks"]) == 10
    assert restored == depth10


def test_depth10_fully_qualified_name() -> None:
    """
    Test depth10 fully qualified name.
    """
    assert OrderBookDepth10.fully_qualified_name() == "nautilus_trader.model:OrderBookDepth10"


def test_depth10_json_roundtrip(depth10: object) -> None:
    """
    Test depth10 json roundtrip.
    """
    json_bytes = depth10.to_json_bytes()
    restored = OrderBookDepth10.from_json(json_bytes)

    assert restored == depth10


def test_depth10_msgpack_roundtrip(depth10: object) -> None:
    """
    Test depth10 msgpack roundtrip.
    """
    msgpack_bytes = depth10.to_msgpack_bytes()
    restored = OrderBookDepth10.from_msgpack(msgpack_bytes)

    assert restored == depth10


def test_depth10_get_metadata() -> None:
    """
    Test depth10 get metadata.
    """
    instrument_id = InstrumentId.from_str("AAPL.XNAS")
    metadata = OrderBookDepth10.get_metadata(instrument_id, 2, 0)

    assert metadata["instrument_id"] == "AAPL.XNAS"


def test_depth10_get_fields() -> None:
    """
    Test depth10 get fields.
    """
    fields = OrderBookDepth10.get_fields()

    assert "flags" in fields
    assert "sequence" in fields
    assert "ts_event" in fields
    assert "ts_init" in fields


def test_order_book_apply_depth_updates_best_prices(depth10: object) -> None:
    """
    Test order book apply depth updates best prices.
    """
    book = OrderBook(instrument_id=depth10.instrument_id, book_type=BookType.L2_MBP)

    book.apply_depth(depth10)

    assert book.best_bid_price() == Price.from_str("99.00")
    assert book.best_ask_price() == Price.from_str("100.00")
    assert book.best_bid_size() == Quantity.from_str("100")
    assert book.best_ask_size() == Quantity.from_str("100")
    assert book.update_count == 1
    assert book.bids_to_dict(depth=2) == {
        Decimal("99.00"): Decimal(100),
        Decimal("98.00"): Decimal(200),
    }
    assert book.asks_to_dict(depth=2) == {
        Decimal("100.00"): Decimal(100),
        Decimal("101.00"): Decimal(200),
    }


def test_book_level_properties(audusd_id: InstrumentId) -> None:
    """
    Test book level properties.
    """
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L2_MBP)
    bid = BookOrder(OrderSide.BUY, Price.from_str("100.50"), Quantity.from_str("10"), 1)

    book.apply_delta(OrderBookDelta(audusd_id, BookAction.ADD, bid, 0, 1, 0, 0))

    level = book.bids()[0]

    assert isinstance(level, BookLevel)
    assert level.side == OrderSide.BUY
    assert level.price == Price.from_str("100.50")
    assert level.len() == 1
    assert not level.is_empty()
    assert level.size() == pytest.approx(10.0)
    assert level.exposure() == pytest.approx(1005.0)
    first = level.first()
    assert first is not None
    assert first.price == level.price
    assert first.size == Quantity.from_str("10")
    assert len(level.get_orders()) == 1

    with pytest.raises(AttributeError):
        level.side = OrderSide.SELL


def test_book_level_comparisons_follow_ladder_order(audusd_id: InstrumentId) -> None:
    """
    Test book level comparisons follow ladder order.
    """
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L2_MBP)

    for sequence, order in enumerate(
        [
            BookOrder(OrderSide.BUY, Price.from_str("100.50"), Quantity.from_str("1"), 1),
            BookOrder(OrderSide.BUY, Price.from_str("100.40"), Quantity.from_str("2"), 2),
            BookOrder(OrderSide.SELL, Price.from_str("100.60"), Quantity.from_str("3"), 3),
            BookOrder(OrderSide.SELL, Price.from_str("100.70"), Quantity.from_str("4"), 4),
        ],
        start=1,
    ):
        book.add(order, flags=0, sequence=sequence, ts_event=sequence)

    best_bid, next_bid = book.bids()
    best_ask, next_ask = book.asks()

    assert best_bid == book.bids()[0]
    assert best_bid != next_bid
    assert best_bid < next_bid
    assert best_bid <= next_bid
    assert next_bid > best_bid
    assert next_bid >= best_bid
    assert best_ask < next_ask
    assert best_ask <= next_ask
    assert next_ask > best_ask
    assert next_ask >= best_ask
    assert best_bid != best_ask
    assert hash(best_bid) == hash(book.bids()[0])


@pytest.mark.parametrize("comparison", [operator.lt, operator.le, operator.gt, operator.ge])
def test_book_level_cross_side_ordering_raises_type_error(
    audusd_id: InstrumentId,
    comparison: object,
) -> None:
    """
    Test book level cross-side ordering raises TypeError.
    """
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L2_MBP)
    book.add(
        BookOrder(OrderSide.BUY, Price.from_str("100.50"), Quantity.from_str("1"), 1),
        flags=0,
        sequence=1,
        ts_event=1,
    )
    book.add(
        BookOrder(OrderSide.SELL, Price.from_str("100.60"), Quantity.from_str("1"), 2),
        flags=0,
        sequence=2,
        ts_event=2,
    )

    with pytest.raises(TypeError):
        comparison(book.bids()[0], book.asks()[0])


def test_order_book_grouped_views(audusd_id: InstrumentId) -> None:
    """
    Test order book grouped views.
    """
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L2_MBP)

    book.apply_delta(
        OrderBookDelta(
            audusd_id,
            BookAction.ADD,
            BookOrder(OrderSide.BUY, Price.from_str("100.59"), Quantity.from_str("10"), 1),
            0,
            1,
            0,
            0,
        ),
    )
    book.apply_delta(
        OrderBookDelta(
            audusd_id,
            BookAction.ADD,
            BookOrder(OrderSide.BUY, Price.from_str("100.51"), Quantity.from_str("5"), 2),
            0,
            2,
            0,
            0,
        ),
    )
    book.apply_delta(
        OrderBookDelta(
            audusd_id,
            BookAction.ADD,
            BookOrder(OrderSide.SELL, Price.from_str("100.61"), Quantity.from_str("7"), 3),
            0,
            3,
            0,
            0,
        ),
    )
    book.apply_delta(
        OrderBookDelta(
            audusd_id,
            BookAction.ADD,
            BookOrder(OrderSide.SELL, Price.from_str("100.69"), Quantity.from_str("8"), 4),
            0,
            4,
            0,
            0,
        ),
    )

    assert book.group_bids(Decimal("0.10")) == {Decimal("100.50"): Decimal(15)}
    assert book.group_asks(Decimal("0.10")) == {Decimal("100.70"): Decimal(15)}


def test_order_book_filtered_view_excludes_own_orders(audusd_id: InstrumentId) -> None:
    """
    Test order book filtered view excludes own orders.
    """
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L2_MBP)
    own_book = OwnOrderBook(instrument_id=audusd_id)

    book.apply_delta(
        OrderBookDelta(
            audusd_id,
            BookAction.ADD,
            BookOrder(OrderSide.BUY, Price.from_str("1.00000"), Quantity.from_int(100_000), 1),
            0,
            1,
            0,
            0,
        ),
    )
    book.apply_delta(
        OrderBookDelta(
            audusd_id,
            BookAction.ADD,
            BookOrder(OrderSide.BUY, Price.from_str("0.99990"), Quantity.from_int(50_000), 2),
            0,
            2,
            0,
            0,
        ),
    )
    book.apply_delta(
        OrderBookDelta(
            audusd_id,
            BookAction.ADD,
            BookOrder(OrderSide.SELL, Price.from_str("1.00010"), Quantity.from_int(70_000), 3),
            0,
            3,
            0,
            0,
        ),
    )

    own_book.add(
        OwnBookOrder(
            trader_id=TraderId("TRADER-001"),
            client_order_id=ClientOrderId("O-1"),
            side=OrderSide.BUY,
            price=Price.from_str("1.00000"),
            size=Quantity.from_int(25_000),
            order_type=OrderType.LIMIT,
            time_in_force=TimeInForce.GTC,
            status=OrderStatus.ACCEPTED,
            ts_last=10,
            ts_accepted=10,
            ts_submitted=0,
            ts_init=0,
        ),
    )
    own_book.add(
        OwnBookOrder(
            trader_id=TraderId("TRADER-001"),
            client_order_id=ClientOrderId("O-2"),
            side=OrderSide.SELL,
            price=Price.from_str("1.00010"),
            size=Quantity.from_int(30_000),
            order_type=OrderType.LIMIT,
            time_in_force=TimeInForce.GTC,
            status=OrderStatus.ACCEPTED,
            ts_last=20,
            ts_accepted=20,
            ts_submitted=0,
            ts_init=0,
        ),
    )

    expected_bids = {
        Decimal("1.00000"): Decimal(75000),
        Decimal("0.99990"): Decimal(50000),
    }
    expected_asks = {Decimal("1.00010"): Decimal(40000)}

    assert book.bids_filtered_to_dict(own_book=own_book) == expected_bids
    assert book.asks_filtered_to_dict(own_book=own_book) == expected_asks
    assert book.group_bids_filtered(Decimal("0.0001"), own_book=own_book) == {
        Decimal("1.0000"): Decimal(75000),
        Decimal("0.9999"): Decimal(50000),
    }
    assert book.group_asks_filtered(Decimal("0.0001"), own_book=own_book) == {
        Decimal("1.0001"): Decimal(40000),
    }

    filtered = book.filtered_view(own_book=own_book)

    assert filtered.bids_to_dict() == expected_bids
    assert filtered.asks_to_dict() == expected_asks


def test_order_book_get_quantity_methods(audusd_id: InstrumentId) -> None:
    """
    Test order book get quantity methods.
    """
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L2_MBP)

    book.apply_delta(
        OrderBookDelta(
            audusd_id,
            BookAction.ADD,
            BookOrder(OrderSide.BUY, Price.from_str("100.50"), Quantity.from_str("10"), 1),
            0,
            1,
            0,
            0,
        ),
    )
    book.apply_delta(
        OrderBookDelta(
            audusd_id,
            BookAction.ADD,
            BookOrder(OrderSide.SELL, Price.from_str("100.60"), Quantity.from_str("5"), 2),
            0,
            2,
            0,
            0,
        ),
    )
    book.apply_delta(
        OrderBookDelta(
            audusd_id,
            BookAction.ADD,
            BookOrder(OrderSide.SELL, Price.from_str("100.70"), Quantity.from_str("15"), 3),
            0,
            3,
            0,
            0,
        ),
    )

    assert book.get_quantity_for_price(Price.from_str("100.60"), OrderSide.BUY) == pytest.approx(
        5.0,
    )
    assert book.get_quantity_at_level(
        Price.from_str("100.60"),
        OrderSide.BUY,
        1,
    ) == Quantity.from_str("5.0")
    assert book.get_quantity_for_price(
        Price.from_str("100.50"),
        OrderSide.SELL,
    ) == pytest.approx(10.0)
    assert book.get_quantity_at_level(
        Price.from_str("100.50"),
        OrderSide.SELL,
        1,
    ) == Quantity.from_str("10.0")


def test_order_book_get_quantity_at_level_rejects_invalid_precision(
    audusd_id: InstrumentId,
) -> None:
    """
    Test order book quantity at level rejects invalid precision.
    """
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L2_MBP)

    with pytest.raises(ValueError, match="precision"):
        book.get_quantity_at_level(Price.from_str("100.60"), OrderSide.BUY, 255)


def test_order_book_get_quantity_at_level_rejects_aggregate_overflow(
    audusd_id: InstrumentId,
) -> None:
    """
    Test order book quantity at level rejects aggregate overflow.
    """
    book, price, size_precision = create_aggregate_overflow_book(audusd_id)

    with pytest.raises(ValueError, match=r"Overflow occurred|QUANTITY_RAW_MAX"):
        book.get_quantity_at_level(price, OrderSide.BUY, size_precision)


def test_order_book_get_all_crossed_levels(audusd_id: InstrumentId) -> None:
    """
    Test order book get all crossed levels.
    """
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L2_MBP)
    orders = [
        BookOrder(OrderSide.BUY, Price.from_str("100.50"), Quantity.from_str("10"), 1),
        BookOrder(OrderSide.BUY, Price.from_str("100.40"), Quantity.from_str("20"), 2),
        BookOrder(OrderSide.SELL, Price.from_str("100.60"), Quantity.from_str("5"), 3),
        BookOrder(OrderSide.SELL, Price.from_str("100.70"), Quantity.from_str("15"), 4),
    ]

    for sequence, order in enumerate(orders, start=1):
        book.add(order, flags=0, sequence=sequence, ts_event=sequence)

    buy_levels = book.get_all_crossed_levels(OrderSide.BUY, Price.from_str("100.70"), 1)
    sell_levels = book.get_all_crossed_levels(OrderSide.SELL, Price.from_str("100.40"), 1)
    no_buy_levels = book.get_all_crossed_levels(OrderSide.BUY, Price.from_str("100.50"), 1)
    empty_levels = OrderBook(audusd_id, BookType.L2_MBP).get_all_crossed_levels(
        OrderSide.SELL,
        Price.from_str("100.50"),
        1,
    )

    assert buy_levels == [
        (Price.from_str("100.60"), Quantity.from_str("5.0")),
        (Price.from_str("100.70"), Quantity.from_str("15.0")),
    ]
    assert sell_levels == [
        (Price.from_str("100.50"), Quantity.from_str("10.0")),
        (Price.from_str("100.40"), Quantity.from_str("20.0")),
    ]
    assert no_buy_levels == []
    assert empty_levels == []


def test_order_book_get_all_crossed_levels_rejects_invalid_precision(
    audusd_id: InstrumentId,
) -> None:
    """
    Test order book get all crossed levels rejects invalid precision.
    """
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L2_MBP)

    with pytest.raises(ValueError, match="precision"):
        book.get_all_crossed_levels(OrderSide.BUY, Price.from_str("100.60"), 255)


def test_order_book_get_all_crossed_levels_rejects_aggregate_overflow(
    audusd_id: InstrumentId,
) -> None:
    """
    Test order book get all crossed levels rejects aggregate overflow.
    """
    book, price, size_precision = create_aggregate_overflow_book(audusd_id)

    with pytest.raises(ValueError, match=r"Overflow occurred|QUANTITY_RAW_MAX"):
        book.get_all_crossed_levels(OrderSide.BUY, price, size_precision)


def test_order_book_to_deltas_preserves_snapshot_contract(audusd_id: InstrumentId) -> None:
    """
    Test order book to deltas preserves snapshot contract.
    """
    book, orders = create_populated_l3_book(audusd_id)

    snapshot = book.to_deltas(ts_event=1_000, ts_init=2_000)

    assert snapshot.instrument_id == audusd_id
    assert len(snapshot.deltas) == 5
    clear = snapshot.deltas[0]
    assert clear.instrument_id == audusd_id
    assert clear.action == BookAction.CLEAR
    assert clear.order.side is None
    assert clear.order.price.raw == 0
    assert clear.order.price.precision == 0
    assert clear.order.size.raw == 0
    assert clear.order.size.precision == 0
    assert clear.order.order_id == 0
    assert clear.flags == RecordFlag.F_SNAPSHOT.value
    assert clear.sequence == 44
    assert clear.ts_event == 1_000
    assert clear.ts_init == 2_000

    for index, (delta, order) in enumerate(zip(snapshot.deltas[1:], orders, strict=True)):
        expected_flags = RecordFlag.F_SNAPSHOT.value
        if index == len(orders) - 1:
            expected_flags |= RecordFlag.F_LAST.value

        assert delta.instrument_id == audusd_id
        assert delta.action == BookAction.ADD
        assert delta.order.side == order.side
        assert delta.order.price.raw == order.price.raw
        assert delta.order.price.precision == order.price.precision
        assert delta.order.size.raw == order.size.raw
        assert delta.order.size.precision == order.size.precision
        assert delta.order.order_id == order.order_id
        assert delta.flags == expected_flags
        assert delta.sequence == 44
        assert delta.ts_event == 1_000
        assert delta.ts_init == 2_000


def test_order_book_to_deltas_marks_empty_snapshot_final(audusd_id: InstrumentId) -> None:
    """
    Test order book to deltas marks empty snapshot final.
    """
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L2_MBP)

    snapshot = book.to_deltas(ts_event=1_000, ts_init=2_000)

    assert snapshot.instrument_id == audusd_id
    assert len(snapshot.deltas) == 1
    assert snapshot.deltas[0].action == BookAction.CLEAR
    assert snapshot.deltas[0].flags == (RecordFlag.F_SNAPSHOT.value | RecordFlag.F_LAST.value)
    assert snapshot.deltas[0].sequence == 0
    assert snapshot.deltas[0].ts_event == 1_000
    assert snapshot.deltas[0].ts_init == 2_000


def test_order_book_get_avg_px_qty_for_exposure(depth10: object) -> None:
    """
    Test order book get avg px qty for exposure.
    """
    book = OrderBook(instrument_id=depth10.instrument_id, book_type=BookType.L2_MBP)

    book.apply_depth(depth10)

    avg_px, filled_qty, worst_px = book.get_avg_px_qty_for_exposure(
        Quantity.from_int(1),
        OrderSide.BUY,
    )

    assert avg_px == pytest.approx(100.0)
    assert filled_qty == pytest.approx(0.01)
    assert worst_px == pytest.approx(100.0)


def test_order_book_simulate_fills(audusd_id: InstrumentId) -> None:
    """
    Test order book simulate fills.
    """
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L2_MBP)

    book.apply_delta(
        OrderBookDelta(
            audusd_id,
            BookAction.ADD,
            BookOrder(OrderSide.BUY, Price.from_str("100.50"), Quantity.from_str("10"), 1),
            0,
            1,
            0,
            0,
        ),
    )
    book.apply_delta(
        OrderBookDelta(
            audusd_id,
            BookAction.ADD,
            BookOrder(OrderSide.SELL, Price.from_str("100.60"), Quantity.from_str("5"), 2),
            0,
            2,
            0,
            0,
        ),
    )
    book.apply_delta(
        OrderBookDelta(
            audusd_id,
            BookAction.ADD,
            BookOrder(OrderSide.SELL, Price.from_str("100.70"), Quantity.from_str("15"), 3),
            0,
            3,
            0,
            0,
        ),
    )

    buy_fills = book.simulate_fills(
        BookOrder(OrderSide.BUY, Price.from_str("999"), Quantity.from_str("12"), 99),
    )
    sell_fills = book.simulate_fills(
        BookOrder(OrderSide.SELL, Price.from_str("0"), Quantity.from_str("7"), 100),
    )

    assert [(str(px), str(qty)) for px, qty in buy_fills] == [
        ("100.60", "5"),
        ("100.70", "7"),
    ]
    assert [(str(px), str(qty)) for px, qty in sell_fills] == [("100.50", "7")]


def test_order_book_clear_stale_levels_removes_crossed_market(audusd_id: InstrumentId) -> None:
    """
    Test order book clear stale levels removes crossed market.
    """
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L2_MBP)

    book.add(
        BookOrder(OrderSide.BUY, Price.from_str("1.00020"), Quantity.from_int(100_000), 1),
        flags=0,
        sequence=1,
        ts_event=1,
    )
    book.add(
        BookOrder(OrderSide.SELL, Price.from_str("1.00010"), Quantity.from_int(100_000), 2),
        flags=0,
        sequence=2,
        ts_event=2,
    )

    removed = book.clear_stale_levels()

    assert removed is not None
    assert len(removed) == 2
    assert [str(level.price) for level in removed] == ["1.00020", "1.00010"]
    assert book.best_bid_price() is None
    assert book.best_ask_price() is None


def test_order_book_check_integrity_on_valid_book(audusd_id: InstrumentId) -> None:
    """
    Test order book check integrity on valid book.
    """
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L2_MBP)

    book.apply_delta(
        OrderBookDelta(
            audusd_id,
            BookAction.ADD,
            BookOrder(OrderSide.BUY, Price.from_str("1.00000"), Quantity.from_int(100_000), 1),
            0,
            1,
            0,
            0,
        ),
    )
    book.apply_delta(
        OrderBookDelta(
            audusd_id,
            BookAction.ADD,
            BookOrder(OrderSide.SELL, Price.from_str("1.00010"), Quantity.from_int(100_000), 2),
            0,
            2,
            0,
            0,
        ),
    )

    book.check_integrity()
