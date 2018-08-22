#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_objects.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from decimal import Decimal

from inv_trader.model.objects import Price


class PriceTests(unittest.TestCase):

    def test_create_price_with_zero_price_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, Price.create, 0.0, 0)

    def test_create_price_with_negative_decimals_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, Price.create, 1.00000, -1)

    def test_create_price_with_valid_inputs_returns_expected_decimal_object(self):
        # Arrange
        # Act
        result1 = Price.create(1.00000, 5)
        result2 = Price.create(1.0, 0)
        result3 = Price.create(1.001, 2)

        # Assert
        self.assertEqual(Decimal('1.00000'), result1)
        self.assertEqual(Decimal('1'), result2)
        self.assertEqual(Decimal('1.00'), result3)
