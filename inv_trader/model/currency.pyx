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

from inv_trader.core.precondition cimport Precondition
from inv_trader.enums.currency cimport Currency, currency_string
from inv_trader.enums.quote_type cimport QuoteType, quote_type_string


cdef class CurrencyCalculator:
    """
    Provides useful calculations between currencies.
    """

    cpdef float exchange_rate(
            self,
            Currency from_currency,
            Currency to_currency,
            QuoteType quote_type,
            dict bid_rates,
            dict ask_rates):
        """
        Return the calculated exchange rate for the given from-currency to the 
        given to-currency for the given quote type using the given dictionary of 
        bid and ask rates.

        :param from_currency: The currency to convert from.
        :param to_currency: The currency to convert to.
        :param quote_type: The quote type for conversion.
        :param bid_rates: The dictionary of currency pair bid rates (Dict[str, float]).
        :param ask_rates: The dictionary of currency pair ask rates (Dict[str, float]).
        :return: float.
        :raises ValueError: If the bid rates is not an equal length to the ask rates.
        """
        Precondition.true(len(bid_rates) == len(ask_rates), 'len(bid_rates) == len(ask_rates)')

        if from_currency == to_currency:
            return 1.0  # No exchange necessary

        cdef str ccy_pair = currency_string(from_currency) + currency_string(to_currency)
        cdef str swapped_ccy_pair = currency_string(to_currency) + currency_string(from_currency)
        cdef dict calculation_rates

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

        if ccy_pair in calculation_rates:
            return calculation_rates[ccy_pair]
        elif swapped_ccy_pair in calculation_rates:
            return 1.0 / calculation_rates[swapped_ccy_pair]
        else:
            raise ValueError(f"Cannot calculate exchange rate - cannot find rate for {ccy_pair} or {swapped_ccy_pair}.")
