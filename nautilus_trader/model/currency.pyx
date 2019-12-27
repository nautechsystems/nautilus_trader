# -------------------------------------------------------------------------------------------------
# <copyright file="currency.pyx" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

from typing import Dict
from itertools import permutations

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.c_enums.currency cimport Currency, currency_to_string
from nautilus_trader.model.c_enums.quote_type cimport QuoteType, quote_type_to_string


cdef class ExchangeRateCalculator:
    """
    Provides exchange rate calculations between currencies. An exchange rate is
    the value of one nation or economic zones currency versus that of another.
    """

    cpdef float get_rate(
            self,
            Currency from_currency,
            Currency to_currency,
            QuoteType quote_type,
            dict bid_rates,
            dict ask_rates) except *:
        """
        Return the calculated exchange rate for the given quote currency to the 
        given base currency for the given quote type using the given dictionary of 
        bid and ask rates.

        :param from_currency: The currency to convert from.
        :param to_currency: The currency to convert to.
        :param quote_type: The quote type for conversion.
        :param bid_rates: The dictionary of currency pair bid rates Dict[str, float].
        :param ask_rates: The dictionary of currency pair ask rates Dict[str, float].
        :return float.
        :raises ConditionFailed: If the bid rates length is not equal to the ask rates length.
        """
        Condition.equal_length(bid_rates, ask_rates, 'bid_rates', 'ask_rates')

        if from_currency == to_currency:
            return 1.0  # No exchange necessary

        if quote_type == QuoteType.BID:
            calculation_rates = bid_rates
        elif quote_type == QuoteType.ASK:
            calculation_rates = ask_rates
        elif quote_type == QuoteType.MID:
            calculation_rates = {}  # type: Dict[str, float]
            for ccy_pair in bid_rates.keys():
                calculation_rates[ccy_pair] = (bid_rates[ccy_pair] + ask_rates[ccy_pair]) / 2.0
        else:
            raise ValueError(f"Cannot calculate exchange rate for quote type {quote_type_to_string(quote_type)}.")

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
        cdef float exchange_rate
        try:
            return exchange_rates[lhs_str][rhs_str]
        except KeyError:
            pass # Exchange rate not yet calculated

        # Continue to calculate remaining currency rates
        cdef float common_ccy1
        cdef float common_ccy2
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
