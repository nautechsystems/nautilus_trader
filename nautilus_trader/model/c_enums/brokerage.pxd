# -------------------------------------------------------------------------------------------------
# <copyright file="broker.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cpdef enum Broker:
    UNKNOWN = -1,
    SIMULATED = 0,
    DUKASCOPY = 1,
    FXCM = 2,
    INTERACTIVE_BROKERS = 3

cdef inline str broker_string(int value):
    if value == 0:
        return "SIMULATED"
    elif value == 1:
        return "DUKASCOPY"
    elif value == 2:
        return "FXCM"
    elif value == 3:
        return "INTERACTIVE_BROKERS"
    elif value == -1:
        return "UNKNOWN"
    else:
        return "UNKNOWN"
