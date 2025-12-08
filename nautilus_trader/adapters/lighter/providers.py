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

from collections.abc import Iterable
from typing import TYPE_CHECKING, Any

from nautilus_trader.adapters.lighter.constants import LIGHTER_VENUE
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.instruments import instruments_from_pyo3

if TYPE_CHECKING:
    LighterHttpClient = Any  # nautilus_pyo3.lighter.LighterHttpClient


class LighterInstrumentProvider(InstrumentProvider):
    """
    Loads Lighter perpetual instruments via the public ``orderBooks`` REST endpoint.
    """

    def __init__(
        self,
        client: LighterHttpClient,
        config: InstrumentProviderConfig | None = None,
    ) -> None:
        PyCondition.not_none(client, "client")
        super().__init__(config=config or InstrumentProviderConfig())
        self._client = client
        self._instruments_pyo3: list[Any] = []
        self._market_index_by_instrument: dict[InstrumentId, int] = {}
        self._loaded_instruments: dict[InstrumentId, Instrument] = {}

    # ---------------------------------------------------------------------
    # Public helpers
    # ---------------------------------------------------------------------

    def instruments_pyo3(self) -> list[Any]:
        """
        Return the cached PyO3 instruments.
        """

        return self._instruments_pyo3

    def market_index_for(self, instrument_id: InstrumentId) -> int | None:
        """
        Return the cached market index for the given instrument ID.
        """

        return self._market_index_by_instrument.get(instrument_id)

    # ---------------------------------------------------------------------
    # InstrumentProvider interface
    # ---------------------------------------------------------------------

    async def load_all_async(self, filters: dict | None = None) -> None:
        filters = filters or self._filters

        self._log.info("Loading Lighter instruments...")

        instruments_pyo3 = await self._client.load_instrument_definitions()
        instruments = instruments_from_pyo3(instruments_pyo3)

        self._reset_caches()
        loaded, skipped = self._ingest_instruments(instruments, filters)
        self._cache_pyo3(instruments_pyo3)
        self._cache_market_indices()

        if not loaded:
            self._log.warning("No Lighter instruments matched the requested filters")
        if skipped:
            self._log.debug("Skipped %d Lighter instruments after applying filters", skipped)

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        PyCondition.not_none(instrument_ids, "instrument_ids")
        if not instrument_ids:
            self._log.debug("No instrument IDs provided; nothing to load")
            return

        for instrument_id in instrument_ids:
            PyCondition.equal(
                instrument_id.venue,
                LIGHTER_VENUE,
                "instrument_id.venue",
                LIGHTER_VENUE.value,
            )

        await self.load_all_async(filters)

        missing = [instrument_id for instrument_id in instrument_ids if instrument_id not in self._instruments]
        if missing:
            self._log.warning(
                "Unable to load %d Lighter instruments: %s",
                len(missing),
                ", ".join(inst.value for inst in missing),
            )

    async def load_async(
        self,
        instrument_id: InstrumentId,
        filters: dict | None = None,
    ) -> None:
        PyCondition.not_none(instrument_id, "instrument_id")
        await self.load_ids_async([instrument_id], filters)

    # ---------------------------------------------------------------------
    # Internals
    # ---------------------------------------------------------------------

    def _reset_caches(self) -> None:
        self._instruments.clear()
        self._currencies.clear()
        self._loaded_instruments.clear()
        self._market_index_by_instrument.clear()

    def _ingest_instruments(
        self,
        instruments: Iterable[Instrument],
        filters: dict | None,
    ) -> tuple[int, int]:
        loaded = 0
        skipped = 0

        for instrument in instruments:
            if not self._accept_instrument(instrument, filters):
                skipped += 1
                continue

            self._loaded_instruments[instrument.id] = instrument
            self.add(instrument)
            loaded += 1

        return loaded, skipped

    def _cache_pyo3(self, instruments_pyo3: list[Any]) -> None:
        self._instruments_pyo3 = instruments_pyo3

    def _cache_market_indices(self) -> None:
        # Only cache indices for instruments that were actually ingested.
        allowed_ids = {instrument_id.value: instrument_id for instrument_id in self._instruments}

        for instrument in self._instruments_pyo3:
            try:
                instrument_id: InstrumentId = instrument.id()
            except Exception:  # pragma: no cover - defensive
                continue

            id_value = getattr(instrument_id, "value", str(instrument_id))
            python_id = allowed_ids.get(id_value)
            if python_id is None:
                continue

            market_index = self._client.get_market_index(instrument_id)
            if market_index is not None:
                self._market_index_by_instrument[python_id] = market_index

    def _accept_instrument(self, instrument: Instrument, filters: dict | None) -> bool:
        if not filters:
            return True

        def normalize(value: Any, *, to_lower: bool = False) -> set[str]:
            if value is None:
                return set()
            values: Iterable[str] = value if isinstance(value, Iterable) and not isinstance(value, str) else [value]  # type: ignore[assignment]
            return {
                (item.lower() if to_lower else item.upper())
                for item in values
                if isinstance(item, str)
            }

        # Filter by market index (P1 requirement)
        market_indices = normalize(filters.get("market_indices") or filters.get("market_index"))
        if market_indices:
            try:
                market_index = self._client.get_market_index(instrument.id)
            except Exception:  # pragma: no cover - defensive
                market_index = None
            if market_index is None or str(market_index) not in market_indices:
                return False

        bases = normalize(filters.get("bases"))
        base_code = getattr(getattr(instrument, "base_currency", None), "code", None)
        if bases and (not base_code or base_code.upper() not in bases):
            return False

        quotes = normalize(filters.get("quotes"))
        quote_code = getattr(getattr(instrument, "quote_currency", None), "code", None)
        if quotes and (not quote_code or quote_code.upper() not in quotes):
            return False

        symbols = normalize(filters.get("symbols"))
        symbol_value = getattr(getattr(instrument, "symbol", None), "value", None)
        return not (symbols and (not symbol_value or symbol_value.upper() not in symbols))
