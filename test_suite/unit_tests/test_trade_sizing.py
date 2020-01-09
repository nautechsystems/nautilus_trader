# -------------------------------------------------------------------------------------------------
# <copyright file="test_trade_sizing.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from nautilus_trader.model.objects import Quantity, Price, Money
from nautilus_trader.trade.sizing import FixedRiskSizer
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
            10,  # 0.1%
            Price(1.00100, 5),
            Price(1.00000, 5),
            exchange_rate=1.0,
            unit_batch_size=1000)

        # Assert
        self.assertEqual(Quantity(999000), result)

    def test_can_calculate_single_unit_with_exchange_rate(self):
        # Arrange
        equity = Money(1000000)

        # Act
        result = self.sizer.calculate(
            equity,
            10,   # 0.1%
            Price(110.010, 3),
            Price(110.000, 3),
            exchange_rate=0.01,
            unit_batch_size=1000)

        # Assert
        self.assertEqual(Quantity(9999000), result)

    def test_can_calculate_single_unit_size_when_risk_too_high(self):
        # Arrange
        equity = Money(100000)

        # Act
        result = self.sizer.calculate(
            equity,
            100,   # 1%
            Price(3.00000, 5),
            Price(1.00000, 5),
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
            Price(1.00010, 5),
            Price(1.00000, 5),
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
            Price(1.00010, 5),
            Price(1.00000, 5),
            units=3,
            unit_batch_size=1000)

        # Assert
        self.assertEqual(Quantity(3333000), result)

    def test_can_calculate_multiple_unit_size_larger_batches(self):
        # Arrange
        equity = Money(1000000)

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

    def test_can_calculate_for_usdjpy(self):
        # Arrange
        sizer = FixedRiskSizer(TestStubs.instrument_usdjpy())
        equity = Money(1000000)

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
        self.assertEqual(Quantity(359000), result)
