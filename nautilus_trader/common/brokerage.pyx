# -------------------------------------------------------------------------------------------------
# <copyright file="brokerage.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from decimal import Decimal

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.functions cimport basis_points_as_percentage
from nautilus_trader.model.objects cimport Money, Quantity, Price
from nautilus_trader.model.identifiers cimport Symbol


cdef class CommissionCalculator:
    """
    Provides a means of calculating commissions.
    """

    def __init__(
            self,
            dict rates={},
            float default_rate_bp=0.20,
            Money minimum=Money(2.00)):
        """
        Initializes a new instance of the CommissionCalculator class.

        Note: Commission rates are expressed as basis points of notional transaction value.
        :param rates: The dictionary of commission rates Dict[Symbol, float].
        :param default_rate_bp: The default rate if not found in dictionary (optional).
        :param minimum: The minimum commission charge per transaction.
        """
        Condition.dict_types(rates, Symbol, Decimal, 'rates')

        self.rates = rates
        self.default_rate_bp = default_rate_bp
        self.minimum = minimum

    cpdef Money calculate(
            self,
            Symbol symbol,
            Quantity filled_quantity,
            Price filled_price,
            float exchange_rate):
        """
        Return the calculated commission for the given arguments.
        
        :param symbol: The symbol for calculation.
        :param filled_quantity: The filled quantity.
        :param filled_price: The filled price.
        :param exchange_rate: The exchange rate (symbol quote currency to account base currency).
        :return Money.
        """
        commission_rate_percent = Decimal(basis_points_as_percentage(self._get_commission_rate(symbol)))
        return max(self.minimum, Money(filled_quantity.value * filled_price.value * Decimal(exchange_rate) * commission_rate_percent))

    cpdef Money calculate_for_notional(self, Symbol symbol, Money notional_value):
        """
        Return the calculated commission for the given arguments.
        
        :param symbol: The symbol for calculation.
        :param notional_value: The notional value for the transaction.
        :return Money.
        """
        commission_rate_percent = Decimal(basis_points_as_percentage(self._get_commission_rate(symbol)))
        return max(self.minimum, notional_value * commission_rate_percent)

    cdef float _get_commission_rate(self, Symbol symbol):
        if symbol in self.rates:
            return float(self.rates[symbol])
        else:
            return float(self.default_rate_bp)
