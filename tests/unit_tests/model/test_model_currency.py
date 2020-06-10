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
