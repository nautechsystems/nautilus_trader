# -------------------------------------------------------------------------------------------------
# <copyright file="test_common_brokerage.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import pandas as pd

from nautilus_trader.model.objects import Quantity, Money, Price
from nautilus_trader.common.brokerage import CommissionCalculator, RolloverInterestCalculator
from test_kit.stubs import TestStubs

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
            filled_price=Price('1.63000'),
            exchange_rate=1.00)

        # Assert
        self.assertEqual(Money(32.60), result)

    def test_can_calculate_correct_minimum_commission(self):
        # Arrange
        calculator = CommissionCalculator()

        # Act
        result = calculator.calculate_for_notional(
            GBPUSD_FXCM,
            Money(1000))

        # Assert
        self.assertEqual(Money(2.00), result)

    def test_can_calculate_correct_commission_for_notional(self):
        # Arrange
        calculator = CommissionCalculator()

        # Act
        result = calculator.calculate_for_notional(
            GBPUSD_FXCM,
            Money(1000000))

        # Assert
        self.assertEqual(Money(20.00), result)

    def test_can_calculate_correct_commission_with_exchange_rate(self):
        # Arrange
        calculator = CommissionCalculator()

        # Act
        result = calculator.calculate(
            USDJPY_FXCM,
            Quantity(1000000),
            filled_price=Price('95.000'),
            exchange_rate=0.01052632)

        # Assert
        self.assertEqual(Money(20.00), result)


class RolloverInterestCalculatorTests(unittest.TestCase):

    def test_rate_dataframe_returns_correct_dataframe(self):
        # Arrange
        calculator = RolloverInterestCalculator()

        # Act
        rate_dataframe = calculator.get_rate_data()

        # Assert
        self.assertEqual(pd.DataFrame, type(rate_dataframe))
        print(rate_dataframe)

    def test_calc_overnight_fx_rate_with_audusd_returns_correct_rate(self):
        # Arrange
        calculator = RolloverInterestCalculator()

        # Act
        rate = calculator.calc_overnight_fx_rate(AUDUSD_FXCM, TestStubs.unix_epoch())

        # Assert
        self.assertEqual(0, rate)
