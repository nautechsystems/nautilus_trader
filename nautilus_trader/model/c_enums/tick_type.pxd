# -------------------------------------------------------------------------------------------------
# <copyright file="tick_type.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cpdef enum TickType:
    UNKNOWN = -1,
    QUOTE = 0,
    TRADE = 1,
    OPEN_INTEREST = 2,


cdef inline str tick_type_to_string(int value):
    if value == 0:
        return 'QUOTE'
    elif value == 1:
        return 'TRADE'
    elif value == 2:
        return 'OPEN_INTEREST'
    else:
        return 'UNKNOWN'


cdef inline TickType tick_type_from_string(str value):
    if value == "QUOTE":
        return TickType.QUOTE
    elif value == "TRADE":
        return TickType.TRADE
    elif value == "OPEN_INTEREST":
        return TickType.OPEN_INTEREST
    elif value == "UNKNOWN":
        return TickType.UNKNOWN
    else:
        return TickType.UNKNOWN
