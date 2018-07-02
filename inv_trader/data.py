#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="redis.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import redis

from redis import StrictRedis
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
        self._subscriptions_tick = List[str]
        self._subscriptions_bars = List[str]

    # Temporary property to expose client.
    @property
    def client(self) -> StrictRedis:
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
            result = self.client.ping
        except ConnectionError:
            print("Connection error to the live database...")
            return False

        print(f"Connected to the live database ({result}ms).")
        return True

    def connect(self) -> str:
        """
        Connect to the live database and provide a client.
        """
        self._client = redis.StrictRedis(host=self._host, port=self._port, db=0)
        self._pubsub = self._client.pubsub()

        return f"Connected to Redis {self._host}:{self._port}."

    def disconnect(self) -> str:
        """
        Disconnects from the local publish subscribe server and the database.
        """
        self._pubsub.disconnect()
        self._client.disconnect()

        return f"Disconnected from Redis {self._host}:{self._port}."

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

        if not self._subscriptions_tick.contains(security_symbol):
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

        if self._subscriptions_tick.contains(security_symbol):
            self._subscriptions_tick.remove(security_symbol).sort()




