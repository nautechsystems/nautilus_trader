#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="brokerage.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from decimal import Decimal

from inv_trader.core.precondition cimport Precondition
from inv_trader.model.objects cimport Symbol, Money, Quantity


cdef class CommissionCalculator:
    """
    Provides a means of calculating commissions.
    """

    def __init__(self, dict rates={}, default: Decimal=Decimal(15)):
        """
        Initializes a new instance of the CommissionCalculator class.

        Note: Commission rates are expressed as Decimals per transaction per million notional.
        :param rates: The dictionary of commission rates Dict[Symbol, Decimal].
        :param default: The default rate if not found in dictionary (optional).
        """
        Precondition.dict_types(rates, Symbol, Decimal, 'rates')
        Precondition.type(default, Decimal, 'default')

        self.rates = rates
        self.default = default

    cpdef Money calculate(
            self,
            Symbol symbol,
            Quantity filled_quantity,
            float exchange_rate):
        """
        Return the calculated commission for the given arguments.
        
        :param symbol: The symbol for calculation.
        :param filled_quantity: The filled quantity.
        :param exchange_rate: The exchange rate (symbol quote currency to account base currency).
        :return: Money.
        """
        cdef float commission_rate

        if symbol in self.rates:
            commission_rate = float(self.rates[symbol])
        else:
            commission_rate = float(self.default)

        return Money(Decimal((float(filled_quantity.value) / 1000000) * commission_rate))
