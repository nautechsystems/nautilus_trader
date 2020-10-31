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

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.data.engine cimport DataEngine
from nautilus_trader.data.wrappers cimport BarData
from nautilus_trader.data.wrappers cimport BarDataBlock
from nautilus_trader.data.wrappers cimport InstrumentDataBlock
from nautilus_trader.data.wrappers cimport QuoteTickDataBlock
from nautilus_trader.data.wrappers cimport TradeTickDataBlock
from nautilus_trader.model.bar cimport Bar
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.identifiers cimport Venue
from nautilus_trader.model.instrument cimport Instrument
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick


# noinspection: Object has warned attribute
# noinspection PyUnresolvedReferences
cdef class DataClient:
    """
    The base class for all data clients.
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
        Initialize a new instance of the DataClient class.

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
        self._venue = venue
        self._is_connected = False

        self._log.info("Initialized.")

    def __repr__(self) -> str:
        return f"{type(self).__name__}({self._venue})"

    @property
    def venue(self):
        """
        The data clients venue.

        Returns
        -------
        Venue

        """
        return self._venue

    @property
    def is_connected(self):
        """
        If the data client is currently connected.

        Returns
        -------
        bool
            True if connected, else False.

        """
        return self._is_connected

# -- ABSTRACT METHODS ------------------------------------------------------------------------------

    cpdef void connect(self) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void disconnect(self) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void reset(self) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void dispose(self) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void request_quote_ticks(
            self,
            Symbol symbol,
            datetime from_datetime,
            datetime to_datetime,
            int limit,
            callback: callable,
    ) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void request_trade_ticks(
            self,
            Symbol symbol,
            datetime from_datetime,
            datetime to_datetime,
            int limit,
            callback: callable,
    ) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void request_bars(
            self,
            BarType bar_type,
            datetime from_datetime,
            datetime to_datetime,
            int limit,
            callback: callable,
    ) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void request_instrument(self, Symbol symbol, callback: callable) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void request_instruments(self, callback: callable) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void subscribe_quote_ticks(self, Symbol symbol) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void subscribe_trade_ticks(self, Symbol symbol) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void subscribe_bars(self, BarType bar_type) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void subscribe_instrument(self, Symbol symbol) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe_quote_ticks(self, Symbol symbol) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe_trade_ticks(self, Symbol symbol) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe_bars(self, BarType bar_type) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe_instrument(self, Symbol symbol) except *:
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

# -- HANDLERS --------------------------------------------------------------------------------------

    cpdef void handle_quote_tick(self, QuoteTick tick) except *:
        self._engine.process(tick)

    cpdef void handle_quote_ticks(self, list ticks) except *:
        cdef QuoteTickDataBlock data = QuoteTickDataBlock(ticks)
        self._engine.process(data)

    cpdef void handle_trade_tick(self, TradeTick tick) except *:
        self._engine.process(tick)

    cpdef void handle_trade_ticks(self, list ticks) except *:
        cdef TradeTickDataBlock data = TradeTickDataBlock(ticks)
        self._engine.process(data)

    cpdef void handle_bar(self, BarType bar_type, Bar bar) except *:
        cdef BarData data = BarData(bar_type, bar)
        self._engine.process(data)

    cpdef void handle_bars(self, BarType bar_type, list bars) except *:
        cdef BarDataBlock data = BarDataBlock(bar_type, bars)
        self._engine.process((bar_type, bars))

    cpdef void handle_instrument(self, Instrument instrument) except *:
        self._engine.process(instrument)

    cpdef void handle_instruments(self, list instruments) except *:
        cdef InstrumentDataBlock data = InstrumentDataBlock(instruments)
        self._engine.process(instruments)
