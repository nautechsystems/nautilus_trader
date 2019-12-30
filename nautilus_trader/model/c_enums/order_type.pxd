# -------------------------------------------------------------------------------------------------
# <copyright file="order_type.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cpdef enum OrderType:
    UNKNOWN = -1,
    MARKET = 0,
    LIMIT = 1,
    STOP_MARKET = 2,
    STOP_LIMIT = 3,
    MIT = 4


cdef inline str order_type_to_string(int value):
    if value == 0:
        return 'MARKET'
    elif value == 1:
        return 'LIMIT'
    elif value == 2:
        return 'STOP_MARKET'
    elif value == 3:
        return 'STOP_LIMIT'
    elif value == 4:
        return 'MIT'
    else:
        return 'UNKNOWN'


cdef inline OrderType order_type_from_string(str value):
    if value == 'MARKET':
        return OrderType.MARKET
    elif value == 'LIMIT':
        return OrderType.LIMIT
    elif value == 'STOP_MARKET':
        return OrderType.STOP_MARKET
    elif value == 'STOP_LIMIT':
        return OrderType.STOP_LIMIT
    elif value == 'MIT':
        return OrderType.MIT
    else:
        return OrderType.UNKNOWN
