#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="data.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import redis

from redis import Redis
from redis import ConnectionError
from typing import List

# Private IP 10.135.55.111


class LiveDataClient:
    """
    Provides a live data client for trading alpha models and strategies.
    """

    def __init__(self,
                 host: str = 'localhost',
                 port: int = 6379):
        """
        Initializes a new instance of the LiveDataClient class.

        :param host: The redis host IP address (default=127.0.0.1).
        :param port: The redis host port (default=6379).
        """
        self._host = host
        self._port = port
        self._client = None
        self._pubsub = None
        self._subscriptions_tick = []
        self._subscriptions_bars = []

    # Temporary property for development
    def client(self) -> Redis:
        return self._client

    @property
    def is_connected(self) -> bool:
        """
        Returns a value indicating whether the client is connected to the live database.

        :return: True if the client is connected, otherwise false.
        """
        if self._client is None:
            return False

        try:
            self._client.ping()
        except ConnectionError:
            return False

        return True

    def connect(self) -> str:
        """
        Connect to the live database and create a local pub/sub server.
        """
        self._client = redis.Redis(host=self._host, port=self._port, db=0)
        self._pubsub = self._client.pubsub()

        return f"Connected to live database at {self._host}:{self._port}."

    def disconnect(self) -> List[str]:
        """
        Disconnects from the local publish subscribe server and the database.
        """
        if self._client is None:
            raise ConnectionError("The client was never connected.")

        unsubscribed_tick = []
        unsubscribed_bars = []

        for symbol in self._subscriptions_tick[:]:
            self._pubsub.unsubscribe(symbol)
            self._subscriptions_tick.remove(symbol)
            unsubscribed_tick.append(symbol)

        for symbol_bartype in self._subscriptions_bars[:]:
            self._pubsub.unsubscribe(symbol_bartype)
            self._subscriptions_bars.remove(symbol_bartype)
            unsubscribed_bars.append(symbol_bartype)

        disconnect_message = [f"Unsubscribed from tick_data {unsubscribed_tick}.",
                              f"Unsubscribed from bars_data {unsubscribed_bars}."]

        self._client.connection_pool.disconnect()

        disconnect_message.append(f"Disconnected from live database at {self._host}:{self._port}.")
        return disconnect_message

    def hacked_tick_message_printer(self, message):
        print(f"{message['channel']}: {message['data']}", end='\r')

    def hacked_bar_message_printer(self, message):
        print(f"{message['channel']}: {message['data']}", end='\r')

    def subscribe_tick_data(
            self,
            symbol: str,
            venue: str) -> str:
        """
        Subscribe to live tick data for the given symbol and venue.
        """
        if symbol is None:
            raise ValueError("The symbol cannot be null.")
        if venue is None:
            raise ValueError("The venue cannot be null.")
        if self._client is None:
            return "No connection has been established to the live database (please connect first)."
        if not self.is_connected:
            return "No connection is established with the live database."

        tick_channel = f'{symbol}.{venue}'

        self._pubsub.subscribe(**{tick_channel: self.hacked_tick_message_printer})
        thread1 = self._pubsub.run_in_thread(sleep_time=0.001)

        if not any(tick_channel for s in self._subscriptions_tick):
            self._subscriptions_tick.append(tick_channel)
            self._subscriptions_tick.sort()

    def unsubscribe_tick_data(
            self,
            symbol: str,
            venue: str) -> str:
        """
        Un-subscribes from live tick data for the given symbol and venue.
        """
        if symbol is None:
            raise ValueError("The symbol cannot be null.")
        if venue is None:
            raise ValueError("The venue cannot be null.")
        if self._client is None:
            return "No connection has been established to the live database (please connect first)."
        if not self.is_connected:
            return "No connection is established with the live database."

        tick_channel = f'{symbol}.{venue}'

        self._pubsub.unsubscribe(tick_channel)

        if any(tick_channel for s in self._subscriptions_tick):
            self._subscriptions_tick.remove(tick_channel)
            self._subscriptions_tick.sort()

    def subscribe_bar_data(
            self,
            symbol: str,
            venue: str,
            period: str,
            resolution: str,
            quote_type: str) -> str:
        """
        Subscribe to live bar data for the given symbol and venue.
        """
        if symbol is None:
            raise ValueError("The symbol cannot be null.")
        if venue is None:
            raise ValueError("The venue cannot be null.")
        if self._client is None:
            return "No connection has been established to the live database (please connect first)."
        if not self.is_connected:
            return "No connection is established with the live database."

        security_symbol = f'{symbol}.{venue}'
        bar_channel = security_symbol + '-1-' + resolution + '[' + quote_type + ']'

        self._pubsub.subscribe(**{bar_channel: self.hacked_bar_message_printer})
        thread2 = self._pubsub.run_in_thread(sleep_time=0.001)

        if not any(bar_channel for s in self._subscriptions_bars):
            self._subscriptions_bars.append(bar_channel)
            self._subscriptions_bars.sort()

    def unsubscribe_bar_data(
            self,
            symbol: str,
            venue: str,
            period: str,
            resolution: str,
            quote_type: str) -> str:
        """
        Un-subscribes from live bar data for the given symbol and venue.
        """
        if symbol is None:
            raise ValueError("The symbol cannot be null.")
        if venue is None:
            raise ValueError("The venue cannot be null.")
        if self._client is None:
            return "No connection has been established to the live database (please connect first)."
        if not self.is_connected:
            return "No connection is established with the live database."

        security_symbol = f'{symbol}.{venue}'
        bar_channel = security_symbol + '-1-' + resolution + '[' + quote_type + ']'

        self._pubsub.unsubscribe(bar_channel)

        if any(bar_channel for s in self._subscriptions_bars):
            self._subscriptions_bars.remove(bar_channel)
            self._subscriptions_bars.sort()
