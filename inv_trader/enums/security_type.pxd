#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="security_type.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False


cpdef enum SecurityType:
    FOREX = 0,
    BOND = 1,
    EQUITY = 2,
    FUTURE = 3,
    CFD = 4,
    OPTION = 5

cdef inline str security_type_string(int value):
    if value == 0:
        return "FOREX"
    elif value == 1:
        return "BOND"
    elif value == 2:
        return "EQUITY"
    elif value == 3:
        return "FUTURE"
    elif value == 4:
        return "CFD"
    elif value == 5:
        return "OPTION"
    else:
        return "UNKNOWN"