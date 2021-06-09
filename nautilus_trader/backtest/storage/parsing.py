from itertools import repeat

import numpy as np

from nautilus_trader.core.uuid import UUID
from nautilus_trader.model.c_enums.aggressor_side import AggressorSideParser
from nautilus_trader.model.c_enums.instrument_status import InstrumentStatusParser
from nautilus_trader.model.c_enums.order_side import OrderSide
from nautilus_trader.model.c_enums.orderbook_delta import OrderBookDeltaType
from nautilus_trader.model.c_enums.orderbook_level import OrderBookLevelParser
from nautilus_trader.model.enums import OrderBookDeltaTypeParser
from nautilus_trader.model.enums import OrderSideParser
from nautilus_trader.model.events import InstrumentClosePrice
from nautilus_trader.model.events import InstrumentStatusEvent
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeMatchId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.betting import BettingInstrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.book import OrderBookData
from nautilus_trader.model.orderbook.book import OrderBookDelta
from nautilus_trader.model.orderbook.book import OrderBookDeltas
from nautilus_trader.model.orderbook.book import OrderBookSnapshot
from nautilus_trader.model.orderbook.order import Order
from nautilus_trader.model.tick import TradeTick


dictionary_columns = {
    TradeTick: ["instrument_id", "aggressor_side"],
}


def _parse_delta(msg, delta):
    return {
        "instrument_id": msg.instrument_id.value,
        "timestamp_ns": msg.timestamp_ns,
        "timestamp_origin_ns": msg.timestamp_origin_ns,
        "type": OrderBookDeltaTypeParser.to_str_py(delta.type),
        "level": OrderBookLevelParser.to_str_py(delta.level),
        "id": delta.order.id if delta.order else None,
        "price": delta.order.price if delta.order else None,
        "volume": delta.order.volume if delta.order else None,
        "side": OrderSideParser.to_str_py(delta.order.side) if delta.order else None,
    }


def _parse_order_book_data(data: OrderBookData):
    if isinstance(data, OrderBookDeltas):
        yield from [_parse_delta(msg=data, delta=delta) for delta in data.deltas]
    elif isinstance(data, OrderBookDelta):
        yield _parse_delta(msg=data, delta=data.delta)
    elif isinstance(data, OrderBookSnapshot):
        yield _parse_delta(
            data,
            OrderBookDelta(
                instrument_id=data.instrument_id,
                level=data.level,
                order=None,
                delta_type=OrderBookDeltaType.CLEAR,
                timestamp_origin_ns=data.timestamp_origin_ns,
                timestamp_ns=data.timestamp_ns,
            ),
        )
        orders = list(zip(repeat(OrderSide.BUY), data.bids)) + list(
            zip(repeat(OrderSide.SELL), data.asks)
        )
        yield from [
            _parse_delta(
                data,
                OrderBookDelta(
                    instrument_id=data.instrument_id,
                    level=data.level,
                    timestamp_ns=data.timestamp_ns,
                    timestamp_origin_ns=data.timestamp_origin_ns,
                    order=Order(price=price, volume=volume, side=side),
                    delta_type=OrderBookDeltaType.ADD,
                ),
            )
            for side, (price, volume) in orders
        ]


def _parse_trade_tick(tick: TradeTick):
    yield {
        "instrument_id": tick.instrument_id.value,
        "price": tick.price.as_double(),
        "size": tick.size.as_double(),
        "aggressor_side": AggressorSideParser.to_str_py(tick.aggressor_side),
        "match_id": tick.match_id.value,
        "timestamp_origin_ns": tick.timestamp_origin_ns,
        "timestamp_ns": tick.timestamp_ns,
    }


def _parse_instrument_status_event(event: InstrumentStatusEvent):
    yield {
        "instrument_id": event.instrument_id.value,
        "status": InstrumentStatusParser.to_str_py(event.status),
        "event_id": event.id.value,
        "timestamp_ns": event.timestamp_ns,
    }


