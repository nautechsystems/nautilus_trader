# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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


BybitInstrument = BybitInstrumentLinear | BybitInstrumentSpot | BybitInstrumentOption


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
