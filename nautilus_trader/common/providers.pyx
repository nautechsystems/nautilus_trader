# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
from typing import Dict, List, Optional

from nautilus_trader.config import InstrumentProviderConfig

from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instruments.base cimport Instrument


cdef class InstrumentProvider:
    """
    The abstract base class for all instrument providers.

    Parameters
    ----------
    venue : Venue
        The venue for the provider.
    logger : Logger
        The logger for the provider.
    config :InstrumentProviderConfig, optional
        The instrument provider config.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        Venue venue not None,
        Logger logger not None,
        config: Optional[InstrumentProviderConfig]=None,
    ):
        if config is None:
            config = InstrumentProviderConfig()
        self._log = LoggerAdapter(type(self).__name__, logger)

        self.venue = venue
        self._instruments = {}  # type: dict[InstrumentId, Instrument]
        self._currencies = {}   # type: dict[str, Currency]

        # Settings
        self._load_all_on_start = config.load_all
        self._load_ids_on_start = set(config.load_ids) if config.load_ids is not None else None
        self._filters = config.filters

        # Async loading flags
        self._loaded = False
        self._loading = False

    @property
    def count(self) -> int:
        """
        The count of instruments held by the provider.

        Returns
        -------
        int

        """
        return len(self._instruments)

    async def load_all_async(self, filters: Optional[Dict] = None) -> None:
        """
        Load the latest instruments into the provider asynchronously, optionally
        applying the given filters.
        """
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def load_ids_async(
        self,
        instrument_ids: List[InstrumentId],
        filters: Optional[Dict]=None,
    ) -> None:
        """
        Load the instruments for the given IDs into the provider, optionally
        applying the given filters.

        Parameters
        ----------
        instrument_ids: List[InstrumentId]
            The instrument IDs to load.
        filters : Dict, optional
            The venue specific instrument loading filters to apply.

        Raises
        ------
        ValueError
            If any `instrument_id.venue` is not equal to `self.venue`.

        """
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def load_async(self, instrument_id: InstrumentId, filters: Optional[Dict] = None):
        """
        Load the instrument for the given ID into the provider asynchronously, optionally
        applying the given filters.

        Parameters
        ----------
        instrument_id: InstrumentId
            The instrument ID to load.
        filters : Dict, optional
            The venue specific instrument loading filters to apply.

        Raises
        ------
        ValueError
            If `instrument_id.venue` is not equal to `self.venue`.

        """
        raise NotImplementedError("method must be implemented in the subclass")  # pragma: no cover

    async def initialize(self) -> None:
        """
        Initialize the instrument provider.

        If `initialize()` then will immediately return.
        """
        if self._loaded:
            return  # Already loaded

        if not self._loading:
            # Set async loading flag
            self._loading = True
            if self._load_all_on_start:
                await self.load_all_async(self._filters)
            elif self._load_ids_on_start:
                instrument_ids = [InstrumentId.from_str_c(i) for i in self._load_ids_on_start]
                await self.load_ids_async(instrument_ids, self._filters)
            self._log.info(f"Loaded {self.count} instruments.")
        else:
            self._log.debug("Awaiting loading...")
            while self._loading:
                # Wait 100ms
                await asyncio.sleep(0.1)

        # Set async loading flags
        self._loading = False
        self._loaded = True

    def load_all(self, filters: Optional[Dict] = None) -> None:
        """
        Load the latest instruments into the provider, optionally applying the
        given filters.

        Parameters
        ----------
        filters : Dict, optional
            The venue specific instrument loading filters to apply.

        """
        loop = asyncio.get_event_loop()
        if loop.is_running():
            loop.create_task(self.load_all_async(filters))
        else:
            loop.run_until_complete(self.load_all_async(filters))

    def load_ids(self, instrument_ids: List[InstrumentId], filters: Optional[Dict] = None) -> None:
        """
        Load the instruments for the given IDs into the provider, optionally
        applying the given filters.

        Parameters
        ----------
        instrument_ids: List[InstrumentId]
            The instrument IDs to load.
        filters : Dict, optional
            The venue specific instrument loading filters to apply.

        """
        loop = asyncio.get_event_loop()
        if loop.is_running():
            loop.create_task(self.load_ids_async(instrument_ids, filters))
        else:
            loop.run_until_complete(self.load_ids_async(instrument_ids, filters))

    def load(self, instrument_id: InstrumentId, filters: Optional[Dict] = None) -> None:
        """
        Load the instrument for the given ID into the provider, optionally
        applying the given filters.

        Parameters
        ----------
        instrument_id: InstrumentId
            The instrument ID to load.
        filters : Dict, optional
            The venue specific instrument loading filters to apply.

        """
        loop = asyncio.get_event_loop()
        if loop.is_running():
            loop.create_task(self.load_async(instrument_id, filters))
        else:
            loop.run_until_complete(self.load_async(instrument_id, filters))

    cpdef void add_currency(self, Currency currency) except *:
        """
        Add the given currency to the provider.

        Parameters
        ----------
        currency : Currency
            The currency to add.

        """
        self._currencies[currency.code] = currency
        Currency.register_c(currency, overwrite=False)

    cpdef void add(self, Instrument instrument) except *:
        """
        Add the given instrument to the provider.

        Parameters
        ----------
        instrument : Instrument
            The instrument to add.

        """
        self._instruments[instrument.id] = instrument

    cpdef void add_bulk(self, list instruments) except *:
        """
        Add the given instruments bulk to the provider.

        Parameters
        ----------
        instruments : list[Instrument]
            The instruments to add.

        """
        Condition.not_none(instruments, "instruments")

        cdef Instrument instrument
        for instrument in instruments:
            self.add(instrument)

    cpdef list list_all(self):
        """
        Return all loaded instruments.

        Returns
        -------
        list[Instrument]

        """
        return list(self.get_all().values())

    cpdef dict get_all(self):
        """
        Return all loaded instruments as a map keyed by instrument ID.

        If no instruments loaded, will return an empty dict.

        Returns
        -------
        dict[InstrumentId, Instrument]

        """
        return self._instruments.copy()

    cpdef dict currencies(self):
        """
        Return all currencies held by the instrument provider.

        Returns
        -------
        dict[str, Currency]

        """
        return self._currencies.copy()

    cpdef Currency currency(self, str code):
        """
        Return the currency with the given code (if found).

        Parameters
        ----------
        code : str
            The currency code.

        Returns
        -------
        Currency or ``None``

        """
        cdef Currency currency = self._currencies.get(code)
        if currency is None:
            currency = Currency.from_str_c(code)
        return currency

    cpdef Instrument find(self, InstrumentId instrument_id):
        """
        Return the instrument for the given instrument ID (if found).

        Parameters
        ----------
        instrument_id : InstrumentId
            The ID for the instrument

        Returns
        -------
        Instrument or ``None``

        """
        return self._instruments.get(instrument_id)
