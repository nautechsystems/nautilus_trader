# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.bybit.common.enums import BybitContractType
from nautilus_trader.adapters.bybit.common.enums import BybitOptionType
from nautilus_trader.adapters.bybit.common.symbol import BybitSymbol
from nautilus_trader.adapters.bybit.schemas.account.fee_rate import BybitFeeRate
from nautilus_trader.adapters.bybit.schemas.common import BybitListResultWithCursor
from nautilus_trader.adapters.bybit.schemas.common import LeverageFilter
from nautilus_trader.adapters.bybit.schemas.common import LinearLotSizeFilter
from nautilus_trader.adapters.bybit.schemas.common import LinearPriceFilter
from nautilus_trader.adapters.bybit.schemas.common import OptionLotSizeFilter
from nautilus_trader.adapters.bybit.schemas.common import SpotLotSizeFilter
from nautilus_trader.adapters.bybit.schemas.common import SpotPriceFilter
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.rust.model import OptionKind
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments import CryptoFuture
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.model.instruments import OptionContract
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
        base_currency: Currency,
        quote_currency: Currency,
        fee_rate: BybitFeeRate,
        ts_event: int,
        ts_init: int,
    ) -> CurrencyPair:
        assert base_currency.code == self.baseCoin
        assert quote_currency.code == self.quoteCoin
        bybit_symbol = BybitSymbol(self.symbol + "-SPOT")
        instrument_id = bybit_symbol.to_instrument_id()
        price_increment = Price.from_str(self.priceFilter.tickSize)
        size_increment = Quantity.from_str(self.lotSizeFilter.basePrecision)
        lot_size = Quantity.from_str(self.lotSizeFilter.basePrecision)
        max_quantity = Quantity.from_str(self.lotSizeFilter.maxOrderQty)
        min_quantity = Quantity.from_str(self.lotSizeFilter.minOrderQty)

        return CurrencyPair(
            instrument_id=instrument_id,
            raw_symbol=Symbol(bybit_symbol.raw_symbol),
            base_currency=base_currency,
            quote_currency=quote_currency,
            price_precision=price_increment.precision,
            size_precision=size_increment.precision,
            price_increment=price_increment,
            size_increment=size_increment,
            margin_init=Decimal("0.1"),
            margin_maint=Decimal("0.1"),
            maker_fee=Decimal(fee_rate.makerFeeRate),
            taker_fee=Decimal(fee_rate.takerFeeRate),
            ts_event=ts_event,
            ts_init=ts_init,
            lot_size=lot_size,
            max_quantity=max_quantity,
            min_quantity=min_quantity,
            min_price=None,
            max_price=None,
            info=msgspec.json.Decoder().decode(msgspec.json.Encoder().encode(self)),
        )


def get_strike_price_from_symbol(symbol: str) -> int:
    ## symbols are in the format of ETH-3JAN23-1250-P
    ## where the strike price is 1250
    return int(symbol.split("-")[2])


