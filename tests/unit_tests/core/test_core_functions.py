# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import numpy as np
import pytest

from nautilus_trader.core.functions import basis_points_as_percentage
from nautilus_trader.core.functions import bisect_double_left
from nautilus_trader.core.functions import bisect_double_right
from nautilus_trader.core.functions import fast_mean
from nautilus_trader.core.functions import fast_mean_iterated
from nautilus_trader.core.functions import fast_std
from nautilus_trader.core.functions import fast_std_with_mean
from nautilus_trader.core.functions import format_bytes
from nautilus_trader.core.functions import get_size_of
from nautilus_trader.core.functions import pad_string


class TestFunctions:
    @pytest.mark.parametrize(
        "a, value, expected",
        [
            [[], 1, 0],
            [[1], 0, 0],
            [[1], 1, 0],
            [[1], 2, 1],
            [[1, 1], 0, 0],
            [[1, 1], 1, 0],
            [[1, 1], 2, 2],
            [[1, 1, 1], 0, 0],
            [[1, 1, 1], 1, 0],
            [[1, 1, 1], 2, 3],
            [[1, 1, 1, 1], 0, 0],
            [[1, 1, 1, 1], 1, 0],
            [[1, 1, 1, 1], 2, 4],
            [[1, 2], 0, 0],
            [[1, 2], 1, 0],
            [[1, 2], 1.5, 1],
            [[1, 2], 2, 1],
            [[1, 2], 3, 2],
            [[1, 1, 2, 2], 0, 0],
            [[1, 1, 2, 2], 1, 0],
            [[1, 1, 2, 2], 1.5, 2],
            [[1, 1, 2, 2], 2, 2],
            [[1, 1, 2, 2], 3, 4],
            [[1, 2, 3], 0, 0],
            [[1, 2, 3], 1, 0],
            [[1, 2, 3], 1.5, 1],
            [[1, 2, 3], 2, 1],
            [[1, 2, 3], 2.5, 2],
            [[1, 2, 3], 3, 2],
            [[1, 2, 3], 4, 3],
            [[1, 2, 2, 3, 3, 3, 4, 4, 4, 4], 0, 0],
            [[1, 2, 2, 3, 3, 3, 4, 4, 4, 4], 1, 0],
            [[1, 2, 2, 3, 3, 3, 4, 4, 4, 4], 1.5, 1],
            [[1, 2, 2, 3, 3, 3, 4, 4, 4, 4], 2, 1],
            [[1, 2, 2, 3, 3, 3, 4, 4, 4, 4], 2.5, 3],
            [[1, 2, 2, 3, 3, 3, 4, 4, 4, 4], 3, 3],
            [[1, 2, 2, 3, 3, 3, 4, 4, 4, 4], 3.5, 6],
            [[1, 2, 2, 3, 3, 3, 4, 4, 4, 4], 4, 6],
            [[1, 2, 2, 3, 3, 3, 4, 4, 4, 4], 5, 10],
        ],
    )
    def test_bisect_left(self, a, value, expected):
        # Arrange, Act
        result = bisect_double_left(a, value)

        # Assert
        assert result == expected

    @pytest.mark.parametrize(
        "a, value, expected",
        [
            [[], 1, 0],
            [[1], 0, 0],
            [[1], 1, 1],
            [[1], 2, 1],
            [[1, 1], 0, 0],
            [[1, 1], 1, 2],
            [[1, 1], 2, 2],
            [[1, 1, 1], 0, 0],
            [[1, 1, 1], 1, 3],
            [[1, 1, 1], 2, 3],
            [[1, 1, 1, 1], 0, 0],
            [[1, 1, 1, 1], 1, 4],
            [[1, 1, 1, 1], 2, 4],
            [[1, 2], 0, 0],
            [[1, 2], 1, 1],
            [[1, 2], 1.5, 1],
            [[1, 2], 2, 2],
            [[1, 2], 3, 2],
            [[1, 1, 2, 2], 0, 0],
            [[1, 1, 2, 2], 1, 2],
            [[1, 1, 2, 2], 1.5, 2],
            [[1, 1, 2, 2], 2, 4],
            [[1, 1, 2, 2], 3, 4],
            [[1, 2, 3], 0, 0],
            [[1, 2, 3], 1, 1],
            [[1, 2, 3], 1.5, 1],
            [[1, 2, 3], 2, 2],
            [[1, 2, 3], 2.5, 2],
            [[1, 2, 3], 3, 3],
            [[1, 2, 3], 4, 3],
            [[1, 2, 2, 3, 3, 3, 4, 4, 4, 4], 0, 0],
            [[1, 2, 2, 3, 3, 3, 4, 4, 4, 4], 1, 1],
            [[1, 2, 2, 3, 3, 3, 4, 4, 4, 4], 1.5, 1],
            [[1, 2, 2, 3, 3, 3, 4, 4, 4, 4], 2, 3],
            [[1, 2, 2, 3, 3, 3, 4, 4, 4, 4], 2.5, 3],
            [[1, 2, 2, 3, 3, 3, 4, 4, 4, 4], 3, 6],
            [[1, 2, 2, 3, 3, 3, 4, 4, 4, 4], 3.5, 6],
            [[1, 2, 2, 3, 3, 3, 4, 4, 4, 4], 4, 10],
            [[1, 2, 2, 3, 3, 3, 4, 4, 4, 4], 5, 10],
        ],
    )
    def test_bisect_right(self, a, value, expected):
        # Arrange, Act
        result = bisect_double_right(a, value)

        # Assert
        assert result == expected

    def test_fast_mean_with_empty_list_returns_zero(self):
        # Arrange
        values = np.asarray([], dtype=np.float64)

        # Act
        result = fast_mean(values)

        # Assert
        assert result == 0

    def test_fast_mean_with_values(self):
        # Arrange
        values = np.asarray([0.0, 1.1, 2.2, 3.3, 4.4, 5.5], dtype=np.float64)

        # Act
        result = fast_mean(values)

        # Assert
        assert result == 2.75
        assert np.mean(values) == 2.75

    def test_fast_mean_iterated_with_empty_list_returns_zero(self):
        # Arrange
        values = np.asarray([], dtype=np.float64)

        # Act
        result = fast_mean_iterated(values, 0.0, 0.0, 6)

        # Assert
        assert result == 0

    def test_fast_mean_iterated_with_values(self):
        # Arrange
        values1 = np.asarray([0.0, 1.1, 2.2], dtype=np.float64)
        values2 = np.asarray([0.0, 1.1, 2.2, 3.3, 4.4], dtype=np.float64)

        # Act
        result1 = fast_mean_iterated(values1, 0.0, fast_mean(values1), 5)
        result2 = fast_mean_iterated(values2, 5.5, np.mean(values2), 5)

        # Assert
        assert result1 == np.mean([0.0, 1.1, 2.2])
        assert result2 == 3.3000000000000003

    def test_std_dev_with_mean(self):
        # Arrange
        values = np.asarray([0.0, 1.1, 2.2, 3.3, 4.4, 8.1, 9.9, -3.0], dtype=np.float64)
        mean = fast_mean(values)

        # Act
        result1 = fast_std(values)
        result2 = fast_std_with_mean(values, mean)

        # Assert
        assert result1 == np.std(values)
        assert result2 == np.std(values)
        assert result1 == 3.943665807342199
        assert result2 == 3.943665807342199

    def test_basis_points_as_percentage(self):
        # Arrange, Act
        result1 = basis_points_as_percentage(0)
        result2 = basis_points_as_percentage(0.020)

        # Assert
        assert result1 == 0.0
        assert result2 == 2.0000000000000003e-06

    def test_get_size_of(self):
        # Arrange, Act
        result1 = get_size_of(0)
        result2 = get_size_of(1.1)
        result3 = get_size_of("abc")

        # Assert
        assert result1 == 24
        assert result2 == 24
        assert result3 == 52

    @pytest.mark.parametrize(
        "original, final_length, expected",
        [
            ["1234", 4, "1234"],
            ["1234", 5, " 1234"],
            ["1234", 6, "  1234"],
            ["1234", 3, "1234"],
        ],
    )
    def test_pad_string(self, original, final_length, expected):
        # Arrange, Act
        result = pad_string(original, final_length=final_length)

        # Assert
        assert result == expected

    def test_format_bytes(self):
        # Arrange, Act
        result0 = format_bytes(1000)
        result1 = format_bytes(100000)
        result2 = format_bytes(10000000)
        result3 = format_bytes(1000000000)
        result4 = format_bytes(10000000000)
        result5 = format_bytes(100000000000000)

        # Assert
        assert result0 == "1,000.0 bytes"
        assert result1 == "97.66 KB"
        assert result2 == "9.54 MB"
        assert result3 == "953.67 MB"
        assert result4 == "9.31 GB"
        assert result5 == "90.95 TB"
