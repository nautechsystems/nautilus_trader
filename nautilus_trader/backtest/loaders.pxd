# -------------------------------------------------------------------------------------------------
# <copyright file="loaders.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Instrument


cdef class CSVTickDataLoader:
    pass


cdef class CSVBarDataLoader:
    pass


cdef class InstrumentLoader:
    cpdef Instrument default_fx_ccy(self, Symbol symbol)
