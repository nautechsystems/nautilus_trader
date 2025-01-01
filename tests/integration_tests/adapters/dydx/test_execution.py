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
# -------------------------------------------------------------------------------------------------"""Unit tests for the execution engine of dYdX."""
"""
Unit tests for the dYdX execution engine.
"""

from uuid import uuid4

import pytest

from nautilus_trader.adapters.dydx.execution import ClientOrderIdHelper
from nautilus_trader.model.identifiers import ClientOrderId


@pytest.fixture
def client_order_id_helper(cache):
    """
    Create a stub ClientOrderIdHelper.
    """
    return ClientOrderIdHelper(cache=cache)


@pytest.mark.parametrize("order_string", [str(uuid4()), str(12345)])
def test_generate_client_order_id_int_uuid(client_order_id_helper, order_string) -> None:
    """
    Test the generate_client_order_id_int method with a UUID4.
    """
    # Prepare
    client_order_id = ClientOrderId(order_string)

    # Act
    result = client_order_id_helper.generate_client_order_id_int(client_order_id)

    # Assert
    assert isinstance(result, int)


def test_generate_client_order_id_int_with_int(client_order_id_helper) -> None:
    """
    Test the generate_client_order_id_int method with an integer.
    """
    # Prepare
    expected_result = 12345
    client_order_id = ClientOrderId(str(expected_result))

    # Act
    result = client_order_id_helper.generate_client_order_id_int(client_order_id)

    # Assert
    assert result == expected_result


def test_retrieve_from_cache(client_order_id_helper) -> None:
    """
    Test the generate_client_order_id_int method with an integer.
    """
    # Prepare
    client_order_id_int = 12345
    expected_result = ClientOrderId(str(client_order_id_int))
    client_order_id_helper.generate_client_order_id_int(expected_result)

    # Act
    result = client_order_id_helper.get_client_order_id(client_order_id_int)

    # Assert
    assert result.value == expected_result.value
    assert result == expected_result


def test_retrieve_from_empty_cache(client_order_id_helper) -> None:
    """
    Test the generate_client_order_id_int method with an integer.
    """
    # Prepare
    client_order_id_int = 12345
    expected_result = ClientOrderId(str(client_order_id_int))

    # Act
    result = client_order_id_helper.get_client_order_id(client_order_id_int)

    # Assert
    assert result.value == expected_result.value
    assert result == expected_result


def test_retrieve_client_order_id_integer_from_cache(client_order_id_helper) -> None:
    """
    Test the generate_client_order_id_int method with an integer.
    """
    # Prepare
    expected_result = 12345
    client_order_id = ClientOrderId(str(expected_result))
    client_order_id_helper.generate_client_order_id_int(client_order_id)

    # Act
    result = client_order_id_helper.get_client_order_id_int(client_order_id)

    # Assert
    assert result == expected_result


def test_retrieve_client_order_id_integer_from_empty_cache(client_order_id_helper) -> None:
    """
    Test the generate_client_order_id_int method with an integer.
    """
    # Prepare
    expected_result = 12345
    client_order_id = ClientOrderId(str(expected_result))

    # Act
    result = client_order_id_helper.get_client_order_id_int(client_order_id)

    # Assert
    assert result == expected_result
