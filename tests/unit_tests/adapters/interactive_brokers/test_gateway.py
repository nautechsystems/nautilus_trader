from __future__ import annotations

import asyncio
from types import SimpleNamespace
from unittest.mock import MagicMock

from nautilus_trader.adapters.interactive_brokers import factories
from nautilus_trader.adapters.interactive_brokers.config import DockerizedIBGatewayConfig


def test_get_cached_ib_client_skips_gateway_start_for_non_owner(monkeypatch) -> None:
    created_clients: list[object] = []

    class _FakeClient:
        def __init__(self, **kwargs) -> None:
            created_clients.append(kwargs)

        def start(self) -> None:
            return None

    gateway_instance = SimpleNamespace(port=4001, safe_start=MagicMock())

    monkeypatch.setattr(factories, "IB_CLIENTS", {})
    monkeypatch.setattr(factories, "GATEWAYS", {("live",): gateway_instance})
    monkeypatch.setattr(factories, "InteractiveBrokersClient", _FakeClient)

    client = factories.get_cached_ib_client(
        loop=asyncio.new_event_loop(),
        msgbus=MagicMock(),
        cache=MagicMock(),
        clock=MagicMock(),
        host="127.0.0.1",
        port=None,
        client_id=107,
        dockerized_gateway=DockerizedIBGatewayConfig(
            trading_mode="live",
            read_only_api=True,
            manage_container=False,
            auto_restart_time="11:45 PM",
            relogin_after_twofa_timeout=False,
            twofa_timeout_action="exit",
        ),
    )

    assert client is not None
    gateway_instance.safe_start.assert_not_called()
    assert created_clients[0]["port"] == 4001
