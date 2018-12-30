#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="enums.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False

cpdef enum Broker:
    DUKASCOPY_ = 0,
    FXCM_ = 1,
    INTERACTIVE_BROKERS = 2

cdef inline str broker_string(int value):
    if value == 0:
        return "DUKASCOPY"
    elif value == 1:
        return "FXCM"
    elif value == 2:
        return "INTERACTIVE_BROKERS"
    else:
        return "UNKNOWN"


cpdef enum Venue:
    DUKASCOPY = 0,
    FXCM = 1,
    IDEAL_PRO = 2,
    NYSE = 3,
    GLOBEX = 4

cdef inline str venue_string(int value):
    if value == 0:
        return "DUKASCOPY"
    elif value == 1:
        return "FXCM"
    elif value == 2:
        return "IDEAL_PRO"
    elif value == 3:
        return "NYSE"
    elif value == 4:
        return "GLOBEX"
    else:
        return "UNKNOWN"


cpdef enum Resolution:
    TICK = 0,
    SECOND = 1,
    MINUTE = 2,
    HOUR = 3,
    DAY_ = 4

cdef inline str resolution_string(int value):
    if value == 0:
        return "TICK"
    elif value == 1:
        return "SECOND"
    elif value == 2:
        return "MINUTE"
    elif value == 3:
        return "HOUR"
    elif value == 4:
        return "DAY"
    else:
        return "UNKNOWN"


cpdef enum QuoteType:
    BID = 0,
    ASK = 1,
    LAST = 2,
    MID = 3

cdef inline str quote_type_string(int value):
    if value == 0:
        return "BID"
    elif value == 1:
        return "ASK"
    elif value == 2:
        return "LAST"
    elif value == 3:
        return "MID"
    else:
        return "UNKNOWN"


cpdef enum OrderSide:
    BUY = 0,
    SELL = 1

cdef inline str order_side_string(int value):
    if value == 0:
        return "BUY"
    elif value == 1:
        return "SELL"
    else:
        return "UNKNOWN"


cpdef enum OrderType:
    MARKET = 0,
    LIMIT = 1,
    STOP_MARKET = 2,
    STOP_LIMIT = 3,
    MIT = 4

cdef inline str order_type_string(int value):
    if value == 0:
        return "MARKET"
    elif value == 1:
        return "LIMIT"
    elif value == 2:
        return "STOP_MARKET"
    elif value == 3:
        return "STOP_LIMIT"
    elif value == 4:
        return "MIT"
    else:
        return "UNKNOWN"

cpdef enum TimeInForce:
    DAY = 0,
    GTC = 1,
    IOC = 2,
    FOC = 3,
    GTD = 4

cdef inline str time_in_force(int value):
    if value == 0:
        return "DAY"
    elif value == 1:
        return "GTC"
    elif value == 2:
        return "IOC"
    elif value == 3:
        return "FOC"
    elif value == 4:
        return "GTD"
    else:
        return "UNKNOWN"
