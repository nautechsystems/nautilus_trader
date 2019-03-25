#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_currency.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from decimal import Decimal

from inv_trader.enums.currency_code import CurrencyCode
from inv_trader.enums.quote_type import QuoteType
from inv_trader.model.currency import CurrencyCalculator


class CurrencyConverterTests(unittest.TestCase):

    def test_can_calculate_exchange_rate(self):
        # Arrange
        converter = CurrencyCalculator()
        bid_rates = {'AUDUSD': 0.80000}
        ask_rates = {'AUDUSD': 0.80010}

        # Act
        result = converter.exchange_rate(
            CurrencyCode.AUD,
            CurrencyCode.USD,
            QuoteType.BID,
            bid_rates,
            ask_rates)

        # Assert
        self.assertEqual(0.800000011920929, result)

    def test_can_calculate_exchange_rate_when_rate_needs_swapping(self):
        # Arrange
        converter = CurrencyCalculator()
        bid_rates = {'USDJPY': 110.100}
        ask_rates = {'USDJPY': 110.130}

        # Act
        result = converter.exchange_rate(
            CurrencyCode.JPY,
            CurrencyCode.USD,
            QuoteType.BID,
            bid_rates,
            ask_rates)

        # Assert
        self.assertEqual(0.009082651697099209, result)
