# -------------------------------------------------------------------------------------------------
# <copyright file="order_status.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
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
    EXPIRED = 6,
    OVER_FILLED = 7,
    PARTIALLY_FILLED = 8,
    FILLED = 9,


cdef inline str order_status_to_string(int value):
    if value == 0:
        return 'INITIALIZED'
    elif value == 1:
        return 'SUBMITTED'
    elif value == 2:
        return 'ACCEPTED'
    elif value == 3:
        return 'REJECTED'
    elif value == 4:
        return 'WORKING'
    elif value == 5:
        return 'CANCELLED'
    elif value == 6:
        return 'EXPIRED'
    elif value == 7:
        return 'OVER_FILLED'
    elif value == 8:
        return 'PARTIALLY_FILLED'
    elif value == 9:
        return 'FILLED'
    else:
        return 'UNKNOWN'


cdef inline OrderStatus order_status_from_string(str value):
    if value == 'INITIALIZED':
        return OrderStatus.INITIALIZED
    elif value == 'SUBMITTED':
        return OrderStatus.SUBMITTED
    elif value == 'ACCEPTED':
        return OrderStatus.ACCEPTED
    elif value == 'REJECTED':
        return OrderStatus.REJECTED
    elif value == 'WORKING':
        return OrderStatus.WORKING
    elif value == 'CANCELLED':
        return OrderStatus.CANCELLED
    elif value == 'EXPIRED':
        return OrderStatus.EXPIRED
    elif value == 'OVER_FILLED':
        return OrderStatus.OVER_FILLED
    elif value == 'PARTIALLY_FILLED':
        return OrderStatus.PARTIALLY_FILLED
    elif value == 'FILLED':
        return OrderStatus.FILLED
    else:
        return OrderStatus.UNKNOWN
