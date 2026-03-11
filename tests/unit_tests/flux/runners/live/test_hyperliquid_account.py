from __future__ import annotations

import importlib
import pytest
from unittest.mock import MagicMock

from nautilus_trader.flux.runners.live.hyperliquid_account import (
    HyperliquidUserResolutionError,
    _resolve_user_role_master_address,
)
from nautilus_trader.flux.runners.live.hyperliquid_account import resolve_hyperliquid_user


def test_resolve_user_role_master_address_parses_realistic_payload() -> None:
    execution_signer = "0x1111111111111111111111111111111111111111"
    master_address = "0x9999999999999999999999999999999999999999"
    captured: dict[str, object] = {}

    def fake_info_client(*, payload, testnet, timeout_secs, http_proxy_url):
        captured.update(
            {
                "payload": payload,
                "testnet": testnet,
                "timeout_secs": timeout_secs,
                "http_proxy_url": http_proxy_url,
            },
        )
        return {
            "role": "agent",
            "data": {
                "user": execution_signer,
                "masterAddress": master_address,
            },
        }

    resolved = _resolve_user_role_master_address(
        execution_signer=execution_signer,
        testnet=False,
        timeout_secs=7,
        http_proxy_url="http://proxy:8080",
        info_client=fake_info_client,
    )

    assert resolved == master_address
    assert captured == {
        "payload": {"type": "userRole", "user": execution_signer},
        "testnet": False,
        "timeout_secs": 7,
        "http_proxy_url": "http://proxy:8080",
    }


def test_resolve_user_role_master_address_parses_live_agent_payload_shape() -> None:
    execution_signer = "0x1111111111111111111111111111111111111111"
    master_address = "0x6ed25f0c7497ccfb5ab429b0f195ba87052b5249"

    resolved = _resolve_user_role_master_address(
        execution_signer=execution_signer,
        testnet=False,
        timeout_secs=7,
        http_proxy_url=None,
        info_client=lambda **kwargs: {
            "role": "agent",
            "data": {"user": master_address},
        },
    )

    assert resolved == master_address


def test_resolve_hyperliquid_user_prefers_explicit_account_address() -> None:
    client = MagicMock()
    client.get_user_address.return_value = "0x1111111111111111111111111111111111111111"
    info_client = MagicMock()

    resolved = resolve_hyperliquid_user(
        client=client,
        account_address="0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        vault_address=None,
        testnet=False,
        http_timeout_secs=10,
        http_proxy_url=None,
        info_client=info_client,
    )

    assert resolved.execution_signer == "0x1111111111111111111111111111111111111111"
    assert resolved.account_query_address == "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    assert resolved.fee_query_address == "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    assert resolved.ws_subscription_address == "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    assert resolved.source == "account_address"
    info_client.assert_not_called()


def test_resolve_hyperliquid_user_prefers_explicit_vault_address() -> None:
    client = MagicMock()
    client.get_user_address.return_value = "0x1111111111111111111111111111111111111111"
    info_client = MagicMock()

    resolved = resolve_hyperliquid_user(
        client=client,
        account_address="0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        vault_address="0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        testnet=False,
        http_timeout_secs=10,
        http_proxy_url=None,
        info_client=info_client,
    )

    assert resolved.execution_signer == "0x1111111111111111111111111111111111111111"
    assert resolved.account_query_address == "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    assert resolved.fee_query_address == "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    assert resolved.ws_subscription_address == "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
    assert resolved.source == "vault_address"
    info_client.assert_not_called()


def test_resolve_hyperliquid_user_keeps_ws_subscription_consistent_with_master() -> None:
    execution_signer = "0x1111111111111111111111111111111111111111"
    master_address = "0x9999999999999999999999999999999999999999"
    client = MagicMock()
    client.get_user_address.return_value = execution_signer

    def fake_info_client(*, payload, testnet, timeout_secs, http_proxy_url):
        assert payload == {"type": "userRole", "user": execution_signer}
        assert testnet is False
        assert timeout_secs == 10
        assert http_proxy_url is None
        return {
            "role": "agent",
            "data": {
                "user": execution_signer,
                "masterAddress": master_address,
            },
        }

    resolved = resolve_hyperliquid_user(
        client=client,
        account_address=None,
        vault_address=None,
        testnet=False,
        http_timeout_secs=10,
        http_proxy_url=None,
        info_client=fake_info_client,
    )

    assert resolved.execution_signer == execution_signer
    assert resolved.account_query_address == master_address
    assert resolved.fee_query_address == master_address
    assert resolved.ws_subscription_address == master_address
    assert resolved.source == "user_role_master"


def test_resolve_hyperliquid_user_raises_when_user_role_lookup_fails() -> None:
    execution_signer = "0x1111111111111111111111111111111111111111"
    client = MagicMock()
    client.get_user_address.return_value = execution_signer

    def failing_info_client(*, payload, testnet, timeout_secs, http_proxy_url):
        assert payload == {"type": "userRole", "user": execution_signer}
        raise OSError("proxy timeout")

    with pytest.raises(HyperliquidUserResolutionError, match="userRole"):
        resolve_hyperliquid_user(
            client=client,
            account_address=None,
            vault_address=None,
            testnet=False,
            http_timeout_secs=10,
            http_proxy_url="http://proxy:8080",
            info_client=failing_info_client,
        )


def test_resolve_hyperliquid_user_fails_closed_for_agent_role_without_distinct_master() -> None:
    execution_signer = "0x1111111111111111111111111111111111111111"
    client = MagicMock()
    client.get_user_address.return_value = execution_signer

    with pytest.raises(HyperliquidUserResolutionError, match="agent"):
        resolve_hyperliquid_user(
            client=client,
            account_address=None,
            vault_address=None,
            testnet=False,
            http_timeout_secs=10,
            http_proxy_url=None,
            info_client=lambda **kwargs: {
                "role": "agent",
                "data": {"user": execution_signer},
            },
        )


def test_hyperliquid_account_module_aliases_share_identity() -> None:
    module_flux = importlib.import_module("flux.runners.live.hyperliquid_account")
    module_compat = importlib.import_module("nautilus_trader.flux.runners.live.hyperliquid_account")

    assert module_flux is module_compat
    assert module_flux.HyperliquidUserResolutionError is module_compat.HyperliquidUserResolutionError


def test_hyperliquid_live_package_aliases_share_identity() -> None:
    package_flux = importlib.import_module("flux.runners.live")
    package_compat = importlib.import_module("nautilus_trader.flux.runners.live")

    assert package_flux is package_compat
