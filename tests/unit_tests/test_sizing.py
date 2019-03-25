#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_sizing.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from inv_trader.model.objects import *
from inv_trader.portfolio.sizing import FixedRiskSizer
from test_kit.stubs import TestStubs


class FixedRiskSizerTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.sizer = FixedRiskSizer(TestStubs.instrument_gbpusd())

    def test_can_calculate_single_unit_size(self):
        # Arrange
        equity = Money(1000000)

        # Act
        result = self.sizer.calculate(
            equity,
            100,  # 1%
            Price('1.00100'),
            Price('1.00000'),
            exchange_rate=1.0,
            unit_batch_size=1000)

        # Assert
        self.assertEqual(Quantity(9985000), result)

    def test_can_calculate_single_unit_with_exchange_rate(self):
        # Arrange
        equity = Money(1000000)

        # Act
        result = self.sizer.calculate(
            equity,
            10,   # 0.1%
            Price('110.010'),
            Price('110.000'),
            exchange_rate=0.01,
            unit_batch_size=1000)

        # Assert
        self.assertEqual(Quantity(9985000), result)

    def test_can_calculate_single_unit_size_when_risk_too_high(self):
        # Arrange
        equity = Money(100000)

        # Act
        result = self.sizer.calculate(
            equity,
            100,   # 1%
            Price('3.00000'),
            Price('1.00000'),
            unit_batch_size=1000)

        # Assert
        self.assertEqual(Quantity(0), result)

    def test_can_impose_hard_limit(self):
        # Arrange
        equity = Money(1000000)

        # Act
        result = self.sizer.calculate(
            equity,
            100,   # 1%
            Price('1.00010'),
            Price('1.00000'),
            hard_limit=500000,
            units=1,
            unit_batch_size=1000)

        # Assert
        self.assertEqual(Quantity(500000), result)

    def test_can_calculate_multiple_unit_size(self):
        # Arrange
        equity = Money(1000000)

        # Act
        result = self.sizer.calculate(
            equity,
            10,   # 0.1%
            Price('1.00010'),
            Price('1.00000'),
            units=3,
            unit_batch_size=1000)

        # Assert
        self.assertEqual(Quantity(3328000), result)

    def test_can_calculate_multiple_unit_size_larger_batches(self):
        # Arrange
        equity = Money(1000000)

        # Act
        result = self.sizer.calculate(
            equity,
            10,   # 0.1%
            Price('1.00087'),
            Price('1.00000'),
            units=4,
            unit_batch_size=25000)

        # Assert
        self.assertEqual(Quantity(275000), result)
