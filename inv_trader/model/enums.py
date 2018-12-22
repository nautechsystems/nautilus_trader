#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="enums.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

from enum import Enum, unique


@unique
class Broker(Enum):
    """
    Represents a brokerage.
    """
    DUKASCOPY = 0,
    FXCM = 1,
    INTERACTIVE_BROKERS = 2


@unique
class Venue(Enum):
    """
    Represents an execution venue.
    """
    DUKASCOPY = 0,
    FXCM = 1,
    IDEAL_PRO = 2,
    NYSE = 3,
    GLOBEX = 4


@unique
class Resolution(Enum):
    """
    Represents the granularity time period.
    """
    TICK = 0,
    SECOND = 1,
    MINUTE = 2,
    HOUR = 3,
    DAY = 4


@unique
class QuoteType(Enum):
    """
    The quote type a price is taken from.
    """
    BID = 0,
    ASK = 1,
    LAST = 2,
    MID = 3


@unique
class OrderSide(Enum):
    """
    Represents the direction of an order.
    """
    BUY = 0,
    SELL = 1


@unique
class OrderType(Enum):
    """
    Represents an orders type.
    """
    MARKET = 0,
    LIMIT = 1,
    STOP_MARKET = 2,
    STOP_LIMIT = 3,
    MIT = 4


@unique
class TimeInForce(Enum):
    """
    Represents an orders time in force type.
    """
    DAY = 0,
    GTC = 1,
    IOC = 3,
    FOC = 4,
    GTD = 5


@unique
class OrderStatus(Enum):
    """
    Represents an orders status.
    """
    INITIALIZED = 0,
    SUBMITTED = 1,
    ACCEPTED = 2,
    REJECTED = 3,
    WORKING = 4,
    CANCELLED = 5,
    OVER_FILLED = 6,
    PARTIALLY_FILLED = 7,
    FILLED = 8,
    EXPIRED = 9


@unique
class MarketPosition(Enum):
    """
    Represents the relative market position.
    """
    FLAT = 0,
    LONG = 1,
    SHORT = 2,


@unique
class SecurityType(Enum):
    """
    Represents a security type.
    """
    FOREX = 0,
    BOND = 1,
    EQUITY = 2,
    FUTURE = 3,
    CFD = 4,
    Option = 5


@unique
class CurrencyCode(Enum):
    """
    Currency codes ISO 4217.
    """
    AUD = 36,
    CAD = 124,
    CHF = 756,
    CNY = 156,
    CNH = 999,
    CZK = 203,
    EUR = 978,
    GBP = 826,
    HKD = 344,
    JPY = 392,
    MXN = 484,
    NOK = 578,
    NZD = 554,
    SEK = 752,
    TRY = 949,
    SGD = 702,
    USD = 840,
    XAG = 961,
    XPT = 962,
    XAU = 959,
    ZAR = 710
