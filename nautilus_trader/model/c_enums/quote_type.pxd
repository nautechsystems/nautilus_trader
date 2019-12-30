# -------------------------------------------------------------------------------------------------
# <copyright file="quote_type.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cpdef enum QuoteType:
    UNKNOWN = -1,
    BID = 0,
    ASK = 1,
    MID = 2,
    LAST = 3


cdef inline str quote_type_to_string(int value):
    if value == 0:
        return 'BID'
    elif value == 1:
        return 'ASK'
    elif value == 2:
        return 'MID'
    elif value == 3:
        return 'LAST'
    else:
        return 'UNKNOWN'


cdef inline QuoteType quote_type_from_string(str value):
    if value == "BID":
        return QuoteType.BID
    elif value == "ASK":
        return QuoteType.ASK
    elif value == "MID":
        return QuoteType.MID
    elif value == "LAST":
        return QuoteType.LAST
    else:
        return QuoteType.UNKNOWN
