import pytest

from nautilus_trader.adapters._template.execution import TemplateLiveExecutionClient
from nautilus_trader.live.execution_client import LiveExecutionClient


pytestmark = pytest.mark.skip(reason="template")


@pytest.fixture
def execution_client() -> LiveExecutionClient:
    return TemplateLiveExecutionClient()  # type: ignore


def test_connect(execution_client: LiveExecutionClient):
    execution_client.connect()
    assert execution_client.is_connected


def test_disconnect(execution_client: LiveExecutionClient):
    execution_client.connect()
    execution_client.disconnect()
    assert not execution_client.is_connected


def test_submit_order(execution_client: LiveExecutionClient):
    pass


def test_submit_bracket_order(execution_client: LiveExecutionClient):
    pass


def test_modify_order(execution_client: LiveExecutionClient):
    pass


def test_cancel_order(execution_client: LiveExecutionClient):
    pass


def test_generate_order_status_report(execution_client: LiveExecutionClient):
    pass
