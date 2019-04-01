#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="sizing.pxd" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from inv_trader.model.objects cimport Quantity, Price, Money, Instrument


cdef class PositionSizer:
    """
    The base class for all position sizers.
    """
    cdef readonly Instrument instrument

    cpdef void update_instrument(self, Instrument instrument)
    cpdef Quantity calculate(
            self,
            Money equity,
            float risk_bp,
            Price price_entry,
            Price price_stop_loss,
            float exchange_rate=*,
            float commission_rate=*,
            int hard_limit=*,
            int units=*,
            int unit_batch_size=*)

    cdef int _calculate_risk_ticks(self, Price entry, Price stop_loss)
    cdef Money _calculate_riskable_money(self, Money equity, float risk_bp, float commission_rate, float exchange_rate)


cdef class FixedRiskSizer(PositionSizer):
    """
    Provides position sizing calculations based on a given risk.
    """
    pass
