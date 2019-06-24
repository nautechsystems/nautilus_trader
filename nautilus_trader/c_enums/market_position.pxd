#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="market_position.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False


cpdef enum MarketPosition:
    UNKNOWN = -1,
    FLAT = 0,
    LONG = 1,
    SHORT = 2

cdef inline str market_position_string(int value):
    if value == 0:
        return "FLAT"
    elif value == 1:
        return "LONG"
    elif value == 2:
        return "SHORT"
    elif value == -1:
        return "UNKNOWN"
    else:
        return "UNKNOWN"
