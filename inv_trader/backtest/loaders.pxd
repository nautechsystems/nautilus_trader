#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="loaders.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

from inv_trader.enums.venue cimport Venue
from inv_trader.model.objects cimport Instrument


cdef class InstrumentLoader:
    """
    Provides instrument template methods for backtesting.
    """

    cpdef Instrument default_fx_ccy(
            self,
            str symbol_code,
            Venue venue,
            int tick_precision)
