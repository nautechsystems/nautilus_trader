#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="broker.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False


cpdef enum Broker:
    UNKNOWN = -1,
    SIMULATED = 0,
    DUKASCOPY = 1,
    FXCM = 2,
    INTERACTIVE_BROKERS = 3

cdef inline str broker_string(int value):
    if value == 0:
        return "SIMULATED"
    elif value == 1:
        return "DUKASCOPY"
    elif value == 2:
        return "FXCM"
    elif value == 3:
        return "INTERACTIVE_BROKERS"
    elif value == -1:
        return "UNKNOWN"
    else:
        return "UNKNOWN"
