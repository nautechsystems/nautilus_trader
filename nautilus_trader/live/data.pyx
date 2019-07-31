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

# Do not rearrange below enums imports (import vs cimport)
from nautilus_trader.core.precondition cimport Precondition
from nautilus_trader.core.message cimport Request
from nautilus_trader.model.enums import Resolution, QuoteType, Venue
from nautilus_trader.model.c_enums.resolution cimport Resolution
from nautilus_trader.model.c_enums.quote_type cimport QuoteType
from nautilus_trader.model.c_enums.venue cimport venue_string
from nautilus_trader.model.objects cimport Symbol, Price, Tick, BarSpecification, BarType, Bar, Instrument
from nautilus_trader.common.clock cimport LiveClock
from nautilus_trader.common.guid cimport LiveGuidFactory
from nautilus_trader.common.logger cimport Logger
from nautilus_trader.common.data cimport DataClient
from nautilus_trader.network.workers import RequestWorker, SubscriberWorker
from nautilus_trader.serialization.base cimport DataSerializer, InstrumentSerializer, RequestSerializer, ResponseSerializer
from nautilus_trader.serialization.data cimport BsonDataSerializer, BsonInstrumentSerializer
from nautilus_trader.serialization.common cimport parse_symbol, parse_symbol, parse_tick, parse_bar_type, parse_bar, convert_datetime_to_string
from nautilus_trader.serialization.message cimport MsgPackRequestSerializer, MsgPackResponseSerializer
from nautilus_trader.network.requests cimport DataRequest
from nautilus_trader.network.responses cimport DataResponse
from nautilus_trader.trade.strategy cimport TradeStrategy

cdef str UTF8 = 'utf-8'


