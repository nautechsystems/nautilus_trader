#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="enums.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

from enum import Enum


class Venue(Enum):
    DUKASCOPY = 0,
    FXCM = 1


class Resolution(Enum):
    TICK = 0,
    SECOND = 1,
    MINUTE = 2,
    HOUR = 3,
    DAY = 4,


class QuoteType(Enum):
    BID = 0,
    ASK = 1,
    LAST = 2,
    MID = 3,


class OrderSide(Enum):
    BUY = 0,
    SELL = 1


class OrderType(Enum):
    MARKET = 0,
    LIMIT = 1,
    STOP = 2


class TimeInForce(Enum):
    DAY = 0,
    GTC = 1,
    IOC = 3,
    FOC = 4,
    GTD = 5


class OrderStatus(Enum):
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
