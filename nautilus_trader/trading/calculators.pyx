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

from itertools import permutations
import os

import pandas as pd

from nautilus_trader import PACKAGE_ROOT

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.price_type cimport PriceType
from nautilus_trader.model.c_enums.price_type cimport price_type_to_string
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.identifiers cimport Symbol


cdef class ExchangeRateCalculator:
    """
    Provides exchange rate calculations between currencies.

    An exchange rate is the value of one asset versus that of another.
    """

    cpdef double get_rate(
            self,
            Currency from_currency,
            Currency to_currency,
            PriceType price_type,
            dict bid_quotes,
            dict ask_quotes
    ) except *:
        """
        Return the calculated exchange rate for the given price type using the
        given dictionary of bid and ask quotes.

        Parameters
        ----------
        from_currency : Currency
            The currency to convert from.
        to_currency : Currency
            The currency to convert to.
        price_type : PriceType
            The price type for conversion.
        bid_quotes : dict
            The dictionary of currency pair bid quotes Dict[str, double].
        ask_quotes : dict
            The dictionary of currency pair ask quotes Dict[str, double].

        Returns
        -------
        double

        Raises
        ------
        ValueError
            If bid_quotes length is not equal to ask_quotes length.
        ValueError
            If price_type is UNDEFINED or LAST.

        Notes
        -----
        If insufficient data to calculate exchange rate then will return 0.

        """
        Condition.not_none(from_currency, "from_currency")
        Condition.not_none(to_currency, "to_currency")
        Condition.not_none(bid_quotes, "bid_quotes")
        Condition.not_none(ask_quotes, "ask_quotes")
        Condition.true(price_type != PriceType.UNDEFINED and price_type != PriceType.LAST, "price_type not UNDEFINED or LAST")

        if from_currency == to_currency:
            return 1.  # No conversion necessary

        if price_type == PriceType.BID:
            calculation_quotes = bid_quotes
        elif price_type == PriceType.ASK:
            calculation_quotes = ask_quotes
        elif price_type == PriceType.MID:
            calculation_quotes = {
                s: (bid_quotes[s] + ask_quotes[s]) / 2.0 for s in bid_quotes
            }  # type: {str, float}
        else:
            raise ValueError(f"Cannot calculate exchange rate for price type "
                             f"{price_type_to_string(price_type)}")

        cdef str symbol
        cdef double quote
        cdef tuple pieces
        cdef str code_lhs
        cdef str code_rhs
        cdef set codes = set()
        cdef dict exchange_rates = {}

        # Build quote table
        for symbol, quote in calculation_quotes.items():
            # Get symbol codes
            pieces = symbol.partition('/')
            code_lhs = pieces[0]
            code_rhs = pieces[2]
            codes.add(code_lhs)
            codes.add(code_rhs)

            # Add currency dictionaries if they do not already exist
            if code_lhs not in exchange_rates:
                exchange_rates[code_lhs] = {}
            if code_rhs not in exchange_rates:
                exchange_rates[code_rhs] = {}
            # Add currency rates
            exchange_rates[code_lhs][code_lhs] = 1.
            exchange_rates[code_rhs][code_rhs] = 1.
            exchange_rates[code_lhs][code_rhs] = quote

        # Generate possible currency pairs from all symbols
        cdef set code_perms = set(permutations(codes, 2))

        # Calculate currency inverses
        cdef dict exchange_rates_perm0
        cdef dict exchange_rates_perm1
        for perm in code_perms:
            exchange_rates_perm0 = exchange_rates.get(perm[0])
            exchange_rates_perm1 = exchange_rates.get(perm[1])
            if exchange_rates_perm0 is None or exchange_rates_perm1 is None:
                continue
            if perm[0] not in exchange_rates_perm1:
                # Search for inverse
                if perm[1] in exchange_rates_perm0:
                    exchange_rates_perm1[perm[0]] = 1. / exchange_rates_perm0[perm[1]]
            if perm[1] not in exchange_rates_perm0:
                # Search for inverse
                if perm[0] in exchange_rates_perm1:
                    exchange_rates_perm0[perm[1]] = 1. / exchange_rates_perm1[perm[0]]

        cdef double xrate
        cdef dict quotes = exchange_rates.get(from_currency.code)
        if quotes is not None:
            xrate = quotes.get(to_currency.code, 0)
            if xrate > 0:
                return xrate

        # Exchange rate not yet calculated
        # Continue to calculate remaining exchange rates
        cdef double common_rate1
        cdef double common_rate2
        cdef dict exchange_rates_code
        for perm in code_perms:
            if perm[0] in exchange_rates[perm[1]]:
                continue
            # Search for common currency
            for code in codes:
                exchange_rates_perm0 = exchange_rates.get(perm[0])
                exchange_rates_perm1 = exchange_rates.get(perm[1])
                exchange_rates_code = exchange_rates.get(code)
                if exchange_rates_perm0 is None or exchange_rates_perm1 is None or exchange_rates_code is None:
                    continue
                if code in exchange_rates_perm0 and code in exchange_rates_perm1:
                    common_rate1 = exchange_rates_perm0[code]
                    common_rate2 = exchange_rates_perm1[code]
                    exchange_rates_perm1[perm[0]] = common_rate2 / common_rate1
                    # Check inverse and calculate if not found
                    if perm[1] not in exchange_rates_perm0:
                        exchange_rates_perm0[perm[1]] = common_rate1 / common_rate2
                elif perm[0] in exchange_rates_code and perm[1] in exchange_rates_code:
                    common_rate1 = exchange_rates_code[perm[0]]
                    common_rate2 = exchange_rates_code[perm[1]]
                    exchange_rates_perm1[perm[0]] = common_rate2 / common_rate1
                    # Check inverse and calculate if not found
                    if perm[1] not in exchange_rates[perm[0]]:
                        exchange_rates_perm0[perm[1]] = common_rate1 / common_rate2

        quotes = exchange_rates.get(from_currency.code)
        if quotes is None:
            # Not enough data
            return 0

        return quotes.get(to_currency.code, 0)


