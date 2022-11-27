# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

import json
from typing import Optional

import pandas as pd

from nautilus_trader.backtest.data.providers import TestDataProvider
from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.backtest.data.wranglers import QuoteTickDataWrangler
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.model.data.bar import Bar
from nautilus_trader.model.data.bar import BarSpecification
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.data.ticker import Ticker
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.data.venue import VenueStatusUpdate
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import InstrumentStatus
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.enums import VenueStatus
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.book import OrderBook
from nautilus_trader.model.orderbook.data import BookOrder
from nautilus_trader.model.orderbook.data import OrderBookDelta
from nautilus_trader.model.orderbook.data import OrderBookDeltas
from nautilus_trader.model.orderbook.data import OrderBookSnapshot
from nautilus_trader.model.orderbook.ladder import Ladder
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs
from tests import TEST_DATA_DIR


class TestDataStubs:
    @staticmethod
    def ticker(instrument_id=None) -> Ticker:
        return Ticker(
            instrument_id=instrument_id or TestIdStubs.audusd_id(),
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def quote_tick_3decimal(
        instrument_id=None,
        bid=None,
        ask=None,
        bid_volume=None,
        ask_volume=None,
    ) -> QuoteTick:
        return QuoteTick(
            instrument_id=instrument_id or TestIdStubs.usdjpy_id(),
            bid=bid or Price.from_str("90.002"),
            ask=ask or Price.from_str("90.005"),
            bid_size=bid_volume or Quantity.from_int(1_000_000),
            ask_size=ask_volume or Quantity.from_int(1_000_000),
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def quote_tick_5decimal(
        instrument_id=None,
        bid=None,
        ask=None,
    ) -> QuoteTick:
        return QuoteTick(
            instrument_id=instrument_id or TestIdStubs.audusd_id(),
            bid=bid or Price.from_str("1.00001"),
            ask=ask or Price.from_str("1.00003"),
            bid_size=Quantity.from_int(1_000_000),
            ask_size=Quantity.from_int(1_000_000),
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def quote_ticks_usdjpy() -> list[QuoteTick]:
        usdjpy = TestInstrumentProvider.default_fx_ccy("USD/JPY")
        wrangler = QuoteTickDataWrangler(instrument=usdjpy)
        provider = TestDataProvider()
        ticks = wrangler.process_bar_data(
            bid_data=provider.read_csv_bars("fxcm-usdjpy-m1-bid-2013.csv")[:2000],
            ask_data=provider.read_csv_bars("fxcm-usdjpy-m1-ask-2013.csv")[:2000],
        )
        return ticks

    @staticmethod
    def trade_tick_3decimal(
        instrument_id=None,
        price=None,
        aggressor_side=None,
        quantity=None,
    ) -> TradeTick:
        return TradeTick(
            instrument_id=instrument_id or TestIdStubs.usdjpy_id(),
            price=price or Price.from_str("1.001"),
            size=quantity or Quantity.from_int(100000),
            aggressor_side=aggressor_side or AggressorSide.BUY,
            trade_id=TradeId("123456"),
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def trade_tick_5decimal(
        instrument_id=None,
        price=None,
        aggressor_side=None,
        quantity=None,
    ) -> TradeTick:
        return TradeTick(
            instrument_id=instrument_id or TestIdStubs.audusd_id(),
            price=price or Price.from_str("1.00001"),
            size=quantity or Quantity.from_int(100000),
            aggressor_side=aggressor_side or AggressorSide.BUY,
            trade_id=TradeId("123456"),
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def bar_spec_1min_bid() -> BarSpecification:
        return BarSpecification(1, BarAggregation.MINUTE, PriceType.BID)

    @staticmethod
    def bar_spec_1min_ask() -> BarSpecification:
        return BarSpecification(1, BarAggregation.MINUTE, PriceType.ASK)

    @staticmethod
    def bar_spec_1min_last() -> BarSpecification:
        return BarSpecification(1, BarAggregation.MINUTE, PriceType.LAST)

    @staticmethod
    def bar_spec_1min_mid() -> BarSpecification:
        return BarSpecification(1, BarAggregation.MINUTE, PriceType.MID)

    @staticmethod
    def bar_spec_1sec_mid() -> BarSpecification:
        return BarSpecification(1, BarAggregation.SECOND, PriceType.MID)

    @staticmethod
    def bar_spec_100tick_last() -> BarSpecification:
        return BarSpecification(100, BarAggregation.TICK, PriceType.LAST)

    @staticmethod
    def bartype_audusd_1min_bid() -> BarType:
        return BarType(TestIdStubs.audusd_id(), TestDataStubs.bar_spec_1min_bid())

    @staticmethod
    def bartype_audusd_1min_ask() -> BarType:
        return BarType(TestIdStubs.audusd_id(), TestDataStubs.bar_spec_1min_ask())

    @staticmethod
    def bartype_gbpusd_1min_bid() -> BarType:
        return BarType(TestIdStubs.gbpusd_id(), TestDataStubs.bar_spec_1min_bid())

    @staticmethod
    def bartype_gbpusd_1min_ask() -> BarType:
        return BarType(TestIdStubs.gbpusd_id(), TestDataStubs.bar_spec_1min_ask())

    @staticmethod
    def bartype_gbpusd_1sec_mid() -> BarType:
        return BarType(TestIdStubs.gbpusd_id(), TestDataStubs.bar_spec_1sec_mid())

    @staticmethod
    def bartype_usdjpy_1min_bid() -> BarType:
        return BarType(TestIdStubs.usdjpy_id(), TestDataStubs.bar_spec_1min_bid())

    @staticmethod
    def bartype_usdjpy_1min_ask() -> BarType:
        return BarType(TestIdStubs.usdjpy_id(), TestDataStubs.bar_spec_1min_ask())

    @staticmethod
    def bartype_btcusdt_binance_100tick_last() -> BarType:
        return BarType(TestIdStubs.btcusdt_binance_id(), TestDataStubs.bar_spec_100tick_last())

    @staticmethod
    def bartype_adabtc_binance_1min_last() -> BarType:
        return BarType(TestIdStubs.adabtc_binance_id(), TestDataStubs.bar_spec_1min_last())

    @staticmethod
    def bar_5decimal() -> Bar:
        return Bar(
            bar_type=TestDataStubs.bartype_audusd_1min_bid(),
            open=Price.from_str("1.00002"),
            high=Price.from_str("1.00004"),
            low=Price.from_str("1.00001"),
            close=Price.from_str("1.00003"),
            volume=Quantity.from_int(1_000_000),
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def bar_3decimal() -> Bar:
        return Bar(
            bar_type=TestDataStubs.bartype_usdjpy_1min_bid(),
            open=Price.from_str("90.002"),
            high=Price.from_str("90.004"),
            low=Price.from_str("90.001"),
            close=Price.from_str("90.003"),
            volume=Quantity.from_int(1_000_000),
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def order(price: float = 100, side: OrderSide = OrderSide.BUY, size=10):
        return BookOrder(price=price, size=size, side=side)

    @staticmethod
    def ladder(reverse: bool, orders: list[BookOrder]):
        ladder = Ladder(reverse=reverse, price_precision=2, size_precision=2)
        for order in orders:
            ladder.add(order)
        return ladder

    @staticmethod
    def order_book(
        instrument=None,
        book_type=BookType.L2_MBP,
        bid_price=10,
        ask_price=15,
        bid_levels=3,
        ask_levels=3,
        bid_volume=10,
        ask_volume=10,
    ) -> OrderBook:
        instrument = instrument or TestInstrumentProvider.default_fx_ccy("AUD/USD")
        order_book = OrderBook.create(
            instrument=instrument,
            book_type=book_type,
        )
        snapshot = TestDataStubs.order_book_snapshot(
            instrument_id=instrument.id,
            bid_price=bid_price,
            ask_price=ask_price,
            bid_levels=bid_levels,
            ask_levels=ask_levels,
            bid_volume=bid_volume,
            ask_volume=ask_volume,
        )
        order_book.apply_snapshot(snapshot)
        return order_book

    @staticmethod
    def order_book_snapshot(
        instrument_id=None,
        bid_price=10,
        ask_price=15,
        bid_levels=3,
        ask_levels=3,
        bid_volume=10,
        ask_volume=10,
        book_type=BookType.L2_MBP,
    ) -> OrderBookSnapshot:
        err = "Too many levels generated; orders will be in cross. Increase bid/ask spread or reduce number of levels"
        assert bid_price < ask_price, err

        return OrderBookSnapshot(
            instrument_id=instrument_id or TestIdStubs.audusd_id(),
            book_type=book_type,
            bids=[(float(bid_price - i), float(bid_volume * (1 + i))) for i in range(bid_levels)],
            asks=[(float(ask_price + i), float(ask_volume * (1 + i))) for i in range(ask_levels)],
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def order_book_delta(instrument_id: Optional[InstrumentId] = None, order=None):
        return OrderBookDelta(
            instrument_id=instrument_id or TestIdStubs.audusd_id(),
            book_type=BookType.L2_MBP,
            action=BookAction.ADD,
            order=order or TestDataStubs.order(),
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def order_book_deltas(deltas=None):
        return OrderBookDeltas(
            instrument_id=TestIdStubs.audusd_id(),
            book_type=BookType.L2_MBP,
            deltas=deltas or [TestDataStubs.order_book_delta()],
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def venue_status_update(
        venue: Venue = None,
        status: VenueStatus = None,
    ):
        return VenueStatusUpdate(
            venue=venue or Venue("BINANCE"),
            status=status or VenueStatus.OPEN,
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def instrument_status_update(
        instrument_id: InstrumentId = None,
        status: InstrumentStatus = None,
    ):
        return InstrumentStatusUpdate(
            instrument_id=instrument_id or InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE")),
            status=status or InstrumentStatus.PAUSE,
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def l1_feed():
        provider = TestDataProvider()
        updates = []
        for _, row in provider.read_csv_ticks("truefx-usdjpy-ticks.csv").iterrows():
            for side, order_side in zip(("bid", "ask"), (OrderSide.BUY, OrderSide.SELL)):
                updates.append(
                    {
                        "op": "update",
                        "order": BookOrder(
                            price=Price(row[side], precision=6),
                            size=Quantity(1e9, precision=2),
                            side=order_side,
                        ),
                    },
                )
        return updates

    @staticmethod
    def l2_feed() -> list:
        def parse_line(d):
            if "status" in d:
                return {}
            elif "close_price" in d:
                # return {'timestamp': d['remote_timestamp'], "close_price": d['close_price']}
                return {}
            if "trade" in d:
                ts = millis_to_nanos(pd.Timestamp(d["remote_timestamp"]).timestamp())
                return {
                    "timestamp": d["remote_timestamp"],
                    "op": "trade",
                    "trade": TradeTick(
                        instrument_id=InstrumentId(Symbol("TEST"), Venue("BETFAIR")),
                        price=Price(d["trade"]["price"], 4),
                        size=Quantity(d["trade"]["volume"], 4),
                        aggressor_side=d["trade"]["side"],
                        trade_id=TradeId(d["trade"]["trade_id"]),
                        ts_event=ts,
                        ts_init=ts,
                    ),
                }
            elif "level" in d and d["level"]["orders"][0]["volume"] == 0:
                op = "delete"
            else:
                op = "update"
            order_like = d["level"]["orders"][0] if op != "trade" else d["trade"]
            return {
                "timestamp": d["remote_timestamp"],
                "op": op,
                "order": BookOrder(
                    price=Price(order_like["price"], precision=6),
                    size=Quantity(abs(order_like["volume"]), precision=4),
                    # Betting sides are reversed
                    side={2: OrderSide.BUY, 1: OrderSide.SELL}[order_like["side"]],
                    id=str(order_like["order_id"]),
                ),
            }

        return [
            parse_line(line) for line in json.loads(open(TEST_DATA_DIR + "/L2_feed.json").read())
        ]

    @staticmethod
    def l3_feed():
        def parser(data):
            parsed = data
            if not isinstance(parsed, list):
                # print(parsed)
                return
            elif isinstance(parsed, list):
                channel, updates = parsed
                if not isinstance(updates[0], list):
                    updates = [updates]
            else:
                raise KeyError()
            if isinstance(updates, int):
                print("Err", updates)
                return
            for values in updates:
                keys = ("order_id", "price", "size")
                data = dict(zip(keys, values))
                side = OrderSide.BUY if data["size"] >= 0 else OrderSide.SELL
                if data["price"] == 0:
                    yield dict(
                        op="delete",
                        order=BookOrder(
                            price=Price(data["price"], precision=9),
                            size=Quantity(abs(data["size"]), precision=9),
                            side=side,
                            id=str(data["order_id"]),
                        ),
                    )
                else:
                    yield dict(
                        op="update",
                        order=BookOrder(
                            price=Price(data["price"], precision=9),
                            size=Quantity(abs(data["size"]), precision=9),
                            side=side,
                            id=str(data["order_id"]),
                        ),
                    )

        return [
            msg
            for data in json.loads(open(TEST_DATA_DIR + "/L3_feed.json").read())
            for msg in parser(data)
        ]
