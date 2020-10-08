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
    Provides exchange rate calculations between currencies. An exchange rate is
    the value of one nation or economic zones currency versus that of another.
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
        Return the calculated exchange rate for the given quote currency to the
        given base currency for the given price type using the given dictionary of
        bid and ask rates.

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
            return 1.0  # No exchange necessary

        if price_type == PriceType.BID:
            calculation_rates = bid_quotes
        elif price_type == PriceType.ASK:
            calculation_rates = ask_quotes
        elif price_type == PriceType.MID:
            calculation_rates = {}  # type: {str, float}
            for ccy_pair in bid_quotes.keys():
                calculation_rates[ccy_pair] = (bid_quotes[ccy_pair] + ask_quotes[ccy_pair]) / 2.0
        else:
            raise ValueError(f"Cannot calculate exchange rate for price type {price_type_to_string(price_type)}")

        cdef dict exchange_rates = {}
        cdef set symbols = set()
        cdef str symbol_lhs
        cdef str symbol_rhs

        # Add given currency rates
        for ccy_pair, rate in calculation_rates.items():
            # Get currency pair symbols
            symbol_lhs = ccy_pair[:3]
            symbol_rhs = ccy_pair[-3:]
            symbols.add(symbol_lhs)
            symbols.add(symbol_rhs)
            # Add currency dictionaries if they do not already exist
            if symbol_lhs not in exchange_rates:
                exchange_rates[symbol_lhs] = {}
            if symbol_rhs not in exchange_rates:
                exchange_rates[symbol_rhs] = {}
            # Add currency rates
            exchange_rates[symbol_lhs][symbol_lhs] = 1.0
            exchange_rates[symbol_rhs][symbol_rhs] = 1.0
            exchange_rates[symbol_lhs][symbol_rhs] = rate

        # Generate possible currency pairs from all symbols
        cdef list possible_pairs = list(permutations(symbols, 2))

        # Calculate currency inverses
        for ccy_pair in possible_pairs:
            if ccy_pair[0] not in exchange_rates[ccy_pair[1]]:
                # Search for inverse
                if ccy_pair[1] in exchange_rates[ccy_pair[0]]:
                    exchange_rates[ccy_pair[1]][ccy_pair[0]] = 1.0 / exchange_rates[ccy_pair[0]][ccy_pair[1]]
            if ccy_pair[1] not in exchange_rates[ccy_pair[0]]:
                # Search for inverse
                if ccy_pair[0] in exchange_rates[ccy_pair[1]]:
                    exchange_rates[ccy_pair[0]][ccy_pair[1]] = 1.0 / exchange_rates[ccy_pair[1]][ccy_pair[0]]

        cdef str symbol
        cdef str lhs_str = from_currency.code
        cdef str rhs_str = to_currency.code
        try:
            return exchange_rates[lhs_str][rhs_str]
        except KeyError:
            pass  # Exchange rate not yet calculated

        # Continue to calculate remaining currency rates
        cdef double common_ccy1
        cdef double common_ccy2
        for ccy_pair in possible_pairs:
            if ccy_pair[0] not in exchange_rates[ccy_pair[1]]:
                # Search for common currency
                for symbol in symbols:
                    if symbol in exchange_rates[ccy_pair[0]] and symbol in exchange_rates[ccy_pair[1]]:
                        common_ccy1 = exchange_rates[ccy_pair[0]][symbol]
                        common_ccy2 = exchange_rates[ccy_pair[1]][symbol]
                        exchange_rates[ccy_pair[1]][ccy_pair[0]] = common_ccy2 / common_ccy1
                        # Check inverse and calculate if not found
                        if ccy_pair[1] not in exchange_rates[ccy_pair[0]]:
                            exchange_rates[ccy_pair[0]][ccy_pair[1]] = common_ccy1 / common_ccy2
                    elif ccy_pair[0] in exchange_rates[symbol] and ccy_pair[1] in exchange_rates[symbol]:
                        common_ccy1 = exchange_rates[symbol][ccy_pair[0]]
                        common_ccy2 = exchange_rates[symbol][ccy_pair[1]]
                        exchange_rates[ccy_pair[1]][ccy_pair[0]] = common_ccy2 / common_ccy1
                        # Check inverse and calculate if not found
                        if ccy_pair[1] not in exchange_rates[ccy_pair[0]]:
                            exchange_rates[ccy_pair[0]][ccy_pair[1]] = common_ccy1 / common_ccy2
        try:
            return exchange_rates[lhs_str][rhs_str]
        except KeyError:
            raise ValueError(f"Cannot calculate exchange rate for {lhs_str}{rhs_str} or {rhs_str}{lhs_str} "
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
