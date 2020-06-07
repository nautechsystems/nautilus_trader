# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Instrument


cdef class CSVTickDataLoader:
    pass


cdef class CSVBarDataLoader:
    pass


cdef class InstrumentLoader:
    cpdef Instrument default_fx_ccy(self, Symbol symbol)
