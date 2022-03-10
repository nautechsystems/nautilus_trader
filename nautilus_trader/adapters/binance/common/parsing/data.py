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

from typing import Dict, List, Tuple

from nautilus_trader.adapters.binance.common.types import BinanceBar
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.model.data.bar import BarSpecification
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AggregationSource
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.data import Order
from nautilus_trader.model.orderbook.data import OrderBookDelta
from nautilus_trader.model.orderbook.data import OrderBookDeltas
from nautilus_trader.model.orderbook.data import OrderBookSnapshot


def parse_trade_tick_http(instrument_id: InstrumentId, msg: Dict, ts_init: int) -> TradeTick:
    return TradeTick(
        instrument_id=instrument_id,
        price=Price.from_str(msg["price"]),
        size=Quantity.from_str(msg["qty"]),
        aggressor_side=AggressorSide.SELL if msg["isBuyerMaker"] else AggressorSide.BUY,
        trade_id=TradeId(str(msg["id"])),
        ts_event=millis_to_nanos(msg["time"]),
        ts_init=ts_init,
    )


def parse_bar_http(bar_type: BarType, values: List, ts_init: int) -> BinanceBar:
    return BinanceBar(
        bar_type=bar_type,
        open=Price.from_str(values[1]),
        high=Price.from_str(values[2]),
        low=Price.from_str(values[3]),
        close=Price.from_str(values[4]),
        volume=Quantity.from_str(values[5]),
        quote_volume=Quantity.from_str(values[7]),
        count=values[8],
        taker_buy_base_volume=Quantity.from_str(values[9]),
        taker_buy_quote_volume=Quantity.from_str(values[10]),
        ts_event=millis_to_nanos(values[0]),
        ts_init=ts_init,
    )


def parse_book_snapshot(
    instrument_id: InstrumentId, msg: Dict, update_id: int, ts_init: int
) -> OrderBookSnapshot:
    ts_event: int = ts_init

    return OrderBookSnapshot(
        instrument_id=instrument_id,
        book_type=BookType.L2_MBP,
        bids=[[float(o[0]), float(o[1])] for o in msg.get("bids")],
        asks=[[float(o[0]), float(o[1])] for o in msg.get("asks")],
        ts_event=ts_event,
        ts_init=ts_init,
        update_id=update_id,
    )


def parse_diff_depth_stream_ws(
    instrument_id: InstrumentId, msg: Dict, ts_init: int
) -> OrderBookDeltas:
    ts_event: int = millis_to_nanos(msg["E"])
    update_id: int = msg["U"]

    bid_deltas: List[OrderBookDelta] = [
        parse_book_delta_ws(instrument_id, OrderSide.BUY, d, ts_event, ts_init, update_id)
        for d in msg["b"]
    ]
    ask_deltas: List[OrderBookDelta] = [
        parse_book_delta_ws(instrument_id, OrderSide.SELL, d, ts_event, ts_init, update_id)
        for d in msg["a"]
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
    delta: Tuple[str, str],
    ts_event: int,
    ts_init: int,
    update_id: int,
) -> OrderBookDelta:
    price = float(delta[0])
    size = float(delta[1])

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


def parse_quote_tick_ws(instrument_id: InstrumentId, msg: Dict, ts_init: int) -> QuoteTick:
    return QuoteTick(
        instrument_id=instrument_id,
        bid=Price.from_str(msg["b"]),
        ask=Price.from_str(msg["a"]),
        bid_size=Quantity.from_str(msg["B"]),
        ask_size=Quantity.from_str(msg["A"]),
        ts_event=ts_init,  # TODO: Investigate
        ts_init=ts_init,
    )


def parse_trade_tick_ws(instrument_id: InstrumentId, msg: Dict, ts_init: int) -> TradeTick:
    return TradeTick(
        instrument_id=instrument_id,
        price=Price.from_str(msg["p"]),
        size=Quantity.from_str(msg["q"]),
        aggressor_side=AggressorSide.SELL if msg["m"] else AggressorSide.BUY,
        trade_id=TradeId(str(msg["t"])),
        ts_event=millis_to_nanos(msg["T"]),
        ts_init=ts_init,
    )


def parse_bar_ws(
    instrument_id: InstrumentId,
    kline: Dict,
    ts_event: int,
    ts_init: int,
) -> BinanceBar:
    interval = kline["i"]
    resolution = interval[1]
    if resolution == "m":
        aggregation = BarAggregation.MINUTE
    elif resolution == "h":
        aggregation = BarAggregation.HOUR
    elif resolution == "d":
        aggregation = BarAggregation.DAY
    else:  # pragma: no cover (design-time error)
        raise RuntimeError(f"unsupported time aggregation resolution, was {resolution}")

    bar_spec = BarSpecification(
        step=int(interval[0]),
        aggregation=aggregation,
        price_type=PriceType.LAST,
    )

    bar_type = BarType(
        instrument_id=instrument_id,
        bar_spec=bar_spec,
        aggregation_source=AggregationSource.EXTERNAL,
    )

    return BinanceBar(
        bar_type=bar_type,
        open=Price.from_str(kline["o"]),
        high=Price.from_str(kline["h"]),
        low=Price.from_str(kline["l"]),
        close=Price.from_str(kline["c"]),
        volume=Quantity.from_str(kline["v"]),
        quote_volume=Quantity.from_str(kline["q"]),
        count=kline["n"],
        taker_buy_base_volume=Quantity.from_str(kline["V"]),
        taker_buy_quote_volume=Quantity.from_str(kline["Q"]),
        ts_event=ts_event,
        ts_init=ts_init,
    )
