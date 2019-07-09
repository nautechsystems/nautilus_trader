#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="order_type.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False


cpdef enum OrderType:
    UNKNOWN = -1,
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
    elif value == -1:
        return "UNKNOWN"
    else:
        return "UNKNOWN"
