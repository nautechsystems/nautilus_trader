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

import json

import pandas as pd
import pytest

from nautilus_trader.model.c_enums.order_side import OrderSide
from nautilus_trader.model.orderbook.order import Order
from nautilus_trader.model.orderbook.orderbook import L1OrderBook
from nautilus_trader.model.orderbook.orderbook import L2OrderBook
from nautilus_trader.model.orderbook.orderbook import L3OrderBook
from tests.test_kit import PACKAGE_ROOT


@pytest.fixture()
def l2_feed():
    def parse_line(d):
        if "status" in d:
            return {}
        elif "close_price" in d:
            # return {'timestamp': d['remote_timestamp'], "close_price": d['close_price']}
            return {}
        if "trade" in d:
            return {}
            # data = TradeTick()
        elif "level" in d and d["level"]["orders"][0]["volume"] == 0:
            op = "delete"
        else:
            op = "update"
        order_like = d["level"]["orders"][0] if op != "trade" else d["trade"]
        return {
            "timestamp": d["remote_timestamp"],
            "op": op,
            "order": Order(
                price=order_like["price"],
                volume=abs(order_like["volume"]),
                # Betting sides are reversed
                side={2: OrderSide.BUY, 1: OrderSide.SELL}[order_like["side"]],
                id=str(order_like["order_id"]),
            ),
        }

    return [
        parse_line(line)
        for line in json.loads(open(PACKAGE_ROOT + "/data/L2_feed.json").read())
    ]


@pytest.fixture()
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
            keys = ("order_id", "price", "volume")
            data = dict(zip(keys, values))
            side = OrderSide.BUY if data["volume"] >= 0 else OrderSide.SELL
            if data["price"] == 0:
                yield dict(
                    op="delete",
                    order=Order(
                        price=data["price"],
                        volume=abs(data["volume"]),
                        side=side,
                        id=str(data["order_id"]),
                    ),
                )
            else:
                yield dict(
                    op="update",
                    order=Order(
                        price=data["price"],
                        volume=abs(data["volume"]),
                        side=side,
                        id=str(data["order_id"]),
                    ),
                )

    return [
        msg
        for data in json.loads(open(PACKAGE_ROOT + "/data/L3_feed.json").read())
        for msg in parser(data)
    ]


def test_l3_feed(l3_feed):
    ob = L3OrderBook()
    # Updates that cause the book to fail integrity checks will be deleted immediately, but we may get also delete later
    skip_deletes = []

    for i, m in enumerate(l3_feed):
        # print(f"[{i}]", m, ob.repr(), "\n") # Print ob summary
        if m["op"] == "update":
            ob.update(order=m["order"])
            if not ob._check_integrity(deep=False):
                ob.delete(order=m["order"])
                skip_deletes.append(m["order"].id)
        elif m["op"] == "delete" and m["order"].id not in skip_deletes:
            ob.delete(order=m["order"])
        assert ob._check_integrity(deep=False)
    assert i == 100_047
    assert ob.best_ask.price == 61405.27923706 and ob.best_ask.volume == 0.12227
    assert ob.best_bid.price == 61391 and ob.best_bid.volume == 1


def test_l2_feed(l2_feed):
    ob = L2OrderBook()

    # Duplicate delete messages
    skip = [
        (12152, "378a3caf-0262-4d8b-95b6-8df65312b9f3"),
        (28646, "8101452c-8a80-4ca9-b0d9-c472691cec28"),
        (68431, "8913f4bf-cc49-4e23-b05d-5eeed948a454"),
    ]

    for i, m in enumerate(l2_feed):
        if not m or (i, m["order"].id) in skip:
            continue
        # print(f"[{i}]", "\n",  m, "\n", ob.repr(), "\n")
        #     print('')
        if m["op"] == "update":
            ob.update(order=m["order"])
        elif m["op"] == "delete":
            ob.delete(order=m["order"])
        assert ob._check_integrity(deep=False)
    assert i == 68462


@pytest.fixture()
def l1_feed():
    df = pd.read_csv(PACKAGE_ROOT + "/data/truefx-usdjpy-ticks.csv")
    updates = []
    for _, row in df.iterrows():
        for side, order_side in zip(("bid", "ask"), (OrderSide.BUY, OrderSide.SELL)):
            updates.append(
                {
                    "op": "update",
                    "order": Order(price=row[side], volume=1e9, side=order_side),
                }
            )
    return updates


def test_l1_orderbook(l1_feed):
    ob = L1OrderBook()
    for i, m in enumerate(l1_feed):
        print(f"[{i}]", "\n", m, "\n", ob.repr(), "\n")
        print("")
        if m["op"] == "update":
            ob.update(order=m["order"])
        else:
            raise KeyError
        assert ob._check_integrity(deep=False)
