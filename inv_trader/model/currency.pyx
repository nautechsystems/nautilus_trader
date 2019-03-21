#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="currency.pyx" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

# cython: language_level=3, boundscheck=False, wraparound=False, nonecheck=False


from inv_trader.enums.currency_code cimport CurrencyCode, currency_code_string
from inv_trader.enums.quote_type cimport QuoteType


cdef class CurrencyConverter:
    """
    Provides exchange rate calculations between currencies.
    """

    cpdef float exchange_rate(
            self,
            CurrencyCode from_currency,
            CurrencyCode to_currency,
            QuoteType quote_type,
            dict bid_rates,
            dict ask_rates):
        """
        Return the calculated exchange rate for the given from currency to the 
        given to currency for the given quote type using the provided
        dictionary of bid and ask rates.

        :param from_currency: The currency to convert from.
        :param to_currency: The currency to convert to.
        :param quote_type: The quote type for conversion.
        :param bid_rates: The dictionary of currency pair bid rates.
        :param ask_rates: The dictionary of currency pair ask rates.
        :return: float.
        """
        cdef str ccy_pair = currency_code_string(from_currency) + currency_code_string(to_currency)
        cdef str swapped_ccy_pair = currency_code_string(to_currency) + currency_code_string(from_currency)
        cdef dict calculation_rates

        if quote_type == QuoteType.BID:
            calculation_rates = bid_rates
        elif quote_type == QuoteType.ASK:
            calculation_rates = ask_rates
        elif quote_type == QuoteType.MID:
            calculation_rates = bid_rates + ask_rates / 2.0
        else:
            raise ValueError(f"Cannot calculate exchange rate for quote type {quote_type}")

        if ccy_pair in calculation_rates:
            return calculation_rates[ccy_pair]
        elif swapped_ccy_pair in calculation_rates:
            return 1 / calculation_rates[swapped_ccy_pair]
        else:
            raise ValueError(f"Cannot calculate exchange rate - cannot find rate for {ccy_pair} or {swapped_ccy_pair}")
