# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core.stats import basis_points_as_percentage
from nautilus_trader.core.stats import fast_mad
from nautilus_trader.core.stats import fast_mad_with_mean
from nautilus_trader.core.stats import fast_mean
from nautilus_trader.core.stats import fast_mean_iterated
from nautilus_trader.core.stats import fast_std
from nautilus_trader.core.stats import fast_std_with_mean


class TestStats:
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

    def test_mean_absolute_deviation_with_mean(self):
        # Arrange
        values = np.asarray([0.0, 1.1, 2.2, 3.3, 4.4, 8.1, 9.9, -3.0], dtype=np.float64)
        mean = fast_mean(values)

        # Act
        result1 = fast_mad(values)
        result2 = fast_mad_with_mean(values, mean)
        # Assert
        assert result1 == np.abs(values - values.mean()).mean()
        assert result2 == np.abs(values - values.mean()).mean()
        assert result1 == 3.175
        assert result2 == 3.175

    def test_basis_points_as_percentage(self):
        # Arrange, Act
        result1 = basis_points_as_percentage(0)
        result2 = basis_points_as_percentage(0.020)

        # Assert
        assert result1 == 0.0
        assert result2 == 2.0000000000000003e-06
