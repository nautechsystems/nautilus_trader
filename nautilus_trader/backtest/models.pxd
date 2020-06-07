# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------


cdef class FillModel:
    cdef readonly double prob_fill_at_limit
    cdef readonly double prob_fill_at_stop
    cdef readonly double prob_slippage

    cpdef bint is_limit_filled(self)
    cpdef bint is_stop_filled(self)
    cpdef bint is_slipped(self)

    cdef bint _did_event_occur(self, double probability)
