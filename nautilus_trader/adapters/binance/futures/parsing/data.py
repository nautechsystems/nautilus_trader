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

from datetime import datetime as dt
from decimal import Decimal
from typing import Dict

import msgspec

from nautilus_trader.adapters.binance.common.constants import BINANCE_VENUE
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceSymbolFilterType
from nautilus_trader.adapters.binance.common.functions import parse_symbol
from nautilus_trader.adapters.binance.common.schemas import BinanceOrderBookData
from nautilus_trader.adapters.binance.futures.schemas.market import BinanceFuturesMarkPriceData
from nautilus_trader.adapters.binance.futures.schemas.market import BinanceFuturesSymbolInfo
from nautilus_trader.adapters.binance.futures.schemas.market import BinanceFuturesTradeData
from nautilus_trader.adapters.binance.futures.types import BinanceFuturesMarkPriceUpdate
from nautilus_trader.adapters.binance.spot.schemas.market import BinanceSymbolFilter
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
from nautilus_trader.model.instruments.crypto_future import CryptoFuture
from nautilus_trader.model.instruments.crypto_perpetual import CryptoPerpetual
from nautilus_trader.model.objects import PRICE_MAX
from nautilus_trader.model.objects import PRICE_MIN
from nautilus_trader.model.objects import QUANTITY_MAX
from nautilus_trader.model.objects import QUANTITY_MIN
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.data import OrderBookSnapshot


def parse_perpetual_instrument_http(
    symbol_info: BinanceFuturesSymbolInfo,
    ts_event: int,
    ts_init: int,
) -> CryptoPerpetual:
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
        precision=symbol_info.quotePrecision,
        iso4217=0,  # Currently undetermined for crypto assets
        name=symbol_info.quoteAsset,
        currency_type=CurrencyType.CRYPTO,
    )

    native_symbol = Symbol(symbol_info.symbol)
    symbol = parse_symbol(symbol_info.symbol, BinanceAccountType.FUTURES_USDT)
    instrument_id = InstrumentId(symbol=Symbol(symbol), venue=BINANCE_VENUE)

    # Parse instrument filters
    filters: Dict[BinanceSymbolFilterType, BinanceSymbolFilter] = {
        f.filterType: f for f in symbol_info.filters
    }
    price_filter: BinanceSymbolFilter = filters.get(BinanceSymbolFilterType.PRICE_FILTER)
    lot_size_filter: BinanceSymbolFilter = filters.get(BinanceSymbolFilterType.LOT_SIZE)
    min_notional_filter: BinanceSymbolFilter = filters.get(BinanceSymbolFilterType.MIN_NOTIONAL)

    tick_size = price_filter.tickSize.rstrip("0")
    step_size = lot_size_filter.stepSize.rstrip("0")
    PyCondition.in_range(float(tick_size), PRICE_MIN, PRICE_MAX, "tick_size")
    PyCondition.in_range(float(step_size), QUANTITY_MIN, QUANTITY_MAX, "step_size")

    price_precision = precision_from_str(tick_size)
    size_precision = precision_from_str(step_size)
    price_increment = Price.from_str(tick_size)
    size_increment = Quantity.from_str(step_size)
    max_quantity = Quantity(float(lot_size_filter.maxQty), precision=size_precision)
    min_quantity = Quantity(float(lot_size_filter.minQty), precision=size_precision)
    min_notional = None
    if filters.get(BinanceSymbolFilterType.MIN_NOTIONAL):
        min_notional = Money(min_notional_filter.minNotional, currency=quote_currency)
    max_price = Price(float(price_filter.maxPrice), precision=price_precision)
    min_price = Price(float(price_filter.minPrice), precision=price_precision)

    # Futures commissions
    maker_fee = Decimal("0.000200")  # TODO
    taker_fee = Decimal("0.000400")  # TODO

    if symbol_info.marginAsset == symbol_info.baseAsset:
        settlement_currency = base_currency
    elif symbol_info.marginAsset == symbol_info.quoteAsset:
        settlement_currency = quote_currency
    else:
        raise ValueError(f"Unrecognized margin asset {symbol_info.marginAsset}")

    # Create instrument
    return CryptoPerpetual(
        instrument_id=instrument_id,
        native_symbol=native_symbol,
        base_currency=base_currency,
        quote_currency=quote_currency,
        settlement_currency=settlement_currency,
        is_inverse=False,  # No inverse instruments trade on Binance
        price_precision=price_precision,
        size_precision=size_precision,
        price_increment=price_increment,
        size_increment=size_increment,
        max_quantity=max_quantity,
        min_quantity=min_quantity,
        max_notional=None,
        min_notional=min_notional,
        max_price=max_price,
        min_price=min_price,
        margin_init=Decimal(float(symbol_info.requiredMarginPercent) / 100),
        margin_maint=Decimal(float(symbol_info.maintMarginPercent) / 100),
        maker_fee=maker_fee,
        taker_fee=taker_fee,
        ts_event=ts_event,
        ts_init=ts_init,
        info=msgspec.json.decode(msgspec.json.encode(symbol_info)),
    )


