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

import unittest
import datetime

from nautilus_trader.model.enums import Currency
from nautilus_trader.model.objects import Quantity, Money, Price
from nautilus_trader.common.brokerage import CommissionCalculator, RolloverInterestCalculator

from tests.test_kit.stubs import TestStubs, UNIX_EPOCH

AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()
GBPUSD_FXCM = TestStubs.symbol_gbpusd_fxcm()
USDJPY_FXCM = TestStubs.symbol_usdjpy_fxcm()


class CommissionCalculatorTests(unittest.TestCase):

    def test_can_calculate_correct_commission(self):
        # Arrange
        calculator = CommissionCalculator()

        # Act
        result = calculator.calculate(
            GBPUSD_FXCM,
            Quantity(1000000),
            filled_price=Price(1.63000, 5),
            exchange_rate=1.00,
            currency=Currency.USD)

        # Assert
        self.assertEqual(Money(32.60, Currency.USD), result)

    def test_can_calculate_correct_minimum_commission(self):
        # Arrange
        calculator = CommissionCalculator()

        # Act
        result = calculator.calculate_for_notional(GBPUSD_FXCM, Money(1000, Currency.USD))

        # Assert
        self.assertEqual(Money(2.00, Currency.USD), result)

    def test_can_calculate_correct_commission_for_notional(self):
        # Arrange
        calculator = CommissionCalculator()

        # Act
        result = calculator.calculate_for_notional(GBPUSD_FXCM, Money(1000000, Currency.USD))

        # Assert
        self.assertEqual(Money(20.00, Currency.USD), result)

    def test_can_calculate_correct_commission_with_exchange_rate(self):
        # Arrange
        calculator = CommissionCalculator()

        # Act
        result = calculator.calculate(
            USDJPY_FXCM,
            Quantity(1000000),
            filled_price=Price(95.000, 3),
            exchange_rate=0.01052632,
            currency=Currency.USD)

        # Assert
        self.assertEqual(Money(20.00, Currency.USD), result)


class RolloverInterestCalculatorTests(unittest.TestCase):

    def test_rate_dataframe_returns_correct_dataframe(self):
        # Arrange
        calculator = RolloverInterestCalculator()

        # Act
        rate_data = calculator.get_rate_data()

        # Assert
        self.assertEqual(dict, type(rate_data))

    def test_calc_overnight_fx_rate_with_audusd_on_unix_epoch_returns_correct_rate(self):
        # Arrange
        calculator = RolloverInterestCalculator()

        # Act
        rate = calculator.calc_overnight_rate(AUDUSD_FXCM, UNIX_EPOCH)

        # Assert
        self.assertEqual(-8.52054794520548e-05, rate)

    def test_calc_overnight_fx_rate_with_audusd_on_later_date_returns_correct_rate(self):
        # Arrange
        calculator = RolloverInterestCalculator()

        # Act
        rate = calculator.calc_overnight_rate(AUDUSD_FXCM, datetime.date(2018, 2, 1))

        # Assert
        self.assertEqual(-2.739726027397263e-07, rate)

    def test_calc_overnight_fx_rate_with_audusd_on_impossible_dates_returns_zero(self):
        # Arrange
        calculator = RolloverInterestCalculator()

        # Act
        # Assert
        self.assertRaises(RuntimeError, calculator.calc_overnight_rate, AUDUSD_FXCM, datetime.date(1900, 1, 1))
        self.assertRaises(RuntimeError, calculator.calc_overnight_rate, AUDUSD_FXCM, datetime.date(2020, 1, 1))
