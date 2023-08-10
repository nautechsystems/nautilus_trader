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

from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook import OrderBook
from nautilus_trader.model.orderbook import OrderBookDelta
from nautilus_trader.model.orderbook import OrderBookDeltas
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


class TestOrderBook:
    def setup(self):
        self.instrument = TestInstrumentProvider.default_fx_ccy("AUD/USD")
        self.empty_book = OrderBook(
            instrument_id=self.instrument.id,
            book_type=BookType.L2_MBP,
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

    def test_create_level_1_order_book(self):
        # Arrange, Act
        book = OrderBook(
            instrument_id=self.instrument.id,
            book_type=BookType.L1_TBBO,
        )

        # Assert
        assert book.book_type == BookType.L1_TBBO

    def test_create_level_2_order_book(self):
        # Arrange, Act
        book = OrderBook(
            instrument_id=self.instrument.id,
            book_type=BookType.L2_MBP,
        )

        # Assert
        assert book.book_type == BookType.L2_MBP

    def test_create_level_3_order_book(self):
        # Arrange
        # Act
        book = OrderBook(
            instrument_id=self.instrument.id,
            book_type=BookType.L3_MBO,
        )

        # Assert
        assert book.book_type == BookType.L3_MBO

    def test_best_bid_or_ask_price_with_no_orders_returns_none(self):
        # Arrange
        book = OrderBook(
            instrument_id=self.instrument.id,
            book_type=BookType.L2_MBP,
        )

        # Act, Assert
        assert book.best_bid_price() is None
        assert book.best_ask_price() is None

    def test_best_bid_or_ask_qty_with_no_orders_returns_none(self):
        # Arrange
        book = OrderBook(
            instrument_id=self.instrument.id,
            book_type=BookType.L2_MBP,
        )

        # Act, Assert
        assert book.best_bid_size() is None
        assert book.best_ask_size() is None

    def test_spread_with_no_orders_returns_none(self):
        # Arrange
        book = OrderBook(
            instrument_id=self.instrument.id,
            book_type=BookType.L2_MBP,
        )

        # Act, Assert
        assert book.spread() is None

    @pytest.mark.parametrize(
        "order_side",
        [
            OrderSide.BUY,
            OrderSide.SELL,
        ],
    )
    def test_get_avg_px_for_quantity_when_no_market(self, order_side: OrderSide) -> None:
        # Arrange
        book = OrderBook(
            instrument_id=self.instrument.id,
            book_type=BookType.L2_MBP,
        )

        quantity = Quantity.from_str("1.0")

        # Act, Assert
        assert book.get_avg_px_for_quantity(quantity, order_side) == 0.0

    @pytest.mark.parametrize(
        ("order_side", "expected"),
        [
            [OrderSide.BUY, 11.0],
            [OrderSide.SELL, 10.0],
        ],
    )
    def test_get_avg_px_for_quantity(self, order_side: OrderSide, expected: float) -> None:
        # Arrange
        book = OrderBook(
            instrument_id=self.instrument.id,
            book_type=BookType.L2_MBP,
        )

        book.add(
            BookOrder(
                price=Price(10.0, 1),
                size=Quantity(5.0, 0),
                side=OrderSide.BUY,
                order_id=0,
            ),
            0,
            0,
        )
        book.add(
            BookOrder(
                price=Price(11.0, 1),
                size=Quantity(6.0, 0),
                side=OrderSide.SELL,
                order_id=1,
            ),
            1,
            1,
        )

        quantity = Quantity.from_str("1.0")

        # Act
        result = book.get_avg_px_for_quantity(quantity, order_side)

        # Assert
        assert result == expected

    def test_add_orders_to_book(self):
        # Arrange
        book = OrderBook(
            instrument_id=self.instrument.id,
            book_type=BookType.L2_MBP,
        )

        # Act
        book.add(
            BookOrder(
                price=Price(10.0, 1),
                size=Quantity(5.0, 0),
                side=OrderSide.BUY,
                order_id=0,
            ),
            0,
            0,
        )
        book.add(
            BookOrder(
                price=Price(11.0, 1),
                size=Quantity(6.0, 0),
                side=OrderSide.SELL,
                order_id=1,
            ),
            1,
            1,
        )

        print(book.pprint())

        # Assert
        assert book.best_bid_price() == 10.0
        assert book.best_ask_price() == 11.0
        assert book.best_bid_size() == 5.0
        assert book.best_ask_size() == 6.0
        assert book.spread() == 1.0
        assert book.midpoint() == 10.5
        assert len(book.bids()) == 1
        assert len(book.asks()) == 1
        assert (
            repr(book.bids())
            == "[Level(price=10.0, orders=[BookOrder { side: Buy, price: 10.0, size: 5, order_id: 10000000000 }])]"
        )
        assert (
            repr(book.asks())
            == "[Level(price=11.0, orders=[BookOrder { side: Sell, price: 11.0, size: 6, order_id: 11000000000 }])]"
        )
        bid_level = book.bids()[0]
        ask_level = book.asks()[0]
        assert len(bid_level.orders()) == 1
        assert len(ask_level.orders()) == 1
        assert bid_level.price == Price.from_str("10.0")
        assert ask_level.price == Price.from_str("11.0")

    def test_repr(self):
        book = OrderBook(
            instrument_id=self.instrument.id,
            book_type=BookType.L2_MBP,
        )

        # Act
        book.add(
            BookOrder(
                price=Price(10.0, 1),
                size=Quantity(5.0, 0),
                side=OrderSide.BUY,
                order_id=0,
            ),
            ts_event=0,
        )
        book.add(
            BookOrder(
                price=Price(11.0, 1),
                size=Quantity(6.0, 0),
                side=OrderSide.SELL,
                order_id=0,
            ),
            ts_event=0,
        )

        # Assert
        assert isinstance(repr(book), str)  # <-- calls pprint internally

    def test_pprint_when_no_orders(self):
        ob = OrderBook(
            instrument_id=TestIdStubs.audusd_id(),
            book_type=BookType.L2_MBP,
        )
        result = ob.pprint()

        assert result == "╭──────┬───────┬──────╮\n│ bids │ price │ asks │\n├──────┼───────┼──────┤"

    # TODO(cs): Repair test
    #     def test_pprint_full_book(self):
    #         result = self.sample_book.pprint()
    #         print(result)
    #         expected = """bids     price   asks
    # ------  -------  ------
    #         0.90000  [20.0]
    #         0.88700  [10.0]
    #         0.88600  [5.0]
    # [4.0]   0.83000
    # [1.0]   0.82000"""
    #         assert result == expected

    def test_add(self):
        self.empty_book.add(
            BookOrder(
                price=Price(10.0, 1),
                size=Quantity(5.0, 0),
                side=OrderSide.BUY,
                order_id=0,
            ),
            0,
            0,
        )
        assert self.empty_book.best_bid_price() == 10.0

    def test_delete_l1(self):
        book = OrderBook(
            instrument_id=self.instrument.id,
            book_type=BookType.L1_TBBO,
        )
        order = TestDataStubs.order(price=10.0, side=OrderSide.BUY)
        book.update(order, 0)
        book.delete(order, 0)

    def test_top(self):
        self.empty_book.add(
            BookOrder(
                price=Price(10.0, 1),
                size=Quantity(5.0, 0),
                side=OrderSide.BUY,
                order_id=0,
            ),
            ts_event=0,
        )
        self.empty_book.add(
            BookOrder(
                price=Price(20.0, 1),
                size=Quantity(5.0, 0),
                side=OrderSide.BUY,
                order_id=1,
            ),
            ts_event=1,
        )
        self.empty_book.add(
            BookOrder(
                price=Price(5.0, 1),
                size=Quantity(5.0, 0),
                side=OrderSide.BUY,
                order_id=2,
            ),
            ts_event=2,
        )
        self.empty_book.add(
            BookOrder(
                price=Price(25.0, 1),
                size=Quantity(5.0, 0),
                side=OrderSide.SELL,
                order_id=3,
            ),
            ts_event=3,
        )
        self.empty_book.add(
            BookOrder(
                price=Price(30.0, 1),
                size=Quantity(5.0, 0),
                side=OrderSide.SELL,
                order_id=4,
            ),
            ts_event=4,
        )
        self.empty_book.add(
            BookOrder(
                price=Price(21.0, 1),
                size=Quantity(5.0, 0),
                side=OrderSide.SELL,
                order_id=5,
            ),
            ts_event=5,
        )
        assert self.empty_book.best_bid_price() == 20.0
        assert self.empty_book.best_ask_price() == 21.0

    # TODO: TBD
    # def test_check_integrity_empty(self):
    #     self.empty_book.check_integrity()

    # def test_check_integrity_shallow(self):
    #     self.empty_book.add(BookOrder(price=10.0, size=5.0, side=OrderSide.SELL))
    #     self.empty_book.check_integrity()
    #     try:
    #         # Orders will be in cross
    #         self.empty_book.add(BookOrder(price=20.0, size=5.0, side=OrderSide.BUY))
    #     except BookIntegrityError:
    #         # Catch the integrity exception and pass to allow the test
    #         pass
    #
    #     with pytest.raises(BookIntegrityError):
    #         self.empty_book.check_integrity()
    #
    # def test_check_integrity_deep(self):
    #     self.empty_book.add(BookOrder(price=10.0, size=5, side=OrderSide.BUY))
    #     self.empty_book.add(BookOrder(price=5.0, size=5, side=OrderSide.BUY))
    #     self.empty_book.check_integrity()

    def test_orderbook_operation_update(self):
        delta = OrderBookDelta(
            instrument_id=TestIdStubs.audusd_id(),
            action=BookAction.UPDATE,
            order=BookOrder(
                OrderSide.SELL,
                Price(0.5814, 4),
                Quantity(672.45, 2),
                0,  # "4a25c3f6-76e7-7584-c5a3-4ec84808e240",
            ),
            sequence=1,
            ts_event=0,
            ts_init=0,
        )
        self.empty_book.apply_delta(delta)
        assert self.empty_book.best_ask_price() == Price(0.5814, 4)
        assert self.empty_book.count == 1
        assert self.empty_book.sequence == 1

    def test_orderbook_operation_add(self):
        delta = OrderBookDelta(
            instrument_id=TestIdStubs.audusd_id(),
            action=BookAction.ADD,
            order=BookOrder(
                OrderSide.SELL,
                Price(0.5900, 4),
                Quantity(672.45, 2),
                0,  # "4a25c3f6-76e7-7584-c5a3-4ec84808e240",
            ),
            sequence=1,
            ts_event=0,
            ts_init=0,
        )
        self.empty_book.apply_delta(delta)
        assert self.empty_book.best_ask_price() == Price(0.5900, 4)
        assert self.empty_book.count == 1
        assert self.empty_book.sequence == 1

    def test_orderbook_operations(self):
        delta = OrderBookDelta(
            instrument_id=TestIdStubs.audusd_id(),
            action=BookAction.UPDATE,
            order=BookOrder(
                OrderSide.SELL,
                Price(0.5814, 4),
                Quantity(672.45, 2),
                0,  # "4a25c3f6-76e7-7584-c5a3-4ec84808e240",
            ),
            sequence=1,
            ts_event=pd.Timestamp.utcnow().timestamp() * 1e9,
            ts_init=pd.Timestamp.utcnow().timestamp() * 1e9,
        )
        deltas = OrderBookDeltas(
            instrument_id=TestIdStubs.audusd_id(),
            deltas=[delta],
        )
        self.empty_book.apply_deltas(deltas)
        assert self.empty_book.best_ask_price() == Price(0.5814, 4)

    def test_orderbook_midpoint(self):
        assert self.sample_book.midpoint() == pytest.approx(0.858)

    def test_orderbook_midpoint_empty(self):
        assert self.empty_book.midpoint() is None

    # def test_timestamp_ns(self):
    #     delta = OrderBookDelta(
    #         instrument_id=TestIdStubs.audusd_id(),
    #         action=BookAction.ADD,
    #         order=BookOrder(
    #             0.5900,
    #             672.45,
    #             OrderSide.SELL,
    #             "4a25c3f6-76e7-7584-c5a3-4ec84808e240",
    #         ),
    #         ts_event=0,
    #         ts_init=0,
    #     )
    #     self.empty_book.apply_delta(delta)
    #     assert self.empty_book.ts_last == delta.ts_init

    @pytest.mark.skip(reason="TBD")
    def test_l3_get_price_for_volume(self):
        bid_price = self.sample_book.get_price_for_volume(True, 5.0)
        ask_price = self.sample_book.get_price_for_volume(False, 12.0)
        assert bid_price == 0.88600
        assert ask_price == 0.0

    @pytest.mark.skip(reason="TBD")
    @pytest.mark.parametrize(
        ("is_buy", "quote_volume", "expected"),
        [
            (True, 0.8860, 0.8860),
            (False, 0.8300, 0.8300),
        ],
    )
    def test_l3_get_price_for_quote_volume(self, is_buy, quote_volume, expected):
        assert self.sample_book.get_price_for_quote_volume(is_buy, quote_volume) == expected

    @pytest.mark.skip(reason="TBD")
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

    @pytest.mark.skip(reason="TBD")
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

    @pytest.mark.skip(reason="TBD")
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

    @pytest.mark.skip(reason="TBD")
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
                "deltas": msgspec.json.encode(
                    [
                        {
                            "type": "OrderBookDelta",
                            "instrument_id": self.instrument_id.value,
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
        expected_ask = Price(0.001, 3)
        expected_ask.add(BookOrder(0.001, 55.81, OrderSide.SELL, "0.00100"))
        assert book.best_ask_price() == expected_ask

        expected_bid = Price(0.990099, 6)
        expected_bid.add(BookOrder(0.990099, 2.0, OrderSide.BUY, "0.99010"))
        assert book.best_bid_price() == expected_bid

    def test_order_book_flatten(self):
        book = OrderBook(
            instrument_id=self.instrument.id,
            book_type=BookType.L1_TBBO,
        )

        deltas = [
            OrderBookDelta.from_dict(d)
            for d in [
                {
                    "type": "OrderBookDelta",
                    "instrument_id": "1.166564490-237491-0.0.BETFAIR",
                    "action": "CLEAR",
                    "order": {"side": "NO_ORDER_SIDE", "price": "0", "size": "0", "order_id": 0},
                    "flags": 0,
                    "sequence": 0,
                    "ts_event": 1576840503572000000,
                    "ts_init": 1576840503572000000,
                },
                {
                    "type": "OrderBookDelta",
                    "instrument_id": "1.166564490-237491-0.0.BETFAIR",
                    "action": "UPDATE",
                    "order": {"side": "BUY", "price": "2", "size": "77", "order_id": 181},
                    "flags": 0,
                    "sequence": 0,
                    "ts_event": 1576840503572000000,
                    "ts_init": 1576840503572000000,
                },
                {
                    "type": "OrderBookDelta",
                    "instrument_id": "1.166564490-237491-0.0.BETFAIR",
                    "action": "UPDATE",
                    "order": {"side": "BUY", "price": "1", "size": "2", "order_id": 103},
                    "flags": 0,
                    "sequence": 0,
                    "ts_event": 1576840503572000000,
                    "ts_init": 1576840503572000000,
                },
                {
                    "type": "OrderBookDelta",
                    "instrument_id": "1.166564490-237491-0.0.BETFAIR",
                    "action": "UPDATE",
                    "order": {"side": "BUY", "price": "1", "size": "40", "order_id": 107},
                    "flags": 0,
                    "sequence": 0,
                    "ts_event": 1576840503572000000,
                    "ts_init": 1576840503572000000,
                },
                {
                    "type": "OrderBookDelta",
                    "instrument_id": "1.166564490-237491-0.0.BETFAIR",
                    "action": "UPDATE",
                    "order": {"side": "BUY", "price": "1", "size": "12", "order_id": 101},
                    "flags": 0,
                    "sequence": 0,
                    "ts_event": 1576840503572000000,
                    "ts_init": 1576840503572000000,
                },
                {
                    "type": "OrderBookDelta",
                    "instrument_id": "1.166564490-237491-0.0.BETFAIR",
                    "action": "UPDATE",
                    "order": {"side": "BUY", "price": "2", "size": "331", "order_id": 192},
                    "flags": 0,
                    "sequence": 0,
                    "ts_event": 1576840503572000000,
                    "ts_init": 1576840503572000000,
                },
                {
                    "type": "OrderBookDelta",
                    "instrument_id": "1.166564490-237491-0.0.BETFAIR",
                    "action": "UPDATE",
                    "order": {"side": "BUY", "price": "2", "size": "119", "order_id": 185},
                    "flags": 0,
                    "sequence": 0,
                    "ts_event": 1576840503572000000,
                    "ts_init": 1576840503572000000,
                },
                {
                    "type": "OrderBookDelta",
                    "instrument_id": "1.166564490-237491-0.0.BETFAIR",
                    "action": "UPDATE",
                    "order": {"side": "BUY", "price": "2", "size": "9", "order_id": 194},
                    "flags": 0,
                    "sequence": 0,
                    "ts_event": 1576840503572000000,
                    "ts_init": 1576840503572000000,
                },
                {
                    "type": "OrderBookDelta",
                    "instrument_id": "1.166564490-237491-0.0.BETFAIR",
                    "action": "UPDATE",
                    "order": {"side": "BUY", "price": "2", "size": "17", "order_id": 193},
                    "flags": 0,
                    "sequence": 0,
                    "ts_event": 1576840503572000000,
                    "ts_init": 1576840503572000000,
                },
                {
                    "type": "OrderBookDelta",
                    "instrument_id": "1.166564490-237491-0.0.BETFAIR",
                    "action": "UPDATE",
                    "order": {"side": "SELL", "price": "2", "size": "0", "order_id": 195},
                    "flags": 0,
                    "sequence": 0,
                    "ts_event": 1576840503572000000,
                    "ts_init": 1576840503572000000,
                },
            ]
        ]

        for delta in deltas:
            book.apply(delta)
            data = book.flatten(3)
            print(data)
