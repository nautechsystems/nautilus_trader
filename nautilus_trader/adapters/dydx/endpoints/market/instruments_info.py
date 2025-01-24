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
"""
Define the instrument info endpoint.
"""

# ruff: noqa: N815

from decimal import Decimal

import msgspec

from nautilus_trader.adapters.dydx.common.constants import CURRENCY_MAP
from nautilus_trader.adapters.dydx.common.enums import DYDXEndpointType
from nautilus_trader.adapters.dydx.common.enums import DYDXPerpetualMarketStatus
from nautilus_trader.adapters.dydx.common.enums import DYDXPerpetualMarketType
from nautilus_trader.adapters.dydx.common.symbol import DYDXSymbol
from nautilus_trader.adapters.dydx.endpoints.endpoint import DYDXHttpEndpoint
from nautilus_trader.adapters.dydx.http.client import DYDXHttpClient
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.nautilus_pyo3 import HttpMethod
from nautilus_trader.model.enums import CurrencyType
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.objects import PRICE_MAX
from nautilus_trader.model.objects import PRICE_MIN
from nautilus_trader.model.objects import QUANTITY_MAX
from nautilus_trader.model.objects import QUANTITY_MIN
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class ListPerpetualMarketsGetParams(msgspec.Struct, omit_defaults=True):
    """
    Represent the dYdX list perpetual markets parameters.
    """

    limit: int | None = None
    ticker: str | None = None


class DYDXPerpetualMarketResponseObject(msgspec.Struct, forbid_unknown_fields=True):
    """
    Represent the dYdX perpetual market response object.
    """

    clobPairId: str
    ticker: str
    status: DYDXPerpetualMarketStatus
    priceChange24H: str
    volume24H: str
    trades24H: int
    nextFundingRate: str
    initialMarginFraction: str
    maintenanceMarginFraction: str
    openInterest: str
    atomicResolution: int
    quantumConversionExponent: int
    tickSize: str
    stepSize: str
    stepBaseQuantums: int
    subticksPerTick: int
    marketType: DYDXPerpetualMarketType
    baseOpenInterest: str
    oraclePrice: str | None = None
    openInterestLowerCap: str | None = None
    openInterestUpperCap: str | None = None
    defaultFundingRate1H: str | None = None

    def parse_base_currency(self) -> Currency:
        """
        Parse the base currency from the markets response.
        """
        code = self.ticker.split("-")[0]
        code = CURRENCY_MAP.get(code, code)
        return Currency(
            code=code,
            name=code,
            currency_type=CurrencyType.CRYPTO,
            precision=8,
            iso4217=0,  # Currently unspecified for crypto assets
        )

    def parse_quote_currency(self) -> Currency:
        """
        Parse the quote currency from the markets response.
        """
        code = self.ticker.split("-")[1]
        code = CURRENCY_MAP.get(code, code)
        return Currency(
            code=code,
            name=code,
            currency_type=CurrencyType.CRYPTO,
            precision=8,
            iso4217=0,  # Currently unspecified for crypto assets
        )

    def parse_to_instrument(
        self,
        base_currency: Currency,
        quote_currency: Currency,
        maker_fee: Decimal,
        taker_fee: Decimal,
        ts_event: int,
        ts_init: int,
    ) -> CryptoPerpetual:
        """
        Parse the instrument information.
        """
        tick_size = self.tickSize
        step_size = self.stepSize
        PyCondition.in_range(float(tick_size), PRICE_MIN, PRICE_MAX, "tick_size")
        PyCondition.in_range(float(step_size), QUANTITY_MIN, QUANTITY_MAX, "step_size")

        price_precision = abs(int(Decimal(tick_size).as_tuple().exponent))
        size_precision = abs(int(Decimal(step_size).as_tuple().exponent))
        price_increment = Price.from_str(tick_size)
        size_increment = Quantity.from_str(step_size)

        raw_symbol = Symbol(self.ticker)
        instrument_id = DYDXSymbol(raw_symbol.value).to_instrument_id()

        return CryptoPerpetual(
            instrument_id=instrument_id,
            raw_symbol=raw_symbol,
            base_currency=base_currency,
            quote_currency=quote_currency,
            settlement_currency=quote_currency,
            is_inverse=False,
            price_precision=price_precision,
            size_precision=size_precision,
            price_increment=price_increment,
            size_increment=size_increment,
            max_quantity=None,
            min_quantity=None,
            max_notional=None,
            min_notional=None,
            max_price=None,
            min_price=None,
            margin_init=Decimal(self.initialMarginFraction),
            margin_maint=Decimal(self.maintenanceMarginFraction),
            maker_fee=maker_fee,
            taker_fee=taker_fee,
            ts_event=ts_event,
            ts_init=ts_init,
            info=msgspec.json.Decoder().decode(msgspec.json.Encoder().encode(self)),
        )


class DYDXListPerpetualMarketsResponse(msgspec.Struct, forbid_unknown_fields=True):
    """
    Represent the dYdX list perpetual markets response object.
    """

    markets: dict[str, DYDXPerpetualMarketResponseObject]


class DYDXListPerpetualMarketsEndpoint(DYDXHttpEndpoint):
    """
    Define the instrument info endpoint.
    """

    def __init__(self, client: DYDXHttpClient) -> None:
        """
        Define the instrument info endpoint.
        """
        url_path = "/perpetualMarkets"
        super().__init__(
            client=client,
            url_path=url_path,
            endpoint_type=DYDXEndpointType.NONE,
            name="DYDXListPerpetualMarketsEndpoint",
        )
        self._response_decoder_list_perpetual_markets = msgspec.json.Decoder(
            DYDXListPerpetualMarketsResponse,
        )

    async def get(
        self,
        params: ListPerpetualMarketsGetParams,
    ) -> DYDXListPerpetualMarketsResponse | None:
        """
        Call the endpoint to list the instruments.
        """
        method_type = HttpMethod.GET
        raw = await self._method(method_type, params)

        if raw is not None:
            return self._response_decoder_list_perpetual_markets.decode(raw)

        return None
