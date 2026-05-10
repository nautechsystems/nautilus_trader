# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core import NAUTILUS_USER_AGENT
from nautilus_trader.core import NAUTILUS_VERSION
from nautilus_trader.core import convert_to_snake_case
from nautilus_trader.core import is_pycapsule
from nautilus_trader.core import mask_api_key


def test_version_constants_are_consistent():
    assert NAUTILUS_VERSION
    assert f"NautilusTrader/{NAUTILUS_VERSION}" == NAUTILUS_USER_AGENT


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        ("SomePascalCase", "some_pascal_case"),
        ("AnotherExample", "another_example"),
        ("someCamelCase", "some_camel_case"),
        ("yetAnotherExample", "yet_another_example"),
        ("some-kebab-case", "some_kebab_case"),
        ("dashed-word-example", "dashed_word_example"),
        ("already_snake_case", "already_snake_case"),
        ("no_change_needed", "no_change_needed"),
        ("UPPER_CASE_EXAMPLE", "upper_case_example"),
        ("ANOTHER_UPPER_CASE", "another_upper_case"),
        ("MiXeD_CaseExample", "mi_xe_d_case_example"),
        ("Another-OneHere", "another_one_here"),
        ("BSPOrderBookDelta", "bsp_order_book_delta"),
        ("OrderBookDelta", "order_book_delta"),
        ("TradeTick", "trade_tick"),
    ],
)
def test_convert_to_snake_case(value, expected):
    assert convert_to_snake_case(value) == expected


def test_mask_api_key_masks_middle():
    result = mask_api_key("sk-abc123xyz789")
    assert result.startswith("sk")
    assert result.endswith("789")
    assert "..." in result


def test_mask_api_key_short_key():
    result = mask_api_key("abc")
    assert isinstance(result, str)


def test_is_pycapsule_rejects_non_capsule():
    assert is_pycapsule("not a capsule") is False
    assert is_pycapsule(42) is False
    assert is_pycapsule(None) is False
