#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="data.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import re
import redis
import iso8601
import time

from decimal import Decimal
from redis import ConnectionError
from typing import List

from inv_trader.enums import Resolution, QuoteType, Venue
from inv_trader.objects import Tick, BarType, Bar
from inv_trader.strategy import TradeStrategy

UTF8 = 'utf-8'


class LiveDataClient:
    """
    Provides a live data client for alpha models and trading strategies.
    """

    def __init__(self,
                 host: str='localhost',
                 port: int=6379):
        """
        Initializes a new instance of the LiveDataClient class.

        :param host: The redis host IP address (default=127.0.0.1).
        :param port: The redis host port (default=6379).
        """
        # Preconditions
        if host is None:
            raise ValueError("The host cannot be None.")
        if port is None:
            raise ValueError("The port cannot be None.")

        self._host = host
        self._port = port
        self._client = None
        self._pubsub = None
        self._pubsub_thread = None
        self._subscriptions_ticks = []
        self._subscriptions_bars = []
        self._tick_handlers = []
        self._bar_handlers = []

    @property
    def is_connected(self) -> bool:
        """
        :return: True if the client is connected, otherwise false.
        """
        if self._client is None:
            return False

        try:
            self._client.ping()
        except ConnectionError:
            return False

        return True

    @property
    def subscriptions_all(self) -> List[str]:
        """
        :return: All subscribed channels from the pub/sub server.
        """
        return [channel.decode(UTF8) for channel in self._client.pubsub_channels()]

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
        Connect to the live database and create a pub/sub server.
        """
        self._client = redis.StrictRedis(host=self._host, port=self._port, db=0)
        self._pubsub = self._client.pubsub()

        self._log(f"Connected to live database at {self._host}:{self._port}.")

    def disconnect(self):
        """
        Disconnects from the local pub/sub server and the database.
        """
        if self._pubsub is not None:
            self._pubsub.unsubscribe()

        if self._pubsub_thread is not None:
            self._pubsub_thread.stop()
            time.sleep(0.100)  # Allows thread to stop.
            self._log(f"Stopped PubSub thread {self._pubsub_thread}.")

        self._log(f"Unsubscribed from tick_data {self._subscriptions_ticks}.")
        self._log(f"Unsubscribed from bars_data {self._subscriptions_bars}.")

        if self._client is not None:
            self._client.connection_pool.disconnect()
            self._log(f"Disconnected from live database at {self._host}:{self._port}.")
        else:
            self._log("Disconnected (the client was already disconnected).")

        self._client = None
        self._pubsub = None
        self._pubsub_thread = None
        self._subscriptions_ticks = []
        self._subscriptions_bars = []
        self._tick_handlers = []
        self._bar_handlers = []

    def subscribe_tick_data(
            self,
            symbol: str,
            venue: Venue,
            handler: callable=None):
        """
        Subscribe to live tick data for the given symbol and venue.

        :param symbol: The symbol for subscription.
        :param venue: The venue for subscription.
        :param handler: The callable handler for subscription (if None will just call print).
        """
        # Preconditions
        if symbol is None:
            raise ValueError("The symbol cannot be None.")
        self._check_connection()
        if venue is None:
            raise ValueError("The venue cannot be None.")
        if handler is not None and not callable(handler):
            raise TypeError("The handler must be a callable object.")

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

        self._log(f"Subscribed to {ticks_channel}.")

    def unsubscribe_tick_data(
            self,
            symbol: str,
            venue: Venue):
        """
        Unsubscribes from live tick data for the given symbol and venue.

        :param symbol: The symbol to unsubscribe from.
        :param venue: The venue to unsubscribe from.
        """
        # Preconditions
        if symbol is None:
            raise ValueError("The symbol cannot be None.")
        self._check_connection()
        if venue is None:
            raise ValueError("The venue cannot be None.")

        self._check_connection()

        tick_channel = self._get_tick_channel_name(symbol, venue)

        self._pubsub.unsubscribe(tick_channel)

        if tick_channel in self._subscriptions_ticks:
            self._subscriptions_ticks.remove(tick_channel)
            self._subscriptions_ticks.sort()

        self._log(f"Unsubscribed from {tick_channel}.")

    def subscribe_bar_data(
            self,
            symbol: str,
            venue: Venue,
            period: int,
            resolution: Resolution,
            quote_type: QuoteType,
            handler: callable=None):
        """
        Subscribe to live bar data for the given bar parameters.

        :param symbol: The symbol for subscription.
        :param venue: The venue for subscription.
        :param period: The bar period for subscription (> 0).
        :param resolution: The bar resolution for subscription.
        :param quote_type: The bar quote type for subscription.
        :param handler: The callable handler for subscription (if None will just call print).
        """
        # Preconditions
        if symbol is None:
            raise ValueError("The symbol cannot be None.")
        if venue is None:
            raise ValueError("The venue cannot be None.")
        if period <= 0:
            raise ValueError("The period must be > 0.")
        if resolution is None:
            raise ValueError("The resolution cannot be None.")
        if quote_type is None:
            raise ValueError("The quote_type cannot be None.")
        if handler is not None and not callable(handler):
            raise TypeError("The handler must be a callable object.")

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

        self._log(f"Subscribed to {bars_channel}.")

    def unsubscribe_bar_data(
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
        """
        # Preconditions
        if symbol is None:
            raise ValueError("The symbol cannot be None.")
        if venue is None:
            raise ValueError("The venue cannot be None.")
        if period <= 0:
            raise ValueError("The period must be > 0.")
        if resolution is None:
            raise ValueError("The resolution cannot be None.")
        if quote_type is None:
            raise ValueError("The quote_type cannot be None.")

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

        self._log(f"Unsubscribed from {bar_channel}.")

    def register_strategy(self, strategy: TradeStrategy):
        """
        Registers the trade strategy to receive all ticks and bars from the
        live data client.

        :param strategy: The strategy inheriting from TradeStrategy.
        :raises: ValueError: If the strategy is None.
        :raises: TypeError: If the strategy is not a type of TradeStrategy.
        """
        # Preconditions
        if strategy is None:
            raise ValueError("The strategy cannot be None.")
        if not isinstance(strategy, TradeStrategy):
            raise TypeError("The strategy must be a type of TradeStrategy.")

        strategy_tick_handler = strategy._update_tick
        if strategy_tick_handler not in self._tick_handlers:
            self._tick_handlers.append(strategy_tick_handler)

        strategy_bar_handler = strategy._update_bars
        if strategy_bar_handler not in self._bar_handlers:
            self._bar_handlers.append(strategy_bar_handler)

        self._log(f"Registered {strategy} with the live data client.")

    @staticmethod
    def _log(message: str):
        """
        Log the given message (if no logger then prints).

        :param message: The message to log.
        """
        print(message)

    @staticmethod
    def _parse_tick(
            tick_channel: str,
            tick_string: str) -> Tick:
        """
        Parse a Tick object from the given UTF-8 string.

        :param tick_string: The channel the tick was received on.
        :param tick_string: The tick string.
        :return: The parsed tick object.
        """
        split_channel = tick_channel.split('.')
        split_tick = tick_string.split(',')

        return Tick(split_channel[0],
                    Venue[str(split_channel[1].upper())],
                    Decimal(split_tick[0]),
                    Decimal(split_tick[1]),
                    iso8601.parse_date(split_tick[2]))

    @staticmethod
    def _parse_bar_type(bar_type_string: str) -> BarType:
        """
        Parse a BarType object from the given UTF-8 string.

        :param bar_type_string: The bar type string to parse.
        :return: The parsed bar type object.
        """
        # TODO: Improve this regex.
        split_string = re.split(r'[.-]+', bar_type_string)
        resolution = split_string[3].split('[')[0]
        quote_type = split_string[3].split('[')[1].strip(']')

        return BarType(split_string[0],
                       Venue[split_string[1].upper()],
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
        return f'{symbol.lower()}.{venue.name.lower()}'

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
        return (f'{symbol.lower()}.{venue.name.lower()}-{period}-'
                f'{resolution.name.lower()}[{quote_type.name.lower()}]')

    def _check_connection(self):
        """
        Check the connection with the live database.

        :raises: ConnectionError if the client is not connected.
        """
        if self._client is None:
            raise ConnectionError(("No connection has been established to the live database "
                                   "(please connect first)."))
        if not self.is_connected:
            raise ConnectionError("No connection is established with the live database.")

    def _tick_handler(self, message):
        """"
        Handle the tick message by parsing to Tick and sending to all relevant subscribers.
        """
        # If no tick handlers then print message to console.
        if len(self._tick_handlers) == 0:
            print(f"Received message {message['channel'].decode(UTF8)} "
                  f"{message['data'].decode(UTF8)}")

        tick = self._parse_tick(
            message['channel'].decode(UTF8),
            message['data'].decode(UTF8))

        [handler(tick) for handler in self._tick_handlers]

    def _bar_handler(self, message):
        """"
        Handle the bar message by parsing to Bar and sending to all relevant subscribers.
        """
        # If no bar handlers then print message to console.
        if len(self._bar_handlers) == 0:
            print(f"Received message {message['channel'].decode(UTF8)} "
                  f"{message['data'].decode(UTF8)}")

        bar_type = self._parse_bar_type(message['channel'].decode(UTF8))
        bar = self._parse_bar(message['data'].decode(UTF8))

        [handler(bar_type, bar) for handler in self._bar_handlers]
