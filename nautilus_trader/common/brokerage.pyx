# -------------------------------------------------------------------------------------------------
# <copyright file="brokerage.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import os
import pandas as pd

from cpython.datetime cimport datetime
from decimal import Decimal

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.functions cimport basis_points_as_percentage
from nautilus_trader.model.c_enums.currency cimport Currency, currency_from_string
from nautilus_trader.model.currency cimport ExchangeRateCalculator
from nautilus_trader.model.objects cimport Money, Quantity, Price
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.version import PACKAGE_ROOT


cdef class CommissionCalculator:
    """
    Provides commission calculations.
    """

    def __init__(self,
                 dict rates=None,
                 float default_rate_bp=0.20,
                 Money minimum=Money(2.00)):
        """
        Initializes a new instance of the CommissionCalculator class.

        Note: Commission rates are expressed as basis points of notional transaction value.
        :param rates: The dictionary of commission rates Dict[Symbol, float].
        :param default_rate_bp: The default rate if not found in dictionary (optional).
        :param minimum: The minimum commission charge per transaction.
        """
        if rates is None:
            rates = {}
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


cdef class RolloverInterestCalculator:
    """
    Provides rollover interest rate calculations. If rate_data_csv_path is empty then
    will default to the included short-term interest rate data csv (data since 1956).
    """

    def __init__(self, str rate_data_csv_path=''):
        """
        Initializes a new instance of the RolloverInterestCalculator class.

        :param rate_data_csv_path: The path to the short term interest rate data csv.
        """
        if rate_data_csv_path == '':
            rate_data_csv_path = os.path.join(PACKAGE_ROOT + '/data/', 'short_term_interest.csv')
        self._exchange_calculator = ExchangeRateCalculator()
        self._rate_data = pd.read_csv(rate_data_csv_path)

    cpdef object get_rate_data(self):
        """
        Return the short-term interest rate dataframe.
        
        :return: pd.DataFrame.
        """
        return self._rate_data

    cpdef float calc_overnight_fx_rate(self, Symbol symbol, datetime timestamp):
        """
        Return the rollover interest rate between the given base currency and quote currency.
        
        :param symbol: The forex currency symbol for the calculation.
        :param timestamp: The timestamp for the calculation.
        :return: float.
        :raises ConditionFailed: If the symbol.code length is not == 6.
        """
        Condition.true(len(symbol.code) == 6, 'len(symbol) == 6')

        cdef Currency base_currency = currency_from_string(symbol.code[:3])
        cdef Currency quote_currency = currency_from_string(symbol.code[3:])

        cdef int year = timestamp.year
        cdef int month = timestamp.month
        cdef int quarter = int(((timestamp.month - 1) // 3) + 1)

        base_frequency = self._rate_data

        print(base_frequency)

        base_interest = 1
        quote_interest = 1

        return (base_interest - quote_interest) / 365
