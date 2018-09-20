#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="data.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import re
import iso8601
import time

from datetime import datetime, timezone
from decimal import Decimal
from redis import StrictRedis, ConnectionError
from typing import List, Dict, Callable, KeysView

from inv_trader.core.precondition import Precondition
from inv_trader.core.logger import Logger, LoggingAdapter

from inv_trader.model.enums import Resolution, QuoteType, Venue
from inv_trader.model.objects import Symbol, Tick, BarType, Bar, Instrument
from inv_trader.serialization import InstrumentSerializer
from inv_trader.strategy import TradeStrategy

UTF8 = 'utf-8'


class LiveDataClient:
    """
    Provides a data service client for alpha models and trading strategies.
    """

    def __init__(self,
                 host: str='localhost',
                 port: int=6379,
                 logger: Logger=None):
        """
        Initializes a new instance of the DataClient class.

        :param host: The redis host IP address (default=127.0.0.1).
        :param port: The redis host port (default=6379).
        :param logger: The logging adapter for the component.
        :raises ValueError: If the host is not a valid string.
        :raises ValueError: If the port is not in range [0, 65535]
        """
        Precondition.valid_string(host, 'host')
        Precondition.in_range(port, 'port', 0, 65535)

        self._host = host
        self._port = port
        if logger is None:
            self._log = LoggingAdapter(f"DataClient")
        else:
            self._log = LoggingAdapter(f"DataClient", logger)
        self._redis_client = None
        self._pubsub = None
        self._pubsub_thread = None
        self._subscriptions_ticks = []  # type: List[str]
        self._subscriptions_bars = []   # type: List[str]
        self._tick_handlers = []        # type: List[Callable]
        self._bar_handlers = []         # type: List[Callable]
        self._instruments = {}          # type: Dict[Symbol, Instrument]

        self._log.info("Initialized.")

    @property
    def is_connected(self) -> bool:
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
    def symbols(self) -> List[Symbol]:
        """
        :return: All instrument symbols held by the data client.
        """
        symbols = []
        for symbol in self._instruments:
            symbols.append(symbol)

        return symbols

    @property
    def instruments(self) -> List[Instrument]:
        """
        :return: All instruments held by the data client.
        """
        instruments = []
        for instrument in self._instruments.values():
            instruments.append(instrument)

        return instruments

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

    def connect(self):
        """
        Connect to the data service and create a pub/sub server.
        """
        self._redis_client = StrictRedis(host=self._host,
                                         port=self._port,
                                         db=0)
        self._pubsub = self._redis_client.pubsub()
        self._log.info(f"Connected to the data service at {self._host}:{self._port}.")

    def disconnect(self):
        """
        Disconnect from the local pub/sub server and the data service.
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
            self._log.info(f"Disconnected from the data service at {self._host}:{self._port}.")
        else:
            self._log.info("Disconnected (the data client was already disconnected).")

        self._redis_client = None
        self._pubsub = None
        self._pubsub_thread = None
        self._subscriptions_ticks = []
        self._subscriptions_bars = []
        self._tick_handlers = []
        self._bar_handlers = []

    def subscribe_ticks(
            self,
            symbol: str,
            venue: Venue,
            handler: Callable=None):
        """
        Subscribe to live tick data for the given symbol and venue.

        :param symbol: The symbol for subscription.
        :param venue: The venue for subscription.
        :param handler: The callable handler for subscription (if None will just call print).
        :raises ValueError: If the symbol is not a valid string.
        """
        Precondition.valid_string(symbol, 'symbol')

        self._check_connection()

        # If a handler is passed in, and doesn't already exist, then add to tick subscribers.
        if handler is not None and handler not in self._tick_handlers:
            self._tick_handlers.append(handler)

        ticks_channel = self._get_tick_channel_name(symbol, venue)
        self._pubsub.subscribe(**{ticks_channel: self._tick_handler})

        if self._pubsub_thread is None:
            self._pubsub_thread = self._pubsub.run_in_thread(0.001)

        if ticks_channel not in self._subscriptions_ticks:
            self._subscriptions_ticks.append(ticks_channel)
            self._subscriptions_ticks.sort()

        self._log.info(f"Subscribed to tick data for {ticks_channel}.")

    def unsubscribe_ticks(
            self,
            symbol: str,
            venue: Venue):
        """
        Unsubscribes from live tick data for the given symbol and venue.

        :param symbol: The symbol to unsubscribe from.
        :param venue: The venue to unsubscribe from.
        :raises ValueError: If the symbol is not a valid string.
        """
        Precondition.valid_string(symbol, 'symbol')

        self._check_connection()

        tick_channel = self._get_tick_channel_name(symbol, venue)

        self._pubsub.unsubscribe(tick_channel)

        if tick_channel in self._subscriptions_ticks:
            self._subscriptions_ticks.remove(tick_channel)
            self._subscriptions_ticks.sort()

        self._log.info(f"Unsubscribed from tick data for {tick_channel}.")

    def subscribe_bars(
            self,
            symbol: str,
            venue: Venue,
            period: int,
            resolution: Resolution,
            quote_type: QuoteType,
            handler: Callable=None):
        """
        Subscribe to live bar data for the given bar parameters.

        :param symbol: The symbol for subscription.
        :param venue: The venue for subscription.
        :param period: The bar period for subscription (> 0).
        :param resolution: The bar resolution for subscription.
        :param quote_type: The bar quote type for subscription.
        :param handler: The callable handler for subscription (if None will just call print).
        :raises ValueError: If the symbol is not a valid string.
        :raises ValueError: If the period is not positive (> 0).
        """
        Precondition.valid_string(symbol, 'symbol')
        Precondition.positive(period, 'period')

        self._check_connection()

        # If a handler is passed in, and doesn't already exist, then add to bar subscribers.
        if handler is not None and handler not in self._bar_handlers:
            self._bar_handlers.append(handler)

        bars_channel = self._get_bar_channel_name(
            symbol,
            venue,
            period,
            resolution,
            quote_type)
        self._pubsub.subscribe(**{bars_channel: self._bar_handler})

        if self._pubsub_thread is None:
            self._pubsub_thread = self._pubsub.run_in_thread(0.001)

        if bars_channel not in self._subscriptions_bars:
            self._subscriptions_bars.append(bars_channel)
            self._subscriptions_bars.sort()

        self._log.info(f"Subscribed to bar data for {bars_channel}.")

    def unsubscribe_bars(
            self,
            symbol: str,
            venue: Venue,
            period: int,
            resolution: Resolution,
            quote_type: QuoteType):
        """
        Unsubscribes from live bar data for the given symbol and venue.

        :param symbol: The symbol to unsubscribe from.
        :param venue: The venue to unsubscribe from.
        :param period: The bar period to unsubscribe from (> 0).
        :param resolution: The bar resolution to unsubscribe from.
        :param quote_type: The bar quote type to unsubscribe from.
        :raises ValueError: If the symbol is not a valid string.
        :raises ValueError: If the period is not positive (> 0).
        """
        Precondition.valid_string(symbol, 'symbol')
        Precondition.positive(period, 'period')

        self._check_connection()

        bar_channel = self._get_bar_channel_name(
            symbol,
            venue,
            period,
            resolution,
            quote_type)

        self._pubsub.unsubscribe(bar_channel)

        if bar_channel in self._subscriptions_bars:
            self._subscriptions_bars.remove(bar_channel)
            self._subscriptions_bars.sort()

        self._log.info(f"Unsubscribed from bar data for {bar_channel}.")

    def historical_bars(
            self,
            symbol: str,
            venue: Venue,
            period: int,
            resolution: Resolution,
            quote_type: QuoteType,
            amount: int or None=None):
        """
        Download the historical bars for the given parameters from the data service.
        Then pass them to all registered strategies.

        Note: Log warnings are given if the downloaded bars don't equal the
        requested amount or from datetime range.

        :param symbol: The symbol for the historical bars.
        :param venue: The venue for the historical bars.
        :param period: The bar period for the historical bars (> 0).
        :param resolution: The bar resolution for the historical bars.
        :param quote_type: The bar quote type for the historical bars.
        :param amount: The number of historical bars to download, if None then will download all.
        :raises ValueError: If the symbol is not a valid string.
        :raises ValueError: If the period is not positive (> 0).
        :raises ValueError: If the amount is not positive (> 0).
        """
        Precondition.valid_string(symbol, 'symbol')
        Precondition.positive(period, 'period')
        if amount is not None:
            Precondition.positive(amount, 'amount')

        self._check_connection()

        keys = self._get_redis_bar_keys(symbol, venue, resolution, quote_type)

        if len(keys) == 0:
            self._log.warning(
                "Cannot get historical bars (No bar keys found for the given parameters).")
            return

        bars = []
        if amount is None:
            for key in keys:
                bar_list = self._redis_client.lrange(key, 0, -1)
                for bar_bytes in bar_list:
                    bars.append(self._parse_bar(bar_bytes.decode(UTF8)))
        else:
            for key in keys[::-1]:
                bar_list = self._redis_client.lrange(key, 0, -1)
                for bar_bytes in bar_list[::-1]:
                    bars.insert(0, self._parse_bar(bar_bytes.decode(UTF8)))
                if len(bars) >= amount:
                    break

            bar_count = len(bars)
            if bar_count >= amount:
                last_index = bar_count - amount
                bars = bars[last_index:]
            else:
                self._log.warning(
                    f"Historical bars are < the requested amount ({len(bars)} vs {amount}).")

        bar_type = BarType(Symbol(symbol, venue), period, resolution, quote_type)
        self._log.info(f"Historical download of {len(bars)} bars for {bar_type} complete.")

        for bar in bars:
            [handler(bar_type, bar) for handler in self._bar_handlers]
        self._log.info(f"Historical bars hydrated for all registered strategies.")

    def historical_bars_from(
            self,
            symbol: str,
            venue: Venue,
            period: int,
            resolution: Resolution,
            quote_type: QuoteType,
            from_datetime: datetime):
        """
        Download the historical bars for the given parameters from the data service.
        Then pass them to all registered strategies.

        Note: Log warnings are given if the downloaded bars don't equal the
        requested amount or from datetime range.

        :param symbol: The symbol for the historical bars.
        :param venue: The venue for the historical bars.
        :param period: The bar period for the historical bars (> 0).
        :param resolution: The bar resolution for the historical bars.
        :param quote_type: The bar quote type for the historical bars.
        :param from_datetime: The datetime from which the historical bars should be downloaded.
        :raises ValueError: If the symbol is not a valid string.
        :raises ValueError: If the period is not positive (> 0).
        :raises ValueError: If the from_datetime is not less than datetime.utcnow().
        """
        Precondition.valid_string(symbol, 'symbol')
        Precondition.positive(period, 'period')
        Precondition.true(from_datetime < datetime.now(timezone.utc),
                          'from_datetime < datetime.now(timezone.utc)')

        self._check_connection()

        keys = self._get_redis_bar_keys(symbol, venue, resolution, quote_type)

        if len(keys) == 0:
            self._log.warning(
                "Cannot get historical bars (No bar keys found for the given parameters).")
            return

        bars = []
        for key in keys[::-1]:
            bar_list = self._redis_client.lrange(key, 0, -1)
            for bar_bytes in bar_list[::-1]:
                bar = self._parse_bar(bar_bytes.decode(UTF8))
                if bar.timestamp >= from_datetime:
                    bars.insert(0, self._parse_bar(bar_bytes.decode(UTF8)))
                else:
                    self._log.debug("His")
                    break  # Reached from_datetime.

        first_bar_timestamp = bars[0].timestamp
        if first_bar_timestamp > from_datetime:
            self._log.warning(
                (f"Historical bars first bar timestamp greater than requested from datetime "
                 f"({first_bar_timestamp.isoformat()} vs {from_datetime.isoformat()})."))

        bar_type = BarType(Symbol(symbol, venue), period, resolution, quote_type)
        self._log.info(f"Historical download of {len(bars)} bars for {bar_type} complete.")

        for bar in bars:
            [handler(bar_type, bar) for handler in self._bar_handlers]
        self._log.info(f"Historical bars hydrated for all registered strategies.")

    def update_all_instruments(self):
        """
        Update all held instruments from the live database.
        """
        keys = self._redis_client.keys('instruments*')

        for key in keys:
            instrument = InstrumentSerializer.deserialize(self._redis_client.get(key))
            self._instruments[instrument.symbol] = instrument
            self._log.info(f"Updated instrument for {instrument.symbol}.")

    def update_instrument(self, symbol: Symbol):
        """
        Update the instrument corresponding to the given symbol (if found).
        Will log a warning is symbol is not found.

        :param symbol: The symbol to update.
        """
        key = f'instruments:{symbol.code.lower()}.{symbol.venue.name.lower()}'

        if key is None:
            self._log.warning(
                f"Cannot update instrument (symbol {symbol}not found in live database).")
            return

        instrument = InstrumentSerializer.deserialize(self._redis_client.get(key))
        self._instruments[symbol] = instrument
        self._log.info(f"Updated instrument for {symbol}.")

    def get_instrument(self, symbol: Symbol) -> Instrument:
        """
        Get the instrument corresponding to the given symbol.

        :param symbol: The symbol of the instrument to get.
        :return: The instrument (if found)
        :raises KeyError: If the instrument is not found.
        """
        if symbol not in self._instruments:
            raise KeyError(f"Cannot find instrument for {symbol}.")

        return self._instruments[symbol]

    def _get_redis_bar_keys(
            self,
            symbol: str,
            venue: Venue,
            resolution: Resolution,
            quote_type: QuoteType,):
        """
        Generate the bar key wildcard pattern and return the held Redis keys.
        """
        keys = self._redis_client.keys(
            (f'bars'
             f':{venue.name.lower()}'
             f':{symbol.lower()}'
             f':{resolution.name.lower()}'
             f':{quote_type.name.lower()}*'))
        keys.sort()
        return keys

    def register_strategy(self, strategy: TradeStrategy):
        """
        Registers the trade strategy to receive all ticks and bars from the
        live data client.

        :param strategy: The strategy inheriting from TradeStrategy.
        """
        strategy_tick_handler = strategy._update_ticks
        if strategy_tick_handler not in self._tick_handlers:
            self._tick_handlers.append(strategy_tick_handler)

        strategy_bar_handler = strategy._update_bars
        if strategy_bar_handler not in self._bar_handlers:
            self._bar_handlers.append(strategy_bar_handler)

        strategy._register_data_client(self)

        self._log.info(f"Registered strategy {strategy} with the data client.")

    @staticmethod
    def _parse_tick(
            tick_channel: str,
            tick_string: str) -> Tick:
        """
        Parse a Tick object from the given UTF-8 string.

        :param tick_string: The channel the tick was received on.
        :param tick_string: The tick string.
        :return: The parsed Tick object.
        """
        split_channel = tick_channel.split('.')
        split_tick = tick_string.split(',')

        return Tick(Symbol(split_channel[0], Venue[str(split_channel[1].upper())]),
                    Decimal(split_tick[0]),
                    Decimal(split_tick[1]),
                    iso8601.parse_date(split_tick[2]))

    @staticmethod
    def _parse_bar_type(bar_type_string: str) -> BarType:
        """
        Parse a BarType object from the given UTF-8 string.

        :param bar_type_string: The bar type string to parse.
        :return: The parsed Bar object.
        """
        split_string = re.split(r'[.-]+', bar_type_string)
        resolution = split_string[3].split('[')[0]
        quote_type = split_string[3].split('[')[1].strip(']')

        return BarType(Symbol(split_string[0], Venue[split_string[1].upper()]),
                       int(split_string[2]),
                       Resolution[resolution.upper()],
                       QuoteType[quote_type.upper()])

    @staticmethod
    def _parse_bar(bar_string: str) -> Bar:
        """
        Parse a Bar object from the given UTF-8 string.

        :param bar_string: The bar string to parse.
        :return: The parsed bar object.
        """
        split_bar = bar_string.split(',')

        return Bar(Decimal(split_bar[0]),
                   Decimal(split_bar[1]),
                   Decimal(split_bar[2]),
                   Decimal(split_bar[3]),
                   int(split_bar[4]),
                   iso8601.parse_date(split_bar[5]))

    @staticmethod
    def _get_tick_channel_name(
            symbol: str,
            venue: Venue) -> str:
        """
        Return the tick channel name from the given parameters.
        """
        return str(f'{symbol.lower()}.{venue.name.lower()}')

    @staticmethod
    def _get_bar_channel_name(
            symbol: str,
            venue: Venue,
            period: int,
            resolution: Resolution,
            quote_type: QuoteType) -> str:
        """
        Return the bar channel name from the given parameters.
        """
        return str(f'{symbol.lower()}.{venue.name.lower()}-{period}-'
                   f'{resolution.name.lower()}[{quote_type.name.lower()}]')

    def _check_connection(self):
        """
        Check the connection with the live database.

        :raises ConnectionError if the client is None.
        :raises ConnectionError if the client is not connected.
        """
        if self._redis_client is None:
            raise ConnectionError(("No connection has been established to the live database "
                                   "(please connect first)."))
        if not self.is_connected:
            raise ConnectionError("No connection is established with the live database.")

    def _tick_handler(self, message: Dict):
        """"
        Handle the tick message by parsing to a Tick and sending to all subscribers.

        :param message: The tick message.
        """
        # If no tick handlers then print message to console.
        if len(self._tick_handlers) == 0:
            print(f"Received message {message['channel'].decode(UTF8)} "
                  f"{message['data'].decode(UTF8)}")

        tick = self._parse_tick(
            message['channel'].decode(UTF8),
            message['data'].decode(UTF8))
        [handler(tick) for handler in self._tick_handlers]

    def _bar_handler(self, message: Dict):
        """"
        Handle the bar message by parsing to a Bar and sending to all subscribers.

        :param message: The bar message.
        """
        # If no bar handlers then print message to console.
        if len(self._bar_handlers) == 0:
            print(f"Received message {message['channel'].decode(UTF8)} "
                  f"{message['data'].decode(UTF8)}")

        bar_type = self._parse_bar_type(message['channel'].decode(UTF8))
        bar = self._parse_bar(message['data'].decode(UTF8))
        [handler(bar_type, bar) for handler in self._bar_handlers]
