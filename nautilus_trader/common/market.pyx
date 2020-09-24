# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import os

import pandas as pd

from nautilus_trader.common.exchange cimport ExchangeRateCalculator
from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.core.functions cimport basis_points_as_percentage
from nautilus_trader.model.c_enums.currency cimport Currency
from nautilus_trader.model.c_enums.currency cimport currency_from_string
from nautilus_trader.model.c_enums.liquidity_side cimport LiquiditySide
from nautilus_trader.model.c_enums.liquidity_side cimport liquidity_side_to_string
from nautilus_trader.model.identifiers cimport Symbol
from nautilus_trader.model.objects cimport Money
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity

from nautilus_trader import PACKAGE_ROOT


cdef class CommissionModel:
    """
    The base class for all commission models.
    """

    cpdef Money calculate(
            self,
            Symbol symbol,
            Quantity filled_quantity,
            Price filled_price,
            double exchange_rate,
            Currency currency,
            LiquiditySide liquidity_side,
    ):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cpdef Money calculate_for_notional(
            self,
            Symbol symbol,
            Money notional_value,
            LiquiditySide liquidity_side,
    ):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")

    cdef double _get_commission_rate(self, Symbol symbol, LiquiditySide liquidity_side):
        # Abstract method
        raise NotImplementedError("method must be implemented in the subclass")


cdef class GenericCommissionModel:
    """
    Provides commission calculations.
    """

    def __init__(
            self,
            dict rates=None,
            double default_rate_bp=0.20,
            Money minimum=None,
    ):
        """
        Initialize a new instance of the CommissionCalculator class.

        Commission rates are expressed as basis points of notional transaction value.

        Parameters
        ----------
        rates : Dict[Symbol, double]
            The commission rates Dict[Symbol, double].
        default_rate_bp : double
            The default rate if symbol not found in the rates dictionary (>= 0).
        minimum : Money
            The minimum commission fee per transaction.

        Raises
        ------
        ValueError
            If rates contains a key type not of Symbol, or value type not of float.
        ValueError
            If default_rate_bp is negative (< 0).

        """
        if rates is None:
            rates = {}
        if minimum is None:
            minimum = Money(0, Currency.USD)
        Condition.dict_types(rates, Symbol, float, "rates")
        Condition.not_negative(default_rate_bp, "default_rate_bp")
        super().__init__()

        self.rates = rates
        self.default_rate_bp = default_rate_bp
        self.minimum= minimum

    cpdef Money calculate(
            self,
            Symbol symbol,
            Quantity filled_quantity,
            Price filled_price,
            double exchange_rate,
            Currency currency,
            LiquiditySide liquidity_side,
    ):
        """
        Return the calculated commission for the given arguments.

        Parameters
        ----------
        symbol : Symbol
            The symbol for calculation.
        filled_quantity : Quantity
            The filled quantity.
        filled_price : Price
            The filled price.
        exchange_rate : double
            The exchange rate (symbol quote currency to account base currency).
        currency : Currency
            The currency for the calculation.
        liquidity_side : LiquiditySide
            The liquidity side of the trade.

        Returns
        -------
        Money

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(filled_quantity, "filled_quantity")
        Condition.not_none(filled_price, "filled_price")
        Condition.positive(exchange_rate, "exchange_rate")

        cdef double commission_rate_percent = basis_points_as_percentage(self._get_commission_rate(symbol, liquidity_side))
        cdef double commission = filled_quantity.as_double() * filled_price.as_double() * exchange_rate * commission_rate_percent
        cdef double final_commission = max(self.minimum.as_double(), commission)
        return Money(final_commission, currency)

    cpdef Money calculate_for_notional(
            self,
            Symbol symbol,
            Money notional_value,
            LiquiditySide liquidity_side,
    ):
        """
        Return the calculated commission for the given arguments.

        Parameters
        ----------
        symbol : Symbol
            The symbol for calculation.
        notional_value : Money
            The notional value for the transaction.
        liquidity_side : LiquiditySide
            The liquidity side of the trade.

        Returns
        -------
        Money

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(notional_value, "notional_value")

        cdef double commission_rate_percent = basis_points_as_percentage(self._get_commission_rate(symbol, liquidity_side))
        cdef double value = max(self.minimum.as_double(), notional_value.as_double() * commission_rate_percent)
        return Money(value, notional_value.currency)

    cdef double _get_commission_rate(self, Symbol symbol, LiquiditySide liquidity_side):
        cdef double rate = self.rates.get(symbol, -1.0)
        if rate != -1.0:
            return rate
        else:
            return self.default_rate_bp


