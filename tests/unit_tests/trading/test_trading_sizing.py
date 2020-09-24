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

from nautilus_trader.model.enums import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.trading.sizing import FixedRiskSizer
from tests.test_kit.stubs import TestStubs


class FixedRiskSizerTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.sizer = FixedRiskSizer(TestStubs.instrument_gbpusd())

    def test_calculate_single_unit_size(self):
        # Arrange
        equity = Money(1000000, Currency.USD)

        # Act
        result = self.sizer.calculate(
            equity,
            10,  # 0.1%
            Price(1.00100, 5),
            Price(1.00000, 5),
            exchange_rate=1.0,
            unit_batch_size=1000)

        # Assert
        self.assertEqual(Quantity(1000000), result)

    def test_calculate_single_unit_with_exchange_rate(self):
        # Arrange
        equity = Money(1000000, Currency.USD)

        # Act
        result = self.sizer.calculate(
            equity,
            10,   # 0.1%
            Price(110.010, 3),
            Price(110.000, 3),
            exchange_rate=0.01)

        # Assert
        self.assertEqual(Quantity(10000000), result)

    def test_calculate_single_unit_size_when_risk_too_high(self):
        # Arrange
        equity = Money(100000, Currency.USD)

        # Act
        result = self.sizer.calculate(
            equity,
            100,   # 1%
            Price(3.00000, 5),
            Price(1.00000, 5),
            unit_batch_size=1000)

        # Assert
        self.assertEqual(Quantity(), result)

    def test_impose_hard_limit(self):
        # Arrange
        equity = Money(1000000, Currency.USD)

        # Act
        result = self.sizer.calculate(
            equity,
            100,   # 1%
            Price(1.00010, 5),
            Price(1.00000, 5),
            hard_limit=500000,
            units=1,
            unit_batch_size=1000)

        # Assert
        self.assertEqual(Quantity(500000), result)

    def test_calculate_multiple_unit_size(self):
        # Arrange
        equity = Money(1000000, Currency.USD)

        # Act
        result = self.sizer.calculate(
            equity,
            10,   # 0.1%
            Price(1.00010, 5),
            Price(1.00000, 5),
            units=3,
            unit_batch_size=1000)

        # Assert
        self.assertEqual(Quantity(3333000), result)

    def test_calculate_multiple_unit_size_larger_batches(self):
        # Arrange
        equity = Money(1000000, Currency.USD)

        # Act
        result = self.sizer.calculate(
            equity,
            10,   # 0.1%
            Price(1.00087, 5),
            Price(1.00000, 5),
            units=4,
            unit_batch_size=25000)

        # Assert
        self.assertEqual(Quantity(275000), result)

    def test_calculate_for_usdjpy(self):
        # Arrange
        sizer = FixedRiskSizer(TestStubs.instrument_usdjpy())
        equity = Money(1000000, Currency.USD)

        # Act
        result = sizer.calculate(
            equity,
            10,   # 0.1%
            Price(107.703, 3),
            Price(107.403, 3),
            exchange_rate=0.0093,
            units=1,
            unit_batch_size=1000)

        # Assert
        self.assertEqual(Quantity(358000), result)
