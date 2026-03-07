# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from typing import Any

from nautilus_trader.adapters.bitget.constants import BITGET_VENUE
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import instruments_from_pyo3


class BitgetInstrumentProvider(InstrumentProvider):
    """Provides Nautilus instrument definitions from Bitget."""

    def __init__(
        self,
        client: Any,
        config: InstrumentProviderConfig | None = None,
    ) -> None:
        super().__init__(config=config)
        self._client = client
        self._instruments_pyo3: list[Any] = []

    def instruments_pyo3(self) -> list[Any]:
        """Return raw PyO3 instrument objects currently cached by the provider."""
        return self._instruments_pyo3

    async def load_all_async(self, filters: dict | None = None) -> None:
        """Load all instruments from Bitget when HTTP client support is available."""
        request = getattr(self._client, "request_instruments", None)
        if request is None:
            self._log.warning(
                "Bitget instrument loading is not available in this build yet"
            )
            return

        pyo3_instruments = await request() if callable(request) else []
        self._instruments_pyo3 = pyo3_instruments
        for instrument in instruments_from_pyo3(pyo3_instruments):
            self.add(instrument)

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        """Load selected instrument IDs, enforcing the `BITGET` venue."""
        if not instrument_ids:
            return

        for instrument_id in instrument_ids:
            PyCondition.equal(
                instrument_id.venue, BITGET_VENUE, "instrument_id.venue", "BITGET"
            )

        existing_pyo3_by_id = {
            instrument.id: raw
            for raw, instrument in zip(
                self._instruments_pyo3,
                instruments_from_pyo3(self._instruments_pyo3),
            )
        }

        await super().load_ids_async(instrument_ids, filters=filters)

        current_ids = set(self._instruments)
        filtered_pyo3: list[Any] = []
        seen_ids: set[InstrumentId] = set()
        loaded_pyo3 = list(self._instruments_pyo3)

        for raw, instrument in zip(loaded_pyo3, instruments_from_pyo3(loaded_pyo3)):
            if instrument.id in current_ids and instrument.id not in seen_ids:
                filtered_pyo3.append(raw)
                seen_ids.add(instrument.id)

        for instrument_id, raw in existing_pyo3_by_id.items():
            if instrument_id in current_ids and instrument_id not in seen_ids:
                filtered_pyo3.append(raw)
                seen_ids.add(instrument_id)

        self._instruments_pyo3 = filtered_pyo3
