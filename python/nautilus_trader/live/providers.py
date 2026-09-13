# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
Instrument loading and local storage for Python adapters.
"""

from __future__ import annotations

import asyncio
from typing import TYPE_CHECKING

# Avoid re-entering the config facade while the live package initializes.
from nautilus_trader._libnautilus.live import InstrumentProviderConfig
from nautilus_trader.common import Logger
from nautilus_trader.model import Currency
from nautilus_trader.model import InstrumentId


if TYPE_CHECKING:
    from asyncio import Task
    from collections.abc import Coroutine

    from nautilus_trader._libnautilus.live import _ClientRuntime as ClientRuntime


class InstrumentProvider:
    """
    Loads instrument definitions into adapter-local storage.
    """

    def __init__(self, config: InstrumentProviderConfig | None = None) -> None:
        """
        Retain configuration and local state without starting network work.
        """
        self._config = config if config is not None else InstrumentProviderConfig()
        self._instruments = {}
        self._currencies = {}
        self._loaded = False
        self._init_lock = asyncio.Lock()
        self._runtime = None
        self._log = Logger(type(self).__name__)

    @property
    def count(self) -> int:
        """
        Return the number of locally stored instruments.
        """
        return len(self._instruments)

    async def initialize(self, reload: bool = False) -> None:  # noqa: FBT001, FBT002 - Preserve the established event or provider call signature.
        """
        Load configured instruments once, retrying after a failed initialization.
        """
        async with self._init_lock:
            if self._loaded and not reload:
                return
            if self._config.load_all:
                await self.load_all_async(self._config.filters)
            elif self._config.load_ids:
                instrument_ids = [InstrumentId.from_str(value) for value in self._config.load_ids]
                await self.load_ids_async(instrument_ids, self._config.filters)
            else:
                self._log.warning("No instrument loading configured")
                return

            self._loaded = True

    async def load_all_async(self, filters: dict | None = None) -> None:
        """
        Load all instruments matching the venue-specific filters.
        """
        raise NotImplementedError

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        """
        Load requested instruments and preserve previously loaded instruments.
        """
        if not instrument_ids:
            return

        previous = self._instruments.copy()
        await self.load_all_async(filters)
        requested = set(instrument_ids)

        self._instruments = {
            instrument_id: instrument
            for instrument_id, instrument in self._instruments.items()
            if instrument_id in requested
        }

        for instrument_id, instrument in previous.items():
            self._instruments.setdefault(instrument_id, instrument)

    async def load_async(self, instrument_id: InstrumentId, filters: dict | None = None) -> None:
        """
        Load one instrument if it is not already present.
        """
        if self.find(instrument_id) is None:
            await self.load_ids_async([instrument_id], filters)

    def load_all(self, filters: dict | None = None) -> Task[object] | None:
        """
        Load instruments synchronously or schedule work through the owning client.
        """
        return self._schedule(self.load_all_async(filters), "load_all")

    def load_ids(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> Task[object] | None:
        """
        Load selected instruments synchronously or through the owning client.
        """
        return self._schedule(self.load_ids_async(instrument_ids, filters), "load_ids")

    def load(
        self,
        instrument_id: InstrumentId,
        filters: dict | None = None,
    ) -> Task[object] | None:
        """
        Load one instrument synchronously or through the owning client.
        """
        return self._schedule(self.load_async(instrument_id, filters), "load")

    def add(self, instrument: object) -> None:
        """
        Store an instrument in the provider without changing the core cache.
        """
        if not isinstance(instrument.id, InstrumentId):
            raise TypeError("Expected an instrument with a Nautilus InstrumentId")
        self._instruments[instrument.id] = instrument

    def add_bulk(self, instruments: list[object]) -> None:
        """
        Store instruments in their supplied order.
        """
        for instrument in instruments:
            self.add(instrument)

    def find(self, instrument_id: InstrumentId) -> object | None:
        """
        Return a locally loaded instrument, or None.
        """
        return self._instruments.get(instrument_id)

    def get_all(self) -> dict:
        """
        Return a copy of the instrument mapping.
        """
        return self._instruments.copy()

    def list_all(self) -> list:
        """
        Return the locally loaded instruments.
        """
        return list(self._instruments.values())

    def add_currency(self, currency: Currency) -> None:
        """
        Register a currency and retain it in provider storage.
        """
        Currency.register(currency, overwrite=False)
        self._currencies[currency.code] = currency

    def currencies(self) -> dict[str, Currency]:
        """
        Return a copy of the local currency mapping.
        """
        return self._currencies.copy()

    def currency(self, code: str) -> Currency:
        """
        Return a local currency or resolve its code in the domain registry.
        """
        if not isinstance(code, str) or not code:
            raise ValueError("Currency code must be a non-empty string")
        return self._currencies.get(code) or Currency.from_str(code)

    def _bind(self, runtime: ClientRuntime) -> None:
        if self._runtime is not None and self._runtime is not runtime:
            raise RuntimeError("An instrument provider cannot belong to multiple clients")
        self._runtime = runtime

    def _schedule(
        self,
        coroutine: Coroutine[object, object, object],
        operation: str,
    ) -> Task[object] | None:
        if self._runtime is not None:
            return self._runtime.create_task(coroutine, f"provider:{operation}")
        try:
            asyncio.get_running_loop()
        except RuntimeError:
            return asyncio.run(coroutine)

        coroutine.close()
        raise RuntimeError("Await the async provider method or bind the provider to a client")
