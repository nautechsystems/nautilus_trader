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
            UUIDFactory uuid_factory not None,
            Logger logger not None,
    ):
        """
        Initialize a new instance of the `DataClient` class.

        venue : Venue
            The venue the client can provide data for.
        engine : DataEngine
            The data engine to connect to the client.
        clock : Clock
            The clock for the component.
        uuid_factory : UUIDFactory
            The UUID factory for the component.
        logger : Logger
            The logger for the component.

        """
        self._clock = clock
        self._uuid_factory = uuid_factory
        self._log = LoggerAdapter(f"{type(self).__name__}-{venue.value}", logger)
        self._engine = engine

        self.venue = venue

        self._log.info("Initialized.")

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self.venue})"

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

# -- DATA HANDLERS ---------------------------------------------------------------------------------

    cpdef void _handle_instrument(self, Instrument instrument) except *:
        self._engine.process(instrument)

    cpdef void _handle_quote_tick(self, QuoteTick tick) except *:
        self._engine.process(tick)

    cpdef void _handle_trade_tick(self, TradeTick tick) except *:
        self._engine.process(tick)

    cpdef void _handle_bar(self, BarType bar_type, Bar bar) except *:
        self._engine.process(BarData(bar_type=bar_type, bar=bar))

    cpdef void _handle_instruments(self, list instruments, UUID correlation_id) except *:
        cdef DataResponse response = DataResponse(
            data_type=Instrument,
            metadata={VENUE: self.venue},
            data=instruments,
            correlation_id=correlation_id,
            response_id=self._uuid_factory.generate(),
            response_timestamp=self._clock.utc_now(),
        )

        self._engine.receive(response)

    cpdef void _handle_quote_ticks(self, Symbol symbol, list ticks, UUID correlation_id) except *:
        cdef DataResponse response = DataResponse(
            data_type=QuoteTick,
            metadata={SYMBOL: symbol},
            data=ticks,
            correlation_id=correlation_id,
            response_id=self._uuid_factory.generate(),
            response_timestamp=self._clock.utc_now(),
        )

        self._engine.receive(response)

    cpdef void _handle_trade_ticks(self, Symbol symbol, list ticks, UUID correlation_id) except *:
        cdef DataResponse response = DataResponse(
            data_type=TradeTick,
            metadata={SYMBOL: symbol},
            data=ticks,
            correlation_id=correlation_id,
            response_id=self._uuid_factory.generate(),
            response_timestamp=self._clock.utc_now(),
        )

        self._engine.receive(response)

    cpdef void _handle_bars(self, BarType bar_type, list bars, UUID correlation_id) except *:
        cdef DataResponse response = DataResponse(
            data_type=Bar,
            metadata={BAR_TYPE: bar_type},
            data=bars,
            correlation_id=correlation_id,
            response_id=self._uuid_factory.generate(),
            response_timestamp=self._clock.utc_now(),
        )

        self._engine.receive(response)
