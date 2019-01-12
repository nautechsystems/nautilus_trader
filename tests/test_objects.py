#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_objects.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from decimal import Decimal

from inv_trader.model.enums import Venue
from inv_trader.model.objects import Price, Symbol


class ObjectTests(unittest.TestCase):

    def test_create_price_with_zero_price_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, Price.create, -1, 0)

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
        result4 = Price.create(1.1, 0)
        result5 = Price.create(1.000001, 5)

        # Assert
        self.assertEqual(Decimal('1.00000'), result1)
        self.assertEqual(Decimal('1'), result2)
        self.assertEqual(Decimal('1.00'), result3)
        self.assertEqual(Decimal('1.0'), result4)
        self.assertEqual(Decimal('1.0'), result5)

    def test_symbol_equality(self):
        # Arrange
        symbol1 = Symbol("AUDUSD", Venue.FXCM)
        symbol2 = Symbol("AUDUSD", Venue.IDEAL_PRO)
        symbol3 = Symbol("GBPUSD", Venue.FXCM)

        # Act
        # Assert
        self.assertTrue(symbol1.equals(symbol1))
        self.assertTrue(not symbol1.equals(symbol2))
        self.assertTrue(not symbol1.equals(symbol3))

    def test_symbol_str_and_repr(self):
        # Arrange
        symbol = Symbol("AUDUSD", Venue.FXCM)

        # Act
        # Assert
        self.assertEqual("AUDUSD.FXCM", str(symbol))
        self.assertTrue(repr(symbol).startswith("<AUDUSD.FXCM object at"))
