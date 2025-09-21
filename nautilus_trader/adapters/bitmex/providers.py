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

from typing import Any

from nautilus_trader.adapters.bitmex.constants import BITMEX_VENUE
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import instruments_from_pyo3


class BitmexInstrumentProvider(InstrumentProvider):
    """
    Provides Nautilus instrument definitions from BitMEX.

    Parameters
    ----------
    client : nautilus_pyo3.BitmexHttpClient
        The BitMEX HTTP client.
    active_only : bool, default True
        Whether to only load active instruments.
    config : InstrumentProviderConfig, optional
        The instrument provider configuration, by default None.

    """

    def __init__(
        self,
        client: nautilus_pyo3.BitmexHttpClient,
        active_only: bool = True,
        config: InstrumentProviderConfig | None = None,
    ) -> None:
        super().__init__(config=config)
        self._client = client
        self._active_only = active_only
        self._log_warnings = config.log_warnings if config else True

        self._instruments_pyo3: list[nautilus_pyo3.Instrument] = []

    @property
    def active_only(self) -> bool:
        """
        Return whether the provider is configured to load only active instruments.

        Returns
        -------
        bool

        """
        return self._active_only

    def instruments_pyo3(self) -> list[Any]:
        """
        Return all BitMEX PyO3 instrument definitions held by the provider.

        Returns
        -------
        list[nautilus_pyo3.Instrument]

        """
        return self._instruments_pyo3

    async def load_all_async(self, filters: dict | None = None) -> None:
        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.info(f"Loading all instruments{filters_str}")

        pyo3_instruments = await self._client.request_instruments(
            self._active_only,
        )

        self._instruments_pyo3 = pyo3_instruments
        instruments = instruments_from_pyo3(pyo3_instruments)
        for instrument in instruments:
            self.add(instrument=instrument)

        self._log.info(f"Loaded {len(self._instruments)} instruments")

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        if not instrument_ids:
            self._log.warning("No instrument IDs given for loading")
            return

        # Check all instrument IDs
        for instrument_id in instrument_ids:
            PyCondition.equal(instrument_id.venue, BITMEX_VENUE, "instrument_id.venue", "BITMEX")

        pyo3_instruments = await self._client.request_instruments(
            self._active_only,
        )

        self._instruments_pyo3 = pyo3_instruments
        instruments = instruments_from_pyo3(pyo3_instruments)

        for instrument in instruments:
            if instrument.id not in instrument_ids:
                continue  # Filter instrument ID
            self.add(instrument=instrument)

    async def load_async(self, instrument_id: InstrumentId, filters: dict | None = None) -> None:
        PyCondition.not_none(instrument_id, "instrument_id")
        await self.load_ids_async([instrument_id], filters)
