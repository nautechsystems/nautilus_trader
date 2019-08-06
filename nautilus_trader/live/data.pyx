# -------------------------------------------------------------------------------------------------
# <copyright file="data.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import zmq

from cpython.datetime cimport datetime
from typing import Callable
from zmq import Context

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.typed_collections cimport ObjectCache
from nautilus_trader.core.message cimport Response
from nautilus_trader.model.objects cimport Venue, Symbol, BarType, Instrument
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.guid cimport LiveGuidFactory
from nautilus_trader.common.logger cimport Logger, LiveLogger
from nautilus_trader.common.data cimport DataClient
from nautilus_trader.network.workers import RequestWorker, SubscriberWorker
from nautilus_trader.serialization.base cimport DataSerializer, InstrumentSerializer, RequestSerializer, ResponseSerializer
from nautilus_trader.serialization.data cimport BsonDataSerializer, BsonInstrumentSerializer
from nautilus_trader.serialization.constants cimport *
from nautilus_trader.serialization.common cimport parse_symbol, parse_tick, parse_bar_type, parse_bar, convert_datetime_to_string
from nautilus_trader.serialization.serializers cimport MsgPackRequestSerializer, MsgPackResponseSerializer
from nautilus_trader.network.requests cimport DataRequest
from nautilus_trader.network.responses cimport MessageRejected, QueryFailure, DataResponse
from nautilus_trader.trade.strategy cimport TradeStrategy
from nautilus_trader.serialization.common import parse_symbol, parse_bar_type


