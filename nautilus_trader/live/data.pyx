# -------------------------------------------------------------------------------------------------
# <copyright file="data.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime
from typing import Callable
from zmq import Context, ZMQError

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.cache cimport ObjectCache
from nautilus_trader.core.message cimport Response
from nautilus_trader.model.identifiers cimport Symbol, Venue
from nautilus_trader.model.objects cimport BarType, Instrument
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.guid cimport LiveGuidFactory
from nautilus_trader.live.logger cimport LiveLogger
from nautilus_trader.common.data cimport DataClient
from nautilus_trader.network.workers import RequestWorker, SubscriberWorker
from nautilus_trader.serialization.base cimport DataSerializer, InstrumentSerializer, RequestSerializer, ResponseSerializer
from nautilus_trader.serialization.data cimport BsonDataSerializer, BsonInstrumentSerializer
from nautilus_trader.serialization.constants cimport *
from nautilus_trader.serialization.common cimport parse_symbol, parse_tick, parse_bar_type, parse_bar, convert_datetime_to_string
from nautilus_trader.serialization.serializers cimport MsgPackRequestSerializer, MsgPackResponseSerializer
from nautilus_trader.network.requests cimport DataRequest
from nautilus_trader.network.responses cimport MessageRejected, QueryFailure
from nautilus_trader.trade.strategy cimport TradingStrategy
from nautilus_trader.serialization.common import parse_symbol, parse_bar_type


