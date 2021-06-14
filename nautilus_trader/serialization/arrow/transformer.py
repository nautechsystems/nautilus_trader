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


# Transform/simplify complex objects into something we can store in a parquet file


# TODO (bm) cythonize
class OrderBookDataTransformer:
    @staticmethod
    def _parse_delta(delta: OrderBookDelta):
        return delta.to_dict()

    @staticmethod
    def serialize(data: OrderBookData):
        def inner():
            if isinstance(data, OrderBookDeltas):
                yield from [
                    OrderBookDataTransformer._parse_delta(delta=delta)
                    for delta in data.deltas
                ]
            elif isinstance(data, OrderBookDelta):
                yield OrderBookDataTransformer._parse_delta(delta=data.delta)
            elif isinstance(data, OrderBookSnapshot):
                # For a snapshot, we store the individual deltas required to rebuild, namely a CLEAR, followed by ADDs
                yield OrderBookDataTransformer._parse_delta(
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
                    OrderBookDataTransformer._parse_delta(
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

    @staticmethod
    def deserialize(data: List[Dict]):
        def _is_orderbook_snapshot(values: list):
            if len(values) < 2:
                return False
            return (
                values[0]["delta_type"] == "CLEAR" and values[1]["delta_type"] == "ADD"
            )

        def _build_orderbook_snapshot(values):
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

        assert not set([d["order_side"] for d in data]).difference(
            (None, "BUY", "SELL")
        ), "Wrong sides"
        results = []
        for _, chunk in itertools.groupby(data, key=timestamp_key):
            chunk = list(chunk)
            if _is_orderbook_snapshot(values=data):
                results.append(_build_orderbook_snapshot(values=chunk))
        return results


TRANSFORMERS = {**{x: OrderBookDataTransformer for x in OrderBookData.__subclasses__()}}


def serialize(obj: object):
    transformer = TRANSFORMERS.get(type(obj))
    if transformer is not None:
        return transformer.serialize(obj)
    return obj.to_dict()


def deserialize(cls, data: List[Dict]):
    transformer = TRANSFORMERS.get(cls)
    if transformer is not None:
        return transformer.deserialize(data)
    return [cls.from_dict(d) for d in data]


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