cdef class MakerTakerCommissionModel:
    """
    Provides a commission model with separate rates for liquidity takers or
    makers.
    """

    def __init__(
            self,
            dict taker_rates=None,
            dict maker_rates=None,
            double taker_default_rate_bp=7.5,
            double maker_default_rate_bp=-2.5,
    ):
        """
        Initialize a new instance of the CommissionCalculator class.

        Commission rates are expressed as basis points of notional transaction value.

        Parameters
        ----------
        taker_rates : Dict[Symbol, double]
            The taker commission rates in basis points.
        taker_rates : Dict[Symbol, double]
            The maker commission rates in basis points.
        taker_default_rate_bp : double
            The taker default rate if symbol not found in taker_rates dictionary.
        maker_default_rate_bp : double
            The maker default rate if symbol not found in maker_rates dictionary.

        Raises
        ------
        ValueError
            If taker_rates contains a key type not of Symbol, or value type not of float.
        ValueError
            If maker_rates contains a key type not of Symbol, or value type not of float.

        """
        if taker_rates is None:
            taker_rates = {}
        if maker_rates is None:
            maker_rates = {}
        Condition.dict_types(taker_rates, Symbol, float, "taker_rates")
        Condition.dict_types(maker_rates, Symbol, float, "maker_rates")
        super().__init__()

        self.taker_rates = taker_rates
        self.maker_rates = maker_rates
        self.taker_default_rate_bp = taker_default_rate_bp
        self.maker_default_rate_bp = maker_default_rate_bp

    cpdef Money calculate(
            self,
            Symbol symbol,
            Quantity filled_quantity,
            Price filled_price,
            double exchange_rate,
            Currency currency,
            LiquiditySide liquidity_side,
    ):
        """
        Return the calculated commission for the given arguments.

        Parameters
        ----------
        symbol : Symbol
            The symbol for calculation.
        filled_quantity : Quantity
            The filled quantity.
        filled_price : Price
            The filled price.
        exchange_rate : double
            The exchange rate (symbol quote currency to account base currency).
        currency : Currency
            The currency for the calculation.
        liquidity_side : LiquiditySide
            The liquidity side of the trade.

        Returns
        -------
        Money

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(filled_quantity, "filled_quantity")
        Condition.not_none(filled_price, "filled_price")
        Condition.positive(exchange_rate, "exchange_rate")

        cdef double commission_rate_percent = basis_points_as_percentage(self._get_commission_rate(symbol, liquidity_side))
        cdef double commission = filled_quantity.as_double() * filled_price.as_double() * exchange_rate * commission_rate_percent
        return Money(commission, currency)

    cpdef Money calculate_for_notional(
            self,
            Symbol symbol,
            Money notional_value,
            LiquiditySide liquidity_side,
    ):
        """
        Return the calculated commission for the given arguments.

        Parameters
        ----------
        symbol : Symbol
            The symbol for calculation.
        notional_value : Money
            The notional value for the transaction.
        liquidity_side : LiquiditySide
            The liquidity side of the trade.

        Returns
        -------
        Money

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(notional_value, "notional_value")

        cdef double commission_rate_percent = basis_points_as_percentage(self._get_commission_rate(symbol, liquidity_side))
        cdef double commission = notional_value.as_double() * commission_rate_percent
        return Money(notional_value.as_double() * commission_rate_percent, notional_value.currency)

    cdef double _get_commission_rate(self, Symbol symbol, LiquiditySide liquidity_side):
        if liquidity_side == LiquiditySide.TAKER:
            rate = self.taker_rates.get(symbol, None)
            return rate if rate is not None else self.taker_default_rate_bp
        elif liquidity_side == LiquiditySide.MAKER:
            rate = self.maker_rates.get(symbol, None)
            return rate if rate is not None else self.maker_default_rate_bp
        else:
            liquidity_side_str = liquidity_side_to_string(liquidity_side)
            raise ValueError(f"Cannot get commission rate (liquidity side was {liquidity_side_str})")


cdef class RolloverInterestCalculator:
    """
    Provides rollover interest rate calculations. If rate_data_csv_path is empty then
    will default to the included short-term interest rate data csv (data since 1956).
    """

    def __init__(self, str short_term_interest_csv_path not None="default"):
        """
        Initialize a new instance of the RolloverInterestCalculator class.

        Parameters
        ----------
        short_term_interest_csv_path : str
            The path to the short term interest rate data csv.

        """
        if short_term_interest_csv_path == "default":
            short_term_interest_csv_path = os.path.join(PACKAGE_ROOT + "/_internal/rates/", "short-term-interest.csv")
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

        Returns
        -------
        pd.DataFrame

        """
        return self._rate_data

    cpdef double calc_overnight_rate(self, Symbol symbol, date date) except *:
        """
        Return the rollover interest rate between the given base currency and quote currency.
        Note: 1% = 0.01 bp

        Parameters
        ----------
        symbol : Symbol
            The forex currency symbol for the calculation.
        date : date
            The date for the overnight rate.

        Returns
        -------
        double

        Raises
        ------
        ValueError
            If symbol.code length is not in range [6, 7].

        """
        Condition.not_none(symbol, "symbol")
        Condition.not_none(date, "timestamp")
        Condition.in_range_int(len(symbol.code), 6, 7, "len(symbol)")

        cdef Currency base_currency = currency_from_string(symbol.code[:3])
        cdef Currency quote_currency = currency_from_string(symbol.code[-3:])

        cdef str time_monthly = f"{date.year}-{str(date.month).zfill(2)}"
        cdef str time_quarter = f"{date.year}-Q{str(int(((date.month - 1) // 3) + 1)).zfill(2)}"

        base_data = self._rate_data[base_currency].loc[self._rate_data[base_currency]['TIME'] == time_monthly]
        if base_data.empty:
            base_data = self._rate_data[base_currency].loc[self._rate_data[base_currency]['TIME'] == time_quarter]

        quote_data = self._rate_data[quote_currency].loc[self._rate_data[quote_currency]['TIME'] == time_monthly]
        if quote_data.empty:
            quote_data = self._rate_data[quote_currency].loc[self._rate_data[quote_currency]['TIME'] == time_quarter]

        if base_data.empty and quote_data.empty:
            raise RuntimeError(f"Cannot find rollover interest rate for {symbol} on {date}.")

        cdef double base_interest = base_data['Value']
        cdef double quote_interest = quote_data['Value']

        return ((base_interest - quote_interest) / 365) / 100
