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

from decimal import Decimal
from typing import List, Tuple

from nautilus_trader.adapters.binance.common.schemas import BinanceCandlestick
from nautilus_trader.adapters.binance.common.schemas import BinanceOrderBookData
from nautilus_trader.adapters.binance.common.schemas import BinanceQuoteData
from nautilus_trader.adapters.binance.common.schemas import BinanceTickerData
from nautilus_trader.adapters.binance.common.schemas import BinanceTrade
from nautilus_trader.adapters.binance.common.types import BinanceBar
from nautilus_trader.adapters.binance.common.types import BinanceTicker
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


def parse_trade_tick_http(
    instrument_id: InstrumentId,
    trade: BinanceTrade,
    ts_init: int,
) -> TradeTick:
    return TradeTick(
        instrument_id=instrument_id,
        price=Price.from_str(trade.price),
        size=Quantity.from_str(trade.qty),
        aggressor_side=AggressorSide.SELL if trade.isBuyerMaker else AggressorSide.BUY,
        trade_id=TradeId(str(trade.id)),
        ts_event=millis_to_nanos(trade.time),
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
        quote_volume=Decimal(values[7]),
        count=values[8],
        taker_buy_base_volume=Decimal(values[9]),
        taker_buy_quote_volume=Decimal(values[10]),
        ts_event=millis_to_nanos(values[0]),
        ts_init=ts_init,
    )


def parse_diff_depth_stream_ws(
    instrument_id: InstrumentId,
    data: BinanceOrderBookData,
    ts_init: int,
) -> OrderBookDeltas:
    ts_event: int = millis_to_nanos(data.T) if data.T is not None else millis_to_nanos(data.E)

    bid_deltas: List[OrderBookDelta] = [
        parse_book_delta_ws(instrument_id, OrderSide.BUY, d, ts_event, ts_init, data.u)
        for d in data.b
    ]
    ask_deltas: List[OrderBookDelta] = [
        parse_book_delta_ws(instrument_id, OrderSide.SELL, d, ts_event, ts_init, data.u)
        for d in data.a
    ]

    return OrderBookDeltas(
        instrument_id=instrument_id,
        book_type=BookType.L2_MBP,
        deltas=bid_deltas + ask_deltas,
        ts_event=ts_event,
        ts_init=ts_init,
        update_id=data.u,
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


def parse_quote_tick_ws(
    instrument_id: InstrumentId,
    data: BinanceQuoteData,
    ts_init: int,
) -> QuoteTick:
    return QuoteTick(
        instrument_id=instrument_id,
        bid=Price.from_str(data.b),
        ask=Price.from_str(data.a),
        bid_size=Quantity.from_str(data.B),
        ask_size=Quantity.from_str(data.A),
        ts_event=ts_init,
        ts_init=ts_init,
    )


def parse_ticker_24hr_ws(
    instrument_id: InstrumentId,
    data: BinanceTickerData,
    ts_init: int,
) -> BinanceTicker:
    return BinanceTicker(
        instrument_id=instrument_id,
        price_change=Decimal(data.p),
        price_change_percent=Decimal(data.P),
        weighted_avg_price=Decimal(data.w),
        prev_close_price=Decimal(data.x) if data.x is not None else None,
        last_price=Decimal(data.c),
        last_qty=Decimal(data.Q),
        bid_price=Decimal(data.b) if data.b is not None else None,
        bid_qty=Decimal(data.B) if data.B is not None else None,
        ask_price=Decimal(data.a) if data.a is not None else None,
        ask_qty=Decimal(data.A) if data.A is not None else None,
        open_price=Decimal(data.o),
        high_price=Decimal(data.h),
        low_price=Decimal(data.l),
        volume=Decimal(data.v),
        quote_volume=Decimal(data.q),
        open_time_ms=data.O,
        close_time_ms=data.C,
        first_id=data.F,
        last_id=data.L,
        count=data.n,
        ts_event=millis_to_nanos(data.E),
        ts_init=ts_init,
    )


def parse_bar_ws(
    instrument_id: InstrumentId,
    data: BinanceCandlestick,
    ts_init: int,
) -> BinanceBar:
    resolution = data.i[-1]
    if resolution == "m":
        aggregation = BarAggregation.MINUTE
    elif resolution == "h":
        aggregation = BarAggregation.HOUR
    elif resolution == "d":
        aggregation = BarAggregation.DAY
    elif resolution == "w":
        aggregation = BarAggregation.WEEK
    elif resolution == "M":
        aggregation = BarAggregation.MONTH
    else:  # pragma: no cover (design-time error)
        raise RuntimeError(f"unsupported time aggregation resolution, was {resolution}")

    bar_spec = BarSpecification(
        step=int(data.i[:-1]),
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
        open=Price.from_str(data.o),
        high=Price.from_str(data.h),
        low=Price.from_str(data.l),
        close=Price.from_str(data.c),
        volume=Quantity.from_str(data.v),
        quote_volume=Decimal(data.q),
        count=data.n,
        taker_buy_base_volume=Decimal(data.V),
        taker_buy_quote_volume=Decimal(data.Q),
        ts_event=millis_to_nanos(data.T),
        ts_init=ts_init,
    )
