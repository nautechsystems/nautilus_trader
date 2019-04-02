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
from itertools import combinations

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

        # cdef str ccy_pair = currency_string(quote_currency) + currency_string(base_currency)
        # cdef str swapped_ccy_pair = currency_string(base_currency) + currency_string(quote_currency)
        # cdef dict calculation_rates

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
        for ccy_pair, rate in calculation_rates.items():
            symbol_lhs = ccy_pair[:3]
            symbol_rhs = ccy_pair[3:]
            symbols.add(symbol_lhs)
            symbols.add(symbol_rhs)
            exchange_rates[symbol_lhs] = { symbol_lhs: 1.0,
                                           symbol_rhs: rate }
            exchange_rates[symbol_rhs] = { symbol_rhs: 1.0 }

        for symbol in symbols:
            if symbol not in exchange_rates[symbol]:
                exchange_rates[symbol_rhs + symbol_lhs] = 1.0 / rate

        cdef list possible_pairs = list(combinations(symbols, 2))
        print(possible_pairs)



        # if ccy_pair in calculation_rates:
        #     return calculation_rates[ccy_pair]
        # elif swapped_ccy_pair in calculation_rates:
        #     return 1.0 / calculation_rates[swapped_ccy_pair]
        # else:
        #     raise ValueError(f"Cannot calculate exchange rate - cannot find rate for {ccy_pair} or {swapped_ccy_pair}.")
