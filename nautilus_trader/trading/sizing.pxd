# -------------------------------------------------------------------------------------------------
# <copyright file="sizing.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from nautilus_trader.model.objects cimport Quantity, Price, Money, Instrument


cdef class PositionSizer:
    cdef readonly Instrument instrument

    cpdef void update_instrument(self, Instrument instrument) except *
    cpdef Quantity calculate(
        self,
        Money equity,
        double risk_bp,
        Price entry,
        Price stop_loss,
        double exchange_rate=*,
        double commission_rate_bp=*,
        double hard_limit=*,
        int units=*,
        int unit_batch_size=*)

    cdef double _calculate_risk_ticks(self, double entry, double stop_loss)
    cdef double _calculate_riskable_money(
        self,
        double equity,
        double risk_bp,
        double commission_rate_bp)


cdef class FixedRiskSizer(PositionSizer):
    pass
