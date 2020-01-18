# -------------------------------------------------------------------------------------------------
# <copyright file="order_state.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cpdef enum OrderState:
    UNDEFINED = -1,  # Invalid value
    INITIALIZED = 0,
    INVALID = 1,
    DENIED = 2,
    SUBMITTED = 3,
    ACCEPTED = 4,
    REJECTED = 5,
    WORKING = 6,
    CANCELLED = 7,
    EXPIRED = 8,
    OVER_FILLED = 9,
    PARTIALLY_FILLED = 10,
    FILLED = 11,


cdef inline str order_state_to_string(int value):
    if value == 0:
        return 'INITIALIZED'
    elif value == 1:
        return 'INVALID'
    elif value == 2:
        return 'DENIED'
    elif value == 3:
        return 'SUBMITTED'
    elif value == 4:
        return 'ACCEPTED'
    elif value == 5:
        return 'REJECTED'
    elif value == 6:
        return 'WORKING'
    elif value == 7:
        return 'CANCELLED'
    elif value == 8:
        return 'EXPIRED'
    elif value == 9:
        return 'OVER_FILLED'
    elif value == 10:
        return 'PARTIALLY_FILLED'
    elif value == 11:
        return 'FILLED'
    else:
        return 'UNDEFINED'


cdef inline OrderState order_state_from_string(str value):
    if value == 'INITIALIZED':
        return OrderState.INITIALIZED
    elif value == 'INVALID':
        return OrderState.INVALID
    elif value == 'DENIED':
        return OrderState.DENIED
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
        return OrderState.UNDEFINED
