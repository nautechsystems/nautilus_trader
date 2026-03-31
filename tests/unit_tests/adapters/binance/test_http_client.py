from __future__ import annotations

from types import SimpleNamespace

import pytest

from nautilus_trader.adapters.binance.http import client as client_module
from nautilus_trader.common.component import LiveClock
from nautilus_trader.core.nautilus_pyo3 import HttpMethod


@pytest.mark.asyncio
async def test_binance_http_client_forwards_timeout_secs_to_underlying_http_client(
    monkeypatch,
) -> None:
    captured: dict[str, dict[str, object]] = {}

    class _FakeHttpClient:
        def __init__(self, **kwargs) -> None:
            captured["init"] = kwargs

        async def request(self, *args, **kwargs):
            captured["request"] = {"args": args, "kwargs": kwargs}
            return SimpleNamespace(status=200, body=b"{}", headers={})

    monkeypatch.setattr(client_module, "HttpClient", _FakeHttpClient)

    client = client_module.BinanceHttpClient(
        clock=LiveClock(),
        api_key="api-key",
        api_secret="api-secret",
        base_url="https://api.binance.com",
        timeout_secs=7,
    )

    assert captured["init"]["timeout_secs"] == 7

    await client.send_request(HttpMethod.GET, "/api/v3/ping")

    assert captured["request"]["kwargs"]["timeout_secs"] == 7
