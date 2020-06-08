# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------


cdef class Indicator:
    """
    The base class for all indicators.
    """
    cdef readonly str name
    cdef readonly str params
    cdef readonly bint check_inputs
    cdef readonly bint has_inputs
    cdef readonly bint initialized

    cdef void _reset_base(self)
