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
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TradeId
from nautilus_trader.model import TraderId
from nautilus_trader.model import TradeTick
from nautilus_trader.model import update_book_with_quote_tick
from nautilus_trader.model import update_book_with_trade_tick


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


def test_update_book_with_quote_tick(audusd_id):
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


def test_update_book_with_trade_tick(audusd_id):
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L1_MBP)
    trade = TradeTick(
        instrument_id=audusd_id,
        price=Price.from_str("100.55"),
        size=Quantity.from_str("7"),
        aggressor_side=AggressorSide.BUYER,
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


@pytest.fixture
def depth10():
    return OrderBookDepth10.get_stub()


def test_depth10_get_stub(depth10):
    assert depth10.instrument_id == InstrumentId.from_str("AAPL.XNAS")
    assert len(depth10.bids) == 10
    assert len(depth10.asks) == 10
    assert len(depth10.bid_counts) == 10
    assert len(depth10.ask_counts) == 10


def test_depth10_properties(depth10):
    assert depth10.flags == 0
    assert depth10.sequence == 0
    assert depth10.ts_event == 1
    assert depth10.ts_init == 2


def test_depth10_bid_ask_structure(depth10):
    for bid in depth10.bids:
        assert bid.side == OrderSide.BUY
    for ask in depth10.asks:
        assert ask.side == OrderSide.SELL

    assert depth10.bids[0].price > depth10.bids[1].price
    assert depth10.asks[0].price < depth10.asks[1].price


def test_depth10_hash(depth10):
    assert isinstance(hash(depth10), int)


def test_depth10_str_and_repr(depth10):
    assert "AAPL.XNAS" in str(depth10)
    assert "OrderBookDepth10" in repr(depth10)


def test_depth10_to_dict_and_from_dict_roundtrip(depth10):
    d = depth10.to_dict()
    restored = OrderBookDepth10.from_dict(d)

    assert d["instrument_id"] == "AAPL.XNAS"
    assert len(d["bids"]) == 10
    assert len(d["asks"]) == 10
    assert restored == depth10


def test_depth10_fully_qualified_name():
    assert "OrderBookDepth10" in OrderBookDepth10.fully_qualified_name()


def test_depth10_json_roundtrip(depth10):
    json_bytes = depth10.to_json_bytes()
    restored = OrderBookDepth10.from_json(json_bytes)

    assert restored == depth10


def test_depth10_msgpack_roundtrip(depth10):
    msgpack_bytes = depth10.to_msgpack_bytes()
    restored = OrderBookDepth10.from_msgpack(msgpack_bytes)

    assert restored == depth10


def test_depth10_get_metadata():
    instrument_id = InstrumentId.from_str("AAPL.XNAS")
    metadata = OrderBookDepth10.get_metadata(instrument_id, 2, 0)

    assert metadata["instrument_id"] == "AAPL.XNAS"


def test_depth10_get_fields():
    fields = OrderBookDepth10.get_fields()

    assert "flags" in fields
    assert "sequence" in fields
    assert "ts_event" in fields
    assert "ts_init" in fields


def test_order_book_apply_depth_updates_best_prices(depth10):
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


def test_book_level_properties(audusd_id):
    book = OrderBook(instrument_id=audusd_id, book_type=BookType.L2_MBP)
    bid = BookOrder(OrderSide.BUY, Price.from_str("100.50"), Quantity.from_str("10"), 1)

    book.apply_delta(OrderBookDelta(audusd_id, BookAction.ADD, bid, 0, 1, 0, 0))

    level = book.bids()[0]

    assert isinstance(level, BookLevel)
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


def test_order_book_grouped_views(audusd_id):
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


def test_order_book_filtered_view_excludes_own_orders(audusd_id):
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


def test_order_book_get_quantity_methods(audusd_id):
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


def test_order_book_get_avg_px_qty_for_exposure(depth10):
    book = OrderBook(instrument_id=depth10.instrument_id, book_type=BookType.L2_MBP)

    book.apply_depth(depth10)

    avg_px, filled_qty, worst_px = book.get_avg_px_qty_for_exposure(
        Quantity.from_int(1),
        OrderSide.BUY,
    )

    assert avg_px == pytest.approx(100.0)
    assert filled_qty == pytest.approx(0.01)
    assert worst_px == pytest.approx(100.0)


def test_order_book_simulate_fills(audusd_id):
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


def test_order_book_clear_stale_levels_removes_crossed_market(audusd_id):
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


def test_order_book_check_integrity_on_valid_book(audusd_id):
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
