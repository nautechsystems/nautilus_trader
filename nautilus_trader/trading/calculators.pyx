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

        """
        Condition.not_none(bid_quotes, "bid_quotes")
        Condition.not_none(ask_quotes, "ask_quotes")
        Condition.equal(len(bid_quotes), len(ask_quotes), "len(bid_quotes)", "len(ask_quotes)")

        if from_currency == to_currency:
            return 1.  # No conversion necessary

        if price_type == PriceType.BID:
            calculation_quotes = bid_quotes
        elif price_type == PriceType.ASK:
            calculation_quotes = ask_quotes
        elif price_type == PriceType.MID:
            calculation_quotes = {s: (bid_quotes[s] + ask_quotes[s]) / 2.0 for s in bid_quotes}  # type: {str, float}
        else:
            raise ValueError(f"Cannot calculate exchange rate for price type {price_type_to_string(price_type)}")

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
            if '/' in symbol:
                pieces = symbol.partition('/')
                code_lhs = pieces[0]
                code_rhs = pieces[2]
            else:
                if len(symbol) != 6:
                    raise ValueError(f"Cannot parse symbol {symbol}")
                code_lhs = symbol[:3]
                code_rhs = symbol[-3:]

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
        for perm in code_perms:
            if perm[0] not in exchange_rates[perm[1]]:
                # Search for inverse
                if perm[1] in exchange_rates[perm[0]]:
                    exchange_rates[perm[1]][perm[0]] = 1. / exchange_rates[perm[0]][perm[1]]
            if perm[1] not in exchange_rates[perm[0]]:
                # Search for inverse
                if perm[0] in exchange_rates[perm[1]]:
                    exchange_rates[perm[0]][perm[1]] = 1. / exchange_rates[perm[1]][perm[0]]

        cdef dict crosses = exchange_rates.get(from_currency.code)
        if not crosses:
            # Not enough data
            raise self._cannot_calculate_exception(from_currency.code, to_currency.code)

        cdef double xrate = crosses.get(to_currency.code, -1)
        if xrate >= 0:
            return xrate

        # Exchange rate not yet calculated
        # Continue to calculate remaining exchange rates
        cdef double common_rate1
        cdef double common_rate2
        for perm in code_perms:
            if perm[0] in exchange_rates[perm[1]]:
                continue
            # Search for common currency
            for code in codes:
                if code in exchange_rates[perm[0]] and code in exchange_rates[perm[1]]:
                    common_rate1 = exchange_rates[perm[0]][code]
                    common_rate2 = exchange_rates[perm[1]][code]
                    exchange_rates[perm[1]][perm[0]] = common_rate2 / common_rate1
                    # Check inverse and calculate if not found
                    if perm[1] not in exchange_rates[perm[0]]:
                        exchange_rates[perm[0]][perm[1]] = common_rate1 / common_rate2
                elif perm[0] in exchange_rates[code] and perm[1] in exchange_rates[code]:
                    common_rate1 = exchange_rates[code][perm[0]]
                    common_rate2 = exchange_rates[code][perm[1]]
                    exchange_rates[perm[1]][perm[0]] = common_rate2 / common_rate1
                    # Check inverse and calculate if not found
                    if perm[1] not in exchange_rates[perm[0]]:
                        exchange_rates[perm[0]][perm[1]] = common_rate1 / common_rate2

        crosses = exchange_rates.get(from_currency.code)
        if not crosses:
            # Not enough data
            raise self._cannot_calculate_exception(from_currency.code, to_currency.code)

        xrate = crosses.get(to_currency.code, -1)
        if xrate >= 0:
            return xrate

        # Not enough data
        raise self._cannot_calculate_exception(from_currency.code, to_currency.code)

    cdef inline object _cannot_calculate_exception(self, str from_code, str to_code):
        return ValueError(f"Cannot calculate exchange rate for "
                         f"{from_code}{to_code} or "
                         f"{to_code}{from_code} "
                         f"(not enough data)")


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
            short_term_interest_csv_path = os.path.join(PACKAGE_ROOT + "/_internal/rates/", "short-term-interest.csv")
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
