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

import numpy as np

from nautilus_trader.core.functions import basis_points_as_percentage
from nautilus_trader.core.functions import fast_mean
from nautilus_trader.core.functions import fast_mean_iterated
from nautilus_trader.core.functions import fast_std
from nautilus_trader.core.functions import fast_std_with_mean
from nautilus_trader.core.functions import format_bytes
from nautilus_trader.core.functions import max_in_dict
from nautilus_trader.core.functions import pad_string


class TestFunctionsTests(unittest.TestCase):

    def test_fast_mean_with_empty_list_returns_zero(self):
        # Arrange
        values = []

        # Act
        result = fast_mean(values)

        # Assert
        self.assertEqual(0, result)

    def test_fast_mean_with_values(self):
        # Arrange
        values = [0.0, 1.1, 2.2, 3.3, 4.4, 5.5]

        # Act
        result = fast_mean(values)

        # Assert
        self.assertEqual(2.75, result)
        self.assertEqual(2.75, np.mean(values))

    def test_fast_mean_iterated_with_empty_list_returns_zero(self):
        # Arrange
        values = []

        # Act
        result = fast_mean_iterated(values, 0.0, 0.0, 6)

        # Assert
        self.assertEqual(0, result)

    def test_fast_mean_iterated_with_values(self):
        # Arrange
        values1 = [0.0, 1.1, 2.2]
        values2 = [0.0, 1.1, 2.2, 3.3, 4.4]

        # Act
        result1 = fast_mean_iterated(values1, 0.0, fast_mean(values1), 5)
        result2 = fast_mean_iterated(values2, 5.5, np.mean(values2), 5)

        # Assert
        self.assertEqual(np.mean([0.0, 1.1, 2.2]), result1)
        self.assertAlmostEqual(3.3, result2)

    def test_std_dev_with_mean(self):
        # Arrange
        values = [0.0, 1.1, 2.2, 3.3, 4.4, 8.1, 9.9, -3.0]
        mean = fast_mean(values)

        # Act
        result1 = fast_std(values)
        result2 = fast_std_with_mean(values, mean)

        # Assert
        self.assertEqual(np.std(values), result1)
        self.assertEqual(np.std(values), result2)
        self.assertAlmostEqual(3.943665807342199, result1)
        self.assertAlmostEqual(3.943665807342199, result2)

    def test_basis_points_as_percentage(self):
        # Arrange
        # Act
        result1 = basis_points_as_percentage(0)
        result2 = basis_points_as_percentage(0.020)

        # Assert
        self.assertEqual(0.0, result1)
        self.assertAlmostEqual(0.000002, result2)

    def test_pad_string(self):
        # Arrange
        test_string = "1234"

        # Act
        result = pad_string(test_string, 5)

        # Assert
        self.assertEqual(" 1234", result)

    def test_format_bytes(self):
        # Arrange
        # Act
        result0 = format_bytes(1000)
        result1 = format_bytes(100000)
        result2 = format_bytes(10000000)
        result3 = format_bytes(1000000000)
        result4 = format_bytes(10000000000)
        result5 = format_bytes(100000000000000)

        # Assert
        self.assertEqual("1,000.0 bytes", result0)
        self.assertEqual("97.66 KB", result1)
        self.assertEqual("9.54 MB", result2)
        self.assertEqual("953.67 MB", result3)
        self.assertEqual("9.31 GB", result4)
        self.assertEqual("90.95 TB", result5)

    def test_max_in_dict_with_various_dictionaries_returns_expected_key(self):
        # Arrange
        dict1 = {1: 10, 2: 20, 3: 30}
        dict2 = {"a": 10.1, "c": 30.1, "b": 20.1, }

        # Act
        result1 = max_in_dict(dict1)
        result2 = max_in_dict(dict2)

        # Assert
        self.assertEqual(3, result1)
        self.assertEqual("c", result2)
