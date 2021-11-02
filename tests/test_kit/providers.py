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

import os
from typing import List

import orjson
import pandas as pd
from pandas import DataFrame

from nautilus_trader.backtest.data.loaders import CSVBarDataLoader
from nautilus_trader.backtest.data.loaders import CSVTickDataLoader
from nautilus_trader.backtest.data.loaders import ParquetTickDataLoader
from nautilus_trader.backtest.data.loaders import TardisQuoteDataLoader
from nautilus_trader.backtest.data.loaders import TardisTradeDataLoader
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.data import Order
from tests.test_kit import PACKAGE_ROOT


class TestDataProvider:
    @staticmethod
    def ethusdt_trades() -> DataFrame:
        path = os.path.join(PACKAGE_ROOT, "data", "binance-ethusdt-trades.csv")
        return CSVTickDataLoader.load(path)

    @staticmethod
    def audusd_ticks() -> DataFrame:
        path = os.path.join(PACKAGE_ROOT, "data", "truefx-audusd-ticks.csv")
        return CSVTickDataLoader.load(path)

    @staticmethod
    def usdjpy_ticks() -> DataFrame:
        path = os.path.join(PACKAGE_ROOT, "data", "truefx-usdjpy-ticks.csv")
        return CSVTickDataLoader.load(path)

    @staticmethod
    def gbpusd_1min_bid() -> DataFrame:
        path = os.path.join(PACKAGE_ROOT, "data", "fxcm-gbpusd-m1-bid-2012.csv")
        return CSVBarDataLoader.load(path)

    @staticmethod
    def gbpusd_1min_ask() -> DataFrame:
        path = os.path.join(PACKAGE_ROOT, "data", "fxcm-gbpusd-m1-ask-2012.csv")
        return CSVBarDataLoader.load(path)

    @staticmethod
    def usdjpy_1min_bid() -> DataFrame:
        path = os.path.join(PACKAGE_ROOT, "data", "fxcm-usdjpy-m1-bid-2013.csv")
        return CSVBarDataLoader.load(path)

    @staticmethod
    def usdjpy_1min_ask() -> DataFrame:
        path = os.path.join(PACKAGE_ROOT, "data", "fxcm-usdjpy-m1-ask-2013.csv")
        return CSVBarDataLoader.load(path)

    @staticmethod
    def tardis_trades() -> DataFrame:
        path = os.path.join(PACKAGE_ROOT, "data", "tardis_trades.csv")
        return TardisTradeDataLoader.load(path)

    @staticmethod
    def tardis_quotes() -> DataFrame:
        path = os.path.join(PACKAGE_ROOT, "data", "tardis_quotes.csv")
        return TardisQuoteDataLoader.load(path)

    @staticmethod
    def parquet_btcusdt_trades() -> DataFrame:
        path = os.path.join(PACKAGE_ROOT, "data", "binance-btcusdt-trades.parquet")
        return ParquetTickDataLoader.load(path)

    @staticmethod
    def parquet_btcusdt_quotes() -> DataFrame:
        path = os.path.join(PACKAGE_ROOT, "data", "binance-btcusdt-quotes.parquet")
        return ParquetTickDataLoader.load(path)

    @staticmethod
    def binance_btcusdt_instrument():
        path = os.path.join(PACKAGE_ROOT, "data", "binance-btcusdt-instrument-repr.txt")
        with open(path, "r") as f:
            return f.readline()

    @staticmethod
    def l1_feed():
        updates = []
        for _, row in TestDataProvider.usdjpy_ticks().iterrows():
            for side, order_side in zip(("bid", "ask"), (OrderSide.BUY, OrderSide.SELL)):
                updates.append(
                    {
                        "op": "update",
                        "order": Order(
                            price=Price(row[side], precision=6),
                            size=Quantity(1e9, precision=2),
                            side=order_side,
                        ),
                    }
                )
        return updates

    @staticmethod
    def l2_feed() -> List:
        def parse_line(d):
            if "status" in d:
                return {}
            elif "close_price" in d:
                # return {'timestamp': d['remote_timestamp'], "close_price": d['close_price']}
                return {}
            if "trade" in d:
                return {
                    "timestamp": d["remote_timestamp"],
                    "op": "trade",
                    "trade": TradeTick(
                        instrument_id=InstrumentId(Symbol("TEST"), Venue("BETFAIR")),
                        price=Price(d["trade"]["price"], 4),
                        size=Quantity(d["trade"]["volume"], 4),
                        aggressor_side=d["trade"]["side"],
                        match_id=(d["trade"]["trade_id"]),
                        ts_event=millis_to_nanos(pd.Timestamp(d["remote_timestamp"]).timestamp()),
                        ts_init=millis_to_nanos(
                            pd.Timestamp(
                                d["remote_timestamp"]
                            ).timestamp()  # TODO(cs): Hardcoded identical for now
                        ),
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
                "order": Order(
                    price=Price(order_like["price"], precision=6),
                    size=Quantity(abs(order_like["volume"]), precision=4),
                    # Betting sides are reversed
                    side={2: OrderSide.BUY, 1: OrderSide.SELL}[order_like["side"]],
                    id=str(order_like["order_id"]),
                ),
            }

        return [
            parse_line(line)
            for line in orjson.loads(open(PACKAGE_ROOT + "/data/L2_feed.json").read())
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
                        order=Order(
                            price=Price(data["price"], precision=10),
                            size=Quantity(abs(data["size"]), precision=10),
                            side=side,
                            id=str(data["order_id"]),
                        ),
                    )
                else:
                    yield dict(
                        op="update",
                        order=Order(
                            price=Price(data["price"], precision=10),
                            size=Quantity(abs(data["size"]), precision=10),
                            side=side,
                            id=str(data["order_id"]),
                        ),
                    )

        return [
            msg
            for data in orjson.loads(open(PACKAGE_ROOT + "/data/L3_feed.json").read())
            for msg in parser(data)
        ]
