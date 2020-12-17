# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from cpython.datetime cimport datetime

import ccxt

from nautilus_trader.adapters.binance.providers import BinanceInstrumentProvider
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.constants cimport *  # str constants only
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.live.data cimport LiveDataClient
from nautilus_trader.live.data cimport LiveDataEngine
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue


cdef class BinanceDataClient(LiveDataClient):
    """
    Provides a data client for the `Binance` exchange.
    """

    def __init__(
        self,
        dict credentials,
        LiveDataEngine engine,
        LiveClock clock,
        Logger logger,
    ):
        """
        Initialize a new instance of the `BinanceDataClient` class.

        Parameters
        ----------
        credentials : dict[str, str]
            The API credentials to use for the client.
        engine : LiveDataEngine
            The live data engine for the client.
        clock : LiveClock
            The clock for the client.
        logger : Logger
            The logger for the client.

        """
        Condition.not_none(credentials, "credentials")
        super().__init__(
            Venue("BINANCE"),
            engine,
            clock,
            logger,
        )

        cdef dict config = {
            "apiKey": credentials.get("api_key"),
            "secret": credentials.get("api_secret"),
            "timeout": 10000,
            "enableRateLimit": True,
        }

        self._is_connected = False
        self._config = config
        self._client = ccxt.binance(config=config)
        self._instrument_provider = BinanceInstrumentProvider(
            client=self._client,
            load_all=False,
        )

    def __repr__(self) -> str:
        return f"{type(self).__name__}"

    cpdef bint is_connected(self) except *:
        """
        Return a value indicating whether the client is connected.

        Returns
        -------
        bool
            True if connected, else False.

        """
        return self._is_connected

    cpdef void connect(self) except *:
        """
        Connect the client.
        """
        self._log.debug("Connecting...")

        self._loop.create_task(self._startup_sequence())

        # Emulates a socket style connection
        self._is_connected = True
        self._log.info("Connected.")

    cpdef void disconnect(self) except *:
        """
        Disconnect the client.
        """
        self._log.debug("Disconnecting...")

        self._is_connected = False

        self._log.info("Disconnected.")

    cpdef void reset(self) except *:
        """
        Reset the client.
        """
        self._client = ccxt.binance(config=self._config)

    cpdef void dispose(self) except *:
        """
        Dispose the client.
        """
        pass  # Nothing to dispose yet

# -- SUBSCRIPTIONS ---------------------------------------------------------------------------------

    cpdef void subscribe_instrument(self, Symbol symbol) except *:
        """
        Subscribe to `Instrument` data for the given symbol.

        Parameters
        ----------
        symbol : Instrument
            The instrument symbol to subscribe to.

        """
        Condition.not_none(symbol, "symbol")

        # TODO: Implement

    cpdef void subscribe_quote_ticks(self, Symbol symbol) except *:
        """
        Subscribe to `QuoteTick` data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol to subscribe to.

        """
        Condition.not_none(symbol, "symbol")

        # TODO: Implement

    cpdef void subscribe_trade_ticks(self, Symbol symbol) except *:
        """
        Subscribe to `TradeTick` data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol to subscribe to.

        """
        Condition.not_none(symbol, "symbol")

        # TODO: Implement

    cpdef void subscribe_bars(self, BarType bar_type) except *:
        """
        Subscribe to `Bar` data for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to subscribe to.

        """
        Condition.not_none(bar_type, "bar_type")

        # TODO: Implement

    cpdef void unsubscribe_instrument(self, Symbol symbol) except *:
        """
        Unsubscribe from `Instrument` data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The instrument symbol to unsubscribe from.

        """
        Condition.not_none(symbol, "symbol")

        # TODO: Implement

    cpdef void unsubscribe_quote_ticks(self, Symbol symbol) except *:
        """
        Unsubscribe from `QuoteTick` data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol to unsubscribe from.

        """
        Condition.not_none(symbol, "symbol")

        # TODO: Implement

    cpdef void unsubscribe_trade_ticks(self, Symbol symbol) except *:
        """
        Unsubscribe from `TradeTick` data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol to unsubscribe from.

        """
        Condition.not_none(symbol, "symbol")

        # TODO: Implement

    cpdef void unsubscribe_bars(self, BarType bar_type) except *:
        """
        Unsubscribe from `Bar` data for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to unsubscribe from.

        """
        Condition.not_none(bar_type, "bar_type")

        # TODO: Implement

# -- REQUESTS --------------------------------------------------------------------------------------

    cpdef void request_instrument(self, Symbol symbol, UUID correlation_id) except *:
        """
        Request the instrument for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the request.
        correlation_id : UUID
            The correlation identifier for the request.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(correlation_id, "correlation_id")

        # TODO: Implement

    cpdef void request_instruments(self, UUID correlation_id) except *:
        """
        Request all instruments.

        Parameters
        ----------
        correlation_id : UUID
            The correlation identifier for the request.

        """
        Condition.not_none(correlation_id, "correlation_id")

        # TODO: Implement

    cpdef void request_quote_ticks(
        self,
        Symbol symbol,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID correlation_id,
    ) except *:
        """
        Request historical quote ticks for the given parameters.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol for the request.
        from_datetime : datetime, optional
            The specified from datetime for the data
        to_datetime : datetime, optional
            The specified to datetime for the data. If None then will default
            to the current datetime.
        limit : int
            The limit for the number of returned ticks.
        correlation_id : UUID
            The correlation identifier for the request.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_negative_int(limit, "limit")
        Condition.not_none(correlation_id, "correlation_id")

        # TODO: Implement

    cpdef void request_trade_ticks(
        self,
        Symbol symbol,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID correlation_id,
    ) except *:
        """
        Request historical trade ticks for the given parameters.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol for the request.
        from_datetime : datetime, optional
            The specified from datetime for the data
        to_datetime : datetime, optional
            The specified to datetime for the data. If None then will default
            to the current datetime.
        limit : int
            The limit for the number of returned ticks.
        correlation_id : UUID
            The correlation identifier for the request.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_negative_int(limit, "limit")
        Condition.not_none(correlation_id, "correlation_id")

        # TODO: Implement

    cpdef void request_bars(
        self,
        BarType bar_type,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID correlation_id,
    ) except *:
        """
        Request historical bars for the given parameters.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the request.
        from_datetime : datetime, optional
            The specified from datetime for the data
        to_datetime : datetime, optional
            The specified to datetime for the data. If None then will default
            to the current datetime.
        limit : int
            The limit for the number of returned bars.
        correlation_id : UUID
            The correlation identifier for the request.

        """
        Condition.not_none(bar_type, "bar_type")
        Condition.not_negative_int(limit, "limit")
        Condition.not_none(correlation_id, "correlation_id")

        # TODO: Implement

    async def _startup_sequence(self):
        await self._load_instruments()
        await self._send_instruments()
        await self._log_instruments_updated()

    async def _load_instruments(self):
        self._instrument_provider.load_all()

    async def _send_instruments(self):
        instruments = self._instrument_provider.get_all()
        for instrument in instruments.values():
            self._handle_instrument(instrument)

    async def _log_instruments_updated(self):
        self._log.info(f"Updated {self._instrument_provider.count} instruments.")
