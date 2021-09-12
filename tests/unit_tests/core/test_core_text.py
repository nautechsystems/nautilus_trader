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

import pytest

from nautilus_trader.core.text import format_bytes
from nautilus_trader.core.text import pad_string


class TestText:
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
