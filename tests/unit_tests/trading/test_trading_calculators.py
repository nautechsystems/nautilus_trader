# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import datetime
from decimal import Decimal
import os
import unittest

import pandas as pd

from nautilus_trader.model.currencies import AUD
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import JPY
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import PriceType
from nautilus_trader.trading.calculators import ExchangeRateCalculator
from nautilus_trader.trading.calculators import RolloverInterestCalculator
from tests.test_kit import PACKAGE_ROOT
from tests.test_kit.stubs import TestStubs
from tests.test_kit.stubs import UNIX_EPOCH


AUDUSD_SIM = TestStubs.audusd_id()
GBPUSD_SIM = TestStubs.gbpusd_id()
USDJPY_SIM = TestStubs.usdjpy_id()


class ExchangeRateCalculatorTests(unittest.TestCase):
    def test_get_rate_when_price_type_last_raises_value_error(self):
        # Arrange
        converter = ExchangeRateCalculator()
        bid_rates = {"AUD/USD": Decimal("0.80000")}
        ask_rates = {"AUD/USD": Decimal("0.80010")}

        # Act
        # Assert
        self.assertRaises(
            ValueError,
            converter.get_rate,
            USD,
            JPY,
            PriceType.LAST,
            bid_rates,
            ask_rates,
        )

    def test_get_rate_when_from_currency_equals_to_currency_returns_one(self):
        # Arrange
        converter = ExchangeRateCalculator()
        bid_rates = {"AUD/USD": Decimal("0.80000")}
        ask_rates = {"AUD/USD": Decimal("0.80010")}

        # Act
        result = converter.get_rate(
            USD,
            USD,
            PriceType.BID,
            bid_rates,
            ask_rates,
        )

        # Assert
        self.assertEqual(1, result)

    def test_get_rate_when_no_currency_rate_returns_zero(self):
        # Arrange
        converter = ExchangeRateCalculator()
        bid_rates = {"AUD/USD": Decimal("0.80000")}
        ask_rates = {"AUD/USD": Decimal("0.80010")}

        # Act
        result = converter.get_rate(
            USD,
            JPY,
            PriceType.BID,
            bid_rates,
            ask_rates,
        )

        # Assert
        self.assertEqual(0, result)

    def test_get_rate(self):
        # Arrange
        converter = ExchangeRateCalculator()
        bid_rates = {"AUD/USD": Decimal("0.80000")}
        ask_rates = {"AUD/USD": Decimal("0.80010")}

        # Act
        result = converter.get_rate(
            AUD,
            USD,
            PriceType.BID,
            bid_rates,
            ask_rates,
        )

        # Assert
        self.assertEqual(Decimal("0.80000"), result)

    def test_get_rate_when_symbol_has_slash(self):
        # Arrange
        converter = ExchangeRateCalculator()
        bid_rates = {"AUD/USD": Decimal("0.80000")}
        ask_rates = {"AUD/USD": Decimal("0.80010")}

        # Act
        result = converter.get_rate(
            AUD,
            USD,
            PriceType.BID,
            bid_rates,
            ask_rates,
        )

        # Assert
        self.assertEqual(Decimal("0.80000"), result)

    def test_get_rate_for_inverse1(self):
        # Arrange
        converter = ExchangeRateCalculator()
        bid_rates = {"BTC/USD": Decimal("10501.5")}
        ask_rates = {"BTC/USD": Decimal("10500.0")}

        # Act
        result = converter.get_rate(
            USD,
            BTC,
            PriceType.BID,
            bid_rates,
            ask_rates,
        )

        # Assert
        self.assertEqual(Decimal("0.00009522449173927534161786411465"), result)

    def test_get_rate_for_inverse2(self):
        # Arrange
        converter = ExchangeRateCalculator()
        bid_rates = {"USD/JPY": Decimal("110.100")}
        ask_rates = {"USD/JPY": Decimal("110.130")}

        # Act
        result = converter.get_rate(
            JPY,
            USD,
            PriceType.BID,
            bid_rates,
            ask_rates,
        )

        # Assert
        self.assertAlmostEqual(Decimal("0.009082652"), result)

    def test_calculate_exchange_rate_by_inference(self):
        # Arrange
        converter = ExchangeRateCalculator()
        bid_rates = {
            "USD/JPY": Decimal("110.100"),
            "AUD/USD": Decimal("0.80000"),
        }
        ask_rates = {
            "USD/JPY": Decimal("110.130"),
            "AUD/USD": Decimal("0.80010"),
        }

        # Act
        result1 = converter.get_rate(
            JPY,
            AUD,
            PriceType.BID,
            bid_rates,
            ask_rates,
        )

        result2 = converter.get_rate(
            AUD,
            JPY,
            PriceType.ASK,
            bid_rates,
            ask_rates,
        )

        # Assert
        self.assertAlmostEqual(Decimal("0.01135331516802906448683015441"), result1)  # JPYAUD
        self.assertAlmostEqual(Decimal("88.11501299999999999999999997"), result2)  # AUDJPY

    def test_calculate_exchange_rate_for_mid_price_type(self):
        # Arrange
        converter = ExchangeRateCalculator()
        bid_rates = {"USD/JPY": Decimal("110.100")}
        ask_rates = {"USD/JPY": Decimal("110.130")}

        # Act
        result = converter.get_rate(
            JPY,
            USD,
            PriceType.MID,
            bid_rates,
            ask_rates,
        )

        # Assert
        self.assertEqual(Decimal("0.009081414884438995595513781047"), result)

    def test_calculate_exchange_rate_for_mid_price_type2(self):
        # Arrange
        converter = ExchangeRateCalculator()
        bid_rates = {"USD/JPY": Decimal("110.100")}
        ask_rates = {"USD/JPY": Decimal("110.130")}

        # Act
        result = converter.get_rate(
            USD,
            JPY,
            PriceType.MID,
            bid_rates,
            ask_rates,
        )

        # Assert
        self.assertEqual(Decimal("110.115"), result)


