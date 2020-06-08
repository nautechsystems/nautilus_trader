# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import unittest

from nautilus_trader.model.enums import Currency, PriceType
from nautilus_trader.model.currency import ExchangeRateCalculator


class ExchangeRateCalculatorTests(unittest.TestCase):

    def test_get_rate_when_no_currency_rate_raises(self):
        # Arrange
        converter = ExchangeRateCalculator()
        bid_rates = {'AUDUSD': 0.80000}
        ask_rates = {'AUDUSD': 0.80010}

        # Act
        # Assert
        self.assertRaises(ValueError,
                          converter.get_rate,
                          Currency.USD,
                          Currency.JPY,
                          PriceType.BID,
                          bid_rates,
                          ask_rates)

    def test_can_calculate_exchange_rate(self):
        # Arrange
        converter = ExchangeRateCalculator()
        bid_rates = {'AUDUSD': 0.80000}
        ask_rates = {'AUDUSD': 0.80010}

        # Act
        result = converter.get_rate(
            Currency.AUD,
            Currency.USD,
            PriceType.BID,
            bid_rates,
            ask_rates)

        # Assert
        self.assertEqual(0.8, result)

    def test_calculate_exchange_rate_for_inverse(self):
        # Arrange
        converter = ExchangeRateCalculator()
        bid_rates = {'USDJPY': 110.100}
        ask_rates = {'USDJPY': 110.130}

        # Act
        result = converter.get_rate(
            Currency.JPY,
            Currency.USD,
            PriceType.BID,
            bid_rates,
            ask_rates)

        # Assert
        self.assertEqual(0.009082652134423252, result)

    def test_calculate_exchange_rate_by_inference(self):
        # Arrange
        converter = ExchangeRateCalculator()
        bid_rates = {
            'USDJPY': 110.100,
            'AUDUSD': 0.80000
        }
        ask_rates = {
            'USDJPY': 110.130,
            'AUDUSD': 0.80010}

        # Act
        result1 = converter.get_rate(
            Currency.JPY,
            Currency.AUD,
            PriceType.BID,
            bid_rates,
            ask_rates)

        result2 = converter.get_rate(
            Currency.AUD,
            Currency.JPY,
            PriceType.ASK,
            bid_rates,
            ask_rates)

        # Assert
        self.assertEqual(0.011353315168029064, result1)  # JPYAUD
        self.assertEqual(88.11501299999999, result2)  # AUDJPY

    def test_calculate_exchange_rate_for_mid_quote_type(self):
        # Arrange
        converter = ExchangeRateCalculator()
        bid_rates = {'USDJPY': 110.100}
        ask_rates = {'USDJPY': 110.130}

        # Act
        result = converter.get_rate(
            Currency.JPY,
            Currency.USD,
            PriceType.MID,
            bid_rates,
            ask_rates)

        # Assert
        self.assertEqual(0.009081414884438995, result)

    def test_calculate_exchange_rate_for_mid_quote_type2(self):
        # Arrange
        converter = ExchangeRateCalculator()
        bid_rates = {'USDJPY': 110.100}
        ask_rates = {'USDJPY': 110.130}

        # Act
        result = converter.get_rate(
            Currency.USD,
            Currency.JPY,
            PriceType.MID,
            bid_rates,
            ask_rates)

        # Assert
        self.assertEqual(110.115, result)
