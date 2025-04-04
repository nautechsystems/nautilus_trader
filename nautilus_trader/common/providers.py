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

from nautilus_trader.common.component import Logger
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Currency


class InstrumentProvider:
    """
    The base class for all instrument providers.

    Parameters
    ----------
    config :InstrumentProviderConfig, optional
        The instrument provider config.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.

    """

    def __init__(self, config: InstrumentProviderConfig | None = None) -> None:
        if config is None:
            config = InstrumentProviderConfig()
        self._log = Logger(name=type(self).__name__)

        self._instruments: dict[InstrumentId, Instrument] = {}
        self._currencies: dict[str, Currency] = {}

        # Configuration
        self._load_all_on_start = config.load_all
        self._load_ids_on_start = set(config.load_ids) if config.load_ids is not None else None
        self._filters = config.filters

        # Async loading flags
        self._loaded = False
        self._loading = False

        self._tasks: set[asyncio.Task] = set()

        self._log.info("READY")

    @property
    def count(self) -> int:
        """
        Return the count of instruments held by the provider.

        Returns
        -------
        int

        """
        return len(self._instruments)

    async def load_all_async(
        self,
        filters: dict | None = None,
    ) -> None:
        """
        Load the latest instruments into the provider asynchronously, optionally
        applying the given filters.
        """
        raise NotImplementedError(
            "method `load_all_async` must be implemented in the subclass",
        )  # pragma: no cover

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        """
        Load the instruments for the given IDs into the provider, optionally applying
        the given filters.

        Parameters
        ----------
        instrument_ids : list[InstrumentId]
            The instrument IDs to load.
        filters : frozendict[str, Any] or dict[str, Any], optional
            The venue specific instrument loading filters to apply.

        Raises
        ------
        ValueError
            If any `instrument_id.venue` is not equal to `self.venue`.

        """
        raise NotImplementedError(
            "method `load_ids_async` must be implemented in the subclass",
        )  # pragma: no cover

    async def load_async(
        self,
        instrument_id: InstrumentId,
        filters: dict | None = None,
    ) -> None:
        """
        Load the instrument for the given ID into the provider asynchronously,
        optionally applying the given filters.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to load.
        filters : frozendict[str, Any] or dict[str, Any], optional
            The venue specific instrument loading filters to apply.

        Raises
        ------
        ValueError
            If `instrument_id.venue` is not equal to `self.venue`.

        """
        raise NotImplementedError(
            "method `load_async` must be implemented in the subclass",
        )  # pragma: no cover

    async def initialize(self, reload: bool = False) -> None:
        """
        Initialize the instrument provider.

        Parameters
        ----------
        reload : bool, default False
            If True, then will always reload instruments.
            If False, then will immediately return if already loaded.

        """
        if not reload and self._loaded:
            return  # Already loaded

        if not self._load_all_on_start and not self._load_ids_on_start:
            self._log.warning(
                "No loading configured: ensure either `load_all=True` or there are `load_ids`",
            )
            return

        if self._loading:
            self._log.debug("Awaiting loading...")
            while self._loading:
                await asyncio.sleep(0.1)

        if not reload and self._loaded:
            return  # Already loaded

        # Set state flag
        self._loading = True
        self._log.info("Initializing instruments...")

        try:
            if self._load_all_on_start:
                await self.load_all_async(self._filters)
            elif self._load_ids_on_start:
                instrument_ids = [
                    i if isinstance(i, InstrumentId) else InstrumentId.from_str(i)
                    for i in self._load_ids_on_start
                ]

                instruments_str = ", ".join([i.value for i in instrument_ids])
                filters_str = "..." if not self._filters else f" with filters {self._filters}..."
                self._log.info(f"Loading instruments: {instruments_str}{filters_str}")

                await self.load_ids_async(instrument_ids, self._filters)
        except Exception as e:
            # Catch unexpected exception to ensure that the self._loading flag
            # is reset to False
            self._log.exception("Failed to load instruments", e)

        if self._instruments:
            self._log.info(f"Loaded {self.count} instruments")
        else:
            self._log.warning("No instruments were loaded, verify config if this is unexpected")

        # Set state flags
        self._loading = False
        self._loaded = True

        self._log.info("Initialized instruments")

    def load_all(self, filters: dict | None = None) -> None:
        """
        Load the latest instruments into the provider, optionally applying the given
        filters.

        Parameters
        ----------
        filters : frozendict[str, Any] or dict[str, Any], optional
            The venue specific instrument loading filters to apply.

        """
        loop = asyncio.get_event_loop()
        if loop.is_running():
            task = loop.create_task(self.load_all_async(filters))
            self._tasks.add(task)
        else:
            loop.run_until_complete(self.load_all_async(filters))

    def load_ids(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        """
        Load the instruments for the given IDs into the provider, optionally applying
        the given filters.

        Parameters
        ----------
        instrument_ids : list[InstrumentId]
            The instrument IDs to load.
        filters : frozendict[str, Any] or dict[str, Any], optional
            The venue specific instrument loading filters to apply.

        """
        PyCondition.not_none(instrument_ids, "instrument_ids")

        loop = asyncio.get_event_loop()
        if loop.is_running():
            task = loop.create_task(self.load_ids_async(instrument_ids, filters))
            self._tasks.add(task)
        else:
            loop.run_until_complete(self.load_ids_async(instrument_ids, filters))

    def load(
        self,
        instrument_id: InstrumentId,
        filters: dict | None = None,
    ) -> None:
        """
        Load the instrument for the given ID into the provider, optionally applying the
        given filters.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to load.
        filters : frozendict[str, Any] or dict[str, Any], optional
            The venue specific instrument loading filters to apply.

        """
        PyCondition.not_none(instrument_id, "instrument_id")

        loop = asyncio.get_event_loop()
        if loop.is_running():
            task = loop.create_task(self.load_async(instrument_id, filters))
            self._tasks.add(task)
        else:
            loop.run_until_complete(self.load_async(instrument_id, filters))

    def add_currency(self, currency: Currency) -> None:
        """
        Add the given currency to the provider.

        Parameters
        ----------
        currency : Currency
            The currency to add.

        """
        PyCondition.not_none(currency, "currency")

        self._currencies[currency.code] = currency
        Currency.register(currency, overwrite=False)

    def add(self, instrument: Instrument) -> None:
        """
        Add the given instrument to the provider.

        Parameters
        ----------
        instrument : Instrument
            The instrument to add.

        """
        PyCondition.not_none(instrument, "instrument")

        self._instruments[instrument.id] = instrument

    def add_bulk(self, instruments: list[Instrument]) -> None:
        """
        Add the given instruments bulk to the provider.

        Parameters
        ----------
        instruments : list[Instrument]
            The instruments to add.

        """
        PyCondition.not_none(instruments, "instruments")

        for instrument in instruments:
            self.add(instrument)

    def list_all(self) -> list[Instrument]:
        """
        Return all loaded instruments.

        Returns
        -------
        list[Instrument]

        """
        return list(self.get_all().values())

    def get_all(self) -> dict[InstrumentId, Instrument]:
        """
        Return all loaded instruments as a map keyed by instrument ID.

        If no instruments loaded, will return an empty dict.

        Returns
        -------
        dict[InstrumentId, Instrument]

        """
        return self._instruments.copy()

    def currencies(self) -> dict[str, Currency]:
        """
        Return all currencies held by the instrument provider.

        Returns
        -------
        dict[str, Currency]

        """
        return self._currencies.copy()

    def currency(self, code: str) -> Currency | None:
        """
        Return the currency with the given code (if found).

        Parameters
        ----------
        code : str
            The currency code.

        Returns
        -------
        Currency or ``None``

        Raises
        ------
        ValueError
            If `code` is not a valid string.

        """
        PyCondition.valid_string(code, "code")

        ccy = self._currencies.get(code)
        if ccy is None:
            ccy = Currency.from_str(code)
        return ccy

    def find(self, instrument_id: InstrumentId) -> Instrument | None:
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
        PyCondition.not_none(instrument_id, "instrument_id")

        return self._instruments.get(instrument_id)
