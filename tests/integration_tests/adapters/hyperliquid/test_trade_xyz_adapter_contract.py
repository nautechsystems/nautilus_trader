from unittest.mock import MagicMock

from nautilus_trader.adapters.hyperliquid.factories import get_cached_hyperliquid_http_client


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
