# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------
"""
Provides instrument provider for Gate.io.
"""

from typing import Any

from nautilus_trader.adapters.gateio2.constants import GATEIO_VENUE
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.nautilus_pyo3 import GateioHttpClient


class GateioInstrumentProvider(InstrumentProvider):
    """
    Provides instruments from Gate.io using the Rust HTTP client.

    Parameters
    ----------
    client : GateioHttpClient
        The Rust HTTP client for making API requests.
    """

    def __init__(
        self,
        client: GateioHttpClient,
    ) -> None:
        super().__init__(venue=GATEIO_VENUE)
        self._client = client

    async def load_all_async(self, filters: dict[str, Any] | None = None) -> None:
        """
        Load all instruments from Gate.io.

        Parameters
        ----------
        filters : dict[str, Any], optional
            Filters to apply when loading instruments (not currently used).
        """
        instruments = await self._client.load_instruments()
        for instrument in instruments:
            self.add(instrument)

    async def load_ids_async(
        self,
        instrument_ids: list[str],
        filters: dict[str, Any] | None = None,
    ) -> None:
        """
        Load specific instruments by ID.

        Parameters
        ----------
        instrument_ids : list[str]
            The instrument IDs to load.
        filters : dict[str, Any], optional
            Filters to apply when loading instruments (not currently used).
        """
        await self.load_all_async(filters)

        # Filter to only requested IDs
        all_instruments = list(self.list_all())
        for instrument in all_instruments:
            if str(instrument.id.symbol) not in instrument_ids:
                self._instruments.pop(instrument.id, None)

    async def load_async(
        self,
        instrument_id: str,
        filters: dict[str, Any] | None = None,
    ) -> None:
        """
        Load a specific instrument by ID.

        Parameters
        ----------
        instrument_id : str
            The instrument ID to load.
        filters : dict[str, Any], optional
            Filters to apply when loading instruments (not currently used).
        """
        await self.load_ids_async([instrument_id], filters)
