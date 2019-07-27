# -------------------------------------------------------------------------------------------------
# <copyright file="venue.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cpdef enum Venue:
    UNKNOWN = -1,
    DUKASCOPY = 0,
    FXCM = 1,
    IDEAL_PRO = 2,
    NYSE = 3,
    GLOBEX = 4

cdef inline str venue_string(int value):
    if value == 0:
        return "DUKASCOPY"
    elif value == 1:
        return "FXCM"
    elif value == 2:
        return "IDEAL_PRO"
    elif value == 3:
        return "NYSE"
    elif value == 4:
        return "GLOBEX"
    elif value == -1:
        return "UNKNOWN"
    else:
        return "UNKNOWN"
