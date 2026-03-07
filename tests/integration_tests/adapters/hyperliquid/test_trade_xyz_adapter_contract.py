from unittest.mock import AsyncMock
from unittest.mock import MagicMock
from types import SimpleNamespace

import pytest

from nautilus_trader.adapters.hyperliquid.config import HyperliquidExecClientConfig
from nautilus_trader.adapters.hyperliquid.execution import HyperliquidExecutionClient
from nautilus_trader.adapters.hyperliquid.factories import get_cached_hyperliquid_http_client
from nautilus_trader.flux.runners.live.hyperliquid_account import HyperliquidUserResolutionError
from tests.integration_tests.adapters.hyperliquid.conftest import _create_ws_mock


def test_cached_http_client_passes_account_address_and_dex(monkeypatch):
    captured: dict[str, object] = {}

    def fake_http_client(
        *,
        private_key=None,
        account_address=None,
        vault_address=None,
        is_testnet=False,
        timeout_secs=None,
        proxy_url=None,
        normalize_prices=True,
        dex=None,
    ):
        captured.update(
            {
                "private_key": private_key,
                "account_address": account_address,
                "vault_address": vault_address,
                "is_testnet": is_testnet,
                "timeout_secs": timeout_secs,
                "proxy_url": proxy_url,
                "normalize_prices": normalize_prices,
                "dex": dex,
            },
        )
        return MagicMock(name="HyperliquidHttpClient")

    monkeypatch.setattr(
        "nautilus_trader.adapters.hyperliquid.factories.nautilus_pyo3.HyperliquidHttpClient",
        fake_http_client,
    )
    get_cached_hyperliquid_http_client.cache_clear()

    get_cached_hyperliquid_http_client(
        private_key="0xabc",
        account_address="0xdef",
        vault_address="0x123",
        timeout_secs=11,
        testnet=True,
        proxy_url="http://proxy:8080",
        normalize_prices=False,
        dex="xyz",
    )

    assert captured == {
        "private_key": "0xabc",
        "account_address": "0xdef",
        "vault_address": "0x123",
        "is_testnet": True,
        "timeout_secs": 11,
        "proxy_url": "http://proxy:8080",
        "normalize_prices": False,
        "dex": "xyz",
    }


def _build_exec_client(
    monkeypatch,
    *,
    event_loop,
    mock_http_client,
    msgbus,
    cache,
    live_clock,
    mock_instrument_provider,
    config_kwargs: dict | None = None,
):
    ws_client = _create_ws_mock()
    monkeypatch.setattr(
        "nautilus_trader.adapters.hyperliquid.execution.nautilus_pyo3.HyperliquidWebSocketClient",
        lambda *args, **kwargs: ws_client,
    )
    monkeypatch.setattr(
        "nautilus_trader.adapters.hyperliquid.execution.HyperliquidExecutionClient._await_account_registered",
        AsyncMock(),
    )

    mock_http_client.reset_mock()
    mock_http_client.get_user_address = MagicMock(
        return_value="0x1111111111111111111111111111111111111111",
    )
    mock_http_client.get_spot_fill_coin_mapping = MagicMock(return_value={})
    mock_instrument_provider.initialize.reset_mock()
    mock_instrument_provider.instruments_pyo3.reset_mock()
    mock_instrument_provider.instruments_pyo3.return_value = []

    config = HyperliquidExecClientConfig(
        testnet=False,
        private_key="0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef",
        **(config_kwargs or {}),
    )

    client = HyperliquidExecutionClient(
        loop=event_loop,
        client=mock_http_client,
        msgbus=msgbus,
        cache=cache,
        clock=live_clock,
        instrument_provider=mock_instrument_provider,
        config=config,
        name=None,
    )
    return client, ws_client


