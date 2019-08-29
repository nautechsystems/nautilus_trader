# -------------------------------------------------------------------------------------------------
# <copyright file="sizing.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.model.objects cimport Quantity, Price, Money, Instrument


cdef class PositionSizer:
    cdef readonly Instrument instrument

    cpdef void update_instrument(self, Instrument instrument)
    cpdef Quantity calculate(
            self,
            Money equity,
            float risk_bp,
            Price price_entry,
            Price price_stop_loss,
            float exchange_rate=*,
            float commission_rate_bp=*,
            int hard_limit=*,
            int units=*,
            int unit_batch_size=*)

    cdef int _calculate_risk_ticks(self, Price entry, Price stop_loss)
    cdef Money _calculate_riskable_money(self, Money equity, float risk_bp, float commission_rate_bp, float exchange_rate)


cdef class FixedRiskSizer(PositionSizer):
    pass
