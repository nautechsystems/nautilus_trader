# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------


cpdef enum PriceType:
    UNDEFINED = 0,  # Invalid value
    BID = 1,
    ASK = 2,
    MID = 3,
    LAST = 4


cdef inline str price_type_to_string(int value):
    if value == 1:
        return 'BID'
    elif value == 2:
        return 'ASK'
    elif value == 3:
        return 'MID'
    elif value == 4:
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
