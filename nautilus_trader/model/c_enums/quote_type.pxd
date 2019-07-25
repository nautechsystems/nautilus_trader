#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="quote_type.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cpdef enum QuoteType:
    UNKNOWN = -1,
    BID = 0,
    ASK = 1,
    MID = 2,
    LAST = 3


cdef inline str quote_type_string(int value):
    if value == 0:
        return "BID"
    elif value == 1:
        return "ASK"
    elif value == 2:
        return "MID"
    elif value == 3:
        return "LAST"
    elif value == -1:
        return "UNKNOWN"
    else:
        return "UNKNOWN"
