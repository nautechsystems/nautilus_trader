# -------------------------------------------------------------------------------------------------
# <copyright file="bar_structure.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cpdef enum BarStructure:
    UNKNOWN = -1,
    TICK = 0,
    TICK_IMBALANCE = 1,
    VOLUME = 2,
    VOLUME_IMBALANCE = 3,
    DOLLAR = 4,
    DOLLAR_IMBALANCE = 5
    SECOND = 6,
    MINUTE = 7,
    HOUR = 8,
    DAY = 9,


cdef inline str bar_structure_to_string(int value):
    if value == 0:
        return 'TICK'
    elif value == 1:
        return 'TICK_IMBALANCE'
    elif value == 2:
        return 'VOLUME'
    elif value == 3:
        return 'VOLUME_IMBALANCE'
    elif value == 4:
        return 'DOLLAR'
    elif value == 5:
        return 'DOLLAR_IMBALANCE'
    elif value == 6:
        return 'SECOND'
    elif value == 7:
        return 'MINUTE'
    elif value == 8:
        return 'HOUR'
    elif value == 9:
        return 'DAY'
    else:
        return 'UNKNOWN'


cdef inline BarStructure bar_structure_from_string(str value):
    if value == 'TICK':
        return BarStructure.TICK
    elif value == 'TICK_IMBALANCE':
        return BarStructure.TICK_IMBALANCE
    elif value == 'VOLUME':
        return BarStructure.VOLUME
    elif value == 'VOLUME_IMBALANCE':
        return BarStructure.VOLUME_IMBALANCE
    elif value == 'DOLLAR':
        return BarStructure.DOLLAR
    elif value == 'DOLLAR_IMBALANCE':
        return BarStructure.DOLLAR_IMBALANCE
    elif value == 'SECOND':
        return BarStructure.SECOND
    elif value == 'MINUTE':
        return BarStructure.MINUTE
    elif value == 'HOUR':
        return BarStructure.HOUR
    elif value == 'DAY':
        return BarStructure.DAY
    else:
        return BarStructure.UNKNOWN
