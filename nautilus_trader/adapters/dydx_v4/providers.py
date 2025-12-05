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
Instrument provider for the dYdX v4 venue.

This provider uses the Rust-backed HTTP client to fetch instruments from dYdX.

"""

from typing import Any

from nautilus_trader.adapters.dydx_v4.constants import DYDX_VENUE
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import instruments_from_pyo3


class DYDXv4InstrumentProvider(InstrumentProvider):
    """
    Provides Nautilus instrument definitions from dYdX v4.

    This provider uses the Rust-backed HTTP client to fetch instruments
    from the dYdX Indexer API.

    Parameters
    ----------
    client : nautilus_pyo3.DydxHttpClient
        The dYdX HTTP client (Rust-backed).
    config : InstrumentProviderConfig, optional
        The instrument provider configuration, by default None.

    """

    def __init__(
        self,
        client: nautilus_pyo3.DydxHttpClient,  # type: ignore[name-defined]
        config: InstrumentProviderConfig | None = None,
    ) -> None:
        super().__init__(config=config)
        self._client = client
        self._log_warnings = config.log_warnings if config else True

        self._instruments_pyo3: list[Any] = []

    def instruments_pyo3(self) -> list[Any]:
        """
        Return all dYdX PyO3 instrument definitions held by the provider.

        Returns
        -------
        list[nautilus_pyo3.Instrument]

        """
        return self._instruments_pyo3

    async def load_all_async(self, filters: dict | None = None) -> None:
        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.info(f"Loading all instruments{filters_str}")

        # Fetch instruments via Rust HTTP client
        pyo3_instruments = await self._client.request_instruments(
            maker_fee=None,
            taker_fee=None,
        )

        self._instruments_pyo3 = pyo3_instruments
        instruments = instruments_from_pyo3(pyo3_instruments)
        for instrument in instruments:
            self.add(instrument=instrument)

        self._log.info(f"Loaded {len(instruments)} instruments")

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        if not instrument_ids:
            self._log.info("No instrument IDs given for loading")
            return

        # Check all instrument IDs
        for instrument_id in instrument_ids:
            PyCondition.equal(instrument_id.venue, DYDX_VENUE, "instrument_id.venue", "DYDX")

        # dYdX doesn't support fetching individual instruments, so we load all and filter
        await self.load_all_async(filters)

        # Filter to only the requested instruments
        loaded_ids = {inst.id for inst in self.get_all().values()}
        missing = [str(iid) for iid in instrument_ids if iid not in loaded_ids]
        if missing and self._log_warnings:
            self._log.warning(f"Instruments not found: {missing}")

    async def load_async(
        self,
        instrument_id: InstrumentId,
        filters: dict | None = None,
    ) -> None:
        PyCondition.equal(instrument_id.venue, DYDX_VENUE, "instrument_id.venue", "DYDX")

        # Check if already loaded
        if self.find(instrument_id) is not None:
            return

        # dYdX doesn't support fetching individual instruments, so we load all
        await self.load_all_async(filters)

        if self.find(instrument_id) is None and self._log_warnings:
            self._log.warning(f"Instrument {instrument_id} not found")
