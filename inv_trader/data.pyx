#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="data.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False

import re
import iso8601
import time

from cpython.datetime cimport datetime
from redis import StrictRedis, ConnectionError
from typing import List, Callable

from inv_trader.core.decimal cimport Decimal
from inv_trader.core.precondition cimport Precondition
from inv_trader.common.clock cimport Clock, LiveClock
from inv_trader.common.logger cimport Logger
from inv_trader.common.data cimport DataClient
from inv_trader.common.serialization cimport InstrumentSerializer
from inv_trader.model.enums import Resolution, QuoteType, Venue
from inv_trader.enums.resolution cimport Resolution
from inv_trader.enums.quote_type cimport QuoteType
from inv_trader.enums.venue cimport Venue
from inv_trader.model.objects cimport Symbol, Tick, BarType, Bar, Instrument
from inv_trader.strategy cimport TradeStrategy

cdef str UTF8 = 'utf-8'


cdef class LiveDataClient(DataClient):
    """
    Provides a live data client for alpha models and trading strategies.
    """
    cdef str _host
    cdef int _port
    cdef object _redis_client
    cdef object _pubsub
    cdef object _pubsub_thread
    cdef InstrumentSerializer _instrument_serializer

    def __init__(self,
                 str host='localhost',
                 int port=6379,
                 Clock clock=LiveClock(),
                 Logger logger=None):
        """
        Initializes a new instance of the DataClient class.

        :param host: The data service host IP address (default=127.0.0.1).
        :param port: The data service port (default=6379).
        :param clock: The internal clock for the component.
        :param logger: The logger for the component.
        :raises ValueError: If the host is not a valid string.
        :raises ValueError: If the port is not in range [0, 65535]
        """
        Precondition.valid_string(host, 'host')
        Precondition.in_range(port, 'port', 0, 65535)

        super().__init__(clock, logger)
        self._host = host
        self._port = port
        self._redis_client = None
        self._pubsub = None
        self._pubsub_thread = None
        self._instrument_serializer = InstrumentSerializer()

    cpdef bint is_connected(self):
        """
        :return: True if the client is connected, otherwise false.
        """
        if self._redis_client is None:
            return False

        try:
            self._redis_client.ping()
        except ConnectionError:
            return False

        return True

    @property
    def subscriptions_all(self) -> List[str]:
        """
        :return: All subscribed channels from the pub/sub server.
        """
        return [channel.decode(UTF8) for channel in self._redis_client.pubsub_channels()]

    @property
    def subscriptions_ticks(self) -> List[str]:
        """
        :return: The list of tick channels subscribed to.
        """
        return self._subscriptions_ticks

    @property
    def subscriptions_bars(self) -> List[str]:
        """
        :return: The list of bar channels subscribed to.
        """
        return self._subscriptions_bars

    cpdef void connect(self):
        """
        Connect to the data service, creating a pub/sub server.
        """
        self._redis_client = StrictRedis(host=self._host,
                                         port=self._port,
                                         db=0)
        self._pubsub = self._redis_client.pubsub()
        self._log.info(f"Connected to the live data service at {self._host}:{self._port}.")

    cpdef void disconnect(self):
        """
        Disconnect from the data service, unsubscribes from the pub/sub server
        and stops the pub/sub thread.
        """
        if self._pubsub is not None:
            self._pubsub.unsubscribe()

        if self._pubsub_thread is not None:
            self._pubsub_thread.stop()
            time.sleep(0.1)  # Allows thread to stop.
            self._log.debug(f"Stopped PubSub thread {self._pubsub_thread}.")

        self._log.info(f"Unsubscribed from tick data {self._subscriptions_ticks}.")
        self._log.info(f"Unsubscribed from bar data {self._subscriptions_bars}.")

        if self._redis_client is not None:
            self._redis_client.connection_pool.disconnect()
            self._log.info(f"Disconnected from the live data service at {self._host}:{self._port}.")
        else:
            self._log.info("Disconnected (the live data client was already disconnected).")

        self._redis_client = None
        self._pubsub = None
        self._pubsub_thread = None
        self._reset()

    cpdef void update_all_instruments(self):
        """
        Update all instruments from the live database.
        """
        cdef list keys = self._redis_client.keys('instruments*')
        cdef Instrument instrument

        for key in keys:
            instrument = self._instrument_serializer.deserialize(self._redis_client.get(key))
            self._instruments[instrument.symbol] = instrument
            self._log.info(f"Updated instrument for {instrument.symbol}.")

    cpdef void update_instrument(self, Symbol symbol):
        """
        Update the instrument corresponding to the given symbol (if found).
        Will log a warning is symbol is not found.

        :param symbol: The symbol to update.
        """
        cdef str key = f'instruments:{symbol.code.lower()}.{symbol.venue_string().lower()}'

        if key is None:
            self._log.warning(
                f"Cannot update instrument (symbol {symbol}not found in live database).")
            return

        cdef Instrument instrument = self._instrument_serializer.deserialize(self._redis_client.get(key))
        self._instruments[symbol] = instrument
        self._log.info(f"Updated instrument for {symbol}.")

    cpdef void register_strategy(self, TradeStrategy strategy):
        """
        Registers the given trade strategy with the data client.

        :param strategy: The strategy to register.
        :raise ValueError: If the strategy does not inherit from TradeStrategy.
        """
        strategy._register_data_client(self)

        self._log.info(f"Registered strategy {strategy} with the data client.")

    cpdef void historical_bars(
            self,
            BarType bar_type,
            int quantity,
            handler: Callable):
        """
        Download the historical bars for the given parameters from the data
        service, then pass them to the callable bar handler.

        Note: A log warnings are given if the downloaded bars quantity does not
        equal the requested quantity.

        :param bar_type: The historical bar type to download.
        :param quantity: The number of historical bars to download (can be None, will download all).
        :param handler: The bar handler to pass the bars to.
        :raises ValueError: If the quantity is not None and not positive (> 0).
        """
        Precondition.type(handler, Callable, 'handler')

        if quantity is not None:
            Precondition.positive(quantity, 'quantity')

        self._check_connection()

        cdef list keys = self._get_redis_bar_keys(bar_type)

        if len(keys) == 0:
            self._log.warning(
                "Cannot get historical bars (No bar keys found for the given parameters).")
            return

        cdef list bars = []
        if quantity is None:
            for key in keys:
                bar_list = self._redis_client.lrange(key, 0, -1)
                for bar_bytes in bar_list:
                    bars.append(self._parse_bar(bar_bytes.decode(UTF8)))
        else:
            for key in keys[::-1]:
                bar_list = self._redis_client.lrange(key, 0, -1)
                for bar_bytes in bar_list[::-1]:
                    bars.insert(0, self._parse_bar(bar_bytes.decode(UTF8)))
                if len(bars) >= quantity:
                    break

            bar_count = len(bars)
            if bar_count >= quantity:
                last_index = bar_count - quantity
                bars = bars[last_index:]
            else:
                self._log.warning(
                    f"Historical bars are < the requested amount ({len(bars)} vs {quantity}).")

        self._log.info(f"Historical download of {len(bars)} bars for {bar_type} complete.")

        for bar in bars:
            handler(bar_type, bar)
        self._log.debug(f"Historical bars hydrated to handler {handler}.")

    cpdef void historical_bars_from(
            self,
            BarType bar_type,
            datetime from_datetime,
            handler: Callable):
        """
        Download the historical bars for the given parameters from the data
        service, then pass them to the callable bar handler.

        Note: A log warning is given if the downloaded bars first timestamp is
        greater than the requested datetime.

        :param bar_type: The historical bar type to download.
        :param from_datetime: The datetime from which the historical bars should be downloaded.
        :param handler: The handler to pass the bars to.
        :raises ValueError: If the from_datetime is not less than that current datetime.
        """
        Precondition.type(handler, Callable, 'handler')
        Precondition.true(from_datetime < self._clock.time_now(), 'from_datetime < self._clock.time_now().')

        self._check_connection()

        cdef list keys = self._get_redis_bar_keys(bar_type)

        if len(keys) == 0:
            self._log.warning(
                "Cannot get historical bars (No bar keys found for the given parameters).")
            return

        cdef list bars = []
        for key in keys[::-1]:
            bar_list = self._redis_client.lrange(key, 0, -1)
            for bar_bytes in bar_list[::-1]:
                bar = self._parse_bar(bar_bytes.decode(UTF8))
                if bar.timestamp >= from_datetime:
                    bars.insert(0, self._parse_bar(bar_bytes.decode(UTF8)))
                else:
                    break  # Reached from_datetime.

        first_bar_timestamp = bars[0].timestamp
        if first_bar_timestamp > from_datetime:
            self._log.warning(
                (f"Historical bars first bar timestamp greater than requested from datetime "
                 f"({first_bar_timestamp.isoformat()} vs {from_datetime.isoformat()})."))

        self._log.info(f"Historical download of {len(bars)} bars for {bar_type} complete.")

        for bar in bars:
            handler(bar_type, bar)
        self._log.debug(f"Historical bars hydrated to handler {handler}.")

    cpdef void subscribe_bars(self, BarType bar_type, handler: Callable):
        """
        Subscribe to live bar data for the given bar parameters.

        :param bar_type: The bar type to subscribe to.
        :param handler: The callable handler for subscription (if None will just call print).
        """
        Precondition.type_or_none(handler, Callable, 'handler')

        self._check_connection()

        self._subscribe_bars(bar_type, handler)

        cdef str bars_channel = self._get_bar_channel_name(bar_type)
        if bars_channel not in self._subscriptions_bars:
            self._pubsub.subscribe(**{bars_channel: self._handle_bar})

            if self._pubsub_thread is None:
                self._pubsub_thread = self._pubsub.run_in_thread(0.001)
            self._subscriptions_bars.append(bars_channel)
            self._subscriptions_bars.sort()
            self._log.info(f"Subscribed to bar data for {bars_channel}.")

    cpdef void unsubscribe_bars(self, BarType bar_type, handler: Callable):
        """
        Unsubscribes from live bar data for the given symbol and venue.

        :param bar_type: The bar type to unsubscribe from.
        :param handler: The callable handler which was subscribed (can be None).
        """
        Precondition.type_or_none(handler, Callable, 'handler')

        self._check_connection()

        self._unsubscribe_bars(bar_type, handler)

        # If no further subscribers for this bar type.
        if len(self._bar_handlers[bar_type]) == 0:
            bar_channel = self._get_bar_channel_name(bar_type)
            self._pubsub.unsubscribe(bar_channel)

            if bar_channel in self._subscriptions_bars:
                self._subscriptions_bars.remove(bar_channel)
                self._subscriptions_bars.sort()
                self._log.info(f"Unsubscribed from bar data for {bar_channel}.")

    cpdef void subscribe_ticks(self, Symbol symbol, handler: Callable):
        """
        Subscribe to live tick data for the given symbol and venue.

        :param symbol: The tick symbol to subscribe to.
        :param handler: The callable handler for subscription (if None will just call print).
        """
        Precondition.type_or_none(handler, Callable, 'handler')

        self._check_connection()

        self._subscribe_ticks(symbol, handler)

        cdef str ticks_channel = self._get_tick_channel_name(symbol)
        if ticks_channel not in self._subscriptions_ticks:
            self._pubsub.subscribe(**{ticks_channel: self._handle_tick})

            if self._pubsub_thread is None:
                self._pubsub_thread = self._pubsub.run_in_thread(0.001)
            self._subscriptions_ticks.append(ticks_channel)
            self._subscriptions_ticks.sort()
            self._log.info(f"Subscribed to tick data for {ticks_channel}.")

    cpdef void unsubscribe_ticks(self, Symbol symbol, handler: Callable):
        """
        Unsubscribes from live tick data for the given symbol and venue.

        :param symbol: The tick symbol to unsubscribe from.
        :param handler: The callable handler which was subscribed (can be None).
        """
        Precondition.type_or_none(handler, Callable, 'handler')

        self._check_connection()

        self._unsubscribe_ticks(symbol, handler)

        # If no further subscribers for this bar type.
        if len(self._tick_handlers[symbol]) == 0:
            tick_channel = self._get_tick_channel_name(symbol)
            self._pubsub.unsubscribe(tick_channel)

            if tick_channel in self._subscriptions_ticks:
                self._subscriptions_ticks.remove(tick_channel)
                self._subscriptions_ticks.sort()

            self._log.info(f"Unsubscribed from tick data for {tick_channel}.")

    cdef list _get_redis_bar_keys(self, BarType bar_type):
        """
        Generate the bar key wildcard pattern and return the held Redis keys
        sorted.
        """
        return self._redis_client.keys(
            (f'bars'
             f':{bar_type.symbol.venue_string().lower()}'
             f':{bar_type.symbol.code.lower()}'
             f':{bar_type.resolution_string().lower()}'
             f':{bar_type.quote_type_string().lower()}*')).sort()

    cpdef object _parse_tick_symbol(self, str tick_channel):
        """
        Parse a Symbol object from the given UTF-8 string.

        :param tick_channel: The channel the tick was received on.
        :return: The parsed Symbol object.
        """
        cdef list split_channel = tick_channel.split('.')

        return Symbol(split_channel[0], Venue[str(split_channel[1].upper())])

    cpdef Tick _parse_tick(self, Symbol symbol, str tick_string):
        """
        Parse a Tick object from the given UTF-8 string.

        :param tick_string: The tick string.
        :return: The parsed Tick object.
        """
        cdef list split_tick = tick_string.split(',')

        return Tick(symbol,
                    Decimal(split_tick[0]),
                    Decimal(split_tick[1]),
                    iso8601.parse_date(split_tick[2]))

    cpdef BarType _parse_bar_type(self, str bar_type_string):
        """
        Parse a BarType object from the given UTF-8 string.

        :param bar_type_string: The bar type string to parse.
        :return: The parsed Bar object.
        """
        cdef list split_string = re.split(r'[.-]+', bar_type_string)
        cdef str resolution = split_string[3].split('[')[0]
        cdef str quote_type = split_string[3].split('[')[1].strip(']')

        return BarType(Symbol(split_string[0], Venue[split_string[1].upper()]),
                       int(split_string[2]),
                       Resolution[resolution.upper()],
                       QuoteType[quote_type.upper()])

    cpdef Bar _parse_bar(self, str bar_string):
        """
        Parse a Bar object from the given UTF-8 string.

        :param bar_string: The bar string to parse.
        :return: The parsed bar object.
        """
        cdef list split_bar = bar_string.split(',')

        return Bar(Decimal(split_bar[0]),
                   Decimal(split_bar[1]),
                   Decimal(split_bar[2]),
                   Decimal(split_bar[3]),
                   int(split_bar[4]),
                   iso8601.parse_date(split_bar[5]))

    cpdef str _get_tick_channel_name(self, Symbol symbol):
        """
        Return the tick channel name from the given parameters.
        """
        return str(f'{symbol.code.lower()}.{symbol.venue_string().lower()}')

    cpdef str _get_bar_channel_name(self, BarType bar_type):
        """
        Return the bar channel name from the given parameters.
        """
        return str(f'{bar_type.symbol.code.lower()}.'
                   f'{bar_type.symbol.venue_string().lower()}-'
                   f'{bar_type.period}-'
                   f'{bar_type.resolution_string().lower()}['
                   f'{bar_type.quote_type_string().lower()}]')

    cdef void _check_connection(self):
        """
        Check the connection with the live database.

        :raises ConnectionError if the client is None.
        :raises ConnectionError if the client is not connected.
        """
        if self._redis_client is None:
            raise ConnectionError(("No connection has been established to the live database "
                                   "(please connect first)."))
        if not self.is_connected():
            raise ConnectionError("No connection is established with the live database.")

    cpdef void _handle_tick(self, dict message):
        """"
        Handle the tick message by parsing to a Tick and sending to all subscribers.

        :param message: The tick message.
        """
        cdef Symbol symbol = self._parse_tick_symbol(message['channel'].decode(UTF8))
        cdef Tick tick = self._parse_tick(symbol, message['data'].decode(UTF8))

        if symbol not in self._tick_handlers:
            # If no tick handlers then print message to console.
            print(f"Received {tick}")
            return

        for handler in self._tick_handlers[symbol]:
            handler(tick)

    cpdef void _handle_bar(self, dict message):
        """"
        Handle the bar message by parsing to a Bar and sending to all subscribers.

        :param message: The bar message.
        """
        cdef BarType bar_type = self._parse_bar_type(message['channel'].decode(UTF8))
        cdef Bar bar = self._parse_bar(message['data'].decode(UTF8))

        if bar_type not in self._bar_handlers:
            # If no bar handlers then print message to console.
            print(f"Received {bar_type}, {bar}")
            return

        for handler in self._bar_handlers[bar_type]:
            handler(bar_type, bar)
