import pytest
from docker.models.containers import ContainerCollection

from nautilus_trader.adapters.interactive_brokers.gateway import DockerizedIBGateway


pytestmark = pytest.mark.skip(reason="Skip due currently flaky mocks")


def test_gateway_start_no_container(mocker):
    # Arrange
    mock_docker = mocker.patch.object(ContainerCollection, "run")
    gateway = DockerizedIBGateway(username="test", password="test")

    # Act
    gateway.start(wait=None)

    # Assert
    expected = {
        "image": "ghcr.io/unusualalpha/ib-gateway",
        "name": "nautilus-ib-gateway",
        "detach": True,
        "ports": {"4001": "4001", "4002": "4002", "5900": "5900"},
        "platform": "amd64",
        "environment": {
            "TWS_USERID": "test",
            "TWS_PASSWORD": "test",
            "TRADING_MODE": "paper",
            "READ_ONLY_API": "yes",
        },
    }
    result = mock_docker.call_args.kwargs
    assert result == expected
