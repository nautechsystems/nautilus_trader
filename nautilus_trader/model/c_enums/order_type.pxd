# -------------------------------------------------------------------------------------------------
# <copyright file="order_type.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cpdef enum OrderType:
    UNDEFINED = 0,  # Invalid value
    MARKET = 1,
    LIMIT = 2,
    STOP = 3,
    STOP_LIMIT = 4,
    MIT = 5


cdef inline str order_type_to_string(int value):
    if value == 1:
        return 'MARKET'
    elif value == 2:
        return 'LIMIT'
    elif value == 3:
        return 'STOP'
    elif value == 4:
        return 'STOP_LIMIT'
    elif value == 5:
        return 'MIT'
    else:
        return 'UNDEFINED'


cdef inline OrderType order_type_from_string(str value):
    if value == 'MARKET':
        return OrderType.MARKET
    elif value == 'LIMIT':
        return OrderType.LIMIT
    elif value == 'STOP':
        return OrderType.STOP
    elif value == 'STOP_LIMIT':
        return OrderType.STOP_LIMIT
    elif value == 'MIT':
        return OrderType.MIT
    else:
        return OrderType.UNDEFINED
