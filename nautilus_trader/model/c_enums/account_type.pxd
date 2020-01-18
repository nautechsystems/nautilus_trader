# -------------------------------------------------------------------------------------------------
# <copyright file="account_type.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cpdef enum AccountType:
    UNDEFINED = -1  # Invalid value
    SIMULATED = 0,
    DEMO = 1,
    REAL = 2,


cdef inline str account_type_to_string(int value):
    if value == 0:
        return 'SIMULATED'
    elif value == 1:
        return 'DEMO'
    elif value == 2:
        return 'REAL'
    else:
        return 'UNDEFINED'


cdef inline AccountType account_type_from_string(str value):
    if value == 'SIMULATED':
        return AccountType.SIMULATED
    elif value == 'DEMO':
        return AccountType.DEMO
    elif value == 'REAL':
        return AccountType.REAL
    else:
        return AccountType.UNDEFINED