cdef class RolloverInterestCalculator:
    """
    Provides rollover interest rate calculations.

    If rate_data_csv_path is empty then will default to the included short-term
    interest rate data csv (data since 1956).
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
            short_term_interest_csv_path = os.path.join(
                PACKAGE_ROOT + "/_internal/rates/", "short-term-interest.csv"
            )

        self._exchange_calculator = ExchangeRateCalculator()

        csv_rate_data = pd.read_csv(short_term_interest_csv_path)
        self._rate_data = {
            'AUD': csv_rate_data.loc[csv_rate_data['LOCATION'] == 'AUS'],
            'CAD': csv_rate_data.loc[csv_rate_data['LOCATION'] == 'CAN'],
            'CHF': csv_rate_data.loc[csv_rate_data['LOCATION'] == 'CHE'],
            'EUR': csv_rate_data.loc[csv_rate_data['LOCATION'] == 'EA19'],
            'USD': csv_rate_data.loc[csv_rate_data['LOCATION'] == 'USA'],
            'JPY': csv_rate_data.loc[csv_rate_data['LOCATION'] == 'JPN'],
            'NZD': csv_rate_data.loc[csv_rate_data['LOCATION'] == 'NZL'],
            'GBP': csv_rate_data.loc[csv_rate_data['LOCATION'] == 'GBR'],
            'RUB': csv_rate_data.loc[csv_rate_data['LOCATION'] == 'RUS'],
            'NOK': csv_rate_data.loc[csv_rate_data['LOCATION'] == 'NOR'],
            'CNY': csv_rate_data.loc[csv_rate_data['LOCATION'] == 'CHN'],
            'CNH': csv_rate_data.loc[csv_rate_data['LOCATION'] == 'CHN'],
            'MXN': csv_rate_data.loc[csv_rate_data['LOCATION'] == 'MEX'],
            'ZAR': csv_rate_data.loc[csv_rate_data['LOCATION'] == 'ZAF'],
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

        cdef str base_currency = symbol.code[:3]
        cdef str quote_currency = symbol.code[-3:]
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
