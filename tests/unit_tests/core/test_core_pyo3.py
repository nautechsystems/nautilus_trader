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

import pytest

from nautilus_trader.core import nautilus_pyo3


@pytest.mark.parametrize(
    ("input", "expected"),
    [
        # PascalCase
        ["SomePascalCase", "some_pascal_case"],
        ["AnotherExample", "another_example"],
        # camelCase
        ["someCamelCase", "some_camel_case"],
        ["yetAnotherExample", "yet_another_example"],
        # kebab-case
        ["some-kebab-case", "some_kebab_case"],
        ["dashed-word-example", "dashed_word_example"],
        # snake_case
        ["already_snake_case", "already_snake_case"],
        ["no_change_needed", "no_change_needed"],
        # UPPER_CASE
        ["UPPER_CASE_EXAMPLE", "upper_case_example"],
        ["ANOTHER_UPPER_CASE", "another_upper_case"],
        # Mixed Cases
        ["MiXeD_CaseExample", "mi_xe_d_case_example"],
        ["Another-OneHere", "another_one_here"],
        # Use case
        ["BSPOrderBookDelta", "bsp_order_book_delta"],
        ["OrderBookDelta", "order_book_delta"],
        ["TradeTick", "trade_tick"],
    ],
)
def test_convert_to_snake_case(input: str, expected: str) -> None:
    # Arrange, Act
    result = nautilus_pyo3.convert_to_snake_case(input)

    # Assert
    assert result == expected
