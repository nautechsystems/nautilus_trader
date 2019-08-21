# -------------------------------------------------------------------------------------------------
# <copyright file="resolution.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cpdef enum Resolution:
    UNKNOWN = -1,
    TICK = 0,
    SECOND = 1,
    MINUTE = 2,
    HOUR = 3,
    DAY = 4


cdef inline str resolution_to_string(int value):
    if value == 0:
        return 'TICK'
    elif value == 1:
        return 'SECOND'
    elif value == 2:
        return 'MINUTE'
    elif value == 3:
        return 'HOUR'
    elif value == 4:
        return 'DAY'
    else:
        return 'UNKNOWN'


cdef inline Resolution resolution_from_string(str value):
    if value == 'TICK':
        return Resolution.TICK
    elif value == 'SECOND':
        return Resolution.SECOND
    elif value == 'MINUTE':
        return Resolution.MINUTE
    elif value == 'HOUR':
        return Resolution.HOUR
    elif value == 'DAY':
        return Resolution.DAY
    else:
        return Resolution.UNKNOWN
