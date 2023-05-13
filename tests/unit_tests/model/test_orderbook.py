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

import msgspec
import pandas as pd
import pytest

from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.objects import Price
from nautilus_trader.model.orderbook import L1OrderBook
from nautilus_trader.model.orderbook import L2OrderBook
from nautilus_trader.model.orderbook import L3OrderBook
from nautilus_trader.model.orderbook import OrderBook
from nautilus_trader.model.orderbook import OrderBookDelta
from nautilus_trader.model.orderbook import OrderBookDeltas
from nautilus_trader.model.orderbook import OrderBookSnapshot
from nautilus_trader.model.orderbook.book import BookIntegrityError
from nautilus_trader.model.orderbook.data import BookOrder
from nautilus_trader.model.orderbook.ladder import Ladder
from nautilus_trader.model.orderbook.level import Level
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


class TestOrderBook:
    def setup(self):
        self.instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        self.instrument_id = self.instrument.id
        self.empty_l2_book = L2OrderBook(
            instrument_id=self.instrument_id,
            price_precision=5,
            size_precision=0,
        )
        self.sample_book = self.make_sample_book()

    def make_sample_book(self):
        return TestDataStubs.make_book(
            instrument=self.instrument,
            book_type=BookType.L3_MBO,
            bids=[
                (0.83000, 4.0),
                (0.82000, 1.0),
            ],
            asks=[
                (0.90000, 20.0),
                (0.88700, 10.0),
                (0.88600, 5.0),
            ],
        )

    def test_instantiate_base_class_directly_raises_value_error(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(RuntimeError):
            OrderBook(
                instrument_id=self.instrument_id,
                book_type=BookType.L2_MBP,
                price_precision=5,
                size_precision=0,
            )

    def test_create_level_1_order_book(self):
        # Arrange
        # Act
        book = L1OrderBook(
            instrument_id=self.instrument_id,
            price_precision=2,
            size_precision=2,
        )

        # Assert
        assert isinstance(book, L1OrderBook)
        assert book.type == BookType.L1_TBBO
        assert isinstance(book.bids, Ladder)
        assert isinstance(book.asks, Ladder)
        assert book.bids.is_reversed
        assert not book.asks.is_reversed
        assert book.ts_last == 0

    def test_create_level_2_order_book(self):
        # Arrange
        # Act
        book = OrderBook.create(
            instrument=self.instrument,
            book_type=BookType.L2_MBP,
        )

        # Assert
        assert isinstance(book, L2OrderBook)
        assert book.type == BookType.L2_MBP
        assert isinstance(book.bids, Ladder)
        assert isinstance(book.asks, Ladder)
        assert book.bids.is_reversed
        assert not book.asks.is_reversed

    def test_create_level_3_order_book(self):
        # Arrange
        # Act
        book = OrderBook.create(
            instrument=self.instrument,
            book_type=BookType.L3_MBO,
        )

        # Assert
        assert isinstance(book, L3OrderBook)
        assert book.type == BookType.L3_MBO
        assert isinstance(book.bids, Ladder)
        assert isinstance(book.asks, Ladder)
        assert book.bids.is_reversed
        assert not book.asks.is_reversed

    def test_create_level_fail(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(ValueError):
            OrderBook.create(
                instrument=self.instrument,
                book_type=0,
            )

    def test_best_bid_or_ask_price_with_no_orders_returns_none(self):
        # Arrange
        book = OrderBook.create(
            instrument=self.instrument,
            book_type=BookType.L2_MBP,
        )

        # Act
        # Assert
        assert book.best_bid_price() is None
        assert book.best_ask_price() is None

    def test_best_bid_or_ask_qty_with_no_orders_returns_none(self):
        # Arrange
        book = OrderBook.create(
            instrument=self.instrument,
            book_type=BookType.L2_MBP,
        )

        # Act
        # Assert
        assert book.best_bid_qty() is None
        assert book.best_ask_qty() is None

    def test_spread_with_no_orders_returns_none(self):
        # Arrange
        book = OrderBook.create(
            instrument=self.instrument,
            book_type=BookType.L2_MBP,
        )

        # Act
        # Assert
        assert book.spread() is None

    def test_add_orders_to_book(self):
        # Arrange
        book = OrderBook.create(
            instrument=self.instrument,
            book_type=BookType.L2_MBP,
        )

        # Act
        book.add(BookOrder(price=10.0, size=5.0, side=OrderSide.BUY))
        book.add(BookOrder(price=11.0, size=6.0, side=OrderSide.SELL))

        # Assert
        assert book.best_bid_price() == 10.0
        assert book.best_ask_price() == 11.0
        assert book.best_bid_qty() == 5.0
        assert book.best_ask_qty() == 6.0
        assert book.spread() == 1

    def test_repr(self):
        book = OrderBook.create(
            instrument=self.instrument,
            book_type=BookType.L2_MBP,
        )

        # Act
        book.add(BookOrder(price=10.0, size=5.0, side=OrderSide.BUY))
        book.add(BookOrder(price=11.0, size=6.0, side=OrderSide.SELL))

        # Assert
        assert isinstance(repr(book), str)  # <-- calls pprint internally

    def test_pprint_when_no_orders(self):
        ob = L2OrderBook(
            instrument_id=TestIdStubs.audusd_id(),
            price_precision=5,
            size_precision=0,
        )
        result = ob.pprint()

        assert result == ""

    def test_pprint_full_book(self):
        result = self.sample_book.pprint()
        print(result)
        expected = """bids     price   asks
------  -------  ------
        0.90000  [20.0]
        0.88700  [10.0]
        0.88600  [5.0]
[4.0]   0.83000
[1.0]   0.82000"""
        assert result == expected

    def test_add(self):
        self.empty_l2_book.add(BookOrder(price=10.0, size=5.0, side=OrderSide.BUY))
        assert self.empty_l2_book.bids.top().price == 10.0

    def test_delete_l1(self):
        book = OrderBook.create(
            instrument=self.instrument,
            book_type=BookType.L1_TBBO,
        )
        order = TestDataStubs.order(price=10.0, side=OrderSide.BUY)
        book.update(order)
        book.delete(order)

    def test_top(self):
        self.empty_l2_book.add(BookOrder(price=10.0, size=5.0, side=OrderSide.BUY))
        self.empty_l2_book.add(BookOrder(price=20.0, size=5.0, side=OrderSide.BUY))
        self.empty_l2_book.add(BookOrder(price=5.0, size=5.0, side=OrderSide.BUY))
        self.empty_l2_book.add(BookOrder(price=25.0, size=5.0, side=OrderSide.SELL))
        self.empty_l2_book.add(BookOrder(price=30.0, size=5.0, side=OrderSide.SELL))
        self.empty_l2_book.add(BookOrder(price=21.0, size=5.0, side=OrderSide.SELL))
        assert self.empty_l2_book.best_bid_level().price == 20
        assert self.empty_l2_book.best_ask_level().price == 21

    def test_check_integrity_empty(self):
        self.empty_l2_book.check_integrity()

    def test_check_integrity_shallow(self):
        self.empty_l2_book.add(BookOrder(price=10.0, size=5.0, side=OrderSide.SELL))
        self.empty_l2_book.check_integrity()
        try:
            # Orders will be in cross
            self.empty_l2_book.add(BookOrder(price=20.0, size=5.0, side=OrderSide.BUY))
        except BookIntegrityError:
            # Catch the integrity exception and pass to allow the test
            pass

        with pytest.raises(BookIntegrityError):
            self.empty_l2_book.check_integrity()

    def test_check_integrity_deep(self):
        self.empty_l2_book.add(BookOrder(price=10.0, size=5, side=OrderSide.BUY))
        self.empty_l2_book.add(BookOrder(price=5.0, size=5, side=OrderSide.BUY))
        self.empty_l2_book.check_integrity()

    def test_orderbook_snapshot(self):
        snapshot = OrderBookSnapshot(
            instrument_id=self.empty_l2_book.instrument_id,
            book_type=BookType.L2_MBP,
            bids=[[1550.15, 0.51], [1580.00, 1.20]],
            asks=[[1552.15, 1.51], [1582.00, 2.20]],
            ts_event=0,
            ts_init=0,
        )
        self.empty_l2_book.apply_snapshot(snapshot)
        assert self.empty_l2_book.best_bid_price() == 1580.0
        assert self.empty_l2_book.best_ask_price() == 1552.15
        assert self.empty_l2_book.count == 4
        assert self.empty_l2_book.sequence == 4

    def test_orderbook_operation_update(self):
        delta = OrderBookDelta(
            instrument_id=TestIdStubs.audusd_id(),
            book_type=BookType.L2_MBP,
            action=BookAction.UPDATE,
            order=BookOrder(
                0.5814,
                672.45,
                OrderSide.SELL,
                "4a25c3f6-76e7-7584-c5a3-4ec84808e240",
            ),
            ts_event=0,
            ts_init=0,
        )
        self.empty_l2_book.apply_delta(delta)
        assert self.empty_l2_book.best_ask_price() == 0.5814
        assert self.empty_l2_book.count == 1
        assert self.empty_l2_book.sequence == 1

    def test_orderbook_operation_add(self):
        delta = OrderBookDelta(
            instrument_id=TestIdStubs.audusd_id(),
            book_type=BookType.L2_MBP,
            action=BookAction.ADD,
            order=BookOrder(
                0.5900,
                672.45,
                OrderSide.SELL,
                "4a25c3f6-76e7-7584-c5a3-4ec84808e240",
            ),
            ts_event=0,
            ts_init=0,
        )
        self.empty_l2_book.apply_delta(delta)
        assert self.empty_l2_book.best_ask_price() == 0.59
        assert self.empty_l2_book.count == 1
        assert self.empty_l2_book.sequence == 1

    def test_orderbook_operations(self):
        delta = OrderBookDelta(
            instrument_id=TestIdStubs.audusd_id(),
            book_type=BookType.L2_MBP,
            action=BookAction.UPDATE,
            order=BookOrder(
                0.5814,
                672.45,
                OrderSide.SELL,
                "4a25c3f6-76e7-7584-c5a3-4ec84808e240",
            ),
            ts_event=pd.Timestamp.utcnow().timestamp() * 1e9,
            ts_init=pd.Timestamp.utcnow().timestamp() * 1e9,
        )
        deltas = OrderBookDeltas(
            instrument_id=TestIdStubs.audusd_id(),
            book_type=BookType.L2_MBP,
            deltas=[delta],
            ts_event=pd.Timestamp.utcnow().timestamp() * 1e9,
            ts_init=pd.Timestamp.utcnow().timestamp() * 1e9,
        )
        self.empty_l2_book.apply_deltas(deltas)
        assert self.empty_l2_book.best_ask_price() == 0.5814

    def test_apply(self):
        snapshot = OrderBookSnapshot(
            instrument_id=self.empty_l2_book.instrument_id,
            book_type=BookType.L2_MBP,
            bids=[[150.0, 0.51]],
            asks=[[160.0, 1.51]],
            ts_event=0,
            ts_init=0,
        )
        self.empty_l2_book.apply_snapshot(snapshot)
        assert self.empty_l2_book.best_ask_price() == 160
        assert self.empty_l2_book.count == 2
        delta = OrderBookDelta(
            instrument_id=TestIdStubs.audusd_id(),
            book_type=BookType.L2_MBP,
            action=BookAction.ADD,
            order=BookOrder(
                155.0,
                672.45,
                OrderSide.SELL,
                "4a25c3f6-76e7-7584-c5a3-4ec84808e240",
            ),
            ts_event=0,
            ts_init=0,
        )
        self.empty_l2_book.apply(delta)
        assert self.empty_l2_book.best_ask_price() == 155
        assert self.empty_l2_book.count == 3

    def test_orderbook_midpoint(self):
        assert self.sample_book.midpoint() == 0.858

    def test_orderbook_midpoint_empty(self):
        assert self.empty_l2_book.midpoint() is None

    def test_timestamp_ns(self):
        delta = OrderBookDelta(
            instrument_id=TestIdStubs.audusd_id(),
            book_type=BookType.L2_MBP,
            action=BookAction.ADD,
            order=BookOrder(
                0.5900,
                672.45,
                OrderSide.SELL,
                "4a25c3f6-76e7-7584-c5a3-4ec84808e240",
            ),
            ts_event=0,
            ts_init=0,
        )
        self.empty_l2_book.apply_delta(delta)
        assert self.empty_l2_book.ts_last == delta.ts_init

    def test_trade_side(self):
        # Sample book is 0.83 @ 0.8860

        # Trade above the ask
        trade = TestDataStubs.trade_tick_5decimal(
            instrument_id=self.instrument_id,
            price=Price.from_str("0.88700"),
        )
        assert self.sample_book.trade_side(trade=trade) == OrderSide.SELL

        # Trade below the bid
        trade = TestDataStubs.trade_tick_5decimal(
            instrument_id=self.instrument_id,
            price=Price.from_str("0.80000"),
        )
        assert self.sample_book.trade_side(trade=trade) == OrderSide.BUY

        # Trade inside the spread
        trade = TestDataStubs.trade_tick_5decimal(
            instrument_id=self.instrument_id,
            price=Price.from_str("0.85000"),
        )
        assert self.sample_book.trade_side(trade=trade) == 0

    def test_l3_get_price_for_volume(self):
        bid_price = self.sample_book.get_price_for_volume(True, 5.0)
        ask_price = self.sample_book.get_price_for_volume(False, 12.0)
        assert bid_price == 0.88600
        assert ask_price == 0.0

    @pytest.mark.parametrize(
        ("is_buy", "quote_volume", "expected"),
        [
            (True, 0.8860, 0.8860),
            (False, 0.8300, 0.8300),
        ],
    )
    def test_l3_get_price_for_quote_volume(self, is_buy, quote_volume, expected):
        assert self.sample_book.get_price_for_quote_volume(is_buy, quote_volume) == expected

    @pytest.mark.parametrize(
        ("is_buy", "price", "expected"),
        [
            (True, 1.0, 35.0),
            (True, 0.88600, 5.0),
            (True, 0.88650, 5.0),
            (True, 0.88700, 15.0),
            (True, 0.82, 0.0),
            (False, 0.83000, 4.0),
            (False, 0.82000, 5.0),
            (False, 0.80000, 5.0),
            (False, 0.88700, 0.0),
        ],
    )
    def test_get_volume_for_price(self, is_buy, price, expected):
        assert self.sample_book.get_volume_for_price(is_buy, price) == expected

    @pytest.mark.parametrize(
        ("is_buy", "price", "expected"),
        [
            (True, 1.0, 31.3),
            (True, 0.88600, 4.43),
            (True, 0.88650, 4.43),
            (True, 0.88700, 13.3),
            (True, 0.82, 0.0),
            (False, 0.83000, 3.32),
            (False, 0.82000, 4.14),
            (False, 0.80000, 4.14),
            (False, 0.88700, 0.0),
        ],
    )
    def test_get_quote_volume_for_price(self, is_buy, price, expected):
        assert self.sample_book.get_quote_volume_for_price(is_buy, price) == expected

    @pytest.mark.parametrize(
        ("is_buy", "volume", "expected"),
        [
            (True, 1.0, 0.886),
            (True, 3.0, 0.886),
            (True, 5.0, 0.88599),
            (True, 7.0, 0.88628),
            (True, 15.0, 0.88666),
            (True, 22.0, 0.89090),
            (False, 1.0, 0.83),
            (False, 3.0, 0.83),
            (False, 5.0, 0.828),
        ],
    )
    def test_get_vwap_for_volume(self, is_buy, volume, expected):
        assert self.sample_book.get_vwap_for_volume(is_buy, volume) == pytest.approx(expected, 0.01)

    def test_l2_update(self):
        # Arrange
        book = TestDataStubs.make_book(
            instrument=self.instrument,
            book_type=BookType.L2_MBP,
            asks=[(0.0010000, 55.81)],
        )
        deltas = OrderBookDeltas.from_dict(
            {
                "type": "OrderBookDeltas",
                "instrument_id": self.instrument_id.value,
                "book_type": "L2_MBP",
                "deltas": msgspec.json.encode(
                    [
                        {
                            "type": "OrderBookDelta",
                            "instrument_id": self.instrument_id.value,
                            "book_type": "L2_MBP",
                            "action": "UPDATE",
                            "price": 0.990099,
                            "size": 2.0,
                            "side": "BUY",
                            "order_id": "ef93694d-64c7-4b26-b03b-48c0bc2afea7",
                            "sequence": 0,
                            "ts_event": 1667288437852999936,
                            "ts_init": 1667288437852999936,
                        },
                    ],
                ),
                "sequence": 0,
                "ts_event": 1667288437852999936,
                "ts_init": 1667288437852999936,
            },
        )

        # Act
        book.apply(deltas)

        # Assert
        expected_ask = Level(price=0.001)
        expected_ask.add(BookOrder(0.001, 55.81, OrderSide.SELL, "0.00100"))
        assert book.best_ask_level() == expected_ask

        expected_bid = Level(price=0.990099)
        expected_bid.add(BookOrder(0.990099, 2.0, OrderSide.BUY, "0.99010"))
        assert book.best_bid_level() == expected_bid
