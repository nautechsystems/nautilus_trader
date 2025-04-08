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
from collections import defaultdict

import msgspec

from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import instruments_from_pyo3


class TardisInstrumentProvider(InstrumentProvider):
    """
    Provides Nautilus instrument definitions from Tardis.

    Parameters
    ----------
    client : TardisHttpClient
        The Tardis HTTP client.
    config : InstrumentProviderConfig, optional
        The instrument provider configuration, by default None.

    """

    def __init__(
        self,
        client: nautilus_pyo3.TardisHttpClient,
        config: InstrumentProviderConfig | None = None,
    ) -> None:
        super().__init__(config=config)
        self._client = client

        self._log_warnings = config.log_warnings if config else True
        self._decoder = msgspec.json.Decoder()
        self._encoder = msgspec.json.Encoder()

    async def load_all_async(self, filters: dict | None = None) -> None:
        if filters is None or not filters.get("venues"):
            raise ValueError(
                "`filters` with a 'venues' key must be provided to load all instruments for each exchange",
            )

        venues = filters["venues"]

        filters = filters or {}
        base_currency = filters.get("base_currency")
        quote_currency = filters.get("quote_currency")
        instrument_type = filters.get("instrument_type")
        contract_type = filters.get("contract_type")
        active = filters.get("active")
        start = filters.get("start")
        end = filters.get("end")
        effective = filters.get("effective")
        ts_init = time.time_ns()

        for venue in venues:
            venue = venue.upper().replace("-", "_")
            for exchange in nautilus_pyo3.tardis_exchange_from_venue_str(venue):
                self._log.info(f"Requesting instruments for {exchange=}")
                pyo3_instruments = await self._client.instruments(
                    exchange=exchange.lower(),
                    base_currency=list(base_currency) if base_currency else None,
                    quote_currency=list(quote_currency) if quote_currency else None,
                    instrument_type=list(instrument_type) if instrument_type else None,
                    contract_type=list(contract_type) if contract_type else None,
                    active=active,
                    start=start,
                    end=end,
                    effective=effective,
                    ts_init=ts_init,
                )
                instruments = instruments_from_pyo3(pyo3_instruments)
                for instrument in instruments:
                    self.add(instrument=instrument)

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        if not instrument_ids:
            self._log.warning("No instrument IDs given for loading")
            return

        filters = filters or {}
        venues = filters.get("venues", [])

        venue_instruments: defaultdict[str, set[str]] = defaultdict(set)
        for venue in venues:
            venue_instruments[venue] = set()

        for instrument_id in instrument_ids:
            venue = instrument_id.venue.value.upper().replace("-", "_")
            for exchange in nautilus_pyo3.tardis_exchange_from_venue_str(venue):
                venue_instruments[exchange].add(instrument_id.symbol.value)

        base_currency = filters.get("base_currency")
        quote_currency = filters.get("quote_currency")
        instrument_type = filters.get("instrument_type")
        contract_type = filters.get("contract_type")
        active = filters.get("active")
        start = filters.get("start")
        end = filters.get("end")
        effective = filters.get("effective")
        ts_init = time.time_ns()

        for exchange, symbols in venue_instruments.items():
            self._log.info(f"Requesting instruments for {exchange=}")
            pyo3_instruments = await self._client.instruments(
                exchange=exchange.lower(),
                base_currency=list(base_currency) if base_currency else None,
                quote_currency=list(quote_currency) if quote_currency else None,
                instrument_type=list(instrument_type) if instrument_type else None,
                contract_type=list(contract_type) if contract_type else None,
                active=active,
                start=start,
                end=end,
                effective=effective,
                ts_init=ts_init,
            )
            instruments = instruments_from_pyo3(pyo3_instruments)

            for instrument in instruments:
                symbol = instrument.id.symbol.value
                if symbols and symbol not in symbols:
                    continue  # Filter instrument ID
                self.add(instrument=instrument)

    async def load_async(self, instrument_id: InstrumentId, filters: dict | None = None) -> None:
        PyCondition.not_none(instrument_id, "instrument_id")
        await self.load_ids_async([instrument_id], filters)
