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

from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Quantity
from nautilus_trader.trading.commission import GenericCommissionModel
from nautilus_trader.trading.commission import MakerTakerCommissionModel
from tests.test_kit.stubs import TestStubs

AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()
GBPUSD_FXCM = TestStubs.symbol_gbpusd_fxcm()
USDJPY_FXCM = TestStubs.symbol_usdjpy_fxcm()


class GenericCommissionModelTests(unittest.TestCase):

    def test_calculate_returns_correct_commission(self):
        # Arrange
        model = GenericCommissionModel()

        # Act
        result = model.calculate(
            GBPUSD_FXCM,
            Quantity(1000000),
            liquidity_side=LiquiditySide.TAKER,
            currency=USD,
        )

        # Assert
        self.assertEqual(Money(20.00, USD), result)

    def test_calculate_returns_correct_minimum_commission(self):
        # Arrange
        model = GenericCommissionModel(minimum=Money(2.00, USD))

        # Act
        result = model.calculate(
            GBPUSD_FXCM,
            Quantity(100),
            liquidity_side=LiquiditySide.TAKER,
            currency=USD,
        )

        # Assert
        self.assertEqual(Money(2.00, USD), result)


class MakerTakerCommissionModelTests(unittest.TestCase):

    def test_calculate_for_taker_returns_correct_commission(self):
        # Arrange
        model = MakerTakerCommissionModel()

        # Act
        result = model.calculate(
            GBPUSD_FXCM,
            Quantity(1000000),
            liquidity_side=LiquiditySide.TAKER,
            currency=USD,
        )

        # Assert
        self.assertEqual(Money(750.00, USD), result)

    def test_calculate_for_maker_returns_correct_commission(self):
        # Arrange
        model = MakerTakerCommissionModel()

        # Act
        result = model.calculate(
            GBPUSD_FXCM,
            Quantity(1000000),
            liquidity_side=LiquiditySide.MAKER,
            currency=USD,
        )

        # Assert
        self.assertEqual(Money(-250.00, USD), result)
