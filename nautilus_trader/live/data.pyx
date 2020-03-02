# -------------------------------------------------------------------------------------------------
# <copyright file="data.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import queue
import threading
import zmq
from cpython.datetime cimport date

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.cache cimport ObjectCache
from nautilus_trader.core.types cimport GUID
from nautilus_trader.model.c_enums.bar_structure cimport BarStructure
from nautilus_trader.model.identifiers cimport Symbol, Venue, TraderId
from nautilus_trader.model.objects cimport BarType
from nautilus_trader.common.data cimport DataClient
from nautilus_trader.network.node_clients cimport MessageClient, MessageSubscriber
from nautilus_trader.serialization.base cimport DictionarySerializer
from nautilus_trader.serialization.base cimport RequestSerializer, ResponseSerializer
from nautilus_trader.serialization.base cimport DataSerializer, InstrumentSerializer
from nautilus_trader.serialization.data cimport Utf8TickSerializer, Utf8BarSerializer
from nautilus_trader.serialization.data cimport BsonDataSerializer, BsonInstrumentSerializer
from nautilus_trader.serialization.constants cimport *
from nautilus_trader.serialization.serializers cimport MsgPackDictionarySerializer
from nautilus_trader.serialization.serializers cimport MsgPackRequestSerializer, MsgPackResponseSerializer
from nautilus_trader.live.clock cimport LiveClock
from nautilus_trader.live.guid cimport LiveGuidFactory
from nautilus_trader.live.logging cimport LiveLogger
from nautilus_trader.network.identifiers cimport ClientId
from nautilus_trader.network.messages cimport Response, MessageReceived, MessageRejected
from nautilus_trader.network.messages cimport DataRequest, DataResponse, QueryFailure
from nautilus_trader.network.compression cimport Compressor, CompressorBypass
from nautilus_trader.network.encryption cimport EncryptionSettings
from nautilus_trader.trading.strategy cimport TradingStrategy