def _parse_instrument_close_price(price: InstrumentClosePrice):
    yield {
        "instrument_id": price.instrument_id.value(),
        "close_price": price.close_price.as_double(),
        "close_type": price.close_type.name,
        "event_id": price.id.value,
        "timestamp_ns": price.timestamp_ns,
    }


# TODO (bm) - duplication with Serializers.pyx -> refactor msgpack serializer and add conversion to basic types
# TODO (bm) - Instruments only queryable by attributes listed here (not quote currency etc)


def _parse_betting_instrument(instrument: BettingInstrument):
    return {
        "venue_name": instrument.id.venue.value,
        "currency": instrument.quote_currency.code,
        "instrument_id": instrument.id.value,
        "event_type_id": instrument.event_type_id,
        "event_type_name": instrument.event_type_name,
        "competition_id": instrument.competition_id,
        "competition_name": instrument.competition_name,
        "event_id": instrument.event_id,
        "event_name": instrument.event_name,
        "event_country_code": instrument.event_country_code,
        "event_open_date": instrument.event_open_date,
        "betting_type": instrument.betting_type,
        "market_id": instrument.market_id,
        "market_name": instrument.market_name,
        "market_start_time": instrument.market_start_time,
        "market_type": instrument.market_type,
        "selection_id": instrument.selection_id,
        "selection_name": instrument.selection_name,
        "selection_handicap": instrument.selection_handicap,
        "timestamp_ns": instrument.timestamp_ns,
        "timestamp_origin_ns": instrument.timestamp_origin_ns,
    }


def _unparse_value(cls, k, v):  # noqa: C901
    # Shared attributes
    if k == "instrument_id" and cls in (
        OrderBookDelta,
        TradeTick,
        InstrumentStatusEvent,
    ):
        symbol, venue = v.rsplit(".", maxsplit=1)
        return InstrumentId(symbol=Symbol(symbol), venue=Venue(venue))
    elif k == "aggressor_side":
        return AggressorSideParser.from_str_py(v)
    elif k == "price" and cls in (TradeTick,):
        return Price.from_str(str(v))
    elif k == "event_id" and cls in (InstrumentStatusEvent,):
        return UUID.from_str(v)

    # Class specific parsing
    if cls == OrderBookDelta:
        if k == "type":
            return OrderBookDeltaTypeParser.from_str_py(v)
        elif k == "level":
            return OrderBookLevelParser.from_str_py(v)
    elif cls == TradeTick:
        if k == "size":
            return Quantity.from_str(str(v))
        if k == "match_id":
            return TradeMatchId(v)
    elif cls == InstrumentStatusEvent:
        if k == "status":
            return InstrumentStatusParser.from_str_py(v)
    return v


def _unparse(cls, d):
    """
    Used to parse/unflatten a dict into a nautilus object - use if fields need renaming or nested objects exist
    """
    if cls == OrderBookDelta:
        kw = {k: d.pop(k, None) for k in ("price", "volume", "side")}
        if not np.isnan(kw["price"]):
            d["order"] = Order(
                price=kw["price"],
                volume=kw["volume"],
                side=OrderSideParser.from_str_py(kw["side"]),
            )
        else:
            d["order"] = None
        d["delta_type"] = d.pop("type")
        d.pop("id")
        return OrderBookDeltas(
            instrument_id=d["instrument_id"],
            level=d["level"],
            deltas=[cls(**d)],
            timestamp_ns=d["timestamp_ns"],
            timestamp_origin_ns=d["timestamp_origin_ns"],
        )

    return cls(**d)


def nautilus_to_dict(obj):
    if isinstance(obj, TradeTick):
        yield from _parse_trade_tick(obj)
    elif isinstance(obj, OrderBookData):
        yield from _parse_order_book_data(obj)
    elif isinstance(obj, InstrumentStatusEvent):
        yield from _parse_instrument_status_event(obj)
    elif isinstance(obj, BettingInstrument):
        yield _parse_betting_instrument(obj)
