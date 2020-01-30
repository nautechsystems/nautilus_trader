# -------------------------------------------------------------------------------------------------
# <copyright file="time_in_force.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cpdef enum TimeInForce:
    UNDEFINED = 0,  # Invalid value
    DAY = 1,
    GTC = 2,
    IOC = 3,
    FOC = 4,
    GTD = 5


cdef inline str time_in_force_to_string(int value):
    if value == 1:
        return 'DAY'
    elif value == 2:
        return 'GTC'
    elif value == 3:
        return 'IOC'
    elif value == 4:
        return 'FOC'
    elif value == 5:
        return 'GTD'
    else:
        return 'UNDEFINED'


cdef inline TimeInForce time_in_force_from_string(str value):
    if value == 'DAY':
        return TimeInForce.DAY
    elif value == 'GTC':
        return TimeInForce.GTC
    elif value == 'IOC':
        return TimeInForce.IOC
    elif value == 'FOC':
        return TimeInForce.FOC
    elif value == 'GTD':
        return TimeInForce.GTD
    else:
        return TimeInForce.UNDEFINED
