#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="objects.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import datetime

from decimal import Decimal

from inv_trader.enums import Venue


class Tick:
    """
    Represents a single tick in a financial market.
    """

    def __init__(self,
                 symbol: str,
                 venue: Venue,
                 bid: Decimal,
                 ask: Decimal,
                 timestamp: datetime.datetime):
        """
        Initializes a new instance of the Tick class.
        """
        self._symbol = symbol.upper()
        self._venue = venue
        self._bid = bid
        self._ask = ask
        self._timestamp = timestamp

    @property
    def symbol(self) -> str:
        """
        :return: The ticks symbol.
        """
        return self._symbol

    @property
    def venue(self) -> Venue:
        """
        :return: The ticks venue.
        """
        return self._venue

    @property
    def bid(self) -> Decimal:
        """
        :return: The ticks bid price.
        """
        return self._bid

    @property
    def ask(self) -> Decimal:
        """
        :return: The ticks ask price.
        """
        return self._ask

    @property
    def timestamp(self) -> datetime.datetime:
        """
        :return: The ticks timestamp.
        """
        return self._timestamp

    def __str__(self) -> str:
        """
        :return: The str() string representation of the tick.
        """
        return f"Tick: {self._symbol}.{self._venue.name},{self._bid},{self._ask},{self._timestamp}"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the tick.
        """
        return f"<{str(self)} object at {id(self)}>"


class Bar:
    """
    Represents a financial market trade bar.
    """

    def __init__(self,
                 open_price: Decimal,
                 high_price: Decimal,
                 low_price: Decimal,
                 close_price: Decimal,
                 volume: int,
                 timestamp: datetime.datetime):
        """
        Initializes a new instance of the Bar class.
        """
        self._open = open_price
        self._high = high_price
        self._low = low_price
        self._close = close_price
        self._volume = volume
        self._timestamp = timestamp

    @property
    def open(self) -> Decimal:
        """
        :return: The bars open price.
        """
        return self._open

    @property
    def high(self) -> Decimal:
        """
        :return: The bars high price.
        """
        return self._high

    @property
    def low(self) -> Decimal:
        """
        :return: The bars low price.
        """
        return self._low

    @property
    def close(self) -> Decimal:
        """
        :return: The bars close price.
        """
        return self._close

    @property
    def volume(self) -> int:
        """
        :return: The bars volume.
        """
        return self._volume

    @property
    def timestamp(self) -> datetime.datetime:
        """
        :return: The bars timestamp.
        """
        return self._timestamp

    def __str__(self) -> str:
        """
        :return: The str() string representation of the bar.
        """
        return f"Bar:{self._open},{self._high},{self._low},{self._close}," \
               f"{self._volume},{self._timestamp}"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the bar.
        """
        return f"<{str(self)} object at {id(self)}>"