class RolloverInterestCalculatorTests(unittest.TestCase):
    def setUp(self):
        # Fixture Setup
        self.data = pd.read_csv(os.path.join(PACKAGE_ROOT, "data", "short-term-interest.csv"))

    def test_rate_dataframe_returns_correct_dataframe(self):
        # Arrange
        calculator = RolloverInterestCalculator(data=self.data)

        # Act
        rate_data = calculator.get_rate_data()

        # Assert
        self.assertEqual(dict, type(rate_data))

    def test_calc_overnight_fx_rate_with_audusd_on_unix_epoch_returns_correct_rate(
        self,
    ):
        # Arrange
        calculator = RolloverInterestCalculator(data=self.data)

        # Act
        rate = calculator.calc_overnight_rate(AUDUSD_SIM, UNIX_EPOCH)

        # Assert
        self.assertEqual(-8.52054794520548e-05, rate)

    def test_calc_overnight_fx_rate_with_audusd_on_later_date_returns_correct_rate(
        self,
    ):
        # Arrange
        calculator = RolloverInterestCalculator(data=self.data)

        # Act
        rate = calculator.calc_overnight_rate(AUDUSD_SIM, datetime.date(2018, 2, 1))

        # Assert
        self.assertEqual(-2.739726027397263e-07, rate)

    def test_calc_overnight_fx_rate_with_audusd_on_impossible_dates_returns_zero(self):
        # Arrange
        calculator = RolloverInterestCalculator(data=self.data)

        # Act
        # Assert
        self.assertRaises(
            RuntimeError,
            calculator.calc_overnight_rate,
            AUDUSD_SIM,
            datetime.date(1900, 1, 1),
        )
        self.assertRaises(
            RuntimeError,
            calculator.calc_overnight_rate,
            AUDUSD_SIM,
            datetime.date(3000, 1, 1),
        )
