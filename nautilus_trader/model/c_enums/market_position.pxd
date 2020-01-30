# -------------------------------------------------------------------------------------------------
# <copyright file="market_position.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cpdef enum MarketPosition:
    UNDEFINED = 0,  # Invalid value
    FLAT = 1,
    LONG = 2,
    SHORT = 3


cdef inline str market_position_to_string(int value):
    if value == 1:
        return 'FLAT'
    elif value == 2:
        return 'LONG'
    elif value == 3:
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