class BybitInstrumentLinear(msgspec.Struct):
    symbol: str
    contractType: BybitContractType
    status: str
    baseCoin: str
    quoteCoin: str
    launchTime: str
    deliveryTime: str
    deliveryFeeRate: str
    priceScale: str
    leverageFilter: LeverageFilter
    priceFilter: LinearPriceFilter
    lotSizeFilter: LinearLotSizeFilter
    unifiedMarginTrade: bool
    fundingInterval: int
    settleCoin: str

    def parse_to_instrument(
        self,
        base_currency: Currency,
        quote_currency: Currency,
        fee_rate: BybitFeeRate,
        ts_event: int,
        ts_init: int,
    ) -> CryptoPerpetual:
        assert base_currency.code == self.baseCoin
        assert quote_currency.code == self.quoteCoin
        bybit_symbol = BybitSymbol(self.symbol + "-LINEAR")
        instrument_id = bybit_symbol.to_instrument_id()
        if self.settleCoin == self.baseCoin:
            settlement_currency = base_currency
        elif self.settleCoin == self.quoteCoin:
            settlement_currency = quote_currency
        else:
            raise ValueError(f"Unrecognized margin asset {self.settleCoin}")

        price_increment = Price.from_str(self.priceFilter.tickSize)
        size_increment = Quantity.from_str(self.lotSizeFilter.qtyStep)
        max_quantity = Quantity.from_str(self.lotSizeFilter.maxOrderQty)
        min_quantity = Quantity.from_str(self.lotSizeFilter.minOrderQty)
        max_price = Price.from_str(self.priceFilter.maxPrice)
        min_price = Price.from_str(self.priceFilter.minPrice)
        maker_fee = fee_rate.makerFeeRate
        taker_fee = fee_rate.takerFeeRate

        if self.contractType == BybitContractType.LINEAR_PERPETUAL:
            instrument = CryptoPerpetual(
                instrument_id=instrument_id,
                raw_symbol=Symbol(bybit_symbol.raw_symbol),
                base_currency=base_currency,
                quote_currency=quote_currency,
                settlement_currency=settlement_currency,
                is_inverse=False,
                price_precision=price_increment.precision,
                size_precision=size_increment.precision,
                price_increment=price_increment,
                size_increment=size_increment,
                max_quantity=max_quantity,
                min_quantity=min_quantity,
                max_notional=None,
                min_notional=None,
                max_price=max_price,
                min_price=min_price,
                margin_init=Decimal("0.1"),
                margin_maint=Decimal("0.1"),
                maker_fee=Decimal(maker_fee),
                taker_fee=Decimal(taker_fee),
                ts_event=ts_event,
                ts_init=ts_init,
                info=msgspec.json.Decoder().decode(msgspec.json.Encoder().encode(self)),
            )
        elif self.contractType == BybitContractType.LINEAR_FUTURE:
            instrument = CryptoFuture(
                instrument_id=instrument_id,
                raw_symbol=Symbol(bybit_symbol.raw_symbol),
                underlying=base_currency,
                quote_currency=quote_currency,
                settlement_currency=settlement_currency,
                activation_ns=millis_to_nanos(int(self.launchTime)),
                expiration_ns=millis_to_nanos(int(self.deliveryTime)),
                is_inverse=False,
                price_precision=price_increment.precision,
                size_precision=size_increment.precision,
                price_increment=price_increment,
                size_increment=size_increment,
                max_quantity=max_quantity,
                min_quantity=min_quantity,
                max_notional=None,
                min_notional=None,
                max_price=max_price,
                min_price=min_price,
                margin_init=Decimal("0.1"),
                margin_maint=Decimal("0.1"),
                maker_fee=Decimal(maker_fee),
                taker_fee=Decimal(taker_fee),
                ts_event=ts_event,
                ts_init=ts_init,
                info=msgspec.json.Decoder().decode(msgspec.json.Encoder().encode(self)),
            )
        else:
            raise ValueError(f"Unrecognized linear contract type '{self.contractType}'")

        return instrument


