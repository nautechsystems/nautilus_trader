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

from nautilus_trader.common.clock cimport Clock
from nautilus_trader.common.logging cimport Logger
from nautilus_trader.common.logging cimport LoggerAdapter
from nautilus_trader.common.uuid cimport UUIDFactory
from nautilus_trader.core.constants cimport *  # str constants only
from nautilus_trader.core.uuid cimport UUID
from nautilus_trader.data.engine cimport DataEngine
from nautilus_trader.data.messages cimport DataResponse
from nautilus_trader.model.bar cimport BarType
from nautilus_trader.model.c_enums.orderbook_level cimport OrderBookLevel
from nautilus_trader.model.data cimport Data
from nautilus_trader.model.identifiers cimport ClientId
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.instruments.base cimport Instrument
from nautilus_trader.model.tick cimport QuoteTick
from nautilus_trader.model.tick cimport TradeTick


cdef class DataClient:
    """
    The abstract base class for all data clients.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        ClientId client_id not None,
        DataEngine engine not None,
        Clock clock not None,
        Logger logger not None,
        dict config=None,
    ):
        """
        Initialize a new instance of the ``DataClient`` class.

        Parameters
        ----------
        client_id : ClientId
            The data client identifier.
        engine : DataEngine
            The data engine to connect to the client.
        clock : Clock
            The clock for the component.
        logger : Logger
            The logger for the component.
        config : dict[str, object], optional
            The configuration options.

        Raises
        ------
        ValueError
            If name is not a valid string.

        """
        if config is None:
            config = {}

        self._clock = clock
        self._uuid_factory = UUIDFactory()
        self._log = LoggerAdapter(
            component=config.get("name", f"DataClient-{client_id.value}"),
            logger=logger,
        )
        self._engine = engine
        self._config = config

        self.id = client_id
        self.is_connected = False

        self._log.info("Initialized.")

    def __repr__(self) -> str:
        return f"{type(self).__name__}-{self.id.value}"

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

    cpdef void subscribe(self, DataType data_type) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe(self, DataType data_type) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

# -- REQUESTS --------------------------------------------------------------------------------------

    cpdef void request(self, DataType data_type, UUID correlation_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

# -- PYTHON WRAPPERS -------------------------------------------------------------------------------

    def _handle_data_py(self, Data data):
        self._handle_data(data)

    def _handle_data_response_py(self, DataType data_type, Data data, UUID correlation_id):
        self._handle_data_response(data_type, data, correlation_id)

# -- DATA HANDLERS ---------------------------------------------------------------------------------

    cdef void _handle_data(self, Data data) except *:
        self._engine.process(data)

    cdef void _handle_data_response(self, DataType data_type, Data data, UUID correlation_id) except *:
        cdef DataResponse response = DataResponse(
            client_id=self.id,
            data_type=data_type,
            data=data,
            correlation_id=correlation_id,
            response_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
        )

        self._engine.receive(response)


cdef class MarketDataClient(DataClient):
    """
    The abstract base class for all market data clients.

    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(
        self,
        ClientId client_id not None,
        DataEngine engine not None,
        Clock clock not None,
        Logger logger not None,
        dict config=None,
    ):
        """
        Initialize a new instance of the ``MarketDataClient`` class.

        Parameters
        ----------
        client_id : ClientId
            The data client identifier (normally the venue).
        engine : DataEngine
            The data engine to connect to the client.
        clock : Clock
            The clock for the component.
        logger : Logger
            The logger for the component.
        config : dict[str, object], optional
            The configuration options.

        """
        super().__init__(
            client_id=client_id,
            engine=engine,
            clock=clock,
            logger=logger,
            config=config,
        )

    cpdef list unavailable_methods(self):
        """
        Return a list of unavailable methods for this data client.

        Returns
        -------
        list[str]
            The names of the unavailable methods.

        """
        return self._config.get("unavailable_methods", []).copy()

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

    cpdef void subscribe(self, DataType data_type) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe(self, DataType data_type) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void subscribe_instrument(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void subscribe_order_book(self, InstrumentId instrument_id, OrderBookLevel level, int depth=0, dict kwargs=None) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void subscribe_order_book_deltas(self, InstrumentId instrument_id, OrderBookLevel level, dict kwargs=None) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void subscribe_quote_ticks(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void subscribe_trade_ticks(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void subscribe_bars(self, BarType bar_type) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe_instrument(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe_order_book(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe_order_book_deltas(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe_quote_ticks(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe_trade_ticks(self, InstrumentId instrument_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void unsubscribe_bars(self, BarType bar_type) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

# -- REQUESTS --------------------------------------------------------------------------------------

    cpdef void request(self, DataType datatype, UUID correlation_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void request_instrument(self, InstrumentId instrument_id, UUID correlation_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void request_instruments(self, UUID correlation_id) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void request_quote_ticks(
        self,
        InstrumentId instrument_id,
        datetime from_datetime,
        datetime to_datetime,
        int limit,
        UUID correlation_id,
    ) except *:
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef void request_trade_ticks(
        self,
        InstrumentId instrument_id,
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

    # Convenient pure Python wrappers for the data handlers. Often Python methods
    # involving threads or the event loop don't work with cpdef methods.
    def _handle_instruments_py(self, list instruments, UUID correlation_id):
        self._handle_instruments(instruments, correlation_id)

    def _handle_quote_ticks_py(self, InstrumentId instrument_id, list ticks, UUID correlation_id):
        self._handle_quote_ticks(instrument_id, ticks, correlation_id)

    def _handle_trade_ticks_py(self, InstrumentId instrument_id, list ticks, UUID correlation_id):
        self._handle_trade_ticks(instrument_id, ticks, correlation_id)

    def _handle_bars_py(self, BarType bar_type, list bars, Bar partial, UUID correlation_id):
        self._handle_bars(bar_type, bars, partial, correlation_id)

# -- DATA HANDLERS ---------------------------------------------------------------------------------

    cdef void _handle_instruments(self, list instruments, UUID correlation_id) except *:
        cdef DataResponse response = DataResponse(
            client_id=self.id,
            data_type=DataType(Instrument),
            data=instruments,
            correlation_id=correlation_id,
            response_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
        )

        self._engine.receive(response)

    cdef void _handle_quote_ticks(self, InstrumentId instrument_id, list ticks, UUID correlation_id) except *:
        cdef DataResponse response = DataResponse(
            client_id=self.id,
            data_type=DataType(QuoteTick, metadata={INSTRUMENT_ID: instrument_id}),
            data=ticks,
            correlation_id=correlation_id,
            response_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
        )

        self._engine.receive(response)

    cdef void _handle_trade_ticks(self, InstrumentId instrument_id, list ticks, UUID correlation_id) except *:
        cdef DataResponse response = DataResponse(
            client_id=self.id,
            data_type=DataType(TradeTick, metadata={INSTRUMENT_ID: instrument_id}),
            data=ticks,
            correlation_id=correlation_id,
            response_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
        )

        self._engine.receive(response)

    cdef void _handle_bars(self, BarType bar_type, list bars, Bar partial, UUID correlation_id) except *:
        cdef DataResponse response = DataResponse(
            client_id=self.id,
            data_type=DataType(Bar, metadata={BAR_TYPE: bar_type, "Partial": partial}),
            data=bars,
            correlation_id=correlation_id,
            response_id=self._uuid_factory.generate(),
            timestamp_ns=self._clock.timestamp_ns(),
        )

        self._engine.receive(response)
