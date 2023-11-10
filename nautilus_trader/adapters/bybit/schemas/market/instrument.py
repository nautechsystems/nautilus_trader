from decimal import Decimal
from typing import Union

import msgspec

from nautilus_trader.adapters.bybit.schemas.common import BybitListResult
from nautilus_trader.adapters.bybit.schemas.common import LeverageFilter
from nautilus_trader.adapters.bybit.schemas.common import LinearPriceFilter
from nautilus_trader.adapters.bybit.schemas.common import LotSizeFilter
from nautilus_trader.adapters.bybit.schemas.common import SpotLotSizeFilter
from nautilus_trader.adapters.bybit.schemas.common import SpotPriceFilter
from nautilus_trader.core.rust.model import CurrencyType
from nautilus_trader.model.currency import Currency


class BybitInstrumentSpot(msgspec.Struct):
    symbol: str
    baseCoin: str
    quoteCoin: str
    innovation: str
    status: str
    marginTrading: str
    lotSizeFilter: SpotLotSizeFilter
    priceFilter: SpotPriceFilter


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

    def get_maker_fee(self) -> Decimal:
        return Decimal(0.0001)

    def get_taker_fee(self) -> Decimal:
        return Decimal(0.0006)


BybitInstrument = Union[BybitInstrumentLinear, BybitInstrumentSpot, BybitInstrumentOption]


class BybitInstrumentsLinearResponse(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitListResult(BybitInstrumentLinear)
    time: int


class BybitInstrumentsSpotResponse(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitListResult(BybitInstrumentSpot)
    time: int


class BybitInstrumentsOptionResponse(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitListResult(BybitInstrumentOption)
    time: int
