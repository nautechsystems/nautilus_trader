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

import asyncio
from typing import Any

from nautilus_trader.common.component import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.nautilus_pyo3.lighter2 import LighterHttpClient
from nautilus_trader.model.identifiers import InstrumentId


class LighterInstrumentProvider(InstrumentProvider):
    """
    Provides a means of loading `Instrument` objects from the Lighter exchange.

    Parameters
    ----------
    client : LighterHttpClient
        The Lighter HTTP client (Rust).
    logger : Logger
        The logger for the provider.

    """

    def __init__(
        self,
        client: LighterHttpClient,
        logger: Logger,
    ) -> None:
        super().__init__()

        self._client = client
        self._log = logger

    async def load_all_async(self, filters: dict[str, Any] | None = None) -> None:
        """
        Load all instruments for the venue.

        Parameters
        ----------
        filters : dict[str, Any], optional
            The filters to apply to the loading (not yet implemented).

        """
        try:
            self._log.info("Loading instruments from Lighter via Rust client")

            # Load instruments from Rust HTTP client
            instruments = await self._client.load_instruments()

            # Add each instrument to the provider
            for instrument in instruments:
                self.add(instrument)

            self._log.info(f"Loaded {len(instruments)} instruments from Lighter")

        except Exception as e:
            self._log.error(f"Failed to load instruments from Lighter: {e}")
            raise

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict[str, Any] | None = None,
    ) -> None:
        """
        Load specific instruments by their IDs.

        Currently loads all instruments and filters to the requested IDs.

        Parameters
        ----------
        instrument_ids : list[InstrumentId]
            The instrument IDs to load.
        filters : dict[str, Any], optional
            The filters to apply to the loading.

        """
        try:
            # Load all instruments first
            await self.load_all_async(filters)

            # Filter to only keep the requested IDs
            requested_ids = set(instrument_ids)
            for instrument_id in list(self._instruments.keys()):
                if instrument_id not in requested_ids:
                    del self._instruments[instrument_id]

            self._log.info(f"Loaded {len(instrument_ids)} specific instruments from Lighter")

        except Exception as e:
            self._log.error(f"Failed to load specific instruments from Lighter: {e}")
            raise

    async def load_async(
        self,
        instrument_id: InstrumentId,
        filters: dict[str, Any] | None = None,
    ) -> None:
        """
        Load a single instrument by ID.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to load.
        filters : dict[str, Any], optional
            The filters to apply to the loading.

        """
        await self.load_ids_async([instrument_id], filters)

    def load_all(self, filters: dict[str, Any] | None = None) -> None:
        """
        Load all instruments for the venue (sync version).

        Parameters
        ----------
        filters : dict[str, Any], optional
            The filters to apply to the loading.

        """
        asyncio.create_task(self.load_all_async(filters))

    def load_ids(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict[str, Any] | None = None,
    ) -> None:
        """
        Load specific instruments by their IDs (sync version).

        Parameters
        ----------
        instrument_ids : list[InstrumentId]
            The instrument IDs to load.
        filters : dict[str, Any], optional
            The filters to apply to the loading.

        """
        asyncio.create_task(self.load_ids_async(instrument_ids, filters))

    def load(
        self,
        instrument_id: InstrumentId,
        filters: dict[str, Any] | None = None,
    ) -> None:
        """
        Load a single instrument by ID (sync version).

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to load.
        filters : dict[str, Any], optional
            The filters to apply to the loading.

        """
        asyncio.create_task(self.load_async(instrument_id, filters))
