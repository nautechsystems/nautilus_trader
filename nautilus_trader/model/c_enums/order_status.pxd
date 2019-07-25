# -------------------------------------------------------------------------------------------------
# <copyright file="order_status.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cpdef enum OrderStatus:
    UNKNOWN = -1,
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
    elif value == -1:
        return "UNKNOWN"
    else:
        return "UNKNOWN"
