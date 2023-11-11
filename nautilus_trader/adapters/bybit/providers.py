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

from nautilus_trader.adapters.bybit.common.constants import BYBIT_VENUE
from nautilus_trader.adapters.bybit.common.enums import BybitInstrumentType
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.http.market import BybitMarketHttpAPI
from nautilus_trader.adapters.bybit.schemas.market.instrument import BybitInstrumentLinear
from nautilus_trader.adapters.bybit.schemas.market.instrument import BybitInstrumentOption
from nautilus_trader.adapters.bybit.schemas.market.instrument import BybitInstrumentSpot
from nautilus_trader.adapters.bybit.schemas.symbol import BybitSymbol
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.objects import PRICE_MAX
from nautilus_trader.model.objects import PRICE_MIN
from nautilus_trader.model.objects import QUANTITY_MAX
from nautilus_trader.model.objects import QUANTITY_MIN
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class BybitInstrumentProvider(InstrumentProvider):
    def __init__(
        self,
        client: BybitHttpClient,
        logger: Logger,
        clock: LiveClock,
        instrument_types: list[BybitInstrumentType],
        is_testnet: bool = False,
        config: InstrumentProviderConfig | None = None,
    ):
        super().__init__(
            logger=logger,
            config=config,
        )
        self._clock = clock
        self._client = client
        self._instrument_types = instrument_types

        self._http_market = BybitMarketHttpAPI(
            client=client,
            clock=clock,
        )

        self._log_warnings = config.log_warnings if config else True
        self._decoder = msgspec.json.Decoder()
        self._encoder = msgspec.json.Encoder()

    async def load_all_async(self, filters: dict | None = None) -> None:
        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.info(f"Loading all instruments{filters_str}")

        instruments_info = await self._http_market.fetch_instruments(self._instrument_types)
        # risk_limits = await self._http_market.get_risk_limits()
        for instrument in instruments_info:
            if isinstance(instrument, BybitInstrumentSpot):
                self._parse_spot_instrument(instrument)
            elif isinstance(instrument, BybitInstrumentLinear):
                self._parse_linear_instrument(instrument)
            elif isinstance(instrument, BybitInstrumentOption):
                self._parse_option_instrument(instrument)
            else:
                raise TypeError("Unsupported instrument type in BybitInstrumentProvider")

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        if not instrument_ids:
            self._log.info("No instrument IDs given for loading.")
            return

    async def load_async(self, instrument_id: InstrumentId, filters: dict | None = None) -> None:
        PyCondition.not_none(instrument_id, "instrument_id")

    def _parse_spot_instrument(
        self,
        instrument: BybitInstrumentSpot,
    ):
        pass

    def _parse_option_instrument(
        self,
        instrument: BybitInstrumentOption,
    ):
        pass

    def _parse_linear_instrument(
        self,
        instrument: BybitInstrumentLinear,
    ):
        try:
            base_currency = instrument.parse_to_base_currency()
            quote_currency = instrument.parse_to_quote_currency()
            raw_symbol = Symbol(instrument.symbol)
            parsed_symbol = BybitSymbol(raw_symbol.value).parse_as_nautilus(
                BybitInstrumentType.LINEAR,
            )
            nautilus_symbol = Symbol(parsed_symbol)
            instrument_id = InstrumentId(symbol=nautilus_symbol, venue=BYBIT_VENUE)
            if instrument.settleCoin == instrument.baseCoin:
                settlement_currency = base_currency
            elif instrument.settleCoin == instrument.quoteCoin:
                settlement_currency = quote_currency
            else:
                raise ValueError(f"Unrecognized margin asset {instrument.settleCoin}")

            tick_size = instrument.priceFilter.tickSize.rstrip("0")
            step_size = instrument.lotSizeFilter.qtyStep.rstrip("0")
            price_precision = abs(int(Decimal(tick_size).as_tuple().exponent))
            size_precision = abs(int(Decimal(step_size).as_tuple().exponent))
            price_increment = Price.from_str(tick_size)
            size_increment = Quantity.from_str(step_size)
            PyCondition.in_range(float(tick_size), PRICE_MIN, PRICE_MAX, "tick_size")
            PyCondition.in_range(float(step_size), QUANTITY_MIN, QUANTITY_MAX, "step_size")
            max_quantity = Quantity(
                float(instrument.lotSizeFilter.maxOrderQty),
                precision=size_precision,
            )
            min_quantity = Quantity(
                float(instrument.lotSizeFilter.minOrderQty),
                precision=size_precision,
            )
            min_notional = None
            max_price = Price(float(instrument.priceFilter.maxPrice), precision=price_precision)
            min_price = Price(float(instrument.priceFilter.minPrice), precision=price_precision)
            maker_fee = instrument.get_maker_fee()
            taker_fee = instrument.get_taker_fee()
            ts_event = self._clock.timestamp_ns()
            ts_init = self._clock.timestamp_ns()
            instrument = CryptoPerpetual(
                instrument_id=instrument_id,
                raw_symbol=raw_symbol,
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
                maker_fee=maker_fee,
                taker_fee=taker_fee,
                ts_event=ts_event,
                ts_init=ts_init,
                info=self._decoder.decode(self._encoder.encode(instrument)),
            )
            self.add_currency(base_currency)
            self.add_currency(quote_currency)
            self.add(instrument=instrument)
        except ValueError as e:
            if self._log_warnings:
                self._log.warning(f"Unable to parse instrument {instrument.symbol}, {e}.")
