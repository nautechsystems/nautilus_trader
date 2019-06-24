#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="order_side.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False


cpdef enum OrderSide:
    UNKNOWN = -1,
    BUY = 0,
    SELL = 1

cdef inline str order_side_string(int value):
    if value == 0:
        return "BUY"
    elif value == 1:
        return "SELL"
    elif value == -1:
        return "UNKNOWN"
    else:
        return "UNKNOWN"
