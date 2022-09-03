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

from typing import Any, Dict, List

import pandas as pd

from nautilus_trader.adapters.ftx.core.types import FTXTicker
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.data import Order
from nautilus_trader.model.orderbook.data import OrderBookDelta
from nautilus_trader.model.orderbook.data import OrderBookDeltas
from nautilus_trader.model.orderbook.data import OrderBookSnapshot


def parse_book_partial_ws(
    instrument_id: InstrumentId,
    data: Dict[str, Any],
    ts_init: int,
) -> OrderBookSnapshot:
    return OrderBookSnapshot(
        instrument_id=instrument_id,
        book_type=BookType.L2_MBP,
        bids=[[o[0], o[1]] for o in data.get("bids")],
        asks=[[o[0], o[1]] for o in data.get("asks")],
        ts_event=millis_to_nanos(data["time"]),
        ts_init=ts_init,
        update_id=data["checksum"],
    )


def parse_book_update_ws(
    instrument_id: InstrumentId,
    data: Dict[str, Any],
    ts_init: int,
) -> OrderBookDeltas:
    ts_event: int = millis_to_nanos(data["time"])
    update_id: int = data["checksum"]

    bid_deltas: List[OrderBookDelta] = [
        parse_book_delta_ws(instrument_id, OrderSide.BUY, d, ts_event, ts_init, update_id)
        for d in data["bids"]
    ]
    ask_deltas: List[OrderBookDelta] = [
        parse_book_delta_ws(instrument_id, OrderSide.SELL, d, ts_event, ts_init, update_id)
        for d in data["asks"]
    ]

    return OrderBookDeltas(
        instrument_id=instrument_id,
        book_type=BookType.L2_MBP,
        deltas=bid_deltas + ask_deltas,
        ts_event=ts_event,
        ts_init=ts_init,
        update_id=update_id,
    )


def parse_book_delta_ws(
    instrument_id: InstrumentId,
    side: OrderSide,
    delta: List[float],
    ts_event: int,
    ts_init: int,
    update_id: int,
) -> OrderBookDelta:
    price: float = delta[0]
    size: float = delta[1]

    order = Order(
        price=price,
        size=size,
        side=side,
    )

    return OrderBookDelta(
        instrument_id=instrument_id,
        book_type=BookType.L2_MBP,
        action=BookAction.UPDATE if size > 0.0 else BookAction.DELETE,
        order=order,
        ts_event=ts_event,
        ts_init=ts_init,
        update_id=update_id,
    )


def parse_ticker_ws(
    instrument: Instrument,
    data: Dict[str, Any],
    ts_init: int,
) -> FTXTicker:
    return FTXTicker(
        instrument_id=instrument.id,
        bid=Price(data["bid"], instrument.price_precision),
        ask=Price(data["ask"], instrument.price_precision),
        bid_size=Quantity(data["bidSize"], instrument.size_precision),
        ask_size=Quantity(data["askSize"], instrument.size_precision),
        last=Price(data["last"], instrument.price_precision),
        ts_event=millis_to_nanos(data["time"]),
        ts_init=ts_init,
    )


def parse_quote_tick_ws(
    instrument: Instrument,
    data: Dict[str, Any],
    ts_init: int,
) -> QuoteTick:
    return QuoteTick(
        instrument_id=instrument.id,
        bid=Price(data["bid"], instrument.price_precision),
        ask=Price(data["ask"], instrument.price_precision),
        bid_size=Quantity(data["bidSize"], instrument.size_precision),
        ask_size=Quantity(data["askSize"], instrument.size_precision),
        ts_event=millis_to_nanos(data["time"]),
        ts_init=ts_init,
    )


def parse_trade_ticks_ws(
    instrument: Instrument,
    data: List[Dict[str, Any]],
    ts_init: int,
) -> List[TradeTick]:
    ticks: List[TradeTick] = []
    for trade in data:
        tick: TradeTick = TradeTick(
            instrument_id=instrument.id,
            price=Price(trade["price"], instrument.price_precision),
            size=Quantity(trade["size"], instrument.size_precision),
            aggressor_side=AggressorSide.BUY if trade["side"] == "buy" else AggressorSide.SELL,
            trade_id=TradeId(str(trade["id"])),
            ts_event=pd.to_datetime(trade["time"], utc=True).value,
            ts_init=ts_init,
        )
        ticks.append(tick)

    return ticks
