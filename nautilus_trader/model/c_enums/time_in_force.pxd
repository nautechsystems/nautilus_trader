# -------------------------------------------------------------------------------------------------
# <copyright file="time_in_force.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cpdef enum TimeInForce:
    UNKNOWN = -1,
    DAY = 0,
    GTC = 1,
    IOC = 2,
    FOC = 3,
    GTD = 4


cdef inline str time_in_force_to_string(int value):
    if value == 0:
        return 'DAY'
    elif value == 1:
        return 'GTC'
    elif value == 2:
        return 'IOC'
    elif value == 3:
        return 'FOC'
    elif value == 4:
        return 'GTD'
    else:
        return 'UNKNOWN'


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
        return TimeInForce.UNKNOWN