cdef class LiveDataClient(DataClient):
    """
    Provides a data client for live trading.
    """

    def __init__(self,
                 TraderId trader_id,
                 str host not None,
                 int tick_server_port,
                 int tick_pub_port,
                 int bar_server_port,
                 int bar_pub_port,
                 int inst_server_port,
                 int inst_pub_port,
                 Compressor compressor not None=CompressorBypass(),
                 EncryptionSettings encryption not None=EncryptionSettings(),
                 DictionarySerializer header_serializer not None=MsgPackDictionarySerializer(),
                 RequestSerializer request_serializer not None=MsgPackRequestSerializer(),
                 ResponseSerializer response_serializer not None=MsgPackResponseSerializer(),
                 DataSerializer data_serializer not None=BsonDataSerializer(),
                 InstrumentSerializer instrument_serializer not None=BsonInstrumentSerializer(),
                 int tick_capacity=100,
                 LiveClock clock not None=LiveClock(),
                 LiveGuidFactory guid_factory not None=LiveGuidFactory(),
                 LiveLogger logger not None=LiveLogger()):
        """
        Initializes a new instance of the LiveDataClient class.

        :param trader_id: The trader identifier for the client.
        :param host: The server host.
        :param tick_server_port: The port for tick requests (default=55501).
        :param tick_pub_port: The port for tick subscriptions (default=55502).
        :param bar_server_port: The port for bar requests (default=55503).
        :param bar_pub_port: The port for bar subscriptions (default=55504).
        :param inst_server_port: The port for instrument requests (default=55505).
        :param inst_pub_port: The port for instrument subscriptions (default=55506).
        :param compressor: The messaging compressor.
        :param encryption: The messaging encryption configuration.
        :param header_serializer: The header serializer.
        :param request_serializer: The request serializer.
        :param response_serializer: The response serializer.
        :param data_serializer: The data serializer.
        :param instrument_serializer: The instrument serializer.
        :param tick_capacity: The length for the internal tick deques.
        :param clock: The clock for the component.
        :param guid_factory: The guid factory for the component.
        :param logger: The logger for the component.
        :raises ValueError: If the host is not a valid string.
        :raises ValueError: If the tick_req_port is not in range [0, 65535].
        :raises ValueError: If the tick_sub_port is not in range [0, 65535].
        :raises ValueError: If the bar_req_port is not in range [0, 65535].
        :raises ValueError: If the bar_sub_port is not in range [0, 65535].
        :raises ValueError: If the inst_req_port is not in range [0, 65535].
        :raises ValueError: If the inst_sub_port is not in range [0, 65535].
        """
        Condition.valid_string(host, 'host')
        Condition.valid_port(tick_server_port, 'tick_server_port')
        Condition.valid_port(tick_pub_port, 'tick_pub_port')
        Condition.valid_port(bar_server_port, 'bar_server_port')
        Condition.valid_port(bar_pub_port, 'bar_pub_port')
        Condition.valid_port(inst_server_port, 'inst_server_port')
        Condition.valid_port(inst_pub_port, 'inst_pub_port')
        Condition.positive_int(tick_capacity, 'tick_capacity')
        super().__init__(tick_capacity, clock, guid_factory, logger)

        self._response_queue = queue.Queue()
        self._response_thread = threading.Thread(target=self._pop_response, daemon=True)
        self._correlation_index = {}  # type: {GUID, callable}

        self.trader_id = trader_id
        self.client_id = ClientId(trader_id.value)
        self.last_request_id = None

        self._tick_client = MessageClient(
            self.client_id,
            host,
            tick_server_port,
            header_serializer,
            request_serializer,
            response_serializer,
            compressor,
            encryption,
            clock,
            guid_factory,
            logger)

        self._tick_client.register_handler(self._handle_response)

        self._bar_client = MessageClient(
            self.client_id,
            host,
            bar_server_port,
            header_serializer,
            request_serializer,
            response_serializer,
            compressor,
            encryption,
            clock,
            guid_factory,
            logger)

        self._bar_client.register_handler(self._handle_response)

        self._inst_client = MessageClient(
            self.client_id,
            host,
            inst_server_port,
            header_serializer,
            request_serializer,
            response_serializer,
            compressor,
            encryption,
            clock,
            guid_factory,
            logger)

        self._inst_client.register_handler(self._handle_response)

        self._tick_subscriber = MessageSubscriber(
            self.client_id,
            host,
            tick_pub_port,
            compressor,
            encryption,
            clock,
            guid_factory,
            logger)

        self._tick_subscriber.register_handler(self._handle_tick_msg)

        self._bar_subscriber = MessageSubscriber(
            self.client_id,
            host,
            bar_pub_port,
            compressor,
            encryption,
            clock,
            guid_factory,
            logger)

        self._bar_subscriber.register_handler(self._handle_bar_msg)

        self._inst_subscriber = MessageSubscriber(
            self.client_id,
            host,
            inst_pub_port,
            compressor,
            encryption,
            clock,
            guid_factory,
            logger)

        self._inst_subscriber.register_handler(self._handle_inst_msg)

        self._data_serializer = data_serializer
        self._instrument_serializer = instrument_serializer

        self._cached_symbols = ObjectCache(Symbol, Symbol.from_string)
        self._cached_bar_types = ObjectCache(BarType, BarType.from_string)

    cpdef void connect(self) except *:
        """
        Connect to the data service.
        """
        self._tick_client.connect()
        self._tick_subscriber.connect()
        self._bar_client.connect()
        self._bar_subscriber.connect()
        self._inst_client.connect()
        self._inst_subscriber.connect()

    cpdef void disconnect(self) except *:
        """
        Disconnect from the data service.
        """
        try:
            self._tick_client.disconnect()
            self._tick_subscriber.disconnect()
            self._bar_client.disconnect()
            self._bar_subscriber.disconnect()
            self._inst_client.disconnect()
            self._inst_subscriber.disconnect()
        except zmq.ZMQError as ex:
            self._log.exception(ex)

    cpdef void reset(self) except *:
        """
        Reset the class to its initial state.
        """
        self._cached_symbols.clear()
        self._cached_bar_types.clear()
        self._reset()

    cpdef void dispose(self) except *:
        """
        Disposes of the data client.
        """
        self._tick_client.dispose()
        self._tick_subscriber.dispose()
        self._bar_client.dispose()
        self._bar_subscriber.dispose()
        self._inst_client.dispose()
        self._inst_subscriber.dispose()

    cpdef void register_strategy(self, TradingStrategy strategy) except *:
        """
        Register the given trade strategy with the data client.

        :param strategy: The strategy to register.
        """
        Condition.not_none(strategy, 'strategy')

        strategy.register_data_client(self)

        self._log.info(f"Registered strategy {strategy}.")

    cpdef void request_ticks(
            self,
            Symbol symbol,
            date from_date,
            date to_date,
            int limit,
            callback: callable) except *:
        """
        Request ticks for the given symbol and query parameters.

        :param symbol: The symbol for the request.
        :param from_date: The from date for the request.
        :param to_date: The to date for the request.
        :param limit: The limit for the number of ticks in the response (default = no limit) (>= 0).
        :param callback: The callback for the response.
        :raises ValueError: If the limit is negative (< 0).
        :raises ValueError: If the callback is not of type callable.
        """
        Condition.not_none(symbol, 'symbol')
        Condition.not_none(from_date, 'from_datetime')
        Condition.not_none(to_date, 'to_datetime')
        Condition.not_negative_int(limit, 'limit')
        Condition.callable(callback, 'callback')

        cdef dict query = {
            DATA_TYPE: "Tick[]",
            SYMBOL: symbol.value,
            FROM_DATE: str(from_date),
            TO_DATE: str(to_date),
            LIMIT: str(limit)
        }

        cdef str limit_string = '' if limit == 0 else f'(limit={limit})'
        self._log.info(f"Requesting {symbol} ticks from {from_date} to {to_date} {limit_string}...")

        cdef GUID request_id = self._guid_factory.generate()
        self._set_callback(request_id, callback)

        cdef DataRequest request = DataRequest(query, request_id, self.time_now())
        self._tick_client.send_request(request)
        self.last_request_id = request_id

    cpdef void request_bars(
            self,
            BarType bar_type,
            date from_date,
            date to_date,
            int limit,
            callback: callable) except *:
        """
        Request bars for the given bar type and query parameters.

        :param bar_type: The bar type for the request.
        :param from_date: The from date for the request.
        :param to_date: The to date for the request.
        :param limit: The limit for the number of ticks in the response (default = no limit) (>= 0).
        :param callback: The callback for the response.
        :raises ValueError: If the limit is negative (< 0).
        :raises ValueError: If the callback is not of type Callable.
        """
        Condition.not_none(bar_type, 'bar_type')
        Condition.not_none(from_date, 'from_date')
        Condition.not_none(to_date, 'to_date')
        Condition.not_negative_int(limit, 'limit')
        Condition.callable(callback, 'callback')

        if bar_type.specification.structure == BarStructure.TICK:
            self._bulk_build_tick_bars(bar_type, from_date, to_date, limit, callback)
            return

        cdef dict query = {
            DATA_TYPE: "Bar[]",
            SYMBOL: bar_type.symbol.value,
            SPECIFICATION: bar_type.specification.to_string(),
            FROM_DATE: str(from_date),
            TO_DATE: str(to_date),
            LIMIT: str(limit),
        }

        cdef str limit_string = '' if limit == 0 else f'(limit={limit})'
        self._log.info(f"Requesting {bar_type} bars from {from_date} to {to_date} {limit_string}...")

        cdef GUID request_id = self._guid_factory.generate()
        self._set_callback(request_id, callback)

        cdef DataRequest request = DataRequest(query, request_id, self.time_now())
        self._bar_client.send_request(request)
        self.last_request_id = request_id

    cpdef void request_instrument(self, Symbol symbol, callback: callable) except *:
        """
        Request the instrument for the given symbol.

        :param symbol: The symbol to update.
        :param callback: The callback for the response.
        :raises ValueError: If the callback is not of type callable.
        """
        Condition.not_none(symbol, 'symbol')
        Condition.callable(callback, 'callback')

        cdef dict query = {
            DATA_TYPE: "Instrument[]",
            SYMBOL: symbol.value,
        }

        self._log.info(f"Requesting instrument for {symbol}...")

        cdef GUID request_id = self._guid_factory.generate()
        self._set_callback(request_id, callback)

        cdef DataRequest request = DataRequest(query, request_id, self.time_now())
        self._inst_client.send_request(request)
        self.last_request_id = request_id

    cpdef void request_instruments(self, Venue venue, callback: callable) except *:
        """
        Request all instrument for given venue.
        
        :param venue: The venue for the request.
        :param callback: The callback for the response.
        :raises ValueError: If the callback is not of type callable.
        """
        Condition.callable(callback, 'callback')

        cdef dict query = {
            DATA_TYPE: "Instrument[]",
            VENUE: venue.value,
        }

        self._log.info(f"Requesting all instruments for {venue}...")

        cdef GUID request_id = self._guid_factory.generate()
        self._set_callback(request_id, callback)

        cdef DataRequest request = DataRequest(query, request_id, self.time_now())
        self._inst_client.send_request(request)
        self.last_request_id = request_id

    cpdef void update_instruments(self, Venue venue) except *:
        """
        Update all instruments for the data clients venue.
        """
        self.request_instruments(venue, self._handle_instruments_py)

    cpdef void _handle_instruments_py(self, list instruments) except *:
        # Method provides a Python wrapper for the callback
        # Handle all instruments individually
        for instrument in instruments:
            self._handle_instrument(instrument)

    cpdef void subscribe_ticks(self, Symbol symbol, handler: callable) except *:
        """
        Subscribe to live tick data for the given symbol and handler.

        :param symbol: The tick symbol to subscribe to.
        :param handler: The callable handler for subscription (if None will just call print).
        :raises ValueError: If the handler is not of type callable.
        """
        Condition.not_none(symbol, 'symbol')
        Condition.callable(handler, 'handler')

        self._add_tick_handler(symbol, handler)
        self._tick_subscriber.subscribe(symbol.to_string())

    cpdef void subscribe_bars(self, BarType bar_type, handler: callable) except *:
        """
        Subscribe to live bar data for the given bar type and handler.

        :param bar_type: The bar type to subscribe to.
        :param handler: The callable handler for subscription.
        :raises ValueError: If the handler is not of type Callable.
        """
        Condition.not_none(bar_type, 'bar_type')
        Condition.callable(handler, 'handler')

        if bar_type.specification.structure == BarStructure.TICK:
            self._self_generate_bars(bar_type, handler)
        else:
            self._add_bar_handler(bar_type, handler)
            self._bar_subscriber.subscribe(bar_type.to_string())

    cpdef void subscribe_instrument(self, Symbol symbol, handler: callable) except *:
        """
        Subscribe to live instrument data updates for the given symbol and handler.

        :param symbol: The instrument symbol to subscribe to.
        :param handler: The callable handler for subscription.
        :raises ValueError: If the handler is not of type Callable.
        """
        Condition.not_none(symbol, 'symbol')
        Condition.callable(handler, 'handler')

        self._add_instrument_handler(symbol, handler)
        self._inst_subscriber.subscribe(symbol.value)

    cpdef void unsubscribe_ticks(self, Symbol symbol, handler: callable) except *:
        """
        Unsubscribe from live tick data for the given symbol and handler.

        :param symbol: The tick symbol to unsubscribe from.
        :param handler: The callable handler which was subscribed.
        :raises ValueError: If the handler is not of type Callable.
        """
        Condition.not_none(symbol, 'symbol')
        Condition.callable(handler, 'handler')

        self._tick_subscriber.unsubscribe(symbol.to_string())
        self._remove_tick_handler(symbol, handler)

    cpdef void unsubscribe_bars(self, BarType bar_type, handler: callable) except *:
        """
        Unsubscribe from live bar data for the given symbol and handler.

        :param bar_type: The bar type to unsubscribe from.
        :param handler: The callable handler which was subscribed.
        :raises ValueError: If the handler is not of type Callable.
        """
        Condition.not_none(bar_type, 'bar_type')
        Condition.callable(handler, 'handler')

        self._bar_subscriber.unsubscribe(bar_type.to_string())
        self._remove_bar_handler(bar_type, handler)

    cpdef void unsubscribe_instrument(self, Symbol symbol, handler: callable) except *:
        """
        Unsubscribe from live instrument data updates for the given symbol and handler.

        :param symbol: The instrument symbol to unsubscribe from.
        :param handler: The callable handler which was subscribed.
        :raises ValueError: If the handler is not of type Callable.
        """
        Condition.not_none(symbol, 'symbol')
        Condition.callable(handler, 'handler')

        self._inst_subscriber.unsubscribe(symbol.value)
        self._remove_instrument_handler(symbol, handler)

    cpdef void _set_callback(self, GUID request_id, handler: callable) except *:
        self._correlation_index[request_id] = handler

    cpdef void _pop_callback(self, GUID correlation_id, list data) except *:
        handler = self._correlation_index.pop(correlation_id, None)
        if handler is not None:
            handler(data)
        else:
            self._log.error(f"No callback found for correlation id {correlation_id.value}")

    cpdef void _handle_response(self, Response response) except *:
        if isinstance(response, MessageRejected):
            self._log.error(response.message)
        elif isinstance(response, MessageReceived):
            self._log.info(response)
        elif isinstance(response, QueryFailure):
            self._log.warning(response)
        elif isinstance(response, DataResponse):
            self._handle_data_response(response)
        else:
            self._log.error(f"Cannot handle {response}")

    cpdef void _handle_data_response(self, DataResponse response) except *:
        cdef dict data_package = self._data_serializer.deserialize(response.data)
        cdef str data_type = data_package[DATA_TYPE]
        cdef dict metadata
        cdef list data
        if data_type == TICK_ARRAY:
            metadata = data_package[METADATA]
            symbol = self._cached_symbols.get(metadata[SYMBOL])
            data = Utf8TickSerializer.deserialize_bytes_list(symbol, data_package[DATA])
        elif data_type == BAR_ARRAY:
            metadata = data_package[METADATA]
            received_bar_type = self._cached_bar_types.get(metadata[SYMBOL] + '-' + metadata[SPECIFICATION])
            data = Utf8BarSerializer.deserialize_bytes_list(data_package[DATA])
        elif data_type == INSTRUMENT_ARRAY:
            data = [self._instrument_serializer.deserialize(inst) for inst in data_package[DATA]]
        else:
            self._log.error(f"The received data type {data_type} is not recognized.")
            return

        self._pop_callback(response.correlation_id, data)

    cpdef void _put_response(self, Response response) except *:
        self._response_queue.put(response)

    cpdef void _pop_response(self) except *:
        self._log.debug("Starting message consumption loop...")

        while True:
            self._handle_response(self._response_queue.get())

    cpdef void _handle_tick_msg(self, str topic, bytes payload) except *:
        # Handle the given tick message published for the given topic
        self._handle_tick(Utf8TickSerializer.deserialize(self._cached_symbols.get(topic), payload))

    cpdef void _handle_bar_msg(self, str topic, bytes payload) except *:
        # Handle the given bar message published for the given topic
        self._handle_bar(self._cached_bar_types.get(topic), Utf8BarSerializer.deserialize(payload))

    cpdef void _handle_inst_msg(self, str topic, bytes payload) except *:
        # Handle the given instrument message published for the given topic
        self._handle_instrument(self._instrument_serializer.deserialize(payload))