@pytest.mark.asyncio
async def test_execution_client_prefers_explicit_funded_account_for_queries_and_ws(
    monkeypatch,
    event_loop,
    mock_http_client,
    msgbus,
    cache,
    live_clock,
    mock_instrument_provider,
):
    client, ws_client = _build_exec_client(
        monkeypatch,
        event_loop=event_loop,
        mock_http_client=mock_http_client,
        msgbus=msgbus,
        cache=cache,
        live_clock=live_clock,
        mock_instrument_provider=mock_instrument_provider,
        config_kwargs={
            "account_address": "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "dex": "xyz",
        },
    )

    await client._connect()

    try:
        assert client._resolved_user.execution_signer == (
            "0x1111111111111111111111111111111111111111"
        )
        assert client._resolved_user.source == "account_address"
        mock_http_client.request_account_state.assert_awaited_once_with(
            account_address="0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            dex="xyz",
        )
        ws_client.subscribe_order_updates.assert_awaited_once_with(
            "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
        ws_client.subscribe_user_events.assert_awaited_once_with(
            "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        )
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_execution_client_prefers_explicit_vault_over_funded_account_for_queries_and_ws(
    monkeypatch,
    event_loop,
    mock_http_client,
    msgbus,
    cache,
    live_clock,
    mock_instrument_provider,
):
    client, ws_client = _build_exec_client(
        monkeypatch,
        event_loop=event_loop,
        mock_http_client=mock_http_client,
        msgbus=msgbus,
        cache=cache,
        live_clock=live_clock,
        mock_instrument_provider=mock_instrument_provider,
        config_kwargs={
            "account_address": "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "vault_address": "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "dex": "xyz",
        },
    )

    await client._connect()

    try:
        assert client._resolved_user.execution_signer == (
            "0x1111111111111111111111111111111111111111"
        )
        assert client._resolved_user.source == "vault_address"
        mock_http_client.request_account_state.assert_awaited_once_with(
            account_address="0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            dex="xyz",
        )
        ws_client.subscribe_order_updates.assert_awaited_once_with(
            "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        )
        ws_client.subscribe_user_events.assert_awaited_once_with(
            "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        )
    finally:
        await client._disconnect()


@pytest.mark.asyncio
async def test_execution_client_resolves_agent_wallet_to_master_for_queries_and_ws(
    monkeypatch,
    event_loop,
    mock_http_client,
    msgbus,
    cache,
    live_clock,
    mock_instrument_provider,
):
    monkeypatch.setattr(
        "nautilus_trader.adapters.hyperliquid.execution.resolve_hyperliquid_user",
        lambda **kwargs: SimpleNamespace(
            execution_signer="0x1111111111111111111111111111111111111111",
            account_query_address="0x9999999999999999999999999999999999999999",
            fee_query_address="0x9999999999999999999999999999999999999999",
            ws_subscription_address="0x9999999999999999999999999999999999999999",
            source="user_role_master",
        ),
        raising=False,
    )

    client, ws_client = _build_exec_client(
        monkeypatch,
        event_loop=event_loop,
        mock_http_client=mock_http_client,
        msgbus=msgbus,
        cache=cache,
        live_clock=live_clock,
        mock_instrument_provider=mock_instrument_provider,
        config_kwargs={"dex": "xyz"},
    )

    await client._connect()

    try:
        assert client._resolved_user.execution_signer == (
            "0x1111111111111111111111111111111111111111"
        )
        assert client._resolved_user.source == "user_role_master"
        mock_http_client.request_account_state.assert_awaited_once_with(
            account_address="0x9999999999999999999999999999999999999999",
            dex="xyz",
        )
        ws_client.subscribe_order_updates.assert_awaited_once_with(
            "0x9999999999999999999999999999999999999999",
        )
        ws_client.subscribe_user_events.assert_awaited_once_with(
            "0x9999999999999999999999999999999999999999",
        )
    finally:
        await client._disconnect()


def test_execution_client_fails_fast_when_effective_user_resolution_fails(
    monkeypatch,
    event_loop,
    mock_http_client,
    msgbus,
    cache,
    live_clock,
    mock_instrument_provider,
):
    monkeypatch.setattr(
        "nautilus_trader.adapters.hyperliquid.execution.resolve_hyperliquid_user",
        MagicMock(side_effect=HyperliquidUserResolutionError("userRole lookup failed")),
    )

    with pytest.raises(HyperliquidUserResolutionError, match="userRole lookup failed"):
        _build_exec_client(
            monkeypatch,
            event_loop=event_loop,
            mock_http_client=mock_http_client,
            msgbus=msgbus,
            cache=cache,
            live_clock=live_clock,
            mock_instrument_provider=mock_instrument_provider,
            config_kwargs={"dex": "xyz"},
        )
