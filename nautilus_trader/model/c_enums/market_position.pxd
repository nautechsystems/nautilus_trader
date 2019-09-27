# -------------------------------------------------------------------------------------------------
# <copyright file="market_position.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cpdef enum MarketPosition:
    SHORT = -1,
    FLAT = 0,
    LONG = 1


cdef inline str market_position_to_string(int value):
    if value == -1:
        return 'SHORT'
    elif value == 0:
        return 'FLAT'
    elif value == 1:
        return 'LONG'


cdef inline MarketPosition market_position_from_string(str value):
    if value == 'SHORT':
        return MarketPosition.SHORT
    elif value == 'FLAT':
        return MarketPosition.FLAT
    elif value == 'LONG':
        return MarketPosition.LONG
