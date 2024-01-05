# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

import time
from decimal import Decimal

import msgspec
import pandas as pd

from nautilus_trader.adapters.bybit.schemas.account.fee_rate import BybitFeeRate
from nautilus_trader.adapters.bybit.schemas.common import BybitListResult
from nautilus_trader.adapters.bybit.schemas.common import LeverageFilter
from nautilus_trader.adapters.bybit.schemas.common import LinearPriceFilter
from nautilus_trader.adapters.bybit.schemas.common import LotSizeFilter
from nautilus_trader.adapters.bybit.schemas.common import SpotLotSizeFilter
from nautilus_trader.adapters.bybit.schemas.common import SpotPriceFilter
from nautilus_trader.adapters.bybit.schemas.symbol import BybitSymbol
from nautilus_trader.adapters.bybit.utils import tick_size_to_precision
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.rust.model import CurrencyType
from nautilus_trader.core.rust.model import OptionKind
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.model.instruments import OptionsContract
from nautilus_trader.model.objects import PRICE_MAX
from nautilus_trader.model.objects import PRICE_MIN
from nautilus_trader.model.objects import QUANTITY_MAX
from nautilus_trader.model.objects import QUANTITY_MIN
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class BybitInstrumentSpot(msgspec.Struct):
    symbol: str
    baseCoin: str
    quoteCoin: str
    innovation: str
    status: str
    marginTrading: str
    lotSizeFilter: SpotLotSizeFilter
    priceFilter: SpotPriceFilter

    def parse_to_instrument(
        self,
        fee_rate: BybitFeeRate,
        ts_event: int,
        ts_init: int,
    ) -> CurrencyPair:
        bybit_symbol = BybitSymbol(self.symbol + "-SPOT")
        tick_size = self.priceFilter.tickSize.rstrip("0")
        # TODO unclear about step size
        step_size = self.priceFilter.tickSize.rstrip("0")
        instrument_id = bybit_symbol.parse_as_nautilus()
        price_precision = tick_size_to_precision(Decimal(self.priceFilter.tickSize))
        price_increment = Price.from_str(tick_size)
        size_increment = Quantity.from_str(step_size)
        return CurrencyPair(
            instrument_id=instrument_id,
            raw_symbol=Symbol(bybit_symbol.raw_symbol),
            base_currency=self.parse_to_base_currency(),
            quote_currency=self.parse_to_quote_currency(),
            price_precision=price_precision,
            size_precision=size_increment,
            price_increment=price_increment,
            size_increment=size_increment,
            margin_init=Decimal(0.1),
            margin_maint=Decimal(0.1),
            maker_fee=Decimal(fee_rate.makerFeeRate),
            taker_fee=Decimal(fee_rate.takerFeeRate),
            ts_event=ts_event,
            ts_init=ts_init,
            lot_size=Quantity.from_str(self.lotSizeFilter.minOrderQty),
            max_quantity=Quantity.from_str(self.lotSizeFilter.maxOrderQty),
            min_quantity=Quantity.from_str(self.lotSizeFilter.minOrderQty),
            min_price=None,
            max_price=None,
            info=msgspec.json.Decoder().decode(msgspec.json.Encoder().encode(self)),
        )

    def parse_to_base_currency(self) -> Currency:
        return Currency(
            code=self.baseCoin,
            name=self.baseCoin,
            currency_type=CurrencyType.CRYPTO,
            precision=abs(int(Decimal(self.lotSizeFilter.basePrecision).as_tuple().exponent)),
            iso4217=0,  # Currently undetermined for crypto assets
        )

    def parse_to_quote_currency(self) -> Currency:
        return Currency(
            code=self.quoteCoin,
            name=self.quoteCoin,
            currency_type=CurrencyType.CRYPTO,
            precision=abs(int(Decimal(self.lotSizeFilter.quotePrecision).as_tuple().exponent)),
            iso4217=0,  # Currently undetermined for crypto assets
        )


def get_strike_price_from_symbol(symbol: str) -> int:
    ## symbols are in the format of ETH-3JAN23-1250-P
    ## where the strike price is 1250
    return int(symbol.split("-")[2])


