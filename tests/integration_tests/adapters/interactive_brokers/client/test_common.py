import asyncio
from unittest.mock import Mock

import pytest

from nautilus_trader.adapters.interactive_brokers.client.common import Base
from nautilus_trader.adapters.interactive_brokers.client.common import Requests
from nautilus_trader.adapters.interactive_brokers.client.common import Subscriptions


# Assuming Base and other required classes are imported or defined above


class ConcreteBase(Base):
    def get(self, req_id=None, name=None):
        return "mocked get response"


@pytest.fixture
def base():
    return ConcreteBase()


def test_add_req_id(base):
    mock_handle = Mock()
    mock_cancel = Mock()
    base.add_req_id(1, "test_name", mock_handle, mock_cancel)

    assert 1 in base._req_id_to_name
    assert 1 in base._req_id_to_handle
    assert 1 in base._req_id_to_cancel


def test_remove_req_id_existing(base):
    mock_handle = Mock()
    mock_cancel = Mock()
    base.add_req_id(1, "test_name", mock_handle, mock_cancel)

    base.remove_req_id(1)

    assert 1 not in base._req_id_to_name
    assert 1 not in base._req_id_to_handle
    assert 1 not in base._req_id_to_cancel


def test_remove_req_id_non_existing(base):
    base.remove_req_id(999)  # Removing a non-existing req_id should not raise an error


def test_remove_by_req_id(base):
    mock_handle = Mock()
    mock_cancel = Mock()
    base.add_req_id(1, "test_name", mock_handle, mock_cancel)

    base.remove(req_id=1)

    assert 1 not in base._req_id_to_name


def test_remove_by_name(base):
    mock_handle = Mock()
    mock_cancel = Mock()
    base.add_req_id(1, "test_name", mock_handle, mock_cancel)

    base.remove(name="test_name")

    assert 1 not in base._req_id_to_name


class TestSubscriptions:
    def setup_method(self):
        self.subscriptions = Subscriptions()

    def test_add_subscription(self):
        handle = Mock()
        cancel = Mock()
        subscription = self.subscriptions.add(1, "test", handle, cancel)
        assert subscription.req_id == 1
        assert subscription.name == "test"
        assert subscription.handle == handle
        assert subscription.cancel == cancel
        assert subscription.last is None

    def test_remove_subscription_by_req_id(self):
        self.subscriptions.add(1, "test", Mock(), Mock())
        self.subscriptions.remove(req_id=1)
        assert self.subscriptions.get(req_id=1) is None

    def test_remove_subscription_by_name(self):
        self.subscriptions.add(1, "test", Mock(), Mock())
        self.subscriptions.remove(name="test")
        assert self.subscriptions.get(name="test") is None

    def test_update_last(self):
        self.subscriptions.add(1, "test", Mock(), Mock())
        self.subscriptions.update_last(1, "updated")
        assert self.subscriptions.get(req_id=1).last == "updated"


class TestRequests:
    @pytest.fixture(autouse=True)
    def setup_method(self):
        self.requests = Requests()

    def test_add_request(self):
        handle = Mock()
        cancel = Mock()
        self.requests.add(1, "test", handle, cancel)
        request = self.requests.get(req_id=1)

        assert request.req_id == 1
        assert request.name == "test"
        assert request.handle == handle
        assert request.cancel == cancel
        assert isinstance(request.future, asyncio.Future)
        assert request.result == []

    def test_remove_request_by_req_id(self):
        handle = Mock()
        cancel = Mock()
        self.requests.add(1, "test", handle, cancel)
        self.requests.remove(req_id=1)

        assert self.requests.get(req_id=1) is None

    def test_remove_request_by_name(self):
        handle = Mock()
        cancel = Mock()
        self.requests.add(1, "test", handle, cancel)
        self.requests.remove(name="test")

        assert self.requests.get(name="test") is None
