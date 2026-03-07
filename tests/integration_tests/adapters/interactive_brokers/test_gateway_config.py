from __future__ import annotations

from types import SimpleNamespace
from unittest.mock import MagicMock
from unittest.mock import patch

from nautilus_trader.adapters.interactive_brokers.config import DockerizedIBGatewayConfig
from nautilus_trader.adapters.interactive_brokers.gateway import DockerizedIBGateway


def test_gateway_start_passes_auto_restart_env() -> None:
    mock_container = MagicMock()
    mock_container.logs.return_value = b"Forking :::\n"

    mock_docker = SimpleNamespace(
        containers=SimpleNamespace(
            list=MagicMock(return_value=[]),
            run=MagicMock(return_value=mock_container),
        ),
    )

    with patch("docker.from_env", return_value=mock_docker):
        gateway = DockerizedIBGateway(
            DockerizedIBGatewayConfig(
                username="user",
                password="pass",
                trading_mode="live",
                read_only_api=True,
                auto_restart_time="11:45 PM",
                time_zone="America/New_York",
                relogin_after_twofa_timeout=True,
            ),
        )

        gateway.start(wait=1)

    environment = mock_docker.containers.run.call_args.kwargs["environment"]
    assert environment["TRADING_MODE"] == "live"
    assert environment["READ_ONLY_API"] == "yes"
    assert environment["AUTO_RESTART_TIME"] == "11:45 PM"
    assert environment["TIME_ZONE"] == "America/New_York"
    assert environment["TZ"] == "America/New_York"
    assert environment["RELOGIN_AFTER_TWOFA_TIMEOUT"] == "yes"
    assert environment["TWOFA_TIMEOUT_ACTION"] == "restart"
