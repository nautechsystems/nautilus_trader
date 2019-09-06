# -------------------------------------------------------------------------------------------------
# <copyright file="order_state.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cpdef enum OrderState:
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


cdef inline str order_state_to_string(int value):
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


cdef inline OrderState order_state_from_string(str value):
    if value == 'INITIALIZED':
        return OrderState.INITIALIZED
    elif value == 'SUBMITTED':
        return OrderState.SUBMITTED
    elif value == 'ACCEPTED':
        return OrderState.ACCEPTED
    elif value == 'REJECTED':
        return OrderState.REJECTED
    elif value == 'WORKING':
        return OrderState.WORKING
    elif value == 'CANCELLED':
        return OrderState.CANCELLED
    elif value == 'EXPIRED':
        return OrderState.EXPIRED
    elif value == 'OVER_FILLED':
        return OrderState.OVER_FILLED
    elif value == 'PARTIALLY_FILLED':
        return OrderState.PARTIALLY_FILLED
    elif value == 'FILLED':
        return OrderState.FILLED
    else:
        return OrderState.UNKNOWN
