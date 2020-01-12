# -------------------------------------------------------------------------------------------------
# <copyright file="brokerage.pxd" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from cpython.datetime cimport datetime

from nautilus_trader.model.objects cimport Money, Quantity, Price
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.currency cimport ExchangeRateCalculator


cdef class CommissionCalculator:
    cdef dict rates
    cdef double default_rate_bp
    cdef Money minimum

    cpdef Money calculate(self, Symbol symbol, Quantity filled_quantity, Price filled_price, double exchange_rate)
    cpdef Money calculate_for_notional(self, Symbol symbol, Money notional_value)

    cdef double _get_commission_rate(self, Symbol symbol)


cdef class RolloverInterestCalculator:
    cdef ExchangeRateCalculator _exchange_calculator
    cdef dict _rate_data

    cpdef object get_rate_data(self)
    cpdef double calc_overnight_rate(self, Symbol symbol, datetime timestamp) except *
