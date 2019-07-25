#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="loaders.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.model.objects cimport Symbol, Instrument


cdef class InstrumentLoader:
    """
    Provides instrument template methods for backtesting.
    """

    cpdef Instrument default_fx_ccy(self, Symbol symbol, int tick_precision)