cdef class LiveDataClient(DataClient):
    """
    Provides a data client for live trading.
    """

    def __init__(self,
                 zmq_context: Context,
                 Venue venue,
                 str service_name='NautilusData',
                 str service_address='localhost',
                 int tick_rep_port=55501,
                 int tick_pub_port=55502,
                 int bar_rep_port=55503,
                 int bar_pub_port=55504,
                 int inst_rep_port=55505,
                 int inst_pub_port=55506,
                 RequestSerializer request_serializer=MsgPackRequestSerializer(),
                 ResponseSerializer response_serializer=MsgPackResponseSerializer(),
                 DataSerializer data_serializer=BsonDataSerializer(),
                 InstrumentSerializer instrument_serializer=BsonInstrumentSerializer(),
                 LiveClock clock=LiveClock(),
                 LiveGuidFactory guid_factory=LiveGuidFactory(),
                 LiveLogger logger=LiveLogger()):
        """
        Initializes a new instance of the LiveDataClient class.

        :param zmq_context: The ZMQ context.
        :param service_name: The name of the service.
        :param service_address: The data service host IP address (default=127.0.0.1).
        :param tick_rep_port: The data service port for tick responses (default=55501).
        :param tick_pub_port: The data service port for tick publications (default=55502).
        :param bar_rep_port: The data service port for bar responses (default=55503).
        :param bar_pub_port: The data service port for bar publications (default=55504).
        :param inst_rep_port: The data service port for instrument responses (default=55505).
        :param inst_pub_port: The data service port for instrument publications (default=55506).
        :param request_serializer: The request serializer for the component.
        :param response_serializer: The response serializer for the component.
        :param data_serializer: The data serializer for the component.
        :param data_serializer: The instrument serializer for the component.
        :param logger: The logger for the component.
        :raises ConditionFailed: If the service_address is not a valid string.
        :raises ConditionFailed: If the tick_req_port is not in range [0, 65535].
        :raises ConditionFailed: If the tick_sub_port is not in range [0, 65535].
        :raises ConditionFailed: If the bar_req_port is not in range [0, 65535].
        :raises ConditionFailed: If the bar_sub_port is not in range [0, 65535].
        :raises ConditionFailed: If the inst_req_port is not in range [0, 65535].
        :raises ConditionFailed: If the inst_sub_port is not in range [0, 65535].
        """
        Condition.valid_string(service_address, 'service_address')
        Condition.in_range(tick_rep_port, 'tick_rep_port', 0, 65535)
        Condition.in_range(tick_pub_port, 'tick_pub_port', 0, 65535)
        Condition.in_range(bar_rep_port, 'bar_rep_port', 0, 65535)
        Condition.in_range(bar_pub_port, 'bar_pub_port', 0, 65535)
        Condition.in_range(inst_rep_port, 'inst_rep_port', 0, 65535)
        Condition.in_range(inst_pub_port, 'inst_pub_port', 0, 65535)

        super().__init__(venue, clock, guid_factory, logger)
        self._zmq_context = zmq_context

        self._tick_req_worker = RequestWorker(
            f'{self.__class__.__name__}.TickReqWorker',
            f'{service_name}.TickProvider',
            service_address,
            tick_rep_port,
            self._zmq_context,
            logger)

        self._bar_req_worker = RequestWorker(
            f'{self.__class__.__name__}.BarReqWorker',
            f'{service_name}.BarProvider',
            service_address,
            bar_rep_port,
            self._zmq_context,
            logger)

        self._inst_req_worker = RequestWorker(
            f'{self.__class__.__name__}.InstReqWorker',
            f'{service_name}.InstrumentProvider',
            service_address,
            inst_rep_port,
            self._zmq_context,
            logger)

        self._tick_sub_worker = SubscriberWorker(
            f'{self.__class__.__name__}.TickSubWorker',
            f'{service_name}.TickPublisher',
            service_address,
            tick_pub_port,
            self._zmq_context,
            self._handle_tick_sub,
            logger)

        self._bar_sub_worker = SubscriberWorker(
            f'{self.__class__.__name__}.BarSubWorker',
            f'{service_name}.BarPublisher',
            service_address,
            bar_pub_port,
            self._zmq_context,
            self._handle_bar_sub,
            logger)

        self._inst_sub_worker = SubscriberWorker(
            f'{self.__class__.__name__}.InstSubWorker',
            f'{service_name}.InstrumentPublisher',
            service_address,
            inst_pub_port,
            self._zmq_context,
            self._handle_inst_sub,
            logger)

        self._request_serializer = request_serializer
        self._response_serializer = response_serializer
        self._data_serializer = data_serializer
        self._instrument_serializer = instrument_serializer

        self._cached_symbols = ObjectCache(Symbol, parse_symbol)
        self._cached_bar_types = ObjectCache(BarType, parse_bar_type)

    cpdef void connect(self):
        """
        Connect to the data service.
        """
        self._tick_req_worker.connect()
        self._tick_sub_worker.connect()
        self._bar_req_worker.connect()
        self._bar_sub_worker.connect()
        self._inst_req_worker.connect()
        self._inst_sub_worker.connect()

    cpdef void disconnect(self):
        """
        Disconnect from the data service.
        """
        try:
            self._tick_req_worker.disconnect()
            self._tick_sub_worker.disconnect()
            self._bar_req_worker.disconnect()
            self._bar_sub_worker.disconnect()
            self._inst_req_worker.disconnect()
            self._inst_sub_worker.disconnect()
        except ZMQError as ex:
            self._log.exception(ex)

    cpdef void reset(self):
        """
        Resets the data client by clearing all stateful internal values and
        returning it to a fresh state.
        """
        self._cached_symbols.clear()
        self._cached_bar_types.clear()
        self._reset()

    cpdef void dispose(self):
        """
        Disposes of the data client.
        """
        self._tick_req_worker.dispose()
        self._tick_sub_worker.dispose()
        self._bar_req_worker.dispose()
        self._bar_sub_worker.dispose()
        self._inst_req_worker.dispose()
        self._inst_sub_worker.dispose()

    cpdef void register_strategy(self, TradingStrategy strategy):
        """
        Register the given trade strategy with the data client.

        :param strategy: The strategy to register.
        """
        strategy.register_data_client(self)

        self._log.info(f"Registered strategy {strategy} with the data client.")

    cpdef void request_ticks(
            self,
            Symbol symbol,
            datetime from_datetime,
            datetime to_datetime,
            callback: Callable):
        """
        Request ticks for the given symbol and query parameters.

        :param symbol: The symbol for the request.
        :param from_datetime: The from date time for the request.
        :param to_datetime: The to date time for the request.
        :param callback: The callback for the response.
        """
        Condition.type(callback, Callable, 'callback')

        cdef dict query = {
            DATA_TYPE: "Tick[]",
            SYMBOL: symbol.value,
            FROM_DATETIME: convert_datetime_to_string(from_datetime),
            TO_DATETIME: convert_datetime_to_string(to_datetime),
        }

        self._log.info(f"Requesting {symbol} ticks from {from_datetime} to {to_datetime} ...")

        cdef DataRequest request = DataRequest(query, self._guid_factory.generate(), self.time_now())
        cdef bytes request_bytes = self._request_serializer.serialize(request)
        cdef bytes response_bytes = self._tick_req_worker.send(request_bytes)
        cdef Response response = self._response_serializer.deserialize(response_bytes)

        if isinstance(response, (MessageRejected, QueryFailure)):
            self._log.error(response.message)
            return

        cdef dict data = self._data_serializer.deserialize(response.data)
        cdef Symbol received_symbol = self._cached_symbols.get(data[SYMBOL])
        assert(received_symbol == symbol)

        callback([parse_tick(received_symbol, values) for values in data[DATA]])

    cpdef void request_bars(
            self,
            BarType bar_type,
            datetime from_datetime,
            datetime to_datetime,
            callback: Callable):
        """
        Request bars for the given bar type and query parameters.

        :param bar_type: The bar type for the request.
        :param from_datetime: The from date time for the request.
        :param to_datetime: The to date time for the request.
        :param callback: The callback for the response.
        """
        Condition.type(callback, Callable, 'callback')

        cdef dict query = {
            DATA_TYPE: "Bar[]",
            SYMBOL: bar_type.symbol.value,
            SPECIFICATION: str(bar_type.specification),
            FROM_DATETIME: convert_datetime_to_string(from_datetime),
            TO_DATETIME: convert_datetime_to_string(to_datetime),
        }

        self._log.info(f"Requesting {bar_type} bars from {from_datetime} to {to_datetime} ...")

        cdef DataRequest request = DataRequest(query, self._guid_factory.generate(), self.time_now())
        cdef bytes request_bytes = self._request_serializer.serialize(request)
        cdef bytes response_bytes = self._bar_req_worker.send(request_bytes)
        cdef Response response = self._response_serializer.deserialize(response_bytes)

        if isinstance(response, (MessageRejected, QueryFailure)):
            self._log.error(response.message)
            return

        cdef dict data = self._data_serializer.deserialize(response.data)
        cdef BarType received_bar_type = self._cached_bar_types.get(data[SYMBOL] + '-' + data[SPECIFICATION])
        assert(received_bar_type == bar_type)

        callback(received_bar_type, [parse_bar(values) for values in data[DATA]])

    cpdef void request_instrument(self, Symbol symbol, callback: Callable):
        """
        Request the instrument for the given symbol.

        :param symbol: The symbol to update.
        :param callback: The callback for the response.
        """
        Condition.type(callback, Callable, 'callback')

        cdef dict query = {
            DATA_TYPE: "Instrument",
            SYMBOL: symbol.value,
        }

        self._log.info(f"Requesting instrument for {symbol} ...")

        cdef DataRequest request = DataRequest(query, self._guid_factory.generate(), self.time_now())
        cdef bytes request_bytes = self._request_serializer.serialize(request)
        cdef bytes response_bytes = self._inst_req_worker.send(request_bytes)
        cdef Response response = self._response_serializer.deserialize(response_bytes)

        if isinstance(response, (MessageRejected, QueryFailure)):
            self._log.error(response.message)
            return

        cdef dict data = self._data_serializer.deserialize(response.data)
        cdef Instrument instrument = self._instrument_serializer.deserialize(data[DATA][0])
        assert(instrument.symbol == symbol)

        callback(instrument)

    cpdef void request_instruments(self, callback: Callable):
        """
        Request all instrument for the data clients venue.
        """
        Condition.type(callback, Callable, 'callback')

        cdef dict query = {
            DATA_TYPE: "Instrument[]",
            VENUE: self.venue.value,
        }

        self._log.info(f"Requesting all instruments for the {self.venue} venue ...")

        cdef DataRequest request = DataRequest(query, self._guid_factory.generate(), self.time_now())
        cdef bytes request_bytes = self._request_serializer.serialize(request)
        cdef bytes response_bytes = self._inst_req_worker.send(request_bytes)
        cdef Response response = self._response_serializer.deserialize(response_bytes)

        if isinstance(response, (MessageRejected, QueryFailure)):
            self._log.error(response.message)
            return

        cdef dict data = self._data_serializer.deserialize(response.data)
        cdef list instruments = [self._instrument_serializer.deserialize(inst) for inst in data[DATA]]
        callback(instruments)

    cpdef void update_instruments(self):
        """
        Update all instruments for the data clients venue.
        """
        self.request_instruments(self._handle_instruments_py)

    cpdef void _handle_instruments_py(self, list instruments):
        # Method provides a Python wrapper for the callback
        # Handle all instruments individually
        for instrument in instruments:
            self._handle_instrument(instrument)

    cpdef void subscribe_ticks(self, Symbol symbol, handler: Callable):
        """
        Subscribe to live tick data for the given symbol and handler.

        :param symbol: The tick symbol to subscribe to.
        :param handler: The callable handler for subscription (if None will just call print).
        :raises ConditionFailed: If the handler is not of type Callable.
        """
        Condition.type(handler, Callable, 'handler')

        self._add_tick_handler(symbol, handler)
        self._tick_sub_worker.subscribe(str(symbol))

    cpdef void subscribe_bars(self, BarType bar_type, handler: Callable):
        """
        Subscribe to live bar data for the given bar type and handler.

        :param bar_type: The bar type to subscribe to.
        :param handler: The callable handler for subscription.
        :raises ConditionFailed: If the handler is not of type Callable.
        """
        Condition.type(handler, Callable, 'handler')

        self._add_bar_handler(bar_type, handler)
        self._bar_sub_worker.subscribe(str(bar_type))

    cpdef void subscribe_instrument(self, Symbol symbol, handler: Callable):
        """
        Subscribe to live instrument data updates for the given symbol and handler.

        :param symbol: The instrument symbol to subscribe to.
        :param handler: The callable handler for subscription.
        :raises ConditionFailed: If the handler is not of type Callable.
        """
        Condition.type(handler, Callable, 'handler')

        self._add_instrument_handler(symbol, handler)
        self._inst_sub_worker.subscribe(symbol.value)

    cpdef void unsubscribe_ticks(self, Symbol symbol, handler: Callable):
        """
        Unsubscribe from live tick data for the given symbol and handler.

        :param symbol: The tick symbol to unsubscribe from.
        :param handler: The callable handler which was subscribed.
        :raises ConditionFailed: If the handler is not of type Callable.
        """
        Condition.type(handler, Callable, 'handler')

        self._tick_sub_worker.unsubscribe(str(symbol))
        self._remove_tick_handler(symbol, handler)

    cpdef void unsubscribe_bars(self, BarType bar_type, handler: Callable):
        """
        Unsubscribe from live bar data for the given symbol and handler.

        :param bar_type: The bar type to unsubscribe from.
        :param handler: The callable handler which was subscribed.
        :raises ConditionFailed: If the handler is not of type Callable.
        """
        Condition.type(handler, Callable, 'handler')

        self._bar_sub_worker.unsubscribe(str(bar_type))
        self._remove_bar_handler(bar_type, handler)

    cpdef void unsubscribe_instrument(self, Symbol symbol, handler: Callable):
        """
        Unsubscribe from live instrument data updates for the given symbol and handler.

        :param symbol: The instrument symbol to unsubscribe from.
        :param handler: The callable handler which was subscribed.
        :raises ConditionFailed: If the handler is not of type Callable.
        """
        Condition.type(handler, Callable, 'handler')

        self._inst_sub_worker.unsubscribe(symbol.value)
        self._remove_instrument_handler(symbol, handler)

    cpdef void _handle_tick_sub(self, str topic, bytes message):
        # Handle the given tick message published for the given topic
        self._handle_tick(parse_tick(self._cached_symbols.get(topic), message.decode(UTF8)))

    cpdef void _handle_bar_sub(self, str topic, bytes message):
        # Handle the given bar message published for the given topic
        self._handle_bar(self._cached_bar_types.get(topic), parse_bar(message.decode(UTF8)))

    cpdef void _handle_inst_sub(self, str topic, bytes message):
        # Handle the given instrument message published for the given topic
        self._handle_instrument(self._instrument_serializer.deserialize(message))
