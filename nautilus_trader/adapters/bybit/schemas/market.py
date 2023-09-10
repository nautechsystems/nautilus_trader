from decimal import Decimal

import msgspec

from nautilus_trader.adapters.bybit.schemas.common import LeverageFilter
from nautilus_trader.adapters.bybit.schemas.common import LotSizeFilter
from nautilus_trader.adapters.bybit.schemas.common import PriceFilter
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import CurrencyType


def BybitSpotListResult(type):
    return msgspec.defstruct("", [("list", list[type])])


class BybitLinearInstrumentStruct(msgspec.Struct):
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
    priceFilter: PriceFilter
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


class BybitInstrumentsResponseStruct(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitSpotListResult(BybitLinearInstrumentStruct)
    time: int


################################################################################
# Risk Limit
################################################################################
class BybitRiskLimitStruct(msgspec.Struct):
    id: int
    symbol: str
    riskLimitValue: str
    maintenanceMargin: str
    initialMargin: str
    isLowestRisk: int
    maxLeverage: str


class BybitRiskLimitResponseStruct(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitSpotListResult(BybitRiskLimitStruct)
    time: int
