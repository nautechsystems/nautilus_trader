# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from decimal import Decimal
from itertools import permutations

import pandas as pd

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.enums_c cimport PriceType
from nautilus_trader.model.enums_c cimport price_type_to_str
from nautilus_trader.model.identifiers cimport InstrumentId


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
        dict ask_quotes,
    ):
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
            The dictionary of currency pair bid quotes dict[Symbol, double].
        ask_quotes : dict
            The dictionary of currency pair ask quotes dict[Symbol, double].

        Returns
        -------
        Decimal

        Raises
        ------
        ValueError
            If `bid_quotes` length is not equal to `ask_quotes` length.
        ValueError
            If `price_type` is ``LAST``.

        Notes
        -----
        If insufficient data to calculate exchange rate then will return 0.

        """
        Condition.not_none(from_currency, "from_currency")
        Condition.not_none(to_currency, "to_currency")
        Condition.not_none(bid_quotes, "bid_quotes")
        Condition.not_none(ask_quotes, "ask_quotes")
        Condition.true(price_type != PriceType.LAST, "price_type was invalid (LAST)")

        if from_currency == to_currency:
            return 1.0  # No conversion necessary

        if price_type == PriceType.BID:
            calculation_quotes = bid_quotes
        elif price_type == PriceType.ASK:
            calculation_quotes = ask_quotes
        elif price_type == PriceType.MID:
            calculation_quotes = {
                s: (bid_quotes[s] + ask_quotes[s]) / 2.0 for s in bid_quotes
            }  # type: dict[str, Decimal]
        else:
            raise ValueError(f"Cannot calculate exchange rate for PriceType."
                             f"{price_type_to_str(price_type)}")

        cdef str symbol
        cdef tuple pieces
        cdef str code_lhs
        cdef str code_rhs
        cdef set codes = set()
        cdef dict exchange_rates = {}

        # Build quote table
        for symbol, quote in calculation_quotes.items():
            # Get instrument_id codes
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
            exchange_rates[code_lhs][code_lhs] = 1.0
            exchange_rates[code_rhs][code_rhs] = 1.0
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
                    exchange_rates_perm1[perm[0]] = 1.0 / exchange_rates_perm0[perm[1]]
            if perm[1] not in exchange_rates_perm0:
                # Search for inverse
                if perm[0] in exchange_rates_perm1:
                    exchange_rates_perm0[perm[1]] = 1.0 / exchange_rates_perm1[perm[0]]

        cdef dict quotes = exchange_rates.get(from_currency.code)
        if quotes:
            xrate = quotes.get(to_currency.code)
            if xrate is not None:
                return xrate

        # Exchange rate not yet calculated
        # Continue to calculate remaining exchange rates
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
            return 0.0

        return quotes.get(to_currency.code, 0.0)


cdef class RolloverInterestCalculator:
    """
    Provides rollover interest rate calculations.

    If rate_data_csv_path is empty then will default to the included short-term
    interest rate data csv (data since 1956).

    Parameters
    ----------
    data : str
        The short term interest rate data.
    """

    def __init__(self, data not None: pd.DataFrame):
        self._rate_data = {
            "AUD": data.loc[data["LOCATION"] == "AUS"],
            "CAD": data.loc[data["LOCATION"] == "CAN"],
            "CHF": data.loc[data["LOCATION"] == "CHE"],
            "EUR": data.loc[data["LOCATION"] == "EA19"],
            "USD": data.loc[data["LOCATION"] == "USA"],
            "JPY": data.loc[data["LOCATION"] == "JPN"],
            "NZD": data.loc[data["LOCATION"] == "NZL"],
            "GBP": data.loc[data["LOCATION"] == "GBR"],
            "RUB": data.loc[data["LOCATION"] == "RUS"],
            "NOK": data.loc[data["LOCATION"] == "NOR"],
            "CNY": data.loc[data["LOCATION"] == "CHN"],
            "CNH": data.loc[data["LOCATION"] == "CHN"],
            "MXN": data.loc[data["LOCATION"] == "MEX"],
            "ZAR": data.loc[data["LOCATION"] == "ZAF"],
        }

    cpdef object get_rate_data(self):
        """
        Return the short-term interest rate dataframe.

        Returns
        -------
        pd.DataFrame

        """
        return self._rate_data

    cpdef object calc_overnight_rate(self, InstrumentId instrument_id, date date):
        """
        Return the rollover interest rate between the given base currency and quote currency.

        Parameters
        ----------
        instrument_id : InstrumentId
            The forex instrument ID for the calculation.
        date : date
            The date for the overnight rate.

        Returns
        -------
        Decimal

        Raises
        ------
        ValueError
            If `instrument_id.symbol` length is not in range [6, 7].

        Notes
        -----
        1% = 0.01 bp

        """
        Condition.not_none(instrument_id, "instrument_id")
        Condition.not_none(date, "timestamp")
        Condition.in_range_int(len(instrument_id.symbol.to_str()), 6, 7, "len(instrument_id)")

        cdef str symbol = instrument_id.symbol.to_str()
        cdef str base_currency = symbol[:3]
        cdef str quote_currency = symbol[-3:]
        cdef str time_monthly = f"{date.year}-{str(date.month).zfill(2)}"
        cdef str time_quarter = f"{date.year}-Q{str(int(((date.month - 1) // 3) + 1)).zfill(2)}"

        base_data = self._rate_data[base_currency].loc[self._rate_data[base_currency]['TIME'] == time_monthly]
        if base_data.empty:
            base_data = self._rate_data[base_currency].loc[self._rate_data[base_currency]['TIME'] == time_quarter]

        quote_data = self._rate_data[quote_currency].loc[self._rate_data[quote_currency]['TIME'] == time_monthly]
        if quote_data.empty:
            quote_data = self._rate_data[quote_currency].loc[self._rate_data[quote_currency]['TIME'] == time_quarter]

        if base_data.empty and quote_data.empty:
            raise RuntimeError(f"cannot find rollover interest rate for {instrument_id} on {date}")  # pragma: no cover

        return Decimal(((<double>base_data['Value'] - <double>quote_data['Value']) / 365) / 100)
