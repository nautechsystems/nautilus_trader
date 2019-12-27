# -------------------------------------------------------------------------------------------------
# <copyright file="test_model_objects.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from decimal import Decimal, InvalidOperation
from datetime import timedelta

from nautilus_trader.core.correctness import ConditionFailed
from nautilus_trader.core.types import ValidString
from nautilus_trader.model.enums import BarStructure, QuoteType
from nautilus_trader.model.identifiers import Symbol, Venue
from nautilus_trader.model.objects import Quantity, Price, Money, Tick, BarSpecification, BarType, Bar
from test_kit.stubs import TestStubs

AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()
GBPUSD_FXCM = TestStubs.symbol_gbpusd_fxcm()
UNIX_EPOCH = TestStubs.unix_epoch()


class ObjectTests(unittest.TestCase):

    def test_quantity_initialized_with_negative_integer_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ConditionFailed, Quantity, -1)

    def test_quantity_initialized_with_valid_inputs(self):
        # Arrange
        # Act
        result0 = Quantity.zero()
        result1 = Quantity(1)

        # Assert
        self.assertEqual(0, result0.value)
        self.assertEqual(1, result1.value)

    def test_quantity_equality(self):
        # Arrange
        # Act
        quantity1 = Quantity(1)
        quantity2 = Quantity(1)
        quantity3 = Quantity(2)

        # Assert
        self.assertEqual(quantity1, quantity2)
        self.assertNotEqual(quantity1, quantity3)

    def test_quantity_str(self):
        # Arrange
        quantity = Quantity(1)

        # Act
        result = str(quantity)

        # Assert
        self.assertEqual('1', result)

    def test_quantity_repr(self):
        # Arrange
        quantity = Quantity(1)

        # Act
        result = repr(quantity)

        # Assert
        self.assertTrue(result.startswith('<Quantity(1) object at'))

    def test_quantity_operators(self):
        # Arrange
        quantity1 = Quantity.zero()
        quantity2 = Quantity(1)
        quantity3 = Quantity(2)

        # Act
        # Assert
        self.assertTrue(quantity1 < quantity2)
        self.assertTrue(quantity1 <= quantity2)
        self.assertTrue(quantity2 <= quantity2)
        self.assertTrue(quantity3 > quantity2)
        self.assertTrue(quantity3 >= quantity3)

    def test_quantity_arithmetic(self):
        # Arrange
        # Act
        result1 = Quantity(1) + 1
        result2 = Quantity(1) + Quantity(1)

        result3 = Quantity(2) - 1
        result4 = Quantity(2) - Quantity(1)

        # Assert
        self.assertEqual(int, type(result1))
        self.assertEqual(2, result1)
        self.assertEqual(int, type(result2))
        self.assertEqual(2, result2)

        self.assertEqual(int, type(result3))
        self.assertEqual(1, result3)
        self.assertEqual(int, type(result4))
        self.assertEqual(1, result4)

    def test_price_initialized_with_invalid_type_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(TypeError, Price, Money.zero())

    def test_price_initialized_with_malformed_string_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(InvalidOperation, Price, 'a')

    def test_price_initialized_with_negative_value_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, Price, -1.0, 2)

    def test_price_initialized_with_negative_precision_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ConditionFailed, Price, 1.00000, -1)

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
        price1 = Price(1.00000, 5)
        price2 = Price(1.0001, 3)

        # Assert
        self.assertEqual(Price('1.00000'), price1)
        self.assertEqual('1.00000',  str(price1))
        self.assertEqual(Price('1.000'), price2)
        self.assertEqual('1.000', str(price2))

    def test_price_initialized_with_valid_inputs(self):
        # Arrange
        # Act
        result0 = Price(1, 1)
        result1 = Price(1.0, 1)
        result2 = Price(1.00000, 5)
        result3 = Price(1.001, 2)
        result4 = Price(1.2, 1)  # Rounding half up
        result5 = Price(1.000001, 5)
        result6 = Price(Decimal('1.000'))
        result7 = Price(87.1, 3)

        # Assert
        self.assertEqual(Price('1.0'), result0)
        self.assertEqual(Price('1.0'), result1)
        self.assertEqual(Price('1.00000'), result2)
        self.assertEqual(Price('1.00'), result3)
        self.assertEqual(Price('1.2'), result4)
        self.assertEqual(Price('1.0'), result5)
        self.assertEqual(1.0, result5.as_float())
        self.assertEqual(Price('1.000'), result6)
        self.assertEqual(1.000, result6.as_float())
        self.assertEqual(Price('87.100'), result7)

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

        # Act
        result = repr(price)

        # Assert
        self.assertTrue(result.startswith('<Price(1.00000) object at'))

    def test_price_equality(self):
        # Arrange
        # Act
        price1 = Price('1.00000')
        price2 = Price('1.00000')
        price3 = Price('2.00000')
        price4 = Price('1.01')

        # Assert
        self.assertEqual(price1, price2)
        self.assertNotEqual(price1, price3)
        self.assertNotEqual(price1, price4)

    def test_price_equality_operators(self):
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

    def test_price_arithmetic_operators(self):
        # Arrange
        # Act
        result1 = Price('1.0000') + 1.0000
        result2 = Price('1.0000') + Decimal('1.0000')
        result3 = Price('1.0000') + Price('1.0000')

        result4 = Price('3.0000') - 1.0000
        result5 = Price('3.0000') - Decimal('1.0000')
        result6 = Price('3.0000') - Price('1.0000')

        result7 = Price('1.0000') / 1.0000
        result8 = Price('1.0000') / Decimal('1.0000')
        result9 = Price('1.0000') / Price('1.0000')

        result10 = Price('3.0000') * 1.0000
        result11 = Price('3.0000') * Decimal('1.0000')
        result12 = Price('3.0000') * Price('1.0000')

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

        self.assertEqual(Decimal, type(result7))
        self.assertEqual(Decimal('1.0000'), result7)
        self.assertEqual(Decimal, type(result8))
        self.assertEqual(Decimal('1.0000'), result8)
        self.assertEqual(Decimal, type(result9))
        self.assertEqual(Decimal('1.0000'), result9)

        self.assertEqual(Decimal, type(result10))
        self.assertEqual(Decimal('3.0000'), result10)
        self.assertEqual(Decimal, type(result11))
        self.assertEqual(Decimal('3.0000'), result11)
        self.assertEqual(Decimal, type(result12))
        self.assertEqual(Decimal('3.0000'), result12)

    def test_price_as_float(self):
        # Arrange
        price = Price(1.00000, 5)

        # Act
        result = price.as_float()

        # Assert
        self.assertEqual(1.0, result)

    def test_price_add_with_different_precisions_raises_exception(self):
        # Arrange
        price1 = Price(1.00000, 5)
        price2 = Price(1.01, 2)

        # Act
        # Assert
        self.assertRaises(ConditionFailed, price1.add, price2)

    def test_price_add_returns_expected_price(self):
        # Arrange
        price1 = Price(1.00000, 5)
        price2 = Price(1.00010, 5)

        # Act
        result = price1.add(price2)

        # Assert
        self.assertEqual(Price('2.00010'), result)

    def test_price_subtract_resulting_in_negative_value_raises_exception(self):
        # Arrange
        price1 = Price(2.00000, 5)
        price2 = Price(1.00010, 5)

        # Act
        # Assert
        self.assertRaises(ValueError, price2.subtract, price1)

    def test_price_subtract_returns_expected_price(self):
        # Arrange
        price1 = Price(2.00000, 5)
        price2 = Price(1.00010, 5)

        # Act
        result = price1.subtract(price2)

        # Assert
        self.assertEqual(Price('0.99990'), result)

    def test_money_initialized_with_malformed_string_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, Money, 'a')

    def test_money_zero_returns_money_with_zero_value(self):
        # Arrange
        # Act
        result = Money.zero()

        # Assert
        self.assertEqual(Money(Decimal('0')), result)
        self.assertEqual('0.00', str(result))

    def test_money_from_string_with_no_decimal(self):
        # Arrange
        # Act
        money = Money(Decimal('1'))

        # Assert
        self.assertEqual(Decimal('1.00'), money.value)
        self.assertEqual('1.00', str(money))

    def test_money_initialized_with_valid_inputs(self):
        # Arrange
        # Act
        result1 = Money(Decimal('1.00'))
        result2 = Money(Decimal('1000.0'))
        result3 = Money(2)

        # Assert
        self.assertEqual(Decimal('1.00'), result1.value)
        self.assertEqual(Decimal('1000.00'), result2.value)
        self.assertEqual(Decimal('2.00'), result3.value)

    def test_money_initialized_with_many_decimals(self):
        # Arrange
        # Act
        result1 = Money(Decimal('1000.333'))
        result2 = Money(Decimal('5005.556666'))

        # Assert
        self.assertEqual(Decimal('1000.33'), result1.value)
        self.assertEqual(Decimal('5005.56'), result2.value)

    def test_money_initialized_with_many_scientific_notation_returns_zero(self):
        # Arrange
        # Act
        result1 = Money(0E-30)
        result2 = Money(-0E-33)
        result3 = Money('0E-30')
        result4 = Money('-0E-33')

        # Assert
        self.assertEqual(Decimal('0.00'), result1.value)
        self.assertEqual(Decimal('0.00'), result2.value)
        self.assertEqual(Decimal('0.00'), result3.value)
        self.assertEqual(Decimal('0.00'), result4.value)
        self.assertEqual(0, result1.as_float())

    def test_money_str(self):
        # Arrange
        money1 = Money(1)
        money2 = Money(1000000)

        # Act
        result1 = str(money1)
        result2 = str(money2)

        # Assert
        self.assertEqual('1.00', result1)
        self.assertEqual('1,000,000.00', result2)

    def test_money_repr(self):
        # Arrange
        money = Money(1)

        # Act
        result = repr(money)

        # Assert
        self.assertTrue(result.startswith('<Money(1.00) object at'))

    def test_money_equality(self):
        # Arrange
        # Act
        money1 = Money('1.00')
        money2 = Money('1.00')
        money3 = Money('2.00')
        money4 = Money('1.01')

        # Assert
        self.assertEqual(money1, money2)
        self.assertNotEqual(money1, money3)
        self.assertNotEqual(money1, money4)

    def test_money_equality_operators(self):
        # Arrange
        money1 = Money('0.50')
        money2 = Money('1.00')
        money3 = Money('1.50')

        # Act
        # Assert
        self.assertTrue(money1 < money2)
        self.assertTrue(money1 <= money2)
        self.assertTrue(money2 <= money2)
        self.assertTrue(money3 > money2)
        self.assertTrue(money3 >= money3)

    def test_money_arithmetic_operators(self):
        # Arrange
        # Act
        result1 = Money('1.00') + Decimal('1.00')
        result2 = Money('1.00') + Money('1.00')
        result3 = Money('1.00') + 1

        result4 = Money('3.00') - Decimal('1.00')
        result5 = Money('3.00') - Money('1.00')
        result6 = Money('3.00') - 1

        result7 = Money('1.00') / Money('2')
        result8 = Money('1.00') / Decimal('2')
        result9 = Money('1.00') / 2

        result10 = Money('1.00') * Money('2')
        result11 = Money('1.00') * Decimal('2')
        result12 = Money('1.00') * 2

        # Assert
        self.assertEqual(Money, type(result1))
        self.assertEqual(Money('2.00'), result1)
        self.assertEqual(Money, type(result2))
        self.assertEqual(Money('2.00'), result2)
        self.assertEqual(Money, type(result3))
        self.assertEqual(Money('2.00'), result3)

        self.assertEqual(Money, type(result4))
        self.assertEqual(Money('2.00'), result4)
        self.assertEqual(Money, type(result5))
        self.assertEqual(Money('2.00'), result5)
        self.assertEqual(Money, type(result6))
        self.assertEqual(Money('2.00'), result6)

        self.assertEqual(Money, type(result7))
        self.assertEqual(Money('0.50'), result7)
        self.assertEqual(Money, type(result8))
        self.assertEqual(Money('0.50'), result8)
        self.assertEqual(Money, type(result9))
        self.assertEqual(Money('0.50'), result9)

        self.assertEqual(Money, type(result10))
        self.assertEqual(Money(2), result10)
        self.assertEqual(Money, type(result11))
        self.assertEqual(Money(2), result11)
        self.assertEqual(Money, type(result12))
        self.assertEqual(Money(2), result12)

    def test_money_as_float(self):
        # Arrange
        money1 = Money.zero()
        money2 = Money('1.00')

        # Act
        result1 = money1.as_float()
        result2 = money2.as_float()

        # Assert
        self.assertEqual(0, result1)
        self.assertEqual(1.0, result2)

    def test_bar_spec_equality(self):
        # Arrange
        bar_spec1 = BarSpecification(1, BarStructure.MINUTE, QuoteType.BID)
        bar_spec2 = BarSpecification(1, BarStructure.MINUTE, QuoteType.BID)
        bar_spec3 = BarSpecification(1, BarStructure.MINUTE, QuoteType.ASK)

        # Act
        # Assert
        self.assertTrue(bar_spec1 == bar_spec1)
        self.assertTrue(bar_spec1 == bar_spec2)
        self.assertTrue(bar_spec1 != bar_spec3)

    def test_bar_spec_str_and_repr(self):
        # Arrange
        bar_spec = BarSpecification(1, BarStructure.MINUTE, QuoteType.BID)

        # Act
        # Assert
        self.assertEqual("1-MINUTE[BID]", str(bar_spec))
        self.assertTrue(repr(bar_spec).startswith("<BarSpecification(1-MINUTE[BID]) object at"))

    def test_can_parse_tick_from_string_with_symbol(self):
        # Arrange
        tick = Tick(AUDUSD_FXCM,
                    Price('1.00000'),
                    Price('1.00001'),
                    UNIX_EPOCH)

        # Act
        result = Tick.py_from_string_with_symbol(AUDUSD_FXCM, str(tick))

        # Assert
        self.assertEqual(tick, result)

    def test_can_parse_tick_from_string(self):
        # Arrange
        tick = Tick(AUDUSD_FXCM,
                    Price('1.00000'),
                    Price('1.00001'),
                    UNIX_EPOCH)

        # Act
        result = Tick.py_from_string(AUDUSD_FXCM.value + ',' + str(tick))

        # Assert
        self.assertEqual(tick, result)

    def test_can_parse_bar_spec_from_string(self):
        # Arrange
        bar_spec = BarSpecification(1, BarStructure.MINUTE, QuoteType.MID)

        # Act
        result = BarSpecification.py_from_string(str(bar_spec))

        # Assert
        self.assertEqual(bar_spec, result)

    def test_bar_type_equality(self):
        # Arrange
        symbol1 = Symbol("AUDUSD", Venue('FXCM'))
        symbol2 = Symbol("GBPUSD", Venue('FXCM'))
        bar_spec = BarSpecification(1, BarStructure.MINUTE, QuoteType.BID)
        bar_type1 = BarType(symbol1, bar_spec)
        bar_type2 = BarType(symbol1, bar_spec)
        bar_type3 = BarType(symbol2, bar_spec)

        # Act
        # Assert
        self.assertTrue(bar_type1 == bar_type1)
        self.assertTrue(bar_type1 == bar_type2)
        self.assertTrue(bar_type1 != bar_type3)

    def test_bar_type_str_and_repr(self):
        # Arrange
        symbol = Symbol("AUDUSD", Venue('FXCM'))
        bar_spec = BarSpecification(1, BarStructure.MINUTE, QuoteType.BID)
        bar_type = BarType(symbol, bar_spec)

        # Act
        # Assert
        self.assertEqual("AUDUSD.FXCM-1-MINUTE[BID]", str(bar_type))
        self.assertTrue(repr(bar_type).startswith("<BarType(AUDUSD.FXCM-1-MINUTE[BID]) object at"))

    def test_can_parse_bar_from_string(self):
        # Arrange
        bar = TestStubs.bar_5decimal()

        # Act
        result = Bar.py_from_string(str(bar))

        # Assert
        self.assertEqual(bar, result)
