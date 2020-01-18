# -------------------------------------------------------------------------------------------------
# <copyright file="price_type.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cpdef enum PriceType:
    UNDEFINED = -1,  # Invalid value
    BID = 0,
    ASK = 1,
    MID = 2,
    LAST = 3


cdef inline str price_type_to_string(int value):
    if value == 0:
        return 'BID'
    elif value == 1:
        return 'ASK'
    elif value == 2:
        return 'MID'
    elif value == 3:
        return 'LAST'
    else:
        return 'UNDEFINED'


cdef inline PriceType price_type_from_string(str value):
    if value == "BID":
        return PriceType.BID
    elif value == "ASK":
        return PriceType.ASK
    elif value == "MID":
        return PriceType.MID
    elif value == "LAST":
        return PriceType.LAST
    else:
        return PriceType.UNDEFINED
