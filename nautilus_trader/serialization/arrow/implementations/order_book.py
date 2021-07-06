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

import itertools
from itertools import repeat
from typing import Dict, List

from nautilus_trader.model.enums import BookLevelParser
from nautilus_trader.model.enums import DeltaType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.orderbook.book import OrderBookData
from nautilus_trader.model.orderbook.book import OrderBookDelta
from nautilus_trader.model.orderbook.book import OrderBookDeltas
from nautilus_trader.model.orderbook.book import OrderBookSnapshot
from nautilus_trader.model.orderbook.order import Order


def _parse_delta(delta: OrderBookDelta):
    return OrderBookDelta.to_dict(delta)


def serialize(data: OrderBookData):
    def inner():
        if isinstance(data, OrderBookDeltas):
            yield from [_parse_delta(delta=delta) for delta in data.deltas]
        elif isinstance(data, OrderBookDelta):
            yield _parse_delta(delta=data)
        elif isinstance(data, OrderBookSnapshot):
            # For a snapshot, we store the individual deltas required to rebuild, namely a CLEAR, followed by ADDs
            yield _parse_delta(
                OrderBookDelta(
                    instrument_id=data.instrument_id,
                    level=data.level,
                    order=None,
                    delta_type=DeltaType.CLEAR,
                    ts_event_ns=data.ts_event_ns,
                    ts_recv_ns=data.ts_recv_ns,
                ),
            )
            orders = list(zip(repeat(OrderSide.BUY), data.bids)) + list(
                zip(repeat(OrderSide.SELL), data.asks)
            )
            yield from [
                _parse_delta(
                    OrderBookDelta(
                        instrument_id=data.instrument_id,
                        level=data.level,
                        ts_event_ns=data.ts_event_ns,
                        ts_recv_ns=data.ts_recv_ns,
                        order=Order(price=price, size=volume, side=side),
                        delta_type=DeltaType.ADD,
                    ),
                )
                for side, (price, volume) in orders
            ]

    return list(inner())


def deserialize(data: List[Dict]):
    def _is_orderbook_snapshot(values: list):
        if len(values) < 2:
            return False
        return values[0]["delta_type"] == "CLEAR" and values[1]["delta_type"] == "ADD"

    def _build_order_book_snapshot(values):
        # First value is a CLEAR message, which we ignore
        return OrderBookSnapshot(
            instrument_id=InstrumentId.from_str(values[1]["instrument_id"]),
            level=BookLevelParser.from_str_py(values[1]["level"]),
            bids=[
                (order["order_price"], order["order_size"])
                for order in data[1:]
                if order["order_side"] == "BUY"
            ],
            asks=[
                (order["order_price"], order["order_size"])
                for order in data[1:]
                if order["order_side"] == "SELL"
            ],
            ts_event_ns=data[1]["ts_event_ns"],
            ts_recv_ns=data[1]["ts_recv_ns"],
        )

    def _build_order_book_deltas(values):
        return OrderBookDeltas(
            instrument_id=InstrumentId.from_str(values[0]["instrument_id"]),
            level=BookLevelParser.from_str_py(values[0]["level"]),
            deltas=[OrderBookDelta.from_dict(v) for v in values],
            ts_event_ns=data[0]["ts_event_ns"],
            ts_recv_ns=data[0]["ts_recv_ns"],
        )

    assert not set([d["order_side"] for d in data]).difference((None, "BUY", "SELL")), "Wrong sides"
    results = []
    for _, chunk in itertools.groupby(data, key=timestamp_key):
        chunk = list(chunk)
        if _is_orderbook_snapshot(values=chunk):
            results.append(_build_order_book_snapshot(values=chunk))
        elif len(chunk) >= 1:
            results.append(_build_order_book_deltas(values=chunk))
    return results


def timestamp_key(x):
    if hasattr(x, "ts_event_ns"):
        return x.ts_event_ns
    elif hasattr(x, "ts_recv_ns"):
        return x.ts_recv_ns
    elif hasattr(x, "timestamp_ns"):
        return x.timestamp_ns
    elif "ts_event_ns" in x:
        return x["ts_event_ns"]
    elif "ts_recv_ns" in x:
        return x["ts_recv_ns"]
    elif "timestamp_ns" in x:
        return x["timestamp_ns"]
    else:
        raise KeyError("Can't find timestamp attribute or key")
