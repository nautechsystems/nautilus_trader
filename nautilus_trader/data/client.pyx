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
The `DataClient` class is responsible for interfacing with a particular API
which may be presented directly by an exchange, or broker intermediary. It
could also be possible to write clients for specialized data provides as long
as all abstract methods are implemented.
"""

from cpython.datetime cimport datetime

from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.common.clock cimport Clock
from nautilus_trader.core.constants cimport *  # str constants only
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.messages cimport DataResponse
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.data.engine cimport DataEngine
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarData
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick


cdef class DataClient:
    """
    The abstract base class for all data clients.

    This class should not be used directly, but through its concrete subclasses.
    """

    def __init__(
        self,
        Venue venue not None,
        DataEngine engine not None,
        Clock clock not None,
        Logger logger not None,
        dict config=None,
    ):
        """
        Initialize a new instance of the `DataClient` class.

        venue : Venue
            The venue the client can provide data for.
        engine : DataEngine
            The data engine to connect to the client.
        clock : Clock
            The clock for the component.
        logger : Logger
            The logger for the component.
        config : dict[str, object], optional
            The configuration options.

        """
        if config is None:
            config = {}

        self._clock = clock
        self._uuid_factory = UUIDFactory()
        self._log = LoggerAdapter(config.get("name", f"DataClient-{venue.value}"), logger)
        self._engine = engine
        self._config = config

        self.venue = venue
        self.initialized = False

        self._log.info("Initialized.")

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self.venue})"

    cpdef list unavailable_methods(self):
        """
        Return a list of unavailable methods for this data client.

        Returns
        -------
        list[str]
            The names of the unavailable methods.

        """
        return self._config.get("unavailable_methods", []).copy()

    cpdef bint is_connected(self) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void connect(self) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void disconnect(self) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void reset(self) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void dispose(self) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

# -- SUBSCRIPTIONS ---------------------------------------------------------------------------------

    cpdef void subscribe_instrument(self, Symbol symbol) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void subscribe_quote_ticks(self, Symbol symbol) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void subscribe_trade_ticks(self, Symbol symbol) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void subscribe_bars(self, BarType bar_type) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe_instrument(self, Symbol symbol) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe_quote_ticks(self, Symbol symbol) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe_trade_ticks(self, Symbol symbol) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe_bars(self, BarType bar_type) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

# -- REQUESTS --------------------------------------------------------------------------------------

    cpdef void request_instrument(self, Symbol symbol, UUID correlation_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void request_instruments(self, UUID correlation_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void request_quote_ticks(
        self,
        Symbol symbol,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID correlation_id,
    ) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void request_trade_ticks(
        self,
        Symbol symbol,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID correlation_id,
    ) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void request_bars(
        self,
        BarType bar_type,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID correlation_id,
    ) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

# -- PYTHON WRAPPERS -------------------------------------------------------------------------------

    def _handle_instrument_py(self, Instrument instrument):
        self._engine.process(instrument)

    def _handle_quote_tick_py(self, QuoteTick tick):
        self._engine.process(tick)

    def _handle_trade_tick_py(self, TradeTick tick):
        self._engine.process(tick)

    def _handle_bar_py(self, BarType bar_type, Bar bar):
        self._engine.process(BarData(bar_type, bar))

    def _handle_instruments_py(self, list instruments, UUID correlation_id):
        self._handle_instruments(instruments, correlation_id)

    def _handle_quote_ticks_py(self, Symbol symbol, list ticks, UUID correlation_id):
        self._handle_quote_ticks(symbol, ticks, correlation_id)

    def _handle_trade_ticks_py(self, Symbol symbol, list ticks, UUID correlation_id):
        self._handle_trade_ticks(symbol, ticks, correlation_id)

    def _handle_bars_py(self, BarType bar_type, list bars, Bar partial, UUID correlation_id):
        self._handle_bars(bar_type, bars, partial, correlation_id)

# -- DATA HANDLERS ---------------------------------------------------------------------------------

    cdef void _handle_instrument(self, Instrument instrument) except *:
        self._engine.process(instrument)

    cdef void _handle_quote_tick(self, QuoteTick tick) except *:
        self._engine.process(tick)

    cdef void _handle_trade_tick(self, TradeTick tick) except *:
        self._engine.process(tick)

    cdef void _handle_bar(self, BarType bar_type, Bar bar) except *:
        self._engine.process(BarData(bar_type, bar))

    cdef void _handle_instruments(self, list instruments, UUID correlation_id) except *:
        cdef DataResponse response = DataResponse(
            venue=self.venue,
            data_type=Instrument,
            metadata={},
            data=instruments,
            correlation_id=correlation_id,
            response_id=self._uuid_factory.generate(),
            response_timestamp=self._clock.utc_now(),
        )

        self._engine.receive(response)

    cdef void _handle_quote_ticks(self, Symbol symbol, list ticks, UUID correlation_id) except *:
        cdef DataResponse response = DataResponse(
            venue=self.venue,
            data_type=QuoteTick,
            metadata={SYMBOL: symbol},
            data=ticks,
            correlation_id=correlation_id,
            response_id=self._uuid_factory.generate(),
            response_timestamp=self._clock.utc_now(),
        )

        self._engine.receive(response)

    cdef void _handle_trade_ticks(self, Symbol symbol, list ticks, UUID correlation_id) except *:
        cdef DataResponse response = DataResponse(
            venue=self.venue,
            data_type=TradeTick,
            metadata={SYMBOL: symbol},
            data=ticks,
            correlation_id=correlation_id,
            response_id=self._uuid_factory.generate(),
            response_timestamp=self._clock.utc_now(),
        )

        self._engine.receive(response)

    cdef void _handle_bars(self, BarType bar_type, list bars, Bar partial, UUID correlation_id) except *:
        cdef DataResponse response = DataResponse(
            venue=self.venue,
            data_type=Bar,
            metadata={BAR_TYPE: bar_type, "Partial": partial},
            data=bars,
            correlation_id=correlation_id,
            response_id=self._uuid_factory.generate(),
            response_timestamp=self._clock.utc_now(),
        )

        self._engine.receive(response)
