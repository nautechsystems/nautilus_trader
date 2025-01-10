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

import asyncio
from unittest.mock import Mock

import pytest

from nautilus_trader.adapters.interactive_brokers.client.common import Base
from nautilus_trader.adapters.interactive_brokers.client.common import Requests
from nautilus_trader.adapters.interactive_brokers.client.common import Subscriptions


class ConcreteBase(Base):
    def get(self, req_id=None, name=None):
        return "mocked get response"


@pytest.fixture
def base():
    return ConcreteBase()


@pytest.fixture
def subscriptions():
    return Subscriptions()


@pytest.fixture
def requests():
    return Requests()


@pytest.fixture
def mock_handle():
    return Mock()


@pytest.fixture
def mock_cancel():
    return Mock()


def test_add_req_id(base, mock_handle, mock_cancel):
    # Arrange

    # Act
    base.add_req_id(1, "test_name", mock_handle, mock_cancel)

    # Assert
    assert 1 in base._req_id_to_name
    assert 1 in base._req_id_to_handle
    assert 1 in base._req_id_to_cancel


def test_remove_req_id_existing(base, mock_handle, mock_cancel):
    # Arrange
    base.add_req_id(1, "test_name", mock_handle, mock_cancel)

    # Act
    base.remove_req_id(1)

    # Assert
    assert 1 not in base._req_id_to_name
    assert 1 not in base._req_id_to_handle
    assert 1 not in base._req_id_to_cancel


def test_remove_req_id_non_existing(base):
    base.remove_req_id(999)  # Removing a non-existing req_id should not raise an error


def test_remove_by_req_id(base, mock_handle, mock_cancel):
    # Arrange
    base.add_req_id(1, "test_name", mock_handle, mock_cancel)

    # Act
    base.remove(req_id=1)

    # Assert
    assert 1 not in base._req_id_to_name


def test_remove_by_name(base, mock_handle, mock_cancel):
    # Arrange
    base.add_req_id(1, "test_name", mock_handle, mock_cancel)

    # Act
    base.remove(name="test_name")

    # Assert
    assert 1 not in base._req_id_to_name


def test_add_subscription(subscriptions, mock_handle, mock_cancel):
    # Arrange

    # Act
    subscription = subscriptions.add(1, "test", mock_handle, mock_cancel)

    # Assert
    assert subscription.req_id == 1
    assert subscription.name == "test"
    assert subscription.handle == mock_handle
    assert subscription.cancel == mock_cancel
    assert subscription.last is None


def test_remove_subscription_by_req_id(subscriptions, mock_handle, mock_cancel):
    # Arrange
    subscriptions.add(1, "test", mock_handle, mock_cancel)

    # Act
    subscriptions.remove(req_id=1)

    # Assert
    assert subscriptions.get(req_id=1) is None


def test_remove_subscription_by_name(subscriptions, mock_handle, mock_cancel):
    # Arrange
    subscriptions.add(1, "test", mock_handle, mock_cancel)

    # Act
    subscriptions.remove(name="test")

    # Assert
    assert subscriptions.get(name="test") is None


def test_update_last(subscriptions, mock_handle, mock_cancel):
    # Arrange
    subscriptions.add(1, "test", mock_handle, mock_cancel)

    # Act
    subscriptions.update_last(1, "updated")

    # Assert
    assert subscriptions.get(req_id=1).last == "updated"


def test_add_request(requests, mock_handle, mock_cancel):
    # Arrange

    # Act
    requests.add(1, "test", mock_handle, mock_cancel)
    request = requests.get(req_id=1)

    # Assert
    assert request.req_id == 1
    assert request.name == "test"
    assert request.handle == mock_handle
    assert request.cancel == mock_cancel
    assert isinstance(request.future, asyncio.Future)
    assert request.result == []


def test_remove_request_by_req_id(requests, mock_handle, mock_cancel):
    # Arrange
    requests.add(1, "test", mock_handle, mock_cancel)

    # Act
    requests.remove(req_id=1)

    # Assert
    assert requests.get(req_id=1) is None


def test_remove_request_by_name(requests, mock_handle, mock_cancel):
    # Arrange
    requests.add(1, "test", mock_handle, mock_cancel)

    # Act
    requests.remove(name="test")

    # Assert
    assert requests.get(name="test") is None
