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

from datetime import datetime
from decimal import Decimal
from typing import Any, Dict, List

from nautilus_trader.adapters.binance.core.constants import BINANCE_VENUE
from nautilus_trader.adapters.binance.core.types import BinanceBar
from nautilus_trader.adapters.binance.parsing.common import parse_balances_futures
from nautilus_trader.adapters.binance.parsing.common import parse_balances_spot
from nautilus_trader.adapters.binance.parsing.common import parse_margins
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.string import precision_from_str
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import CurrencyType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.instruments.crypto_future import CryptoFuture
from nautilus_trader.model.instruments.crypto_perpetual import CryptoPerpetual
from nautilus_trader.model.instruments.currency import CurrencySpot
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


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


def parse_account_balances_spot_http(raw_balances: List[Dict[str, str]]) -> List[AccountBalance]:
    return parse_balances_spot(raw_balances, "asset", "free", "locked")


def parse_account_balances_futures_http(raw_balances: List[Dict[str, str]]) -> List[AccountBalance]:
    return parse_balances_futures(
        raw_balances, "asset", "availableBalance", "initialMargin", "maintMargin"
    )


def parse_account_margins_http(raw_balances: List[Dict[str, str]]) -> List[MarginBalance]:
    return parse_margins(raw_balances, "asset", "initialMargin", "maintMargin")


def parse_spot_instrument_http(
    data: Dict[str, Any],
    fees: Dict[str, Any],
    ts_event: int,
    ts_init: int,
) -> Instrument:
    native_symbol = Symbol(data["symbol"])

    # Create base asset
    base_asset: str = data["baseAsset"]
    base_currency = Currency(
        code=base_asset,
        precision=data["baseAssetPrecision"],
        iso4217=0,  # Currently undetermined for crypto assets
        name=base_asset,
        currency_type=CurrencyType.CRYPTO,
    )

    # Create quote asset
    quote_asset: str = data["quoteAsset"]
    quote_currency = Currency(
        code=quote_asset,
        precision=data["quoteAssetPrecision"],
        iso4217=0,  # Currently undetermined for crypto assets
        name=quote_asset,
        currency_type=CurrencyType.CRYPTO,
    )

    # symbol = Symbol(base_currency.code + "/" + quote_currency.code)
    instrument_id = InstrumentId(symbol=native_symbol, venue=BINANCE_VENUE)

    # Parse instrument filters
    symbol_filters = {f["filterType"]: f for f in data["filters"]}
    price_filter = symbol_filters.get("PRICE_FILTER")
    lot_size_filter = symbol_filters.get("LOT_SIZE")
    min_notional_filter = symbol_filters.get("MIN_NOTIONAL")
    # market_lot_size_filter = symbol_filters.get("MARKET_LOT_SIZE")

    tick_size = price_filter["tickSize"].rstrip("0")
    step_size = lot_size_filter["stepSize"].rstrip("0")
    price_precision = precision_from_str(tick_size)
    size_precision = precision_from_str(step_size)
    price_increment = Price.from_str(tick_size)
    size_increment = Quantity.from_str(step_size)
    lot_size = Quantity.from_str(step_size)
    max_quantity = Quantity(float(lot_size_filter["maxQty"]), precision=size_precision)
    min_quantity = Quantity(float(lot_size_filter["minQty"]), precision=size_precision)
    min_notional = None
    if min_notional_filter is not None:
        min_notional = Money(min_notional_filter["minNotional"], currency=quote_currency)
    max_price = Price(float(price_filter["maxPrice"]), precision=price_precision)
    min_price = Price(float(price_filter["minPrice"]), precision=price_precision)

    # Parse fees
    pair_fees = fees.get(native_symbol.value)
    maker_fee: Decimal = Decimal(0)
    taker_fee: Decimal = Decimal(0)
    if pair_fees:
        maker_fee = Decimal(pair_fees["makerCommission"])
        taker_fee = Decimal(pair_fees["takerCommission"])

    # Create instrument
    return CurrencySpot(
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
        info=data,
    )


