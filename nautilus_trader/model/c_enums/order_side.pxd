# -------------------------------------------------------------------------------------------------
# <copyright file="order_side.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cpdef enum OrderSide:
    UNDEFINED = 0,  # Invalid value
    BUY = 1,
    SELL = 2


cdef inline str order_side_to_string(int value):
    if value == 1:
        return 'BUY'
    elif value == 2:
        return 'SELL'
    else:
        return 'UNDEFINED'


cdef inline OrderSide order_side_from_string(str value):
    if value == 'BUY':
        return OrderSide.BUY
    elif value == 'SELL':
        return OrderSide.SELL
    else:
        return OrderSide.UNDEFINED