cdef class LiveDataClient(DataClient):
    """
    Provides a data client for live trading.
    """
    cdef object _zmq_context
    cdef object _tick_req_worker
    cdef object _tick_sub_worker
    cdef object _bar_req_worker
    cdef object _bar_sub_worker
    cdef object _inst_req_worker
    cdef object _inst_sub_worker
    cdef RequestSerializer _request_serializer
    cdef ResponseSerializer _response_serializer
    cdef DataSerializer _data_serializer
    cdef InstrumentSerializer _instrument_serializer
    cdef ObjectCache _cached_symbols
    cdef ObjectCache _cached_bar_types

    def __init__(self,
                 zmq_context: Context,
                 Venue venue,
                 str service_address='localhost',
                 int tick_req_port=55501,
                 int tick_sub_port=55502,
                 int bar_req_port=55503,
                 int bar_sub_port=55504,
                 int inst_req_port=55505,
                 int inst_sub_port=55506,
                 RequestSerializer request_serializer=MsgPackRequestSerializer(),
                 ResponseSerializer response_serializer=MsgPackResponseSerializer(),
                 DataSerializer data_serializer=BsonDataSerializer(),
                 InstrumentSerializer instrument_serializer=BsonInstrumentSerializer(),
                 Logger logger=LiveLogger()):
        """
        Initializes a new instance of the LiveDataClient class.

        :param zmq_context: The ZMQ context.
        :param service_address: The data service host IP address (default=127.0.0.1).
        :param tick_req_port: The data service port for tick requests (default=55501).
        :param tick_sub_port: The data service port for tick subscriptions (default=55502).
        :param bar_req_port: The data service port for bar requests (default=55503).
        :param bar_sub_port: The data service port for bar subscriptions (default=55504).
        :param inst_req_port: The data service port for instrument requests (default=55505).
        :param inst_sub_port: The data service port for instrument subscriptions (default=55506).
        :param request_serializer: The request serializer for the component.
        :param response_serializer: The response serializer for the component.
        :param data_serializer: The data serializer for the component.
        :param data_serializer: The instrument serializer for the component.
        :param logger: The logger for the component.
        :raises ValueError: If the service_address is not a valid string.
        :raises ValueError: If the tick_req_port is not in range [0, 65535]
        :raises ValueError: If the tick_sub_port is not in range [0, 65535]
        :raises ValueError: If the bar_req_port is not in range [0, 65535]
        :raises ValueError: If the bar_sub_port is not in range [0, 65535]
        :raises ValueError: If the inst_req_port is not in range [0, 65535]
        :raises ValueError: If the inst_sub_port is not in range [0, 65535]
        """
        Condition.valid_string(service_address, 'service_address')
        Condition.in_range(tick_req_port, 'tick_req_port', 0, 65535)
        Condition.in_range(tick_sub_port, 'tick_sub_port', 0, 65535)
        Condition.in_range(bar_req_port, 'bar_req_port', 0, 65535)
        Condition.in_range(bar_sub_port, 'bar_sub_port', 0, 65535)
        Condition.in_range(inst_req_port, 'inst_req_port', 0, 65535)
        Condition.in_range(inst_sub_port, 'inst_sub_port', 0, 65535)

        super().__init__(venue, LiveClock(), LiveGuidFactory(), logger)
        self._zmq_context = zmq_context

        self._tick_req_worker = RequestWorker(
            'DataClient.TickReqWorker',
            'NautilusData',
            service_address,
            tick_req_port,
            self._zmq_context,
            logger)

        self._bar_req_worker = RequestWorker(
            'DataClient.BarReqWorker',
            'NautilusData',
            service_address,
            bar_req_port,
            self._zmq_context,
            logger)

        self._inst_req_worker = RequestWorker(
            'DataClient.InstReqWorker',
            'NautilusData',
            service_address,
            inst_req_port,
            self._zmq_context,
            logger)

        self._tick_sub_worker = SubscriberWorker(
            "DataClient.TickSubWorker",
            'NautilusData',
            service_address,
            tick_sub_port,
            self._zmq_context,
            self._handle_tick_sub,
            logger)

        self._bar_sub_worker = SubscriberWorker(
            "DataClient.BarSubWorker",
            'NautilusData',
            service_address,
            bar_sub_port,
            self._zmq_context,
            self._handle_bar_sub,
            logger)

        self._inst_sub_worker = SubscriberWorker(
            "DataClient.InstSubWorker",
            'NautilusData',
            service_address,
            inst_sub_port,
            self._zmq_context,
            self._handle_inst_sub,
            logger)

        self._request_serializer = request_serializer
        self._response_serializer = response_serializer
        self._data_serializer = data_serializer
        self._instrument_serializer = instrument_serializer

        self._cached_symbols = ObjectCache(Symbol, parse_symbol)
        self._cached_bar_types = ObjectCache(BarType, parse_bar_type)

        self._log.info(f"ZMQ v{zmq.pyzmq_version()}.")

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
        self._tick_req_worker.disconnect()
        self._tick_sub_worker.disconnect()
        self._bar_req_worker.disconnect()
        self._bar_sub_worker.disconnect()
        self._inst_req_worker.disconnect()
        self._inst_sub_worker.disconnect()

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

    cpdef void register_strategy(self, TradeStrategy strategy):
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
        cdef dict query = {
            DATA_TYPE: "Tick[]",
            SYMBOL: symbol.value,
            FROM_DATETIME: convert_datetime_to_string(from_datetime),
            TO_DATETIME: convert_datetime_to_string(to_datetime),
        }

        self._log.info(f"Requesting {symbol} ticks from {from_datetime} to {to_datetime}...")

        cdef DataRequest request = DataRequest(query, self._guid_factory.generate(), self.time_now())
        cdef bytes request_bytes = self._request_serializer.serialize(request)
        cdef bytes response_bytes = self._tick_req_worker.send(request_bytes)
        cdef Response response = self._response_serializer.deserialize(response_bytes)

        if isinstance(response, (MessageRejected, QueryFailure)):
            self._log.error(response)
            return

        cdef dict data = self._data_serializer.deserialize(response.data)
        cdef Symbol received_symbol = self._cached_symbols.get(data[SYMBOL])
        assert(received_symbol == symbol)

        callback([parse_tick(received_symbol, values.decode(UTF8)) for values in data[DATA]])

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
        cdef dict query = {
            DATA_TYPE: "Bar[]",
            SYMBOL: bar_type.symbol.value,
            SPECIFICATION: str(bar_type.specification),
            FROM_DATETIME: convert_datetime_to_string(from_datetime),
            TO_DATETIME: convert_datetime_to_string(to_datetime),
        }

        self._log.info(f"Requesting {bar_type} bars from {from_datetime} to {to_datetime}...")

        cdef DataRequest request = DataRequest(query, self._guid_factory.generate(), self.time_now())
        cdef bytes request_bytes = self._request_serializer.serialize(request)
        cdef bytes response_bytes = self._bar_req_worker.send(request_bytes)
        cdef Response response = self._response_serializer.deserialize(response_bytes)

        if isinstance(response, (MessageRejected, QueryFailure)):
            self._log.error(response)
            return

        cdef dict data = self._data_serializer.deserialize(response.data)
        cdef BarType received_bar_type = self._cached_bar_types.get(data[SYMBOL] + '-' + data[SPECIFICATION])
        assert(received_bar_type == bar_type)

        callback(received_bar_type, [parse_bar(values.decode(UTF8)) for values in data[DATA]])

    cpdef void request_instrument(self, Symbol symbol, callback: Callable):
        """
        Request the instrument for the given symbol.

        :param symbol: The symbol to update.
        :param callback: The callback for the response.
        """
        cdef dict query = {
            DATA_TYPE: "Instrument",
            SYMBOL: symbol.value,
        }

        self._log.info(f"Requesting instrument for {symbol}...")

        cdef DataRequest request = DataRequest(query, self._guid_factory.generate(), self.time_now())
        cdef bytes request_bytes = self._request_serializer.serialize(request)
        cdef bytes response_bytes = self._inst_req_worker.send(request_bytes)
        cdef Response response = self._response_serializer.deserialize(response_bytes)

        if isinstance(response, (MessageRejected, QueryFailure)):
            self._log.error(response)
            return

        cdef dict data = self._data_serializer.deserialize(response.data)
        cdef Instrument instrument = self._instrument_serializer.deserialize(data[DATA][0])
        assert(instrument.symbol == symbol)

        callback(instrument)

    cpdef void request_instruments(self, callback: Callable):
        """
        Request all instrument for the data clients venue.
        """
        cdef dict query = {
            DATA_TYPE: "Instrument[]",
            VENUE: self.venue.value,
        }

        self._log.info(f"Requesting all instruments for the {self.venue} ...")

        cdef DataRequest request = DataRequest(query, self._guid_factory.generate(), self.time_now())
        cdef bytes request_bytes = self._request_serializer.serialize(request)
        cdef bytes response_bytes = self._inst_req_worker.send(request_bytes)
        cdef Response response = self._response_serializer.deserialize(response_bytes)

        if isinstance(response, (MessageRejected, QueryFailure)):
            self._log.error(response)
            return

        cdef dict data = self._data_serializer.deserialize(response.data)
        cdef list instruments = [self._instrument_serializer.deserialize(inst) for inst in data[DATA]]
        callback(instruments)

    cpdef void update_instruments(self):
        """
        Update all instruments for the data clients venue.
        """
        self.request_instruments(self.temp_handle_instruments)

    cpdef void temp_handle_instruments(self, list instruments):
        self._handle_instruments(instruments)

    cpdef void subscribe_ticks(self, Symbol symbol, handler: Callable):
        """
        Subscribe to live tick data for the given symbol and handler.

        :param symbol: The tick symbol to subscribe to.
        :param handler: The callable handler for subscription (if None will just call print).
        :raises ValueError: If the handler is not of type Callable.
        """
        Condition.type(handler, Callable, 'handler')

        self._add_tick_handler(symbol, handler)
        self._tick_sub_worker.subscribe(str(symbol))

    cpdef void unsubscribe_ticks(self, Symbol symbol, handler: Callable):
        """
        Unsubscribe from live tick data for the given symbol and handler.

        :param symbol: The tick symbol to unsubscribe from.
        :param handler: The callable handler which was subscribed.
        :raises ValueError: If the handler is not of type Callable.
        """
        Condition.type(handler, Callable, 'handler')

        self._tick_sub_worker.unsubscribe(str(symbol))
        self._remove_tick_handler(symbol, handler)

    cpdef void subscribe_bars(self, BarType bar_type, handler: Callable):
        """
        Subscribe to live bar data for the given bar type and handler.

        :param bar_type: The bar type to subscribe to.
        :param handler: The callable handler for subscription.
        :raises ValueError: If the handler is not of type Callable.
        """
        Condition.type(handler, Callable, 'handler')

        self._add_bar_handler(bar_type, handler)
        self._bar_sub_worker.subscribe(str(bar_type))

    cpdef void unsubscribe_bars(self, BarType bar_type, handler: Callable):
        """
        Unsubscribe from live bar data for the given symbol and handler.

        :param bar_type: The bar type to unsubscribe from.
        :param handler: The callable handler which was subscribed.
        :raises ValueError: If the handler is not of type Callable.
        """
        Condition.type(handler, Callable, 'handler')

        self._bar_sub_worker.unsubscribe(str(bar_type))
        self._remove_bar_handler(bar_type, handler)

    cpdef void subscribe_instrument(self, Symbol symbol, handler: Callable):
        """
        Subscribe to live instrument data updates for the given symbol and handler.

        :param symbol: The instrument symbol to subscribe to.
        :param handler: The callable handler for subscription.
        :raises ValueError: If the handler is not of type Callable.
        """
        Condition.type(handler, Callable, 'handler')

        self._add_instrument_handler(symbol, handler)
        self._inst_sub_worker.subscribe(symbol.value)

    cpdef void unsubscribe_instrument(self, Symbol symbol, handler: Callable):
        """
        Unsubscribe from live instrument data updates for the given symbol and handler.

        :param symbol: The instrument symbol to unsubscribe from.
        :param handler: The callable handler which was subscribed.
        :raises ValueError: If the handler is not of type Callable.
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
