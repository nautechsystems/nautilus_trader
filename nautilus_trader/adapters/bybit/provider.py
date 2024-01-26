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

import msgspec

from nautilus_trader.adapters.bybit.common.constants import BYBIT_VENUE
from nautilus_trader.adapters.bybit.common.enums import BybitInstrumentType
from nautilus_trader.adapters.bybit.http.account import BybitAccountHttpAPI
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.http.market import BybitMarketHttpAPI
from nautilus_trader.adapters.bybit.schemas.account.fee_rate import BybitFeeRate
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrument
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrumentLinear
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrumentList
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrumentOption
from nautilus_trader.adapters.bybit.schemas.instrument import BybitInstrumentSpot
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.identifiers import InstrumentId


class BybitInstrumentProvider(InstrumentProvider):
    """
    Provides a way to load instruments from Bybit.

    Parameters
    ----------
    client : BybitHttpClient
        The Bybit HTTP client.
    clock : LiveClock
        The clock instance.
    instrument_types : list[BybitInstrumentType]
        The instrument types to load.
    config : InstrumentProviderConfig, optional
        The instrument provider configuration, by default None.

    """

    def __init__(
        self,
        client: BybitHttpClient,
        clock: LiveClock,
        instrument_types: list[BybitInstrumentType],
        config: InstrumentProviderConfig | None = None,
    ) -> None:
        super().__init__(config=config)
        self._clock = clock
        self._client = client
        self._instrument_types = instrument_types

        self._http_market = BybitMarketHttpAPI(
            client=client,
            clock=clock,
        )
        self._http_account = BybitAccountHttpAPI(
            client=client,
            clock=clock,
        )

        self._log_warnings = config.log_warnings if config else True
        self._decoder = msgspec.json.Decoder()
        self._encoder = msgspec.json.Encoder()

    async def load_all_async(self, filters: dict | None = None) -> None:
        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.info(f"Loading all instruments{filters_str}")

        instrument_infos: dict[BybitInstrumentType, BybitInstrumentList] = {}
        fee_rates_infos: dict[BybitInstrumentType, list[BybitFeeRate]] = {}

        for instrument_type in self._instrument_types:
            instrument_infos[instrument_type] = await self._http_market.fetch_instruments(
                instrument_type,
            )
            fee_rates_infos[instrument_type] = await self._http_account.fetch_fee_rate(
                instrument_type,
            )

        # risk_limits = await self._http_market.get_risk_limits()
        for instrument_type in instrument_infos:
            for instrument in instrument_infos[instrument_type]:
                ## find target fee rate in list by symbol
                target_fee_rate = next(
                    (
                        item
                        for item in fee_rates_infos[instrument_type]
                        if item.symbol == instrument.symbol
                    ),
                    None,
                )
                if target_fee_rate:
                    self._parse_instrument(instrument, target_fee_rate)
                else:
                    self._log.warning(
                        f"Unable to find fee rate for instrument {instrument}.",
                    )
        self._log.info(f"Loaded {len(self._instruments)} instruments.")

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        if not instrument_ids:
            self._log.info("No instrument IDs given for loading.")
            return

        # Check all instrument IDs
        for instrument_id in instrument_ids:
            PyCondition.equal(instrument_id.venue, BYBIT_VENUE, "instrument_id.venue", "BYBIT")

        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.info(f"Loading instruments {instrument_ids}{filters_str}.")

        # extract symbol strings and instrument types
        # for instrument_id in instrument_ids:
        #     bybit_symbol = BybitSymbol(instrument_id.symbol.value)
        #     instrument = await self._http_market.fetch_instrument(
        #         bybit_symbol.instrument_type,
        #         bybit_symbol.raw_symbol,
        #     )
        #     self._parse_instrument(instrument)

    def _parse_instrument(
        self,
        instrument: BybitInstrument,
        fee_rate: BybitFeeRate,
    ) -> None:
        if isinstance(instrument, BybitInstrumentSpot):
            self._parse_spot_instrument(instrument, fee_rate)
        elif isinstance(instrument, BybitInstrumentLinear):
            self._parse_linear_instrument(instrument, fee_rate)
        elif isinstance(instrument, BybitInstrumentOption):
            self._parse_option_instrument(instrument)
        else:
            raise TypeError("Unsupported instrument type in BybitInstrumentProvider")

    async def load_async(self, instrument_id: InstrumentId, filters: dict | None = None) -> None:
        PyCondition.not_none(instrument_id, "instrument_id")

    def _parse_spot_instrument(
        self,
        data: BybitInstrumentSpot,
        fee_rate: BybitFeeRate,
    ) -> None:
        try:
            base_currency = data.parse_to_base_currency()
            quote_currency = data.parse_to_quote_currency()
            ts_event = self._clock.timestamp_ns()
            ts_init = self._clock.timestamp_ns()
            instrument = data.parse_to_instrument(
                fee_rate=fee_rate,
                ts_event=ts_event,
                ts_init=ts_init,
            )
            self.add_currency(base_currency)
            self.add_currency(quote_currency)
            self.add(instrument=instrument)
        except ValueError as e:
            if self._log_warnings:
                self._log.warning(f"Unable to parse option instrument {data.symbol}, {e}.")

    def _parse_option_instrument(
        self,
        instrument: BybitInstrumentOption,
    ) -> None:
        try:
            pass
        except ValueError as e:
            if self._log_warnings:
                self._log.warning(f"Unable to parse option instrument {instrument.symbol}, {e}.")

    def _parse_linear_instrument(
        self,
        data: BybitInstrumentLinear,
        fee_rate: BybitFeeRate,
    ) -> None:
        try:
            base_currency = data.parse_to_base_currency()
            quote_currency = data.parse_to_quote_currency()
            ts_event = self._clock.timestamp_ns()
            ts_init = self._clock.timestamp_ns()
            instrument = data.parse_to_instrument(
                fee_rate=fee_rate,
                ts_event=ts_event,
                ts_init=ts_init,
            )
            self.add_currency(base_currency)
            self.add_currency(quote_currency)
            self.add(instrument=instrument)
        except ValueError as e:
            if self._log_warnings:
                self._log.warning(f"Unable to parse instrument {data.symbol}, {e}.")
