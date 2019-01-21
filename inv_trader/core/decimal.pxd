#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="decimal.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False


cdef class Decimal:
    cdef object _value
    cdef int _precision


cpdef enum ROUNDING:
    ROUND_HALF_EVEN,
    ROUND_05UP,
    ROUND_CEILING,
    ROUND_DOWN,
    ROUND_FLOOR,
    ROUND_HALF_DOWN,
    ROUND_HALF_UP,
    ROUND_UP


cdef inline str rounding_string(int value):
    if value == 0:
        return "ROUND_HALF_EVEN"
    elif value == 1:
        return "ROUND_05UP"
    elif value == 2:
        return "ROUND_CEILING"
    elif value == 3:
        return "ROUND_DOWN"
    elif value == 4:
        return "ROUND_FLOOR"
    elif value == 5:
        return "ROUND_HALF_DOWN"
    elif value == 6:
        return "ROUND_HALF_UP"
    elif value == 6:
        return "ROUND_UP"
    else:
        return "ROUND_HALF_EVEN"


cpdef ROUNDING get_rounding()
cpdef void set_rounding(ROUNDING rounding)
