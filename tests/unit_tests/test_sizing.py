#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_serialization.py" company="Invariance Pte">
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
        equity = Money('1000000')

        # Act
        result = self.sizer.calculate(
            equity,
            Decimal('1'),
            10,
            Price('1.00100'),
            Price('1.00000'))

        # Assert
        self.assertEqual(Quantity(0), result)

