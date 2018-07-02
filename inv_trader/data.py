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


class LiveDataClient:
    """
    Provides a live data client for trading alpha models and strategies.
    """

    def __init__(self,
                 host: str='localhost',
                 port: int=6379):
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

        security_symbol = f'{symbol}.{venue}'

        self._pubsub.subscribe(security_symbol)

        if not any(security_symbol for s in self._subscriptions_tick):
            self._subscriptions_tick.append(security_symbol).sort()

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

        security_symbol = f'{symbol}.{venue}'

        self._pubsub.unsubscribe(security_symbol)

        if any(security_symbol for s in self._subscriptions_tick):
            self._subscriptions_tick.remove(security_symbol).sort()




