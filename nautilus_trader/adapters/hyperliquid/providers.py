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

from __future__ import annotations

from typing import TYPE_CHECKING, Any

from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.identifiers import InstrumentId


if TYPE_CHECKING:
    pass  # TODO: Add imports for actual HyperliquidHttpClient when available


class HyperliquidInstrumentProvider(InstrumentProvider):
    """
    Provides Nautilus instrument definitions from Hyperliquid.

    Parameters
    ----------
    client : Any
        The Hyperliquid HTTP client (to be implemented).
    config : InstrumentProviderConfig, optional
        The instrument provider configuration, by default None.

    """

    def __init__(
        self,
        client: Any | None = None,  # TODO: Replace with actual HyperliquidHttpClient when available
        config: InstrumentProviderConfig | None = None,
    ) -> None:
        super().__init__(config=config)
        self._client = client
        self._log_warnings = config.log_warnings if config else True

        self._instruments_pyo3: list[Any] = []

    def instruments_pyo3(self) -> list[Any]:
        """
        Return all Hyperliquid PyO3 instrument definitions held by the provider.

        Returns
        -------
        list[Any]

        """
        return self._instruments_pyo3

    async def load_all_async(self, filters: dict | None = None) -> None:
        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.info(f"Loading all instruments{filters_str}")

        # TODO: Implement actual API call when HyperliquidHttpClient is available
        # For now, we'll create a placeholder implementation
        self._log.warning("Hyperliquid instrument loading not yet implemented - using placeholder")

        # When the actual client is available, this should be:
        # pyo3_instruments = await self._client.request_instruments()
        # self._instruments_pyo3 = pyo3_instruments
        # instruments = instruments_from_pyo3(pyo3_instruments)
        # for instrument in instruments:
        #     self.add(instrument=instrument)

        self._log.info("Loaded 0 instruments (placeholder)")

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        if not instrument_ids:
            self._log.warning("No instrument IDs given for loading")
            return

        PyCondition.not_none(instrument_ids, "instrument_ids")

        self._log.info(f"Loading instruments {instrument_ids}...")

        # TODO: Implement actual API call when HyperliquidHttpClient is available
        self._log.warning("Hyperliquid specific instrument loading not yet implemented")

        self._log.info("Loaded 0 specific instruments (placeholder)")