class BybitInstrumentOption(msgspec.Struct):
    symbol: str
    status: str
    baseCoin: str
    quoteCoin: str
    settleCoin: str
    optionsType: str
    launchTime: str
    deliveryTime: str
    deliveryFeeRate: str
    priceFilter: LinearPriceFilter
    lotSizeFilter: LotSizeFilter

    def parse_to_instrument(
        self,
    ) -> OptionsContract:
        bybit_symbol = BybitSymbol(self.symbol + "-OPTION")
        instrument_id = bybit_symbol.parse_as_nautilus()
        price_precision = tick_size_to_precision(Decimal(self.priceFilter.tickSize))
        price_increment = Price(float(self.priceFilter.minPrice), price_precision)
        if self.optionsType == "Call":
            option_kind = OptionKind.CALL
        elif self.optionsType == "Put":
            option_kind = OptionKind.PUT
        else:
            raise ValueError(f"Unknown Bybit option type {self.optionsType}")
        timestamp = time.time_ns()
        strike_price = get_strike_price_from_symbol(self.symbol)
        activation_ns = pd.Timedelta(milliseconds=int(self.launchTime)).total_seconds() * 1e9
        expiration_ns = pd.Timedelta(milliseconds=int(self.deliveryTime)).total_seconds() * 1e9
        return OptionsContract(
            instrument_id=instrument_id,
            raw_symbol=Symbol(bybit_symbol.raw_symbol),
            asset_class=AssetClass.CRYPTOCURRENCY,
            currency=self.parse_to_quote_currency(),
            price_precision=price_precision,
            price_increment=price_increment,
            multiplier=Quantity.from_str("1.0"),
            lot_size=Quantity.from_str(self.lotSizeFilter.qtyStep),
            underlying=self.baseCoin,
            kind=option_kind,
            activation_ns=activation_ns,
            expiration_ns=expiration_ns,
            strike_price=Price.from_int(strike_price),
            ts_init=timestamp,
            ts_event=timestamp,
        )

    def parse_to_quote_currency(self) -> Currency:
        return Currency(
            code=self.quoteCoin,
            name=self.quoteCoin,
            currency_type=CurrencyType.CRYPTO,
            precision=1,
            iso4217=0,  # Currently undetermined for crypto assets
        )


class BybitInstrumentLinear(msgspec.Struct):
    symbol: str
    contractType: str
    status: str
    baseCoin: str
    quoteCoin: str
    launchTime: str
    deliveryTime: str
    deliveryFeeRate: str
    priceScale: str
    leverageFilter: LeverageFilter
    priceFilter: LinearPriceFilter
    lotSizeFilter: LotSizeFilter
    unifiedMarginTrade: bool
    fundingInterval: int
    settleCoin: str

    def parse_to_instrument(
        self,
        fee_rate: BybitFeeRate,
        ts_event: int,
        ts_init: int,
    ) -> CryptoPerpetual:
        base_currency = self.parse_to_base_currency()
        quote_currency = self.parse_to_quote_currency()
        bybit_symbol = BybitSymbol(self.symbol + "-LINEAR")
        instrument_id = bybit_symbol.parse_as_nautilus()
        if self.settleCoin == self.baseCoin:
            settlement_currency = base_currency
        elif self.settleCoin == self.quoteCoin:
            settlement_currency = quote_currency
        else:
            raise ValueError(f"Unrecognized margin asset {self.settleCoin}")

        tick_size = self.priceFilter.tickSize.rstrip("0")
        step_size = self.lotSizeFilter.qtyStep.rstrip("0")
        price_precision = abs(int(Decimal(tick_size).as_tuple().exponent))
        size_precision = abs(int(Decimal(step_size).as_tuple().exponent))
        price_increment = Price.from_str(tick_size)
        size_increment = Quantity.from_str(step_size)
        PyCondition.in_range(float(tick_size), PRICE_MIN, PRICE_MAX, "tick_size")
        PyCondition.in_range(float(step_size), QUANTITY_MIN, QUANTITY_MAX, "step_size")
        max_quantity = Quantity(
            float(self.lotSizeFilter.maxOrderQty),
            precision=size_precision,
        )
        min_quantity = Quantity(
            float(self.lotSizeFilter.minOrderQty),
            precision=size_precision,
        )
        min_notional = None
        max_price = Price(float(self.priceFilter.maxPrice), precision=price_precision)
        min_price = Price(float(self.priceFilter.minPrice), precision=price_precision)
        maker_fee = fee_rate.makerFeeRate
        taker_fee = fee_rate.takerFeeRate
        instrument = CryptoPerpetual(
            instrument_id=instrument_id,
            raw_symbol=Symbol(str(bybit_symbol)),
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
            margin_init=Decimal(0.1),
            margin_maint=Decimal(0.1),
            maker_fee=Decimal(maker_fee),
            taker_fee=Decimal(taker_fee),
            ts_event=ts_event,
            ts_init=ts_init,
            info=msgspec.json.Decoder().decode(msgspec.json.Encoder().encode(self)),
        )
        return instrument

    def parse_to_base_currency(self) -> Currency:
        return Currency(
            code=self.baseCoin,
            name=self.baseCoin,
            currency_type=CurrencyType.CRYPTO,
            precision=int(self.priceScale),
            iso4217=0,  # Currently undetermined for crypto assets
        )

    def parse_to_quote_currency(self) -> Currency:
        return Currency(
            code=self.quoteCoin,
            name=self.quoteCoin,
            currency_type=CurrencyType.CRYPTO,
            precision=int(self.priceScale),
            iso4217=0,  # Currently undetermined for crypto assets
        )


BybitInstrument = BybitInstrumentLinear | BybitInstrumentSpot | BybitInstrumentOption

BybitInstrumentList = (
    list[BybitInstrumentLinear] | list[BybitInstrumentSpot] | list[BybitInstrumentOption]
)


class BybitInstrumentsLinearResponse(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitListResult[BybitInstrumentLinear]
    time: int


class BybitInstrumentsSpotResponse(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitListResult[BybitInstrumentSpot]
    time: int


class BybitInstrumentsOptionResponse(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitListResult[BybitInstrumentOption]
    time: int
