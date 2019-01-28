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
from inv_trader.model.objects import Symbol, Price


class ObjectTests(unittest.TestCase):

    def test_symbol_equality(self):
        # Arrange
        symbol1 = Symbol("AUDUSD", Venue.FXCM)
        symbol2 = Symbol("AUDUSD", Venue.IDEAL_PRO)
        symbol3 = Symbol("GBPUSD", Venue.FXCM)

        # Act
        # Assert
        self.assertTrue(symbol1 == symbol1)
        self.assertTrue(symbol1 != symbol2)
        self.assertTrue(symbol1 != symbol3)

    def test_symbol_str_and_repr(self):
        # Arrange
        symbol = Symbol("AUDUSD", Venue.FXCM)

        # Act
        # Assert
        self.assertEqual("AUDUSD.FXCM", str(symbol))
        self.assertTrue(repr(symbol).startswith("<AUDUSD.FXCM object at"))

    def test_price_initialized_with_negative_value_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(AssertionError, Price, -1.0)

    def test_price_initialized_with_negative_precision_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(AssertionError, Price, 1.00000, -1)

    def test_price_from_string_with_no_decimal(self):
        # Arrange
        # Act
        price = Price('1')

        # Assert
        self.assertEqual(Decimal('1.0'), price.value)
        self.assertEqual(0, price.precision)

    def test_price_from_float(self):
        # Arrange
        # Act
        price = Price(1.00000, 5)

        # Assert
        self.assertEqual(Price('1.00000'), price)

    def test_price_initialized_with_valid_inputs(self):
        # Arrange
        # Act
        result1 = Price(1.0)
        result2 = Price(1.00000, 5)
        result3 = Price(1.001, 2)
        result4 = Price(1.15)  # Rounding half down
        result5 = Price(1.000001, 5)
        result6 = Price(Decimal('1.000'))

        # Assert
        self.assertEqual(Price('1.0'), result1)
        self.assertEqual(Price('1.00000'), result2)
        self.assertEqual(Price('1.00'), result3)
        self.assertEqual(Price('1.1'), result4)
        self.assertEqual(Price('1.0'), result5)
        self.assertEqual(1.0, result5.as_float())
        self.assertEqual(Price('1.000'), result6)
        self.assertEqual(1.000, result6.as_float())

    def test_price_equality(self):
        # Arrange
        # Act
        price1 = Price('1.00000')
        price2 = Price('1.00000')
        price3 = Price('2.00000')
        price4 = Price('1.01')

        result1 = price1 != price4

        # Assert
        self.assertEqual(price1, price2)
        self.assertNotEqual(price1, price3)
        self.assertNotEqual(price1, price4)
        self.assertTrue(result1)

    def test_price_str(self):
        # Arrange
        price = Price(1.00000, 5)

        # Act
        result = str(price)

        # Assert
        self.assertEqual('1.00000', result)

    def test_price_repr(self):
        # Arrange
        price = Price(1.00000, 5)
        print(price)
        # Act
        result = repr(price)

        # Assert
        self.assertTrue(result.startswith('<Price(1.00000) object at'))

    def test_price_operators(self):
        # Arrange
        price1 = Price('0.500')
        price2 = Price('1.000')
        price3 = Price('1.500')

        # Act
        # Assert
        self.assertTrue(price1 < price2)
        self.assertTrue(price1 <= price2)
        self.assertTrue(price2 <= price2)
        self.assertTrue(price3 > price2)
        self.assertTrue(price3 >= price3)

    def test_price_arithmetic(self):
        # Arrange
        # Act
        result1 = Price('1.0000') + 1.0000
        result2 = Price('1.0000') + Decimal('1.0000')
        result3 = Price('1.0000') + Price('1.0000')

        result4 = Price('3.0000') - 1.0000
        result5 = Price('3.0000') - Decimal('1.0000')
        result6 = Price('3.0000') - Price('1.0000')

        # Assert
        self.assertEqual(Decimal, type(result1))
        self.assertEqual(Decimal('2.0000'), result1)
        self.assertEqual(Decimal, type(result2))
        self.assertEqual(Decimal('2.0000'), result2)
        self.assertEqual(Decimal, type(result3))
        self.assertEqual(Decimal('2.0000'), result3)

        self.assertEqual(Decimal, type(result4))
        self.assertEqual(Decimal('2.0000'), result4)
        self.assertEqual(Decimal, type(result5))
        self.assertEqual(Decimal('2.0000'), result5)
        self.assertEqual(Decimal, type(result6))
        self.assertEqual(Decimal('2.0000'), result6)
