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

from nautilus_trader.adapters.binance.common.constants import BINANCE_VENUE
from nautilus_trader.adapters.binance.common.enums import BinanceSymbolFilterType
from nautilus_trader.adapters.binance.futures.schemas.market import BinanceFuturesSymbolInfo
from nautilus_trader.adapters.binance.spot.schemas.market import BinanceSymbolFilter
from nautilus_trader.core.string import precision_from_str
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import CurrencyType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments.crypto_future import CryptoFuture
from nautilus_trader.model.instruments.crypto_perpetual import CryptoPerpetual
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


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

    symbol = Symbol(symbol_info.symbol + "-PERP")
    instrument_id = InstrumentId(symbol=symbol, venue=BINANCE_VENUE)

    # Parse instrument filters
    filters: Dict[BinanceSymbolFilterType, BinanceSymbolFilter] = {
        f.filterType: f for f in symbol_info.filters
    }
    price_filter: BinanceSymbolFilter = filters.get(BinanceSymbolFilterType.PRICE_FILTER)
    lot_size_filter: BinanceSymbolFilter = filters.get(BinanceSymbolFilterType.LOT_SIZE)
    min_notional_filter: BinanceSymbolFilter = filters.get(BinanceSymbolFilterType.MIN_NOTIONAL)

    tick_size = price_filter.tickSize.rstrip("0")
    step_size = lot_size_filter.stepSize.rstrip("0")
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
    maker_fee = Decimal("0.0002")  # TODO
    taker_fee = Decimal("0.0004")  # TODO

    assert symbol_info.marginAsset == symbol_info.quoteAsset

    # Create instrument
    return CryptoPerpetual(
        instrument_id=instrument_id,
        native_symbol=Symbol(symbol_info.symbol),
        base_currency=base_currency,
        quote_currency=quote_currency,
        settlement_currency=quote_currency,
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
        margin_init=Decimal(0),
        margin_maint=Decimal(0),
        maker_fee=maker_fee,
        taker_fee=taker_fee,
        ts_event=ts_event,
        ts_init=ts_init,
        # info={f: getattr(symbol_info, f) for f in symbol_info.__struct_fields__},
    )


def parse_future_instrument_http(
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
    instrument_id = InstrumentId(symbol=native_symbol, venue=BINANCE_VENUE)

    # Parse instrument filters
    filters: Dict[BinanceSymbolFilterType, BinanceSymbolFilter] = {
        f.filterType: f for f in symbol_info.filters
    }
    price_filter: BinanceSymbolFilter = filters.get(BinanceSymbolFilterType.PRICE_FILTER)
    lot_size_filter: BinanceSymbolFilter = filters.get(BinanceSymbolFilterType.LOT_SIZE)
    min_notional_filter: BinanceSymbolFilter = filters.get(BinanceSymbolFilterType.MIN_NOTIONAL)

    tick_size = price_filter.tickSize.rstrip("0")
    step_size = lot_size_filter.stepSize.rstrip("0")
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
    maker_fee = Decimal("0.0002")  # TODO
    taker_fee = Decimal("0.0004")  # TODO

    assert symbol_info.marginAsset == symbol_info.quoteAsset

    # Create instrument
    return CryptoFuture(
        instrument_id=instrument_id,
        native_symbol=native_symbol,
        underlying=base_currency,
        quote_currency=quote_currency,
        settlement_currency=quote_currency,
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
        margin_init=Decimal(0),
        margin_maint=Decimal(0),
        maker_fee=maker_fee,
        taker_fee=taker_fee,
        ts_event=ts_event,
        ts_init=ts_init,
        # info={f: getattr(symbol_info, f) for f in symbol_info.__struct_fields__},
    )
