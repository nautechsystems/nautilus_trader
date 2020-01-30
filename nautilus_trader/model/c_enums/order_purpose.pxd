# -------------------------------------------------------------------------------------------------
# <copyright file="order_purpose.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cpdef enum OrderPurpose:
    UNDEFINED = 0,  # Invalid value
    NONE = 1,
    ENTRY = 2,
    EXIT = 3,
    STOP_LOSS = 4,
    TAKE_PROFIT = 5


cdef inline str order_purpose_to_string(int value):
    if value == 1:
        return 'NONE'
    elif value == 2:
        return 'ENTRY'
    elif value == 3:
        return 'EXIT'
    elif value == 4:
        return 'STOP_LOSS'
    elif value == 5:
        return 'TAKE_PROFIT'
    else:
        return 'UNDEFINED'


cdef inline OrderPurpose order_purpose_from_string(str value):
    if value == 'NONE':
        return OrderPurpose.NONE
    elif value == 'ENTRY':
        return OrderPurpose.ENTRY
    elif value == 'EXIT':
        return OrderPurpose.EXIT
    elif value == 'STOP_LOSS':
        return OrderPurpose.STOP_LOSS
    elif value == 'TAKE_PROFIT':
        return OrderPurpose.TAKE_PROFIT
    else:
        return OrderPurpose.UNDEFINED
