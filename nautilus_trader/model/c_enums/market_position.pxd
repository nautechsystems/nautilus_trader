# -------------------------------------------------------------------------------------------------
# <copyright file="market_position.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cpdef enum MarketPosition:
    UNDEFINED = -1,  # Invalid value
    FLAT = 0,
    LONG = 1,
    SHORT = 2


cdef inline str market_position_to_string(int value):
    if value == 0:
        return 'FLAT'
    elif value == 1:
        return 'LONG'
    elif value == 2:
        return 'SHORT'
    else:
        return 'UNDEFINED'


cdef inline MarketPosition market_position_from_string(str value):
    if value == 'FLAT':
        return MarketPosition.FLAT
    elif value == 'LONG':
        return MarketPosition.LONG
    elif value == 'SHORT':
        return MarketPosition.SHORT
    else:
        return MarketPosition.UNDEFINED
