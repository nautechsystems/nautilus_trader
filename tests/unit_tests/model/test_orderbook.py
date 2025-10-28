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

import copy
import pickle

import pandas as pd
import pytest

from nautilus_trader import TEST_DATA_DIR
from nautilus_trader.adapters.databento.loaders import DatabentoDataLoader
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
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

    def test_order_book_pickleable(self):
        # Arrange
        book = OrderBook(
            instrument_id=InstrumentId.from_str("1.166564490-237491-0.0.BETFAIR"),
            book_type=BookType.L2_MBP,
        )
        raw_updates = [
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
        ]
        updates = [OrderBookDelta.from_dict(upd) for upd in raw_updates]

        # Act, Assert
        for update in updates:
            book.apply_delta(update)
            copy.deepcopy(book)

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
            book_type=BookType.L1_MBP,
        )

        # Assert
        assert book.book_type == BookType.L1_MBP

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

        # Assert
        assert book.update_count == 2
        assert book.ts_last == 1
        assert book.best_bid_price() == 10.0
        assert book.best_ask_price() == 11.0
        assert book.best_bid_size() == 5.0
        assert book.best_ask_size() == 6.0
        assert book.spread() == 1.0
        assert book.midpoint() == 10.5
        assert len(book.bids()) == 1
        assert len(book.asks()) == 1
        # For L2_MBP books, order_ids are deterministic hashes of the price
        # NOTE: We test hash properties rather than exact values because AHash produces
        # platform-specific results (different on Windows vs Unix due to architecture
        # differences). The important properties are: determinism within a platform,
        # non-zero values, and collision resistance (different prices -> different IDs).
        bid_level = book.bids()[0]
        ask_level = book.asks()[0]
        bid_order_id = bid_level.orders()[0].order_id
        ask_order_id = ask_level.orders()[0].order_id
        # Verify order_ids are non-zero and different
        assert bid_order_id > 0, "Bid order_id should be non-zero"
        assert ask_order_id > 0, "Ask order_id should be non-zero"
        assert bid_order_id != ask_order_id, "Different prices should produce different order_ids"
        assert bid_level.side == OrderSide.BUY
        assert ask_level.side == OrderSide.SELL
        assert len(bid_level.orders()) == 1
        assert len(ask_level.orders()) == 1
        assert bid_level.price == Price.from_str("10.0")
        assert ask_level.price == Price.from_str("11.0")

    def test_repr(self):
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
        assert (
            repr(book) == "OrderBook L2_MBP\n"
            "instrument: AUD/USD.SIM\n"
            "sequence: 0\n"
            "ts_last: 0\n"
            "update_count: 2\n"
            "bid_levels: 1\n"
            "ask_levels: 1\n"
            "sequence: 0\n"
            "update_count: 2\n"
            "ts_last: 0\n"
            "╭──────┬───────┬──────╮\n"
            "│ bids │ price │ asks │\n"
            "├──────┼───────┼──────┤\n"
            "│      │ 11.0  │ [6]  │\n"
            "│ [5]  │ 10.0  │      │\n"
            "╰──────┴───────┴──────╯"
        )  # <-- Calls pprint internally

    def test_pprint_when_no_orders(self):
        # Arrange
        ob = OrderBook(
            instrument_id=TestIdStubs.audusd_id(),
            book_type=BookType.L2_MBP,
        )

        # Act
        result = ob.pprint()

        # Assert
        assert (
            result == "bid_levels: 0\n"
            "ask_levels: 0\n"
            "sequence: 0\n"
            "update_count: 0\n"
            "ts_last: 0\n"
            "╭──────┬───────┬──────╮\n"
            "│ bids │ price │ asks │\n"
            "├──────┼───────┼──────┤"
        )

    def test_add(self):
        # Arrange, Act
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

        # Assert
        assert self.empty_book.best_bid_price() == 10.0

    def test_delete_l1(self):
        # Arrange
        book = OrderBook(
            instrument_id=self.instrument.id,
            book_type=BookType.L1_MBP,
        )

        order = TestDataStubs.order(price=10.0, side=OrderSide.BUY)
        book.update(order, 0)

        # Act
        book.delete(order, 0)

        # Assert
        assert len(book.bids()) == 0
        assert len(book.asks()) == 0

    def test_top(self):
        # Arrange, Act
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

        # Assert
        assert self.empty_book.best_bid_price() == 20.0
        assert self.empty_book.best_ask_price() == 21.0

    def test_orderbook_operation_update(self):
        # Arrange
        delta = OrderBookDelta(
            instrument_id=TestIdStubs.audusd_id(),
            action=BookAction.UPDATE,
            order=BookOrder(
                OrderSide.SELL,
                Price(0.5814, 4),
                Quantity(672.45, 2),
                0,  # "4a25c3f6-76e7-7584-c5a3-4ec84808e240",
            ),
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )

        # Act
        self.empty_book.apply_delta(delta)

        # Assert
        assert self.empty_book.best_ask_price() == Price(0.5814, 4)
        assert self.empty_book.update_count == 1
        assert self.empty_book.sequence == 1

    def test_orderbook_operation_add(self):
        # Arrange
        delta = OrderBookDelta(
            instrument_id=TestIdStubs.audusd_id(),
            action=BookAction.ADD,
            order=BookOrder(
                OrderSide.SELL,
                Price(0.5900, 4),
                Quantity(672.45, 2),
                0,
            ),
            flags=0,
            sequence=1,
            ts_event=0,
            ts_init=0,
        )

        # Act
        self.empty_book.apply_delta(delta)

        # Assert
        assert self.empty_book.best_ask_price() == Price(0.5900, 4)
        assert self.empty_book.update_count == 1
        assert self.empty_book.sequence == 1

    def test_orderbook_operations(self):
        # Arrange
        delta = OrderBookDelta(
            instrument_id=TestIdStubs.audusd_id(),
            action=BookAction.UPDATE,
            order=BookOrder(
                OrderSide.SELL,
                Price(0.5814, 4),
                Quantity(672.45, 2),
                0,  # "4a25c3f6-76e7-7584-c5a3-4ec84808e240",
            ),
            flags=0,
            sequence=1,
            ts_event=pd.Timestamp.utcnow().timestamp() * 1e9,
            ts_init=pd.Timestamp.utcnow().timestamp() * 1e9,
        )
        deltas = OrderBookDeltas(
            instrument_id=TestIdStubs.audusd_id(),
            deltas=[delta],
        )

        # Act
        self.empty_book.apply_deltas(deltas)

        # Assert
        assert self.empty_book.best_ask_price() == Price(0.5814, 4)

    def test_orderbook_midpoint(self):
        assert self.sample_book.midpoint() == pytest.approx(0.858)

    def test_orderbook_midpoint_empty(self):
        assert self.empty_book.midpoint() is None

    def test_l3_get_avg_px_for_quantity(self):
        bid_price = self.sample_book.get_avg_px_for_quantity(Quantity(5.0, 0), 1)
        ask_price = self.sample_book.get_avg_px_for_quantity(Quantity(12.0, 0), 2)
        assert bid_price == 0.88600
        assert ask_price == 0.82800

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
    def test_get_quantity_for_price(self, is_buy, price, expected):
        assert (
            self.sample_book.get_quantity_for_price(
                Price(price, 5),
                OrderSide.BUY if is_buy else OrderSide.SELL,
            )
            == expected
        )

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
                "instrument_id": "AUD/USD.SIM",
                "deltas": [
                    {
                        "type": "OrderBookDelta",
                        "instrument_id": "AUD/USD.SIM",
                        "action": "UPDATE",
                        "order": {
                            "price": "0.99009",
                            "size": "200000",
                            "side": "BUY",
                            "order_id": 1,
                        },
                        "flags": 0,
                        "sequence": 0,
                        "ts_event": 1667288437852999936,
                        "ts_init": 1667288437852999936,
                    },
                ],
                "sequence": 0,
                "ts_event": 1667288437852999936,
                "ts_init": 1667288437852999936,
            },
        )

        book.apply(deltas)

        # Act
        book.add(
            BookOrder(
                side=OrderSide.SELL,
                price=Price(0.001, 3),
                size=Quantity(55.81, 2),
                order_id=0,
            ),
            1667288437852999936,
            0,
        )

        # Assert
        expected_ask = Price(0.00100, 5)
        assert book.best_ask_price() == expected_ask

        expected_bid = Price(0.99010, 5)
        book.add(
            BookOrder(
                side=OrderSide.BUY,
                price=Price(0.99010, 5),
                size=Quantity(2.0, 2),
                order_id=0,
            ),
            1667288437852999936,
            0,
        )
        assert book.best_bid_price() == expected_bid

    def test_book_order_pickle_round_trip(self):
        # Arrange
        book = TestDataStubs.make_book(
            instrument=self.instrument,
            book_type=BookType.L2_MBP,
            bids=[(0.0040000, 100.0)],
            asks=[(0.0010000, 55.81)],
        )
        # Act
        pickled = pickle.dumps(book)
        unpickled = pickle.loads(pickled)  # noqa: S301 (pickle safe here)

        # Assert
        assert str(book) == str(unpickled)
        assert book.bids()[0].orders()[0].price == Price.from_str("0.00400")

    def test_orderbook_deep_copy(self):
        # Arrange
        instrument_id = InstrumentId.from_str("1.166564490-237491-0.0.BETFAIR")
        book = OrderBook(instrument_id, BookType.L2_MBP)

        def make_delta(side: OrderSide, price: float, size: float, ts: int) -> OrderBookDelta:
            order = BookOrder(
                price=Price(price, 2),
                size=Quantity(size, 0),
                side=side,
                order_id=0,
            )
            return TestDataStubs.order_book_delta(
                instrument_id=instrument_id,
                order=order,
                flags=0,
                sequence=0,
                ts_init=ts,
                ts_event=ts,
            )

        updates = [
            TestDataStubs.order_book_delta_clear(instrument_id=instrument_id),
            make_delta(OrderSide.BUY, price=2.0, size=77.0, ts=1),
            make_delta(OrderSide.BUY, price=1.0, size=2.0, ts=2),
            make_delta(OrderSide.BUY, price=1.0, size=40.0, ts=3),
            make_delta(OrderSide.BUY, price=1.0, size=331.0, ts=4),
        ]

        # Act
        for update in updates:
            print(update)
            book.apply_delta(update)
            book.check_integrity()
        new = copy.deepcopy(book)

        # Assert
        assert book.ts_last == new.ts_last
        assert book.sequence == new.sequence

    def test_orderbook_esh4_glbx_20231224_mbo_l3(self) -> None:
        # Arrange
        loader = DatabentoDataLoader()
        instrument = TestInstrumentProvider.es_future(expiry_year=2024, expiry_month=3)

        path_20231224 = TEST_DATA_DIR / "databento" / "esh4-glbx-mdp3-20231224.mbo.dbn.zst"
        path_20231225 = TEST_DATA_DIR / "databento" / "esh4-glbx-mdp3-20231225.mbo.dbn.zst"

        # Act
        data = loader.from_dbn_file(
            path_20231224,
            instrument_id=instrument.id,
            as_legacy_cython=True,
        )
        data.extend(
            loader.from_dbn_file(
                path_20231225,
                instrument_id=instrument.id,
                as_legacy_cython=True,
            ),
        )

        book = TestDataStubs.make_book(
            instrument=instrument,
            book_type=BookType.L3_MBO,
        )

        for delta in data:
            book.apply_delta(delta)

        # Assert
        assert len(data) == 74544  # Includes NoOrderSide deltas that are now decoded
        assert book.ts_last == 1703548799446821072
        assert book.sequence == 59585
        assert book.update_count == 74537  # 28 NoOrderSide resolved, 7 skipped
        assert len(book.bids()) == 922
        assert len(book.asks()) == 565
        assert book.best_bid_price() == Price.from_str("4810.00")
        assert book.best_ask_price() == Price.from_str("4810.25")

    @pytest.mark.parametrize(
        ("book_type"),
        [
            BookType.L1_MBP,
            BookType.L2_MBP,
            BookType.L3_MBO,
        ],
    )
    def test_check_integrity_when_book_crossed(self, book_type: BookType) -> None:
        # Arrange
        book = OrderBook(
            instrument_id=self.instrument.id,
            book_type=book_type,
        )

        book.update(
            BookOrder(
                price=Price(11.0, 1),
                size=Quantity(5.0, 0),
                side=OrderSide.BUY,
                order_id=0,
            ),
            0,
            0,
        )
        book.update(
            BookOrder(
                price=Price(10.0, 1),
                size=Quantity(5.0, 0),
                side=OrderSide.SELL,
                order_id=0,
            ),
            0,
            0,
        )

        # Act, Assert
        assert book.best_bid_price() > book.best_ask_price()
        with pytest.raises(RuntimeError):
            book.check_integrity()

    @pytest.mark.parametrize(
        ("book_type"),
        [
            BookType.L2_MBP,
            BookType.L3_MBO,
        ],
    )
    def test_update_quote_tick_other_than_l1_raises_exception(
        self,
        book_type: BookType,
    ) -> None:
        # Arrange
        book = OrderBook(
            instrument_id=self.instrument.id,
            book_type=book_type,
        )

        # Act, Assert
        quote = TestDataStubs.quote_tick(self.instrument)
        with pytest.raises(RuntimeError):
            book.update_quote_tick(quote)

    @pytest.mark.parametrize(
        ("book_type"),
        [
            BookType.L2_MBP,
            BookType.L3_MBO,
        ],
    )
    def test_update_trade_tick_other_than_l1_raises_exception(
        self,
        book_type: BookType,
    ) -> None:
        # Arrange
        book = OrderBook(
            instrument_id=self.instrument.id,
            book_type=book_type,
        )

        # Act, Assert
        trade = TestDataStubs.trade_tick(self.instrument)
        with pytest.raises(RuntimeError):
            book.update_trade_tick(trade)
