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

from nautilus_trader.model.currency import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.quicktions import Fraction
from tests.test_kit.stubs import TestStubs

AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()
GBPUSD_FXCM = TestStubs.symbol_gbpusd_fxcm()


class ObjectTests(unittest.TestCase):

    def test_quantity_initialized_with_negative_integer_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, Quantity, -1)

    def test_quantity_initialized_with_valid_inputs(self):
        # Arrange
        # Act
        result0 = Quantity("0")
        result1 = Quantity("1")

        # Assert
        self.assertEqual(0, result0.precision)
        self.assertEqual(0, result0)
        self.assertEqual(1, result1)

    def test_quantity_equality(self):
        # Arrange
        # Act
        quantity1 = Quantity("1")
        quantity2 = Quantity("1")
        quantity3 = Quantity("2")

        # Assert
        self.assertEqual(quantity1, quantity2)
        self.assertNotEqual(quantity1, quantity3)

    def test_quantity_str(self):
        # Arrange
        # Act
        # Assert
        self.assertEqual("0", str(Quantity("0")))
        self.assertEqual("1000", str(Quantity("1000")))
        self.assertEqual("10.05", Quantity("10.05").to_string())
        # self.assertEqual("1K", Quantity("1000").to_string_formatted())
        self.assertEqual("120,100", Quantity("120100").to_string_formatted())
        # self.assertEqual("200K", Quantity("200000").to_string_formatted())
        # self.assertEqual("1M", Quantity("1000000").to_string_formatted())
        # self.assertEqual("2.5M", Quantity("2500000").to_string_formatted())
        self.assertEqual("1,111,111", Quantity("1111111").to_string_formatted())
        # self.assertEqual("2.523M", Quantity("2523000").to_string_formatted())
        # self.assertEqual("100M", Quantity("100000000").to_string_formatted())
        # self.assertEqual("1,000.0", Quantity("1000").to_string(format_commas=True))

    def test_quantity_comparisons(self):
        # Arrange
        quantity1 = Quantity("0")
        quantity2 = Quantity("1")
        quantity3 = Quantity("2")

        # Act
        # Assert
        self.assertTrue(quantity1 < 1)
        self.assertTrue(quantity1 <= 1)
        self.assertTrue(quantity2 <= 1)
        self.assertTrue(quantity3 > 1)
        self.assertTrue(quantity3 >= 2)
        self.assertTrue(quantity1 < quantity2)
        self.assertTrue(quantity1 <= quantity2)
        self.assertTrue(quantity2 <= quantity2)
        self.assertTrue(quantity3 > quantity2)
        self.assertTrue(quantity3 >= quantity3)

    def test_quantity_arithmetic(self):
        # Arrange
        # Act
        result1 = Quantity("1") + 1
        result2 = Quantity("1") + Quantity("1")

        result3 = Quantity("2") - 1
        result4 = Quantity("2") - Quantity("1")

        result7 = Quantity("2") * 2
        result8 = Quantity("2") * Quantity("2")

        # Assert
        self.assertEqual(Fraction, type(result1))
        self.assertEqual(2, result1)
        self.assertEqual(Fraction, type(result2))
        self.assertEqual(2, result2)

        self.assertEqual(Fraction, type(result3))
        self.assertEqual(1, result3)
        self.assertEqual(Fraction, type(result4))
        self.assertEqual(1, result4)

        self.assertEqual(Fraction, type(result7))
        self.assertEqual(4, result7)
        self.assertEqual(Fraction, type(result8))
        self.assertEqual(4, result8)

    def test_price_initialized_with_negative_value_raises_exception(self):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ValueError, Price, "-0.0000000000000000000000000000000000000000000000001")

    def test_price_str(self):
        # Arrange
        price = Price("1.00000")

        # Act
        result = str(price)

        # Assert
        self.assertEqual("1.00000", result)

    def test_price_repr(self):
        # Arrange
        price = Price.from_float(1.00000, 5)

        # Act
        result = repr(price)

        print(repr(price))
        # Assert
        self.assertTrue(result.startswith("<Price(1.00000) object at"))

    def test_price_equality(self):
        # Arrange
        # Act
        price1 = Price("1.00000")
        price2 = Price("1.00000")
        price3 = Price("2.00000")
        price4 = Price("1.01")

        # Assert
        self.assertEqual(price1, price2)
        self.assertNotEqual(price1, price3)
        self.assertNotEqual(price1, price4)

    def test_price_equality_operators(self):
        # Arrange
        price1 = Price("0.500")
        price2 = Price("1.000")
        price3 = Price("1.500")
        price4 = Price.from_float(1.2, 1)

        # Act
        # Assert
        self.assertTrue(price4 == price4)
        self.assertTrue(price1 < price2)
        self.assertTrue(price1 <= price2)
        self.assertTrue(price2 <= price2)
        self.assertTrue(price3 > price2)
        self.assertTrue(price3 >= price3)

    def test_price_arithmetic_operators(self):
        # Arrange
        # Act
        result1 = Price.from_float(1.0000, 5) + 1.0000

        result3 = Price.from_float(3.0000, 5) - 1.0000

        result5 = Price.from_float(1.0000, 5) / 1.0000
        result6 = Price.from_float(3.0000, 5) * 1.0000

        # Assert
        self.assertEqual(float, type(result1))
        self.assertEqual(2.0000, result1)

        self.assertEqual(float, type(result3))
        self.assertEqual(2.0000, result3)

        self.assertEqual(float, type(result5))
        self.assertEqual(1.0000, result5)
        self.assertEqual(float, type(result6))
        self.assertEqual(3.0000, result6)

    def test_price_add_returns_expected_decimal(self):
        # Arrange
        price1 = Price("1.00000")
        price2 = Price("1.00010")

        # Act
        result = price1 + price2

        # Assert
        self.assertEqual(Price("2.00010"), result)

    def test_price_subtract_returns_expected_decimal(self):
        # Arrange
        price1 = Price.from_float(2.00000, 5)
        price2 = Price.from_float(1.00010, 5)

        # Act
        result = price1 - price2

        # Assert
        self.assertEqual(Price.from_float(0.99990, 5), result)

    def test_money_from_string_with_no_decimal(self):
        # Arrange
        # Act
        money = Money(1, Currency.USD())

        # Assert
        self.assertEqual(1.0, money.as_double())
        self.assertEqual("1.00", str(money))

    def test_money_initialized_with_valid_inputs(self):
        # Arrange
        # Act
        result1 = Money("1.00", Currency.USD())
        result2 = Money(1000.0, Currency.USD())
        result3 = Money(2, Currency.USD())

        # Assert
        self.assertEqual(1.00, result1.as_double())
        self.assertEqual(1000.00, result2.as_double())
        self.assertEqual(2.00, result3.as_double())

    def test_money_initialized_with_many_decimals(self):
        # Arrange
        # Act
        result1 = Money(1000.333, Currency.USD())
        result2 = Money(5005.556666, Currency.USD())

        # Assert
        self.assertEqual("1,000.33 USD", result1.to_string_formatted())
        self.assertEqual("5,005.56 USD", result2.to_string_formatted())

    def test_money_str(self):
        # Arrange
        money0 = Money(0, Currency.USD())
        money1 = Money(1, Currency.USD())
        money2 = Money(1000000, Currency.USD())

        # Act
        # Assert
        self.assertEqual("0.00", str(money0))
        self.assertEqual("1.00", str(money1))
        self.assertEqual("1000000.00", str(money2))
        self.assertEqual("1,000,000.00 USD", money2.to_string_formatted())

    def test_money_repr(self):
        # Arrange
        money = Money("1.00", Currency.USD())

        # Act
        result = repr(money)

        # Assert
        self.assertTrue(result.startswith("<Money(1.00, currency=USD) object at"))

    def test_money_equality(self):
        # Arrange
        # Act
        money1 = Money(1.00, Currency.USD())
        money2 = Money(1.00, Currency.USD())
        money3 = Money("2.00", Currency.USD())
        money4 = Money("1.01", Currency.USD())

        # Assert
        self.assertEqual(money1, money2)
        self.assertNotEqual(money1, money3)
        self.assertNotEqual(money1, money4)

    def test_money_equality_operators(self):
        # Arrange
        money1 = Money("0.50", Currency.USD())
        money2 = Money(1.00, Currency.USD())
        money3 = Money("1.50", Currency.USD())

        # Act
        # Assert
        self.assertTrue(money1 == money1)
        self.assertTrue(money1 != money2)
        self.assertTrue(money1 < money2)
        self.assertTrue(money1 <= money2)
        self.assertTrue(money2 <= money2)
        self.assertTrue(money3 > money2)
        self.assertTrue(money3 >= money3)

    def test_money_arithmetic_operators(self):
        # Arrange
        # Act
        result1 = Money(1.00, Currency.USD()) + 1.00
        result2 = Money("1.00", Currency.USD())
        # result3 = Money(1.00, Currency.USD()) + 1
        #
        # result4 = Money(3.00, Currency.USD()) - 1.00
        # result5 = Money(3.00, Currency.USD())
        # result6 = Money(3.00, Currency.USD()) - 1
        #
        # result7 = Money(1.00, Currency.USD()) / 2.0
        # result8 = Money(1.00, Currency.USD()) / 2
        #
        # result9 = Money(1.00, Currency.USD()) * 2.00
        # result10 = Money(1.00, Currency.USD()) * 2

        # Assert
        self.assertEqual(float, type(result1))
        self.assertEqual(float(2.00), result1)
        self.assertEqual(Money, type(result2))
        # self.assertEqual(2., result2)
        # self.assertEqual(float, type(result3))
        # self.assertEqual(float(2.00), result3)
        #
        # self.assertEqual(float, type(result4))
        # self.assertEqual(float(2.00), result4)
        # self.assertEqual(Money, type(result5))
        # self.assertEqual(Money(2.00, 2, Currency.USD), result5)
        # self.assertEqual(float, type(result6))
        # self.assertEqual(float(2.00), result6)
        #
        # self.assertEqual(float, type(result7))
        # self.assertEqual(float(0.50), result7)
        # self.assertEqual(float, type(result8))
        # self.assertEqual(float(0.50), result8)
        # self.assertEqual(float, type(result9))
        # self.assertEqual(float(2.00), result9)
        #
        # self.assertEqual(float, type(result10))
        # self.assertEqual(float(2), result10)