cdef class LiveDataClient(DataClient):
    """
    Provides a data client for live trading.
    """
    cdef object _tick_req_worker
    cdef object _tick_sub_worker
    cdef object _bar_req_worker
    cdef object _bar_sub_worker
    cdef object _instrument_req_worker
    cdef object _instrument_sub_worker
    cdef RequestSerializer _request_serializer
    cdef ResponseSerializer _response_serializer
    cdef DataSerializer _data_serializer
    cdef InstrumentSerializer _instrument_serializer

    def __init__(self,
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
                 Logger logger=None):
        """
        Initializes a new instance of the DataClient class.

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
        :raises ValueError: If the host is not a valid string.
        :raises ValueError: If the port is not in range [0, 65535]
        """
        Precondition.valid_string(service_address, 'service_address')
        Precondition.in_range(tick_req_port, 'tick_req_port', 0, 65535)
        Precondition.in_range(tick_sub_port, 'tick_sub_port', 0, 65535)
        Precondition.in_range(bar_req_port, 'bar_req_port', 0, 65535)
        Precondition.in_range(bar_sub_port, 'bar_sub_port', 0, 65535)
        Precondition.in_range(inst_req_port, 'inst_req_port', 0, 65535)
        Precondition.in_range(inst_sub_port, 'inst_sub_port', 0, 65535)

        super().__init__(LiveClock(), LiveGuidFactory(), logger)
        self.zmq_context = zmq.Context()
        self._tick_req_worker = RequestWorker(
            'DataClient.TickReqWorker',
            self.zmq_context,
            service_address,
            tick_req_port,
            logger)
        self._bar_req_worker = RequestWorker(
            'DataClient.BarReqWorker',
            self.zmq_context,
            service_address,
            bar_req_port,
            logger)
        self._inst_req_worker = RequestWorker(
            'DataClient.InstReqWorker',
            self.zmq_context,
            service_address,
            inst_req_port,
            logger)
        self._tick_sub_worker = SubscriberWorker(
            "DataClient.TickSubWorker",
            self.zmq_context,
            service_address,
            tick_sub_port,
            self._handle_tick_sub,
            logger)
        self._bar_sub_worker = SubscriberWorker(
            "DataClient.BarSubWorker",
            self.zmq_context,
            service_address,
            bar_sub_port,
            self._handle_bar_sub,
            logger)
        self._inst_sub_worker = SubscriberWorker(
            "DataClient.InstSubWorker",
            self.zmq_context,
            service_address,
            inst_sub_port,
            self._handle_inst_sub,
            logger)
        self._request_serializer = request_serializer
        self._response_serializer = response_serializer
        self._data_serializer = data_serializer
        self._instrument_serializer = instrument_serializer

        self._log.info(f"ZMQ v{zmq.pyzmq_version()}.")

    cpdef void connect(self):
        """
        Connect to the data service, creating a pub/sub server.
        """
        self._tick_req_worker.start()
        self._tick_sub_worker.start()
        self._bar_req_worker.start()
        self._bar_sub_worker.start()
        self._instrument_req_worker.start()
        self._instrument_sub_worker.start()

    cpdef void disconnect(self):
        """
        Disconnect from the data service, unsubscribes from the pub/sub server
        and stops the pub/sub thread.
        """
        self._tick_req_worker.stop()
        self._tick_sub_worker.stop()
        self._bar_req_worker.stop()
        self._bar_sub_worker.stop()
        self._instrument_req_worker.stop()
        self._instrument_sub_worker.stop()

    cpdef void reset(self):
        """
        Resets the live data client by clearing all stateful internal values and
        returning it to a fresh state.
        """
        self._reset()

    cpdef void dispose(self):
        """
        Disposes of the live data client.
        """
        self.zmq_context.term()

    cpdef void register_strategy(self, TradeStrategy strategy):
        """
        Register the given trade strategy with the data client.

        :param strategy: The strategy to register.
        :raises ValueError: If the strategy does not inherit from TradeStrategy.
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
        Update the instrument corresponding to the given symbol (if found).
        Will log a warning is symbol is not found.

        :param symbol: The symbol for the request.
        :param from_datetime: The from date time for the request.
        :param to_datetime: The to date time for the request.
        :param callback: The callback for the response.
        """
        cdef dict query = {
            "DataType": "Tick[]",
            "Symbol": symbol.value,
            "FromDateTime": convert_datetime_to_string(from_datetime),
            "ToDateTime": convert_datetime_to_string(to_datetime),
        }

        cdef DataRequest request = DataRequest(
            query,
            self._guid_factory.generate(),
            self.time_now())

        self._tick_req_worker(
            self._request_serializer.serialize(request),
            self._handle_response,
            callback)

        self._log.info(f"Requested {symbol} ticks from {from_datetime} to {to_datetime}.")

    cpdef void request_bars(
            self,
            BarType bar_type,
            datetime from_datetime,
            datetime to_datetime,
            callback: Callable):
        """
        Update the instrument corresponding to the given symbol (if found).
        Will log a warning is symbol is not found.

        :param bar_type: The bar type for the request.
        :param from_datetime: The from date time for the request.
        :param to_datetime: The to date time for the request.
        :param callback: The callback for the response.
        """
        cdef dict query = {
            "DataType": "Bar[]",
            "Symbol": bar_type.symbol.value,
            "Specification": str(bar_type.specification),
            "FromDateTime": convert_datetime_to_string(from_datetime),
            "ToDateTime": convert_datetime_to_string(to_datetime),
        }

        cdef DataRequest request = DataRequest(
            query,
            self._guid_factory.generate(),
            self.time_now())

        self._bar_req_worker(
            self._request_serializer.serialize(request),
            self._handle_response,
            callback)

        self._log.info(f"Requested {bar_type} bars from {from_datetime} to {to_datetime}.")

    cpdef void request_instrument(self, Symbol symbol, callback: Callable):
        """
        Update the instrument corresponding to the given symbol (if found).
        Will log a warning is symbol is not found.

        :param symbol: The symbol to update.
        :param callback: The callback for the response.
        """
        cdef dict query = {
            "DataType": "Instrument",
            "Symbol": symbol.value,
        }

        cdef DataRequest request = Request(
            query,
            self._guid_factory.generate(),
            self.time_now())

        self._inst_req_worker(
            self._request_serializer.serialize(request),
            self._handle_response,
            callback)

        self._log.info(f"Requested instrument for {symbol}.")

    cpdef void request_instruments(self, callback: Callable):
        """
        Update all instruments from the live database.
        """
        cdef dict query = {
            "DataType": "Instrument[]",
            "Venue": venue_string(self.venue),
        }

        cdef DataRequest request = DataRequest(
            query,
            self._guid_factory.generate(),
            self.time_now())

        self._inst_req_worker.send(
            self._request_serializer.serialize(request),
            self._handle_response,
            callback)

        self._log.info(f"Requested all instruments for the {self.venue} venue.")

    cpdef void update_instruments(self):
        """
        Update all instruments for the data clients venue.
        """
        self.request_instruments(self._handle_instruments_response)

    cpdef void subscribe_ticks(self, Symbol symbol, handler: Callable):
        """
        Subscribe to live tick data for the given symbol and handler.

        :param symbol: The tick symbol to subscribe to.
        :param handler: The callable handler for subscription (if None will just call print).
        :raises ValueError: If the handler is not of type Callable.
        """
        Precondition.type(handler, Callable, 'handler')

        self._add_tick_handler(symbol, handler)
        self._tick_sub_worker.subscribe(str(symbol))

    cpdef void unsubscribe_ticks(self, Symbol symbol, handler: Callable):
        """
        Unsubscribe from live tick data for the given symbol and handler.

        :param symbol: The tick symbol to unsubscribe from.
        :param handler: The callable handler which was subscribed.
        :raises ValueError: If the handler is not of type Callable.
        """
        Precondition.type(handler, Callable, 'handler')

        self._tick_sub_worker.unsubscribe(str(symbol))
        self._remove_tick_handler(symbol, handler)

    cpdef void subscribe_bars(self, BarType bar_type, handler: Callable):
        """
        Subscribe to live bar data for the given bar type and handler.

        :param bar_type: The bar type to subscribe to.
        :param handler: The callable handler for subscription.
        :raises ValueError: If the handler is not of type Callable.
        """
        Precondition.type(handler, Callable, 'handler')

        self._add_bar_handler(bar_type, handler)
        self._bar_sub_worker.subscribe(str(bar_type))

    cpdef void unsubscribe_bars(self, BarType bar_type, handler: Callable):
        """
        Unsubscribe from live bar data for the given symbol and handler.

        :param bar_type: The bar type to unsubscribe from.
        :param handler: The callable handler which was subscribed.
        :raises ValueError: If the handler is not of type Callable.
        """
        Precondition.type(handler, Callable, 'handler')

        self._bar_sub_worker.unsubscribe(str(bar_type))
        self._remove_bar_handler(bar_type, handler)

    cpdef void subscribe_instrument(self, Symbol symbol, handler: Callable):
        """
        Subscribe to the instrument for the given symbol and handler.

        :param symbol: The instrument symbol to subscribe to.
        :param handler: The callable handler for subscription.
        :raises ValueError: If the handler is not of type Callable.
        """
        Precondition.type(handler, Callable, 'handler')

        self._add_instrument_handler(symbol, handler)
        self._inst_sub_worker.subscribe(symbol.value)

    cpdef void unsubscribe_instrument(self, Symbol symbol, handler: Callable):
        """
        Unsubscribe from the instrument for the given symbol.

        :param symbol: The instrument symbol to unsubscribe from.
        :param handler: The callable handler which was subscribed.
        :raises ValueError: If the handler is not of type Callable.
        """
        Precondition.type(handler, Callable, 'handler')

        self._inst_sub_worker.unsubscribe(symbol.value)
        self._remove_instrument_handler(symbol, handler)

    cpdef void _handle_response(self, bytes message, callback: Callable):
        """
        Handle the given tick response message and send to the give callback.
        
        :param message: The response message bytes to handle.
        :param callback: The callback to send the deserialized response to.
        """
        callback(self._response_serializer.deserialize(message))

    cpdef void _handle_instruments_response(self, DataResponse response):
        """
        Handle the instruments data response by deserializing all instrument data.
        """
        for inst_bson in self._data_serializer.deserialize(response.data)['Values']:
            self._handle_instrument(self._instrument_serializer.deserialize(inst_bson))

    cpdef void _handle_tick_sub(self, str topic, bytes message):
        """
        Handle the given tick message published for the given topic.
        
        :param message: The published message to handle.
        """
        self._handle_tick(parse_tick(parse_symbol(topic), message.decode(UTF8)))

    cpdef void _handle_bar_sub(self, str topic, bytes message):
        """
        Handle the given bar message published for the given topic.
        
        :param message: The published message to handle.
        """
        self._handle_bar(parse_bar_type(topic), parse_bar(message.decode(UTF8)))

    cpdef void _handle_inst_sub(self, str topic, bytes message):
        """
        Handle the given instrument message published for the given topic.
        
        :param message: The published message to handle.
        """
        self._handle_instrument(self._instrument_serializer.deserialize(message))
