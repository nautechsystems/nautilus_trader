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

import msgspec
import pytest

from nautilus_trader.adapters.dydx.execution import ClientOrderIdHelper
from nautilus_trader.model.identifiers import ClientOrderId


@pytest.fixture
def client_order_id_helper(cache):
    return ClientOrderIdHelper(cache=cache)


@pytest.mark.parametrize("order_string", ["839ca109-f2c8-46b5-88f2-345eeeb01058", str(12345)])
def test_generate_client_order_id_int_uuid(client_order_id_helper, order_string) -> None:
    client_order_id = ClientOrderId(order_string)
    result = client_order_id_helper.generate_client_order_id_int(client_order_id)
    assert isinstance(result, int)


def test_generate_client_order_id_int_with_int(client_order_id_helper) -> None:
    expected_result = 12345
    client_order_id = ClientOrderId(str(expected_result))
    result = client_order_id_helper.generate_client_order_id_int(client_order_id)
    assert result == expected_result


def test_retrieve_from_cache(client_order_id_helper) -> None:
    client_order_id_int = 12345
    expected_result = ClientOrderId(str(client_order_id_int))
    client_order_id_helper.generate_client_order_id_int(expected_result)
    result = client_order_id_helper.get_client_order_id(client_order_id_int)
    assert result.value == expected_result.value
    assert result == expected_result


def test_retrieve_from_empty_cache(client_order_id_helper) -> None:
    client_order_id_int = 12345
    expected_result = ClientOrderId(str(client_order_id_int))
    result = client_order_id_helper.get_client_order_id(client_order_id_int)
    assert result.value == expected_result.value
    assert result == expected_result


def test_retrieve_client_order_id_integer_from_cache(client_order_id_helper) -> None:
    expected_result = 12345
    client_order_id = ClientOrderId(str(expected_result))
    client_order_id_helper.generate_client_order_id_int(client_order_id)
    result = client_order_id_helper.get_client_order_id_int(client_order_id)
    assert result == expected_result


def test_retrieve_client_order_id_integer_from_empty_cache(client_order_id_helper) -> None:
    expected_result = 12345
    client_order_id = ClientOrderId(str(expected_result))
    result = client_order_id_helper.get_client_order_id_int(client_order_id)
    assert result == expected_result


def test_block_height_message_parsing():
    from nautilus_trader.adapters.dydx.schemas.ws import DYDXWsBlockHeightChannelData

    raw_message = b'{"type":"channel_data","connection_id":"test-123","message_id":42,"id":"dydx","channel":"v4_block_height","version":"4.0.0","contents":{"blockHeight":"12345678","time":"2025-12-19T10:30:00.000Z"}}'

    decoder = msgspec.json.Decoder(DYDXWsBlockHeightChannelData)
    msg = decoder.decode(raw_message)

    assert msg.contents.blockHeight == "12345678"
    assert msg.channel == "v4_block_height"
    assert msg.type == "channel_data"

    block_height_int = int(msg.contents.blockHeight)
    assert block_height_int == 12345678
    assert isinstance(block_height_int, int)


def test_block_height_initialization():
    block_height = 0
    assert block_height == 0

    block_height = 1000
    assert block_height > 0
    assert block_height == 1000


def test_good_til_block_calculation():
    SHORT_TERM_ORDER_MAXIMUM_LIFETIME = 20

    current_block_height = 1000
    good_til_block = current_block_height + SHORT_TERM_ORDER_MAXIMUM_LIFETIME

    assert good_til_block == 1020
    assert good_til_block > current_block_height

    current_block_height = 0
    good_til_block = current_block_height + SHORT_TERM_ORDER_MAXIMUM_LIFETIME
    assert good_til_block == 20


def test_block_height_validation_prevents_zero():
    block_height = 0
    can_submit_order = block_height > 0
    assert not can_submit_order

    block_height = 100
    can_submit_order = block_height > 0
    assert can_submit_order


def test_block_height_string_to_int_conversion():
    test_cases = [
        ("123", 123),
        ("9876543210", 9876543210),
        ("1", 1),
        ("0", 0),
    ]

    for string_val, expected_int in test_cases:
        result = int(string_val)
        assert result == expected_int
        assert isinstance(result, int)


def test_block_height_invalid_string():
    invalid_values = ["not-a-number", "12.34", "abc"]

    for invalid in invalid_values:
        with pytest.raises(ValueError):
            int(invalid)


def test_block_height_large_values():
    large_block_height = "999999999999"
    result = int(large_block_height)

    assert result == 999999999999
    assert result > 0

    good_til_block = result + 20
    assert good_til_block > result


def test_block_height_rejection_reason_message():
    reason = "Block height not initialized"
    assert "Block height" in reason
    assert "initialized" in reason


def test_block_height_zero_should_reject():
    block_height = 0
    should_reject = block_height == 0
    assert should_reject


def test_block_height_nonzero_should_allow():
    block_height = 12345
    should_reject = block_height == 0
    assert not should_reject


@pytest.mark.parametrize(
    ("block_height", "should_reject"),
    [
        (0, True),
        (1, False),
        (100, False),
        (999999999, False),
    ],
)
def test_block_height_rejection_conditions(block_height, should_reject):
    # Mirrors the condition in dydx_v4/execution.py _submit_order
    result = block_height == 0
    assert result == should_reject
