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
"""
Unit tests for the common module.
"""

from nautilus_trader.adapters.dydx.common.common import DYDXOrderTags
from nautilus_trader.model.objects import Price


def test_serialize_tags() -> None:
    """
    Test the DYDXOrderTags serialization to string.
    """
    # Arrange
    tags = DYDXOrderTags()
    expected_result = (
        'DYDXOrderTags:{"is_short_term_order":true,"num_blocks_open":20,"market_order_price":null}'
    )

    # Act
    result = tags.value

    # Assert
    assert result == expected_result


def test_serialize_tags_with_price() -> None:
    """
    Test the DYDXOrderTags serialization to string when setting a market order Price.
    """
    # Arrange
    tags = DYDXOrderTags(market_order_price=Price.from_int(100_000))
    expected_result = 'DYDXOrderTags:{"is_short_term_order":true,"num_blocks_open":20,"market_order_price":"100000"}'

    # Act
    result = tags.value

    # Assert
    assert result == expected_result


def test_parsing_string() -> None:
    """
    Test the DYDXOrderTags serialization to string when setting a market order Price.
    """
    # Arrange
    expected_result = DYDXOrderTags(market_order_price=Price.from_int(100_000))
    tag_string = 'DYDXOrderTags:{"is_short_term_order":true,"num_blocks_open":20,"market_order_price":"100000"}'

    # Act
    result = DYDXOrderTags.parse(tag_string.replace("DYDXOrderTags:", ""))

    # Assert
    assert result == expected_result
