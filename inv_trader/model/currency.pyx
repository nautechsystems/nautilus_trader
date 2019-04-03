#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="currency.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False

from typing import Dict
from itertools import permutations

from inv_trader.core.precondition cimport Precondition
from inv_trader.enums.currency cimport Currency, currency_string
from inv_trader.enums.quote_type cimport QuoteType, quote_type_string


cdef class CurrencyCalculator:
    """
    Provides useful calculations between currencies.
    """

    cpdef float exchange_rate(
            self,
            Currency quote_currency,
            Currency base_currency,
            QuoteType quote_type,
            dict bid_rates,
            dict ask_rates):
        """
        Return the calculated exchange rate for the given quote currency to the 
        given base currency for the given quote type using the given dictionary of 
        bid and ask rates.

        :param quote_currency: The quote currency to convert from.
        :param base_currency: The base currency to convert to.
        :param quote_type: The quote type for conversion.
        :param bid_rates: The dictionary of currency pair bid rates (Dict[str, float]).
        :param ask_rates: The dictionary of currency pair ask rates (Dict[str, float]).
        :return: float.
        :raises ValueError: If the bid rates length is not equal to the ask rates length.
        """
        Precondition.true(len(bid_rates) == len(ask_rates), 'len(bid_rates) == len(ask_rates)')

        if quote_currency == base_currency:
            return 1.0  # No exchange necessary

        if quote_type == QuoteType.BID:
            calculation_rates = bid_rates
        elif quote_type == QuoteType.ASK:
            calculation_rates = ask_rates
        elif quote_type == QuoteType.MID:
            calculation_rates = {}  # type: Dict[str, float]
            for symbol in bid_rates.keys():
                calculation_rates[symbol] = (bid_rates[symbol] + ask_rates[symbol]) / 2.0
        else:
            raise ValueError(f"Cannot calculate exchange rate for quote type {quote_type_string(quote_type)}.")

        cdef dict exchange_rates = {}
        cdef set symbols = set()
        cdef str symbol_lhs
        cdef str symbol_rhs
        cdef float common_1
        cdef float common_2

        for ccy_pair, rate in calculation_rates.items():
            symbol_lhs = ccy_pair[:3]
            symbol_rhs = ccy_pair[3:]
            symbols.add(symbol_lhs)
            symbols.add(symbol_rhs)
            # Add dictionary if it doesnt exist
            if symbol_lhs not in exchange_rates:
                exchange_rates[symbol_lhs] = {}
            if symbol_rhs not in exchange_rates:
                exchange_rates[symbol_rhs] = {}
            # Add rates
            exchange_rates[symbol_lhs][symbol_lhs] = 1.0
            exchange_rates[symbol_rhs][symbol_rhs] = 1.0
            exchange_rates[symbol_lhs][symbol_rhs] = rate

        cdef list possible_pairs = list(permutations(symbols, 2))

        # Calculate inverses
        for ccy_pair in possible_pairs:
            if ccy_pair[0] not in exchange_rates[ccy_pair[1]]:
                # Search for inverse
                if ccy_pair[1] in exchange_rates[ccy_pair[0]]:
                    exchange_rates[ccy_pair[1]][ccy_pair[0]] = 1.0 / exchange_rates[ccy_pair[0]][ccy_pair[1]]
            if ccy_pair[1] not in exchange_rates[ccy_pair[0]]:
                # Search for inverse
                if ccy_pair[0] in exchange_rates[ccy_pair[1]]:
                    exchange_rates[ccy_pair[0]][ccy_pair[1]] = 1.0 / exchange_rates[ccy_pair[1]][ccy_pair[0]]

        # Calculate remaining rates
        for ccy_pair in possible_pairs:
            if ccy_pair[0] not in exchange_rates[ccy_pair[1]]:
                # Search for common currency
                for symbol in symbols:
                    if symbol in exchange_rates[ccy_pair[0]] and symbol in exchange_rates[ccy_pair[1]]:
                        common_1 = exchange_rates[ccy_pair[0]][symbol]
                        common_2 = exchange_rates[ccy_pair[1]][symbol]
                        exchange_rates[ccy_pair[1]][ccy_pair[0]] = common_2 / common_1
                        # Check inverse and calculate if not found
                        if ccy_pair[1] not in exchange_rates[ccy_pair[0]]:
                            exchange_rates[ccy_pair[0]][ccy_pair[1]] = common_1 / common_2
                    elif ccy_pair[0] in exchange_rates[symbol] and ccy_pair[1] in exchange_rates[symbol]:
                        common_1 = exchange_rates[symbol][ccy_pair[0]]
                        common_2 = exchange_rates[symbol][ccy_pair[1]]
                        exchange_rates[ccy_pair[1]][ccy_pair[0]] = common_2 / common_1
                        # Check inverse and calculate if not found
                        if ccy_pair[1] not in exchange_rates[ccy_pair[0]]:
                            exchange_rates[ccy_pair[0]][ccy_pair[1]] = common_1 / common_2

        cdef str lhs_str = currency_string(quote_currency)
        cdef str rhs_str = currency_string(base_currency)

        if rhs_str not in exchange_rates[lhs_str]:
            raise ValueError(f"Cannot calculate exchange rate for {lhs_str}{rhs_str}")

        return exchange_rates[lhs_str][rhs_str]
