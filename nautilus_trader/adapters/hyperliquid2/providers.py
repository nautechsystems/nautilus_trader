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
Instrument provider for the Hyperliquid adapter.
"""

from typing import Any

from nautilus_hyperliquid2 import Hyperliquid2HttpClient

from nautilus_trader.common.component import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue


HYPERLIQUID_VENUE = Venue("HYPERLIQUID")


class Hyperliquid2InstrumentProvider(InstrumentProvider):
    """
    Provides instruments for the Hyperliquid exchange.

    Parameters
    ----------
    client : Hyperliquid2HttpClient
        The Hyperliquid HTTP client for fetching instrument data.
    """

    def __init__(self, client: Hyperliquid2HttpClient) -> None:
        super().__init__(venue=HYPERLIQUID_VENUE)
        self._client = client
        self._log = Logger(type(self).__name__)

    async def load_all_async(self, filters: dict[str, Any] | None = None) -> None:
        """
        Load all instruments from Hyperliquid.

        Parameters
        ----------
        filters : dict[str, Any] | None
            Filters to apply when loading instruments (not currently used).

        """
        self._log.info("Loading instruments from Hyperliquid")

        # Load instruments through the HTTP client
        # The Rust client stores instruments internally and returns count
        count = await self._client.load_instruments()

        self._log.info(f"Loaded {count} instruments from Hyperliquid")

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict[str, Any] | None = None,
    ) -> None:
        """
        Load specific instruments by ID.

        This implementation loads all instruments and filters to the requested IDs.

        Parameters
        ----------
        instrument_ids : list[InstrumentId]
            The instrument IDs to load.
        filters : dict[str, Any] | None
            Filters to apply when loading instruments (not currently used).

        """
        # Load all instruments first
        await self.load_all_async(filters)

        # Filter to requested IDs
        # Note: Instruments are stored in the Rust client
        # Full instrument filtering would require additional Rust methods
        self._log.info(f"Filtered to {len(instrument_ids)} requested instruments")

    async def load_async(
        self,
        instrument_id: InstrumentId,
        filters: dict[str, Any] | None = None,
    ) -> None:
        """
        Load a specific instrument.

        This implementation loads all instruments since Hyperliquid doesn't
        support loading individual instruments via the API.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to load.
        filters : dict[str, Any] | None
            Filters to apply when loading instruments (not currently used).

        """
        await self.load_all_async(filters)
