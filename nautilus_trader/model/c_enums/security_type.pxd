# -------------------------------------------------------------------------------------------------
# <copyright file="security_type.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------


cpdef enum SecurityType:
    UNDEFINED = -1,  # Invalid value
    FOREX = 0,
    BOND = 1,
    EQUITY = 2,
    FUTURE = 3,
    CFD = 4,
    OPTION = 5,
    CRYPTO = 6


cdef inline str security_type_to_string(int value):
    if value == 0:
        return 'FOREX'
    elif value == 1:
        return 'BOND'
    elif value == 2:
        return 'EQUITY'
    elif value == 3:
        return 'FUTURE'
    elif value == 4:
        return 'CFD'
    elif value == 5:
        return 'OPTION'
    elif value == 6:
        return 'CRYPTO'
    else:
        return 'UNDEFINED'


cdef inline SecurityType security_type_from_string(str value):
    if value == 'FOREX':
        return SecurityType.FOREX
    elif value == 'BOND':
        return SecurityType.BOND
    elif value == 'EQUITY':
        return SecurityType.EQUITY
    elif value == 'FUTURE':
        return SecurityType.FUTURE
    elif value == 'CFD':
        return SecurityType.CFD
    elif value == 'OPTION':
        return SecurityType.OPTION
    elif value == 'CRYPTO':
        return SecurityType.CRYPTO
    else:
        return SecurityType.UNDEFINED
