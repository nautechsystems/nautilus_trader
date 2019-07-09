#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="time_in_force.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False


cpdef enum TimeInForce:
    UNKNOWN = -1,
    DAY = 0,
    GTC = 1,
    IOC = 2,
    FOC = 3,
    GTD = 4

cdef inline str time_in_force_string(int value):
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
    elif value == -1:
        return "UNKNOWN"
    else:
        return "UNKNOWN"
