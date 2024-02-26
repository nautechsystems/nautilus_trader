# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
from datetime import datetime
from os import PathLike
from typing import Any

import pandas as pd
import pytz

from nautilus_trader.core.data import Data
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.data import NULL_ORDER
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarSpecification
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.data import InstrumentClose
from nautilus_trader.model.data import InstrumentStatus
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import OrderBookDepth10
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.data import VenueStatus
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import InstrumentCloseType
from nautilus_trader.model.enums import MarketStatus
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.persistence.wranglers import BarDataWrangler
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


UNIX_EPOCH = datetime(1970, 1, 1, 0, 0, 0, 0, tzinfo=pytz.utc)


class TestDataStubs:
    @staticmethod
    def quote_tick(
        instrument: Instrument | None = None,
        bid_price: float = 1.0,
        ask_price: float = 1.0,
        bid_size: float = 100_000.0,
        ask_size: float = 100_000.0,
        ts_event: int = 0,
        ts_init: int = 0,
    ) -> QuoteTick:
        inst: Instrument = instrument or TestInstrumentProvider.default_fx_ccy("AUD/USD")
        return QuoteTick(
            instrument_id=inst.id,
            bid_price=inst.make_price(bid_price),
            ask_price=inst.make_price(ask_price),
            bid_size=inst.make_qty(bid_size),
            ask_size=inst.make_qty(ask_size),
            ts_event=ts_event,
            ts_init=ts_init,
        )

    @staticmethod
    def trade_tick(
        instrument: Instrument | None = None,
        price: float = 1.0,
        size: float = 100_000,
        aggressor_side: AggressorSide = AggressorSide.BUYER,
        trade_id: str = "123456",
        ts_event: int = 0,
        ts_init: int = 0,
    ) -> TradeTick:
        inst: Instrument = instrument or TestInstrumentProvider.default_fx_ccy("AUD/USD")
        return TradeTick(
            instrument_id=inst.id,
            price=inst.make_price(price),
            size=inst.make_qty(size),
            aggressor_side=aggressor_side,
            trade_id=TradeId(trade_id),
            ts_event=ts_event,
            ts_init=ts_init,
        )

    @staticmethod
    def quote_ticks_usdjpy() -> list[QuoteTick]:
        usdjpy = TestInstrumentProvider.default_fx_ccy("USD/JPY")
        wrangler = QuoteTickDataWrangler(instrument=usdjpy)
        provider = TestDataProvider()
        ticks = wrangler.process_bar_data(
            bid_data=provider.read_csv_bars("fxcm/usdjpy-m1-bid-2013.csv")[:2000],
            ask_data=provider.read_csv_bars("fxcm/usdjpy-m1-ask-2013.csv")[:2000],
        )
        return ticks

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
    def instrument_close() -> InstrumentClose:
        from nautilus_trader.adapters.betfair.constants import BETFAIR_PRICE_PRECISION

        return InstrumentClose(
            TestIdStubs.betting_instrument_id(),
            Price(1.0, BETFAIR_PRICE_PRECISION),
            InstrumentCloseType.CONTRACT_EXPIRED,
            0,
            0,
        )

    @staticmethod
    def order(
        side: OrderSide = OrderSide.BUY,
        price: float = 100.0,
        size: float = 10.0,
    ) -> BookOrder:
        return BookOrder(
            price=Price(price, 2),
            size=Quantity(size, 0),
            side=side,
            order_id=0,
        )

    @staticmethod
    def order_book(
        instrument: Instrument | None = None,
        book_type: BookType = BookType.L2_MBP,
        bid_price: float = 10.0,
        ask_price: float = 15.0,
        bid_size: float = 10.0,
        ask_size: float = 10.0,
        bid_levels: int = 3,
        ask_levels: int = 3,
        ts_event: int = 0,
        ts_init: int = 0,
    ) -> OrderBook:
        instrument = instrument or TestInstrumentProvider.default_fx_ccy("AUD/USD")
        assert instrument
        order_book = OrderBook(
            instrument_id=instrument.id,
            book_type=book_type,
        )
        snapshot = TestDataStubs.order_book_snapshot(
            instrument=instrument,
            bid_price=bid_price,
            ask_price=ask_price,
            bid_levels=bid_levels,
            ask_levels=ask_levels,
            bid_size=bid_size,
            ask_size=ask_size,
            ts_event=ts_event,
            ts_init=ts_init,
        )
        order_book.apply_deltas(snapshot)
        return order_book

    @staticmethod
    def order_book_snapshot(
        instrument: Instrument | None = None,
        bid_price: float = 10.0,
        ask_price: float = 15.0,
        bid_size: float = 10.0,
        ask_size: float = 10.0,
        bid_levels: int = 3,
        ask_levels: int = 3,
        ts_event: int = 0,
        ts_init: int = 0,
    ) -> OrderBookDeltas:
        err = "Too many levels generated; orders will be in cross. Increase bid/ask spread or reduce number of levels"
        assert bid_price < ask_price, err
        instrument = instrument or TestInstrumentProvider.default_fx_ccy("AUD/USD")
        assert instrument
        bids = [
            BookOrder(
                OrderSide.BUY,
                instrument.make_price(bid_price - i),
                instrument.make_qty(bid_size * (1 + i)),
                0,
            )
            for i in range(bid_levels)
        ]
        asks = [
            BookOrder(
                OrderSide.SELL,
                instrument.make_price(ask_price + i),
                instrument.make_qty(ask_size * (1 + i)),
                0,
            )
            for i in range(ask_levels)
        ]

        deltas = [OrderBookDelta.clear(instrument.id, ts_event, ts_init)]
        deltas += [
            OrderBookDelta(instrument.id, BookAction.ADD, order, ts_event, ts_init)
            for order in bids + asks
        ]
        return OrderBookDeltas(
            instrument_id=instrument.id,
            deltas=deltas,
        )

    @staticmethod
    def order_book_delta(
        instrument_id: InstrumentId | None = None,
        order: BookOrder | None = None,
        ts_event: int = 0,
        ts_init: int = 0,
    ) -> OrderBookDeltas:
        return OrderBookDelta(
            instrument_id=instrument_id or TestIdStubs.audusd_id(),
            action=BookAction.UPDATE,
            order=order or TestDataStubs.order(),
            ts_event=ts_event,
            ts_init=ts_init,
        )

    @staticmethod
    def order_book_depth10(
        instrument_id: InstrumentId | None = None,
        flags: int = 0,
        sequence: int = 0,
        ts_event: int = 0,
        ts_init: int = 0,
    ) -> OrderBookDepth10:
        bids: list[BookOrder] = []
        asks: list[BookOrder] = []

        # Create bids
        price = 99.00
        quantity = 100.0
        order_id = 1

        for _ in range(10):
            order = BookOrder(
                OrderSide.BUY,
                Price(price, 2),
                Quantity(quantity, 0),
                order_id,
            )

            bids.append(order)

            price -= 1.0
            quantity += 100.0
            order_id += 1

        # Create asks
        price = 100.00
        quantity = 100.0
        order_id = 11

        for _ in range(10):
            order = BookOrder(
                OrderSide.SELL,
                Price(price, 2),
                Quantity(quantity, 0),
                order_id,
            )

            asks.append(order)

            price += 1.0
            quantity += 100.0
            order_id += 1

        bid_counts = [1] * 10
        ask_counts = [1] * 10

        return OrderBookDepth10(
            instrument_id=instrument_id or TestIdStubs.aapl_xnas_id(),
            bids=bids,
            asks=asks,
            bid_counts=bid_counts,
            ask_counts=ask_counts,
            flags=flags,
            sequence=sequence,
            ts_event=ts_event,
            ts_init=ts_init,
        )

    @staticmethod
    def order_book_delta_clear(
        instrument_id: InstrumentId | None = None,
    ) -> OrderBookDeltas:
        return OrderBookDelta(
            instrument_id=instrument_id or TestIdStubs.audusd_id(),
            action=BookAction.CLEAR,
            order=NULL_ORDER,
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def order_book_deltas(
        instrument_id: InstrumentId | None = None,
        deltas: list[OrderBookDelta] | None = None,
    ) -> OrderBookDeltas:
        return OrderBookDeltas(
            instrument_id=instrument_id or TestIdStubs.audusd_id(),
            deltas=deltas or [TestDataStubs.order_book_delta()],
        )

    @staticmethod
    def make_book(
        instrument: Instrument,
        book_type: BookType,
        bids: list[tuple] | None = None,
        asks: list[tuple] | None = None,
    ) -> OrderBook:
        book = OrderBook(
            instrument_id=instrument.id,
            book_type=book_type,
        )

        bids_counter: int = 0
        asks_counter: int = 0

        for price, size in bids or []:
            order = BookOrder(
                side=OrderSide.BUY,
                price=Price(price, instrument.price_precision),
                size=Quantity(size, instrument.size_precision),
                order_id=bids_counter,
            )
            book.add(order, 0)
            bids_counter += 1
        for price, size in asks or []:
            order = BookOrder(
                side=OrderSide.SELL,
                price=Price(price, instrument.price_precision),
                size=Quantity(size, instrument.size_precision),
                order_id=asks_counter,
            )
            book.add(order, 0)
            asks_counter += 1

        return book

    @staticmethod
    def venue_status(
        venue: Venue | None = None,
        status: MarketStatus | None = None,
    ) -> VenueStatus:
        return VenueStatus(
            venue=venue or Venue("BINANCE"),
            status=status or MarketStatus.OPEN,
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def instrument_status(
        instrument_id: InstrumentId | None = None,
        status: MarketStatus | None = None,
    ) -> InstrumentStatus:
        return InstrumentStatus(
            instrument_id=instrument_id or InstrumentId(Symbol("BTCUSDT"), Venue("BINANCE")),
            status=status or MarketStatus.PAUSE,
            ts_event=0,
            ts_init=0,
        )

    @staticmethod
    def l1_feed():
        provider = TestDataProvider()
        updates = []
        for _, row in provider.read_csv_ticks("truefx/usdjpy-ticks.csv").iterrows():
            for side, order_side in zip(("bid", "ask"), (OrderSide.BUY, OrderSide.SELL)):
                updates.append(
                    {
                        "op": "update",
                        "order": BookOrder(
                            price=Price(row[side], precision=6),
                            size=Quantity(1e9, precision=2),
                            side=order_side,
                            order_id=0,
                        ),
                    },
                )
        return updates

    @staticmethod
    def l2_feed(filename: PathLike[str] | str) -> list:
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
                    order_id=0,
                ),
            }

        return [parse_line(line) for line in json.loads(open(filename).read())]

    @staticmethod
    def l3_feed(filename: PathLike[str] | str) -> list[dict[str, Any]]:
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
                raise KeyError
            if isinstance(updates, int):
                print("Err", updates)
                return
            for values in updates:
                keys = ("order_id", "price", "size")
                data = dict(zip(keys, values))
                side = OrderSide.BUY if data["size"] >= 0 else OrderSide.SELL
                if data["price"] == 0:
                    yield {
                        "op": "delete",
                        "order": BookOrder(
                            price=Price(data["price"], precision=9),
                            size=Quantity(abs(data["size"]), precision=9),
                            side=side,
                            order_id=data["order_id"],
                        ),
                    }
                else:
                    yield {
                        "op": "update",
                        "order": BookOrder(
                            price=Price(data["price"], precision=9),
                            size=Quantity(abs(data["size"]), precision=9),
                            side=side,
                            order_id=data["order_id"],
                        ),
                    }

        return [msg for data in json.loads(open(filename).read()) for msg in parser(data)]

    @staticmethod
    def bar_data_from_csv(
        filename: str,
        bar_type: BarType,
        instrument: Instrument,
        names=None,
    ) -> list[Bar]:
        wrangler = BarDataWrangler(bar_type, instrument)
        data = TestDataProvider().read_csv(filename, names=names)
        data["timestamp"] = data["timestamp"].astype("datetime64[ms]")
        data = data.set_index("timestamp")
        bars = wrangler.process(data)
        return bars

    @staticmethod
    def binance_bars_from_csv(filename: str, bar_type: BarType, instrument: Instrument):
        names = [
            "timestamp",
            "open",
            "high",
            "low",
            "close",
            "volume",
            "ts_close",
            "quote_volume",
            "n_trades",
            "taker_buy_base_volume",
            "taker_buy_quote_volume",
            "ignore",
        ]
        return TestDataStubs.bar_data_from_csv(
            filename=filename,
            bar_type=bar_type,
            instrument=instrument,
            names=names,
        )


class MyData(Data):
    """
    Represents an example user-defined data class.
    """

    def __init__(
        self,
        value,
        ts_event=0,
        ts_init=0,
    ):
        self.value = value
        self._ts_event = ts_event
        self._ts_init = ts_init

    @property
    def ts_event(self) -> int:
        return self._ts_event

    @property
    def ts_init(self) -> int:
        return self._ts_init
