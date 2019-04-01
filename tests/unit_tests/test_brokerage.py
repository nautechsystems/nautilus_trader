#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_brokerage.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from inv_trader.model.objects import Quantity, Money
from inv_trader.common.brokerage import CommissionCalculator
from test_kit.stubs import TestStubs

GBPUSD_FXCM = TestStubs.instrument_gbpusd().symbol
USDJPY_FXCM = TestStubs.instrument_usdjpy().symbol


class CommissionCalculatorTests(unittest.TestCase):

    def test_can_calculate_correct_commission(self):
        # Arrange
        calculator = CommissionCalculator()

        # Act
        result = calculator.calculate(
            GBPUSD_FXCM,
            Quantity(1000000),
            exchange_rate=1.0)

        # Assert
        self.assertEqual(Money(15), result)

    def test_can_calculate_correct_commission_for_notional(self):
        # Arrange
        calculator = CommissionCalculator()

        # Act
        result = calculator.calculate_for_notional(
            GBPUSD_FXCM,
            Money(1000000))

        # Assert
        self.assertEqual(Money(15), result)

    def test_can_calculate_correct_commission_with_exchange_rate(self):
        # Arrange
        calculator = CommissionCalculator()

        # Act
        result = calculator.calculate(
            USDJPY_FXCM,
            Quantity(10000000),
            exchange_rate=0.01)

        # Assert
        self.assertEqual(Money(150.00), result)
