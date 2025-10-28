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
from typing import TYPE_CHECKING
from typing import Any

from nautilus_trader.adapters.hyperliquid.constants import HYPERLIQUID_VENUE
from nautilus_trader.adapters.hyperliquid.enums import DEFAULT_PRODUCT_TYPES
from nautilus_trader.adapters.hyperliquid.enums import HyperliquidProductType
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.instruments import instruments_from_pyo3


if TYPE_CHECKING:
    # PyO3 types from Rust (temporary namespace qualification)
    HyperliquidHttpClient = Any  # nautilus_pyo3.HyperliquidHttpClient (stub not yet available)


class HyperliquidInstrumentProvider(InstrumentProvider):
    """
    Load spot and perpetual instruments from Hyperliquid's REST ``/info`` API.
    """

    def __init__(
        self,
        client: HyperliquidHttpClient,
        config: InstrumentProviderConfig | None = None,
        *,
        product_types: Iterable[HyperliquidProductType] | None = None,
    ) -> None:
        PyCondition.not_none(client, "client")
        super().__init__(config=config or InstrumentProviderConfig())

        self._client: HyperliquidHttpClient = client

        resolved_types = (
            DEFAULT_PRODUCT_TYPES
            if product_types is None
            else frozenset(HyperliquidProductType(pt) for pt in product_types)
        )
        if not resolved_types:
            raise ValueError("product_types must contain at least one entry")

        self._product_types = resolved_types

        self._loaded_instruments: dict[InstrumentId, Instrument] = {}
        self._instruments_pyo3: list[Any] = []

    # ---------------------------------------------------------------------
    # Public helpers
    # ---------------------------------------------------------------------

    def instruments_pyo3(self) -> list[Any]:
        """
        Return the cached PyO3 instruments (for WebSocket client).

        Returns
        -------
        list[nautilus_pyo3.Instrument]

        """
        return self._instruments_pyo3

    # ---------------------------------------------------------------------
    # InstrumentProvider interface
    # ---------------------------------------------------------------------

    async def load_all_async(self, filters: dict | None = None) -> None:
        filters = filters or self._filters

        self._log.info("Loading Hyperliquid instruments...")

        instruments = await self._load_instruments()

        self._log.info("Applying filters")

        self._reset_caches()

        loaded, skipped = self._ingest_instruments(instruments, filters)

        if loaded:
            self._log.info(f"Loaded {loaded} instruments for venue {HYPERLIQUID_VENUE.value}")
        else:
            self._log.warning("No Hyperliquid instruments matched the requested filters")

        if skipped:
            self._log.debug(f"Skipped {skipped} instruments after applying filters")

    async def _load_instruments(self) -> list[Instrument]:
        try:
            pyo3_instruments = await self._client.load_instrument_definitions(
                include_perp=HyperliquidProductType.PERP in self._product_types,
                include_spot=HyperliquidProductType.SPOT in self._product_types,
            )
            # Store PyO3 instruments for WebSocket client
            self._instruments_pyo3 = pyo3_instruments
            # Convert PyO3 instruments to Python (Cython) instruments
            # This is necessary because the data engine expects Python instruments
            instruments = instruments_from_pyo3(pyo3_instruments)
            return instruments
        except AttributeError:  # method missing (old wheel?)
            self._log.error("HyperliquidHttpClient is missing load_instrument_definitions")
            raise
        except Exception as e:  # pragma: no cover - defensive logging
            self._log.exception("Failed to fetch Hyperliquid instrument metadata", e)
            raise

    def _reset_caches(self) -> None:
        self._instruments.clear()
        self._currencies.clear()
        self._loaded_instruments.clear()
        self._instruments_pyo3.clear()

    def _ingest_instruments(
        self,
        instruments: Iterable[Instrument],
        filters: dict | None,
    ) -> tuple[int, int]:
        loaded = 0
        skipped = 0

        for instrument in instruments:
            product_type = self._instrument_product_type(instrument)
            if product_type is None:
                skipped += 1
                continue

            if product_type not in self._product_types:
                continue

            if not self._accept_instrument(instrument, filters):
                continue

            self._loaded_instruments[instrument.id] = instrument
            self.add(instrument)
            loaded += 1

        return loaded, skipped

    def _instrument_product_type(
        self,
        instrument: Instrument,
    ) -> HyperliquidProductType | None:
        if isinstance(instrument, CryptoPerpetual):
            return HyperliquidProductType.PERP
        if isinstance(instrument, CurrencyPair):
            return HyperliquidProductType.SPOT

        self._log.warning(
            f"Ignoring Hyperliquid instrument {instrument.id.value} (unsupported type {type(instrument).__name__})",
        )
        return None

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
                HYPERLIQUID_VENUE,
                "instrument_id.venue",
                HYPERLIQUID_VENUE.value,
            )

        # We currently fetch the full catalog (low cost) and rely on filtering afterwards.
        await self.load_all_async(filters)

        missing = [i for i in instrument_ids if i not in self._instruments]
        if missing:
            self._log.warning(
                "Unable to load %d Hyperliquid instruments: %s",
                len(missing),
                ", ".join(i.value for i in missing),
            )

    async def load_async(
        self,
        instrument_id: InstrumentId,
        filters: dict | None = None,
    ) -> None:
        PyCondition.not_none(instrument_id, "instrument_id")
        await self.load_ids_async([instrument_id], filters)

    # ------------------------------------------------------------------
    # Internal helpers
    # ------------------------------------------------------------------

    def _accept_instrument(
        self,
        instrument: Instrument,
        filters: dict | None,
    ) -> bool:
        if not filters:
            return True

        def _normalize(value: Any, *, to_lower: bool = False) -> set[str]:
            if value is None:
                return set()
            if isinstance(value, str):
                values: Iterable[str] = [value]
            else:
                values = value
            return {
                (item.lower() if to_lower else item.upper())
                for item in values
                if isinstance(item, str)
            }

        market_type = "perp" if isinstance(instrument, CryptoPerpetual) else "spot"
        kinds = _normalize(filters.get("market_types") or filters.get("kinds"), to_lower=True)
        if kinds and market_type not in kinds:
            return False

        base_code = getattr(getattr(instrument, "base_currency", None), "code", None)
        bases = _normalize(filters.get("bases"))
        if bases and (not base_code or base_code.upper() not in bases):
            return False

        quote_code = getattr(getattr(instrument, "quote_currency", None), "code", None)
        quotes = _normalize(filters.get("quotes"))
        if quotes and (not quote_code or quote_code.upper() not in quotes):
            return False

        symbol_value = getattr(getattr(instrument, "symbol", None), "value", None)
        symbols = _normalize(filters.get("symbols"))
        if symbols and (not symbol_value or symbol_value.upper() not in symbols):
            return False

        return True
