#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="order_side.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
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