def parse_futures_instrument_http(
    symbol_info: BinanceFuturesSymbolInfo,
    ts_event: int,
    ts_init: int,
) -> CryptoFuture:
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
        precision=symbol_info.quotePrecision,
        iso4217=0,  # Currently undetermined for crypto assets
        name=symbol_info.quoteAsset,
        currency_type=CurrencyType.CRYPTO,
    )

    native_symbol = Symbol(symbol_info.symbol)
    symbol = parse_symbol(symbol_info.symbol, BinanceAccountType.FUTURES_USDT)
    instrument_id = InstrumentId(symbol=Symbol(symbol), venue=BINANCE_VENUE)

    # Parse instrument filters
    filters: Dict[BinanceSymbolFilterType, BinanceSymbolFilter] = {
        f.filterType: f for f in symbol_info.filters
    }
    price_filter: BinanceSymbolFilter = filters.get(BinanceSymbolFilterType.PRICE_FILTER)
    lot_size_filter: BinanceSymbolFilter = filters.get(BinanceSymbolFilterType.LOT_SIZE)
    min_notional_filter: BinanceSymbolFilter = filters.get(BinanceSymbolFilterType.MIN_NOTIONAL)

    tick_size = price_filter.tickSize.rstrip("0")
    step_size = lot_size_filter.stepSize.rstrip("0")
    PyCondition.in_range(float(tick_size), PRICE_MIN, PRICE_MAX, "tick_size")
    PyCondition.in_range(float(step_size), QUANTITY_MIN, QUANTITY_MAX, "step_size")

    price_precision = precision_from_str(tick_size)
    size_precision = precision_from_str(step_size)
    price_increment = Price.from_str(tick_size)
    size_increment = Quantity.from_str(step_size)
    max_quantity = Quantity(float(lot_size_filter.maxQty), precision=size_precision)
    min_quantity = Quantity(float(lot_size_filter.minQty), precision=size_precision)
    min_notional = None
    if filters.get(BinanceSymbolFilterType.MIN_NOTIONAL):
        min_notional = Money(min_notional_filter.minNotional, currency=quote_currency)
    max_price = Price(float(price_filter.maxPrice), precision=price_precision)
    min_price = Price(float(price_filter.minPrice), precision=price_precision)

    # Futures commissions
    maker_fee = Decimal("0.000200")  # TODO
    taker_fee = Decimal("0.000400")  # TODO

    if symbol_info.marginAsset == symbol_info.baseAsset:
        settlement_currency = base_currency
    elif symbol_info.marginAsset == symbol_info.quoteAsset:
        settlement_currency = quote_currency
    else:
        raise ValueError(f"Unrecognized margin asset {symbol_info.marginAsset}")

    # Create instrument
    return CryptoFuture(
        instrument_id=instrument_id,
        native_symbol=native_symbol,
        underlying=base_currency,
        quote_currency=quote_currency,
        settlement_currency=settlement_currency,
        expiry_date=dt.strptime(symbol_info.symbol.partition("_")[2], "%y%m%d").date(),
        price_precision=price_precision,
        size_precision=size_precision,
        price_increment=price_increment,
        size_increment=size_increment,
        max_quantity=max_quantity,
        min_quantity=min_quantity,
        max_notional=None,
        min_notional=min_notional,
        max_price=max_price,
        min_price=min_price,
        margin_init=Decimal(float(symbol_info.requiredMarginPercent) / 100),
        margin_maint=Decimal(float(symbol_info.maintMarginPercent) / 100),
        maker_fee=maker_fee,
        taker_fee=taker_fee,
        ts_event=ts_event,
        ts_init=ts_init,
        info=msgspec.json.decode(msgspec.json.encode(symbol_info)),
    )


def parse_futures_book_snapshot(
    instrument_id: InstrumentId,
    data: BinanceOrderBookData,
    ts_init: int,
) -> OrderBookSnapshot:
    return OrderBookSnapshot(
        instrument_id=instrument_id,
        book_type=BookType.L2_MBP,
        bids=[[float(o[0]), float(o[1])] for o in data.b],
        asks=[[float(o[0]), float(o[1])] for o in data.a],
        ts_event=millis_to_nanos(data.T),
        ts_init=ts_init,
        update_id=data.u,
    )


def parse_futures_mark_price_ws(
    instrument_id: InstrumentId,
    data: BinanceFuturesMarkPriceData,
    ts_init: int,
) -> BinanceFuturesMarkPriceUpdate:
    return BinanceFuturesMarkPriceUpdate(
        instrument_id=instrument_id,
        mark=Price.from_str(data.p),
        index=Price.from_str(data.i),
        estimated_settle=Price.from_str(data.P),
        funding_rate=Decimal(data.r),
        ts_next_funding=millis_to_nanos(data.T),
        ts_event=millis_to_nanos(data.E),
        ts_init=ts_init,
    )


def parse_futures_trade_tick_ws(
    instrument_id: InstrumentId,
    data: BinanceFuturesTradeData,
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
