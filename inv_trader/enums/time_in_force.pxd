#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="time_in_force.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False


cpdef enum TimeInForce:
    NONE = 0,
    DAY = 1,
    GTC = 2,
    IOC = 3,
    FOC = 4,
    GTD = 5

cdef inline str time_in_force_string(int value):
    if value == 0:
        return "NONE"
    elif value == 1:
        return "DAY"
    elif value == 2:
        return "GTC"
    elif value == 3:
        return "IOC"
    elif value == 4:
        return "FOC"
    elif value == 5:
        return "GTD"
    else:
        return "UNKNOWN"
