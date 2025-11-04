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
Providers for Asterdex adapters.
"""

from typing import Any

from nautilus_trader.adapters.asterdex2.http.client import AsterdexHttpClient
from nautilus_trader.common.enums import LogColor
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue


ASTERDEX_VENUE = Venue("ASTERDEX")


class AsterdexInstrumentProvider(InstrumentProvider):
    """
    Provides instrument definitions for Asterdex exchange.

    Parameters
    ----------
    client : AsterdexHttpClient
        The Asterdex HTTP client.
    """

    def __init__(self, client: AsterdexHttpClient) -> None:
        super().__init__(venue=ASTERDEX_VENUE)
        self._client = client
        self._log_warnings = True

    async def load_all_async(self, filters: dict[str, Any] | None = None) -> None:
        """
        Load all instruments into the provider asynchronously.

        Parameters
        ----------
        filters : dict[str, Any], optional
            Not applicable for Asterdex.

        """
        # Load instruments from the HTTP client
        # Note: The Rust client returns count, actual instruments stored internally
        count = await self._client.load_instruments()

        if self._log_warnings:
            self._log.info(
                f"Loaded {count} instruments from Asterdex",
                LogColor.BLUE,
            )

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict[str, Any] | None = None,
    ) -> None:
        """
        Load specific instrument IDs into the provider asynchronously.

        Parameters
        ----------
        instrument_ids : list[InstrumentId]
            The instrument IDs to load.
        filters : dict[str, Any], optional
            Not applicable for Asterdex.

        """
        # For now, load all instruments
        # TODO: Implement selective loading if needed
        await self.load_all_async(filters=filters)

    async def load_async(
        self,
        instrument_id: InstrumentId,
        filters: dict[str, Any] | None = None,
    ) -> None:
        """
        Load a specific instrument into the provider asynchronously.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to load.
        filters : dict[str, Any], optional
            Not applicable for Asterdex.

        """
        # For now, load all instruments
        # TODO: Implement selective loading if needed
        await self.load_all_async(filters=filters)
