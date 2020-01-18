# -------------------------------------------------------------------------------------------------
# <copyright file="tick_type.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cpdef enum TickType:
    UNDEFINED = -1,  # Invalid value
    TRADE = 0,
    QUOTE = 1,
    OPEN_INTEREST = 2,


cdef inline str tick_type_to_string(int value):
    if value == 0:
        return 'TRADE'
    elif value == 1:
        return 'QUOTE'
    elif value == 2:
        return 'OPEN_INTEREST'
    else:
        return 'UNDEFINED'


cdef inline TickType tick_type_from_string(str value):
    if value == "TRADE":
        return TickType.TRADE
    elif value == "QUOTE":
        return TickType.QUOTE
    elif value == "OPEN_INTEREST":
        return TickType.OPEN_INTEREST
    else:
        return TickType.UNDEFINED
