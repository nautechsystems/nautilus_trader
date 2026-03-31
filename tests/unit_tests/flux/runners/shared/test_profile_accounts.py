from __future__ import annotations

import sys
from types import ModuleType

import pytest

_IB_PACKAGE_STUB = ModuleType("nautilus_trader.adapters.interactive_brokers")
_IB_PACKAGE_STUB.__path__ = []
_IB_COMMON_STUB = ModuleType("nautilus_trader.adapters.interactive_brokers.common")
_IB_COMMON_STUB.IBOrderTags = lambda **_kwargs: None
_IB_COMMON_STUB.IB_CLIENT_ID = "INTERACTIVE_BROKERS"
_IB_COMMON_STUB.IB_VENUE = "INTERACTIVE_BROKERS"
_IB_CONFIG_STUB = ModuleType("nautilus_trader.adapters.interactive_brokers.config")
_IB_CONFIG_STUB.DockerizedIBGatewayConfig = object
_IB_SHARED_REFERENCE_STUB = ModuleType("nautilus_trader.adapters.interactive_brokers.shared_reference")
_IB_SHARED_REFERENCE_STUB.InteractiveBrokersSharedReferenceDataClientConfig = object
_IB_SHARED_REFERENCE_STUB.InteractiveBrokersSharedReferenceLiveDataClientFactory = object
sys.modules.setdefault(_IB_PACKAGE_STUB.__name__, _IB_PACKAGE_STUB)
sys.modules.setdefault(_IB_COMMON_STUB.__name__, _IB_COMMON_STUB)
sys.modules.setdefault(_IB_CONFIG_STUB.__name__, _IB_CONFIG_STUB)
sys.modules.setdefault(_IB_SHARED_REFERENCE_STUB.__name__, _IB_SHARED_REFERENCE_STUB)

import flux.runners.shared.profile_accounts as profile_accounts_module
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.spot.schemas.account import BinancePortfolioMarginAccountInfo
from nautilus_trader.adapters.binance.spot.schemas.account import BinancePortfolioMarginBalanceInfo
from nautilus_trader.flux.common.account_scopes import AccountScopeConfig
from flux.runners.shared.profile_accounts import (
    _build_binance_spot_margin_account_snapshot,
)


def test_build_binance_spot_margin_account_snapshot_publishes_shared_cash_rows() -> None:
    snapshot = _build_binance_spot_margin_account_snapshot(
        account_info=BinancePortfolioMarginAccountInfo(
            balances=[
                BinancePortfolioMarginBalanceInfo(
                    asset="USDT",
                    totalWalletBalance="1285.28070703",
                    crossMarginAsset="1285.28070703",
                    crossMarginBorrowed="0",
                    crossMarginInterest="0",
                    crossMarginLocked="0",
                    updateTime=1_700_000_000_100,
                ),
            ],
            updateTime=1_700_000_000_100,
        ),
        account_id="BINANCE-main",
        exchange="binance_spot",
        ts_ms=1_700_000_000_100,
    )

    assert snapshot["source_scope"] == "shared_account"
    assert snapshot["totals"] == {}
    assert len(snapshot["rows"]) == 1
    row = snapshot["rows"][0]
    assert row["account"] == "BINANCE-main"
    assert row["account_id"] == "BINANCE-main"
    assert row["asset"] == "USDT"
    assert row["base"] == "USDT"
    assert row["coin"] == "USDT"
    assert row["exchange"] == "binance_spot"
    assert row["free"] == "1285.28070703"
    assert row["locked"] == "0.00000000"
    assert row["mark_raw"] == 1.0
    assert row["market_type"] == "spot"
    assert row["mv_raw"] == 1285.28070703
    assert row["product_type"] == "spot"
    assert row["total"] == "1285.28070703"
    assert row["ts_ms"] == 1_700_000_000_100