class BybitInstrumentInverse(msgspec.Struct):
    symbol: str
    contractType: BybitContractType
    status: str
    baseCoin: str
    quoteCoin: str
    launchTime: str
    deliveryTime: str
    deliveryFeeRate: str
    priceScale: str
    leverageFilter: LeverageFilter
    priceFilter: LinearPriceFilter
    lotSizeFilter: LinearLotSizeFilter
    unifiedMarginTrade: bool
    fundingInterval: int
    settleCoin: str

    def parse_to_instrument(
        self,
        base_currency: Currency,
        quote_currency: Currency,
        fee_rate: BybitFeeRate,
        ts_event: int,
        ts_init: int,
    ) -> CryptoPerpetual:
        assert base_currency.code == self.baseCoin
        assert quote_currency.code == self.quoteCoin
        bybit_symbol = BybitSymbol(self.symbol + "-INVERSE")
        instrument_id = bybit_symbol.to_instrument_id()
        if self.settleCoin == self.baseCoin:
            settlement_currency = base_currency
        elif self.settleCoin == self.quoteCoin:
            settlement_currency = quote_currency
        else:
            raise ValueError(f"Unrecognized margin asset {self.settleCoin}")

        price_increment = Price.from_str(self.priceFilter.tickSize)
        size_increment = Quantity.from_str(self.lotSizeFilter.qtyStep)
        max_quantity = Quantity.from_str(self.lotSizeFilter.maxOrderQty)
        min_quantity = Quantity.from_str(self.lotSizeFilter.minOrderQty)
        max_price = Price.from_str(self.priceFilter.maxPrice)
        min_price = Price.from_str(self.priceFilter.minPrice)
        maker_fee = fee_rate.makerFeeRate
        taker_fee = fee_rate.takerFeeRate

        if self.contractType == BybitContractType.INVERSE_PERPETUAL:
            instrument = CryptoPerpetual(
                instrument_id=instrument_id,
                raw_symbol=Symbol(bybit_symbol.raw_symbol),
                base_currency=base_currency,
                quote_currency=quote_currency,
                settlement_currency=settlement_currency,
                is_inverse=True,
                price_precision=price_increment.precision,
                size_precision=size_increment.precision,
                price_increment=price_increment,
                size_increment=size_increment,
                max_quantity=max_quantity,
                min_quantity=min_quantity,
                max_notional=None,
                min_notional=None,
                max_price=max_price,
                min_price=min_price,
                margin_init=Decimal("0.1"),
                margin_maint=Decimal("0.1"),
                maker_fee=Decimal(maker_fee),
                taker_fee=Decimal(taker_fee),
                ts_event=ts_event,
                ts_init=ts_init,
                info=msgspec.json.Decoder().decode(msgspec.json.Encoder().encode(self)),
            )
        elif self.contractType == BybitContractType.INVERSE_FUTURE:
            instrument = CryptoFuture(
                instrument_id=instrument_id,
                raw_symbol=Symbol(bybit_symbol.raw_symbol),
                underlying=base_currency,
                quote_currency=quote_currency,
                settlement_currency=settlement_currency,
                activation_ns=millis_to_nanos(int(self.launchTime)),
                expiration_ns=millis_to_nanos(int(self.deliveryTime)),
                is_inverse=True,
                price_precision=price_increment.precision,
                size_precision=size_increment.precision,
                price_increment=price_increment,
                size_increment=size_increment,
                max_quantity=max_quantity,
                min_quantity=min_quantity,
                max_notional=None,
                min_notional=None,
                max_price=max_price,
                min_price=min_price,
                margin_init=Decimal("0.1"),
                margin_maint=Decimal("0.1"),
                maker_fee=Decimal(maker_fee),
                taker_fee=Decimal(taker_fee),
                ts_event=ts_event,
                ts_init=ts_init,
                info=msgspec.json.Decoder().decode(msgspec.json.Encoder().encode(self)),
            )
        else:
            raise ValueError(f"Unrecognized inverse contract type '{self.contractType}'")
        return instrument


class BybitInstrumentOption(msgspec.Struct):
    symbol: str
    status: str
    baseCoin: str
    quoteCoin: str
    settleCoin: str
    optionsType: BybitOptionType
    launchTime: str
    deliveryTime: str
    deliveryFeeRate: str
    priceFilter: LinearPriceFilter
    lotSizeFilter: OptionLotSizeFilter

    def parse_to_instrument(
        self,
        quote_currency: Currency,
    ) -> OptionContract:
        assert quote_currency.code == self.quoteCoin
        bybit_symbol = BybitSymbol(self.symbol + "-OPTION")
        instrument_id = bybit_symbol.to_instrument_id()
        price_increment = Price.from_str(self.priceFilter.tickSize)
        if self.optionsType == BybitOptionType.CALL:
            option_kind = OptionKind.CALL
        elif self.optionsType == BybitOptionType.PUT:
            option_kind = OptionKind.PUT
        else:
            raise ValueError(f"Unknown Bybit option type {self.optionsType}")

        timestamp = time.time_ns()
        strike_price = get_strike_price_from_symbol(self.symbol)
        activation_ns = pd.Timedelta(milliseconds=int(self.launchTime)).total_seconds() * 1e9
        expiration_ns = pd.Timedelta(milliseconds=int(self.deliveryTime)).total_seconds() * 1e9

        return OptionContract(
            instrument_id=instrument_id,
            raw_symbol=Symbol(bybit_symbol.raw_symbol),
            asset_class=AssetClass.CRYPTOCURRENCY,
            currency=quote_currency,
            price_precision=price_increment.precision,
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


BybitInstrument = (
    BybitInstrumentSpot | BybitInstrumentLinear | BybitInstrumentInverse | BybitInstrumentOption
)

BybitInstrumentList = (
    list[BybitInstrumentSpot]
    | list[BybitInstrumentLinear]
    | list[BybitInstrumentInverse]
    | list[BybitInstrumentOption]
)


class BybitInstrumentsSpotResponse(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitListResultWithCursor[BybitInstrumentSpot]
    time: int


class BybitInstrumentsLinearResponse(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitListResultWithCursor[BybitInstrumentLinear]
    time: int


class BybitInstrumentsInverseResponse(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitListResultWithCursor[BybitInstrumentInverse]
    time: int


class BybitInstrumentsOptionResponse(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitListResultWithCursor[BybitInstrumentOption]
    time: int
