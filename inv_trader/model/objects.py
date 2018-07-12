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

from inv_trader.core.checks import typechecking
from inv_trader.model.enums import Venue, Resolution, QuoteType


class Symbol:
    """
    Represents the symbol for a financial market tradeable instrument.
    """

    @typechecking
    def __init__(self,
                 code: str,
                 venue: Venue):
        """
        Initializes a new instance of the Symbol class.

        :param code: The symbols code.
        :param venue: The symbols venue.
        """
        self._code = code
        self._venue = venue

    @property
    def code(self) -> str:
        """
        :return: The symbols code.
        """
        return self._code

    @property
    def venue(self) -> Venue:
        """
        :return: The symbols venue.
        """
        return self._venue

    def __eq__(self, other) -> bool:
        """
        Override the default equality comparison.
        """
        if isinstance(other, self.__class__):
            return self.__dict__ == other.__dict__
        else:
            return False

    def __ne__(self, other):
        """
        Override the default not-equals comparison.
        """
        return not self.__eq__(other)

    def __str__(self) -> str:
        """
        :return: The str() string representation of the symbol.
        """
        return f"{self._code}.{self._venue.name}"

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the symbol.
        """
        return f"<{str(self)} object at {id(self)}>"


class Tick:
    """
    Represents a single tick in a financial market.
    """

    @typechecking
    def __init__(self,
                 symbol: str,
                 venue: Venue,
                 bid: Decimal,
                 ask: Decimal,
                 timestamp: datetime.datetime):
        """
        Initializes a new instance of the Tick class.

        :param: symbol: The tick symbol.
        :param: venue: The tick venue.
        :param: bid: The tick bid price.
        :param: ask: The tick ask price.
        :param: timestamp: The tick timestamp.
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
        :return: The ticks timestamp (ISO8601).
        """
        return self._timestamp

    def __eq__(self, other) -> bool:
        """
        Override the default equality comparison.
        """
        if isinstance(other, self.__class__):
            return self.__dict__ == other.__dict__
        else:
            return False

    def __ne__(self, other):
        """
        Override the default not-equals comparison.
        """
        return not self.__eq__(other)

    def __str__(self) -> str:
        """
        :return: The str() string representation of the tick.
        """
        return (f"Tick: {self._symbol}.{self._venue.name},"
                f"{self._bid},{self._ask},{self._timestamp.isoformat()}")

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the tick.
        """
        return f"<{str(self)} object at {id(self)}>"


class BarType:
    """
    Represents a symbol and bar specification.
    """

    @typechecking
    def __init__(self,
                 symbol: str,
                 venue: Venue,
                 period: int,
                 resolution: Resolution,
                 quote_type: QuoteType):
        """
        Initializes a new instance of the BarType class.

        :param symbol: The bar symbol.
        :param venue: The bar venue.
        :param period: The bar period.
        :param resolution: The bar resolution.
        :param quote_type: The bar quote type.
        """
        self._symbol = symbol
        self._venue = venue
        self._period = period
        self._resolution = resolution
        self._quote_type = quote_type

    @property
    def symbol(self) -> str:
        """
        :return: The bar types symbol.
        """
        return self._symbol

    @property
    def venue(self) -> Venue:
        """
        :return: The bar types venue.
        """
        return self._venue

    @property
    def period(self) -> int:
        """
        :return: The bar types period.
        """
        return self._period

    @property
    def resolution(self) -> Resolution:
        """
        :return: The bar types resolution.
        """
        return self._resolution

    @property
    def quote_type(self) -> QuoteType:
        """
        :return: The bar types quote type.
        """
        return self._quote_type

    def __eq__(self, other) -> bool:
        """
        Override the default equality comparison.
        """
        if isinstance(other, self.__class__):
            return self.__dict__ == other.__dict__
        else:
            return False

    def __ne__(self, other):
        """
        Override the default not-equals comparison.
        """
        return not self.__eq__(other)

    def __hash__(self):
        """"
        Override the default hash implementation.
        """
        return hash((self.symbol, self.venue, self.period, self.resolution, self.quote_type))

    def __str__(self) -> str:
        """
        :return: The str() string representation of the bar type.
        """
        return (f"{self._symbol.lower()}.{self._venue.name.lower()}"
                f"-{self._period}-{self._resolution.name.lower()}[{self._quote_type.name.lower()}]")

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the bar type.
        """
        return (f"<{self._symbol.lower()}.{self._venue.name.lower()}"
                f"-{self._period}-{self._resolution.name.lower()}[{self._quote_type.name.lower()}] "
                f"object at {id(self)}>")


class Bar:
    """
    Represents a financial market trade bar.
    """

    @typechecking
    def __init__(self,
                 open_price: Decimal,
                 high_price: Decimal,
                 low_price: Decimal,
                 close_price: Decimal,
                 volume: int,
                 timestamp: datetime.datetime):
        """
        Initializes a new instance of the Bar class.

        :param open_price: The bars open price.
        :param high_price: The bars high price.
        :param low_price: The bars low price.
        :param close_price: The bars close price.
        :param volume: The bars volume.
        :param timestamp: The bars timestamp.
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
        :return: The bars volume (tick volume).
        """
        return self._volume

    @property
    def timestamp(self) -> datetime.datetime:
        """
        :return: The bars timestamp (ISO8601).
        """
        return self._timestamp

    def __eq__(self, other) -> bool:
        """
        Override the default equality comparison.
        """
        if isinstance(other, self.__class__):
            return self.__dict__ == other.__dict__
        else:
            return False

    def __ne__(self, other):
        """
        Override the default not-equals comparison.
        """
        return not self.__eq__(other)

    def __str__(self) -> str:
        """
        :return: The str() string representation of the bar.
        """
        return (f"Bar: {self._open},{self._high},{self._low},{self._close},"
                f"{self._volume},{self._timestamp.isoformat()}")

    def __repr__(self) -> str:
        """
        :return: The repr() string representation of the bar.
        """
        return f"<{str(self)} object at {id(self)}>"
