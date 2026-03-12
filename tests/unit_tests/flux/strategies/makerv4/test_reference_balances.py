from __future__ import annotations

import asyncio

from flux.strategies.makerv4 import reference_balances


def test_ibkr_reference_balance_provider_refresh_reuses_standalone_loop(
    monkeypatch,
) -> None:
    refresh_count = 0
    cached_client = None

    class _FakeClient:
        def __init__(self, loop: asyncio.AbstractEventLoop) -> None:
            self._loop = loop

        async def wait_until_ready(self, _timeout: int) -> None:
            if asyncio.get_running_loop() is not self._loop:
                raise RuntimeError("bound to a different event loop")

        def accounts(self) -> set[str]:
            return {"U1234567"}

    def _fake_get_cached_ib_client(**kwargs):
        nonlocal cached_client
        if cached_client is None:
            cached_client = _FakeClient(kwargs["loop"])
        return cached_client

    async def _fake_fetch_snapshot(self, strategy):
        nonlocal refresh_count
        loop = asyncio.get_running_loop()
        client = reference_balances.get_cached_ib_client(
            loop=loop,
            msgbus=strategy.msgbus,
            cache=strategy.cache,
            clock=strategy.clock,
            host=self._config.ibg_host,
            port=self._config.ibg_port,
            client_id=self._config.ibg_client_id,
            dockerized_gateway=self._config.dockerized_gateway,
            request_timeout_secs=self._config.request_timeout_secs,
        )
        await client.wait_until_ready(self._config.connection_timeout)
        refresh_count += 1
        return {
            "accounts": [],
            "positions": [],
            "rows": [
                {
                    "row_id": f"refresh-{refresh_count}",
                },
            ],
        }

    monkeypatch.setattr(reference_balances, "get_cached_ib_client", _fake_get_cached_ib_client)
    monkeypatch.setattr(
        reference_balances.IbkrReferenceBalanceSnapshotProvider,
        "_fetch_snapshot",
        _fake_fetch_snapshot,
    )

    provider = reference_balances.IbkrReferenceBalanceSnapshotProvider(
        reference_balances.IbkrReferenceBalanceSnapshotProviderConfig(
            refresh_interval_secs=0.0,
        ),
    )

    first = provider.refresh()
    second = provider.refresh()

    assert first is not None
    assert second is not None
    assert first["rows"][0]["row_id"] == "refresh-1"
    assert second["rows"][0]["row_id"] == "refresh-2"
