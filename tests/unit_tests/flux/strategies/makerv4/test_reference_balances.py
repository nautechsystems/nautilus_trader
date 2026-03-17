from __future__ import annotations

import asyncio
from decimal import Decimal

from nautilus_trader.flux.strategies.makerv4 import (
    reference_balances as makerv4_reference_balances,
)
from flux.strategies.shared.equities_arb import reference_balances


def test_makerv4_reference_balances_reexport_shared_equities_arb_provider() -> None:
    assert (
        makerv4_reference_balances.IbkrReferenceBalanceSnapshotProvider
        is reference_balances.IbkrReferenceBalanceSnapshotProvider
    )
    assert (
        makerv4_reference_balances.IbkrReferenceBalanceSnapshotProviderConfig
        is reference_balances.IbkrReferenceBalanceSnapshotProviderConfig
    )
    assert (
        makerv4_reference_balances.get_cached_ibkr_reference_balance_provider
        is reference_balances.get_cached_ibkr_reference_balance_provider
    )


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


def test_ibkr_reference_balance_provider_keeps_total_cash_only_currency_rows() -> None:
    class _FakeClient:
        def __init__(self) -> None:
            self._callbacks = {}

        def subscribe_event(self, name: str, callback) -> None:
            self._callbacks[name] = callback

        def subscribe_account_summary(self) -> None:
            callback = self._callbacks["accountSummary-U1234567"]
            for tag, value, currency in (
                ("NetLiquidation", "85671.33", "HKD"),
                ("FullAvailableFunds", "85671.33", "HKD"),
                ("FullInitMarginReq", "0", "HKD"),
                ("FullMaintMarginReq", "0", "HKD"),
                ("TotalCashValue", "85671.33", "HKD"),
                ("TotalCashValue", "1250.50", "USD"),
            ):
                callback(tag, value, currency)
            self._callbacks["accountSummaryEnd"](0)

        def unsubscribe_event(self, name: str) -> None:
            self._callbacks.pop(name, None)

    provider = reference_balances.IbkrReferenceBalanceSnapshotProvider(
        reference_balances.IbkrReferenceBalanceSnapshotProviderConfig(
            request_timeout_secs=1,
        ),
    )

    payload = asyncio.run(
        provider._fetch_account_payload(
            _FakeClient(),
            account_id="U1234567",
            ts_ms=1_700_000_000_000,
        ),
    )

    balances = payload["events"][0]["balances"]
    usd_balance = next(balance for balance in balances if balance["currency"] == "USD")

    assert Decimal(usd_balance["total"]) == Decimal("1250.50")
    assert Decimal(usd_balance["free"]) == Decimal("1250.50")
    assert Decimal(usd_balance["locked"]) == Decimal("0")


def test_ibkr_reference_balance_provider_waits_for_account_summary_end() -> None:
    class _FakeClient:
        def __init__(self) -> None:
            self._callbacks = {}

        def subscribe_event(self, name: str, callback) -> None:
            self._callbacks[name] = callback

        def subscribe_account_summary(self) -> None:
            callback = self._callbacks["accountSummary-U1234567"]
            callback("NetLiquidation", "85671.33", "HKD")
            callback("FullAvailableFunds", "85671.33", "HKD")
            callback("FullInitMarginReq", "0", "HKD")
            callback("FullMaintMarginReq", "0", "HKD")
            callback("TotalCashValue", "85671.33", "HKD")
            loop = asyncio.get_running_loop()
            loop.call_soon(callback, "TotalCashValue", "1250.50", "USD")
            loop.call_soon(self._callbacks["accountSummaryEnd"], 0)

        def unsubscribe_event(self, name: str) -> None:
            self._callbacks.pop(name, None)

    provider = reference_balances.IbkrReferenceBalanceSnapshotProvider(
        reference_balances.IbkrReferenceBalanceSnapshotProviderConfig(
            request_timeout_secs=1,
        ),
    )

    payload = asyncio.run(
        provider._fetch_account_payload(
            _FakeClient(),
            account_id="U1234567",
            ts_ms=1_700_000_000_000,
        ),
    )

    balances = {
        balance["currency"]: balance
        for balance in payload["events"][0]["balances"]
    }

    assert set(balances) == {"HKD", "USD"}
