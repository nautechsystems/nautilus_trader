#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="venue.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False


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