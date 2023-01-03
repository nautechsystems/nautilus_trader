# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

import pytest

from nautilus_trader.core.collections import bisect_left
from nautilus_trader.core.collections import bisect_right


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
        result = bisect_left(a, value)

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
        result = bisect_right(a, value)

        # Assert
        assert result == expected
