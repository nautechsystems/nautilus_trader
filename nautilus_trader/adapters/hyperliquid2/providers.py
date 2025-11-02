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

from nautilus_trader.adapters.hyperliquid2.constants import HYPERLIQUID_VENUE
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import instruments_from_pyo3


class HyperliquidInstrumentProvider(InstrumentProvider):
    """
    Provides Nautilus instrument definitions from Hyperliquid.

    Parameters
    ----------
    client : nautilus_pyo3.HyperliquidHttpClient
        The Hyperliquid HTTP client.
    config : InstrumentProviderConfig, optional
        The instrument provider configuration, by default None.

    """

    def __init__(
        self,
        client: nautilus_pyo3.HyperliquidHttpClient,
        config: InstrumentProviderConfig | None = None,
    ) -> None:
        super().__init__(config=config)
        self._client = client
        self._log_warnings = config.log_warnings if config else True

        self._instruments_pyo3: list[nautilus_pyo3.Instrument] = []

    def instruments_pyo3(self) -> list[Any]:
        """
        Return all Hyperliquid PyO3 instrument definitions held by the provider.

        Returns
        -------
        list[nautilus_pyo3.Instrument]

        """
        return self._instruments_pyo3

    async def load_all_async(self, filters: dict | None = None) -> None:
        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.info(f"Loading all instruments{filters_str}")

        all_pyo3_instruments = []

        try:
            # Get universe from Hyperliquid
            universe = await self._client.get_universe()
            
            # Convert universe to instruments
            pyo3_instruments = await self._client.parse_instruments_pyo3(universe)
            all_pyo3_instruments.extend(pyo3_instruments)

        except Exception as e:
            self._log.error(f"Error loading instruments from Hyperliquid: {e}")
            if self._log_warnings:
                self._log.warning(f"Failed to load instruments: {e}")
            return

        if not all_pyo3_instruments:
            self._log.warning("No instruments loaded")
            return

        self._log.info(f"Loaded {len(all_pyo3_instruments)} instruments")

        # Store the PyO3 instruments
        self._instruments_pyo3 = all_pyo3_instruments

        # Convert to Nautilus instruments and add to internal collection
        nautilus_instruments = instruments_from_pyo3(all_pyo3_instruments)
        for instrument in nautilus_instruments:
            self.add(instrument)

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        if not instrument_ids:
            self._log.info("No instrument IDs provided")
            return

        # Filter for Hyperliquid venue
        hyperliquid_ids = [
            instrument_id for instrument_id in instrument_ids
            if instrument_id.venue == HYPERLIQUID_VENUE
        ]

        if not hyperliquid_ids:
            self._log.info("No Hyperliquid instrument IDs to load")
            return

        filters_str = f"with filters {filters}" if filters else ""
        self._log.info(
            f"Loading instruments {[str(i) for i in hyperliquid_ids]} {filters_str}",
        )

        # For Hyperliquid, we need to load all instruments first since
        # the API doesn't support loading individual instruments by ID
        await self.load_all_async(filters)

        # Filter to requested instruments
        requested_instruments = []
        for instrument_id in hyperliquid_ids:
            instrument = self.find(instrument_id)
            if instrument:
                requested_instruments.append(instrument)
            else:
                self._log.warning(f"Could not find instrument {instrument_id}")

        # Clear all instruments and add only requested ones
        self.clear()
        for instrument in requested_instruments:
            self.add(instrument)

    async def load_async(
        self,
        instrument_id: InstrumentId,
        filters: dict | None = None,
    ) -> None:
        PyCondition.not_none(instrument_id, "instrument_id")

        await self.load_ids_async([instrument_id], filters)
