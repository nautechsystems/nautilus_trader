# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
This module provides a data producer for backtesting.
"""

from cpython.datetime cimport datetime

from nautilus_trader.data.client cimport DataClient
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.data.engine cimport DataEngine
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument


cdef class BacktestDataClient(DataClient):
    """
    Provides an implementation of `DataClient` for backtesting.
    """

    def __init__(
        self,
        list instruments not None,
        Venue venue not None,
        DataEngine engine not None,
        Clock clock not None,
        Logger logger not None,
    ):
        """
        Initialize a new instance of the `BacktestDataProducer` class.

        instruments : list[Instrument]
            The instruments for the data client.
        venue : Venue
            The venue for the data client.
        engine : DataEngine
            The data engine to connect to the client.
        clock : Clock
            The clock for the component.
        logger : Logger
            The logger for the component.

        """
        super().__init__(
            venue,
            engine,
            clock,
            logger,
        )

        self._instruments = {}
        for instrument in instruments:
            Condition.equal(instrument.symbol.venue, self.venue, "instrument.symbol.venue", "self.venue")
            self._instruments[instrument.symbol] = instrument

        self._is_connected = False
        self.initialized = True

# -- COMMANDS --------------------------------------------------------------------------------------

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
        self._log.debug(f"Connecting...")

        self._is_connected = True

        self._log.info(f"Connected.")

    cpdef void disconnect(self) except *:
        """
        Disconnect the client.
        """
        self._log.debug(f"Disconnecting...")

        self._is_connected = False

        self._log.info(f"Disconnected.")

    cpdef void reset(self) except *:
        """
        Reset the data client.

        All stateful fields are reset to their initial value.
        """
        self._log.debug(f"Resetting...")

        self._log.info("Reset.")

    cpdef void dispose(self) except *:
        """
        Dispose of the data client.

        This method is idempotent and irreversible. No other methods should be
        called after disposal.
        """
        # Nothing to dispose
        self._log.info(f"Disposed.")

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

        if not self._is_connected:  # Simulate connection behaviour
            self._log.error(f"Cannot subscribe to instrument for {symbol} (not connected).")
            return

        # Do nothing else for backtest

    cpdef void subscribe_quote_ticks(self, Symbol symbol) except *:
        """
        Subscribe to `QuoteTick` data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol to subscribe to.

        """
        Condition.not_none(symbol, "symbol")

        if not self._is_connected:  # Simulate connection behaviour
            self._log.error(f"Cannot subscribe to quote ticks for {symbol} (not connected).")
            return

        # Do nothing else for backtest

    cpdef void subscribe_trade_ticks(self, Symbol symbol) except *:
        """
        Subscribe to `TradeTick` data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol to subscribe to.

        """
        Condition.not_none(symbol, "symbol")

        if not self._is_connected:  # Simulate connection behaviour
            self._log.error(f"Cannot subscribe to trade ticks for {symbol} (not connected).")
            return

        # Do nothing else for backtest

    cpdef void subscribe_bars(self, BarType bar_type) except *:
        """
        Subscribe to `Bar` data for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to subscribe to.

        """
        Condition.not_none(bar_type, "bar_type")

        if not self._is_connected:  # Simulate connection behaviour
            self._log.error(f"Cannot subscribe to bars for {bar_type} (not connected).")
            return

        self._log.error(f"Cannot subscribe to externally aggregated bars "
                        f"(backtesting only supports internal aggregation at this stage).")

    cpdef void unsubscribe_instrument(self, Symbol symbol) except *:
        """
        Unsubscribe from `Instrument` data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The instrument symbol to unsubscribe from.

        """
        Condition.not_none(symbol, "symbol")

        if not self._is_connected:  # Simulate connection behaviour
            self._log.error(f"Cannot unsubscribe from instrument for {symbol} (not connected).")
            return

        # Do nothing else for backtest

    cpdef void unsubscribe_quote_ticks(self, Symbol symbol) except *:
        """
        Unsubscribe from `QuoteTick` data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol to unsubscribe from.

        """
        Condition.not_none(symbol, "symbol")

        if not self._is_connected:  # Simulate connection behaviour
            self._log.error(f"Cannot unsubscribe from quote ticks for {symbol} (not connected).")
            return

        # Do nothing else for backtest

    cpdef void unsubscribe_trade_ticks(self, Symbol symbol) except *:
        """
        Unsubscribe from `TradeTick` data for the given symbol.

        Parameters
        ----------
        symbol : Symbol
            The tick symbol to unsubscribe from.

        """
        Condition.not_none(symbol, "symbol")

        if not self._is_connected:  # Simulate connection behaviour
            self._log.error(f"Cannot unsubscribe from trade ticks for {symbol} (not connected).")
            return

        # Do nothing else for backtest

    cpdef void unsubscribe_bars(self, BarType bar_type) except *:
        """
        Unsubscribe from `Bar` data for the given bar type.

        Parameters
        ----------
        bar_type : BarType
            The bar type to unsubscribe from.

        """
        Condition.not_none(bar_type, "bar_type")

        if not self._is_connected:  # Simulate connection behaviour
            self._log.error(f"Cannot unsubscribe from bars {bar_type} (not connected).")
            return

        self._log.error(f"Cannot unsubscribe from externally aggregated bars "
                        f"(backtesting only supports internal aggregation at this stage).")

        # Do nothing else for backtest

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

        if not self._is_connected:  # Simulate connection behaviour
            self._log.error(f"Cannot request instrument for {symbol} (not connected).")
            return

        cdef Instrument instrument = self._instruments.get(symbol)

        if instrument is None:
            self._log.warning(f"No instrument found for {symbol}.")
            return

        self._handle_instruments([instrument], correlation_id)

    cpdef void request_instruments(self, UUID correlation_id) except *:
        """
        Request all instruments.

        Parameters
        ----------
        correlation_id : UUID
            The correlation identifier for the request.

        """
        Condition.not_none(correlation_id, "correlation_id")

        self._handle_instruments(list(self._instruments.values()), correlation_id)

    cpdef void request_quote_ticks(
        self,
        Symbol symbol,
        datetime from_datetime,  # Can be None
        datetime to_datetime,    # Can be None
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
            The specified from datetime for the data.
        to_datetime : datetime, optional
            The specified to datetime for the data. If None then will default
            to the current datetime.
        limit : int
            The limit for the number of returned ticks.
        correlation_id : UUID
            The correlation identifier for the request.

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(correlation_id, "correlation_id")

        if not self._is_connected:  # Simulate connection behaviour
            self._log.error(f"Cannot request quote ticks for {symbol} (not connected).")
            return

        # Do nothing else for backtest

    cpdef void request_trade_ticks(
        self,
        Symbol symbol,
        datetime from_datetime,  # Can be None
        datetime to_datetime,    # Can be None
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
            The specified from datetime for the data.
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

        if not self._is_connected:  # Simulate connection behaviour
            self._log.error(f"Cannot request trade ticks for {symbol} (not connected).")
            return

        # Do nothing else for backtest

    cpdef void request_bars(
        self,
        BarType bar_type,
        datetime from_datetime,  # Can be None
        datetime to_datetime,    # Can be None
        int limit,
        UUID correlation_id,
    ) except *:
        """
        Request historical bars for the given parameters from the data engine.

        Parameters
        ----------
        bar_type : BarType
            The bar type for the request.
        from_datetime : datetime, optional
            The specified from datetime for the data.
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

        if not self._is_connected:  # Simulate connection behaviour
            self._log.error(f"Cannot request bars for {bar_type} (not connected).")
            return

        # Do nothing else for backtest