def test_binance_spot_margin_projection_provider_marks_cached_snapshot_unhealthy_after_refresh_failure(
    monkeypatch,
) -> None:
    monkeypatch.setattr(
        profile_accounts_module,
        "get_cached_binance_http_client",
        lambda **_kwargs: object(),
    )

    class _DummySpotAccountHttpAPI:
        def __init__(self, *, client, clock, account_type) -> None:
            self.client = client
            self.clock = clock
            self.account_type = account_type

    monkeypatch.setattr(
        profile_accounts_module,
        "BinanceSpotAccountHttpAPI",
        _DummySpotAccountHttpAPI,
    )

    monotonic_values = iter([0.0, 0.0, 2.0, 2.0])
    time_values = iter([1_700_000_000.0, 1_700_000_002.5, 1_700_000_002.5])
    monkeypatch.setattr(profile_accounts_module.time, "monotonic", lambda: next(monotonic_values))
    monkeypatch.setattr(profile_accounts_module.time, "time", lambda: next(time_values))

    provider = profile_accounts_module.BinanceSpotMarginAccountProjectionProvider(
        profile_accounts_module.BinanceSpotMarginAccountProjectionProviderConfig(
            api_key="api-key",
            api_secret="api-secret",
            account_type=BinanceAccountType.PORTFOLIO_MARGIN,
            refresh_interval_secs=1.0,
        ),
    )

    async def _success() -> dict[str, object]:
        return {
            "source_scope": "shared_account",
            "rows": [
                {
                    "account": "BINANCE-main",
                    "account_id": "BINANCE-main",
                    "asset": "USDT",
                    "exchange": "binance_spot",
                    "free": "1285.28070703",
                    "total": "1285.28070703",
                    "ts_ms": 1_700_000_000_000,
                },
            ],
            "totals": {},
        }

    monkeypatch.setattr(provider, "_fetch_snapshot", _success)
    healthy_snapshot = provider.refresh()

    assert healthy_snapshot is not None
    assert healthy_snapshot["projection_status"]["healthy"] is True
    assert healthy_snapshot["rows"][0]["stale"] is False
    assert healthy_snapshot["rows"][0]["include_in_reconciliation"] is True

    async def _failure() -> dict[str, object]:
        raise TimeoutError("boom")

    monkeypatch.setattr(provider, "_fetch_snapshot", _failure)
    failed_snapshot = provider.refresh()

    assert failed_snapshot is not None
    assert failed_snapshot["projection_status"]["healthy"] is False
    assert failed_snapshot["projection_status"]["last_error_type"] == "TimeoutError"
    assert failed_snapshot["projection_status"]["last_success_ts_ms"] == 1_700_000_000_000
    assert failed_snapshot["rows"][0]["stale"] is True
    assert failed_snapshot["rows"][0]["include_in_reconciliation"] is False


def test_build_account_projection_provider_routes_binance_portfolio_margin_private_api_to_papi(
    monkeypatch,
) -> None:
    monkeypatch.setenv("BINANCE_API_KEY", "api-key")
    monkeypatch.setenv("BINANCE_API_SECRET", "api-secret")

    captured_kwargs: dict[str, object] = {}

    def _capture_http_client(**kwargs):
        captured_kwargs.update(kwargs)
        return object()

    monkeypatch.setattr(
        profile_accounts_module,
        "get_cached_binance_http_client",
        _capture_http_client,
    )

    class _DummySpotAccountHttpAPI:
        def __init__(self, *, client, clock, account_type) -> None:
            self.client = client
            self.clock = clock
            self.account_type = account_type

    monkeypatch.setattr(
        profile_accounts_module,
        "BinanceSpotAccountHttpAPI",
        _DummySpotAccountHttpAPI,
    )

    scope_config = AccountScopeConfig(
        scope_id="binance.pm.main",
        provider="binance",
        venue="BINANCE",
        api_key_env="BINANCE_API_KEY",
        api_secret_env="BINANCE_API_SECRET",
        account_type="PORTFOLIO_MARGIN",
        private_api_family="PORTFOLIO_MARGIN",
        http_timeout_secs=17,
    )

    provider = profile_accounts_module.build_account_projection_provider(
        scope_config=scope_config,
        account_scope_id=scope_config.scope_id,
        source_strategy_ids=("plumeusdt_binance_spot_makerv3",),
    )

    assert isinstance(provider, profile_accounts_module.BinanceSpotMarginAccountProjectionProvider)
    assert captured_kwargs["account_type"] == BinanceAccountType.PORTFOLIO_MARGIN
    assert captured_kwargs["base_url"] == "https://papi.binance.com"
    assert captured_kwargs["timeout_secs"] == 17


def test_build_account_projection_provider_rejects_binance_isolated_margin_scope(
    monkeypatch,
) -> None:
    monkeypatch.setenv("BINANCE_API_KEY", "api-key")
    monkeypatch.setenv("BINANCE_API_SECRET", "api-secret")

    scope_config = AccountScopeConfig(
        scope_id="binance.isolated.main",
        provider="binance",
        venue="BINANCE",
        api_key_env="BINANCE_API_KEY",
        api_secret_env="BINANCE_API_SECRET",
        account_type="ISOLATED_MARGIN",
    )

    with pytest.raises(ValueError, match="ISOLATED_MARGIN"):
        profile_accounts_module.build_account_projection_provider(
            scope_config=scope_config,
            account_scope_id=scope_config.scope_id,
            source_strategy_ids=("plumeusdt_binance_spot_makerv3",),
        )
