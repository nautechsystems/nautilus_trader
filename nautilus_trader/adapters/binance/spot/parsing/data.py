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
from typing import Dict, Optional

import msgspec

from nautilus_trader.adapters.binance.common.constants import BINANCE_VENUE
from nautilus_trader.adapters.binance.common.enums import BinanceSymbolFilterType
from nautilus_trader.adapters.binance.spot.schemas.market import BinanceSpotOrderBookDepthData
from nautilus_trader.adapters.binance.spot.schemas.market import BinanceSpotSymbolInfo
from nautilus_trader.adapters.binance.spot.schemas.market import BinanceSpotTradeData
from nautilus_trader.adapters.binance.spot.schemas.market import BinanceSymbolFilter
from nautilus_trader.adapters.binance.spot.schemas.wallet import BinanceSpotTradeFees
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.string import precision_from_str
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import CurrencyType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.instruments.currency_pair import CurrencyPair
from nautilus_trader.model.objects import PRICE_MAX
from nautilus_trader.model.objects import PRICE_MIN
from nautilus_trader.model.objects import QUANTITY_MAX
from nautilus_trader.model.objects import QUANTITY_MIN
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.data import OrderBookSnapshot


def parse_spot_instrument_http(
    symbol_info: BinanceSpotSymbolInfo,
    fees: Optional[BinanceSpotTradeFees],
    ts_event: int,
    ts_init: int,
) -> Instrument:
    # Create base asset
    base_currency = Currency(
        code=symbol_info.baseAsset,
        precision=symbol_info.baseAssetPrecision,
        iso4217=0,  # Currently undetermined for crypto assets
        name=symbol_info.baseAsset,
        currency_type=CurrencyType.CRYPTO,
    )

    # Create quote asset
    quote_currency = Currency(
        code=symbol_info.quoteAsset,
        precision=symbol_info.quoteAssetPrecision,
        iso4217=0,  # Currently undetermined for crypto assets
        name=symbol_info.quoteAsset,
        currency_type=CurrencyType.CRYPTO,
    )

    native_symbol = Symbol(symbol_info.symbol)
    instrument_id = InstrumentId(symbol=native_symbol, venue=BINANCE_VENUE)

    # Parse instrument filters
    filters: Dict[BinanceSymbolFilterType, BinanceSymbolFilter] = {
        f.filterType: f for f in symbol_info.filters
    }
    price_filter: BinanceSymbolFilter = filters.get(BinanceSymbolFilterType.PRICE_FILTER)
    lot_size_filter: BinanceSymbolFilter = filters.get(BinanceSymbolFilterType.LOT_SIZE)
    min_notional_filter: BinanceSymbolFilter = filters.get(BinanceSymbolFilterType.MIN_NOTIONAL)
    # market_lot_size_filter = symbol_filters.get("MARKET_LOT_SIZE")

    tick_size = price_filter.tickSize.rstrip("0")
    step_size = lot_size_filter.stepSize.rstrip("0")
    PyCondition.in_range(float(tick_size), PRICE_MIN, PRICE_MAX, "tick_size")
    PyCondition.in_range(float(step_size), QUANTITY_MIN, QUANTITY_MAX, "step_size")

    price_precision = precision_from_str(tick_size)
    size_precision = precision_from_str(step_size)
    price_increment = Price.from_str(tick_size)
    size_increment = Quantity.from_str(step_size)
    lot_size = Quantity.from_str(step_size)

    PyCondition.in_range(float(lot_size_filter.maxQty), QUANTITY_MIN, QUANTITY_MAX, "maxQty")
    PyCondition.in_range(float(lot_size_filter.minQty), QUANTITY_MIN, QUANTITY_MAX, "minQty")
    max_quantity = Quantity(float(lot_size_filter.maxQty), precision=size_precision)
    min_quantity = Quantity(float(lot_size_filter.minQty), precision=size_precision)
    min_notional = None
    if filters.get(BinanceSymbolFilterType.MIN_NOTIONAL):
        min_notional = Money(min_notional_filter.minNotional, currency=quote_currency)
    max_price = Price(min(float(price_filter.maxPrice), 4294967296.0), precision=price_precision)
    min_price = Price(max(float(price_filter.minPrice), 0.0), precision=price_precision)

    # Parse fees
    maker_fee: Decimal = Decimal(0)
    taker_fee: Decimal = Decimal(0)
    if fees:
        maker_fee = Decimal(fees.makerCommission)
        taker_fee = Decimal(fees.takerCommission)

    # Create instrument
    return CurrencyPair(
        instrument_id=instrument_id,
        native_symbol=native_symbol,
        base_currency=base_currency,
        quote_currency=quote_currency,
        price_precision=price_precision,
        size_precision=size_precision,
        price_increment=price_increment,
        size_increment=size_increment,
        lot_size=lot_size,
        max_quantity=max_quantity,
        min_quantity=min_quantity,
        max_notional=None,
        min_notional=min_notional,
        max_price=max_price,
        min_price=min_price,
        margin_init=Decimal(0),
        margin_maint=Decimal(0),
        maker_fee=maker_fee,
        taker_fee=taker_fee,
        ts_event=ts_event,
        ts_init=ts_init,
        info=msgspec.json.decode(msgspec.json.encode(symbol_info)),
    )


def parse_spot_book_snapshot(
    instrument_id: InstrumentId,
    data: BinanceSpotOrderBookDepthData,
    ts_init: int,
) -> OrderBookSnapshot:
    return OrderBookSnapshot(
        instrument_id=instrument_id,
        book_type=BookType.L2_MBP,
        bids=[[float(o[0]), float(o[1])] for o in data.bids],
        asks=[[float(o[0]), float(o[1])] for o in data.asks],
        ts_event=ts_init,
        ts_init=ts_init,
        update_id=data.lastUpdateId,
    )


def parse_spot_trade_tick_ws(
    instrument_id: InstrumentId,
    data: BinanceSpotTradeData,
    ts_init: int,
) -> TradeTick:
    return TradeTick(
        instrument_id=instrument_id,
        price=Price.from_str(data.p),
        size=Quantity.from_str(data.q),
        aggressor_side=AggressorSide.SELL if data.m else AggressorSide.BUY,
        trade_id=TradeId(str(data.t)),
        ts_event=millis_to_nanos(data.T),
        ts_init=ts_init,
    )
