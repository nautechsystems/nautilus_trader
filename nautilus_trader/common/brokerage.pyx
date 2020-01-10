# -------------------------------------------------------------------------------------------------
# <copyright file="brokerage.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import os
import pandas as pd

from cpython.datetime cimport datetime

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.functions cimport basis_points_as_percentage
from nautilus_trader.model.c_enums.currency cimport Currency, currency_from_string
from nautilus_trader.model.currency cimport ExchangeRateCalculator
from nautilus_trader.model.objects cimport Decimal, Money, Quantity, Price
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.__info__ import PACKAGE_ROOT


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
        :param default_rate_bp: The default rate if not found in dictionary.
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
        cdef float commission_rate_percent = basis_points_as_percentage(self._get_commission_rate(symbol))
        return Money(max(self.minimum.as_float(), filled_quantity.value * filled_price.as_float() * exchange_rate * commission_rate_percent))

    cpdef Money calculate_for_notional(self, Symbol symbol, Money notional_value):
        """
        Return the calculated commission for the given arguments.
        
        :param symbol: The symbol for calculation.
        :param notional_value: The notional value for the transaction.
        :return Money.
        """
        cdef float commission_rate_percent = basis_points_as_percentage(self._get_commission_rate(symbol))
        return Money(max(self.minimum.as_float(), notional_value.as_float() * commission_rate_percent))

    cdef float _get_commission_rate(self, Symbol symbol):
        cdef float rate = self.rates.get(symbol, -1.0)
        if rate != -1.0:
            print("***** rate found")
            return rate
        else:
            return self.default_rate_bp


cdef class RolloverInterestCalculator:
    """
    Provides rollover interest rate calculations. If rate_data_csv_path is empty then
    will default to the included short-term interest rate data csv (data since 1956).
    """

    def __init__(self, str short_term_interest_csv_path='default'):
        """
        Initializes a new instance of the RolloverInterestCalculator class.

        :param short_term_interest_csv_path: The path to the short term interest rate data csv.
        """
        if short_term_interest_csv_path == 'default':
            short_term_interest_csv_path = os.path.join(PACKAGE_ROOT + '/_data/', 'short_term_interest.csv')
        self._exchange_calculator = ExchangeRateCalculator()

        csv_rate_data = pd.read_csv(short_term_interest_csv_path)
        self._rate_data = {
            Currency.AUD: csv_rate_data.loc[csv_rate_data['LOCATION'] == 'AUS'],
            Currency.CAD: csv_rate_data.loc[csv_rate_data['LOCATION'] == 'CAN'],
            Currency.CHF: csv_rate_data.loc[csv_rate_data['LOCATION'] == 'CHE'],
            Currency.EUR: csv_rate_data.loc[csv_rate_data['LOCATION'] == 'EA19'],
            Currency.USD: csv_rate_data.loc[csv_rate_data['LOCATION'] == 'USA'],
            Currency.JPY: csv_rate_data.loc[csv_rate_data['LOCATION'] == 'JPN'],
            Currency.NZD: csv_rate_data.loc[csv_rate_data['LOCATION'] == 'NZL'],
            Currency.GBP: csv_rate_data.loc[csv_rate_data['LOCATION'] == 'GBR'],
            Currency.RUB: csv_rate_data.loc[csv_rate_data['LOCATION'] == 'RUS'],
            Currency.NOK: csv_rate_data.loc[csv_rate_data['LOCATION'] == 'NOR'],
            Currency.CNY: csv_rate_data.loc[csv_rate_data['LOCATION'] == 'CHN'],
            Currency.CNH: csv_rate_data.loc[csv_rate_data['LOCATION'] == 'CHN'],
            Currency.MXN: csv_rate_data.loc[csv_rate_data['LOCATION'] == 'MEX'],
            Currency.ZAR: csv_rate_data.loc[csv_rate_data['LOCATION'] == 'ZAF'],
        }

    cpdef object get_rate_data(self):
        """
        Return the short-term interest rate dataframe.
        
        :return: pd.DataFrame.
        """
        return self._rate_data

    cpdef float calc_overnight_rate(self, Symbol symbol, datetime timestamp) except *:
        """
        Return the rollover interest rate between the given base currency and quote currency.
        
        Note: 1% = 0.01
        
        :param symbol: The forex currency symbol for the calculation.
        :param timestamp: The timestamp for the calculation.
        :return: float.
        :raises ConditionFailed: If the symbol.code length is not == 6.
        """
        Condition.true(len(symbol.code) == 6, 'len(symbol) == 6')

        cdef Currency base_currency = currency_from_string(symbol.code[:3])
        cdef Currency quote_currency = currency_from_string(symbol.code[3:])

        cdef str time_monthly = f'{timestamp.year}-{str(timestamp.month).zfill(2)}'
        cdef str time_quarter = f'{timestamp.year}-Q{str(int(((timestamp.month - 1) // 3) + 1)).zfill(2)}'

        base_data = self._rate_data[base_currency].loc[self._rate_data[base_currency]['TIME'] == time_monthly]
        if len(base_data) == 0:
            base_data = self._rate_data[base_currency].loc[self._rate_data[base_currency]['TIME'] == time_quarter]

        quote_data = self._rate_data[quote_currency].loc[self._rate_data[quote_currency]['TIME'] == time_monthly]
        if len(quote_data) == 0:
            quote_data = self._rate_data[quote_currency].loc[self._rate_data[quote_currency]['TIME'] == time_quarter]

        if len(base_data) == 0 or len(quote_data) == 0:
            raise RuntimeError(f'Cannot find rollover interest rate for {symbol} on {timestamp.date()}.')

        cdef float base_interest = base_data['Value']
        cdef float quote_interest = quote_data['Value']

        return ((base_interest - quote_interest) / 365) / 100
