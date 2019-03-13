#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="order_status.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False


cpdef enum OrderStatus:
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

cdef inline str order_status_string(int value):
    if value == 0:
        return "INITIALIZED"
    elif value == 1:
        return "SUBMITTED"
    elif value == 2:
        return "ACCEPTED"
    elif value == 3:
        return "REJECTED"
    elif value == 4:
        return "WORKING"
    elif value == 5:
        return "CANCELLED"
    elif value == 6:
        return "OVER_FILLED"
    elif value == 7:
        return "PARTIALLY_FILLED"
    elif value == 8:
        return "FILLED"
    elif value == 9:
        return "EXPIRED"
    else:
        return "UNKNOWN"
