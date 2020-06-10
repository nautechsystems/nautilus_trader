# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/gpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from itertools import permutations

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.currency cimport Currency, currency_to_string
from nautilus_trader.model.c_enums.price_type cimport PriceType, price_type_to_string


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
            dict bid_rates,
            dict ask_rates) except *:
        """
        Return the calculated exchange rate for the given quote currency to the 
        given base currency for the given price type using the given dictionary of 
        bid and ask rates.

        :param from_currency: The currency to convert from.
        :param to_currency: The currency to convert to.
        :param price_type: The price type for conversion.
        :param bid_rates: The dictionary of currency pair bid rates Dict[str, double].
        :param ask_rates: The dictionary of currency pair ask rates Dict[str, double].
        :raises ValueError: If the bid rates length is not equal to the ask rates length.
        :return double.
        """
        Condition.not_none(bid_rates, 'bid_rates')
        Condition.not_none(ask_rates, 'ask_rates')
        Condition.equal(len(bid_rates), len(ask_rates), 'len(bid_rates)', 'len(ask_rates)')

        if from_currency == to_currency:
            return 1.0  # No exchange necessary

        if price_type == PriceType.BID:
            calculation_rates = bid_rates
        elif price_type == PriceType.ASK:
            calculation_rates = ask_rates
        elif price_type == PriceType.MID:
            calculation_rates = {}  # type: {str, float}
            for ccy_pair in bid_rates.keys():
                calculation_rates[ccy_pair] = (bid_rates[ccy_pair] + ask_rates[ccy_pair]) / 2.0
        else:
            raise ValueError(f"Cannot calculate exchange rate for price type {price_type_to_string(price_type)}.")

        cdef dict exchange_rates = {}
        cdef set symbols = set()
        cdef str symbol_lhs
        cdef str symbol_rhs

        # Add given currency rates
        for ccy_pair, rate in calculation_rates.items():
            # Get currency pair symbols
            symbol_lhs = ccy_pair[:3]
            symbol_rhs = ccy_pair[3:]
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

        cdef str lhs_str = currency_to_string(from_currency)
        cdef str rhs_str = currency_to_string(to_currency)
        cdef double exchange_rate
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
                             f"(not enough data).")
