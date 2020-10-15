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

from parameterized import parameterized

from nautilus_trader.core.types import ValidString


class ValidStringTests(unittest.TestCase):

    @parameterized.expand([
        [None, TypeError],
        ["", ValueError],
        [" ", ValueError],
        ["  ", ValueError],
        [1234, TypeError],
    ])
    def test_instantiate_given_various_invalid_values_raises_exception(self, value, ex):
        # Arrange
        # Act
        # Assert
        self.assertRaises(ex, ValidString, value)

    def test_equality(self):
        # Arrange
        string1 = ValidString("abc123")
        string2 = ValidString("abc123")
        string3 = ValidString("def456")

        # Act
        # Assert
        self.assertTrue("abc123", string1.value)
        self.assertTrue(string1 == string1)
        self.assertTrue(string1 == string2)
        self.assertTrue(string1 != string3)

    def test_comparison(self):
        # Arrange
        string1 = ValidString("123")
        string2 = ValidString("456")
        string3 = ValidString("abc")
        string4 = ValidString("def")

        # Act
        # Assert
        self.assertTrue(string1 <= string1)
        self.assertTrue(string1 <= string2)
        self.assertTrue(string1 < string2)
        self.assertTrue(string2 > string1)
        self.assertTrue(string2 >= string1)
        self.assertTrue(string2 >= string2)
        self.assertTrue(string3 <= string4)

    def test_hash_returns_int_type(self):
        # Arrange
        value = ValidString("abc")

        # Act
        # Assert
        self.assertEqual(int, type(hash(value)))

    def test_to_string_returns_expected_string(self):
        # Arrange
        value = ValidString("abc")

        # Act
        # Assert
        self.assertEqual("abc", value.to_string())

    def test_str_returns_expected_strings(self):
        # Arrange
        value = ValidString("abc")

        # Act
        result1 = str(value)
        result2 = value.to_string()
        result3 = value.to_string(with_class=True)

        # Assert
        self.assertEqual("abc", result1)
        self.assertEqual("abc", result2)
        self.assertEqual("ValidString(abc)", result3)

    def test_repr_returns_expected_string(self):
        # Arrange
        value = ValidString("abc")

        # Act
        result = repr(value)

        # Assert
        self.assertTrue(result.startswith("<ValidString(abc) object at "))
        self.assertTrue(result.endswith(">"))