def parse_perpetual_instrument_http(
    data: Dict[str, Any],
    ts_event: int,
    ts_init: int,
) -> CryptoPerpetual:
    native_symbol = Symbol(data["symbol"])

    # Create base asset
    base_asset: str = data["baseAsset"]
    base_currency = Currency(
        code=base_asset,
        precision=data["baseAssetPrecision"],
        iso4217=0,  # Currently undetermined for crypto assets
        name=base_asset,
        currency_type=CurrencyType.CRYPTO,
    )

    # Create quote asset
    quote_asset: str = data["quoteAsset"]
    quote_currency = Currency(
        code=quote_asset,
        precision=data["quotePrecision"],
        iso4217=0,  # Currently undetermined for crypto assets
        name=quote_asset,
        currency_type=CurrencyType.CRYPTO,
    )

    symbol = Symbol(data["symbol"] + "-PERP")
    instrument_id = InstrumentId(symbol=symbol, venue=BINANCE_VENUE)

    # Parse instrument filters
    symbol_filters = {f["filterType"]: f for f in data["filters"]}
    price_filter = symbol_filters.get("PRICE_FILTER")
    lot_size_filter = symbol_filters.get("LOT_SIZE")
    min_notional_filter = symbol_filters.get("MIN_NOTIONAL")
    # market_lot_size_filter = symbol_filters.get("MARKET_LOT_SIZE")

    tick_size = price_filter["tickSize"].rstrip("0")
    step_size = lot_size_filter["stepSize"].rstrip("0")
    price_precision = precision_from_str(tick_size)
    size_precision = precision_from_str(step_size)
    price_increment = Price.from_str(tick_size)
    size_increment = Quantity.from_str(step_size)
    max_quantity = Quantity(float(lot_size_filter["maxQty"]), precision=size_precision)
    min_quantity = Quantity(float(lot_size_filter["minQty"]), precision=size_precision)
    min_notional = None
    if min_notional_filter is not None:
        min_notional = Money(min_notional_filter["notional"], currency=quote_currency)
    max_price = Price(float(price_filter["maxPrice"]), precision=price_precision)
    min_price = Price(float(price_filter["minPrice"]), precision=price_precision)

    # Futures commissions
    maker_fee = Decimal("0.0002")  # TODO
    taker_fee = Decimal("0.0004")  # TODO

    assert data["marginAsset"] == quote_asset

    # Create instrument
    return CryptoPerpetual(
        instrument_id=instrument_id,
        native_symbol=native_symbol,
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
        info=data,
    )


def parse_future_instrument_http(
    data: Dict[str, Any],
    ts_event: int,
    ts_init: int,
) -> CryptoFuture:
    native_symbol = Symbol(data["symbol"])

    # Create base asset
    base_asset: str = data["baseAsset"]
    base_currency = Currency(
        code=base_asset,
        precision=data["baseAssetPrecision"],
        iso4217=0,  # Currently undetermined for crypto assets
        name=base_asset,
        currency_type=CurrencyType.CRYPTO,
    )

    # Create quote asset
    quote_asset: str = data["quoteAsset"]
    quote_currency = Currency(
        code=quote_asset,
        precision=data["quotePrecision"],
        iso4217=0,  # Currently undetermined for crypto assets
        name=quote_asset,
        currency_type=CurrencyType.CRYPTO,
    )

    instrument_id = InstrumentId(symbol=native_symbol, venue=BINANCE_VENUE)

    # Parse instrument filters
    symbol_filters = {f["filterType"]: f for f in data["filters"]}
    price_filter = symbol_filters.get("PRICE_FILTER")
    lot_size_filter = symbol_filters.get("LOT_SIZE")
    min_notional_filter = symbol_filters.get("MIN_NOTIONAL")
    # market_lot_size_filter = symbol_filters.get("MARKET_LOT_SIZE")

    tick_size = price_filter["tickSize"].rstrip("0")
    step_size = lot_size_filter["stepSize"].rstrip("0")
    price_precision = data["pricePrecision"]
    size_precision = data["quantityPrecision"]
    price_increment = Price.from_str(tick_size)
    size_increment = Quantity.from_str(step_size)
    max_quantity = Quantity(float(lot_size_filter["maxQty"]), precision=size_precision)
    min_quantity = Quantity(float(lot_size_filter["minQty"]), precision=size_precision)
    min_notional = None
    if min_notional_filter is not None:
        min_notional = Money(min_notional_filter["notional"], currency=quote_currency)
    max_price = Price(float(price_filter["maxPrice"]), precision=price_precision)
    min_price = Price(float(price_filter["minPrice"]), precision=price_precision)

    # Futures commissions
    maker_fee = Decimal("0.0002")  # TODO
    taker_fee = Decimal("0.0004")  # TODO

    assert data["marginAsset"] == quote_asset

    # Create instrument
    return CryptoFuture(
        instrument_id=instrument_id,
        native_symbol=native_symbol,
        underlying=base_currency,
        quote_currency=quote_currency,
        settlement_currency=quote_currency,
        expiry_date=datetime.strptime(data["symbol"].partition("_")[2], "%y%m%d").date(),
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
        info=data,
    )
